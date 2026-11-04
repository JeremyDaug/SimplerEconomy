use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::actors::Actors;
use crate::game::deal::{DealMaker, ProposedDeal, freight_bill};
use crate::game::factuals::Factuals;
use crate::game::good::{Good, TIME};
use crate::game::job::{
    break_even_amv, sought_sale_runs, Job, JobLine, listed_decay, trade_score,
};
use crate::game::marketorder::MarketOrder;
use crate::game::pop::{DemoRow, MigrantGroup, Pop};
use crate::game::process::{InputType, Process};
use rand::Rng;

/// Dead zone around zero for a stored AMV.
///
/// AMV may be negative. A value inside `(-AMV_EPSILON, AMV_EPSILON)` is not
/// stored: it bounces to `AMV_EPSILON` on the other side of zero from the
/// previous sign. This is not a configured price floor.
pub const AMV_EPSILON: f64 = 1e-9;

/// Salability of a good with no recorded value yet.
pub const SALABILITY_DEFAULT: f64 = 0.5;

/// Lowest stored salability. A good may be nearly illiquid. It is not stored as 0.
pub const SALABILITY_MIN: f64 = 0.0001;

/// Highest stored salability. Up to 1 is illiquid to par. Above 1 is at-par and currency.
pub const SALABILITY_MAX: f64 = 2.0;

/// Floor and ceiling of the factor that scales a positive AMV.
///
/// Salability below this floor still counts as the floor. Salability above 1
/// does not raise the factor past 1; that excess is [`monetary_rating`].
pub const AMV_SCALE_MIN: f64 = 0.05;

/// Fraction of the gap between morning holding and the day's print closed in one night.
const AMV_PRINT_STEP: f64 = 0.1;

/// Largest one-night move from the print, the unsold ease, and the process
/// pull, as a share of the payment scale.
const AMV_STEP_CAP: f64 = 0.05;

/// Flat share of the payment scale subtracted when a good was offered and nothing sold.
const AMV_EXCESS_NUDGE: f64 = 0.005;

/// Largest one-night production-flow fraction.
const AMV_FLOW_CAP: f64 = 0.005;

/// AMV added when buy orders were still open.
///
/// The step does not grow with the price or with the tender that paid.
const AMV_UNMET_NUDGE: f64 = 0.01;

/// Full mass-book move, as a share of the price on a surplus and as AMV on a shortage.
///
/// [`mass_idle_fraction`] takes this times the part of the larger side that
/// did not meet the other. A surplus subtracts that fraction of the price.
/// A shortage adds that fraction as AMV, so a dearer good does not take a
/// larger step.
const MASS_IDLE_STEP: f64 = 0.03;

/// A rise nobody can pay is replaced by this many times that rise, downward.
///
/// [`drag_unaffordable`] applies it. The fall is the rise it replaces, and
/// the same rise again.
const UNAFFORDABLE_DRAG: f64 = 2.0;

/// Most times the mass books clear in one day.
///
/// A pass that exchanges a unit runs another, so a tender taken in can
/// fund a later clear. The loop stops earlier when a pass exchanges nothing.
const MASS_CLEAR_PASSES: usize = 8;

/// Share of the gap [`step_ask_walk`] moves in one night.
///
/// A fill steps this share back toward the cost floor. An offer that did
/// not trade steps this share toward the card. A no-cross, and an ask
/// already on the card, are that function's exceptions.
const ASK_WALK: f64 = 0.25;

/// Physical rot is divided by this before [`decay_cut`] applies it.
///
/// A base that all rots would take a quarter of a positive price.
const AMV_ROT_DIVISOR: f64 = 4.0;

/// Largest share of a positive price that one night of rot may remove.
///
/// One night stops at 5 percent.
const AMV_ROT_CAP: f64 = 0.05;

/// One night's salability step when AMV did not fall.
///
/// Payment and a purchase that was not paid out both use this step.
/// [`Market::record_keeping`] stops the purchase at par.
const SALABILITY_UP_STEP: f64 = 0.05;

/// Largest salability lost on a night the AMV fell.
const SALABILITY_LOSS_CAP: f64 = 0.01;

/// Factor applied to a positive AMV. Clamped to [`AMV_SCALE_MIN`]..=1.
pub fn amv_scale(salability: f64) -> f64 {
    salability.clamp(AMV_SCALE_MIN, 1.0)
}

/// Monetary standing above par. Zero while salability is at or below 1.
pub fn monetary_rating(salability: f64) -> f64 {
    (salability - 1.0).max(0.0)
}

/// Tender slots on one market.
pub const CURRENCY_SLOTS: usize = 4;

/// Good id in the first tender slot of a new market.
///
/// This is the world good `gold_token`.
const OPENING_CURRENCY: usize = 5;

/// Why a pop was removed today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopExit {
    /// Household count was under 1 after growth and the population cap.
    UnderOneHousehold,
    /// No running line and no whole coin. `to` received the households.
    NoWorkNoCoin { to: Vec<usize> },
    /// The move would have left fewer than one household, so everyone went.
    /// `to` received the households.
    UnderOneMove { to: Vec<usize> },
    /// The same demographic and the same process set. `to` is the earlier pop.
    SameJob { to: Vec<usize> },
}

/// A pop removed today.
///
/// `id` is the pop. `why` is the rule that removed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopRemoval {
    pub id: usize,
    pub why: PopExit,
}

/// A local market. Member ids point at actors. Goods hold the stored price.
#[derive(Debug, Clone)]
pub struct Market {
    /// Unique id. Matches the region this market represents, when it has one.
    pub id: usize,
    /// Pop ids present here.
    pub pops: HashSet<usize>,
    /// Firm ids present here.
    pub firms: HashSet<usize>,
    /// Institution ids present here. An institution may sit in several markets.
    pub institution_ids: HashSet<usize>,
    /// Per-good price and quantity record. Keyed by good id.
    pub goods: HashMap<usize, MarketGood>,
    /// Distance / size multiplier on deal bulk. 0 on a one-hex market.
    pub friction: f64,
    /// Tender goods. [`CURRENCY_SLOTS`] entries. `None` is an empty slot.
    pub currencies: [Option<usize>; CURRENCY_SLOTS],
    /// One row per good the mass market quoted today, in the order cleared.
    mass_closes: Vec<MassClose>,
    /// Refused-ask fraction per good. [`step_ask_walk`] writes it.
    ask_walk: HashMap<usize, f64>,
    /// Goods a seller priced below zero today. [`Self::reset_day`] clears them.
    negative_value: HashSet<usize>,
    /// Units bid today without a purchase, by good. [`Self::reset_day`] clears them.
    unpaid_wants: HashMap<usize, f64>,
    /// Goods a buyer asked for and no buyer covered one unit of.
    ///
    /// [`Self::set_unaffordable`] writes the set. [`Self::reset_day`] clears it.
    /// [`drag_unaffordable`] reads it.
    unaffordable: HashSet<usize>,
    /// Market days left before another household may leave to found.
    founding_rest: u32,
    /// Pops removed today. [`Self::reset_day`] clears them.
    pop_removals: Vec<PopRemoval>,
    /// Units listed at the open, before a reservation or a sale.
    ///
    /// [`Self::reset_day`] clears it. The night prices do not read it.
    initial_offering: HashMap<usize, f64>,
    /// Initial offering minus units pulled off the book for a reservation.
    ///
    /// [`Self::reset_day`] clears it. The night prices do not read it.
    effective_offering: HashMap<usize, f64>,
    /// Units still listed after the ladder and the durability swap.
    ///
    /// [`Self::reset_day`] clears it. The night prices do not read it.
    excess_supply: HashMap<usize, f64>,
    /// Units buyers would purchase if the purse were not the limit.
    ///
    /// [`Self::reset_day`] clears it. The night prices do not read it.
    maximal_order: HashMap<usize, f64>,
    /// Units actually purchased today.
    ///
    /// [`Self::reset_day`] clears it. The night prices do not read it.
    real_order: HashMap<usize, f64>,
}

impl Market {
    /// # New
    ///
    /// Empty market with this id.
    ///
    /// `id` is the market id. The tender list is [`CURRENCY_SLOTS`] long.
    /// The first slot holds [`OPENING_CURRENCY`]. The other three are empty.
    /// Returns that market.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            pops: HashSet::new(),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            friction: 0.0,
            currencies: [Some(OPENING_CURRENCY), None, None, None],
            mass_closes: Vec::new(),
            ask_walk: HashMap::new(),
            negative_value: HashSet::new(),
            unpaid_wants: HashMap::new(),
            unaffordable: HashSet::new(),
            founding_rest: 0,
            pop_removals: Vec::new(),
            initial_offering: HashMap::new(),
            effective_offering: HashMap::new(),
            excess_supply: HashMap::new(),
            maximal_order: HashMap::new(),
            real_order: HashMap::new(),
        }
    }

    /// # Initial Offering
    ///
    /// Units listed at the open, by good.
    ///
    /// The figure is the morning book, working stock and saved stock together.
    /// [`MarketGood::stock`] is yesterday's survivors and is a different
    /// number. Returns that map.
    pub fn initial_offering(&self) -> &HashMap<usize, f64> {
        &self.initial_offering
    }

    /// # Effective Offering
    ///
    /// Initial offering minus reservations, by good.
    ///
    /// A sale stays in this figure. Returns that map.
    pub fn effective_offering(&self) -> &HashMap<usize, f64> {
        &self.effective_offering
    }

    /// # Excess Supply
    ///
    /// Units still listed after the day's ladder, by good.
    ///
    /// Returns that map.
    pub fn excess_supply(&self) -> &HashMap<usize, f64> {
        &self.excess_supply
    }

    /// # Maximal Order
    ///
    /// Units a buyer would purchase if the purse were not the limit, by good.
    ///
    /// Returns that map.
    pub fn maximal_order(&self) -> &HashMap<usize, f64> {
        &self.maximal_order
    }

    /// # Real Order
    ///
    /// Units purchased today, by good.
    ///
    /// Returns that map.
    pub fn real_order(&self) -> &HashMap<usize, f64> {
        &self.real_order
    }

    /// # Store Book Facts
    ///
    /// Records the day's offering and order totals.
    ///
    /// `initial` is the morning book. `retracted` is units pulled for a
    /// reservation. Effective offering is `initial` minus that pull, and
    /// not below zero. `excess` is what the book still lists. `maximal`
    /// and `real` are the buy orders. Nothing is returned.
    pub(crate) fn store_book_facts(
        &mut self,
        initial: HashMap<usize, f64>,
        retracted: &HashMap<usize, f64>,
        excess: HashMap<usize, f64>,
        maximal: HashMap<usize, f64>,
        real: HashMap<usize, f64>,
    ) {
        let mut effective = HashMap::new();
        let mut goods: Vec<usize> = initial.keys().chain(retracted.keys()).copied().collect();
        goods.sort_unstable();
        goods.dedup();
        for good in goods {
            let listed = initial.get(&good).copied().unwrap_or(0.0);
            let pull = retracted.get(&good).copied().unwrap_or(0.0);
            effective.insert(good, (listed - pull).max(0.0));
        }
        self.initial_offering = initial;
        self.effective_offering = effective;
        self.excess_supply = excess;
        self.maximal_order = maximal;
        self.real_order = real;
    }

    /// # Pop Removals
    ///
    /// Pops removed today, in the order they were recorded.
    ///
    /// Returns that list. [`Self::reset_day`] clears it at the next day.
    pub fn pop_removals(&self) -> &[PopRemoval] {
        &self.pop_removals
    }

    /// # Mass Closes
    ///
    /// The mass-market rows recorded today.
    ///
    /// Each row is one good that drew a quote. `offered` and `requested`
    /// are the whole units on that book. `exchanged` is what changed hands.
    /// `price` is the clearing AMV when at least one unit traded. Returns
    /// the rows in clear order.
    pub fn mass_closes(&self) -> &[MassClose] {
        &self.mass_closes
    }

    /// # Currencies
    ///
    /// The tender slots on this market.
    ///
    /// Each entry is a good id, or `None` when that slot is empty. Returns
    /// the four slots.
    pub fn currencies(&self) -> [Option<usize>; CURRENCY_SLOTS] {
        self.currencies
    }

    /// Sets the market friction factor. Must be `>= 0.0`.
    pub fn with_friction(mut self, friction: f64) -> Self {
        debug_assert!(friction >= 0.0, "friction must be >= 0.0");
        self.friction = friction;
        self
    }

    /// # Record Keeping
    ///
    /// Writes tomorrow's AMV and salability from each good's day record, then
    /// clears the exchange and flow counters. Each good keeps the AMV its own
    /// day wrote.
    /// [`MarketGood::decayed`] and `volume`
    /// stay until the next morning's reset. Stock is left as it stands.
    /// The print move is [`print_push`], applied in holding space.
    /// Production flow is a share of [`Self::payment_scale`]. Decay is
    /// [`decay_cut`]: a positive price falls, and it stops at
    /// [`AMV_EPSILON`]. When `sought_unmet` is positive, that night's rot
    /// is added back and [`AMV_UNMET_NUDGE`] is added on top. The nudge is
    /// an amount of AMV. It does not grow with the price or the tender.
    /// A mass book that [`mass_book_can_move`] allows then moves by
    /// [`mass_idle_fraction`]. A surplus takes that fraction off the
    /// absolute AMV. A shortage adds that fraction as AMV. A currency and
    /// the good it buys cannot multiply one another.
    /// [`drag_unaffordable`] replaces that fraction when the book was asked
    /// and nobody could buy one unit.
    /// A currency's [`MarketGood::paid`] is added to both sides of that
    /// fraction, so tender spent on other goods shrinks the move.
    /// [`MarketGood::production`] covers part of a short bid. Output made
    /// after the book, including a good that was eaten, saved, or never
    /// offered, does not.
    /// [`use_pull`] then moves that price by the day's trade
    /// and process shares. The process share stops at [`AMV_STEP_CAP`] of
    /// the payment scale. A short book buyers could pay does not finish
    /// below the morning ask. [`keep_scarce_ask`] is that floor.
    /// A currency [`currency_book_can_move`] rejects keeps the AMV it opened
    /// at. Rot, flow, and [`use_pull`] do not write it. A real bid larger
    /// than the offer still adds the positive [`mass_idle_fraction`].
    /// The drag still writes when it replaced that fraction.
    /// The one-unit unpaid want is not that bid.
    /// Desire consumption and rot are in the share
    /// and do not add a second step. [`valued_price`] then keeps the result at
    /// [`AMV_EPSILON`] unless a seller priced that good below zero today.
    /// None of that is stored unless the good exchanged at least one unit.
    /// [`Self::traded_today`] is that test. A good that did not trade keeps the
    /// AMV it opened at, unless a real bid was left and the night's step is a
    /// fall. The unpaid one-unit want is not that bid. A surplus against only
    /// that want keeps the price it opened at.
    /// An empty offer with a real bid rises by the idle step, and a surplus
    /// fall stops, at [`make_ceiling`]. A price already on that ask does not
    /// cross it. No process that gains from a higher ask leaves the old rule.
    /// [`Self::keep_positive_asks`] then lifts a price that
    /// landed in the dead zone.
    /// `factuals` is that tradeable check. Rot itself was added
    /// through [`Self::note_decay`] before this runs.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let history = self.history();
        let scale = self.payment_scale();
        let currencies = self.currencies();
        // The morning grant. Goods made after the book do not cover a bid.
        let mut covered = HashMap::new();
        for (&id, good) in &self.goods {
            if good.production > 0.0 && good.production.is_finite() {
                covered.insert(id, good.production);
            }
        }
        let velocity = currency_velocity(&self.goods, &currencies);
        let idle = mass_idle_by_good(
            &self.mass_closes,
            &currencies,
            factuals,
            &history,
            &covered,
            &velocity,
        );
        let mut ids: Vec<usize> = self.goods.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            // A currency surplus with no other tender offered stays.
            let unpaid = self.unpaid_wants.get(&id).copied().unwrap_or(0.0);
            let price_live = currency_book_can_move(&self.mass_closes, &currencies, id, unpaid);
            let quoted = self.mass_closes.iter().find(|close| close.good == id);
            let offered = quoted.map(|close| close.offered).unwrap_or(0.0);
            let requested = quoted.map(|close| close.requested).unwrap_or(0.0);
            let exchanged = quoted.map(|close| close.exchanged).unwrap_or(0.0);
            let real_bid = (requested - unpaid).max(0.0);
            // A held book is 0 here. A tender that cannot pay is left alone.
            let raw = idle.get(&id).copied().unwrap_or(0.0);
            let step = if mass_book_can_move(factuals, &currencies, &history, id) {
                drag_unaffordable(raw, offered, requested, self.unaffordable.contains(&id))
            } else {
                raw
            };
            let salability = self
                .goods
                .get(&id)
                .map(|good| good.salability)
                .unwrap_or(SALABILITY_DEFAULT);
            let ceiling = make_ceiling(factuals, &history, id, salability);
            let good = self
                .goods
                .get_mut(&id)
                .expect("id was just copied from goods");
            let old = good.amv;
            // No trade, no new AMV. The book can still clear its tape below.
            let moved = Self::traded_today(good, exchanged);
            if moved && price_live {
                // Print gap, and the ease when nothing sold.
                let push = print_push(id, good, &history, scale);
                // Production flow. Consumption against production.
                let flow_denom = good.stock + good.production + good.consumption + 1.0;
                let flow = ((good.consumption - good.production) / flow_denom)
                    .clamp(-AMV_FLOW_CAP, AMV_FLOW_CAP);
                // Holding move, then flow, as a share of the payment scale.
                let mut next = amv_from_holding_move(old, good.salability, push);
                next += scale * flow;
                // Rot, taken off the price those terms just set.
                let decayed = good.decayed;
                let decay_base = if good.volume > 0.0 {
                    good.volume
                } else {
                    good.stock
                };
                let mut rot_cut = 0.0;
                if decay_base > 0.0 && decayed > 0.0 {
                    let fraction = (decayed / decay_base / AMV_ROT_DIVISOR).clamp(0.0, AMV_ROT_CAP);
                    rot_cut = decay_cut(next, fraction);
                    next -= rot_cut;
                }
                // Open buys. That rot goes back on, then the flat unmet step.
                if good.sought_unmet > 0.0 {
                    next += rot_cut + AMV_UNMET_NUDGE;
                }
                // Mass book. A surplus takes a share of the price. A shortage adds AMV.
                // The move stops one idle step above the cheapest process that pays.
                if step != 0.0 {
                    let before = next;
                    next = hold_the_make(before, before + idle_move(old, step), ceiling);
                }
                // Use shares. Trade pulls. Process pulls, and stops at the print cap.
                let traded = good.clear_units;
                let trade_price = if traded > 0.0 {
                    good.clear_value / traded
                } else {
                    0.0
                };
                let process = good.process_used;
                let output_price = if process > 0.0 {
                    good.process_value / process
                } else {
                    0.0
                };
                next = use_pull(
                    next,
                    decayed,
                    traded,
                    trade_price,
                    process,
                    output_price,
                    good.desire_used,
                    scale * AMV_STEP_CAP,
                );
                // A scarce affordable book stays at or above the morning ask.
                let scarce = real_bid > offered && !self.unaffordable.contains(&id);
                next = keep_scarce_ask(old, next, scarce);
                // A positive ask keeps the stored price positive.
                next = valued_price(self.negative_value.contains(&id), next);
                // Store the new AMV.
                good.set_amv(next);
            } else if moved && step < raw {
                // The drag still falls a currency the full write skipped.
                let next = valued_price(
                    self.negative_value.contains(&id),
                    hold_the_make(old, old + idle_move(old, step), ceiling),
                );
                good.set_amv(next);
            } else if !moved
                && price_live
                && step < 0.0
                && real_bid >= 1.0
                && self.unaffordable.contains(&id)
                && mass_book_can_move(factuals, &currencies, &history, id)
            {
                // A real bid was left unpaid. The one-unit want is not that bid.
                let next = valued_price(
                    self.negative_value.contains(&id),
                    hold_the_make(old, old + idle_move(old, step), ceiling),
                );
                good.set_amv(next);
            } else if moved && real_bid > offered {
                // A real short bid still lifts a currency the full write skipped.
                if step > 0.0 {
                    let next = valued_price(
                        self.negative_value.contains(&id),
                        hold_the_make(old, old + idle_move(old, step), ceiling),
                    );
                    good.set_amv(next);
                }
            } else if !moved
                && price_live
                && step > 0.0
                && offered < 1.0
                && real_bid >= 1.0
                && !self.unaffordable.contains(&id)
                && ceiling.is_some_and(|ask| old < ask)
                && mass_book_can_move(factuals, &currencies, &history, id)
            {
                // Nobody offered, and a buyer could pay. Rise toward the make.
                let next = valued_price(
                    self.negative_value.contains(&id),
                    hold_the_make(old, old + idle_move(old, step), ceiling),
                );
                good.set_amv(next);
            }
            // Salability. A fall lowers it. Payment without a fall raises it,
            // including past par. A good that was bought and not paid out
            // rises toward par and stops there.
            let paid_units = good.paid;
            let informed = good.traded + good.paid + good.offered_unsold > 0.0;
            let fell = good.amv < old;
            if fell && old.abs() >= AMV_EPSILON {
                let drop = ((old - good.amv) / old.abs()).min(SALABILITY_LOSS_CAP);
                good.set_salability(good.salability - drop);
            } else if informed && paid_units > 0.0 {
                good.set_salability(good.salability + SALABILITY_UP_STEP);
            } else if good.traded > 0.0 && good.salability < 1.0 {
                let next = (good.salability + SALABILITY_UP_STEP).min(1.0);
                good.set_salability(next);
            } else if good.amv > old && old.abs() >= AMV_EPSILON && good.salability < 1.0 {
                // A rise that was not paid restores liquidity toward par.
                let rise = ((good.amv - old) / old.abs()).min(SALABILITY_LOSS_CAP);
                good.set_salability((good.salability + rise).min(1.0));
            }
            // Clear today's exchange and flow.
            good.clear_exchange();
        }
        // A night write can land inside the dead zone.
        self.keep_positive_asks();
        // Refused-ask walk, from today's mass rows.
        step_ask_walk(&mut self.ask_walk, &self.mass_closes);
    }

    /// # Gather Use
    ///
    /// Copies today's process and desire use onto each good.
    ///
    /// `actors` holds the members. Process inputs and desire takes, including
    /// rest, weigh [`use_pull`]. Firms are skipped. Nothing is returned.
    fn gather_use(&mut self, actors: &Actors) {
        for actor in self.members() {
            let Actor::Pop(id) = actor else {
                continue;
            };
            let pop = actors.pop(id);
            for (&input, credit) in pop.job.process_credit() {
                self.note_process(input, credit);
            }
            for (&good, &units) in pop.desire_eaten() {
                self.note_desire(good, units);
            }
            for (&good, &units) in pop.rest_taken() {
                self.note_desire(good, units);
            }
            for (&good, &units) in pop.desire_held() {
                self.note_desire(good, units);
            }
        }
    }

    /// # Note Process
    ///
    /// Adds one input's process credit to that good's night record.
    ///
    /// `credit` is the units used up and the output units those runs made.
    /// Output value is each output's current AMV times its units. An output
    /// counts when it is on the market and at least one unit of it cleared
    /// today. An output that did not clear is not a price for its inputs.
    /// When at least one output is priced and the input itself is on the
    /// market, the units and that value are stored for [`use_pull`].
    /// Nothing is returned.
    fn note_process(&mut self, input: usize, credit: &crate::game::job::ProcessCredit) {
        if credit.units <= 0.0 {
            return;
        }
        let mut value = 0.0;
        let mut priced = false;
        for (&output, &qty) in &credit.outputs {
            if qty <= 0.0 {
                continue;
            }
            let Some(good) = self.goods.get(&output) else {
                continue;
            };
            // A card nobody bought is not what the input was worth.
            if good.clear_units < 1.0 {
                continue;
            }
            value += qty * good.amv;
            priced = true;
        }
        if !priced {
            return;
        }
        let Some(row) = self.goods.get_mut(&input) else {
            return;
        };
        row.process_used += credit.units;
        row.process_value += value;
    }

    /// # Note Desire
    ///
    /// Adds `units` of `good` to today's desire share.
    ///
    /// A good that is not on the market is skipped. Nothing is returned.
    fn note_desire(&mut self, good: usize, units: f64) {
        if units <= 0.0 || !units.is_finite() {
            return;
        }
        let Some(row) = self.goods.get_mut(&good) else {
            return;
        };
        row.desire_used += units;
    }

    /// # Note Supply
    ///
    /// Adds `amount` to `good`'s production.
    ///
    /// `amount` is units made today. Zero does nothing. The good is inserted
    /// on the default card when it is missing.
    pub fn note_supply(&mut self, good: usize, amount: f64) {
        debug_assert!(amount.is_finite(), "supply must be finite");
        debug_assert!(amount >= 0.0, "supply must be >= 0");
        if amount == 0.0 {
            return;
        }
        let row = self.goods.entry(good).or_insert_with(MarketGood::new);
        row.production += amount;
    }

    /// Add rot observed today for `good`. `decayed` is units lost. `volume`
    /// is the stock those units came from. [`Self::record_keeping`] turns
    /// `decayed / volume` (or stock, when volume is 0) into an AMV reduction.
    pub fn note_decay(&mut self, good: usize, decayed: f64, volume: f64) {
        let row = self.goods.entry(good).or_insert_with(MarketGood::new);
        row.decayed += decayed.max(0.0);
        row.volume += volume.max(0.0);
    }

    /// Aggregate emigration and hiring pressure for this region. Not written yet.
    pub fn sum_migratory_pressure(&mut self, actors: &Actors, factuals: &Factuals) {
        let _ = (self, actors, factuals);
        todo!("Market sum migratory pressure (positive / negative / net, migrant pool)")
    }

    /// # Payment Scale
    ///
    /// Per-unit holding value of the goods taken in payment today.
    ///
    /// A good counts when `paid` is positive and [`MarketHistory::holding_per_unit`]
    /// is positive. The result is the paid-unit weighted average of those
    /// values. A night with no such payment returns 1.
    fn payment_scale(&self) -> f64 {
        let history = self.history();
        let mut value = 0.0;
        let mut units = 0.0;
        for (&id, good) in &self.goods {
            if good.paid <= 0.0 {
                continue;
            }
            let holding = history.holding_per_unit(id);
            if holding <= 0.0 {
                continue;
            }
            value += holding * good.paid;
            units += good.paid;
        }
        if units > 0.0 { value / units } else { 1.0 }
    }

    /// # Traded Today
    ///
    /// Whether `good` exchanged at least one unit.
    ///
    /// `exchanged` is the mass book's count. The good's clear and its traded
    /// count are that same exchange when the book did not store a row. A
    /// count below one is not a trade. Returns that answer.
    fn traded_today(good: &MarketGood, exchanged: f64) -> bool {
        let mass = if exchanged.is_finite() {
            exchanged.max(0.0)
        } else {
            0.0
        };
        mass >= 1.0 || good.clear_units >= 1.0 || good.traded >= 1.0
    }

    /// # Keep Positive Asks
    ///
    /// Lifts each stored AMV that no seller valued below zero.
    ///
    /// The night write can leave a price in the dead zone.
    /// [`valued_price`] decides the floor. A good a seller priced below
    /// zero is left as stored.
    fn keep_positive_asks(&mut self) {
        let mut ids: Vec<usize> = self.goods.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let below = self.negative_value.contains(&id);
            let good = self
                .goods
                .get_mut(&id)
                .expect("id was just copied from goods");
            good.set_amv(valued_price(below, good.amv));
        }
    }

    /// # Note Sells Below Zero
    ///
    /// Records `good` when a sell's minimum is below zero.
    ///
    /// `orders` are that good's mass quotes. A negative
    /// [`MarketOrder::accept_price`] on a sell is that minimum. A buy
    /// limit is not.
    fn note_sells_below_zero(&mut self, good: usize, orders: &[MarketOrder]) {
        let below = orders.iter().any(|order| {
            order.target == good && order.target_amount < 0.0 && order.accept_price < 0.0
        });
        if below {
            self.negative_value.insert(good);
        }
    }

    /// # Note Negative Value
    ///
    /// Records that a seller valued `good` below zero today.
    fn note_negative_value(&mut self, good: usize) {
        self.negative_value.insert(good);
    }

    /// # History
    ///
    /// Snapshot of stored AMV, salability, units still sought, production,
    /// the mass book's offer and exchange, the refused-ask walk, and the
    /// tender slots. Filled slots are copied in order. A good in two slots
    /// is listed once.
    ///
    /// Sought starts from [`MarketGood::sought_unmet`]. An unfilled mass
    /// request replaces that count when it is larger. Production is copied
    /// from [`MarketGood::production`]. Each mass close copies its offered
    /// and exchanged units. A snapshot taken before exchange has no sought
    /// units and an empty book. The night plan reads it.
    pub fn history(&self) -> MarketHistory {
        let mut history = MarketHistory::new();
        for (&good_id, good) in &self.goods {
            history.prices.insert(good_id, good.amv);
            history.salability.insert(good_id, good.salability);
            if good.sought_unmet > 0.0 {
                history.sought.insert(good_id, good.sought_unmet);
            }
            if good.production > 0.0 {
                history.produced.insert(good_id, good.production);
            }
        }
        // The night plan reads this book's offer and exchange.
        for close in &self.mass_closes {
            if close.offered > 0.0 {
                let entry = history.mass_offered.entry(close.good).or_insert(0.0);
                if close.offered > *entry {
                    *entry = close.offered;
                }
            }
            if close.exchanged > 0.0 {
                let entry = history.mass_exchanged.entry(close.good).or_insert(0.0);
                if close.exchanged > *entry {
                    *entry = close.exchanged;
                }
            }
            // An unfilled mass request replaces the stored sought count when it is larger.
            let left = (close.requested - close.exchanged).max(0.0);
            if left <= 0.0 {
                continue;
            }
            let entry = history.sought.entry(close.good).or_insert(0.0);
            if left > *entry {
                *entry = left;
            }
        }
        history.friction = self.friction;
        history.ask_walk = self.ask_walk.clone();
        history.unaffordable.clone_from(&self.unaffordable);
        for id in self.currencies().into_iter().flatten() {
            if !history.currencies.contains(&id) {
                history.currencies.push(id);
            }
        }
        history
    }

    /// # Market Day
    ///
    /// One day for the actors registered on this market.
    ///
    /// Clears yesterday's exchange tape and rot, and each member's
    /// [`crate::game::deal::DealMaker::reset_day`]. Each pop then writes its
    /// purchase targets. [`Self::buy_ladder`] runs the sell book and the buy
    /// ladder, reserving each desire as the walk reaches it, held inputs
    /// return to property, then
    /// produce, consume, [`crate::game::deal::DealMaker::rest`],
    /// [`crate::game::pop::Pop::update_sentiments`], pop growth,
    /// [`Self::rebalance_population_cap`],
    /// [`Self::prune_pops_below_one_household`],
    /// [`Self::internal_migration`], [`Self::found_pop`],
    /// [`Self::fold_duplicate_pops`], decay,
    /// [`crate::game::pop::Pop::rescale_desires`], actor record keeping,
    /// [`Self::gather_use`], and [`Self::record_keeping`].
    ///
    /// Sentiment runs before growth, so today's mood scales this year's births.
    /// Rescale is after decay, so this day's desire effects stay at the
    /// previous size. It also runs after migration and founding, so
    /// satisfaction and targets match the size still on the market.
    ///
    /// Institutions and states use the empty defaults, so their unimplemented
    /// decay and record-keeping methods stay uncalled.
    ///
    /// Nothing is returned. Today's rot stays on [`MarketGood::decayed`].
    /// Inter-market work stays outside this call.
    pub fn market_day(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        rng: &mut impl Rng,
    ) {
        // Cleanup Phase. Drop yesterday's tape and rot. Production stays.
        // The plan still reads yesterday's offer and exchange.
        let prior = self.history();
        self.reset_day(actors);
        let mut history = self.history();
        history.mass_offered = prior.mass_offered;
        history.mass_exchanged = prior.mass_exchanged;
        // Yesterday's unfilled bids stay for the plan and the sell book.
        history.sought.clone_from(&prior.sought);
        // Yesterday's unaffordable goods stay for the plan.
        history.unaffordable.clone_from(&prior.unaffordable);
        let members = self.members();
        // A meal already set aside stays out of the working pile.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                let pop = actors.pop_mut(id);
                let locked = pop.meal_to_keep();
                let (gaps, basic_count) = pop.need_gaps();
                let property = pop.property.clone();
                let before = pop.job.lines.len();
                let households = pop.demographics.household.count;
                let saved = pop.meal.clone();
                let _hold = pop.job.plan_purchases_for(
                    &property,
                    &locked,
                    &saved,
                    factuals,
                    &history,
                    &gaps,
                    basic_count,
                    households,
                );
                // A dropped line changes the craft the migration gate reads.
                if pop.job.lines.len() != before {
                    let processes: Vec<usize> =
                        pop.job.lines.iter().map(|line| line.process).collect();
                    pop.job.craft = craft_id_for(factuals, &processes);
                }
            }
        }
        // One sell book, then each pop buys in household-liquidity order.
        self.buy_ladder(actors, factuals, rng, &history.sought);
        // Held inputs are back in property for the run.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).release_input_hold();
            }
        }
        // Production Phase
        for actor in &members {
            actors.get_mut(*actor).produce(factuals);
        }
        // The stock just made is the luxury day, so a further level leaves it.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).hold_luxury_stock();
            }
        }
        // Consumption Phase
        for actor in &members {
            actors.get_mut(*actor).consume();
        }
        // Leftover goods the species still accepts. Living includes that sink.
        for actor in &members {
            actors.get_mut(*actor).rest(factuals);
        }
        // Sentiment. Consume and rest have written today's living. Growth reads it.
        let sentiment_history = self.history();
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).update_sentiments(&sentiment_history);
            }
        }
        // Growth. The new size is applied to desires after decay.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).growth_phase(factuals);
            }
        }
        // Fit capped pops to their combined ceiling.
        self.rebalance_population_cap(actors);
        // Drop pops the year left under one household.
        self.prune_pops_below_one_household(actors);
        // Move households inside this market. Decay then rots the stock that remains.
        self.internal_migration(actors, factuals);
        // The stake output was held out of the meal. Founding reads it in quantity.
        for actor in &self.members() {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).release_stake_hold();
            }
        }
        // One new pop, one process added or dropped, from the most days of set-aside.
        self.found_pop(actors, factuals, rng);
        // A pop whose job matches an earlier pop joins that pop.
        self.fold_duplicate_pops(actors);
        // Decay Phase. Membership is whoever migration and founding left.
        let members = self.members();
        for actor in &members {
            for (good, (lost, volume)) in actors.get_mut(*actor).decay_goods(factuals) {
                self.note_decay(good, lost, volume);
            }
        }
        // Match desires to the new size now that this day's effects are spent.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).rescale_desires(factuals);
            }
        }
        // Record keeping. The morning plan already wrote today's targets.
        let history = self.history();
        for actor in &members {
            actors.get_mut(*actor).record_keeping(factuals, &history);
        }
        // Day's process, desire, and freight, so the night can weight them.
        self.gather_use(actors);
        // The morning grant can cover a short bid. Later output does not.
        self.record_keeping(factuals);
    }

    /// # Reset Day
    ///
    /// Clears this market's day tape and each member's yesterday.
    ///
    /// `actors` supplies the members. AMV, salability, stock, and production
    /// stay. Today's mass-market rows are dropped, and so is any sell
    /// priced below zero. The refused-ask walk stays. Notes of pops removed
    /// on the previous day are cleared. Production stays so
    /// [`crate::game::actors::Actors::start_day`],
    /// which runs before this, still counts at the night write. Pop
    /// satisfaction and reserves go to zero. Firms run
    /// [`crate::game::firm::Firm::reset_day`]. Institutions keep the empty
    /// [`crate::game::deal::DealMaker::reset_day`] default.
    fn reset_day(&mut self, actors: &mut Actors) {
        for good in self.goods.values_mut() {
            good.clear_day();
        }
        self.mass_closes.clear();
        self.negative_value.clear();
        self.unpaid_wants.clear();
        self.unaffordable.clear();
        self.pop_removals.clear();
        self.initial_offering.clear();
        self.effective_offering.clear();
        self.excess_supply.clear();
        self.maximal_order.clear();
        self.real_order.clear();
        for actor in self.members() {
            actors.get_mut(actor).reset_day();
        }
    }

    /// # Mass Market
    ///
    /// Currency pass for this day.
    ///
    /// `actors` holds the members. `factuals` names goods. `rng` draws the
    /// fill when one side is short. Goods on the board are visited in id
    /// order, and every filled tender slot is visited with them. Payment
    /// for each good is [`payment_tenders`]. Each member lays out its buys
    /// through [`crate::game::deal::DealMaker::prepare_mass_buys`] before any
    /// good is visited, then may sit out through
    /// [`crate::game::deal::DealMaker::mass_order`]. A returned order sets
    /// the units and the price limit. The working price starts at yesterday's
    /// AMV, which may be negative. Stock moves only for actors who accept
    /// the clearing price. Each quoted good is stored on [`Self::mass_closes`].
    /// A good that exchanges a unit lays the buys out again, so a tender just
    /// taken can fund a later good. The books then run again while a pass
    /// still exchanges a unit, and stop at [`MASS_CLEAR_PASSES`].
    /// Filled tenders stay in that id order. [`Self::trade_durable_tenders`]
    /// then clears the most durable one once more, paid by a strictly less
    /// durable tender. Nothing is returned.
    fn mass_market(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        rng: &mut impl rand::RngCore,
    ) {
        let history = self.history();
        let currencies = self.currencies();
        let filled: Vec<usize> = currencies.into_iter().flatten().collect();
        if filled.is_empty() {
            return;
        }
        let members = self.members();
        let mut goods: Vec<usize> = self.goods.keys().copied().collect();
        for tender in &filled {
            if !goods.contains(tender) {
                goods.push(*tender);
            }
        }
        goods.sort_unstable();
        // One sale room per tender, shared by every pass.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).note_currency_sale_room(&currencies);
            }
        }
        for _pass in 0..MASS_CLEAR_PASSES {
            for actor in &members {
                actors
                    .get_mut(*actor)
                    .prepare_mass_buys(&history, &currencies, factuals);
            }
            let mut exchanged = 0.0;
            for &good in &goods {
                // No other tender means this good would be paid with itself.
                let Some(tender) = payment_tender(&currencies, good) else {
                    continue;
                };
                let mut orders = Vec::new();
                for actor in &members {
                    let Some(order) =
                        actors
                            .get(*actor)
                            .mass_order(good, &history, &currencies, factuals)
                    else {
                        continue;
                    };
                    // A posted tender sell spends today's room.
                    if order.target_amount < 0.0 {
                        if let Actor::Pop(id) = order.origin {
                            let units = order.target_amount.abs().floor();
                            actors.pop_mut(id).spend_currency_sale_room(good, units);
                        }
                    }
                    orders.push(order);
                }
                if orders.is_empty() {
                    continue;
                }
                let moved =
                    self.clear_mass_good(actors, factuals, &history, good, tender, &orders, rng);
                exchanged += moved;
                // The tender just taken can fund a good that has not cleared yet.
                if moved >= 1.0 {
                    for actor in &members {
                        actors
                            .get_mut(*actor)
                            .prepare_mass_buys(&history, &currencies, factuals);
                    }
                }
            }
            if exchanged < 1.0 {
                break;
            }
        }
        self.trade_durable_tenders(actors, factuals, &history, rng);
    }

    /// # Trade Durable Tenders
    ///
    /// One more clear of the most durable filled tender.
    ///
    /// `actors` holds the members. `factuals` names decay. `history` is the
    /// morning card. `rng` draws a long side. Currencies already cleared
    /// with the other goods. This pass lets spare liquid of a strictly less
    /// durable tender bid for the one that lasts longer.
    ///
    /// Filled slots are ranked by [`listed_decay`] ascending, then lower id.
    /// A missing good decays at 0, so two unregistered tenders tie and this
    /// pass posts nothing. Equal decay does not convert. The payers are the
    /// other filled slots whose decay is strictly higher, in slot order.
    /// A non-positive card on the durable tender posts nothing.
    ///
    /// Sale room is noted again from the surplus still on hand, so the same
    /// tenth can be listed once more. Sellers come from
    /// [`DealMaker::mass_card_sell`]. A buyer who posted that sell is left
    /// out. Everyone else may bid through [`DealMaker::mass_conversion_buy`],
    /// spending only the less durable tenders. The book clears once.
    /// Nothing is returned.
    fn trade_durable_tenders(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        history: &MarketHistory,
        rng: &mut impl rand::RngCore,
    ) {
        let currencies = self.currencies();
        let mut filled = Vec::new();
        for id in currencies.into_iter().flatten() {
            if !filled.contains(&id) {
                filled.push(id);
            }
        }
        if filled.len() < 2 {
            return;
        }
        let mut ranked = filled.clone();
        ranked.sort_by(|&left, &right| {
            listed_decay(factuals, left)
                .partial_cmp(&listed_decay(factuals, right))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.cmp(&right))
        });
        let durable = ranked[0];
        let decay = listed_decay(factuals, durable);
        let payers: Vec<usize> = filled
            .into_iter()
            .filter(|&id| id != durable && listed_decay(factuals, id) > decay)
            .collect();
        if payers.is_empty() {
            return;
        }
        let price = history.price(durable);
        if !price.is_finite() || price <= 0.0 {
            return;
        }
        let members = self.members();
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).note_currency_sale_room(&currencies);
            }
        }
        let mut orders = Vec::new();
        let mut sellers = Vec::new();
        for actor in &members {
            let Some(order) =
                actors
                    .get(*actor)
                    .mass_card_sell(durable, history, &currencies, factuals)
            else {
                continue;
            };
            if order.target_amount < 0.0 {
                if let Actor::Pop(id) = order.origin {
                    let units = order.target_amount.abs().floor();
                    actors.pop_mut(id).spend_currency_sale_room(durable, units);
                }
                sellers.push(order.origin);
                orders.push(order);
            }
        }
        for actor in &members {
            if sellers.contains(actor) {
                continue;
            }
            let Some(order) = actors
                .get(*actor)
                .mass_conversion_buy(durable, history, &payers, factuals)
            else {
                continue;
            };
            orders.push(order);
        }
        if orders.is_empty() {
            return;
        }
        let Some(tender) = payers.first().copied() else {
            return;
        };
        self.clear_mass_good_paid(
            actors,
            factuals,
            history,
            durable,
            tender,
            &orders,
            Some(&payers),
            rng,
        );
    }

    /// # Mass Payment Tenders
    ///
    /// Tenders that can pay for `good` on this market.
    ///
    /// The list is [`payment_tenders`]. When that list is empty and
    /// `fallback` is not `good`, `fallback` is the payment. A good with no
    /// tender returns an empty list.
    fn mass_payment_tenders(&self, good: usize, fallback: usize) -> Vec<usize> {
        let mut tenders = payment_tenders(&self.currencies(), good);
        if tenders.is_empty() && fallback != good {
            tenders.push(fallback);
        }
        tenders
    }

    /// # Clear Mass Good
    ///
    /// Clears one good's mass-market book and moves the stock.
    ///
    /// `orders` are the quotes for `good`. `history` is yesterday's card.
    /// `tender` is the fallback in [`Self::mass_payment_tenders`]. `rng`
    /// shuffles a side that is long. The working price starts at the good's
    /// AMV. A negative price means the seller pays to have the good taken.
    /// Actors who accept that price trade whole units. The payment is the
    /// mix from [`mix_change`]. The payer who cannot cover it takes fewer
    /// units. One row is stored on [`Self::mass_closes`].
    /// A clear that moves at least one unit also writes that price and the
    /// totals onto the tape, in [`Self::note_mass_exchange`]. A side short
    /// of its freight half buys that haul from the other side. A non-positive
    /// transport card still has a token price, so the haul is not refused
    /// for a crashed time AMV. A fill neither side can haul shrinks to the
    /// largest quantity that does, and is skipped when one unit still does
    /// not fit. A book whose limits do not cross, and a book whose fills
    /// all die on the haul, records that miss. Returns the units that
    /// changed hands, or `0` when the book did not trade.
    fn clear_mass_good(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        history: &MarketHistory,
        good: usize,
        tender: usize,
        orders: &[MarketOrder],
        rng: &mut impl rand::RngCore,
    ) -> f64 {
        self.clear_mass_good_paid(actors, factuals, history, good, tender, orders, None, rng)
    }

    /// # Clear Mass Good Paid
    ///
    /// Clears one good the way [`Self::clear_mass_good`] does.
    ///
    /// `pay_with` replaces [`Self::mass_payment_tenders`] when this clear
    /// spends only those tenders. `None` uses that list, with `tender` as
    /// its fallback. The other arguments match [`Self::clear_mass_good`].
    /// Returns the units that changed hands.
    fn clear_mass_good_paid(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        history: &MarketHistory,
        good: usize,
        tender: usize,
        orders: &[MarketOrder],
        pay_with: Option<&[usize]>,
        rng: &mut impl rand::RngCore,
    ) -> f64 {
        let tenders = match pay_with {
            Some(list) => list.to_vec(),
            None => self.mass_payment_tenders(good, tender),
        };
        if tenders.is_empty() {
            return 0.0;
        }
        self.note_sells_below_zero(good, orders);
        let amv = history.price(good);
        let (buys, sells) = mass_quotes(orders, good, amv);
        let offered: f64 = sells.iter().map(|quote| quote.units).sum();
        let requested: f64 = buys.iter().map(|quote| quote.units).sum();
        if offered < 1.0 && requested < 1.0 {
            return 0.0;
        }
        let price = clearing_price(&buy_limits(&buys), &sell_limits(&sells), amv)
            .filter(|price| price.is_finite() && *price != 0.0);
        let Some(price) = price else {
            let miss = both_sides_miss(offered, requested, MassMiss::NoCross);
            self.remember_mass_close(good, offered, requested, 0.0, None, miss);
            return 0.0;
        };
        // A tender with no positive price cannot pay. Another one still can.
        let payable: Vec<usize> = tenders
            .into_iter()
            .filter(|&id| {
                let currency_amv = history.price(id);
                currency_amv.is_finite() && currency_amv > 0.0
            })
            .collect();
        if payable.is_empty() {
            self.remember_mass_close(good, offered, requested, 0.0, None, None);
            return 0.0;
        }
        let open_buys: Vec<(Actor, f64)> = buys
            .iter()
            .filter(|quote| quote.limit >= price)
            .map(|quote| (quote.origin, quote.units))
            .collect();
        let open_sells: Vec<(Actor, f64)> = sells
            .iter()
            .filter(|quote| quote.limit <= price)
            .map(|quote| (quote.origin, quote.units))
            .collect();
        let volume = open_buys
            .iter()
            .map(|(_, units)| *units)
            .sum::<f64>()
            .min(open_sells.iter().map(|(_, units)| *units).sum());
        if volume < 1.0 {
            let miss = both_sides_miss(offered, requested, MassMiss::NoCross);
            self.remember_mass_close(good, offered, requested, 0.0, None, miss);
            return 0.0;
        }
        // The long side is shuffled. Whole units stop where the short side stops.
        let mut buy_fill = ration(&open_buys, volume, rng);
        let mut sell_fill = ration(&open_sells, volume, rng);
        let mut buy_at = 0;
        let mut sell_at = 0;
        let mut steps = 0;
        let mut exchanged = 0.0;
        let mut paid: HashMap<usize, f64> = HashMap::new();
        let mut haul_blocked = false;
        while buy_at < buy_fill.len() && sell_at < sell_fill.len() {
            steps += 1;
            if steps > 1_000_000 {
                break;
            }
            let buyer = buy_fill[buy_at].0;
            let seller = sell_fill[sell_at].0;
            let seller_free = actors.get(seller).free_units(good).floor();
            // A negative price is a disposal fee. The seller pays it.
            let payer = if price > 0.0 { buyer } else { seller };
            let units = buy_fill[buy_at]
                .1
                .min(sell_fill[sell_at].1)
                .min(seller_free);
            let transport_amv = haul_good(factuals)
                .map(|(id, _)| history.price(id))
                .unwrap_or(0.0);
            let fit = mass_units_that_fit(
                actors,
                factuals,
                history,
                buyer,
                seller,
                good,
                &payable,
                units,
                price,
                transport_amv,
            );
            if fit.moved >= 1.0 && pay_quantity(&fit.pay) >= 1.0 {
                let half = mass_freight_half(factuals, history.friction, good, fit.moved, &fit.pay);
                self.settle_mass_trade(
                    actors,
                    factuals,
                    buyer,
                    seller,
                    good,
                    price,
                    fit.moved,
                    &fit.pay,
                    half,
                    fit.haul,
                    &fit.haul_pay,
                    fit.seller_advances,
                );
                exchanged += fit.moved;
                for &(id, qty) in fit.pay.iter().chain(fit.haul_pay.iter()) {
                    if qty >= 1.0 {
                        *paid.entry(id).or_insert(0.0) += qty;
                    }
                }
                buy_fill[buy_at].1 -= fit.moved;
                sell_fill[sell_at].1 -= fit.moved;
            } else if buy_fill[buy_at].1 >= 1.0
                && sell_fill[sell_at].1 >= 1.0
                && seller_free >= 1.0
                && mix_change(
                    1.0,
                    price,
                    &priced_coins(&payable, history, |id| {
                        actors.get(payer).free_units(id).floor()
                    }),
                )
                .0 >= 1.0
            {
                // One unit was payable and still could not be hauled.
                haul_blocked = true;
            }
            let buyer_open =
                buy_fill[buy_at].1 >= 1.0 && (price < 0.0 || holds_tender(actors, buyer, &payable));
            let seller_open = sell_fill[sell_at].1 >= 1.0
                && actors.get(seller).free_units(good).floor() >= 1.0
                && (price > 0.0 || holds_tender(actors, seller, &payable));
            if !buyer_open {
                buy_at += 1;
            } else if !seller_open || fit.moved < 1.0 || sell_fill[sell_at].1 < 1.0 {
                sell_at += 1;
            }
        }
        let taken = if exchanged >= 1.0 { Some(price) } else { None };
        if taken.is_some() {
            self.note_mass_exchange(good, price, exchanged, &paid);
        }
        let miss = if taken.is_none() && haul_blocked {
            both_sides_miss(offered, requested, MassMiss::Haul)
        } else {
            None
        };
        self.remember_mass_close(good, offered, requested, exchanged, taken, miss);
        exchanged
    }

    /// # Add Unpaid Want
    ///
    /// Adds `units` to the unpaid bid stored for `good`.
    ///
    /// A non-positive or non-finite amount is ignored. The night reads the
    /// total from [`Self::record_keeping`]. Nothing is returned.
    pub(crate) fn add_unpaid_want(&mut self, good: usize, units: f64) {
        if units > 0.0 && units.is_finite() {
            *self.unpaid_wants.entry(good).or_insert(0.0) += units;
        }
    }

    /// # Set Unaffordable
    ///
    /// Replaces the goods nobody could buy for one unit today.
    ///
    /// `goods` is that set. The previous set is dropped. [`drag_unaffordable`]
    /// reads it at the night write. Nothing is returned.
    pub(crate) fn set_unaffordable(&mut self, goods: HashSet<usize>) {
        self.unaffordable = goods;
    }

    /// # Remember Mass Close
    ///
    /// Stores one mass-market row for `good`.
    ///
    /// `offered` and `requested` are the amounts quoted. `exchanged` is
    /// what changed hands. `price` is the clearing AMV when that exchange
    /// happened. `miss` is why both sides quoted and nothing moved. A later
    /// pass for the same good keeps the larger quote, adds the exchange, and
    /// keeps a price that traded. A pass that exchanges at least one unit
    /// clears `miss`.
    /// A later pass that does not trade keeps a miss already recorded.
    /// Nothing is returned.
    pub(crate) fn remember_mass_close(
        &mut self,
        good: usize,
        offered: f64,
        requested: f64,
        exchanged: f64,
        price: Option<f64>,
        miss: Option<MassMiss>,
    ) {
        if let Some(row) = self.mass_closes.iter_mut().find(|row| row.good == good) {
            row.offered = row.offered.max(offered);
            row.requested = row.requested.max(requested);
            row.exchanged += exchanged;
            if row.exchanged > row.offered {
                row.offered = row.exchanged;
            }
            if row.exchanged > row.requested {
                row.requested = row.exchanged;
            }
            if price.is_some() {
                row.price = price;
            }
            if row.exchanged >= 1.0 {
                row.miss = None;
            } else if miss.is_some() {
                row.miss = miss;
            }
            return;
        }
        self.mass_closes.push(MassClose {
            good,
            offered,
            requested,
            exchanged,
            price,
            miss,
        });
    }

    /// # Note Mass Exchange
    ///
    /// Writes one mass clear onto the tape the night prices from.
    ///
    /// `price` is the clearing AMV of `good`. `exchanged` is the units that
    /// moved. `paid` is the tender units that paid for them, haul included,
    /// by id. The good's print target is the holding value of `price`, so
    /// the night steps its card toward that AMV. `traded` takes `exchanged`
    /// on the good that was bought. Each tender's `paid` takes that
    /// tender's units. `clear_units` and `clear_value` take the good's own
    /// exchange. Nothing is returned.
    pub(crate) fn note_mass_exchange(
        &mut self,
        good: usize,
        price: f64,
        exchanged: f64,
        paid: &HashMap<usize, f64>,
    ) {
        let row = self.goods.entry(good).or_insert_with(MarketGood::new);
        let holding = if price > 0.0 {
            price * amv_scale(row.salability)
        } else {
            price
        };
        row.print_units += exchanged;
        row.print_holding += holding * exchanged;
        row.traded += exchanged;
        row.clear_units += exchanged;
        row.clear_value += price * exchanged;
        for (&id, &qty) in paid {
            if qty >= 1.0 && id != good {
                let pay = self.goods.entry(id).or_insert_with(MarketGood::new);
                pay.paid += qty;
            }
        }
    }

    /// # Settle Mass Trade
    ///
    /// Moves one mass-market fill. The tape is written once for the good.
    ///
    /// `moved` units of `good` go to `buyer`. `pay` is the tender mix. Each
    /// entry moves from the buyer when `price` is positive, and from the
    /// seller when `price` is negative. `half` is that side's freight from
    /// [`mass_freight_half`]. `haul` is transport advanced first so the
    /// short side can pay `half`. `haul_pay` is the tender mix for that
    /// advance. `seller_advances` means the seller hands that transport
    /// over and the buyer pays `haul_pay`. Otherwise the buyer hands it
    /// over and the seller pays. The buyer pays the half before the good
    /// arrives, and the seller pays it after the good leaves, so neither
    /// side spends units this fill is moving. The deal's freight stays 0
    /// so finalize does not charge it again. Nothing is returned.
    fn settle_mass_trade(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        buyer: Actor,
        seller: Actor,
        good: usize,
        price: f64,
        moved: f64,
        pay: &[(usize, f64)],
        half: f64,
        haul: f64,
        haul_pay: &[(usize, f64)],
        seller_advances: bool,
    ) {
        if let Some((transport, _)) = haul_good(factuals) {
            if haul >= 1.0 && pay_quantity(haul_pay) >= 1.0 {
                let mut goods = HashMap::new();
                let transport_qty = if seller_advances { haul } else { -haul };
                goods.insert(transport, transport_qty);
                for &(id, qty) in haul_pay {
                    if qty >= 1.0 && id != transport {
                        let signed = if seller_advances { -qty } else { qty };
                        goods.insert(id, signed);
                    }
                }
                let mut advance = ProposedDeal {
                    buyer,
                    seller,
                    match_good: transport,
                    goods,
                    fresh: HashMap::new(),
                    freight: 0.0,
                };
                advance.stamp_fresh_shares(|actor, held| actors.get(actor).fresh_share(held));
                actors.get_mut(buyer).finalize(&advance, factuals);
                actors.get_mut(seller).finalize(&advance, factuals);
            }
        }
        let mut goods = HashMap::from([(good, moved)]);
        for &(id, qty) in pay {
            if qty < 1.0 || id == good {
                continue;
            }
            let signed = if price > 0.0 { -qty } else { qty };
            goods.insert(id, signed);
        }
        let mut deal = ProposedDeal {
            buyer,
            seller,
            match_good: good,
            goods,
            fresh: HashMap::new(),
            freight: 0.0,
        };
        deal.stamp_fresh_shares(|actor, held| actors.get(actor).fresh_share(held));
        // The arrival is not cover. Take the buyer's half from stock already free.
        actors.get_mut(buyer).pay_transport(half, factuals);
        actors.get_mut(buyer).finalize(&deal, factuals);
        actors.get_mut(seller).finalize(&deal, factuals);
        actors.get_mut(seller).pay_transport(half, factuals);
    }

    /// # Rebalance Population Cap
    ///
    /// Scales every capped pop by one factor when their combined headcount
    /// is above the sum of their shares.
    ///
    /// `actors` holds the pops. Each [`Pop::population_cap`](crate::game::pop::Pop::population_cap)
    /// is that pop's share, in people. A pop with no share is left out of
    /// both sums and is not scaled. The factor is the summed shares divided
    /// by the summed headcount, applied to household count, so the age mix
    /// stays and the capped pops sum to the ceiling. A non-finite or negative
    /// share is left out. A summed headcount of 0, or one already at or below
    /// the ceiling, leaves every pop alone. Desire amounts are matched later
    /// by [`Pop::rescale_desires`](crate::game::pop::Pop::rescale_desires).
    pub(crate) fn rebalance_population_cap(&self, actors: &mut Actors) {
        let mut ceiling = 0.0;
        let mut people = 0.0;
        let mut ids = Vec::new();
        for actor in self.members() {
            let Actor::Pop(id) = actor else {
                continue;
            };
            let pop = actors.pop(id);
            let Some(share) = pop.population_cap else {
                continue;
            };
            if !share.is_finite() || share < 0.0 {
                continue;
            }
            let size = pop.demographics.total_population();
            if !size.is_finite() || size < 0.0 {
                continue;
            }
            ceiling += share;
            people += size;
            ids.push(id);
        }
        if people <= ceiling || people == 0.0 {
            return;
        }
        let factor = ceiling / people;
        for id in ids {
            actors.pop_mut(id).demographics.household.count *= factor;
        }
    }

    /// # Prune Pops Below One Household
    ///
    /// Removes every member pop whose household count is under 1.
    ///
    /// The pop is dropped from this market and from `actors`. Its goods go
    /// with it. Growth and [`Self::rebalance_population_cap`] can both land
    /// a pop here. A count of 1 stays. Each removed id is recorded as
    /// [`PopExit::UnderOneHousehold`], lowest id first.
    pub(crate) fn prune_pops_below_one_household(&mut self, actors: &mut Actors) {
        let mut doomed: Vec<usize> = self
            .pops
            .iter()
            .copied()
            .filter(|&id| actors.pop(id).demographics.household.count < 1.0)
            .collect();
        doomed.sort_unstable();
        for id in doomed {
            self.pop_removals.push(PopRemoval {
                id,
                why: PopExit::UnderOneHousehold,
            });
            self.pops.remove(&id);
            actors.pops.remove(&id);
        }
    }

    /// # Internal Migration
    ///
    /// Moves households between the pops on this market, then clears the pool.
    ///
    /// The search space is these pops. Standard of living is a
    /// household-weighted average. Wealth is per person, and that average
    /// weights people. Profit is the best running trade score, and that
    /// average counts each pop once. A non-finite living standard
    /// is left out of that average and out of the living comparison. One point
    /// of living is the same gap at any scale, and one point of profit is
    /// that same gap. Each gap is squared. Fear adds
    /// its full share and anger adds half of its share, on top of that level.
    /// Wealth below the average adds the square of its relative shortfall.
    /// A pop above the average living standard, or above the average profit,
    /// pulls. Those surpluses are squared and added, and hope adds its full
    /// share. The paced share is [`MIGRATION_PACE`] times `score / (1 + score)`.
    /// The offer and the room are that share times [`migration_size_scale`],
    /// each capped at the whole pop. A one-point gap, with no fear or anger,
    /// is a twentieth before that scale. A pop [`keeps_a_sought_shop`]
    /// reports offers none of that paced share.
    ///
    /// An offer splits across open destinations in proportion to each one's
    /// gated room. A destination keeps at most its room, and each pair is
    /// also capped by that gate. Households with nowhere to sit stay home.
    /// A move that would leave the origin under one household takes the whole
    /// count instead, and the receivers may pass their room. The origin is
    /// then removed. [`Self::note_full_leave`] records why. Flows into a pop
    /// that is itself leaving are dropped first.
    /// [`seat_evacuation`] then replaces that flow for a pop [`is_evacuating`]
    /// reports. A dead pop that still has nowhere to sit, including the last
    /// pop on the market, switches job through [`retool_stranded`].
    ///
    /// The gate is demographic distance times wealth. A different species is
    /// closed. Distance otherwise counts mismatches of culture, stratum, and
    /// religion, and a matching species is one of the four ids. A distance
    /// of 0 is open. Otherwise [`assimilation_cap`] limits the pair. That cap
    /// averages culture assimilation with religion conversion. Moving to a
    /// richer pop is harder by `origin / destination` wealth per person. A
    /// different craft closes once the destination is more than 10% richer.
    /// Craft `0` is not a shared craft. An origin with wealth at or below 0
    /// has no upward move. A poorer destination stays open.
    ///
    /// Property leaves through [`Pop::take_migrants`](crate::game::pop::Pop::take_migrants).
    /// The destination's age mix updates through
    /// [`Pop::receive_migrants`](crate::game::pop::Pop::receive_migrants).
    /// A pop under one household is not a participant. The pool is empty
    /// again at the end of the pass. The net household change stays on
    /// [`Pop::migration`](crate::game::pop::Pop::migration) until the next pass.
    pub(crate) fn internal_migration(&mut self, actors: &mut Actors, factuals: &Factuals) {
        let history = self.history();
        // Clear yesterday's net on every pop. A pop that does not move this pass stays at 0.
        let mut ids: Vec<usize> = self.pops.iter().copied().collect();
        ids.sort_unstable();
        for &id in &ids {
            actors.pop_mut(id).migration = 0.0;
        }
        // Keep pops of at least one household. Sorted ids make the goods split stable.
        let currencies = self.currencies();
        let mut rows = Vec::new();
        for id in ids {
            let pop = actors.pop(id);
            let count = pop.demographics.household.count;
            if count < 1.0 {
                continue;
            }
            rows.push(Mover {
                id,
                count,
                people: pop.demographics.total_population(),
                sol: pop.standard_of_living(),
                wealth: pop.wealth_per_person(&history),
                profit: running_profit(pop, factuals, &history),
                fear: pop.sentiment.fear(),
                anger: pop.sentiment.anger(),
                hope: pop.sentiment.hope(),
                craft: pop.job.craft,
                species: pop.demographics.species,
                culture: pop.demographics.culture,
                stratum: pop.demographics.stratum,
                religion: pop.demographics.religion,
                offer: 0.0,
                room: 0.0,
                evacuate: is_evacuating(pop, &currencies),
            });
        }
        // One pop, or none, has no other pop on this market to move to.
        if rows.len() < 2 {
            self.retool_stranded(actors, factuals, &rows);
            return;
        }
        // Offer and room, against the living, wealth, and profit averages.
        let avg_sol = weighted_sol(&rows);
        let avg_wealth = weighted_wealth(&rows);
        let avg_profit = mean_profit(&rows);
        for row in &mut rows {
            let push = push_score(
                sol_gap(row.sol, avg_sol),
                wealth_shortfall(row.wealth, avg_wealth),
                sol_gap(row.profit, avg_profit),
                row.fear,
                row.anger,
            );
            let pull = pull_score(
                sol_surplus(row.sol, avg_sol),
                sol_surplus(row.profit, avg_profit),
                row.hope,
            );
            // Offer and room each grow with the people, and stop at the whole pop.
            let copies = migration_size_scale(row.people);
            row.offer = row.count * (migration_fraction(push) * copies).min(1.0);
            row.room = row.count * (migration_fraction(pull) * copies).min(1.0);
            // Zero the paced offer. Evacuation still replaces an idle pop.
            if keeps_a_sought_shop(actors.pop(row.id), factuals, &history) {
                row.offer = 0.0;
            }
        }

        // Weight each destination by its room and by the wealth and demographic gate.
        let weight = pair_weights(&rows, factuals);
        // Split each offer across those weights, then hold each destination to its room.
        let mut flow = seat_offers(&rows, &weight);
        // An idle pop with no coin replaces that seating with its whole count.
        seat_evacuation(&rows, &mut flow, factuals);
        // Do not deliver into a pop that would be left under one household.
        clear_arrivals_at_leavers(&rows, &mut flow);
        // That pop sends everyone. Receivers may pass the room just applied.
        let leaving = expand_leavers(&rows, &mut flow);

        // Take every origin before any destination receives.
        let n = rows.len();
        let mut parcels: Vec<(usize, MigrantGroup)> = Vec::new();
        for i in 0..n {
            let out = outflow(&flow, i);
            if out <= 0.0 {
                continue;
            }
            let group = {
                let pop = actors.pop_mut(rows[i].id);
                let group = pop.take_migrants(out);
                pop.migration -= group.households;
                group
            };
            let dests: Vec<usize> = (0..n).filter(|&j| flow[i][j] > 0.0).collect();
            let shares: Vec<f64> = dests.iter().map(|&j| flow[i][j] / out).collect();
            let quantities = split_stock(&group.quantity, &shares);
            let used_parts = split_stock(&group.used, &shares);
            for ((j, quantity), used) in dests.into_iter().zip(quantities).zip(used_parts) {
                parcels.push((
                    rows[j].id,
                    MigrantGroup {
                        households: flow[i][j],
                        shape: group.shape,
                        quantity,
                        used,
                    },
                ));
            }
        }
        // Record the gain and fold the arrivals into the destination.
        for (dest, group) in parcels {
            let pop = actors.pop_mut(dest);
            pop.migration += group.households;
            pop.receive_migrants(&group);
        }
        // Remove a pop that left in full. Goods that did not travel go with it.
        for i in 0..rows.len() {
            if leaving[i] {
                let id = rows[i].id;
                self.note_full_leave(&rows, &flow, i);
                self.pops.remove(&id);
                actors.pops.remove(&id);
            }
        }
        // A dead pop that stayed has no seat. Point it at work it can still run.
        self.retool_stranded(actors, factuals, &rows);
    }

    /// # Note Full Leave
    ///
    /// Records that the pop at `index` left in full.
    ///
    /// `rows` are the movers. `flow` is the seating after [`expand_leavers`].
    /// An evacuating pop is [`PopExit::NoWorkNoCoin`]. Any other full leave
    /// is [`PopExit::UnderOneMove`]. Destinations are the positive cells on
    /// that row, lowest id first.
    fn note_full_leave(&mut self, rows: &[Mover], flow: &[Vec<f64>], index: usize) {
        let mut to: Vec<usize> = (0..rows.len())
            .filter(|&dest| flow[index][dest] > 0.0)
            .map(|dest| rows[dest].id)
            .collect();
        to.sort_unstable();
        let why = if rows[index].evacuate {
            PopExit::NoWorkNoCoin { to }
        } else {
            PopExit::UnderOneMove { to }
        };
        self.pop_removals.push(PopRemoval {
            id: rows[index].id,
            why,
        });
    }

    /// # Found Pop
    ///
    /// Adds one pop, or sends the leaving households into a pop that already has the job.
    ///
    /// `actors` holds the pops. `factuals` supplies processes and crafts.
    /// The candidates are [`founding_candidates`]. The first whose edit can leave is the
    /// founder. A pop with no edit does not block the next. A pop under
    /// [`FOUNDING_MIN_HOUSEHOLDS`] cannot found,
    /// and neither can one whose set-aside covers fewer days than
    /// [`crate::game::config::PopConfig::founding_wealth`]. A founding
    /// then waits [`crate::game::config::PopConfig::founding_gap`] market
    /// days before another party may leave. The job is
    /// [`founding_processes`]: the founder's lines with one process added,
    /// one removed, or both. [`craft_id_for`] names the craft. One pop is added,
    /// unless another pop already has that demographic and that process
    /// set. That pop receives the households instead. [`floor_processes`]
    /// adds subsistence the new job cannot run, and that floor is part of
    /// the set. A floor that leaves the founder's own set puts the
    /// households back, and the next candidate is asked. The founder gives
    /// [`FOUNDING_HOUSEHOLDS`].
    /// Working goods move in that share of the pile, so the parent keeps
    /// the same amount per household. [`Pop::take_founding_households`]
    /// also sends [`crate::game::config::PopConfig::founding_goods_weight`]
    /// days of set-aside for each household that leaves.
    /// The child's averages are the founder's, and its count is the
    /// households that moved. It copies the four demographic ids, starts
    /// with no population-cap share, and its migration is the households
    /// it received. The founder's migration falls by the same count.
    /// Nothing is returned.
    pub(crate) fn found_pop(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        rng: &mut impl Rng,
    ) {
        if self.founding_rest > 0 {
            self.founding_rest -= 1;
            return;
        }
        let wealth = factuals.config.pop.founding_wealth;
        let savings_days = factuals.config.pop.founding_goods_weight;
        for founder in founding_candidates(self, actors, wealth) {
            let Some(choice) = founding_processes(self, actors, factuals, founder, rng) else {
                continue;
            };
            let demo = actors.pop(founder).demographics;
            let history = self.history();
            let group = {
                let pop = actors.pop_mut(founder);
                // Working goods stay proportioned. The gift is days of set-aside.
                let group =
                    pop.take_founding_households(FOUNDING_HOUSEHOLDS, savings_days, &history);
                pop.migration -= group.households;
                group
            };
            if group.households <= 0.0 {
                continue;
            }
            let floor = floor_processes(factuals, choice.processes(), &group.quantity);
            let sent = match choice {
                FoundingChoice::Join { dest, .. } => {
                    let pop = actors.pop_mut(dest);
                    pop.migration += group.households;
                    pop.receive_migrants(&group);
                    // The specialty cannot run on the goods a fresh pop would hold.
                    let added = open_missing_lines(&mut pop.job, &floor);
                    if added {
                        let processes = founder_processes(pop);
                        pop.job.craft = craft_id_for(factuals, &processes);
                    }
                    true
                }
                FoundingChoice::Create(mut processes) => {
                    for id in &floor {
                        if !processes.contains(id) {
                            processes.push(*id);
                        }
                    }
                    // The floor can land on a job that already exists.
                    if let Some(dest) =
                        other_pop_with_set(&self.pops, actors, founder, demo, &processes)
                    {
                        let pop = actors.pop_mut(dest);
                        pop.migration += group.households;
                        pop.receive_migrants(&group);
                        let added = open_missing_lines(&mut pop.job, &floor);
                        if added {
                            let processes = founder_processes(pop);
                            pop.job.craft = craft_id_for(factuals, &processes);
                        }
                        true
                    } else if process_set_taken(&self.pops, actors, demo, &processes) {
                        let pop = actors.pop_mut(founder);
                        pop.migration += group.households;
                        pop.receive_migrants(&group);
                        false
                    } else {
                        let id = next_pop_id(self, actors);
                        let mut child = Pop::new(id);
                        child.demographics.species = demo.species;
                        child.demographics.culture = demo.culture;
                        child.demographics.stratum = demo.stratum;
                        child.demographics.religion = demo.religion;
                        child.demographics.household = group.shape;
                        child.demographics.household.count = group.households;
                        for (&good, &qty) in &group.quantity {
                            if qty != 0.0 {
                                child.property.entry(good).or_default().quantity += qty;
                            }
                        }
                        for (&good, &qty) in &group.used {
                            if qty != 0.0 {
                                child.property.entry(good).or_default().used += qty;
                            }
                        }
                        child.job = Job::new(craft_id_for(factuals, &processes), Vec::new());
                        open_missing_lines(&mut child.job, &processes);
                        child.migration = group.households;
                        child.update_desires(factuals);
                        self.pops.insert(id);
                        actors.pops.insert(id, child);
                        true
                    }
                }
            };
            if sent {
                self.arm_founding_gap(factuals.config.pop.founding_gap);
                return;
            }
        }
    }

    /// # Fold Duplicate Pops
    ///
    /// Sends a later pop into the earlier pop with the same demographic and job.
    ///
    /// `actors` holds the pops. The demographic is species, culture, stratum,
    /// and religion. The job is the process ids on the lines. Line order does
    /// not matter. An empty job is not a match. The lowest id keeps its lines
    /// and its targets. Each later pop gives every household and the goods it
    /// holds, through [`Pop::give_way`](crate::game::pop::Pop::give_way), and
    /// is removed. The removal is [`PopExit::SameJob`]. Nothing is returned.
    fn fold_duplicate_pops(&mut self, actors: &mut Actors) {
        let mut ids: Vec<usize> = self
            .pops
            .iter()
            .copied()
            .filter(|id| actors.pops.contains_key(id))
            .collect();
        ids.sort_unstable();
        let mut removed = HashSet::new();
        for (index, &id) in ids.iter().enumerate() {
            if removed.contains(&id) {
                continue;
            }
            let key = job_key(actors.pop(id));
            if key.processes.is_empty() {
                continue;
            }
            let Some(dest) = ids[..index].iter().copied().find(|&other| {
                !removed.contains(&other)
                    && actors.pops.contains_key(&other)
                    && same_job_key(actors.pop(other), &key)
            }) else {
                continue;
            };
            if actors.pop(id).demographics.household.count <= 0.0 {
                continue;
            }
            let mut origin = actors.pops.remove(&id).expect("pop");
            origin.give_way(actors.pop_mut(dest));
            self.pops.remove(&id);
            removed.insert(id);
            self.pop_removals.push(PopRemoval {
                id,
                why: PopExit::SameJob { to: vec![dest] },
            });
        }
    }

    /// # Arm Founding Gap
    ///
    /// Starts the wait after a household has left.
    ///
    /// `gap` is the market days from this founding to the next one that may
    /// leave. One day, or a gap that is not finite, arms nothing. A longer
    /// gap blocks the calls in between.
    fn arm_founding_gap(&mut self, gap: f64) {
        if !gap.is_finite() || gap <= 1.0 {
            return;
        }
        self.founding_rest = (gap.floor() as u32).saturating_sub(1);
    }

    /// # Is Founding Ready
    ///
    /// Whether `pop` may found a new job.
    ///
    /// `wealth` is the set-aside floor. The answer is
    /// [`clears_founding_gate`] on this market's card. The wait between
    /// foundings is [`Self::founding_wait`] and is not part of this answer.
    pub fn is_founding_ready(&self, pop: &Pop, wealth: f64) -> bool {
        clears_founding_gate(pop, &self.history(), wealth)
    }

    /// # Founding Wait
    ///
    /// Market days still blocked after a household has left to found.
    ///
    /// Zero means the next founding may leave. Returns that count.
    pub fn founding_wait(&self) -> u32 {
        self.founding_rest
    }

    /// # Retool Stranded
    ///
    /// Points each dead pop in `rows` at work it can still run.
    ///
    /// `actors` holds the pops. `factuals` supplies the processes. A pop
    /// [`is_evacuating`] reports, and that is still stored, receives
    /// [`recovery_processes`]. A pop that already runs that set is left
    /// as it is. Nothing is returned.
    fn retool_stranded(&self, actors: &mut Actors, factuals: &Factuals, rows: &[Mover]) {
        let currencies = self.currencies();
        for row in rows {
            if !actors.pops.contains_key(&row.id) {
                continue;
            }
            if !is_evacuating(actors.pop(row.id), &currencies) {
                continue;
            }
            let processes = recovery_processes(factuals, actors.pop(row.id));
            if processes.is_empty()
                || same_process_set(&founder_processes(actors.pop(row.id)), &processes)
            {
                continue;
            }
            let craft = craft_id_for(factuals, &processes);
            let pop = actors.pop_mut(row.id);
            pop.job = Job::new(craft, Vec::new());
            open_missing_lines(&mut pop.job, &processes);
        }
    }

    /// Member actors, pops then firms then institutions, each id ascending.
    fn members(&self) -> Vec<Actor> {
        let mut actors = Vec::new();
        let mut ids: Vec<usize> = self.pops.iter().copied().collect();
        ids.sort_unstable();
        actors.extend(ids.iter().copied().map(Actor::Pop));
        ids.clear();
        ids.extend(self.firms.iter().copied());
        ids.sort_unstable();
        actors.extend(ids.iter().copied().map(Actor::Firm));
        ids.clear();
        ids.extend(self.institution_ids.iter().copied());
        ids.sort_unstable();
        actors.extend(ids.iter().copied().map(Actor::Institution));
        actors
    }
}

/// # Tradeable
///
/// True when `good` may be bought and sold.
///
/// A good missing from `factuals` is tradeable. A stored good is tradeable
/// when [`crate::game::good::Good::is_buyable`] says so.
pub(crate) fn tradeable(factuals: &Factuals, good: usize) -> bool {
    factuals.get_good(good).is_none_or(Good::is_buyable)
}

/// # Payment Tenders
///
/// Every filled tender slot that can pay for `good`, in slot order.
///
/// `currencies` are the market slots, empty entries included. The good
/// itself is left out, so a tender is not bought with itself. Duplicate
/// slots are listed once. No other filled slot returns an empty list.
pub(crate) fn payment_tenders(currencies: &[Option<usize>], good: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for id in currencies.iter().copied().flatten() {
        if id != good && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// # Payment Tender
///
/// The first tender in [`payment_tenders`], or `None` when that list is empty.
///
/// `currencies` are the market slots, empty entries included. `good` is
/// the good being paid for. The mix that actually pays is the whole list.
pub(crate) fn payment_tender(currencies: &[Option<usize>], good: usize) -> Option<usize> {
    payment_tenders(currencies, good).into_iter().next()
}

/// A saved price snapshot for one market.
#[derive(Debug, Clone)]
pub struct MarketHistory {
    /// Last known AMV per good.
    pub prices: HashMap<usize, f64>,
    /// Last known salability per good.
    pub salability: HashMap<usize, f64>,
    /// Units the night plan still treats as wanted.
    ///
    /// [`Market::history`] fills this. Empty on a snapshot taken before exchange.
    pub sought: HashMap<usize, f64>,
    /// Units produced today, per good.
    ///
    /// Copied from [`MarketGood::production`]. Empty when nothing was made.
    pub produced: HashMap<usize, f64>,
    /// Whole units offered on today's mass book.
    ///
    /// [`Market::history`] copies [`MassClose::offered`]. Empty when the book
    /// quoted no sell. [`Self::is_mass_supplied`] is what the night plan reads.
    pub mass_offered: HashMap<usize, f64>,
    /// Whole units exchanged on today's mass book.
    ///
    /// [`Market::history`] copies [`MassClose::exchanged`].
    pub mass_exchanged: HashMap<usize, f64>,
    /// Copied from [`Market::friction`].
    pub friction: f64,
    /// Used when a good has no recorded salability.
    pub default_salability: f64,
    /// Refused-ask fraction per good, copied from the market.
    pub ask_walk: HashMap<usize, f64>,
    /// Tender good ids, filled slots only, in slot order.
    ///
    /// [`Market::history`] copies them. Empty when the snapshot has no tender.
    pub currencies: Vec<usize>,
    /// Goods a buyer asked that nobody could cover for one unit.
    ///
    /// [`Market::history`] copies the market's set. Empty when every ask
    /// could buy, or when the snapshot was taken after the set was cleared.
    pub unaffordable: HashSet<usize>,
}

impl Default for MarketHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketHistory {
    pub fn new() -> Self {
        Self {
            prices: HashMap::new(),
            salability: HashMap::new(),
            sought: HashMap::new(),
            produced: HashMap::new(),
            mass_offered: HashMap::new(),
            mass_exchanged: HashMap::new(),
            friction: 0.0,
            default_salability: SALABILITY_DEFAULT,
            ask_walk: HashMap::new(),
            currencies: Vec::new(),
            unaffordable: HashSet::new(),
        }
    }

    /// # Is Unaffordable
    ///
    /// True when a buyer asked for `good_id` and nobody could cover one unit.
    ///
    /// A good missing from the set is false.
    pub fn is_unaffordable(&self, good_id: usize) -> bool {
        self.unaffordable.contains(&good_id)
    }

    /// # Currencies
    ///
    /// Tender good ids on this snapshot, in slot order.
    pub fn currencies(&self) -> &[usize] {
        &self.currencies
    }

    /// # Ask Walk
    ///
    /// How far `good_id`'s refused sell floor has walked toward the card.
    ///
    /// A good with no stored fraction returns 0.
    pub fn ask_walk(&self, good_id: usize) -> f64 {
        self.ask_walk.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Sought
    ///
    /// Units of `good_id` still sought on this snapshot, or 0.
    pub fn sought(&self, good_id: usize) -> f64 {
        self.sought.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Produced
    ///
    /// Units of `good_id` produced on this snapshot, or 0.
    pub fn produced(&self, good_id: usize) -> f64 {
        self.produced.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Mass Offered
    ///
    /// Whole units of `good_id` offered on this snapshot's mass book, or 0.
    pub fn mass_offered(&self, good_id: usize) -> f64 {
        self.mass_offered.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Mass Exchanged
    ///
    /// Whole units of `good_id` exchanged on this snapshot's mass book, or 0.
    pub fn mass_exchanged(&self, good_id: usize) -> f64 {
        self.mass_exchanged.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Is Mass Supplied
    ///
    /// True when the mass book offered or exchanged at least one unit of `good_id`.
    ///
    /// A good missing from both maps is false. A fraction under one is false.
    pub fn is_mass_supplied(&self, good_id: usize) -> bool {
        self.mass_offered.get(&good_id).copied().unwrap_or(0.0) >= 1.0
            || self.mass_exchanged.get(&good_id).copied().unwrap_or(0.0) >= 1.0
    }

    /// AMV for `good_id`, or 1.0 if this snapshot has none.
    pub fn price(&self, good_id: usize) -> f64 {
        self.prices.get(&good_id).copied().unwrap_or(1.0)
    }

    /// Salability for `good_id`, or this snapshot's default if missing.
    pub fn salability(&self, good_id: usize) -> f64 {
        self.salability
            .get(&good_id)
            .copied()
            .unwrap_or(self.default_salability)
    }

    /// Indirect value of one unit on this card.
    ///
    /// A positive AMV is multiplied by [`amv_scale`]. A negative AMV is the
    /// cost of holding it and is not scaled.
    pub fn holding_per_unit(&self, good_id: usize) -> f64 {
        let amv = self.price(good_id);
        if amv > 0.0 {
            amv * amv_scale(self.salability(good_id))
        } else {
            amv
        }
    }
}

/// Per-market price snapshots plus pop-id to market-id.
#[derive(Debug, Clone, Default)]
pub struct MarketLookups {
    pub histories: HashMap<usize, MarketHistory>,
    pub pop_to_market: HashMap<usize, usize>,
}

impl MarketLookups {
    pub fn new() -> Self {
        Self::default()
    }

    /// One history per market, and each member pop id mapped to that market id.
    pub fn from_markets(markets: &HashMap<usize, Market>) -> Self {
        let mut histories = HashMap::new();
        let mut pop_to_market = HashMap::new();
        for market in markets.values() {
            histories.insert(market.id, market.history());
            for &pop_id in &market.pops {
                pop_to_market.insert(pop_id, market.id);
            }
        }
        Self {
            histories,
            pop_to_market,
        }
    }

    /// History for `pop_id`'s market, or `empty` if the pop is in none.
    pub fn history_for_pop<'a>(
        &'a self,
        pop_id: usize,
        empty: &'a MarketHistory,
    ) -> &'a MarketHistory {
        self.pop_to_market
            .get(&pop_id)
            .and_then(|mid| self.histories.get(mid))
            .unwrap_or(empty)
    }
}

/// # Mass Idle By Good
///
/// The AMV fraction each mass book applies.
///
/// `closes` are today's rows. `currencies` are the tender slots.
/// `factuals` and `history` are the inputs of [`mass_book_can_move`].
/// `made` is today's output, the `made` argument of [`mass_idle_fraction`].
/// `velocity` is the tender spent on other goods, added to both sides.
/// A fraction of 0, and a good that cannot trade, are left out.
/// [`Market::record_keeping`] decides which of these fractions it writes.
/// Returns [`mass_idle_fraction`] per good.
fn mass_idle_by_good(
    closes: &[MassClose],
    currencies: &[Option<usize>],
    factuals: &Factuals,
    history: &MarketHistory,
    made: &HashMap<usize, f64>,
    velocity: &HashMap<usize, f64>,
) -> HashMap<usize, f64> {
    let mut steps = HashMap::new();
    for close in closes {
        let covered = made.get(&close.good).copied().unwrap_or(0.0);
        let spent = velocity.get(&close.good).copied().unwrap_or(0.0);
        let step = mass_idle_fraction(close, covered, spent);
        if step == 0.0 || !mass_book_can_move(factuals, currencies, history, close.good) {
            continue;
        }
        steps.insert(close.good, step);
    }
    steps
}

/// # Currency Book Can Move
///
/// True when today's full night write for `good` may run.
///
/// `closes` are today's rows. `currencies` are the tender slots. `unpaid`
/// is the one-unit want on `good`, and it is not a purchase bid. A good
/// that is not a filled slot may move. A currency may move when some other
/// tender was offered for at least one unit. It may also move when its own
/// row offered at least one unit and the purchase bid is at least one unit:
/// other goods can pay that book, so the quote is a price. A surplus whose
/// only bid is the unpaid want, and a one-sided row with no other tender
/// offered, return false. [`Market::record_keeping`] leaves that AMV
/// unwritten, except a purchase bid larger than the offer. Returns that
/// answer.
fn currency_book_can_move(
    closes: &[MassClose],
    currencies: &[Option<usize>],
    good: usize,
    unpaid: f64,
) -> bool {
    if !currencies.iter().flatten().any(|&id| id == good) {
        return true;
    }
    let tenders = payment_tenders(currencies, good);
    if tenders.is_empty() {
        return true;
    }
    let other_offered = tenders.iter().any(|&tender| {
        closes
            .iter()
            .any(|close| close.good == tender && close.offered >= 1.0)
    });
    if other_offered {
        return true;
    }
    let unpaid = if unpaid.is_finite() { unpaid.max(0.0) } else { 0.0 };
    // Other goods can pay a currency, so a real bid on both sides is a price.
    closes.iter().any(|close| {
        let real = (close.requested - unpaid).max(0.0);
        close.good == good && close.offered >= 1.0 && real >= 1.0
    })
}

/// # Both Sides Miss
///
/// `miss` when both sides quoted at least one unit, otherwise nothing.
///
/// A one-sided book still takes the idle step. A book with both an offer
/// and a bid records why it did not trade.
fn both_sides_miss(offered: f64, requested: f64, miss: MassMiss) -> Option<MassMiss> {
    if offered >= 1.0 && requested >= 1.0 {
        Some(miss)
    } else {
        None
    }
}

/// # Idle Move
///
/// How far one mass book moves `old`.
///
/// `step` is [`mass_idle_fraction`]. A surplus takes that fraction off
/// `old`, so a glut shrinks toward zero. A shortage adds `step` as AMV.
/// The same short book then raises a price of 1 and a price of 8 by the
/// same amount. A zero `step` adds nothing. Returns that move.
fn idle_move(old: f64, step: f64) -> f64 {
    if step < 0.0 {
        old.abs() * step
    } else if step > 0.0 {
        step
    } else {
        0.0
    }
}

/// # Mass Idle Fraction
///
/// The fraction [`idle_move`] applies from the side that was left over.
///
/// `close` is that book. `made` is units produced today. The morning book
/// did not list them. A book with both sides quoted that missed on
/// [`MassMiss::NoCross`] or [`MassMiss::Haul`] returns 0: those days did
/// not show the good unwanted, and [`step_ask_walk`] is what pulls the ask
/// to the card. A larger offer returns `-[MASS_IDLE_STEP]` times the unsold
/// share of that offer. A larger request returns [`MASS_IDLE_STEP`] times
/// the share of that request still uncovered after `made`. Output that
/// covers the bid returns 0. Nothing on one side, and no output, is the
/// full step. Equal sides return 0. `velocity` is added to both sides.
/// The gap does not change. The share shrinks. A book that was already
/// equal stays equal.
fn mass_idle_fraction(close: &MassClose, made: f64, velocity: f64) -> f64 {
    let both = close.offered >= 1.0 && close.requested >= 1.0;
    if both && matches!(close.miss, Some(MassMiss::NoCross | MassMiss::Haul)) {
        return 0.0;
    }
    let velocity = if velocity.is_finite() { velocity.max(0.0) } else { 0.0 };
    // Spent tender is supply and demand together, so it only widens the book.
    let offered = close.offered + velocity;
    let requested = close.requested + velocity;
    if offered > requested {
        let share = (offered - requested) / offered;
        -MASS_IDLE_STEP * share
    } else if requested > offered {
        // Today's make was not on the book. It covers the bid it fills.
        let fresh = if made.is_finite() { made.max(0.0) } else { 0.0 };
        let uncovered = (requested - offered - fresh).max(0.0);
        let share = uncovered / requested;
        MASS_IDLE_STEP * share
    } else {
        0.0
    }
}

/// # Drag Unaffordable
///
/// The mass fraction written when `step` would raise a good nobody can buy.
///
/// `offered` and `requested` are the book's whole units. `unaffordable` is
/// set when a buyer asked and no buyer could cover one unit at the card.
/// Sellers treat not selling as getting nothing. A positive `step` is
/// replaced by that step times [`UNAFFORDABLE_DRAG`], signed down, so the
/// fall is larger than the rise it replaces. A zero step on a two-sided
/// book becomes a full [`MASS_IDLE_STEP`] downward. Any other step is
/// returned unchanged.
fn drag_unaffordable(step: f64, offered: f64, requested: f64, unaffordable: bool) -> f64 {
    if !unaffordable || offered < 1.0 || requested < 1.0 {
        return step;
    }
    if step > 0.0 {
        -UNAFFORDABLE_DRAG * step
    } else if step == 0.0 {
        -MASS_IDLE_STEP
    } else {
        step
    }
}

/// # Make Ceiling
///
/// The ask one idle step above the cheapest process that just pays for `good`.
///
/// `factuals` supplies the processes. `history` prices their other goods.
/// `salability` is this good's liquidity. [`break_even_amv`] is the ask
/// where a run scores zero. The cheapest of those asks wins. [`MASS_IDLE_STEP`]
/// is added so the stored price can sit where that run pays. A process that
/// already pays at every positive ask, and a good no process gains by
/// raising, return `None`. Returns that ask.
fn make_ceiling(
    factuals: &Factuals,
    history: &MarketHistory,
    good: usize,
    salability: f64,
) -> Option<f64> {
    let mut best: Option<f64> = None;
    for process in factuals.each_process() {
        let Some(zero) = break_even_amv(process, good, history, salability) else {
            continue;
        };
        // Someone can already make it at any positive ask.
        if zero <= 0.0 {
            return None;
        }
        best = Some(best.map_or(zero, |old| old.min(zero)));
    }
    best.map(|zero| zero + MASS_IDLE_STEP)
}

/// # Hold The Make
///
/// `next` after the mass book, kept on the paying side of `ceiling`.
///
/// `old` is the price before that book moved it. A rise that starts below
/// the ceiling stops on it. A fall that starts above the ceiling stops on
/// it. A move that is already on the paying side of the ceiling is left.
/// No ceiling returns `next`. Returns that price.
fn hold_the_make(old: f64, next: f64, ceiling: Option<f64>) -> f64 {
    let Some(ceiling) = ceiling.filter(|ask| ask.is_finite() && *ask > 0.0) else {
        return next;
    };
    if next > old {
        if old >= ceiling { old } else { next.min(ceiling) }
    } else if next < old {
        if old <= ceiling { old } else { next.max(ceiling) }
    } else {
        next
    }
}

/// # Keep Scarce Ask
///
/// `next` held at the morning ask when a short book would finish cheaper.
///
/// `old` is the ask before this night's write. `scarce` is a real bid
/// above the offer on a good buyers could pay. That book keeps `old` when
/// `next` is below it. Any other book returns `next`.
fn keep_scarce_ask(old: f64, next: f64, scarce: bool) -> f64 {
    if scarce && next < old { old } else { next }
}

/// # Currency Velocity
///
/// Units of each tender paid for other goods today.
///
/// `goods` is the day's tape. `currencies` are the tender slots. A tender's
/// [`MarketGood::paid`] is that count. A missing row, and a count that is
/// not finite and positive, are left out. Returns the count per tender.
fn currency_velocity(
    goods: &HashMap<usize, MarketGood>,
    currencies: &[Option<usize>; CURRENCY_SLOTS],
) -> HashMap<usize, f64> {
    let mut out = HashMap::new();
    for id in currencies.iter().flatten() {
        if out.contains_key(id) {
            continue;
        }
        let Some(good) = goods.get(id) else {
            continue;
        };
        if good.paid.is_finite() && good.paid > 0.0 {
            out.insert(*id, good.paid);
        }
    }
    out
}

/// # Step Ask Walk
///
/// Moves each quoted good's sell-floor walk after today's mass book.
///
/// `walk` is the stored fraction per good. `closes` are today's rows.
/// A good that exchanged at least one unit steps toward 0 by [`ASK_WALK`].
/// A walk that has already landed on the card stays there, so a sale does
/// not lift the ask back above the bid. A good that was offered and
/// exchanged nothing steps toward 1 by [`ASK_WALK`]. A book that missed on
/// [`MassMiss::NoCross`] finishes that trip in this night, so the next ask
/// lands on the card. A haul miss keeps the quarter step. The walk is what
/// brings the ask to the card while [`mass_idle_fraction`] leaves the card
/// where it is. A good with no offer is left as it stands. The fraction
/// stays in 0..=1. A fraction that lands on 0 is dropped.
fn step_ask_walk(walk: &mut HashMap<usize, f64>, closes: &[MassClose]) {
    for close in closes {
        let toward = if close.exchanged >= 1.0 {
            0.0
        } else if close.offered >= 1.0 {
            1.0
        } else {
            continue;
        };
        let current = walk.get(&close.good).copied().unwrap_or(0.0);
        // A no-cross finishes the trip. An ask already on the card stays there.
        let share = if toward > 0.0 && matches!(close.miss, Some(MassMiss::NoCross)) {
            1.0
        } else if toward == 0.0 && current >= 1.0 {
            0.0
        } else {
            ASK_WALK
        };
        let next = current + (toward - current) * share;
        if next <= 0.0 {
            walk.remove(&close.good);
        } else {
            walk.insert(close.good, next.clamp(0.0, 1.0));
        }
    }
}

/// # Mass Book Can Move
///
/// True when a mass book for `good` is allowed to move the card.
///
/// `factuals` says the good is buyable. A good missing there is buyable.
/// `currencies` and `history` pick the tenders in [`payment_tenders`] and
/// price them. No tender, or no tender AMV that is finite and positive,
/// leaves the book where it is.
fn mass_book_can_move(
    factuals: &Factuals,
    currencies: &[Option<usize>],
    history: &MarketHistory,
    good: usize,
) -> bool {
    if !tradeable(factuals, good) {
        return false;
    }
    payment_tenders(currencies, good).iter().any(|&tender| {
        let currency_amv = history.price(tender);
        currency_amv.is_finite() && currency_amv > 0.0
    })
}

/// # Print Push
///
/// Holding-space move from today's print and from an unsold offer.
///
/// `id` is the good. `history` is the pre-write card. `scale` is
/// [`Market::payment_scale`]. When `print_units` is positive, the move is
/// [`AMV_PRINT_STEP`] times the gap from morning holding to
/// `print_holding / print_units`. When the good was offered and `traded`
/// is 0, [`AMV_EXCESS_NUDGE`] times `scale` is subtracted. The sum is
/// clamped to ±[`AMV_STEP_CAP`] times `scale`.
fn print_push(id: usize, good: &MarketGood, history: &MarketHistory, scale: f64) -> f64 {
    let mut push = 0.0;
    if good.print_units > 0.0 {
        let target = good.print_holding / good.print_units;
        push += (target - history.holding_per_unit(id)) * AMV_PRINT_STEP;
    }
    if good.offered_unsold > 0.0 && good.traded == 0.0 {
        push -= scale * AMV_EXCESS_NUDGE;
    }
    let limit = scale * AMV_STEP_CAP;
    push.clamp(-limit, limit)
}

/// # AMV From Holding Move
///
/// AMV after `move_holding` is added to `old`'s holding value.
///
/// Positive holding is `old` times [`amv_scale`] of `salability`. A
/// non-positive `old` is already its holding value. A positive result is
/// divided by that same scale. A non-positive result is the holding value.
fn amv_from_holding_move(old: f64, salability: f64, move_holding: f64) -> f64 {
    let holding = if old > 0.0 {
        old * amv_scale(salability)
    } else {
        old
    };
    let next = holding + move_holding;
    if next > 0.0 {
        next / amv_scale(salability)
    } else {
        next
    }
}

/// # Valued Price
///
/// `price` kept above zero unless `below_zero` is set.
///
/// `below_zero` means a seller's minimum for this good was negative.
/// The night's decline may then cross zero. That minimum does not become
/// the price. Every other `price` below [`AMV_EPSILON`] is returned as
/// [`AMV_EPSILON`].
fn valued_price(below_zero: bool, price: f64) -> f64 {
    if below_zero || price >= AMV_EPSILON {
        price
    } else {
        AMV_EPSILON
    }
}

/// # Use Pull
///
/// Moves `price` toward today's trade and process results, by their share of the day.
///
/// `decayed` is units lost to rot. `traded` is units that cleared, and
/// `trade_price` is their clearing AMV. `process` is units destroyed in
/// processes, and `output_price` is the output-card value of those runs
/// per unit of this good. `desire` is units eaten, used, or taken as rest.
/// `step_cap` is the most the process term may add or subtract.
/// The four amounts are the day's accounted use. Trade moves toward
/// `trade_price` by its share. Process moves toward `output_price` by its
/// share, then stops at `step_cap`. Desire's target is `price`, so that
/// share adds nothing. Decay's downward move is [`decay_cut`]; its units
/// only shrink the other shares. A day with no accounted use returns `price`.
fn use_pull(
    price: f64,
    decayed: f64,
    traded: f64,
    trade_price: f64,
    process: f64,
    output_price: f64,
    desire: f64,
    step_cap: f64,
) -> f64 {
    let accounted = decayed.max(0.0) + traded.max(0.0) + process.max(0.0) + desire.max(0.0);
    if accounted <= 0.0 || !price.is_finite() {
        return price;
    }
    let mut next = price;
    if traded > 0.0 && trade_price.is_finite() {
        next += (trade_price - price) * (traded / accounted);
    }
    if process > 0.0 && output_price.is_finite() {
        let step = (output_price - price) * (process / accounted);
        next += step.clamp(-step_cap, step_cap);
    }
    next
}

/// # Decay Cut
///
/// How far rot pulls `price` down tonight.
///
/// `fraction` is the capped rot share. A positive `price` loses that share
/// of itself, and the result stays at [`AMV_EPSILON`]. A non-positive
/// `price` loses nothing: decay is waste, and a negative card means the
/// good is harmful to hold. Returns the amount subtracted.
fn decay_cut(price: f64, fraction: f64) -> f64 {
    debug_assert!(price.is_finite(), "price must be finite");
    debug_assert!(fraction.is_finite(), "fraction must be finite");
    if price <= 0.0 || fraction <= 0.0 {
        return 0.0;
    }
    let raw = price * fraction.min(1.0);
    let room = (price - AMV_EPSILON).max(0.0);
    raw.min(room)
}

/// If `new` is inside the AMV dead zone, land [`AMV_EPSILON`] on the other
/// side of 0 from `old`. Otherwise return `new`.
fn bounce_away_from_zero(old: f64, new: f64) -> f64 {
    debug_assert!(new.is_finite(), "new AMV must be finite");
    if new.abs() >= AMV_EPSILON {
        new
    } else if old >= 0.0 {
        -AMV_EPSILON
    } else {
        AMV_EPSILON
    }
}

/// Why a mass book that quoted both sides moved nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassMiss {
    /// No price had a buyer and a seller.
    NoCross,
    /// A clearing price was found and every fill died on the haul.
    Haul,
}

/// One good's mass-market book for the day.
///
/// `offered` and `requested` are whole units quoted. `exchanged` is the
/// units that changed hands. `price` is the clearing AMV when `exchanged`
/// is at least one, and missing when the book did not trade. `miss` is
/// why both sides quoted and nothing moved. A one-sided book leaves it
/// empty, and so does a book that traded.
#[derive(Debug, Clone, PartialEq)]
pub struct MassClose {
    pub good: usize,
    pub offered: f64,
    pub requested: f64,
    pub exchanged: f64,
    pub price: Option<f64>,
    pub miss: Option<MassMiss>,
}

/// Stored price and quantity for one good in a market.
#[derive(Debug, Clone)]
pub struct MarketGood {
    /// Abstract market value. May be negative. Not stored as zero.
    /// Assign through [`Self::set_amv`] so the dead zone is applied.
    pub amv: f64,
    /// How readily the good trades. `0..=1` scales a positive AMV down to par.
    /// Above 1 keeps full AMV credit; the excess is monetary rating.
    /// Clamped to [`SALABILITY_MIN`]..=[`SALABILITY_MAX`].
    pub salability: f64,
    /// Units made today.
    pub production: f64,
    /// Units consumed today.
    pub consumption: f64,
    /// Units that survived from yesterday.
    pub stock: f64,
    /// Units of this good bought today. Tender spent on another good is [`Self::paid`].
    pub traded: f64,
    /// Units that changed hands as payment today.
    pub paid: f64,
    /// Holding value attributed to this good by accepted deals today.
    pub print_holding: f64,
    /// Units of this good the buyer received in those deals.
    pub print_units: f64,
    /// Buy-order units still open when matching stopped.
    pub sought_unmet: f64,
    /// Sell-order units still open when matching stopped.
    pub offered_unsold: f64,
    /// Units lost to rot today.
    pub decayed: f64,
    /// Stock those lost units came from. The night uses `decayed / volume`.
    pub volume: f64,
    /// Units destroyed in processes today. [`use_pull`] reads this.
    pub process_used: f64,
    /// Output-card value of those runs. Per unit is this over [`Self::process_used`].
    pub process_value: f64,
    /// Units a desire ate, used, or took as rest today.
    pub desire_used: f64,
    /// Units whose clearing price is stored in [`Self::clear_value`].
    pub clear_units: f64,
    /// Sum of clearing AMV times [`Self::clear_units`].
    pub clear_value: f64,
}

impl Default for MarketGood {
    fn default() -> Self {
        Self {
            amv: 1.0,
            salability: SALABILITY_DEFAULT,
            production: 0.0,
            consumption: 0.0,
            stock: 0.0,
            traded: 0.0,
            paid: 0.0,
            print_holding: 0.0,
            print_units: 0.0,
            sought_unmet: 0.0,
            offered_unsold: 0.0,
            decayed: 0.0,
            volume: 0.0,
            process_used: 0.0,
            process_value: 0.0,
            desire_used: 0.0,
            clear_units: 0.0,
            clear_value: 0.0,
        }
    }
}

impl MarketGood {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets AMV. Values inside the dead zone bounce past zero.
    pub fn set_amv(&mut self, amv: f64) {
        self.amv = bounce_away_from_zero(self.amv, amv);
    }

    /// [`Self::set_amv`] as a builder. Starts from the current AMV.
    pub fn with_amv(mut self, amv: f64) -> Self {
        self.set_amv(amv);
        self
    }

    /// # Set Salability
    ///
    /// Stores `salability`, clamped to [`SALABILITY_MIN`]..=[`SALABILITY_MAX`].
    ///
    /// `salability` must be finite.
    pub fn set_salability(&mut self, salability: f64) {
        debug_assert!(salability.is_finite(), "salability must be finite");
        self.salability = salability.clamp(SALABILITY_MIN, SALABILITY_MAX);
    }

    pub fn with_salability(mut self, salability: f64) -> Self {
        self.set_salability(salability);
        self
    }

    pub fn set_production(&mut self, production: f64) {
        debug_assert!(production >= 0.0, "production must be >= 0");
        self.production = production;
    }

    pub fn set_consumption(&mut self, consumption: f64) {
        debug_assert!(consumption >= 0.0, "consumption must be >= 0");
        self.consumption = consumption;
    }

    pub fn set_stock(&mut self, stock: f64) {
        debug_assert!(stock >= 0.0, "stock must be >= 0");
        self.stock = stock;
    }

    /// # Clear Exchange
    ///
    /// Zeros today's exchange, print, production flow, and use shares.
    ///
    /// Leaves AMV, salability, stock, and rot (`decayed`, `volume`).
    fn clear_exchange(&mut self) {
        self.production = 0.0;
        self.consumption = 0.0;
        self.traded = 0.0;
        self.paid = 0.0;
        self.print_holding = 0.0;
        self.print_units = 0.0;
        self.sought_unmet = 0.0;
        self.offered_unsold = 0.0;
        self.process_used = 0.0;
        self.process_value = 0.0;
        self.desire_used = 0.0;
        self.clear_units = 0.0;
        self.clear_value = 0.0;
    }

    /// # Clear Day
    ///
    /// Zeros today's exchange tape, print, rot, and use shares.
    ///
    /// Leaves AMV, salability, stock, and production. The morning reset uses
    /// this so yesterday's rot does not feed the next night.
    fn clear_day(&mut self) {
        self.consumption = 0.0;
        self.traded = 0.0;
        self.paid = 0.0;
        self.print_holding = 0.0;
        self.print_units = 0.0;
        self.sought_unmet = 0.0;
        self.offered_unsold = 0.0;
        self.decayed = 0.0;
        self.volume = 0.0;
        self.process_used = 0.0;
        self.process_value = 0.0;
        self.desire_used = 0.0;
        self.clear_units = 0.0;
        self.clear_value = 0.0;
    }
}

/// Anger weighs this much beside fear on the way out.
const MIGRATION_ANGER_WEIGHT: f64 = 0.5;
/// Share of a saturated push or pull that moves in one year.
///
/// A one-point living gap saturates at one half, so this pace is a
/// twentieth of the households before [`migration_size_scale`]. The
/// saturated fraction stays under this share.
const MIGRATION_PACE: f64 = 0.1;
/// A different craft closes above this multiple of the origin's wealth.
const MIGRATION_WEALTH_CLOSE: f64 = 1.10;
/// Households one founding moves.
const FOUNDING_HOUSEHOLDS: f64 = 5.0;
/// Households a pop needs before those five may leave.
///
/// Five is a fifth of this count.
const FOUNDING_MIN_HOUSEHOLDS: f64 = 25.0;
/// Share of foundings that apply both an add and a drop.
const FOUNDING_BOTH_SHARE: f64 = 0.2;
/// Half again a drop's base weight, when its product's offer already covered the request.
const FOUNDING_AVAILABLE_DROP: f64 = 1.5;
/// Species, culture, stratum, and religion.
const DEMO_AXES: usize = 4;

/// One pop's inputs to [`Market::internal_migration`].
struct Mover {
    id: usize,
    count: f64,
    people: f64,
    sol: f64,
    wealth: f64,
    profit: f64,
    fear: f64,
    anger: f64,
    hope: f64,
    craft: usize,
    species: usize,
    culture: usize,
    stratum: usize,
    religion: usize,
    offer: f64,
    room: f64,
    evacuate: bool,
}

/// # Push Score
///
/// Squared living gap, wealth shortfall, and profit gap, scaled by fear and anger.
///
/// `sol_gap` is points of living below the market. `wealth_short` is the
/// relative wealth shortfall. `profit_gap` is points of profit below the
/// market, on the same scale as living. Fear adds its full share. Anger adds
/// [`MIGRATION_ANGER_WEIGHT`] of its share. A zero level stays zero.
fn push_score(sol_gap: f64, wealth_short: f64, profit_gap: f64, fear: f64, anger: f64) -> f64 {
    let level = sol_gap.max(0.0).powi(2)
        + wealth_short.max(0.0).powi(2)
        + profit_gap.max(0.0).powi(2);
    level * (1.0 + fear + MIGRATION_ANGER_WEIGHT * anger)
}

/// # Pull Score
///
/// Squared living surplus plus squared profit surplus, scaled by hope.
///
/// `sol_surplus` is points of living above the market. `profit_surplus` is
/// points of profit above it. Hope adds its full share.
fn pull_score(sol_surplus: f64, profit_surplus: f64, hope: f64) -> f64 {
    (sol_surplus.max(0.0).powi(2) + profit_surplus.max(0.0).powi(2)) * (1.0 + hope)
}

/// # Migration Size Scale
///
/// How many copies of the paced share `people` adds to an offer or a room.
///
/// Two people are one copy. Each doubling adds a copy. A count at or
/// below one, or a count that is not finite, adds nothing from this scale.
/// Returns `log2` of the count that remains.
fn migration_size_scale(people: f64) -> f64 {
    let people = if people.is_finite() && people > 1.0 {
        people
    } else {
        1.0
    };
    people.log2()
}

/// # Keeps A Sought Shop
///
/// Whether `pop` offers no paced migrants while a paying output is still sought.
///
/// `factuals` supplies each process. `history` prices the score and the
/// bid. A pop of at least [`FOUNDING_MIN_HOUSEHOLDS`] returns false. A
/// running line that pays, scores above zero, and has a
/// [`sought_sale_runs`] count above zero returns true.
/// Returns that answer.
fn keeps_a_sought_shop(pop: &Pop, factuals: &Factuals, history: &MarketHistory) -> bool {
    if pop.demographics.household.count >= FOUNDING_MIN_HOUSEHOLDS {
        return false;
    }
    for line in &pop.job.lines {
        let running = match line.target {
            None => true,
            Some(target) => target.is_finite() && target > 0.0,
        };
        if !running || !line.pays {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let score = trade_score(process, history);
        if !score.is_finite() || score <= 0.0 {
            continue;
        }
        if sought_sale_runs(process, factuals, history) > 0.0 {
            return true;
        }
    }
    false
}

/// # Running Profit
///
/// Best finite trade score among `pop`'s running lines.
///
/// A line runs when its target is `None`, or when the target is finite and
/// above zero. `factuals` supplies the process. A missing process is skipped.
/// `history` prices the score. No running scored line returns 0. A negative
/// best score is kept.
fn running_profit(pop: &Pop, factuals: &Factuals, history: &MarketHistory) -> f64 {
    let mut best: Option<f64> = None;
    for line in &pop.job.lines {
        let running = match line.target {
            None => true,
            Some(target) => target.is_finite() && target > 0.0,
        };
        if !running {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let score = trade_score(process, history);
        if !score.is_finite() {
            continue;
        }
        best = Some(best.map_or(score, |old| old.max(score)));
    }
    best.unwrap_or(0.0)
}

/// # Mean Profit
///
/// Unweighted average of each pop's profit. An empty list returns 0.
///
/// Each pop counts once. A person-weighted mean would sit on the largest pop.
fn mean_profit(rows: &[Mover]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    rows.iter().map(|row| row.profit).sum::<f64>() / rows.len() as f64
}

/// # Migration Fraction
///
/// [`MIGRATION_PACE`] times `score / (1 + score)`.
///
/// The result is 0 when `score` is not a positive finite number. The
/// saturated share is at most 1, so one year stays under the pace.
fn migration_fraction(score: f64) -> f64 {
    if !score.is_finite() || score <= 0.0 {
        0.0
    } else {
        MIGRATION_PACE * score / (1.0 + score)
    }
}

/// # Sol Gap
///
/// Points of `sol` below `avg`. A non-finite input is 0.
///
/// A profit score uses this same gap. One point is one point at either scale.
fn sol_gap(sol: f64, avg: f64) -> f64 {
    if !sol.is_finite() || !avg.is_finite() {
        0.0
    } else {
        (avg - sol).max(0.0)
    }
}

/// # Sol Surplus
///
/// Points of `sol` above `avg`. A non-finite input is 0.
///
/// A profit score uses this same surplus.
fn sol_surplus(sol: f64, avg: f64) -> f64 {
    if !sol.is_finite() || !avg.is_finite() {
        0.0
    } else {
        (sol - avg).max(0.0)
    }
}

/// # Wealth Shortfall
///
/// Relative shortfall of `wealth` under `avg`. Zero at or above the average,
/// or when the average is 0.
fn wealth_shortfall(wealth: f64, avg: f64) -> f64 {
    if !wealth.is_finite() || !avg.is_finite() || avg == 0.0 || wealth >= avg {
        return 0.0;
    }
    (avg - wealth) / avg.abs()
}

/// # Weighted Sol
///
/// Household-weighted living average. Non-finite rows are skipped.
/// No finite row returns NaN.
fn weighted_sol(rows: &[Mover]) -> f64 {
    let mut weight = 0.0;
    let mut sum = 0.0;
    for row in rows {
        if row.sol.is_finite() {
            weight += row.count;
            sum += row.count * row.sol;
        }
    }
    if weight == 0.0 {
        f64::NAN
    } else {
        sum / weight
    }
}

/// # Weighted Wealth
///
/// Person-weighted wealth average over `rows`.
///
/// An empty weight returns 0. The result is total holding over total people.
fn weighted_wealth(rows: &[Mover]) -> f64 {
    let mut weight = 0.0;
    let mut sum = 0.0;
    for row in rows {
        if row.people > 0.0 && row.wealth.is_finite() {
            weight += row.people;
            sum += row.people * row.wealth;
        }
    }
    if weight == 0.0 { 0.0 } else { sum / weight }
}

/// # Is Leaving
///
/// Whether sending `out` households would leave `count` under one.
///
/// A zero outflow is not leaving. Any positive outflow that leaves fewer
/// than one household is.
fn is_leaving(count: f64, out: f64) -> bool {
    out > 0.0 && count - out < 1.0
}

/// # Outflow
///
/// Households leaving `origin` on `flow`.
fn outflow(flow: &[Vec<f64>], origin: usize) -> f64 {
    flow[origin].iter().sum()
}

/// # Inflow
///
/// Households arriving at `dest` on `flow`.
fn inflow(flow: &[Vec<f64>], dest: usize) -> f64 {
    flow.iter().map(|row| row[dest]).sum()
}

/// # Pair Weights
///
/// Room at the destination times [`migration_gate`] from the origin.
///
/// The diagonal is 0. A closed gate or an empty room is 0. [`seat_offers`]
/// splits an offer across one row of these weights.
fn pair_weights(rows: &[Mover], factuals: &Factuals) -> Vec<Vec<f64>> {
    let n = rows.len();
    let mut weight = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            weight[i][j] = rows[j].room * migration_gate(&rows[i], &rows[j], factuals);
        }
    }
    weight
}

/// # Is Evacuating
///
/// Whether `pop` has no running line and no whole currency coin.
///
/// `currencies` are the market's tender slots. A line runs when its target
/// is `None`, or when the target is finite and above zero. A whole coin is
/// `floor` of at least one unit in working stock or in saved currency.
/// Dust does not count. An empty job with no such coin returns true.
fn is_evacuating(pop: &Pop, currencies: &[Option<usize>; CURRENCY_SLOTS]) -> bool {
    let running = pop.job.lines.iter().any(|line| match line.target {
        None => true,
        Some(target) => target.is_finite() && target > 0.0,
    });
    if running {
        return false;
    }
    for id in currencies.iter().flatten() {
        let working = pop.working_units(*id).floor();
        let saved = pop.saved_currency_units(*id).floor();
        if working >= 1.0 || saved >= 1.0 {
            return false;
        }
    }
    true
}

/// # Evacuation Gate
///
/// How open a rapid leave from `from` to `to` is. `0` is closed.
///
/// A different species returns 0. Otherwise the factor is
/// [`assimilation_cap`]. Wealth and room are not read.
fn evacuation_gate(from: &Mover, to: &Mover, factuals: &Factuals) -> f64 {
    if from.species != to.species {
        return 0.0;
    }
    assimilation_cap(from, to, factuals)
}

/// # Seat Evacuation
///
/// Replaces the paced flow of each evacuating origin that has a seat.
///
/// `rows` are the movers. `flow` is the seating from [`seat_offers`].
/// `factuals` supplies assimilation. An evacuating origin sends its whole
/// household count when some other pop is not evacuating and
/// [`evacuation_gate`] is above zero. The count splits in proportion to
/// those gates, so a partial gap does not shrink the total. Pace, room,
/// and wealth are not applied. An origin with no such destination keeps
/// the paced flow.
fn seat_evacuation(rows: &[Mover], flow: &mut [Vec<f64>], factuals: &Factuals) {
    let n = rows.len();
    for i in 0..n {
        if !rows[i].evacuate {
            continue;
        }
        let mut gates = vec![0.0; n];
        let mut sum = 0.0;
        for j in 0..n {
            if i == j || rows[j].evacuate {
                continue;
            }
            let gate = evacuation_gate(&rows[i], &rows[j], factuals);
            if gate > 0.0 {
                gates[j] = gate;
                sum += gate;
            }
        }
        if sum <= 0.0 {
            continue;
        }
        // The whole count leaves. The paced seats on this row are dropped.
        for j in 0..n {
            flow[i][j] = if gates[j] > 0.0 {
                rows[i].count * gates[j] / sum
            } else {
                0.0
            };
        }
    }
}

/// # Seat Offers
///
/// Splits each offer across its open weights, then caps each destination at its room.
///
/// A pair is also capped by its own weight. An offer with no open weight stays
/// put. The room scale does not run when nothing arrived.
fn seat_offers(rows: &[Mover], weight: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = rows.len();
    let mut flow = vec![vec![0.0; n]; n];
    for i in 0..n {
        let open: f64 = weight[i].iter().sum();
        if open <= 0.0 || rows[i].offer <= 0.0 {
            continue;
        }
        for j in 0..n {
            if weight[i][j] <= 0.0 {
                continue;
            }
            let share = rows[i].offer * weight[i][j] / open;
            flow[i][j] = share.min(weight[i][j]);
        }
    }
    for j in 0..n {
        let arrived = inflow(&flow, j);
        if arrived > rows[j].room && arrived > 0.0 {
            let scale = rows[j].room / arrived;
            for row in &mut flow {
                row[j] *= scale;
            }
        }
    }
    flow
}

/// # Clear Arrivals At Leavers
///
/// Zeros every flow into a pop [`is_leaving`] reports.
///
/// One pass is enough. Clearing those arrivals only shrinks other outflows,
/// so a pop that was staying cannot start leaving afterward.
fn clear_arrivals_at_leavers(rows: &[Mover], flow: &mut [Vec<f64>]) {
    let leaving: Vec<bool> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| is_leaving(row.count, outflow(flow, i)))
        .collect();
    for row in flow.iter_mut() {
        for (j, cell) in row.iter_mut().enumerate() {
            if leaving[j] {
                *cell = 0.0;
            }
        }
    }
}

/// # Expand Leavers
///
/// Scales a leaving pop's row up to its whole household count.
///
/// Returns which rows left. Receivers are not pulled back to their room.
/// A row with no outflow is left as it is.
fn expand_leavers(rows: &[Mover], flow: &mut [Vec<f64>]) -> Vec<bool> {
    let mut leaving = vec![false; rows.len()];
    for (i, row) in rows.iter().enumerate() {
        let out = outflow(flow, i);
        if is_leaving(row.count, out) {
            leaving[i] = true;
            let scale = row.count / out;
            for cell in &mut flow[i] {
                *cell *= scale;
            }
        }
    }
    leaving
}

/// # Migration Gate
///
/// How open the move from `from` to `to` is. `0` is closed.
///
/// A different species returns 0 without reading wealth or culture.
/// Otherwise wealth and assimilation each return a factor in `0..=1`.
/// The pair is their product. A closed wealth factor returns 0 without
/// reading culture.
fn migration_gate(from: &Mover, to: &Mover, factuals: &Factuals) -> f64 {
    if from.species != to.species {
        return 0.0;
    }
    let wealth = wealth_factor(from, to);
    if wealth <= 0.0 {
        return 0.0;
    }
    wealth * assimilation_cap(from, to, factuals)
}

/// # Wealth Factor
///
/// How easily `from` may enter `to` on wealth.
///
/// A destination at or below the origin's wealth per person is `1`. A richer
/// destination is `origin / destination`. That move closes when the origin's
/// wealth is at or below 0, and when the crafts differ and the destination
/// is more than [`MIGRATION_WEALTH_CLOSE`] times as rich. Craft `0` is not
/// a shared craft.
fn wealth_factor(from: &Mover, to: &Mover) -> f64 {
    if to.wealth <= from.wealth {
        return 1.0;
    }
    if from.wealth <= 0.0 {
        return 0.0;
    }
    let same_craft = from.craft != 0 && from.craft == to.craft;
    if !same_craft && to.wealth > from.wealth * MIGRATION_WEALTH_CLOSE {
        return 0.0;
    }
    from.wealth / to.wealth
}

/// # Assimilation Cap
///
/// How much of a move the demographic gap allows.
///
/// A distance of 0 is `1`. Otherwise the ease is the mean of the two
/// cultures' assimilation and the two religions' conversion. That ease
/// times the share of the four ids that still match is the cap, clamped
/// to `0..=1`. Culture id `0` and religion id `0` each have ease `1`.
/// Stratum has no ease. A mismatch there only widens the distance.
/// [`migration_gate`] has already closed a different species.
fn assimilation_cap(from: &Mover, to: &Mover, factuals: &Factuals) -> f64 {
    let distance = demo_distance(from, to);
    if distance == 0 {
        return 1.0;
    }
    let culture = culture_ease(factuals, from.culture) + culture_ease(factuals, to.culture);
    let religion =
        religion_conversion(factuals, from.religion) + religion_conversion(factuals, to.religion);
    let ease = (culture + religion) / 2.0;
    let alike = (DEMO_AXES - distance) as f64 / DEMO_AXES as f64;
    (ease * alike).clamp(0.0, 1.0)
}

/// # Demo Distance
///
/// How many of species, culture, stratum, and religion differ.
///
/// Each mismatched id adds one. Id `0` matches id `0`. The result is `0..=4`.
fn demo_distance(from: &Mover, to: &Mover) -> usize {
    let mut distance = 0;
    if from.species != to.species {
        distance += 1;
    }
    if from.culture != to.culture {
        distance += 1;
    }
    if from.stratum != to.stratum {
        distance += 1;
    }
    if from.religion != to.religion {
        distance += 1;
    }
    distance
}

/// # Culture Ease
///
/// Assimilation ease for this culture. Id `0` is open and is not looked up.
///
/// Any other id is that culture's assimilation, clamped to `0..=1`.
fn culture_ease(factuals: &Factuals, id: usize) -> f64 {
    if id == 0 {
        return 1.0;
    }
    factuals.find_culture(id).assimilation.clamp(0.0, 1.0)
}

/// # Religion Conversion
///
/// Conversion ease for this religion. Id `0` is open and is not looked up.
///
/// Any other id is that religion's conversion, clamped to `0..=1`.
fn religion_conversion(factuals: &Factuals, id: usize) -> f64 {
    if id == 0 {
        return 1.0;
    }
    factuals.find_religion(id).conversion.clamp(0.0, 1.0)
}

/// # Split Stock
///
/// Splits `source` by `shares`. The last share is whatever is left.
///
/// Earlier shares are that fraction of each good. A good that comes out at 0
/// is left out. An empty `shares` returns nothing.
fn split_stock(source: &HashMap<usize, f64>, shares: &[f64]) -> Vec<HashMap<usize, f64>> {
    let mut sent = HashMap::new();
    let mut parts = Vec::with_capacity(shares.len());
    for (k, &share) in shares.iter().enumerate() {
        if k + 1 == shares.len() {
            parts.push(remainder_map(source, &sent));
        } else {
            let part = slice_map(source, share);
            add_map(&mut sent, &part);
            parts.push(part);
        }
    }
    parts
}

/// # Slice Map
///
/// `fraction` of each entry in `source`.
///
/// A part of 0 is left out.
fn slice_map(source: &HashMap<usize, f64>, fraction: f64) -> HashMap<usize, f64> {
    source
        .iter()
        .filter_map(|(&good, &qty)| {
            let part = qty * fraction;
            if part == 0.0 {
                None
            } else {
                Some((good, part))
            }
        })
        .collect()
}

/// One side of a mass-market quote. `units` are whole. `limit` is the price.
struct MassQuote {
    origin: Actor,
    units: f64,
    limit: f64,
}

/// # Mass Quotes
///
/// Splits `orders` for `good` into buys and sells.
///
/// `amv` replaces a zero accept price. A quote for another good, a
/// non-finite amount, a zero limit, or less than one whole unit is dropped.
/// A negative limit stays: the seller pays to be rid of the good, or the
/// buyer must be paid to take it. The first order from an actor is kept.
/// Returns `(buys, sells)`.
fn mass_quotes(orders: &[MarketOrder], good: usize, amv: f64) -> (Vec<MassQuote>, Vec<MassQuote>) {
    let mut seen = HashSet::new();
    let mut buys = Vec::new();
    let mut sells = Vec::new();
    for order in orders {
        if order.target != good || !seen.insert(order.origin) {
            continue;
        }
        if !order.target_amount.is_finite() || order.target_amount == 0.0 {
            continue;
        }
        let units = order.target_amount.abs().floor();
        if units < 1.0 {
            continue;
        }
        let Some(limit) = order_limit(order, amv) else {
            continue;
        };
        let quote = MassQuote {
            origin: order.origin,
            units,
            limit,
        };
        if order.target_amount > 0.0 {
            buys.push(quote);
        } else {
            sells.push(quote);
        }
    }
    (buys, sells)
}

/// # Order Limit
///
/// Raw AMV per unit `order` will trade at.
///
/// A non-zero [`MarketOrder::accept_price`] is that price. Otherwise `amv`
/// is the card. A missing, zero, or non-finite price is `None`. A negative
/// price is kept.
fn order_limit(order: &MarketOrder, amv: f64) -> Option<f64> {
    let price = if order.accept_price != 0.0 && order.accept_price.is_finite() {
        order.accept_price
    } else {
        amv
    };
    if price.is_finite() && price != 0.0 {
        Some(price)
    } else {
        None
    }
}

/// # Buy Limits
///
/// `(units, max price)` for each buy quote.
fn buy_limits(quotes: &[MassQuote]) -> Vec<(f64, f64)> {
    quotes
        .iter()
        .map(|quote| (quote.units, quote.limit))
        .collect()
}

/// # Sell Limits
///
/// `(units, min price)` for each sell quote.
fn sell_limits(quotes: &[MassQuote]) -> Vec<(f64, f64)> {
    quotes
        .iter()
        .map(|quote| (quote.units, quote.limit))
        .collect()
}

/// # Book At
///
/// Demand and supply willing to trade at `price`.
///
/// A buy counts when its max is at least `price`. A sell counts when its
/// min is at most `price`. Returns `(demand, supply)`.
fn book_at(buys: &[(f64, f64)], sells: &[(f64, f64)], price: f64) -> (f64, f64) {
    let demand = buys
        .iter()
        .filter(|(_, limit)| *limit >= price)
        .map(|(units, _)| *units)
        .sum();
    let supply = sells
        .iter()
        .filter(|(_, limit)| *limit <= price)
        .map(|(units, _)| *units)
        .sum();
    (demand, supply)
}

/// # Clearing Price
///
/// Uniform price where supply meets demand, starting from `amv`.
///
/// Buys are `(units, max price)`. Sells are `(units, min price)`. An exact
/// match at `amv` is that price. Otherwise the order limit closest to `amv`
/// that matches is the price. When no limit matches, adjacent limits whose
/// excess demand changes sign are interpolated, and only when the lower
/// price still has a seller and the higher price still has a buyer, so the
/// price between them has both sides. A span that is empty on one side is
/// skipped. The span closest to `amv` wins. Otherwise the limit closest to
/// `amv` at which both sides are positive is the price. A negative limit
/// is a real price. Returns `None` when a side is empty or no non-zero
/// price has both sides.
fn clearing_price(buys: &[(f64, f64)], sells: &[(f64, f64)], amv: f64) -> Option<f64> {
    if buys.is_empty() || sells.is_empty() {
        return None;
    }
    // Yesterday's AMV is the working price. An exact book there stops the search.
    if amv.is_finite() && amv != 0.0 {
        let (demand, supply) = book_at(buys, sells, amv);
        if demand > 0.0 && demand == supply {
            return Some(amv);
        }
    }
    let mut prices = Vec::new();
    if amv.is_finite() && amv != 0.0 {
        prices.push(amv);
    }
    for (_, limit) in buys.iter().chain(sells.iter()) {
        if limit.is_finite() && *limit != 0.0 {
            prices.push(*limit);
        }
    }
    prices.sort_by(|left, right| left.total_cmp(right));
    prices.dedup_by(|later, earlier| (*later - *earlier).abs() <= 1e-9);
    if prices.is_empty() {
        return None;
    }
    let mut books = Vec::new();
    let mut exact: Option<f64> = None;
    for price in &prices {
        let (demand, supply) = book_at(buys, sells, *price);
        books.push((demand, supply));
        if demand > 0.0 && demand == supply {
            exact = Some(nearer(exact, *price, amv));
        }
    }
    if let Some(price) = exact {
        return Some(price);
    }
    // No quoted price balances. Interpolate the two that bracket the cross.
    let mut crossed: Option<(f64, f64)> = None;
    for index in 0..prices.len().saturating_sub(1) {
        let excess_low = books[index].0 - books[index].1;
        let excess_high = books[index + 1].0 - books[index + 1].1;
        let changes =
            (excess_low > 0.0 && excess_high < 0.0) || (excess_low < 0.0 && excess_high > 0.0);
        // Sellers at the lower price and buyers at the higher price stay
        // present through the span. The empty gap under the first ask is not.
        let interior = books[index].1 > 0.0 && books[index + 1].0 > 0.0;
        if !changes || !interior {
            continue;
        }
        let share = excess_low / (excess_low - excess_high);
        let price = crate::game::util::lerp(prices[index], prices[index + 1], share);
        if !price.is_finite() {
            continue;
        }
        let span = span_gap(amv, prices[index], prices[index + 1]);
        let rank = span + (price - amv).abs() * 1e-9;
        if crossed.map(|(old, _)| rank < old).unwrap_or(true) {
            crossed = Some((rank, price));
        }
    }
    if let Some((_, price)) = crossed {
        return Some(price);
    }
    let mut fallback: Option<f64> = None;
    for (index, price) in prices.iter().enumerate() {
        let (demand, supply) = books[index];
        if demand > 0.0 && supply > 0.0 {
            fallback = Some(nearer(fallback, *price, amv));
        }
    }
    fallback
}

/// # Nearer
///
/// `next` when it sits closer to `amv` than `current` does.
///
/// A missing `current` is `next`. An equal distance keeps `current`.
fn nearer(current: Option<f64>, next: f64, amv: f64) -> f64 {
    match current {
        None => next,
        Some(old) if (next - amv).abs() < (old - amv).abs() => next,
        Some(old) => old,
    }
}

/// # Span Gap
///
/// How far `point` sits from the closed span `low..=high`.
///
/// A point inside the span is `0`.
fn span_gap(point: f64, low: f64, high: f64) -> f64 {
    if point < low {
        low - point
    } else if point > high {
        point - high
    } else {
        0.0
    }
}

/// # Ration
///
/// Whole units of `volume` dealt across `orders`.
///
/// `orders` are `(actor, units)`. `rng` shuffles them first, so a short
/// volume does not always fill the earliest actor. Each take is at most
/// that actor's units. Returns the actors who received at least one unit.
fn ration(orders: &[(Actor, f64)], volume: f64, rng: &mut impl rand::RngCore) -> Vec<(Actor, f64)> {
    let mut rows = orders.to_vec();
    shuffle(&mut rows, rng);
    let mut left = volume.floor();
    let mut filled = Vec::new();
    for (actor, units) in rows {
        if left < 1.0 {
            break;
        }
        let take = units.min(left).floor();
        if take >= 1.0 {
            filled.push((actor, take));
            left -= take;
        }
    }
    filled
}

/// # Shuffle
///
/// Reorders `items` from `rng`.
///
/// An empty list or a single entry is unchanged. Nothing is returned.
fn shuffle<T>(items: &mut [T], rng: &mut impl rand::RngCore) {
    let mut index = items.len();
    while index > 1 {
        index -= 1;
        let pick = crate::game::util::random_index(rng, index + 1);
        items.swap(index, pick);
    }
}

/// # Make Change
///
/// Whole units of the good and of the tender whose values sit closest.
///
/// `units` is the good quantity wanted. `price` is AMV per unit of that
/// good. A negative price uses its magnitude: the caller passes the
/// payer's free tender, which is the seller when the price is negative.
/// `currency_amv` is AMV per unit of the tender. `currency_free` is the
/// tender that payer can spend. The tender paid is the whole amount
/// nearest the good's value. A tie keeps the smaller payment. A larger
/// quantity never becomes payable once a smaller one is not, so the search
/// halves the range. Returns `(good units, tender units)`, or `(0, 0)` when
/// no whole payment fits.
pub(crate) fn make_change(
    units: f64,
    price: f64,
    currency_amv: f64,
    currency_free: f64,
) -> (f64, f64) {
    if !price.is_finite() || price == 0.0 || !currency_amv.is_finite() || currency_amv <= 0.0 {
        return (0.0, 0.0);
    }
    let price = price.abs();
    let mut low = 1.0;
    let mut high = units.floor();
    if !high.is_finite() || high < 1.0 {
        return (0.0, 0.0);
    }
    let mut best = (0.0, 0.0);
    while low <= high {
        let mid = ((low + high) / 2.0).floor();
        if mid < 1.0 {
            break;
        }
        let pay = tender_for(mid * price, currency_amv, currency_free);
        if pay >= 1.0 {
            best = (mid, pay);
            low = mid + 1.0;
        } else if mid <= 1.0 {
            break;
        } else {
            high = mid - 1.0;
        }
    }
    best
}

/// # Tender For
///
/// Whole tender units that pay `value`.
///
/// `currency_amv` prices one tender unit. `currency_free` is what the payer
/// can spend. The pay is the whole unit at or beside `value / currency_amv`,
/// and a tie keeps the smaller one. A pay under 1 does not count. Returns
/// that pay, or 0 when none fits.
fn tender_for(value: f64, currency_amv: f64, currency_free: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 || !currency_amv.is_finite() || currency_amv <= 0.0 {
        return 0.0;
    }
    let ideal = value / currency_amv;
    if !ideal.is_finite() || ideal <= 0.0 {
        return 0.0;
    }
    let affordable = currency_free.floor();
    let mut best: Option<(f64, f64)> = None;
    for pay in [ideal.floor(), ideal.ceil()] {
        if pay < 1.0 || pay > affordable {
            continue;
        }
        let gap = (pay * currency_amv - value).abs();
        let closer = best.map(|(_, old)| gap < old).unwrap_or(true);
        if closer {
            best = Some((pay, gap));
        }
    }
    best.map(|(pay, _)| pay).unwrap_or(0.0)
}

/// # Nearest Count
///
/// Whole units nearest `ideal`, and not above `cap`.
///
/// A tie keeps the smaller count. A non-finite ideal, a non-positive
/// ideal, or a cap under one unit is 0. Returns that count.
fn nearest_count(ideal: f64, cap: f64) -> f64 {
    if !cap.is_finite() {
        return 0.0;
    }
    let cap = cap.floor().max(0.0);
    if cap < 1.0 || !ideal.is_finite() || ideal <= 0.0 {
        return 0.0;
    }
    let low = ideal.floor().min(cap);
    let high = ideal.ceil().min(cap);
    if (high - ideal).abs() < (low - ideal).abs() {
        high
    } else {
        low
    }
}

/// # Count Cap
///
/// Most whole units of one coin [`change_counts`] may use.
///
/// `amv` prices the coin. `free` is what the purse holds. `value` is the
/// payment being made. `budget` limits the count to `free`. An open count
/// may pass `value` by one unit. A non-positive AMV caps at 0. Returns
/// that cap.
fn count_cap(amv: f64, free: f64, value: f64, budget: bool) -> f64 {
    if !amv.is_finite() || amv <= 0.0 {
        return 0.0;
    }
    if budget {
        free.floor().max(0.0)
    } else if !value.is_finite() || value <= 0.0 {
        0.0
    } else {
        (value / amv).ceil() + 1.0
    }
}

/// # Change Counts
///
/// Whole units of each coin that pay `value`, one count per coin.
///
/// `coins` are `(id, AMV, free units)` and are already usable. `budget`
/// keeps each count inside its free units. An open count may pass
/// `value`. One coin is [`tender_for`]. Two or more give every coin but
/// the last its share of `value`, in proportion to that coin's part of
/// the purse, as [`nearest_count`]. The last coin takes the whole count
/// nearest what those shares left unpaid.
/// Returns the gap from `value` and those counts, or nothing when the
/// counts sum to under one unit.
fn change_counts(value: f64, coins: &[(usize, f64, f64)], budget: bool) -> Option<(f64, Vec<f64>)> {
    if coins.is_empty() {
        return None;
    }
    let mut purse: f64 = 0.0;
    for coin in coins {
        purse += coin.2.floor().max(0.0) * coin.1;
    }
    let mut counts = vec![0.0; coins.len()];
    if coins.len() == 1 {
        let coin = coins[0];
        let free = if budget {
            coin.2
        } else {
            count_cap(coin.1, coin.2, value, false)
        };
        counts[0] = tender_for(value, coin.1, free);
    } else {
        let mut spent: f64 = 0.0;
        for index in 0..coins.len() - 1 {
            let coin = coins[index];
            let held = coin.2.floor().max(0.0);
            let share = if purse > 0.0 {
                value * (held * coin.1) / purse
            } else {
                0.0
            };
            let count = nearest_count(share / coin.1, count_cap(coin.1, coin.2, value, budget));
            counts[index] = count;
            spent += count * coin.1;
        }
        let last = coins[coins.len() - 1];
        let ideal = (value - spent) / last.1;
        counts[coins.len() - 1] = nearest_count(ideal, count_cap(last.1, last.2, value, budget));
    }
    let mut paid: f64 = 0.0;
    let mut units: f64 = 0.0;
    for (index, coin) in coins.iter().enumerate() {
        paid += counts[index] * coin.1;
        units += counts[index];
    }
    if units < 1.0 || !paid.is_finite() {
        None
    } else {
        Some(((paid - value).abs(), counts))
    }
}

/// # Mix For
///
/// Whole units of each coin that pay `value`.
///
/// `coins` are `(id, AMV, free units)`. A coin with a non-positive AMV or
/// under one free unit is ignored. The mix is [`change_counts`]. A mix the
/// purse cannot bring as close as an open purse is refused. At least one
/// unit is required. Returns those units by id, or nothing when no mix fits.
fn mix_for(value: f64, coins: &[(usize, f64, f64)]) -> Option<Vec<(usize, f64)>> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let usable: Vec<(usize, f64, f64)> = coins
        .iter()
        .copied()
        .filter(|coin| coin.1.is_finite() && coin.1 > 0.0 && coin.2.floor() >= 1.0)
        .collect();
    if usable.is_empty() {
        return None;
    }
    let (open_gap, _) = change_counts(value, &usable, false)?;
    let (gap, paid) = change_counts(value, &usable, true)?;
    if gap > open_gap + 1e-6 {
        return None;
    }
    let mut rows = Vec::new();
    for (index, coin) in usable.iter().enumerate() {
        let count = paid[index];
        if count >= 1.0 {
            rows.push((coin.0, count));
        }
    }
    if rows.is_empty() { None } else { Some(rows) }
}

/// # Mix Change
///
/// Whole units of the good and the tender mix that pays for them.
///
/// `units` is the quantity wanted. `price` is AMV per unit of that good.
/// A negative price uses its magnitude. `coins` are the tenders the payer
/// can spend, as `(id, AMV, free units)`. One coin is [`make_change`].
/// Two or more are priced by [`mix_for`]. The full quantity is tried
/// once. When that change does not fit, the quantity is scaled down by
/// halving the range, and a larger quantity is not tried once a smaller
/// one does not fit. Returns `(good units, mix)`, or `(0, empty)` when
/// nothing fits.
pub(crate) fn mix_change(
    units: f64,
    price: f64,
    coins: &[(usize, f64, f64)],
) -> (f64, Vec<(usize, f64)>) {
    if !price.is_finite() || price == 0.0 {
        return (0.0, Vec::new());
    }
    let usable: Vec<(usize, f64, f64)> = coins
        .iter()
        .copied()
        .filter(|coin| coin.1.is_finite() && coin.1 > 0.0 && coin.2.floor() >= 1.0)
        .collect();
    if usable.len() == 1 {
        let (got, pay) = make_change(units, price, usable[0].1, usable[0].2);
        if got >= 1.0 && pay >= 1.0 {
            return (got, vec![(usable[0].0, pay)]);
        }
        return (0.0, Vec::new());
    }
    let price = price.abs();
    let mut high = units.floor();
    if !high.is_finite() || high < 1.0 {
        return (0.0, Vec::new());
    }
    // The purse covers the ask. No smaller quantity is required.
    if let Some(mix) = mix_for(high * price, coins) {
        return (high, mix);
    }
    let mut low = 1.0;
    high -= 1.0;
    let mut best = (0.0, Vec::new());
    while low <= high {
        let mid = ((low + high) / 2.0).floor();
        if mid < 1.0 {
            break;
        }
        if let Some(mix) = mix_for(mid * price, coins) {
            best = (mid, mix);
            low = mid + 1.0;
        } else if mid <= 1.0 {
            break;
        } else {
            high = mid - 1.0;
        }
    }
    best
}

/// # Pay Quantity
///
/// Whole tender units in `pay`.
///
/// Each entry is `(id, units)`. Returns the sum.
fn pay_quantity(pay: &[(usize, f64)]) -> f64 {
    pay.iter().map(|(_, qty)| *qty).sum()
}

/// # Priced Coins
///
/// Tenders that can pay, as `(id, AMV, free units)`.
///
/// `tenders` are the payment slots. `history` prices each one. `free_of`
/// is the units the payer can spend. A non-positive AMV, or a holding
/// under one unit, is left out.
pub(crate) fn priced_coins(
    tenders: &[usize],
    history: &MarketHistory,
    mut free_of: impl FnMut(usize) -> f64,
) -> Vec<(usize, f64, f64)> {
    let mut coins = Vec::new();
    for &id in tenders {
        let amv = history.price(id);
        if !amv.is_finite() || amv <= 0.0 {
            continue;
        }
        let free = free_of(id);
        if free >= 1.0 {
            coins.push((id, amv, free));
        }
    }
    coins
}

/// # Holds Tender
///
/// True when `actor` has at least one free unit of a good in `tenders`.
///
/// `actors` holds the stock. A tender under one free unit does not count.
fn holds_tender(actors: &Actors, actor: Actor, tenders: &[usize]) -> bool {
    tenders
        .iter()
        .any(|id| actors.get(actor).free_units(*id).floor() >= 1.0)
}

/// # Mass Freight Half
///
/// One side's share of the freight bill for a mass fill.
///
/// `moved` units of `good` and `pay` are what changes hands. `pay` is
/// `(tender, units)`. `friction` is the market's bulk multiplier. The bill
/// is [`freight_bill`], which is 0 when the world has no transport good.
/// Each side pays half of that bill. Returns that half.
fn mass_freight_half(
    factuals: &Factuals,
    friction: f64,
    good: usize,
    moved: f64,
    pay: &[(usize, f64)],
) -> f64 {
    let mut basket = HashMap::from([(good, moved)]);
    for &(id, qty) in pay {
        if qty > 0.0 && id != good {
            *basket.entry(id).or_insert(0.0) += qty;
        }
    }
    freight_bill(factuals, friction, &basket) / 2.0
}

/// # Coins Left
///
/// `coins` after `spent`, without `skip`.
///
/// `coins` are `(id, AMV, free units)`. `spent` is units already promised.
/// A coin left under one unit is dropped. Returns the rows still spendable.
fn coins_left(
    coins: &[(usize, f64, f64)],
    spent: &[(usize, f64)],
    skip: Option<usize>,
) -> Vec<(usize, f64, f64)> {
    let mut left = Vec::new();
    for &(id, amv, free) in coins {
        if Some(id) == skip || !amv.is_finite() || amv <= 0.0 {
            continue;
        }
        let used = spent
            .iter()
            .find(|(spent_id, _)| *spent_id == id)
            .map(|(_, qty)| *qty)
            .unwrap_or(0.0);
        let remain = (free - used).floor();
        if remain >= 1.0 {
            left.push((id, amv, remain));
        }
    }
    left
}

/// # Transport Spare
///
/// Transport cover `actor` still has after `leaving` units are given away.
///
/// `factuals` names goods. Cover is free units times transport efficiency.
/// `leaving` is `(good, units)` this actor is about to give, and those
/// units are not cover. Goods they are about to receive are not added.
/// Returns that cover, floored at zero.
fn transport_spare(actor: &dyn DealMaker, factuals: &Factuals, leaving: &[(usize, f64)]) -> f64 {
    let mut cover = 0.0;
    for (&id, good) in &factuals.goods {
        cover += actor.free_units(id) * good.transport_efficiency();
    }
    for &(id, qty) in leaving {
        if qty <= 0.0 {
            continue;
        }
        let efficiency = factuals
            .get_good(id)
            .map(Good::transport_efficiency)
            .unwrap_or(0.0);
        let free = actor.free_units(id);
        cover -= qty.min(free) * efficiency;
    }
    cover.max(0.0)
}

/// One mass fill that fits both purses and the freight split.
struct MassFit {
    /// Whole units of the good.
    moved: f64,
    /// Tender units that pay for `moved`, by id. The haul is not included.
    pay: Vec<(usize, f64)>,
    /// Transport units advanced so the short side can pay their half.
    haul: f64,
    /// Tender units that pay for `haul`, by id.
    haul_pay: Vec<(usize, f64)>,
    /// The seller hands `haul` to the buyer. False means the buyer hands it over.
    seller_advances: bool,
}

/// # Empty Mass Fit
///
/// A fill that moved nothing.
fn empty_fit() -> MassFit {
    MassFit {
        moved: 0.0,
        pay: Vec::new(),
        haul: 0.0,
        haul_pay: Vec::new(),
        seller_advances: true,
    }
}

/// # Haul Good
///
/// The transport good a mass fill buys when a buyer is short of their half.
///
/// The lowest id with a positive transport efficiency wins. A world with
/// none returns `None`. Returns that id and its efficiency.
fn haul_good(factuals: &Factuals) -> Option<(usize, f64)> {
    let mut ids: Vec<usize> = factuals.goods.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let efficiency = factuals
            .get_good(id)
            .map(Good::transport_efficiency)
            .unwrap_or(0.0);
        if efficiency > 0.0 {
            return Some((id, efficiency));
        }
    }
    None
}

/// # Mass Units That Fit
///
/// Whole units of `good` this pair can exchange, and the tender mix that pays.
///
/// `units` is the overlap still open. `price` is the clearing AMV.
/// `history` prices `tenders` and carries the market friction. [`mix_change`]
/// picks the payment for the good. Each side owes [`mass_freight_half`].
/// A side short of that half, on a positive price, buys whole transport
/// units from the other. A transport card that is not a positive finite
/// price is priced at [`AMV_EPSILON`], so a crashed time AMV does not veto
/// the fill. The giver must still cover their own half after handing those
/// units over, and the receiver must have tender left for them. The flat
/// fee does not shrink with the quantity, so a pair that still cannot haul
/// fits no fill. A larger quantity never hauls once a smaller one does
/// not, so the search halves the range below the payable size. Returns
/// that fit, with zeros when nothing fits.
fn mass_units_that_fit(
    actors: &Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    buyer: Actor,
    seller: Actor,
    good: usize,
    tenders: &[usize],
    units: f64,
    price: f64,
    transport_amv: f64,
) -> MassFit {
    // A crashed transport card still buys the haul. The token price is one step off zero.
    let transport_amv = if transport_amv.is_finite() && transport_amv > 0.0 {
        transport_amv
    } else {
        AMV_EPSILON
    };
    let buyer_coins = priced_coins(tenders, history, |id| {
        actors.get(buyer).free_units(id).floor()
    });
    let seller_coins = priced_coins(tenders, history, |id| {
        actors.get(seller).free_units(id).floor()
    });
    // A negative price is a disposal fee. The seller pays it.
    let payer_coins = if price > 0.0 {
        &buyer_coins
    } else {
        &seller_coins
    };
    let (payable, _) = mix_change(units, price, payer_coins);
    if payable < 1.0 {
        return empty_fit();
    }
    let mut low = 1.0;
    let mut high = payable;
    let mut best = empty_fit();
    while low <= high {
        let mid = ((low + high) / 2.0).floor();
        if mid < 1.0 {
            break;
        }
        let (moved, pay) = mix_change(mid, price, payer_coins);
        let fit = if moved == mid && pay_quantity(&pay) >= 1.0 {
            mass_fit_of(
                actors,
                factuals,
                history.friction,
                buyer,
                seller,
                good,
                moved,
                &pay,
                price,
                transport_amv,
                &buyer_coins,
                &seller_coins,
            )
        } else {
            None
        };
        if let Some(fit) = fit {
            best = fit;
            low = mid + 1.0;
        } else if mid <= 1.0 {
            break;
        } else {
            high = mid - 1.0;
        }
    }
    best
}

/// # Mass Fit Of
///
/// The fill for exactly `moved` units paying `pay`, when that size hauls.
///
/// `pay` is the mix for `moved`. `buyer_coins` and `seller_coins` are what
/// each side can still spend. Both sides must cover
/// [`mass_freight_half`]. A buyer short of that half buys it from the
/// seller through [`buyer_haul`]. A seller short of it buys it from the
/// buyer through [`seller_haul`]. Returns that fill, or `None` when this
/// size does not haul.
fn mass_fit_of(
    actors: &Actors,
    factuals: &Factuals,
    friction: f64,
    buyer: Actor,
    seller: Actor,
    good: usize,
    moved: f64,
    pay: &[(usize, f64)],
    price: f64,
    transport_amv: f64,
    buyer_coins: &[(usize, f64, f64)],
    seller_coins: &[(usize, f64, f64)],
) -> Option<MassFit> {
    let half = mass_freight_half(factuals, friction, good, moved, pay);
    let buyer_leave: Vec<(usize, f64)> = if price > 0.0 {
        pay.to_vec()
    } else {
        Vec::new()
    };
    let mut seller_leave = vec![(good, moved)];
    if price < 0.0 {
        seller_leave.extend(pay.iter().copied());
    }
    let buyer_cover = transport_spare(actors.get(buyer), factuals, &buyer_leave);
    let seller_cover = transport_spare(actors.get(seller), factuals, &seller_leave);
    if buyer_cover >= half && seller_cover >= half {
        return Some(MassFit {
            moved,
            pay: pay.to_vec(),
            haul: 0.0,
            haul_pay: Vec::new(),
            seller_advances: true,
        });
    }
    if price > 0.0 && buyer_cover < half {
        return buyer_haul(
            actors,
            factuals,
            seller,
            good,
            moved,
            pay,
            half,
            seller_cover,
            buyer_cover,
            transport_amv,
            buyer_coins,
        );
    }
    if price > 0.0 && seller_cover < half {
        return seller_haul(
            actors,
            factuals,
            buyer,
            moved,
            pay,
            half,
            seller_cover,
            buyer_cover,
            transport_amv,
            seller_coins,
        );
    }
    None
}

/// # Buyer Haul
///
/// Transport the seller advances so the buyer can pay `half`.
///
/// `buyer_cover` is the buyer's spare. `seller_cover` is the seller's spare
/// after giving `moved` of `good`. The advance is the whole transport units
/// that close the gap, priced at `transport_amv` and paid from the tenders
/// left after `pay`. The transport good itself is not one of those tenders.
/// The seller's remaining spare has to cover `half`. Returns that fit, or
/// `None` when the seller cannot spare it or the buyer cannot pay for it.
fn buyer_haul(
    actors: &Actors,
    factuals: &Factuals,
    seller: Actor,
    good: usize,
    moved: f64,
    pay: &[(usize, f64)],
    half: f64,
    seller_cover: f64,
    buyer_cover: f64,
    transport_amv: f64,
    buyer_coins: &[(usize, f64, f64)],
) -> Option<MassFit> {
    let (transport, efficiency) = haul_good(factuals)?;
    if efficiency <= 0.0 || !transport_amv.is_finite() || transport_amv <= 0.0 {
        return None;
    }
    let gap = half - buyer_cover;
    let haul = (gap / efficiency).ceil();
    if !haul.is_finite() || haul < 1.0 {
        return None;
    }
    let selling = if good == transport { moved } else { 0.0 };
    let free = actors.get(seller).free_units(transport);
    if free < selling + haul {
        return None;
    }
    let seller_after = seller_cover - haul * efficiency;
    if seller_after < half {
        return None;
    }
    let left = coins_left(buyer_coins, pay, Some(transport));
    let haul_pay = mix_for(haul * transport_amv, &left)?;
    Some(MassFit {
        moved,
        pay: pay.to_vec(),
        haul,
        haul_pay,
        seller_advances: true,
    })
}

/// # Seller Haul
///
/// Transport the buyer advances so the seller can pay `half`.
///
/// `seller_cover` is the seller's spare after giving `moved`. `buyer_cover`
/// is the buyer's spare. The advance is the whole transport units that
/// close the seller's gap, priced at `transport_amv` and paid from the
/// tenders the seller already holds. The transport good itself is not one
/// of those tenders. The buyer's remaining spare has to cover `half`.
/// Returns that fit, or `None` when the buyer cannot spare it or the seller
/// cannot pay for it.
fn seller_haul(
    actors: &Actors,
    factuals: &Factuals,
    buyer: Actor,
    moved: f64,
    pay: &[(usize, f64)],
    half: f64,
    seller_cover: f64,
    buyer_cover: f64,
    transport_amv: f64,
    seller_coins: &[(usize, f64, f64)],
) -> Option<MassFit> {
    let (transport, efficiency) = haul_good(factuals)?;
    if efficiency <= 0.0 || !transport_amv.is_finite() || transport_amv <= 0.0 {
        return None;
    }
    let gap = half - seller_cover;
    let haul = (gap / efficiency).ceil();
    if !haul.is_finite() || haul < 1.0 {
        return None;
    }
    let free = actors.get(buyer).free_units(transport);
    if free < haul {
        return None;
    }
    let buyer_after = buyer_cover - haul * efficiency;
    if buyer_after < half {
        return None;
    }
    let left = coins_left(seller_coins, &[], Some(transport));
    let haul_pay = mix_for(haul * transport_amv, &left)?;
    Some(MassFit {
        moved,
        pay: pay.to_vec(),
        haul,
        haul_pay,
        seller_advances: false,
    })
}

/// # Remainder Map
///
/// What `source` still holds after `taken`.
///
/// A good left at 0 is dropped. A good missing from `taken` stays whole.
fn remainder_map(source: &HashMap<usize, f64>, taken: &HashMap<usize, f64>) -> HashMap<usize, f64> {
    source
        .iter()
        .filter_map(|(&good, &qty)| {
            let left = qty - taken.get(&good).copied().unwrap_or(0.0);
            if left == 0.0 {
                None
            } else {
                Some((good, left))
            }
        })
        .collect()
}

/// # Add Map
///
/// Adds each entry of `part` into `sum`.
///
/// A missing good in `sum` starts at 0.
fn add_map(sum: &mut HashMap<usize, f64>, part: &HashMap<usize, f64>) {
    for (&good, &qty) in part {
        *sum.entry(good).or_insert(0.0) += qty;
    }
}

/// # Founding Candidates
///
/// The pops on `market` that can give [`FOUNDING_HOUSEHOLDS`], most days first.
///
/// `actors` holds the pops. A candidate has to [`clears_founding_gate`].
/// The most [`Pop::set_aside_days`] comes first, then more households,
/// then a higher finite standard of living. A non-finite living ranks last.
/// The lower id breaks the rest. `wealth` is the floor that gate reads.
/// Returns those ids. An empty list is nobody.
fn founding_candidates(market: &Market, actors: &Actors, wealth: f64) -> Vec<usize> {
    let history = market.history();
    let mut ids: Vec<usize> = market
        .pops
        .iter()
        .copied()
        .filter(|&id| {
            actors.pops.contains_key(&id)
                && clears_founding_gate(actors.pop(id), &history, wealth)
        })
        .collect();
    ids.sort_by(|&left, &right| {
        let rank = |id| {
            let pop = actors.pop(id);
            let sol = pop.standard_of_living();
            let sol = if sol.is_finite() { sol } else { f64::NEG_INFINITY };
            (pop.set_aside_days(&history), pop_households(pop), sol)
        };
        let left_rank = rank(left);
        let right_rank = rank(right);
        right_rank
            .0
            .total_cmp(&left_rank.0)
            .then(right_rank.1.total_cmp(&left_rank.1))
            .then(right_rank.2.total_cmp(&left_rank.2))
            .then(left.cmp(&right))
    });
    ids
}

/// # Clears Founding Gate
///
/// Whether `pop` may give [`FOUNDING_HOUSEHOLDS`] to a new job.
///
/// `history` prices the set-aside. `wealth` is the days of set-aside
/// required. A non-finite or non-positive floor accepts any set-aside,
/// including none. The pop needs [`FOUNDING_MIN_HOUSEHOLDS`], and
/// [`Pop::set_aside_days`] must cover the floor. Returns that answer.
/// The wait between foundings is a separate market count.
fn clears_founding_gate(pop: &Pop, history: &MarketHistory, wealth: f64) -> bool {
    let floor = if wealth.is_finite() && wealth > 0.0 {
        wealth
    } else {
        0.0
    };
    pop_households(pop) >= FOUNDING_MIN_HOUSEHOLDS
        && (floor == 0.0 || pop.set_aside_days(history) + 1e-9 >= floor)
}

/// # Pop Households
///
/// Households on `pop`, or 0 when the count is not a positive finite number.
fn pop_households(pop: &Pop) -> f64 {
    let count = pop.demographics.household.count;
    if count.is_finite() && count > 0.0 {
        count
    } else {
        0.0
    }
}

/// One founding. `Join` sends the household to a pop that already has the set.
enum FoundingChoice {
    Create(Vec<usize>),
    Join { dest: usize, processes: Vec<usize> },
}

impl FoundingChoice {
    /// # Processes
    ///
    /// The process ids this founding would run.
    ///
    /// A join and a new pop carry the same list. Returns those ids.
    fn processes(&self) -> &[usize] {
        match self {
            Self::Create(processes) | Self::Join { processes, .. } => processes,
        }
    }
}

/// One change applied to the founder's lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FoundingEdit {
    /// Add this process.
    Add(usize),
    /// Remove this process.
    Drop(usize),
}

/// # Founding Goods Fraction
///
/// The share of working goods [`FOUNDING_HOUSEHOLDS`] take from a pop of `households`.
///
/// The fraction is those households over `households`, capped at the whole
/// pile. The parent keeps the same working goods per household that stays.
/// A non-finite or non-positive count returns 0. The set-aside gift is
/// [`Pop::founding_savings_gift`], not this fraction.
pub(crate) fn founding_goods_fraction(households: f64) -> f64 {
    if !households.is_finite() || households <= 0.0 {
        return 0.0;
    }
    (FOUNDING_HOUSEHOLDS / households).clamp(0.0, 1.0)
}

/// # Founding Stake
///
/// Goods that would travel with one founding from `pop`.
///
/// Working quantity moves by [`founding_goods_fraction`]. `savings_days`
/// is the set-aside each leaving household takes, priced on `history`
/// and added from [`Pop::founding_savings_gift`]. Time is left out. A
/// non-positive pile is left out. Returns that map.
fn founding_stake(pop: &Pop, history: &MarketHistory, savings_days: f64) -> HashMap<usize, f64> {
    let mut stake = HashMap::new();
    let fraction = founding_goods_fraction(pop_households(pop));
    for (&good, row) in &pop.property {
        if good == TIME || !row.quantity.is_finite() || row.quantity <= 0.0 {
            continue;
        }
        let have = row.quantity * fraction;
        if have > 0.0 {
            stake.insert(good, have);
        }
    }
    for (good, qty) in pop.founding_savings_gift(history, savings_days, FOUNDING_HOUSEHOLDS) {
        if qty > 0.0 {
            *stake.entry(good).or_insert(0.0) += qty;
        }
    }
    stake
}

/// # Stake Units
///
/// The sum of the positive finite amounts in `stake`.
fn stake_units(stake: &HashMap<usize, f64>) -> f64 {
    stake
        .values()
        .copied()
        .filter(|amount| amount.is_finite() && *amount > 0.0)
        .sum()
}

/// # Is Process Stocked
///
/// Whether `stake` can start `process` for `runs` iterations.
///
/// Make time returns false. `runs` must be finite and positive. The stake
/// must hold at least `runs` units in total, so a time-only process can
/// start from any goods. A required input of time is the morning grant and
/// is not taken from the stake. A consumed or destroyed input needs
/// `amount * runs`. Capital needs `amount` once. Factors and optional
/// inputs are not required. Returns that answer.
fn is_process_stocked(process: &Process, stake: &HashMap<usize, f64>, runs: f64) -> bool {
    if makes_time(process) || !runs.is_finite() || runs <= 0.0 {
        return false;
    }
    if stake_units(stake) + 1e-9 < runs {
        return false;
    }
    for input in process.requirements() {
        if input.good == TIME || !input.amount.is_finite() || input.amount <= 0.0 {
            continue;
        }
        let have = stake.get(&input.good).copied().unwrap_or(0.0);
        let need = match input.input_type {
            InputType::Capital => input.amount,
            InputType::Destroyed | InputType::Consumed => input.amount * runs,
            InputType::Factor => continue,
        };
        if have + 1e-9 < need {
            return false;
        }
    }
    true
}

/// # Is Offer Covered
///
/// True when `good` was offered for at least one unit and that offer met the request.
///
/// `closes` are the day's books. A good quoted twice keeps the larger offer
/// and the larger request. No quote is false.
fn is_offer_covered(closes: &[MassClose], good: usize) -> bool {
    let mut offered = 0.0;
    let mut requested = 0.0;
    let mut seen = false;
    for close in closes {
        if close.good != good {
            continue;
        }
        seen = true;
        if close.offered > offered {
            offered = close.offered;
        }
        if close.requested > requested {
            requested = close.requested;
        }
    }
    seen && offered >= 1.0 && offered + 1e-9 >= requested
}

/// # Founding Drop Weight
///
/// The weight of dropping `process`.
///
/// The base is 1. [`FOUNDING_AVAILABLE_DROP`] replaces it when a variable
/// product's offer covered the request. `closes` are the day's books. Time
/// and a fixed output do not count. Returns that weight.
fn founding_drop_weight(process: &Process, closes: &[MassClose]) -> f64 {
    let available = variable_products(process)
        .iter()
        .any(|good| is_offer_covered(closes, *good));
    if available {
        FOUNDING_AVAILABLE_DROP
    } else {
        1.0
    }
}

/// # Founding Add Weight
///
/// The weight of adding one process the stake can run.
///
/// The base is 1. `new_good` doubles it. `asked` is the units the market
/// asked for across that process's variable products. An ask of at least
/// one doubles the weight and then multiplies it by one plus the natural
/// log of the ask, so a larger bid weighs more than a single unit. `stuck`
/// is a product of the founder's lowest unmet desire, and it quadruples
/// the weight. Returns that weight.
fn founding_add_weight(new_good: bool, asked: f64, stuck: bool) -> f64 {
    let mut weight = 1.0;
    if new_good {
        weight *= 2.0;
    }
    if asked.is_finite() && asked >= 1.0 {
        weight *= 2.0 * (1.0 + asked.ln());
    }
    if stuck {
        weight *= 4.0;
    }
    weight
}

/// # Pick Weighted
///
/// The item `roll` lands on.
///
/// `roll` is in `[0, 1)`. `items` is walked in the order given. A weight
/// that is not positive and finite is skipped. The roll times the sum of
/// the kept weights selects one item. A roll of 1 lands on the last kept
/// item. No positive weight returns `None`.
fn pick_weighted(roll: f64, items: &[(FoundingEdit, f64)]) -> Option<FoundingEdit> {
    let mut total = 0.0;
    for (_, weight) in items {
        if weight.is_finite() && *weight > 0.0 {
            total += *weight;
        }
    }
    if total <= 0.0 {
        return None;
    }
    let roll = if roll.is_finite() {
        roll.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut cursor = if roll >= 1.0 {
        (total - f64::EPSILON).max(0.0)
    } else {
        roll * total
    };
    if cursor < 0.0 {
        cursor = 0.0;
    }
    let mut last = None;
    for (edit, weight) in items {
        if !weight.is_finite() || *weight <= 0.0 {
            continue;
        }
        last = Some(*edit);
        if cursor < *weight {
            return Some(*edit);
        }
        cursor -= *weight;
    }
    last
}

/// # Choose Useful
///
/// Whether this founding draws from the useful pool.
///
/// `roll` is in `[0, 1)`. `share` is the useful chance. A finite share is
/// clamped to 0..=1. Any other share is 0. An empty useful pool returns
/// false. An empty other pool returns true when the useful pool has an
/// edit. Otherwise the roll has to land under `share`.
fn choose_useful(roll: f64, share: f64, has_useful: bool, has_other: bool) -> bool {
    if !has_useful {
        return false;
    }
    if !has_other {
        return true;
    }
    let share = if share.is_finite() {
        share.clamp(0.0, 1.0)
    } else {
        0.0
    };
    roll < share
}

/// # Variable Products
///
/// Variable outputs of `process`, excluding time.
///
/// Returns [`Process::variable_goods`].
fn variable_products(process: &Process) -> Vec<usize> {
    process.variable_goods()
}

/// # Produced Goods
///
/// Variable products any pop on `market` already makes.
///
/// `actors` holds the pops. `factuals` supplies the processes. A missing
/// process adds nothing. Time and fixed outputs are left out. Returns
/// those good ids.
fn produced_goods(market: &Market, actors: &Actors, factuals: &Factuals) -> HashSet<usize> {
    let mut goods = HashSet::new();
    for id in &market.pops {
        if !actors.pops.contains_key(id) {
            continue;
        }
        for process in founder_processes(actors.pop(*id)) {
            let Some(recipe) = factuals.get_process(process) else {
                continue;
            };
            goods.extend(variable_products(recipe));
        }
    }
    goods
}

/// # Demanded Goods
///
/// Goods an open bid or a desire on `market` already asks for.
///
/// An open bid comes from [`open_bid_goods`]. A desire target on a pop
/// seated here counts. `actors` holds those pops. Returns those good ids.
fn demanded_goods(market: &Market, actors: &Actors) -> HashSet<usize> {
    let mut goods: HashSet<usize> = open_bid_goods(&market.mass_closes).into_iter().collect();
    for id in &market.pops {
        if !actors.pops.contains_key(id) {
            continue;
        }
        for tier in &actors.pop(*id).desires {
            for desire in tier {
                for target in &desire.target {
                    goods.insert(target.good);
                }
            }
        }
    }
    goods
}

/// # Is New Good
///
/// Whether `process` has a variable product nobody in `produced` makes.
fn is_new_good(process: &Process, produced: &HashSet<usize>) -> bool {
    variable_products(process).iter().any(|good| !produced.contains(good))
}

/// # Has Demanded Output
///
/// Whether `process` has a variable product in `demanded`.
fn has_demanded_output(process: &Process, demanded: &HashSet<usize>) -> bool {
    variable_products(process)
        .iter()
        .any(|good| demanded.contains(good))
}

/// # Is Remainder Stocked
///
/// Whether dropping `drop_id` leaves a loaded process [`is_process_stocked`] accepts.
///
/// `current` is the founder's process ids. Fewer than two ids returns
/// false. An id with no recipe does not count. Returns that answer.
fn is_remainder_stocked(
    factuals: &Factuals,
    current: &[usize],
    drop_id: usize,
    stake: &HashMap<usize, f64>,
    runs: f64,
) -> bool {
    if current.len() < 2 {
        return false;
    }
    for id in current {
        if *id == drop_id {
            continue;
        }
        let Some(process) = factuals.get_process(*id) else {
            continue;
        };
        if is_process_stocked(process, stake, runs) {
            return true;
        }
    }
    false
}

/// # Drop Replaced Subsistence
///
/// Removes subsistence processes that make a variable good `added` makes.
///
/// `factuals` supplies the recipes. `processes` is the job after the add.
/// The added id stays. A missing recipe, a subsistence add, or a specialty
/// with no shared variable good removes nothing. Nothing is returned.
fn drop_replaced_subsistence(factuals: &Factuals, processes: &mut Vec<usize>, added: usize) {
    let Some(process) = factuals.get_process(added) else {
        return;
    };
    if process.is_subsistence() {
        return;
    }
    let goods = process.variable_goods();
    if goods.is_empty() {
        return;
    }
    // A specialty replaces the subsistence line that makes the same good.
    processes.retain(|id| {
        if *id == added {
            return true;
        }
        let Some(other) = factuals.get_process(*id) else {
            return true;
        };
        if !other.is_subsistence() {
            return true;
        }
        !other
            .variable_goods()
            .iter()
            .any(|good| goods.contains(good))
    });
}

/// # Stuck Desire Goods
///
/// Targets of the founder's lowest unmet desire.
///
/// `pop` is the founder. A desire with no target returns an empty set. A
/// target this market already makes still counts. Returns those good ids.
fn stuck_desire_goods(pop: &Pop) -> HashSet<usize> {
    let Some((_, desire)) = pop.lowest_unmet_desire() else {
        return HashSet::new();
    };
    desire.target.iter().map(|target| target.good).collect()
}

/// # Output Ask
///
/// The largest request among `process`'s variable products.
///
/// `closes` are yesterday's books. `demanded` are desire targets and open
/// bids. A good in `demanded` whose request is under one counts as one.
/// Returns that ask.
fn output_ask(process: &Process, demanded: &HashSet<usize>, closes: &[MassClose]) -> f64 {
    let mut best = 0.0;
    for good in variable_products(process) {
        let mut asked = 0.0;
        for close in closes {
            if close.good == good && close.requested > asked {
                asked = close.requested;
            }
        }
        if asked < 1.0 && demanded.contains(&good) {
            asked = 1.0;
        }
        if asked > best {
            best = asked;
        }
    }
    best
}

/// # Founding Processes
///
/// The job a founding from `founder` would run, and where it goes.
///
/// `market` supplies the open bids, the price board, and the pops already
/// working. `actors` holds those pops. `factuals` supplies processes and
/// the founding settings. `rng` draws the pool and then the edit. The
/// stake is [`founding_stake`], priced before anything moves. An edit has to pass [`is_process_stocked`] for
/// [`crate::game::config::PopConfig::founding_runs`]. The useful pool adds
/// a process whose [`trade_score`] is finite and positive, and a process
/// that makes a good in [`stuck_desire_goods`] even when that score is not
/// positive. It also drops a process [`is_process_unprofitable`] marks when
/// [`is_remainder_stocked`] passes. [`founding_add_weight`] sizes an add
/// from [`output_ask`] and that desire. [`choose_useful`] picks that pool
/// with [`crate::game::config::PopConfig::founding_useful_share`], and an
/// empty pool falls through to the other. The other pool adds
/// [`founding_process`] for an open bid that is not already useful, when
/// the stake covers it. [`pick_weighted`] walks the chosen pool in process
/// id order, adds before drops. A drop weighs [`founding_drop_weight`].
/// One edit is applied, then
/// [`apply_both_edit`]. An add also calls [`drop_replaced_subsistence`].
/// [`complete_craft_groups`] then fills a multi-step craft the set already runs.
/// An empty set is skipped. A set the
/// founder already runs is skipped. Another pop with that demographic and
/// that set is [`FoundingChoice::Join`]. Returns that choice, or `None`.
fn founding_processes(
    market: &Market,
    actors: &Actors,
    factuals: &Factuals,
    founder: usize,
    rng: &mut impl Rng,
) -> Option<FoundingChoice> {
    let pop = actors.pop(founder);
    let demo = pop.demographics;
    let current = founder_processes(pop);
    let history = market.history();
    // Price the leaving goods before anything moves.
    let stake = founding_stake(pop, &history, factuals.config.pop.founding_goods_weight);
    let runs = factuals.config.pop.founding_runs;
    let share = factuals.config.pop.founding_useful_share;
    let produced = produced_goods(market, actors, factuals);
    let demanded = demanded_goods(market, actors);
    let stuck_goods = stuck_desire_goods(pop);
    let consider = |processes: &[usize]| -> Option<FoundingChoice> {
        if processes.is_empty() {
            return None;
        }
        if let Some(dest) = other_pop_with_set(&market.pops, actors, founder, demo, processes) {
            return Some(FoundingChoice::Join {
                dest,
                processes: processes.to_vec(),
            });
        }
        if process_set_taken(&market.pops, actors, demo, processes) {
            return None;
        }
        Some(FoundingChoice::Create(processes.to_vec()))
    };
    // A paying add, a stuck desire the stake can run, or a drop the leaving goods can still run.
    let mut process_ids: Vec<usize> = factuals.each_process().map(|process| process.id).collect();
    process_ids.sort_unstable();
    process_ids.dedup();
    let mut useful: Vec<(FoundingEdit, f64)> = Vec::new();
    for id in process_ids {
        let Some(process) = factuals.get_process(id) else {
            continue;
        };
        if current.contains(&id) || makes_time(process) || !is_process_stocked(process, &stake, runs)
        {
            continue;
        }
        let score = trade_score(process, &history);
        let stuck = has_demanded_output(process, &stuck_goods);
        if !score.is_finite() || (score <= 0.0 && !stuck) {
            continue;
        }
        let weight = founding_add_weight(
            is_new_good(process, &produced),
            output_ask(process, &demanded, &market.mass_closes),
            stuck,
        );
        useful.push((FoundingEdit::Add(id), weight));
    }
    let mut drops: Vec<usize> = current
        .iter()
        .copied()
        .filter(|id| is_process_unprofitable(factuals, &history, *id))
        .filter(|id| is_remainder_stocked(factuals, &current, *id, &stake, runs))
        .collect();
    drops.sort_unstable();
    drops.dedup();
    for id in drops {
        let weight = factuals
            .get_process(id)
            .map(|process| founding_drop_weight(process, &market.mass_closes))
            .unwrap_or(1.0);
        useful.push((FoundingEdit::Drop(id), weight));
    }
    // An open bid the useful pool did not take, still only when the stake covers it.
    let mut other_ids = Vec::new();
    for good in open_bid_goods(&market.mass_closes) {
        let Some(process) = founding_process(factuals, good) else {
            continue;
        };
        if current.contains(&process) || other_ids.contains(&process) {
            continue;
        }
        if useful
            .iter()
            .any(|(edit, _)| matches!(edit, FoundingEdit::Add(id) if *id == process))
        {
            continue;
        }
        let Some(recipe) = factuals.get_process(process) else {
            continue;
        };
        if makes_time(recipe) || !is_process_stocked(recipe, &stake, runs) {
            continue;
        }
        other_ids.push(process);
    }
    other_ids.sort_unstable();
    let other: Vec<(FoundingEdit, f64)> = other_ids
        .into_iter()
        .map(|id| (FoundingEdit::Add(id), 1.0))
        .collect();
    if useful.is_empty() && other.is_empty() {
        return None;
    }
    let pick_useful = choose_useful(
        rng.r#gen::<f64>(),
        share,
        !useful.is_empty(),
        !other.is_empty(),
    );
    let pool = if pick_useful { &useful } else { &other };
    let Some(edit) = pick_weighted(rng.r#gen::<f64>(), pool) else {
        return None;
    };
    let mut next = current.clone();
    let (added, dropped) = match edit {
        FoundingEdit::Add(id) => {
            if !next.contains(&id) {
                next.push(id);
            }
            drop_replaced_subsistence(factuals, &mut next, id);
            (Some(id), None)
        }
        FoundingEdit::Drop(id) => {
            next.retain(|process| *process != id);
            (None, Some(id))
        }
    };
    // The other kind of edit, on the share of foundings that do both.
    let useful_adds: Vec<(FoundingEdit, f64)> = useful
        .iter()
        .copied()
        .filter(|(edit, _)| matches!(edit, FoundingEdit::Add(_)))
        .collect();
    let add_pool: &[(FoundingEdit, f64)] = if useful_adds.is_empty() {
        &other
    } else {
        &useful_adds
    };
    apply_both_edit(
        factuals,
        &history,
        &market.mass_closes,
        &stake,
        runs,
        add_pool,
        added,
        dropped,
        &mut next,
        rng,
    );
    complete_craft_groups(factuals, &mut next);
    consider(&next)
}

/// # Complete Craft Groups
///
/// Unions a multi-step craft onto `next` when the job already runs part of it.
///
/// `factuals` supplies the crafts. A craft qualifies when it has no origin,
/// lists more than one process, and does not list every process of the open
/// subsistence craft. No open subsistence craft leaves `next` unchanged.
/// Each process of a qualifying craft that meets `next` is added. Each of
/// those processes then drops the subsistence line that makes the same good.
/// Nothing is returned.
fn complete_craft_groups(factuals: &Factuals, next: &mut Vec<usize>) {
    let base: HashSet<usize> = factuals
        .each_craft()
        .filter(|craft| craft.origin.is_none() && craft.name == "subsistence")
        .flat_map(|craft| craft.processes.iter().copied())
        .collect();
    if base.is_empty() {
        return;
    }
    let mut ids: Vec<usize> = factuals
        .each_craft()
        .filter(|craft| {
            if craft.origin.is_some() {
                return false;
            }
            let own: HashSet<usize> = craft.processes.iter().copied().collect();
            own.len() > 1 && !base.is_subset(&own) && own.iter().any(|id| next.contains(id))
        })
        .map(|craft| craft.id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        let processes = factuals
            .get_craft(id)
            .map(|craft| craft.processes.clone())
            .unwrap_or_default();
        for process in &processes {
            if !next.contains(process) {
                next.push(*process);
            }
        }
        for process in &processes {
            if next.contains(process) {
                drop_replaced_subsistence(factuals, next, *process);
            }
        }
    }
}

/// # Apply Both Edit
///
/// Applies the other kind of edit when this founding does both.
///
/// `factuals` supplies the recipes. `history` prices a drop. `closes` are
/// the day's books. `stake` and `runs` are the goods that would travel and
/// how many times they must start a line. `adds` are the processes a drop may also take, in the
/// order already weighted. `added` is the process the first edit added.
/// `dropped` is the process it removed. `next` is the job after that edit.
/// `rng` draws the gate, then the candidate when the gate lands.
///
/// A roll at or above [`FOUNDING_BOTH_SHARE`] leaves `next` as it is. An
/// add then drops one unprofitable line still on `next`, other than the
/// process just added, when [`is_remainder_stocked`] passes. That drop
/// weighs [`founding_drop_weight`]. A drop then
/// adds one entry of `adds` that is not already on `next` and is not the
/// process just removed, and [`drop_replaced_subsistence`] runs for that
/// add. Candidates are walked in id order. No candidate leaves `next`
/// unchanged. Nothing is returned.
fn apply_both_edit(
    factuals: &Factuals,
    history: &MarketHistory,
    closes: &[MassClose],
    stake: &HashMap<usize, f64>,
    runs: f64,
    adds: &[(FoundingEdit, f64)],
    added: Option<usize>,
    dropped: Option<usize>,
    next: &mut Vec<usize>,
    rng: &mut impl Rng,
) {
    let roll = rng.r#gen::<f64>();
    let roll = if roll.is_finite() { roll } else { 0.0 };
    if roll >= FOUNDING_BOTH_SHARE {
        return;
    }
    if let Some(added_id) = added {
        let mut drops = Vec::new();
        for id in next.iter().copied() {
            if id == added_id {
                continue;
            }
            if !is_process_unprofitable(factuals, history, id) {
                continue;
            }
            if !is_remainder_stocked(factuals, next, id, stake, runs) {
                continue;
            }
            let weight = factuals
                .get_process(id)
                .map(|process| founding_drop_weight(process, closes))
                .unwrap_or(1.0);
            drops.push((FoundingEdit::Drop(id), weight));
        }
        drops.sort_unstable_by_key(|(edit, _)| match *edit {
            FoundingEdit::Add(id) | FoundingEdit::Drop(id) => id,
        });
        if drops.is_empty() {
            return;
        }
        if let Some(FoundingEdit::Drop(id)) = pick_weighted(rng.r#gen::<f64>(), &drops) {
            next.retain(|process| *process != id);
        }
        return;
    }
    let mut choices = Vec::new();
    for (edit, weight) in adds {
        let FoundingEdit::Add(id) = *edit else {
            continue;
        };
        if next.contains(&id) || dropped == Some(id) {
            continue;
        }
        choices.push((*edit, *weight));
    }
    if choices.is_empty() {
        return;
    }
    if let Some(FoundingEdit::Add(id)) = pick_weighted(rng.r#gen::<f64>(), &choices) {
        if !next.contains(&id) {
            next.push(id);
        }
        drop_replaced_subsistence(factuals, next, id);
    }
}

/// The four demographic ids and the process set on one pop.
///
/// Process ids are sorted and are not repeated. The household shape is not
/// part of the key. [`job_key`] builds it.
struct JobKey {
    species: usize,
    culture: usize,
    stratum: usize,
    religion: usize,
    processes: Vec<usize>,
}

/// # Job Key
///
/// Builds a [`JobKey`] from `pop`.
fn job_key(pop: &Pop) -> JobKey {
    let mut processes = founder_processes(pop);
    processes.sort_unstable();
    processes.dedup();
    JobKey {
        species: pop.demographics.species,
        culture: pop.demographics.culture,
        stratum: pop.demographics.stratum,
        religion: pop.demographics.religion,
        processes,
    }
}

/// # Same Job Key
///
/// Whether `pop` has the demographic and the process set in `key`.
///
/// Line order does not matter. Returns that answer.
fn same_job_key(pop: &Pop, key: &JobKey) -> bool {
    let mut processes = founder_processes(pop);
    processes.sort_unstable();
    processes.dedup();
    pop.demographics.species == key.species
        && pop.demographics.culture == key.culture
        && pop.demographics.stratum == key.stratum
        && pop.demographics.religion == key.religion
        && processes == key.processes
}

/// # Other Pop With Set
///
/// The lowest id in `pops`, other than `founder`, with this demographic and job.
///
/// `actors` holds the pops. `demo` is the four ids. `processes` is the job.
/// Line order does not matter. No match returns `None`.
fn other_pop_with_set(
    pops: &HashSet<usize>,
    actors: &Actors,
    founder: usize,
    demo: DemoRow,
    processes: &[usize],
) -> Option<usize> {
    let want: HashSet<usize> = processes.iter().copied().collect();
    let mut ids: Vec<usize> = pops
        .iter()
        .copied()
        .filter(|&id| id != founder && actors.pops.contains_key(&id))
        .collect();
    ids.sort_unstable();
    ids.into_iter().find(|&id| {
        let pop = actors.pop(id);
        let have: HashSet<usize> = pop.job.lines.iter().map(|line| line.process).collect();
        pop.demographics.species == demo.species
            && pop.demographics.culture == demo.culture
            && pop.demographics.stratum == demo.stratum
            && pop.demographics.religion == demo.religion
            && have == want
    })
}

/// # Same Process Set
///
/// Whether `left` and `right` hold the same process ids.
///
/// Order does not matter. A repeated id counts once. Returns that answer.
fn same_process_set(left: &[usize], right: &[usize]) -> bool {
    let mut left: Vec<usize> = left.to_vec();
    let mut right: Vec<usize> = right.to_vec();
    left.sort_unstable();
    right.sort_unstable();
    left.dedup();
    right.dedup();
    left == right
}

/// # Makes Time
///
/// Whether `process` has a variable output of time.
///
/// A fixed byproduct of time does not count. Returns that answer.
fn makes_time(process: &Process) -> bool {
    process
        .outputs
        .iter()
        .any(|output| !output.fixed && output.good == TIME && output.amount > 0.0)
}

/// # Process Runs On
///
/// Whether one run of `process` needs only time and goods in `goods`.
///
/// A variable time output does not run. Optional inputs and factors are
/// not required. Time is the morning grant, so it does not have to be in
/// `goods`. Any other required good needs `floor` of at least one unit.
/// Returns that answer.
fn process_runs_on(process: &Process, goods: &HashMap<usize, f64>) -> bool {
    if makes_time(process) {
        return false;
    }
    process.requirements().iter().all(|input| {
        input.good == TIME
            || input.amount <= 0.0
            || goods.get(&input.good).copied().unwrap_or(0.0).floor() >= 1.0
    })
}

/// # Set Needs Floor
///
/// Whether every loaded process in `processes` fails [`process_runs_on`].
///
/// `factuals` supplies the recipes. `goods` is the stock a fresh pop would
/// hold. An id with no recipe is ignored. No loaded recipe returns false,
/// so a set this board cannot read is left as it is. Returns that answer.
fn set_needs_floor(factuals: &Factuals, processes: &[usize], goods: &HashMap<usize, f64>) -> bool {
    let mut known = false;
    for id in processes {
        let Some(process) = factuals.get_process(*id) else {
            continue;
        };
        known = true;
        if process_runs_on(process, goods) {
            return false;
        }
    }
    known
}

/// # Floor Processes
///
/// Subsistence processes a fresh pop can run when `processes` cannot.
///
/// `factuals` supplies the recipes. `goods` is the stock that pop would
/// hold. [`set_needs_floor`] decides whether a floor is needed. Each
/// returned id is subsistence, is not make time, and passes
/// [`process_runs_on`]. The ids are sorted and are not repeated.
fn floor_processes(
    factuals: &Factuals,
    processes: &[usize],
    goods: &HashMap<usize, f64>,
) -> Vec<usize> {
    if !set_needs_floor(factuals, processes, goods) {
        return Vec::new();
    }
    let mut ids: Vec<usize> = factuals
        .each_process()
        .filter(|process| {
            process.is_subsistence() && process_runs_on(process, goods)
        })
        .map(|process| process.id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// # Pop Stock
///
/// Working units plus saved units of each good `pop` holds.
///
/// Time is left out. A non-positive pile is left out. Returns that map.
fn pop_stock(pop: &Pop) -> HashMap<usize, f64> {
    let mut goods = HashMap::new();
    for &good in pop.property.keys() {
        if good == TIME {
            continue;
        }
        let have = pop.working_units(good) + pop.saved.get(&good).copied().unwrap_or(0.0);
        if have.floor() >= 1.0 {
            goods.insert(good, have);
        }
    }
    for (&good, &qty) in &pop.saved {
        if good == TIME || goods.contains_key(&good) {
            continue;
        }
        let have = pop.working_units(good) + qty;
        if have.floor() >= 1.0 {
            goods.insert(good, have);
        }
    }
    goods
}

/// # Recovery Processes
///
/// The job a dead `pop` switches to.
///
/// `factuals` supplies the processes. The first choice is every
/// subsistence process [`process_runs_on`] accepts for [`pop_stock`].
/// When the pop already runs that set, the next runnable process that is
/// not subsistence and not make time is the whole job. The search starts
/// above the highest id the pop runs now, then wraps. No such process
/// returns the subsistence list, which may be empty.
fn recovery_processes(factuals: &Factuals, pop: &Pop) -> Vec<usize> {
    let goods = pop_stock(pop);
    let mut subsistence: Vec<usize> = factuals
        .each_process()
        .filter(|process| process.is_subsistence() && process_runs_on(process, &goods))
        .map(|process| process.id)
        .collect();
    subsistence.sort_unstable();
    subsistence.dedup();
    let current = founder_processes(pop);
    if !subsistence.is_empty() && !same_process_set(&current, &subsistence) {
        return subsistence;
    }
    let mut others: Vec<usize> = factuals
        .each_process()
        .filter(|process| {
            !process.is_subsistence() && process_runs_on(process, &goods)
        })
        .map(|process| process.id)
        .collect();
    others.sort_unstable();
    others.dedup();
    let Some(next) = others
        .iter()
        .copied()
        .find(|id| *id > current.iter().copied().max().unwrap_or(0))
        .or_else(|| others.first().copied())
    else {
        return subsistence;
    };
    vec![next]
}

/// # Open Missing Lines
///
/// Adds a line at target `None` for each process the job does not already run.
///
/// `processes` is the list to cover, in order. A process already on a line
/// keeps that line. Returns whether a line was added.
fn open_missing_lines(job: &mut Job, processes: &[usize]) -> bool {
    let mut added = false;
    for process in processes {
        if job.lines.iter().any(|line| line.process == *process) {
            continue;
        }
        job.lines.push(JobLine::new(*process, None, Vec::new()));
        added = true;
    }
    added
}

/// # Founder Processes
///
/// Process ids on `pop`'s job, in line order, each id once.
fn founder_processes(pop: &Pop) -> Vec<usize> {
    let mut processes = Vec::new();
    for line in &pop.job.lines {
        if !processes.contains(&line.process) {
            processes.push(line.process);
        }
    }
    processes
}

/// # Is Process Unprofitable
///
/// True when one run of `process` does not gain holding.
///
/// `factuals` supplies the process. `history` prices [`trade_score`]. A
/// missing process is false. A score that is not finite is false. A score
/// at or below zero is true.
fn is_process_unprofitable(factuals: &Factuals, history: &MarketHistory, process: usize) -> bool {
    factuals.get_process(process).is_some_and(|recipe| {
        let score = trade_score(recipe, history);
        score.is_finite() && score <= 0.0
    })
}

/// # Open Bid Goods
///
/// Goods on `closes` that were asked for and not offered.
///
/// A good quoted twice keeps the larger offer and the larger request. An
/// open good requested at least one unit and offered less than one. Returns
/// those ids, lowest first.
fn open_bid_goods(closes: &[MassClose]) -> Vec<usize> {
    let mut offered: HashMap<usize, f64> = HashMap::new();
    let mut requested: HashMap<usize, f64> = HashMap::new();
    for close in closes {
        let offer = offered.entry(close.good).or_insert(0.0);
        *offer = (*offer).max(close.offered);
        let ask = requested.entry(close.good).or_insert(0.0);
        *ask = (*ask).max(close.requested);
    }
    let mut goods: Vec<usize> = requested
        .into_iter()
        .filter(|(good, asked)| *asked >= 1.0 && offered.get(good).copied().unwrap_or(0.0) < 1.0)
        .map(|(good, _)| good)
        .collect();
    goods.sort_unstable();
    goods
}

/// # Founding Process
///
/// The process that founds `good`.
///
/// Returns [`crate::game::job::recipe_for_good`].
fn founding_process(factuals: &Factuals, good: usize) -> Option<usize> {
    crate::game::job::recipe_for_good(factuals, good)
}

/// # Craft Id For
///
/// The craft id whose open process set matches `processes`.
///
/// `factuals` supplies the crafts. An exact set match uses the lowest craft
/// id. Otherwise the open craft whose processes are a non-empty subset of
/// the job wins, most processes first, and the lowest id breaks a tie. No
/// match is `0`.
fn craft_id_for(factuals: &Factuals, processes: &[usize]) -> usize {
    let set: HashSet<usize> = processes.iter().copied().collect();
    let mut exact: Vec<usize> = factuals
        .each_craft()
        .filter(|craft| {
            craft.origin.is_none()
                && craft
                    .processes
                    .iter()
                    .copied()
                    .collect::<HashSet<_>>()
                    == set
        })
        .map(|craft| craft.id)
        .collect();
    exact.sort_unstable();
    if let Some(id) = exact.first() {
        return *id;
    }
    let mut contained: Vec<(usize, usize)> = factuals
        .each_craft()
        .filter_map(|craft| {
            if craft.origin.is_some() {
                return None;
            }
            let own: HashSet<usize> = craft.processes.iter().copied().collect();
            if own.is_empty() || !own.is_subset(&set) {
                return None;
            }
            Some((own.len(), craft.id))
        })
        .collect();
    contained.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    contained.first().map(|(_, id)| *id).unwrap_or(0)
}

/// # Process Set Taken
///
/// True when a pop in `pops` already has this demographic and this job.
///
/// `actors` holds the pops. `demo` is the four ids. `processes` is the job.
/// Line order does not matter.
fn process_set_taken(
    pops: &HashSet<usize>,
    actors: &Actors,
    demo: DemoRow,
    processes: &[usize],
) -> bool {
    let want: HashSet<usize> = processes.iter().copied().collect();
    pops.iter().any(|&id| {
        if !actors.pops.contains_key(&id) {
            return false;
        }
        let pop = actors.pop(id);
        let have: HashSet<usize> = pop.job.lines.iter().map(|line| line.process).collect();
        pop.demographics.species == demo.species
            && pop.demographics.culture == demo.culture
            && pop.demographics.stratum == demo.stratum
            && pop.demographics.religion == demo.religion
            && have == want
    })
}

/// # Next Pop Id
///
/// One higher than any pop id on `market` or in `actors`.
fn next_pop_id(market: &Market, actors: &Actors) -> usize {
    market
        .pops
        .iter()
        .copied()
        .chain(actors.pops.keys().copied())
        .max()
        .unwrap_or(0)
        + 1
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::{
        AMV_EPSILON, CURRENCY_SLOTS, Market, MarketGood, MarketHistory, MassClose, MassMiss,
        OPENING_CURRENCY, PopExit, PopRemoval, SALABILITY_DEFAULT, SALABILITY_MAX, SALABILITY_MIN,
        amv_scale,
        clearing_price, make_change, mix_change, monetary_rating, payment_tender, payment_tenders,
        ration,
    };
    use rand::SeedableRng;

    use crate::game::actor::Actor;
    use crate::game::actors::Actors;
    use crate::game::craft::Craft;
    use crate::game::culture::Culture;
    use crate::game::deal::DealMaker;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag, TIME};
    use crate::game::job::{Job, JobLine};
    use crate::game::marketorder::MarketOrder;
    use crate::game::pop::{Pop, PopPRow};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};
    use crate::game::religion::Religion;
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::sentiment::Sentiment;
    use crate::game::species::Species;

    #[test]
    fn amv_scale_clamps_and_monetary_rating_starts_at_par() {
        assert!((amv_scale(0.0) - 0.05).abs() < 1e-12);
        assert!((amv_scale(0.1) - 0.1).abs() < 1e-12);
        assert!((amv_scale(1.5) - 1.0).abs() < 1e-12);
        assert!((monetary_rating(1.5) - 0.5).abs() < 1e-12);
        assert_eq!(monetary_rating(0.1), 0.0);
        assert!((SALABILITY_DEFAULT - 0.5).abs() < 1e-12);
    }

    #[test]
    fn set_salability_stops_at_the_floor_and_the_cap() {
        let mut good = MarketGood::new();
        good.set_salability(0.0);
        assert!((good.salability - SALABILITY_MIN).abs() < 1e-12);
        good.set_salability(0.0002);
        assert!((good.salability - 0.0002).abs() < 1e-12);
        good.set_salability(9.0);
        assert!((good.salability - SALABILITY_MAX).abs() < 1e-12);
    }

    #[test]
    fn holding_value_uses_the_scale_not_the_raw_salability() {
        let mut history = MarketHistory::new();
        history.prices.insert(1, 2.0);
        history.salability.insert(1, 1.5);
        assert!((history.holding_per_unit(1) - 2.0).abs() < 1e-12);
        history.salability.insert(1, 0.1);
        assert!((history.holding_per_unit(1) - 0.2).abs() < 1e-12);
        history.prices.insert(1, -2.0);
        history.salability.insert(1, 0.1);
        assert!((history.holding_per_unit(1) - -2.0).abs() < 1e-12);
    }

    /// A new market has four tender slots. The first holds the opening good.
    #[test]
    fn new_market_fills_one_currency_slot() {
        let market = Market::new(1);
        assert_eq!(market.currencies().len(), CURRENCY_SLOTS);
        let filled: Vec<usize> = market.currencies().into_iter().flatten().collect();
        assert_eq!(filled, vec![OPENING_CURRENCY]);
    }

    /// The mass-market quote sits out until an actor posts one.
    #[test]
    fn mass_order_sits_out() {
        let pop = Pop::new(1);
        let history = MarketHistory::new();
        assert!(
            pop.mass_order(
                1,
                &history,
                &[Some(OPENING_CURRENCY), None, None, None],
                &Factuals::new(),
            )
            .is_none()
        );
    }

    /// No quote means the mass pass leaves stock where it is.
    #[test]
    fn mass_market_leaves_stock_while_actors_sit_out() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(2.0));
        let mut actors = Actors::new();
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(10.0));
        seat(&mut market, &mut actors, seller);
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.mass_market(&mut actors, &Factuals::new(), &mut rng);
        assert!((actors.pop(2).property[&1].quantity - 10.0).abs() < 1e-9);
    }

    /// A balanced book clears at yesterday's AMV.
    #[test]
    fn clearing_price_stays_on_the_amv_when_the_book_balances() {
        let price = clearing_price(&[(4.0, 5.0)], &[(4.0, 1.0)], 2.0);
        assert!((price.expect("price") - 2.0).abs() < 1e-9);
    }

    /// The cross between two limits is the interpolated price.
    #[test]
    fn clearing_price_interpolates_between_the_bracketing_limits() {
        let buys = [(9.0, 10.0), (9.0, 4.0)];
        let sells = [(5.0, 2.0), (5.0, 8.0)];
        let price = clearing_price(&buys, &sells, 6.0).expect("price");
        assert!((price - 7.6).abs() < 1e-9);
    }

    /// A book that never balances keeps the limit closest to yesterday's AMV.
    #[test]
    fn clearing_price_keeps_the_limit_closest_to_the_amv() {
        let price = clearing_price(&[(10.0, 3.0)], &[(4.0, 1.0)], 2.0);
        assert!((price.expect("price") - 2.0).abs() < 1e-9);
    }

    /// No overlapping limit leaves the good uncleared.
    #[test]
    fn clearing_price_is_missing_when_the_limits_do_not_overlap() {
        assert!(clearing_price(&[(4.0, 1.0)], &[(4.0, 5.0)], 3.0).is_none());
    }

    /// The card sits under every ask, and a bid sits above the lowest ask.
    /// The price is that ask. It is not a point in the empty gap under it.
    #[test]
    fn clearing_price_uses_the_ask_when_the_card_has_no_seller() {
        let price = clearing_price(&[(44.0, 0.839)], &[(62.0, 0.586)], 0.581).expect("price");
        assert!((price - 0.586).abs() < 1e-9);
    }

    /// A later pass that does not trade keeps the no-cross already recorded.
    /// A pass that exchanges a unit clears it.
    #[test]
    fn remember_mass_close_keeps_a_no_cross_until_a_unit_trades() {
        let mut market = Market::new(1);
        market.remember_mass_close(1, 10.0, 4.0, 0.0, None, Some(MassMiss::NoCross));
        market.remember_mass_close(1, 0.0, 0.0, 0.0, None, None);
        assert_eq!(market.mass_closes[0].miss, Some(MassMiss::NoCross));
        assert!((market.mass_closes[0].offered - 10.0).abs() < 1e-9);
        market.remember_mass_close(1, 10.0, 4.0, 2.0, Some(1.5), None);
        assert!(market.mass_closes[0].miss.is_none());
        assert!(market.mass_closes[0].price.is_some());
    }

    /// A tie in the tender amount keeps the smaller whole payment.
    #[test]
    fn make_change_keeps_the_smaller_payment_on_a_tie() {
        assert_eq!(make_change(1.0, 1.5, 1.0, 10.0), (1.0, 1.0));
    }

    /// The nearer whole tender is the one that pays.
    #[test]
    fn make_change_pays_the_nearer_whole_tender() {
        assert_eq!(make_change(1.0, 1.6, 1.0, 10.0), (1.0, 2.0));
    }

    /// A buyer short of tender takes fewer units of the good.
    #[test]
    fn make_change_steps_the_good_down_when_the_tender_is_short() {
        assert_eq!(make_change(5.0, 1.0, 1.0, 2.0), (2.0, 2.0));
    }

    /// A long quote stops at the largest whole quantity the tender can pay.
    #[test]
    fn make_change_jumps_to_the_payable_quantity() {
        assert_eq!(make_change(10_000.0, 1.0, 1.0, 4.0), (4.0, 4.0));
        // Three units at 3 against a tender of 2 want 4.5. The tie at 4 and 5
        // keeps 4, and four units want 6, which is past the five on hand.
        assert_eq!(make_change(10_000.0, 3.0, 2.0, 5.0), (3.0, 4.0));
    }

    /// The short side is filled. The long side stops at that volume.
    #[test]
    fn ration_stops_at_the_shorter_side() {
        let orders = [(Actor::Pop(1), 5.0), (Actor::Pop(2), 5.0)];
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut filled = ration(&orders, 6.0, &mut rng);
        filled.sort_by_key(|(_, units)| *units as i64);
        assert_eq!(filled.len(), 2);
        assert!((filled[0].1 - 1.0).abs() < 1e-9);
        assert!((filled[1].1 - 5.0).abs() < 1e-9);
    }

    /// One unit at 1.5 AMV pays one tender when the two whole payments tie.
    #[test]
    fn clear_mass_good_pays_the_closer_tender() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(1.5));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(4.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 1.0).with_accept_price(3.0),
            MarketOrder::sell(Actor::Pop(2), 1, 1.0).with_accept_price(1.0),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &Factuals::new(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        assert!((actors.pop(1).property[&1].quantity - 1.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 9.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].traded - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].clear_units - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].clear_value - 1.5).abs() < 1e-9);
        assert_eq!(market.goods[&OPENING_CURRENCY].clear_units, 0.0);
        assert!((market.goods[&OPENING_CURRENCY].paid - 1.0).abs() < 1e-9);
        // Salability 0.5 scales the 1.5 clear to a holding print of 0.75.
        assert!((market.goods[&1].print_units - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].print_holding - 0.75).abs() < 1e-9);
        let close = &market.mass_closes()[0];
        assert_eq!(close.good, 1);
        assert!((close.offered - 1.0).abs() < 1e-9);
        assert!((close.requested - 1.0).abs() < 1e-9);
        assert!((close.exchanged - 1.0).abs() < 1e-9);
        assert!((close.price.expect("price") - 1.5).abs() < 1e-9);
    }

    /// Time covers freight at one per unit. Bulk is zero, so the bill is the flat fee.
    fn freight_world() -> Factuals {
        let mut time = Good {
            id: 0,
            name: "time".to_string(),
            class: None,
            decay_rate: 1.0,
            decay_result: HashMap::new(),
            mass: 0.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: Vec::new(),
        };
        time.tags.insert(GoodTag::transport(1.0));
        let grain = Good {
            id: 1,
            name: "grain".to_string(),
            class: None,
            decay_rate: 0.0,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: Vec::new(),
        };
        Factuals::new().with_good(time).with_good(grain)
    }

    /// Each side pays half of the flat fee. One time covers that half.
    #[test]
    fn clear_mass_good_splits_the_freight() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(1.5));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        buyer.property.insert(0, PopPRow::new(1.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(4.0));
        seller.property.insert(0, PopPRow::new(1.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 1.0).with_accept_price(3.0),
            MarketOrder::sell(Actor::Pop(2), 1, 1.0).with_accept_price(1.0),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut world = freight_world();
        world.goods.get_mut(&1).expect("grain").mass = 0.0;
        market.clear_mass_good(
            &mut actors,
            &world,
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        assert!((actors.pop(1).property[&1].quantity - 1.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 9.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 1.0).abs() < 1e-9);
        // Flat fee 1, split. Each side spends 0.5 time.
        assert!((actors.pop(1).property[&0].quantity - 0.5).abs() < 1e-9);
        assert!((actors.pop(2).property[&0].quantity - 0.5).abs() < 1e-9);
    }

    /// A buyer with no transport does not take the fill.
    #[test]
    fn clear_mass_good_skips_a_buyer_who_cannot_haul() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(1.5));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(4.0));
        seller.property.insert(0, PopPRow::new(1.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 1.0).with_accept_price(3.0),
            MarketOrder::sell(Actor::Pop(2), 1, 1.0).with_accept_price(1.0),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &freight_world(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        assert!(actors.pop(1).property.get(&1).is_none());
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 10.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&0].quantity - 1.0).abs() < 1e-9);
        assert_eq!(market.mass_closes()[0].exchanged, 0.0);
        assert!(market.mass_closes()[0].price.is_none());
    }

    /// The buyer holds no time. The seller has enough to advance one unit and
    /// still pay their own half, so the fill goes through. Time is priced at 1.
    #[test]
    fn clear_mass_good_buys_the_buyers_haul_from_the_seller() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(1.5));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        market.goods.insert(0, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(4.0));
        seller.property.insert(0, PopPRow::new(2.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 1.0).with_accept_price(3.0),
            MarketOrder::sell(Actor::Pop(2), 1, 1.0).with_accept_price(1.0),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &freight_world(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        assert!((actors.pop(1).property[&1].quantity - 1.0).abs() < 1e-9);
        // Grain takes 1 tender. The one time advanced takes another.
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 8.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&0].quantity - 0.5).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&0].quantity - 0.5).abs() < 1e-9);
        assert!((market.mass_closes()[0].exchanged - 1.0).abs() < 1e-9);
    }

    /// A negative time card still buys the haul. The payment is one tender.
    #[test]
    fn clear_mass_good_buys_the_haul_when_time_is_not_positive() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(1.5));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        market.goods.insert(0, MarketGood::new().with_amv(-1.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(4.0));
        seller.property.insert(0, PopPRow::new(2.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 1.0).with_accept_price(3.0),
            MarketOrder::sell(Actor::Pop(2), 1, 1.0).with_accept_price(1.0),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &freight_world(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        assert!((actors.pop(1).property[&1].quantity - 1.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 8.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&0].quantity - 0.5).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&0].quantity - 0.5).abs() < 1e-9);
        assert!((market.mass_closes()[0].exchanged - 1.0).abs() < 1e-9);
    }

    /// The seller holds no time. The buyer has enough to advance one unit and
    /// still pay their own half, so the fill goes through.
    #[test]
    fn clear_mass_good_buys_the_sellers_haul_from_the_buyer() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(1.5));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        market.goods.insert(0, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        buyer.property.insert(0, PopPRow::new(2.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(4.0));
        seller.property.insert(OPENING_CURRENCY, PopPRow::new(5.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 1.0).with_accept_price(3.0),
            MarketOrder::sell(Actor::Pop(2), 1, 1.0).with_accept_price(1.0),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &freight_world(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        assert!((actors.pop(1).property[&1].quantity - 1.0).abs() < 1e-9);
        // Grain costs 1. The seller pays 1 back for the time the buyer advanced.
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 10.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&0].quantity - 0.5).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 5.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&0].quantity - 0.5).abs() < 1e-9);
        assert!((market.mass_closes()[0].exchanged - 1.0).abs() < 1e-9);
    }

    /// Bulk plus the flat fee is more than one time can haul. The fill steps down to one.
    #[test]
    fn clear_mass_good_steps_the_fill_down_to_the_haul() {
        let mut market = Market::new(1).with_friction(1.0);
        market.goods.insert(1, MarketGood::new().with_amv(1.0));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        buyer.property.insert(0, PopPRow::new(1.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(4.0));
        seller.property.insert(0, PopPRow::new(1.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 4.0).with_accept_price(2.0),
            MarketOrder::sell(Actor::Pop(2), 1, 4.0).with_accept_price(0.5),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &freight_world(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        // One grain is bulk 1. The bill is 2, and each side can haul 1.
        assert!((actors.pop(1).property[&1].quantity - 1.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 9.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 1.0).abs() < 1e-9);
        assert_eq!(actors.pop(1).property[&0].quantity, 0.0);
        assert_eq!(actors.pop(2).property[&0].quantity, 0.0);
        assert!((market.mass_closes()[0].exchanged - 1.0).abs() < 1e-9);
    }

    /// A long quote still stops at the one unit both sides can haul.
    #[test]
    fn clear_mass_good_jumps_the_fill_down_to_the_haul() {
        let mut market = Market::new(1).with_friction(1.0);
        market.goods.insert(1, MarketGood::new().with_amv(1.0));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(400.0));
        buyer.property.insert(0, PopPRow::new(1.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(200.0));
        seller.property.insert(0, PopPRow::new(1.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 200.0).with_accept_price(2.0),
            MarketOrder::sell(Actor::Pop(2), 1, 200.0).with_accept_price(0.5),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &freight_world(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        assert!((actors.pop(1).property[&1].quantity - 1.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 399.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 199.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 1.0).abs() < 1e-9);
        assert_eq!(actors.pop(1).property[&0].quantity, 0.0);
        assert_eq!(actors.pop(2).property[&0].quantity, 0.0);
        assert!((market.mass_closes()[0].exchanged - 1.0).abs() < 1e-9);
    }

    /// A negative price uses the same whole tender as its magnitude.
    #[test]
    fn make_change_uses_the_magnitude_of_a_negative_price() {
        assert_eq!(make_change(1.0, -1.5, 1.0, 10.0), (1.0, 1.0));
    }

    /// Five grain and one gold are the only whole mix that pays six.
    #[test]
    fn mix_change_pays_five_grain_and_one_gold() {
        let (units, mix) = mix_change(1.0, 6.0, &[(1, 1.0, 5.0), (4, 1.0, 1.0)]);
        assert!((units - 1.0).abs() < 1e-9);
        assert_eq!(mix, vec![(1, 5.0), (4, 1.0)]);
    }

    /// Equal prices with a heavy second purse spend that purse.
    #[test]
    fn mix_change_spends_the_larger_purse() {
        let (units, mix) = mix_change(4.0, 1.0, &[(1, 1.0, 4.0), (4, 1.0, 26.0)]);
        assert!((units - 4.0).abs() < 1e-9);
        assert_eq!(mix, vec![(1, 1.0), (4, 3.0)]);
    }

    /// Equal purses split a payment that lands on whole units.
    #[test]
    fn mix_change_splits_equal_purses() {
        let (units, mix) = mix_change(4.0, 1.0, &[(1, 1.0, 10.0), (4, 1.0, 10.0)]);
        assert!((units - 4.0).abs() < 1e-9);
        assert_eq!(mix, vec![(1, 2.0), (4, 2.0)]);
    }

    /// A short purse buys fewer units. The coin counts are not searched.
    #[test]
    fn mix_change_scales_down_to_the_purse() {
        let (units, mix) = mix_change(10.0, 1.0, &[(1, 1.0, 3.0), (4, 1.0, 3.0)]);
        assert!((units - 6.0).abs() < 1e-9);
        assert_eq!(mix, vec![(1, 3.0), (4, 3.0)]);
    }

    /// One coin pays the same whole units [`make_change`] does.
    #[test]
    fn mix_change_matches_one_coin() {
        let one = |units, price, amv, free| {
            let (got, mix) = mix_change(units, price, &[(5, amv, free)]);
            let pay = mix.first().map(|(_, qty)| *qty).unwrap_or(0.0);
            (got, pay)
        };
        assert_eq!(one(1.0, 1.5, 1.0, 10.0), (1.0, 1.0));
        assert_eq!(one(1.0, 1.6, 1.0, 10.0), (1.0, 2.0));
        assert_eq!(one(5.0, 1.0, 1.0, 2.0), (2.0, 2.0));
        assert_eq!(one(1.0, -1.5, 1.0, 10.0), (1.0, 1.0));
        assert_eq!(one(10_000.0, 1.0, 1.0, 4.0), (4.0, 4.0));
        assert_eq!(one(10_000.0, 3.0, 2.0, 5.0), (3.0, 4.0));
    }

    /// The only filled tender is skipped. A second tender pays for the first.
    #[test]
    fn payment_tender_skips_the_only_tender_and_swaps_the_rest() {
        let one = [Some(5usize), None, None, None];
        assert_eq!(payment_tender(&one, 5), None);
        assert_eq!(payment_tender(&one, 1), Some(5));
        let two = [Some(5), Some(7), None, None];
        assert_eq!(payment_tender(&two, 1), Some(5));
        assert_eq!(payment_tender(&two, 5), Some(7));
        assert_eq!(payment_tender(&two, 7), Some(5));
    }

    /// An ordinary good is paid with every other filled slot.
    #[test]
    fn payment_tenders_include_every_other_slot() {
        let two = [Some(5usize), Some(7), None, None];
        assert_eq!(payment_tenders(&two, 1), vec![5, 7]);
        assert_eq!(payment_tenders(&two, 5), vec![7]);
        assert_eq!(payment_tenders(&two, 7), vec![5]);
    }

    /// Both sides accept the negative card, so the clear stays there.
    #[test]
    fn clearing_price_stays_on_a_negative_amv() {
        let price = clearing_price(&[(1.0, -0.5)], &[(1.0, -3.0)], -2.0);
        assert!((price.expect("price") - -2.0).abs() < 1e-9);
    }

    /// The seller pays two tender to move one unit of a good priced at -2.
    #[test]
    fn clear_mass_good_charges_the_seller_for_a_negative_price() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(-2.0));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let buyer = Pop::new(1);
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(4.0));
        seller.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 1.0).with_accept_price(-0.5),
            MarketOrder::sell(Actor::Pop(2), 1, 1.0).with_accept_price(-3.0),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &Factuals::new(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        assert!((actors.pop(1).property[&1].quantity - 1.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 8.0).abs() < 1e-9);
        assert!((market.goods[&OPENING_CURRENCY].paid - 2.0).abs() < 1e-9);
        // A negative clear is its own holding value.
        assert!((market.goods[&1].print_units - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].print_holding - -2.0).abs() < 1e-9);
        assert!((market.mass_closes()[0].price.expect("price") - -2.0).abs() < 1e-9);
    }

    /// A sell priced below zero is not a trade when nobody buys.
    ///
    /// Nothing changed hands, so the night does not write the decline.
    /// The price stays just above [`AMV_EPSILON`]. The ask of -1 is not
    /// the price. The currency stays at 1.
    #[test]
    fn clear_mass_good_holds_a_negative_sell_that_did_not_trade() {
        let mut market = Market::new(1);
        let price = AMV_EPSILON * 1.02;
        market
            .goods
            .insert(1, MarketGood::new().with_amv(price).with_salability(1.0));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        let orders = [MarketOrder::sell(Actor::Pop(2), 1, 4.0).with_accept_price(-1.0)];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut Actors::new(),
            &Factuals::new(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        market.note_decay(1, 10.0, 10.0);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - price).abs() < 1e-15);
        assert!((market.goods[&OPENING_CURRENCY].amv - 1.0).abs() < 1e-9);
    }

    /// Limits that do not meet still leave the book on the mass row, with no price.
    #[test]
    fn clear_mass_good_records_a_book_that_does_not_trade() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(3.0));
        let mut actors = Actors::new();
        let orders = [
            MarketOrder::buy(Actor::Pop(1), 1, 4.0).with_accept_price(1.0),
            MarketOrder::sell(Actor::Pop(2), 1, 4.0).with_accept_price(5.0),
        ];
        let history = market.history();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.clear_mass_good(
            &mut actors,
            &Factuals::new(),
            &history,
            1,
            OPENING_CURRENCY,
            &orders,
            &mut rng,
        );
        let close = &market.mass_closes()[0];
        assert!((close.offered - 4.0).abs() < 1e-9);
        assert!((close.requested - 4.0).abs() < 1e-9);
        assert_eq!(close.exchanged, 0.0);
        assert!(close.price.is_none());
        assert_eq!(market.goods[&1].print_units, 0.0);
        assert_eq!(market.goods[&1].traded, 0.0);
    }

    /// Good 7 is a second tender. Twenty on hand lists two. The buyer takes
    /// those two. The opening currency is not sold for itself.
    #[test]
    fn mass_market_swaps_the_second_tender() {
        let mut market = Market::new(1);
        market.currencies = [Some(OPENING_CURRENCY), Some(7), None, None];
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(1.0));
        market.goods.insert(7, MarketGood::new().with_amv(2.0));
        let mut actors = Actors::new();
        let mut buyer = Pop::new(1);
        buyer.property.insert(OPENING_CURRENCY, PopPRow::new(10.0));
        buyer.desires[0].push(Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(7, DesireTargetType::Consume, 1.0)],
            amount: 2.0,
            satisfaction: 0.0,
            target_taken: vec![0.0],
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        });
        let mut seller = Pop::new(2);
        seller.property.insert(7, PopPRow::new(20.0));
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, seller);
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.mass_market(&mut actors, &Factuals::new(), &mut rng);
        assert!((actors.pop(1).property[&7].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&OPENING_CURRENCY].quantity - 6.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&7].quantity - 18.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&OPENING_CURRENCY].quantity - 4.0).abs() < 1e-9);
    }

    /// The second tender buys grain, and that grain is mixed into the bread
    /// payment before the day ends.
    ///
    /// Each tender lists its sale room once. Bread is paid with both tenders.
    /// The standing one-unit bid takes one more loaf from the larger purse.
    #[test]
    fn mass_market_spends_tender_taken_in_the_same_day() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(7), None, None];
        market.goods.insert(1, MarketGood::new().with_amv(1.0));
        market.goods.insert(3, MarketGood::new().with_amv(1.0));
        market.goods.insert(7, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        let mut farmer = Pop::new(1);
        farmer.property.insert(1, PopPRow::new(40.0));
        let mut buyer = Pop::new(2);
        buyer.property.insert(7, PopPRow::new(30.0));
        buyer.desires[0].push(Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            amount: 10.0,
            satisfaction: 0.0,
            target_taken: vec![0.0],
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        });
        buyer.desires[0].push(Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 2,
            priority: 0,
            target: vec![DesireTarget::new(3, DesireTargetType::Consume, 1.0)],
            amount: 4.0,
            satisfaction: 0.0,
            target_taken: vec![0.0],
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        });
        let mut baker = Pop::new(3);
        baker.property.insert(3, PopPRow::new(10.0));
        seat(&mut market, &mut actors, farmer);
        seat(&mut market, &mut actors, buyer);
        seat(&mut market, &mut actors, baker);
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.mass_market(&mut actors, &Factuals::new(), &mut rng);
        assert!((actors.pop(1).property[&1].quantity - 36.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&7].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&3].quantity - 5.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&7].quantity - 22.0).abs() < 1e-9);
        assert!((actors.pop(3).property[&3].quantity - 5.0).abs() < 1e-9);
        assert!((actors.pop(3).property[&1].quantity - 1.0).abs() < 1e-9);
        assert!((actors.pop(3).property[&7].quantity - 4.0).abs() < 1e-9);
        let grain = market
            .mass_closes()
            .iter()
            .find(|row| row.good == 1)
            .expect("grain");
        assert!((grain.exchanged - 4.0).abs() < 1e-9);
        let bread = market
            .mass_closes()
            .iter()
            .find(|row| row.good == 3)
            .expect("bread");
        assert!((bread.exchanged - 5.0).abs() < 1e-9);
    }

    /// Grain rots faster than gold. Spare grain buys the gold the other pop will sell.
    #[test]
    fn mass_market_buys_durable_gold_with_spare_grain() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market.goods.insert(1, MarketGood::new().with_amv(1.0));
        market.goods.insert(4, MarketGood::new().with_amv(1.0));
        let factuals = Factuals::new()
            .with_good(plain_good(1, "grain", 0.5))
            .with_good(plain_good(4, "gold", 0.0));
        let mut actors = Actors::new();
        let mut holder = Pop::new(1);
        holder.property.insert(1, PopPRow::new(20.0));
        let mut seller = Pop::new(2);
        seller.property.insert(4, PopPRow::new(20.0));
        seat(&mut market, &mut actors, holder);
        seat(&mut market, &mut actors, seller);
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);

        market.mass_market(&mut actors, &factuals, &mut rng);

        assert!((actors.pop(1).property[&1].quantity - 18.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&4].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&4].quantity - 18.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 2.0).abs() < 1e-9);
    }

    /// Two tenders with the same decay do not convert.
    #[test]
    fn mass_market_does_not_convert_tenders_of_equal_durability() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market.goods.insert(1, MarketGood::new().with_amv(1.0));
        market.goods.insert(4, MarketGood::new().with_amv(1.0));
        let factuals = Factuals::new()
            .with_good(plain_good(1, "grain", 0.2))
            .with_good(plain_good(4, "gold", 0.2));
        let mut actors = Actors::new();
        let mut holder = Pop::new(1);
        holder.property.insert(1, PopPRow::new(20.0));
        let mut seller = Pop::new(2);
        seller.property.insert(4, PopPRow::new(20.0));
        seat(&mut market, &mut actors, holder);
        seat(&mut market, &mut actors, seller);
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);

        market.mass_market(&mut actors, &factuals, &mut rng);

        assert!((actors.pop(1).property[&1].quantity - 20.0).abs() < 1e-9);
        assert!(actors.pop(1).property.get(&4).is_none());
        assert!((actors.pop(2).property[&4].quantity - 20.0).abs() < 1e-9);
    }

    /// Four grain still clear for the meal. The farmer then buys one gold
    /// with one grain, so the seller of that gold holds five grain.
    #[test]
    fn mass_market_buys_grain_and_then_the_more_durable_gold() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market.goods.insert(1, MarketGood::new().with_amv(1.0));
        market.goods.insert(4, MarketGood::new().with_amv(1.0));
        let factuals = Factuals::new()
            .with_good(plain_good(1, "grain", 0.5))
            .with_good(plain_good(4, "gold", 0.0));
        let mut actors = Actors::new();
        let mut farmer = Pop::new(1);
        farmer.property.insert(1, PopPRow::new(40.0));
        let mut hungry = Pop::new(2);
        hungry.property.insert(4, PopPRow::new(20.0));
        hungry.desires[0].push(Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            amount: 4.0,
            satisfaction: 0.0,
            target_taken: vec![0.0],
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        });
        seat(&mut market, &mut actors, farmer);
        seat(&mut market, &mut actors, hungry);
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);

        market.mass_market(&mut actors, &factuals, &mut rng);

        assert!((actors.pop(2).property[&1].quantity - 5.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&4].quantity - 15.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 35.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&4].quantity - 5.0).abs() < 1e-9);
    }

    /// Gold costs more than the card even after a night of no decay, so it is not sold.
    #[test]
    fn mass_market_keeps_gold_when_the_card_is_under_its_cost() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market.goods.insert(0, MarketGood::new().with_amv(1.0));
        market.goods.insert(1, MarketGood::new().with_amv(1.0));
        market.goods.insert(4, MarketGood::new().with_amv(1.0));
        let process = Process::new(4, "make gold", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(4, 3.0, true));
        let factuals = Factuals::new()
            .with_good(plain_good(1, "grain", 0.5))
            .with_good(plain_good(4, "gold", 0.0))
            .with_process(process);
        let mut actors = Actors::new();
        let mut holder = Pop::new(1);
        holder.property.insert(1, PopPRow::new(20.0));
        let mut maker = Pop::new(2);
        maker.property.insert(4, PopPRow::new(30.0));
        maker.job = Job::new(1, vec![JobLine::new(4, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, holder);
        seat(&mut market, &mut actors, maker);
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);

        market.mass_market(&mut actors, &factuals, &mut rng);

        assert!((actors.pop(1).property[&1].quantity - 20.0).abs() < 1e-9);
        assert!(actors.pop(1).property.get(&4).is_none());
        assert!((actors.pop(2).property[&4].quantity - 30.0).abs() < 1e-9);
    }

    fn plain_good(id: usize, name: &str, decay: f64) -> Good {
        Good {
            id,
            name: name.to_string(),
            class: None,
            decay_rate: decay,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: Vec::new(),
        }
    }

    /// Production on the card is the night's produced snapshot. A good that
    /// was not made reads as zero.
    #[test]
    fn history_copies_production() {
        let mut market = Market::new(1);
        let mut made = MarketGood::new();
        made.production = 12.0;
        market.goods.insert(6, made);
        let history = market.history();
        assert!((history.produced(6) - 12.0).abs() < 1e-12);
        assert_eq!(history.produced(1), 0.0);
    }

    /// Stored sought is four. The mass book still wants nine, so the plan
    /// sees nine. A smaller mass remainder does not shrink the stored count.
    #[test]
    fn history_keeps_the_larger_unfilled_want() {
        let mut market = Market::new(1);
        let mut tools = MarketGood::new();
        tools.sought_unmet = 4.0;
        market.goods.insert(9, tools);
        market.mass_closes.push(MassClose {
            good: 9,
            offered: 0.0,
            requested: 10.0,
            exchanged: 1.0,
            price: None,
            miss: None,
        });
        market.mass_closes.push(MassClose {
            good: 1,
            offered: 8.0,
            requested: 3.0,
            exchanged: 3.0,
            price: Some(1.0),
            miss: None,
        });
        let mut grain = MarketGood::new();
        grain.sought_unmet = 12.0;
        market.goods.insert(1, grain);

        let history = market.history();

        assert!((history.sought(9) - 9.0).abs() < 1e-12);
        assert!((history.sought(1) - 12.0).abs() < 1e-12);
    }

    /// One unit offered or exchanged is supply. A fraction under one is not.
    #[test]
    fn history_copies_the_mass_book() {
        let mut market = Market::new(1);
        market.mass_closes.push(MassClose {
            good: 1,
            offered: 8.0,
            requested: 3.0,
            exchanged: 3.0,
            price: Some(1.0),
            miss: None,
        });
        market.mass_closes.push(MassClose {
            good: 9,
            offered: 0.0,
            requested: 2.0,
            exchanged: 1.0,
            price: None,
            miss: None,
        });
        market.mass_closes.push(MassClose {
            good: 4,
            offered: 0.5,
            requested: 0.0,
            exchanged: 0.0,
            price: None,
            miss: None,
        });

        let history = market.history();

        assert!((history.mass_offered[&1] - 8.0).abs() < 1e-12);
        assert!((history.mass_exchanged[&1] - 3.0).abs() < 1e-12);
        assert!(history.is_mass_supplied(1));
        assert!(history.mass_offered.get(&9).is_none());
        assert!(history.is_mass_supplied(9));
        assert!(!history.is_mass_supplied(4));
        assert!(!history.is_mass_supplied(2));
    }

    /// Filled tender slots are copied in order. An empty slot is left out,
    /// and a good that fills two slots is listed once.
    #[test]
    fn history_copies_the_tender_slots() {
        let mut market = Market::new(1);
        market.currencies = [
            Some(OPENING_CURRENCY),
            None,
            Some(1),
            Some(OPENING_CURRENCY),
        ];

        let history = market.history();

        assert_eq!(history.currencies(), &[OPENING_CURRENCY, 1]);
    }

    /// Nothing traded, so the unsold ease does not land. The tape still clears.
    #[test]
    fn record_keeping_holds_an_unsold_good_and_clears_the_tape() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.0);
        good.offered_unsold = 10.0;
        market.goods.insert(1, good);
        let before = market.history();
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((before.price(1) - 2.0).abs() < 1e-12);
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.0).abs() < 1e-9);
        assert_eq!(market.goods[&1].offered_unsold, 0.0);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// A glut whose only bid is the unpaid want keeps its price. Nothing
    /// traded. The same numbers with the bid on the long side, and a real
    /// bid, fall by twice the rise.
    #[test]
    fn record_keeping_holds_an_unaffordable_surplus_that_did_not_trade() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market.unaffordable.insert(1);
        market.unaffordable.insert(2);
        market.unpaid_wants.insert(1, 1.0);
        market.mass_closes.push(idle_close(1, 10.0, 1.0, 0.0));
        market.mass_closes.push(idle_close(2, 1.0, 6.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.0).abs() < 1e-9);
        let dragged = 2.0 * 2.0 * 0.03 * (5.0 / 6.0);
        assert!((market.goods[&2].amv - (2.0 - dragged)).abs() < 1e-9);
        assert!((market.goods[&2].salability - 0.99).abs() < 1e-9);
    }

    /// A glut nobody could buy, with a real bid and nothing traded, falls
    /// by the unsold share of its price.
    #[test]
    fn record_keeping_eases_an_unaffordable_surplus_a_buyer_asked_for() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market.unaffordable.insert(1);
        market.mass_closes.push(idle_close(1, 10.0, 4.0, 0.0));
        market.record_keeping(&Factuals::new());
        let eased = 2.0 * 0.03 * (6.0 / 10.0);
        assert!((market.goods[&1].amv - (2.0 - eased)).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.99).abs() < 1e-9);
    }

    /// A mass surplus that did not trade leaves the opening AMV.
    /// No currency is stored.
    #[test]
    fn record_keeping_holds_a_mass_surplus_that_did_not_trade() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// A mass shortage that did not trade leaves both cards, including a negative one.
    #[test]
    fn record_keeping_holds_a_mass_shortage_that_did_not_trade() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(-4.0).with_salability(1.0));
        market.note_negative_value(2);
        market.mass_closes.push(idle_close(1, 0.0, 8.0, 0.0));
        market.mass_closes.push(idle_close(2, 1.0, 6.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - -4.0).abs() < 1e-9);
    }

    /// One unit in and one point of complexity break even at 2. The ceiling
    /// sits one idle step above that.
    fn paying_output() -> Factuals {
        Factuals::new().with_process(
            Process::new(3, "make", 0)
                .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(2, 1.0, false)),
        )
    }

    /// A traded surplus stops one idle step above the ask where the run pays.
    /// The uncapped step would have crossed that ask.
    #[test]
    fn record_keeping_stops_a_surplus_fall_at_the_make() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(2.05).with_salability(1.0));
        market.mass_closes.push(idle_close(2, 10.0, 0.0, 10.0));
        market.record_keeping(&paying_output());
        assert!((market.goods[&2].amv - 2.03).abs() < 1e-9);
    }

    /// An empty offer with a real bid rises by the idle step, and stops at the make.
    /// A book nobody could buy does not rise. A price already above the make stays.
    #[test]
    fn record_keeping_raises_an_empty_offer_toward_the_make() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(2, 0.0, 8.0, 0.0));
        market.record_keeping(&paying_output());
        assert!((market.goods[&2].amv - 1.03).abs() < 1e-9);

        market.goods.get_mut(&2).unwrap().set_amv(1.0);
        market.unaffordable.insert(2);
        market.mass_closes = vec![idle_close(2, 0.0, 8.0, 0.0)];
        market.record_keeping(&paying_output());
        assert!((market.goods[&2].amv - 1.0).abs() < 1e-9);

        market.unaffordable.remove(&2);
        market.goods.get_mut(&2).unwrap().set_amv(3.0);
        market.mass_closes = vec![idle_close(2, 0.0, 8.0, 0.0)];
        market.record_keeping(&paying_output());
        assert!((market.goods[&2].amv - 3.0).abs() < 1e-9);
    }

    /// Both sides quoted and never shared a price. The card stays, and the
    /// ask walk finishes on the card. A later fill leaves that ask there.
    #[test]
    fn record_keeping_holds_the_card_when_the_sides_do_not_cross() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        let mut close = idle_close(1, 10.0, 4.0, 0.0);
        close.miss = Some(MassMiss::NoCross);
        market.mass_closes.push(close);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.history().ask_walk(1) - 1.0).abs() < 1e-12);

        market.mass_closes = vec![idle_close(1, 10.0, 4.0, 4.0)];
        market.record_keeping(&Factuals::new());
        assert!((market.history().ask_walk(1) - 1.0).abs() < 1e-12);
    }

    /// Both sides quoted and the haul blocked every fill. The card stays.
    #[test]
    fn record_keeping_holds_the_card_when_the_haul_blocks_the_book() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        let mut close = idle_close(1, 10.0, 4.0, 0.0);
        close.miss = Some(MassMiss::Haul);
        market.mass_closes.push(close);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.history().ask_walk(1) - 0.25).abs() < 1e-12);
    }

    /// A partial surplus that traded steps down by the unsold share.
    /// A balanced book, an untradeable good, and the only tender did not
    /// trade, so they stay at the opening AMV.
    #[test]
    fn record_keeping_steps_a_partial_surplus_and_leaves_the_rest() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(4.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(4.0).with_salability(1.0));
        market
            .goods
            .insert(3, MarketGood::new().with_amv(4.0).with_salability(1.0));
        market.goods.insert(
            OPENING_CURRENCY,
            MarketGood::new().with_amv(4.0).with_salability(1.0),
        );
        market.mass_closes.push(idle_close(1, 9.0, 1.0, 2.0));
        market.mass_closes.push(idle_close(2, 4.0, 4.0, 0.0));
        market.mass_closes.push(idle_close(3, 6.0, 0.0, 0.0));
        market
            .mass_closes
            .push(idle_close(OPENING_CURRENCY, 6.0, 0.0, 0.0));
        let mut tags = HashSet::new();
        tags.insert(GoodTag::Untradeable);
        let factuals = Factuals::new().with_good(Good {
            id: 3,
            name: "sealed".to_string(),
            class: None,
            decay_rate: 0.0,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 1.0,
            tags,
            categories: Vec::new(),
        });
        market.record_keeping(&factuals);
        // Good 1 offered 9 against 1 bid and traded. The unsold share is
        // 8/9 of the 3 percent step, taken off its opening 4. The currency
        // did not trade, so its AMV stays.
        let stepped = 4.0 * (1.0 - 0.03 * (8.0 / 9.0));
        assert!((market.goods[&1].amv - stepped).abs() < 1e-9);
        assert!((market.goods[&2].amv - 4.0).abs() < 1e-9);
        assert!((market.goods[&3].amv - 4.0).abs() < 1e-9);
        assert!((market.goods[&OPENING_CURRENCY].amv - 4.0).abs() < 1e-9);
    }

    /// Both sides quoted grain and nothing traded. The card stays.
    #[test]
    fn record_keeping_holds_grain_when_both_sides_quote_and_nothing_traded() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 2.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// Eight grain paid for other goods sit on both sides of a ten-by-two book.
    /// One unit traded. The gap stays eight. The share is eight eighteenths
    /// of the step. Gold did not trade, so grain keeps that step.
    #[test]
    fn record_keeping_dampens_a_grain_surplus_by_the_units_paid_out() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        let mut grain = MarketGood::new().with_amv(1.0).with_salability(1.0);
        grain.paid = 8.0;
        market.goods.insert(1, grain);
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 2.0, 1.0));
        market.record_keeping(&Factuals::new());
        let grain = 1.0 - 0.03 * (8.0 / 18.0);
        assert!((market.goods[&1].amv - grain).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// Eight grain paid for other goods, and grain's own book exchanged
    /// nothing. The surplus does not write. Gold did not trade, so both stay
    /// where they opened.
    #[test]
    fn record_keeping_holds_grain_spent_as_tender_when_grain_did_not_sell() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        let mut grain = MarketGood::new().with_amv(1.0).with_salability(1.0);
        grain.paid = 8.0;
        market.goods.insert(1, grain);
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 2.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// The same eight paid units halve an eight-bid shortage. One unit
    /// traded. Gold is not offered and did not trade, so grain keeps that step.
    #[test]
    fn record_keeping_dampens_a_grain_shortage_by_the_units_paid_out() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        let mut grain = MarketGood::new().with_amv(1.0).with_salability(1.0);
        grain.paid = 8.0;
        market.goods.insert(1, grain);
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 0.0, 8.0, 1.0));
        market.record_keeping(&Factuals::new());
        let grain = 1.0 + 0.03 * (8.0 / 16.0);
        assert!((market.goods[&1].amv - grain).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// Payment on a good that is not a tender does not widen its book.
    /// One unit traded. Ten offered and none bid is still the full 3 percent cut.
    #[test]
    fn record_keeping_does_not_dampen_a_non_currency_by_its_payment() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        let mut bread = MarketGood::new().with_amv(1.0).with_salability(1.0);
        bread.paid = 100.0;
        market.goods.insert(2, bread);
        market.mass_closes.push(idle_close(2, 10.0, 0.0, 1.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&2].amv - 0.97).abs() < 1e-9);
    }

    /// Grain is a currency. Ten unsold units and no gold on offer do not cut it.
    /// Nothing traded, so both currencies keep the AMV they opened at.
    #[test]
    fn record_keeping_holds_unsold_grain_when_gold_is_not_offered() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// The same unsold grain falls once one unit of it traded and gold is offered.
    /// Gold's own book is balanced and did not trade, so grain keeps the step.
    #[test]
    fn record_keeping_lowers_unsold_grain_when_gold_is_offered() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 1.0));
        market.mass_closes.push(idle_close(4, 5.0, 5.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 0.97).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// No gold is offered. Eight purchase bids, one of them filled, raise grain.
    /// Gold did not trade, so grain keeps that step.
    #[test]
    fn record_keeping_raises_grain_when_it_is_bid_and_gold_is_not_offered() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 0.0, 8.0, 1.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.03).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// The same empty offer, when the eight bids are only the unpaid want,
    /// leaves the card. That want is not a purchase.
    #[test]
    fn record_keeping_holds_grain_when_the_short_bid_is_only_the_unpaid_want() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.unpaid_wants.insert(1, 8.0);
        market.mass_closes.push(idle_close(1, 0.0, 8.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// Ten unsold units and four unpaid wants do not cut grain. The want
    /// is not the bid that makes a surplus a price.
    #[test]
    fn record_keeping_holds_unsold_grain_when_the_bid_is_only_the_unpaid_want() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.unpaid_wants.insert(1, 4.0);
        market.mass_closes.push(idle_close(1, 10.0, 4.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// Gold is offered. Eight bids and one filled unit raise grain.
    /// Gold's own book is balanced and did not trade, so grain keeps the step.
    #[test]
    fn record_keeping_raises_grain_when_it_is_bid_and_gold_is_offered() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 0.0, 8.0, 1.0));
        market.mass_closes.push(idle_close(4, 5.0, 5.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.03).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// No gold is offered. A pile that rots completely leaves the card.
    /// The book could not be paid, so the rot is not a lower price.
    #[test]
    fn record_keeping_holds_rotting_grain_when_gold_is_not_offered() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.note_decay(1, 10.0, 10.0);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.0).abs() < 1e-9);
    }

    /// Gold is offered and its book is balanced. Grain traded one unit on a
    /// balanced book, so only the rot moves it. A total rot takes 5 percent.
    /// Gold did not trade, so grain keeps that cut.
    #[test]
    fn record_keeping_lets_grain_rot_when_gold_is_offered() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 10.0, 1.0));
        market.mass_closes.push(idle_close(4, 5.0, 5.0, 0.0));
        market.note_decay(1, 10.0, 10.0);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 0.95).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// No gold is offered. Process use that would clear the print cap
    /// leaves the card. That use is not a price the book could pay.
    #[test]
    fn record_keeping_holds_grain_process_use_when_gold_is_not_offered() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        let mut grain = MarketGood::new().with_amv(1.0).with_salability(1.0);
        grain.process_used = 10.0;
        grain.process_value = 40.0;
        market.goods.insert(1, grain);
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// Grain was paid, and gold is not offered. The AMV stays. Payment
    /// still raises salability.
    #[test]
    fn record_keeping_raises_grain_salability_while_the_price_stays() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        let mut grain = MarketGood::new().with_amv(1.0).with_salability(1.0);
        grain.paid = 1.0;
        market.goods.insert(1, grain);
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.05).abs() < 1e-9);
    }

    /// Eight bids and an empty offer, one of them filled, and the morning
    /// grant already made them. The shortage step does not rise. The flow
    /// term would ease the card, and the short book keeps the morning ask.
    #[test]
    fn record_keeping_holds_a_shortage_the_morning_grant_covers() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.0);
        good.production = 8.0;
        market.goods.insert(1, good);
        market.mass_closes.push(idle_close(1, 0.0, 8.0, 1.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.0).abs() < 1e-9);
    }

    /// A shortage that traded one unit adds the idle step, and salability
    /// follows toward par. The card is 2, so the same step is less than
    /// 3 percent of the price. The restore stops at the 1 percent cap.
    #[test]
    fn record_keeping_restores_salability_when_the_card_rises() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(0.4));
        market.mass_closes.push(idle_close(1, 0.0, 8.0, 1.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.03).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.41).abs() < 1e-9);
    }

    /// Grain pays for cotton and cotton pays for grain. Both books are short
    /// and both traded. Each rise is the idle fraction as AMV, so the dearer
    /// card does not take a larger step than a card at 1.
    #[test]
    fn record_keeping_does_not_ratchet_a_traded_pair() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        let mut grain = MarketGood::new().with_amv(8.0).with_salability(1.0);
        grain.paid = 20.0;
        grain.sought_unmet = 4.0;
        let mut cotton = MarketGood::new().with_amv(8.0).with_salability(1.0);
        cotton.paid = 10.0;
        cotton.sought_unmet = 4.0;
        market.goods.insert(1, grain);
        market.goods.insert(2, cotton);
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 4.0, 20.0, 4.0));
        market.mass_closes.push(idle_close(2, 4.0, 20.0, 4.0));
        market.record_keeping(&Factuals::new());
        // Grain was spent, so the paid units sit on both sides. The gap is
        // 16 of a book of 40. Cotton has no such spend. Each also takes the
        // flat unmet step.
        let grain = 8.0 + 0.03 * (16.0 / 40.0) + 0.01;
        let cotton = 8.0 + 0.03 * (16.0 / 20.0) + 0.01;
        assert!(
            (market.goods[&1].amv - grain).abs() < 1e-9,
            "grain {}",
            market.goods[&1].amv
        );
        assert!(
            (market.goods[&2].amv - cotton).abs() < 1e-9,
            "cotton {}",
            market.goods[&2].amv
        );
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }

    /// A tender with no positive price means the book cannot trade.
    /// Nobody valued that tender below zero, so it sits at [`AMV_EPSILON`].
    /// That mean is not above the dead zone, and the card is not restated.
    #[test]
    fn record_keeping_skips_a_mass_book_whose_tender_cannot_pay() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(-1.0));
        market.mass_closes.push(idle_close(1, 5.0, 0.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&OPENING_CURRENCY].amv - AMV_EPSILON).abs() < 1e-15);
    }

    /// One offered against six bids, and one unit traded, is a rise of five
    /// sixths of the idle step. Nobody could buy that good, so the night
    /// writes twice that rise downward. The same book with no such mark still
    /// rises. A bid with nothing offered still rises. Salability follows the
    /// fall, and stops at the one percent cap.
    #[test]
    fn record_keeping_drags_an_unaffordable_shortage_down_by_twice_the_rise() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market
            .goods
            .insert(3, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market.unaffordable.insert(1);
        market.unaffordable.insert(3);
        market.mass_closes.push(idle_close(1, 1.0, 6.0, 1.0));
        market.mass_closes.push(idle_close(2, 1.0, 6.0, 1.0));
        market.mass_closes.push(idle_close(3, 0.0, 8.0, 1.0));
        market.record_keeping(&Factuals::new());
        // The drag is twice the rise, taken off the price. The rise itself
        // is the idle fraction as AMV, on the book somebody could buy and
        // on the empty offer.
        let dragged = 2.0 * 2.0 * 0.03 * (5.0 / 6.0);
        let shortage = 0.03 * (5.0 / 6.0);
        assert!((market.goods[&1].amv - (2.0 - dragged)).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.99).abs() < 1e-9);
        assert!((market.goods[&2].amv - (2.0 + shortage)).abs() < 1e-9);
        assert!((market.goods[&3].amv - 2.03).abs() < 1e-9);
    }

    /// One unit traded on the good. The mass row missed on no-cross, so
    /// both sides quoted and the card would have held. Nobody could buy
    /// it, so the night takes the full idle step down. The ask finishes
    /// on the card.
    #[test]
    fn record_keeping_drags_a_held_book_by_the_full_idle_step() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.0);
        good.traded = 1.0;
        market.goods.insert(1, good);
        let mut close = idle_close(1, 10.0, 4.0, 0.0);
        close.miss = Some(MassMiss::NoCross);
        market.mass_closes.push(close);
        market.unaffordable.insert(1);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.94).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.99).abs() < 1e-9);
        assert!((market.history().ask_walk(1) - 1.0).abs() < 1e-12);
    }

    /// The offer is already the long side, and one unit traded. The
    /// unaffordable mark does not deepen that fall. Ten against two is
    /// four fifths of the idle step.
    #[test]
    fn record_keeping_leaves_a_falling_surplus_when_the_book_is_unaffordable() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 2.0, 1.0));
        market.unaffordable.insert(1);
        market.record_keeping(&Factuals::new());
        let step = 0.03 * (8.0 / 10.0);
        assert!((market.goods[&1].amv - (2.0 - 2.0 * step)).abs() < 1e-9);
    }

    /// Grain's only bid is the unpaid want, so the full write is skipped.
    /// One unit still changed hands. The book has both sides and nobody
    /// could buy it, so the drag falls grain. Gold did not trade, so grain
    /// keeps that fall.
    #[test]
    fn record_keeping_drags_unaffordable_grain_the_full_write_skipped() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), Some(4), None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.unpaid_wants.insert(1, 6.0);
        market.mass_closes.push(idle_close(1, 1.0, 6.0, 1.0));
        market.unaffordable.insert(1);
        market.record_keeping(&Factuals::new());
        let grain = 1.0 - 2.0 * 0.03 * (5.0 / 6.0);
        assert!((market.goods[&1].amv - grain).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.99).abs() < 1e-9);
        assert!(market.goods[&1].amv < 1.0);
    }

    /// The tender has no positive price, so the book cannot trade.
    /// Both sides quoted and the good is marked unaffordable. The card stays.
    #[test]
    fn record_keeping_does_not_drag_a_book_whose_tender_cannot_pay() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market
            .goods
            .insert(OPENING_CURRENCY, MarketGood::new().with_amv(-1.0));
        market.mass_closes.push(idle_close(1, 5.0, 5.0, 0.0));
        market.unaffordable.insert(1);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&OPENING_CURRENCY].amv - AMV_EPSILON).abs() < 1e-15);
    }

    /// # Idle Close
    ///
    /// One mass row with no clearing price.
    ///
    /// `offered`, `requested`, and `exchanged` are the book. Returns that row.
    fn idle_close(good: usize, offered: f64, requested: f64, exchanged: f64) -> MassClose {
        MassClose {
            good,
            offered,
            requested,
            exchanged,
            price: None,
            miss: None,
        }
    }

    /// Payment without a fall raises salability. No currency is stored,
    /// so the AMV stays where the night wrote it.
    #[test]
    fn record_keeping_raises_salability_when_payment_does_not_drop_amv() {
        let mut market = Market::new(1);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(0.5);
        good.traded = 4.0;
        good.paid = 4.0;
        market.goods.insert(1, good);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.55).abs() < 1e-9);
    }

    /// A good that was bought and not paid out steps toward par, and stops there.
    #[test]
    fn record_keeping_raises_a_bought_good_toward_par() {
        let mut market = Market::new(1);
        let mut low = MarketGood::new().with_amv(1.0).with_salability(0.5);
        low.traded = 4.0;
        let mut near = MarketGood::new().with_amv(1.0).with_salability(0.99);
        near.traded = 4.0;
        market.goods.insert(1, low);
        market.goods.insert(2, near);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].salability - 0.55).abs() < 1e-9);
        assert!((market.goods[&2].salability - 1.0).abs() < 1e-9);
    }

    /// A tenth of the pile rots, and one unit traded, so the price loses a
    /// quarter of that share.
    ///
    /// That quarter is 2.5 percent, under the 5 percent night cap. 2.0
    /// becomes 1.95. Salability loses only the loss cap.
    #[test]
    fn record_keeping_takes_a_quarter_of_the_rot_share() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.5);
        good.traded = 1.0;
        market.goods.insert(1, good);
        market.note_decay(1, 1.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.95).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.49).abs() < 1e-9);
        assert!((market.goods[&1].decayed - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].volume - 10.0).abs() < 1e-9);
    }

    /// A pile that rots completely, after one unit traded, stops at 5 percent
    /// of the price.
    ///
    /// A quarter of a total rot would be 25 percent. The night cap leaves
    /// 5 percent, so 2.0 becomes 1.90.
    #[test]
    fn record_keeping_caps_a_total_rot_at_five_percent() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.5);
        good.traded = 1.0;
        market.goods.insert(1, good);
        market.note_decay(1, 10.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.9).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.49).abs() < 1e-9);
    }

    /// A rot cut that would pass epsilon, after one unit traded, stops on the
    /// positive side of zero.
    ///
    /// The price sits just above [`AMV_EPSILON`]. Five percent of it is more
    /// than that gap, so decay leaves [`AMV_EPSILON`]. Salability loses only
    /// the loss cap.
    #[test]
    fn record_keeping_stops_rot_at_epsilon() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new()
            .with_amv(AMV_EPSILON * 1.02)
            .with_salability(1.5);
        good.traded = 1.0;
        market.goods.insert(1, good);
        market.note_decay(1, 10.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - AMV_EPSILON).abs() < 1e-15);
        assert!((market.goods[&1].salability - 1.49).abs() < 1e-9);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// A seller valued this good below zero, so decay leaves the negative price.
    #[test]
    fn record_keeping_leaves_a_negative_price_when_the_pile_rots() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(-2.0).with_salability(1.5));
        market.note_negative_value(1);
        market.note_decay(1, 40.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - -2.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.5).abs() < 1e-9);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// Nobody valued the good below zero, so a stored negative is lifted.
    #[test]
    fn record_keeping_lifts_a_negative_price_no_one_valued_below_zero() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(-2.0).with_salability(1.5));
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - AMV_EPSILON).abs() < 1e-15);
        assert!((market.goods[&1].salability - 1.5).abs() < 1e-9);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// Full rot on a price just above epsilon, plus an unsold surplus that
    /// traded one unit, stops on the positive side of zero.
    ///
    /// Five percent uses up the room above [`AMV_EPSILON`]. The idle step
    /// then takes 3 percent of the opening price. No seller priced the good
    /// below zero, so the night is put back at [`AMV_EPSILON`].
    #[test]
    fn record_keeping_stops_a_rotted_surplus_at_epsilon() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new()
            .with_amv(AMV_EPSILON * 1.02)
            .with_salability(1.0);
        good.traded = 1.0;
        market.goods.insert(1, good);
        market.note_decay(1, 10.0, 10.0);
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - AMV_EPSILON).abs() < 1e-15);
    }

    /// The same rot and surplus, after one unit traded, cross zero when a
    /// seller priced below zero.
    ///
    /// Five percent uses up the room above [`AMV_EPSILON`]. The idle step
    /// crosses into the dead zone, so the stored price is -[AMV_EPSILON].
    /// The negative minimum is not the new price.
    #[test]
    fn record_keeping_lets_a_rotted_surplus_cross_after_a_negative_value() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new()
            .with_amv(AMV_EPSILON * 1.02)
            .with_salability(1.0);
        good.traded = 1.0;
        market.goods.insert(1, good);
        market.note_negative_value(1);
        market.note_decay(1, 10.0, 10.0);
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - -AMV_EPSILON).abs() < 1e-15);
    }

    /// One unit traded, so the production flow is written.
    #[test]
    fn record_keeping_production_lowers_amv_and_salability_with_it() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.5);
        good.production = 10.0;
        good.traded = 1.0;
        market.goods.insert(1, good);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        // Nothing was paid, so the scale is 1. Flow cap 0.005 of that scale.
        assert!((market.goods[&1].amv - 1.995).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.4975).abs() < 1e-9);
        assert_eq!(market.goods[&1].production, 0.0);
    }

    /// Production written before the day is still on the good at night.
    ///
    /// The morning clear drops rot and the exchange tape, including the
    /// planted trade. It leaves production, and the night write then zeros
    /// it. Nothing traded that day, so the flow does not move AMV.
    #[test]
    fn market_day_keeps_production_recorded_before_the_reset() {
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.5);
        good.production = 10.0;
        good.traded = 4.0;
        good.decayed = 3.0;
        market.goods.insert(1, good);
        let mut actors = crate::game::actors::Actors::new();
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(
            &mut actors,
            &crate::game::factuals::Factuals::new(),
            &mut rng,
        );

        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.5).abs() < 1e-9);
        assert_eq!(market.goods[&1].production, 0.0);
        assert_eq!(market.goods[&1].decayed, 0.0);
        assert_eq!(market.goods[&1].traded, 0.0);
    }

    /// Open buys that traded rise by the unmet step and keep that write.
    ///
    /// The coin was paid, so its salability rises. The step is the flat
    /// unmet nudge, not a share of the coin. A price of 32 and a price of
    /// 0.2 gain the same amount. The coin's own day wrote nothing else, so
    /// it stays at 4. Good 8 was valued below zero, so its sign stays.
    #[test]
    fn record_keeping_raises_unmet_buys_on_the_goods_that_traded() {
        let mut market = Market::new(1);
        market.currencies = [Some(9), None, None, None];
        let mut dear = MarketGood::new().with_amv(32.0).with_salability(1.0);
        dear.sought_unmet = 10.0;
        dear.traded = 1.0;
        let mut cheap = MarketGood::new().with_amv(0.2).with_salability(1.0);
        cheap.sought_unmet = 10.0;
        cheap.traded = 1.0;
        let mut coin = MarketGood::new().with_amv(4.0).with_salability(1.0);
        coin.paid = 3.0;
        coin.traded = 1.0;
        let mut debt = MarketGood::new().with_amv(-2.0).with_salability(1.0);
        debt.paid = 100.0;
        debt.traded = 1.0;
        market.note_negative_value(8);
        let mut jewel = MarketGood::new()
            .with_amv(8.0)
            .with_salability(SALABILITY_MAX);
        jewel.traded = 1.0;
        market.goods.insert(1, dear);
        market.goods.insert(2, cheap);
        market.goods.insert(3, jewel);
        market.goods.insert(9, coin);
        market.goods.insert(8, debt);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 32.01).abs() < 1e-9);
        assert!((market.goods[&2].amv - 0.21).abs() < 1e-9);
        assert!((market.goods[&3].amv - 8.0).abs() < 1e-9);
        assert!((market.goods[&9].amv - 4.0).abs() < 1e-9);
        assert!((market.goods[&8].amv - -2.0).abs() < 1e-9);
        assert!((market.goods[&9].salability - 1.05).abs() < 1e-9);
    }

    /// Open buys that traded put the night's rot back and add the unmet step.
    ///
    /// Half of this pile is past the night cap, so rot would take 5 percent.
    /// The unmet step is 0.01. Nothing was paid.
    #[test]
    fn record_keeping_unmet_buys_outweigh_rot() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.0);
        good.sought_unmet = 4.0;
        good.traded = 1.0;
        market.goods.insert(1, good);
        market.note_decay(1, 5.0, 10.0);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.01).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.0).abs() < 1e-9);
        assert_eq!(market.goods[&1].sought_unmet, 0.0);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// A print at the morning holding leaves the price where it is.
    #[test]
    fn record_keeping_leaves_a_par_print_unmoved() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut bread = MarketGood::new().with_amv(1.0).with_salability(1.0);
        bread.traded = 4.0;
        bread.print_units = 4.0;
        bread.print_holding = 4.0;
        market.goods.insert(2, bread);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&2].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&2].salability - 1.0).abs() < 1e-9);
        assert_eq!(market.goods[&2].print_units, 0.0);
    }

    /// The night closes a tenth of the gap when that is inside the cap.
    #[test]
    fn record_keeping_closes_a_tenth_of_the_print_gap() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut bread = MarketGood::new().with_amv(1.0).with_salability(1.0);
        bread.traded = 4.0;
        bread.print_units = 4.0;
        bread.print_holding = 6.0;
        market.goods.insert(2, bread);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&2].amv - 1.05).abs() < 1e-9);
    }

    /// A low salability stores the same holding move as a larger AMV step.
    ///
    /// A tenth of this gap is 0.1 of holding, and the step cap cuts that to
    /// 0.05. On a 0.5 scale that 0.05 is 0.1 of AMV.
    #[test]
    fn record_keeping_applies_the_print_in_holding_space() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut bread = MarketGood::new().with_amv(2.0).with_salability(0.5);
        bread.traded = 1.0;
        bread.print_units = 1.0;
        bread.print_holding = 2.0;
        market.goods.insert(2, bread);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&2].amv - 2.1).abs() < 1e-9);
        // Bought, and the price did not fall, so salability steps toward par.
        assert!((market.goods[&2].salability - 0.55).abs() < 1e-9);
    }

    /// The print step cannot exceed 0.05 of the payment scale.
    ///
    /// The coin's holding value is 4, so the cap is 0.2. The coin did not
    /// trade, so bread lands at 1.2 and the coin stays at 4.
    #[test]
    fn record_keeping_caps_the_print_step_at_the_payment_scale() {
        let mut market = Market::new(1);
        market.currencies = [Some(9), None, None, None];
        let mut bread = MarketGood::new().with_amv(1.0).with_salability(1.0);
        bread.traded = 1.0;
        bread.print_units = 1.0;
        bread.print_holding = 100.0;
        let mut coin = MarketGood::new().with_amv(4.0).with_salability(1.0);
        coin.paid = 3.0;
        market.goods.insert(2, bread);
        market.goods.insert(9, coin);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&2].amv - 1.2).abs() < 1e-9);
        assert!((market.goods[&9].amv - 4.0).abs() < 1e-9);
        assert!((market.goods[&9].salability - 1.05).abs() < 1e-9);
    }

    /// Nothing was paid and nothing traded, so a high salability does not
    /// restate the card and the unsold good keeps its AMV.
    #[test]
    fn record_keeping_does_not_restate_a_high_salability_good() {
        let mut market = Market::new(1);
        let mut grain = MarketGood::new().with_amv(2.0).with_salability(1.0);
        grain.offered_unsold = 10.0;
        market.goods.insert(1, grain);
        market
            .goods
            .insert(2, MarketGood::new().with_amv(4.0).with_salability(1.5));
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&2].amv - 4.0).abs() < 1e-9);
        assert!((market.goods[&2].salability - 1.5).abs() < 1e-9);
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.0).abs() < 1e-9);
    }

    /// The stored currency traded, and its own day wrote nothing else, so it
    /// stays at 8. The other good did not trade and keeps the AMV it opened at.
    #[test]
    fn record_keeping_keeps_a_traded_currency_at_its_own_write() {
        let mut market = Market::new(1);
        let mut coin = MarketGood::new().with_amv(8.0).with_salability(1.0);
        coin.traded = 1.0;
        market.goods.insert(5, coin);
        market
            .goods
            .insert(2, MarketGood::new().with_amv(4.0).with_salability(1.0));
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&5].amv - 8.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 4.0).abs() < 1e-9);
    }

    /// Two currencies that both traded each keep the AMV their own day wrote.
    /// Payment still raises both salabilities.
    #[test]
    fn record_keeping_keeps_each_traded_currency_at_its_own_write() {
        let mut market = Market::new(1);
        market.currencies = [Some(5), Some(2), None, None];
        let mut small = MarketGood::new().with_amv(4.0).with_salability(1.0);
        small.paid = 1.0;
        small.traded = 1.0;
        let mut large = MarketGood::new().with_amv(8.0).with_salability(1.0);
        large.paid = 3.0;
        large.traded = 1.0;
        market.goods.insert(2, small);
        market.goods.insert(5, large);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&5].amv - 8.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 4.0).abs() < 1e-9);
        assert!((market.goods[&5].salability - 1.05).abs() < 1e-9);
        assert!((market.goods[&2].salability - 1.05).abs() < 1e-9);
    }

    /// An offered book that does not trade walks the ask a quarter of the way
    /// toward the card. A later fill walks a quarter of the way back.
    /// A book with no offer does not walk.
    #[test]
    fn record_keeping_walks_a_refused_ask_and_eases_a_fill() {
        let mut market = Market::new(1);
        market.goods.insert(
            OPENING_CURRENCY,
            MarketGood::new().with_amv(1.0).with_salability(1.0),
        );
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 0.0));
        market.mass_closes.push(idle_close(2, 0.0, 5.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.history().ask_walk(1) - 0.25).abs() < 1e-12);
        assert_eq!(market.history().ask_walk(2), 0.0);

        market.mass_closes = vec![idle_close(1, 10.0, 4.0, 4.0)];
        market.record_keeping(&Factuals::new());
        assert!((market.history().ask_walk(1) - 0.1875).abs() < 1e-12);
    }

    /// Process use does not move an input that did not trade.
    ///
    /// Ten units carry forty of output value. The opening price stays 0.2.
    /// The credit is still cleared.
    #[test]
    fn record_keeping_holds_process_use_when_the_input_did_not_trade() {
        let mut market = Market::new(1);
        let mut input = MarketGood::new().with_amv(0.2).with_salability(1.0);
        input.process_used = 10.0;
        input.process_value = 40.0;
        market.goods.insert(1, input);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 0.2).abs() < 1e-9);
        assert_eq!(market.goods[&1].process_used, 0.0);
    }

    /// An equal desire share keeps half of a process gap that fits under the cap.
    ///
    /// One unit traded, so the pull is written. The target is 1.06. Half of
    /// the 0.06 gap is 0.03, so 1.0 lands on 1.03.
    #[test]
    fn record_keeping_lets_desire_use_hold_half_the_process_gap() {
        let mut market = Market::new(1);
        let mut input = MarketGood::new().with_amv(1.0).with_salability(1.0);
        input.process_used = 10.0;
        input.process_value = 10.6;
        input.desire_used = 10.0;
        input.traded = 1.0;
        market.goods.insert(1, input);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.03).abs() < 1e-9);
    }

    /// A recorded clear pulls by its share. Desire use of twice that volume holds the rest.
    ///
    /// Three units cleared at 1, and six were eaten. From 4 the night lands on 3.
    /// The fall is past the salability loss cap, so salability loses only that cap.
    #[test]
    fn record_keeping_pulls_a_clear_toward_its_price_by_the_trade_share() {
        let mut market = Market::new(1);
        let mut input = MarketGood::new().with_amv(4.0).with_salability(1.0);
        input.clear_units = 3.0;
        input.clear_value = 3.0;
        input.desire_used = 6.0;
        market.goods.insert(1, input);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 3.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.99).abs() < 1e-9);
    }

    /// Three units cleared at 1, and the book still wants six against one
    /// offered. Buyers could pay. The clear would pull 4 down to 1, so the
    /// morning ask stays.
    #[test]
    fn record_keeping_keeps_a_scarce_ask_from_the_clear() {
        let mut market = Market::new(1);
        let mut good = MarketGood::new().with_amv(4.0).with_salability(1.0);
        good.clear_units = 3.0;
        good.clear_value = 3.0;
        good.traded = 3.0;
        market.goods.insert(1, good);
        market.mass_closes.push(idle_close(1, 1.0, 6.0, 3.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 4.0).abs() < 1e-9);
    }

    /// One unit traded, so rot still takes its cut. The decayed units then
    /// shrink the process pull.
    ///
    /// One percent of a thousand-unit pile is a quarter of a percent of the
    /// price, so 2 becomes 1.995 before the pull. Process use is half the
    /// accounted day and its output value per unit is 2.035, under the cap,
    /// so the night moves halfway and lands on 2.015.
    #[test]
    fn record_keeping_lets_rot_shrink_the_process_pull() {
        let mut market = Market::new(1);
        let mut input = MarketGood::new().with_amv(2.0).with_salability(1.0);
        input.process_used = 10.0;
        input.process_value = 20.35;
        input.traded = 1.0;
        market.goods.insert(1, input);
        market.note_decay(1, 10.0, 1000.0);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.015).abs() < 1e-9);
    }

    /// Desire use alone leaves the rot cut as the only move. One unit traded,
    /// so that cut is written.
    #[test]
    fn record_keeping_holds_a_price_when_the_day_was_desire() {
        let mut market = Market::new(1);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.0);
        good.desire_used = 5.0;
        good.traded = 1.0;
        market.goods.insert(1, good);
        market.note_decay(1, 1.0, 10.0);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.95).abs() < 1e-9);
    }

    /// One grain becomes one bread priced at 4, and that bread cleared.
    /// The night stores that credit. Grain itself did not trade, so its
    /// AMV stays.
    #[test]
    fn gather_use_prices_a_process_input_at_its_output() {
        let factuals = Factuals::new().with_process(
            Process::new(7, "bake", 0)
                .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(2, 1.0, true)),
        );
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);
        let mut job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        job.reserve(&mut property, &factuals);
        job.produce(&mut property, &factuals);
        let mut pop = Pop::new(1);
        pop.job = job;
        pop.property = property;

        let mut market = Market::new(1);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(0.2).with_salability(1.0));
        let mut bread = MarketGood::new().with_amv(4.0).with_salability(1.0);
        bread.clear_units = 1.0;
        bread.clear_value = 4.0;
        market.goods.insert(2, bread);
        market.pops.insert(1);
        let mut actors = Actors::new();
        actors.pops.insert(1, pop);

        market.gather_use(&actors);

        assert!((market.goods[&1].process_used - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].process_value - 4.0).abs() < 1e-9);
        assert!(market.goods[&1].consumption.abs() < 1e-9);
        market.record_keeping(&factuals);
        assert!((market.goods[&1].amv - 0.2).abs() < 1e-9);
        assert!((market.goods[&2].amv - 4.0).abs() < 1e-9);
    }

    /// The same run, with the bread unsold, does not price the grain.
    #[test]
    fn gather_use_ignores_an_output_that_did_not_clear() {
        let factuals = Factuals::new().with_process(
            Process::new(7, "bake", 0)
                .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(2, 1.0, true)),
        );
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);
        let mut job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        job.reserve(&mut property, &factuals);
        job.produce(&mut property, &factuals);
        let mut pop = Pop::new(1);
        pop.job = job;
        pop.property = property;

        let mut market = Market::new(1);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(0.2).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(4.0).with_salability(1.0));
        market.pops.insert(1);
        let mut actors = Actors::new();
        actors.pops.insert(1, pop);

        market.gather_use(&actors);
        assert_eq!(market.goods[&1].process_used, 0.0);
        market.record_keeping(&factuals);
        assert!((market.goods[&1].amv - 0.2).abs() < 1e-9);
    }

    /// Two leftover units are desire use. They hold the price and do not
    /// enter the production flow.
    #[test]
    fn gather_use_counts_rest_as_desire() {
        let mut pop = Pop::new(1);
        pop.property.insert(0, PopPRow::new(2.0));
        pop.desires[0].push(Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![],
            amount: 1.0,
            satisfaction: 0.0,
            target_taken: vec![],
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        });
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_sink(0, 0.05));
        pop.rest(&factuals);

        let mut market = Market::new(1);
        market
            .goods
            .insert(0, MarketGood::new().with_amv(0.2).with_salability(1.0));
        market.pops.insert(1);
        let mut actors = Actors::new();
        actors.pops.insert(1, pop);

        market.gather_use(&actors);
        assert!((market.goods[&0].desire_used - 2.0).abs() < 1e-9);
        assert!(market.goods[&0].consumption.abs() < 1e-9);
        market.record_keeping(&factuals);

        assert!((market.goods[&0].amv - 0.2).abs() < 1e-9);
    }

    /// # Insert Salability Anchor
    ///
    /// Inserts good 0 at AMV 1 and [`SALABILITY_MAX`].
    ///
    /// Good 0 anchors salability. It does not trade, so the other good keeps
    /// the AMV this night's step wrote.
    fn insert_salability_anchor(market: &mut Market) {
        market.goods.insert(
            0,
            MarketGood::new()
                .with_amv(1.0)
                .with_salability(SALABILITY_MAX),
        );
    }

    /// A pop with one desire at `sol` levels, `count` households, and `qty` of good 1.
    fn mover(id: usize, count: f64, sol: f64, qty: f64) -> Pop {
        let mut pop = Pop::new(id);
        pop.demographics.household.count = count;
        pop.property.insert(1, PopPRow::new(qty));
        pop.desires[0].push(Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![],
            amount: 1.0,
            satisfaction: sol,
            target_taken: vec![],
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        });
        pop
    }

    fn seat(market: &mut Market, actors: &mut Actors, pop: Pop) {
        market.pops.insert(pop.id);
        actors.pops.insert(pop.id, pop);
    }

    /// A later pop with the same demographic and the same processes joins the earlier one.
    ///
    /// Line order does not matter. The earlier targets stay. Working stock,
    /// used stock, and the saved meal travel. A different process stays.
    /// An empty job is not a match.
    #[test]
    fn fold_duplicate_pops_sends_the_later_pop_into_the_earlier_job() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut first = mover(1, 4.0, 1.0, 10.0);
        first.job = Job::new(
            1,
            vec![
                JobLine::new(29, Some(3.0), vec![]),
                JobLine::new(30, Some(1.0), vec![]),
            ],
        );
        let mut other = mover(2, 3.0, 1.0, 1.0);
        other.job = Job::new(1, vec![JobLine::new(5, Some(1.0), vec![])]);
        let mut later = mover(3, 2.0, 1.0, 6.0);
        later.job = Job::new(
            0,
            vec![
                JobLine::new(30, Some(9.0), vec![]),
                JobLine::new(29, None, vec![]),
            ],
        );
        later.property.get_mut(&1).unwrap().used = 1.5;
        later.saved.insert(4, 2.0);
        later.meal.insert(4, 2.0);
        later.job.input_hold.insert(4, 0.5);
        later.migration = -1.0;
        let mut blank = mover(4, 2.0, 1.0, 8.0);
        blank.job = Job::new(0, vec![]);
        let mut also_blank = mover(5, 2.0, 1.0, 8.0);
        also_blank.job = Job::new(0, vec![]);
        seat(&mut market, &mut actors, first);
        seat(&mut market, &mut actors, other);
        seat(&mut market, &mut actors, later);
        seat(&mut market, &mut actors, blank);
        seat(&mut market, &mut actors, also_blank);

        market.fold_duplicate_pops(&mut actors);

        assert!(actors.pops.get(&3).is_none());
        assert!(!market.pops.contains(&3));
        assert!((actors.pop(1).demographics.household.count - 6.0).abs() < 1e-9);
        assert!((actors.pop(1).migration - 2.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 16.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].used - 1.5).abs() < 1e-9);
        assert!((actors.pop(1).saved[&4] - 2.0).abs() < 1e-9);
        assert!((actors.pop(1).meal[&4] - 2.0).abs() < 1e-9);
        assert!((actors.pop(1).job.input_hold[&4] - 0.5).abs() < 1e-9);
        let lines: Vec<(usize, Option<f64>)> = actors
            .pop(1)
            .job
            .lines
            .iter()
            .map(|line| (line.process, line.target))
            .collect();
        assert_eq!(lines, vec![(29, Some(3.0)), (30, Some(1.0))]);
        assert!((actors.pop(2).demographics.household.count - 3.0).abs() < 1e-9);
        assert!((actors.pop(4).demographics.household.count - 2.0).abs() < 1e-9);
        assert!((actors.pop(5).demographics.household.count - 2.0).abs() < 1e-9);
        assert_eq!(
            market.pop_removals(),
            &[PopRemoval {
                id: 3,
                why: PopExit::SameJob { to: vec![1] },
            }]
        );
    }

    /// A different species keeps its own pop, even on the same processes.
    #[test]
    fn fold_duplicate_pops_leaves_a_different_species() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut first = mover(1, 4.0, 1.0, 10.0);
        first.job = Job::new(1, vec![JobLine::new(29, Some(1.0), vec![])]);
        let mut later = mover(2, 2.0, 1.0, 6.0);
        later.demographics.species = 2;
        later.job = Job::new(1, vec![JobLine::new(29, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, first);
        seat(&mut market, &mut actors, later);

        market.fold_duplicate_pops(&mut actors);

        assert!((actors.pop(1).demographics.household.count - 4.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 2.0).abs() < 1e-9);
        assert!(market.pop_removals().is_empty());
    }

    /// One point under the average. Forty people offer less than fifty can
    /// take, once both sides scale by log2 of their people, so the offer
    /// moves. Half of that share of the stock goes along.
    #[test]
    fn internal_migration_moves_the_log_offer_for_one_point_of_living() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.demographics.household.adult = 4.0;
        origin.demographics.household.elder = 0.0;
        origin.demographics.household.child = 0.0;
        origin.demographics.household.adult_mf = 1.0;
        origin.demographics.household.adult_labor = 0.0;
        origin.demographics.household.partnership_rate = 4.0;
        origin.property.get_mut(&1).unwrap().used = 8.0;
        origin.property.get_mut(&1).unwrap().fresh = 40.0;
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.demographics.household.adult_mf = 0.5;
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        let origin = actors.pop(1);
        let dest = actors.pop(2);
        // Push 1 is a twentieth. Forty people scale it. The room on fifty is larger.
        let moved = 10.0 * (0.05 * 40.0_f64.log2()).min(1.0);
        let arrive = 10.0 + moved;
        let goods = 0.5 * moved / 10.0 * 200.0;
        let used = 0.5 * moved / 10.0 * 8.0;
        let fresh = 0.5 * moved / 10.0 * 40.0;
        assert!((origin.demographics.household.count - (10.0 - moved)).abs() < 1e-9);
        assert!((dest.demographics.household.count - arrive).abs() < 1e-9);
        assert!((origin.property[&1].quantity - (200.0 - goods)).abs() < 1e-9);
        assert!((dest.property[&1].quantity - (200.0 + goods)).abs() < 1e-9);
        assert!((origin.property[&1].used - (8.0 - used)).abs() < 1e-9);
        assert!((dest.property[&1].used - used).abs() < 1e-9);
        assert!((origin.property[&1].fresh - (40.0 - fresh)).abs() < 1e-9);
        assert_eq!(dest.property[&1].fresh, 0.0);
        assert!((origin.demographics.household.adult - 4.0).abs() < 1e-9);
        let adults = 20.0 + 4.0 * moved;
        assert!((dest.demographics.household.adult - adults / arrive).abs() < 1e-9);
        assert!((dest.demographics.household.adult_mf - (10.0 + 4.0 * moved) / adults).abs() < 1e-9);
        assert!((dest.demographics.household.elder - 5.0 / arrive).abs() < 1e-9);
        assert!((dest.demographics.household.child - 25.0 / arrive).abs() < 1e-9);
        assert!((dest.demographics.household.adult_labor - 10.0 / arrive).abs() < 1e-9);
        assert!(
            (dest.demographics.household.partnership_rate - (25.0 + 4.0 * moved) / arrive).abs()
                < 1e-9
        );
        assert!((origin.migration - -moved).abs() < 1e-9);
        assert!((dest.migration - moved).abs() < 1e-9);
    }

    /// Equal living and equal wealth sends nobody.
    #[test]
    fn internal_migration_stays_home_when_the_pops_match() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        seat(&mut market, &mut actors, mover(1, 10.0, 5.0, 200.0));
        seat(&mut market, &mut actors, mover(2, 10.0, 5.0, 200.0));

        market.internal_migration(&mut actors, &Factuals::new());

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 200.0).abs() < 1e-9);
        assert_eq!(actors.pop(1).migration, 0.0);
        assert_eq!(actors.pop(2).migration, 0.0);
    }

    /// Fear raises the offer. Both pops have fifty people. Hope's room is
    /// larger than that offer, so the offer is what arrives.
    #[test]
    fn internal_migration_fear_sends_more_households() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 100.0);
        origin.sentiment = Sentiment::from_parts(0.0, 0.5, 0.0, 0.5, 0.0);
        let mut dest = mover(2, 10.0, 6.0, 100.0);
        dest.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 0.0, 1.0);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        // Push 1.5 saturates at 0.06. Fifty people scale it, under hope's room.
        let moved = 10.0 * (0.06 * 50.0_f64.log2()).min(1.0);
        assert!((actors.pop(1).demographics.household.count - (10.0 - moved)).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - (10.0 + moved)).abs() < 1e-9);
        let goods = 0.5 * moved / 10.0 * 100.0;
        assert!((actors.pop(1).property[&1].quantity - (100.0 - goods)).abs() < 1e-6);
        assert!((actors.pop(2).property[&1].quantity - (100.0 + goods)).abs() < 1e-6);
    }

    /// Equal goods per household. The origin has twice the people, so it is
    /// poorer per person, and a different craft more than 10% richer stays closed.
    #[test]
    fn internal_migration_closes_a_different_craft_on_wealth_per_person() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.demographics.household.adult = 8.0;
        origin.demographics.household.elder = 0.0;
        origin.demographics.household.child = 0.0;
        origin.job = Job::new(1, vec![]);
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.demographics.household.adult = 4.0;
        dest.demographics.household.elder = 0.0;
        dest.demographics.household.child = 0.0;
        dest.job = Job::new(2, vec![]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 200.0).abs() < 1e-9);
    }

    /// A worse trade score pushes. The offer grows with log2 of the people.
    /// A stopped line with a fatter output does not count. The destination
    /// is large enough that its room does not bind.
    ///
    /// Outputs of 2 and 8, at complexity 1 and holding 1, score 1 and 7.
    /// The mean is 4. Three points square to 9, and the pace takes 0.09.
    #[test]
    fn internal_migration_sends_households_toward_the_more_profitable_job() {
        let mut market = Market::new(1);
        market.goods.insert(
            9,
            MarketGood::new().with_amv(1.0).with_salability(1.0),
        );
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 5.0, 50.0);
        origin.job = Job::new(
            1,
            vec![
                JobLine::new(12, Some(0.0), vec![]),
                JobLine::new(10, Some(1.0), vec![]),
            ],
        );
        let mut dest = mover(2, 100.0, 5.0, 500.0);
        dest.job = Job::new(1, vec![JobLine::new(11, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);
        let factuals = Factuals::new()
            .with_process(
                Process::new(10, "small", 0).with_output(ProcessOutput::new(9, 2.0, true)),
            )
            .with_process(
                Process::new(11, "large", 0).with_output(ProcessOutput::new(9, 8.0, true)),
            )
            .with_process(
                Process::new(12, "idle", 0).with_output(ProcessOutput::new(9, 100.0, true)),
            );

        market.internal_migration(&mut actors, &factuals);

        let offer = 10.0 * (0.09 * 50.0_f64.log2()).min(1.0);
        assert!((actors.pop(1).demographics.household.count - (10.0 - offer)).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - (100.0 + offer)).abs() < 1e-9);
        let goods = offer / 10.0 * 0.5 * 50.0;
        assert!((actors.pop(1).property[&1].quantity - (50.0 - goods)).abs() < 1e-6);
        assert!((actors.pop(2).property[&1].quantity - (500.0 + goods)).abs() < 1e-6);
    }

    /// A different craft cannot enter a pop more than 10% richer.
    #[test]
    fn internal_migration_closes_a_different_craft_above_ten_percent_richer() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.job = Job::new(1, vec![]);
        let mut dest = mover(2, 10.0, 6.0, 400.0);
        dest.job = Job::new(2, vec![]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 200.0).abs() < 1e-9);
    }

    /// A shared craft may move up, and the room shrinks by the wealth ratio.
    #[test]
    fn internal_migration_shrinks_an_upward_move_by_the_wealth_ratio() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.job = Job::new(1, vec![]);
        let mut dest = mover(2, 10.0, 6.0, 400.0);
        dest.job = Job::new(1, vec![]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        // Twice the wealth per person closes half the log room, under the offer.
        let moved = 10.0 * (0.05 * 50.0_f64.log2()).min(1.0) * 0.5;
        let goods = 0.5 * moved / 10.0 * 200.0;
        assert!((actors.pop(1).demographics.household.count - (10.0 - moved)).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - (10.0 + moved)).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - (200.0 - goods)).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - (400.0 + goods)).abs() < 1e-9);
    }

    /// An origin with no wealth does not move into a richer pop.
    #[test]
    fn internal_migration_blocks_upward_from_zero_wealth() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 0.0);
        origin.job = Job::new(1, vec![]);
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.job = Job::new(1, vec![]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
    }

    /// No running line and no whole coin sends the whole pop. Dust is not a coin.
    /// A richer different craft would have closed the paced gate.
    #[test]
    fn internal_migration_sends_an_idle_broke_pop_all_at_once() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 8.0, 4.0, 0.0);
        origin.property.insert(2, PopPRow::new(80.0));
        origin
            .property
            .insert(OPENING_CURRENCY, PopPRow::new(0.4));
        origin.job = Job::new(1, vec![JobLine::new(9, Some(0.0), vec![])]);
        let mut dest = mover(2, 4.0, 6.0, 0.0);
        dest.property.insert(3, PopPRow::new(400.0));
        dest.job = Job::new(2, vec![JobLine::new(9, Some(2.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!(actors.pops.get(&1).is_none());
        assert!(!market.pops.contains(&1));
        assert!((actors.pop(2).demographics.household.count - 12.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&2].quantity - 40.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&3].quantity - 400.0).abs() < 1e-9);
        assert!((actors.pop(2).migration - 8.0).abs() < 1e-9);
    }

    /// A line that is still set to run stays on the paced gate, even with no coin.
    #[test]
    fn internal_migration_keeps_a_broke_pop_that_is_still_producing() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 0.0);
        origin.job = Job::new(
            1,
            vec![
                JobLine::new(1, Some(0.0), vec![]),
                JobLine::new(2, Some(1.0), vec![]),
            ],
        );
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.job = Job::new(2, vec![JobLine::new(1, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
    }

    /// A target of `None` is still work, so a broke pop does not evacuate.
    #[test]
    fn internal_migration_keeps_a_broke_pop_whose_line_has_no_target() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 0.0);
        origin.job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.job = Job::new(2, vec![JobLine::new(1, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
    }

    /// A saved whole coin keeps an idle pop on the paced share.
    #[test]
    fn internal_migration_keeps_an_idle_pop_that_holds_a_coin() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), None, None, None];
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 0.0);
        origin.saved.insert(1, 2.0);
        origin.property.insert(2, PopPRow::new(100.0));
        let mut dest = mover(2, 10.0, 6.0, 0.0);
        dest.property.insert(2, PopPRow::new(102.0));
        dest.job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!(actors.pops.get(&1).is_some());
        let moved = 10.0 * (0.05 * 50.0_f64.log2()).min(1.0);
        assert!((actors.pop(1).demographics.household.count - (10.0 - moved)).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - (10.0 + moved)).abs() < 1e-9);
    }

    /// An idle broke pop does not leave onto another species.
    #[test]
    fn internal_migration_does_not_evacuate_across_species() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 8.0, 4.0, 0.0);
        origin.demographics.species = 1;
        origin.job = Job::new(1, vec![JobLine::new(1, Some(0.0), vec![])]);
        let mut dest = mover(2, 4.0, 6.0, 400.0);
        dest.demographics.species = 2;
        dest.job = Job::new(2, vec![JobLine::new(1, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!((actors.pop(1).demographics.household.count - 8.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 4.0).abs() < 1e-9);
    }

    /// The only pop is idle and broke, so it takes the subsistence work it can run.
    #[test]
    fn internal_migration_retools_the_last_idle_pop_onto_subsistence() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 6.0, 4.0, 0.0);
        origin.job = Job::new(0, vec![JobLine::new(4, Some(0.0), vec![])]);
        seat(&mut market, &mut actors, origin);

        market.internal_migration(&mut actors, &runnable_factuals());

        assert!(market.pops.contains(&1));
        assert!((actors.pop(1).demographics.household.count - 6.0).abs() < 1e-9);
        assert_eq!(actors.pop(1).job.craft, 1);
        let lines: Vec<usize> = actors.pop(1).job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![29, 30, 31]);
        assert!(actors.pop(1).job.lines.iter().all(|line| line.target.is_none()));
    }

    /// Two dead pops of different species cannot seat each other, so both switch.
    #[test]
    fn internal_migration_retools_dead_pops_that_cannot_cross_species() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 8.0, 4.0, 0.0);
        origin.demographics.species = 1;
        origin.job = Job::new(0, vec![JobLine::new(4, Some(0.0), vec![])]);
        let mut other = mover(2, 4.0, 4.0, 0.0);
        other.demographics.species = 2;
        other.job = Job::new(0, vec![JobLine::new(4, Some(0.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, other);

        market.internal_migration(&mut actors, &runnable_factuals());

        assert!((actors.pop(1).demographics.household.count - 8.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 4.0).abs() < 1e-9);
        for id in [1, 2] {
            let lines: Vec<usize> = actors
                .pop(id)
                .job
                .lines
                .iter()
                .map(|line| line.process)
                .collect();
            assert_eq!(lines, vec![29, 30, 31]);
            assert!(
                actors
                    .pop(id)
                    .job
                    .lines
                    .iter()
                    .all(|line| line.target.is_none())
            );
        }
    }

    /// Already on subsistence, and still dead. Grain bread is skipped. Spin uses time.
    #[test]
    fn internal_migration_cycles_a_dead_subsistence_pop_past_a_job_it_cannot_feed() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 6.0, 4.0, 0.0);
        origin.job = subsistence_job();
        seat(&mut market, &mut actors, origin);

        market.internal_migration(&mut actors, &recovery_factuals());

        assert!((actors.pop(1).demographics.household.count - 6.0).abs() < 1e-9);
        let lines: Vec<usize> = actors.pop(1).job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![33]);
        assert!(actors.pop(1).job.lines[0].target.is_none());
    }

    /// Grain on hand makes the bread process the next job. Spin stays later.
    #[test]
    fn internal_migration_cycles_a_dead_subsistence_pop_onto_a_good_it_holds() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 6.0, 4.0, 5.0);
        origin.job = subsistence_job();
        seat(&mut market, &mut actors, origin);

        market.internal_migration(&mut actors, &recovery_factuals());

        assert!((actors.pop(1).demographics.household.count - 6.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 5.0).abs() < 1e-9);
        let lines: Vec<usize> = actors.pop(1).job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![32]);
        assert!(actors.pop(1).job.lines[0].target.is_none());
    }

    /// The whole count leaves, split by the demographic gap, not shrunk by it.
    #[test]
    fn internal_migration_splits_an_evacuation_by_the_demographic_gap() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 9.0, 4.0, 0.0);
        origin.demographics.culture = 1;
        origin.demographics.stratum = 1;
        origin.demographics.religion = 1;
        origin.job = Job::new(1, vec![JobLine::new(1, Some(0.0), vec![])]);
        let mut near = mover(2, 4.0, 6.0, 200.0);
        near.demographics.culture = 1;
        near.demographics.stratum = 1;
        near.demographics.religion = 1;
        near.job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        let mut far = mover(3, 4.0, 6.0, 200.0);
        far.demographics.culture = 2;
        far.demographics.stratum = 2;
        far.demographics.religion = 2;
        far.job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, near);
        seat(&mut market, &mut actors, far);
        let factuals = Factuals::new()
            .with_culture(Culture::new(1, "open"))
            .with_culture(Culture::new(2, "open"))
            .with_religion(Religion::new(1, "open"))
            .with_religion(Religion::new(2, "open"));

        market.internal_migration(&mut actors, &factuals);

        assert!(actors.pops.get(&1).is_none());
        assert!((actors.pop(2).demographics.household.count - 10.0).abs() < 1e-9);
        assert!((actors.pop(3).demographics.household.count - 7.0).abs() < 1e-9);
    }

    /// Two open cultures that share no demographic id do not exchange households.
    #[test]
    fn internal_migration_blocks_a_full_demographic_gap() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.demographics.species = 1;
        origin.demographics.culture = 1;
        origin.demographics.stratum = 1;
        origin.demographics.religion = 1;
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.demographics.species = 2;
        dest.demographics.culture = 2;
        dest.demographics.stratum = 2;
        dest.demographics.religion = 2;
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);
        let factuals = Factuals::new()
            .with_culture(Culture::new(1, "open"))
            .with_culture(Culture::new(2, "also open"))
            .with_religion(Religion::new(1, "open"))
            .with_religion(Religion::new(2, "also open"));

        market.internal_migration(&mut actors, &factuals);

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
    }

    /// Culture, stratum, and religion differ. Species matches, so the gate stays open.
    #[test]
    fn internal_migration_caps_a_partial_demographic_gap() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.demographics.culture = 1;
        origin.demographics.stratum = 1;
        origin.demographics.religion = 1;
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.demographics.culture = 2;
        dest.demographics.stratum = 2;
        dest.demographics.religion = 2;
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);
        let factuals = Factuals::new()
            .with_culture(Culture::new(1, "open"))
            .with_culture(Culture::new(2, "open"))
            .with_religion(Religion::new(1, "open"))
            .with_religion(Religion::new(2, "open"));

        market.internal_migration(&mut actors, &factuals);

        // Distance 3 of 4, species matched. Both eases are open, so the mean is 2.
        // Cap is 2 * 1/4 = 0.5, so half of the log-scaled room.
        let moved = 10.0 * (0.05 * 50.0_f64.log2()).min(1.0) * 0.5;
        assert!((actors.pop(1).demographics.household.count - (10.0 - moved)).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - (10.0 + moved)).abs() < 1e-9);
    }

    /// A different species stays put even when the living gap would move households.
    #[test]
    fn internal_migration_does_not_cross_species() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.demographics.species = 1;
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.demographics.species = 2;
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.0).abs() < 1e-9);
    }

    /// Insular cultures still move when conversion is open. The mean ease is 1,
    /// and one mismatch leaves three of the four ids alike, so the cap is 0.75.
    #[test]
    fn internal_migration_open_conversion_eases_an_insular_culture() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.demographics.culture = 1;
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.demographics.culture = 2;
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);
        let factuals = Factuals::new()
            .with_culture(Culture::new(1, "shut").with_assimilation(0.0))
            .with_culture(Culture::new(2, "shut").with_assimilation(0.0));

        market.internal_migration(&mut actors, &factuals);

        // Mean ease 1 and three of four ids alike cap the log room at 0.75.
        let moved = 10.0 * (0.05 * 50.0_f64.log2()).min(1.0) * 0.75;
        assert!((actors.pop(1).demographics.household.count - (10.0 - moved)).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - (10.0 + moved)).abs() < 1e-9);
    }

    /// Closed conversion shrinks a religion-only gap the same way.
    #[test]
    fn internal_migration_closed_conversion_shrinks_a_religion_gap() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.demographics.religion = 1;
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.demographics.religion = 2;
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);
        let factuals = Factuals::new()
            .with_religion(Religion::new(1, "shut").with_conversion(0.0))
            .with_religion(Religion::new(2, "shut").with_conversion(0.0));

        market.internal_migration(&mut actors, &factuals);

        let moved = 10.0 * (0.05 * 50.0_f64.log2()).min(1.0) * 0.75;
        assert!((actors.pop(1).demographics.household.count - (10.0 - moved)).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - (10.0 + moved)).abs() < 1e-9);
    }

    /// Both eases at 0 close the gate on a single mismatched id.
    #[test]
    fn internal_migration_insular_culture_and_religion_do_not_cross() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 10.0, 4.0, 200.0);
        origin.demographics.culture = 1;
        origin.demographics.religion = 1;
        let mut dest = mover(2, 10.0, 6.0, 200.0);
        dest.demographics.culture = 2;
        dest.demographics.religion = 2;
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);
        let factuals = Factuals::new()
            .with_culture(Culture::new(1, "shut").with_assimilation(0.0))
            .with_culture(Culture::new(2, "shut").with_assimilation(0.0))
            .with_religion(Religion::new(1, "shut").with_conversion(0.0))
            .with_religion(Religion::new(2, "shut").with_conversion(0.0));

        market.internal_migration(&mut actors, &factuals);

        assert!((actors.pop(1).demographics.household.count - 10.0).abs() < 1e-9);
    }

    /// A pop of one household that finds a seat leaves entirely, and half its goods arrive.
    #[test]
    fn internal_migration_sends_the_whole_pop_when_the_rest_would_be_under_one() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        seat(&mut market, &mut actors, mover(1, 1.0, 0.0, 40.0));
        seat(&mut market, &mut actors, mover(2, 10.0, 2.0, 0.0));

        market.internal_migration(&mut actors, &Factuals::new());

        assert!(actors.pops.get(&1).is_none());
        assert!(!market.pops.contains(&1));
        assert_eq!(
            market.pop_removals(),
            &[PopRemoval {
                id: 1,
                why: PopExit::NoWorkNoCoin { to: vec![2] },
            }]
        );
        assert!((actors.pop(2).demographics.household.count - 11.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 20.0).abs() < 1e-9);
    }

    /// One household stays while its bread is still sought. The same shop
    /// leaves when the book seeks nothing. Twenty-five households still
    /// offer. An idle pop with no coin still leaves while bread is sought.
    #[test]
    fn internal_migration_keeps_a_small_shop_while_its_output_is_sought() {
        let bread = Process::new(3, "make bread", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(3, 12.0, false));
        let factuals = Factuals::new().with_process(bread);

        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut shop = mover(1, 1.0, 4.0, 40.0);
        shop.job = Job::new(1, vec![JobLine::new(3, Some(2.0), vec![])]);
        let mut town = mover(2, 20.0, 6.0, 40.0);
        town.job = Job::new(1, vec![JobLine::new(3, Some(2.0), vec![])]);
        seat(&mut market, &mut actors, shop);
        seat(&mut market, &mut actors, town);
        let mut card = MarketGood::new();
        card.sought_unmet = 30.0;
        market.goods.insert(3, card);

        market.internal_migration(&mut actors, &factuals);

        assert!((actors.pop(1).demographics.household.count - 1.0).abs() < 1e-9);
        assert!(actors.pop(1).migration.abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 20.0).abs() < 1e-9);
    }

    /// Nothing is sought, so the one household still leaves whole.
    #[test]
    fn internal_migration_sends_a_small_shop_when_nothing_is_sought() {
        let bread = Process::new(3, "make bread", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(3, 12.0, false));
        let factuals = Factuals::new().with_process(bread);
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut shop = mover(1, 1.0, 4.0, 40.0);
        shop.job = Job::new(1, vec![JobLine::new(3, Some(2.0), vec![])]);
        let mut town = mover(2, 20.0, 6.0, 40.0);
        town.job = Job::new(1, vec![JobLine::new(3, Some(2.0), vec![])]);
        seat(&mut market, &mut actors, shop);
        seat(&mut market, &mut actors, town);

        market.internal_migration(&mut actors, &factuals);

        assert!(actors.pops.get(&1).is_none());
        assert_eq!(
            market.pop_removals(),
            &[PopRemoval {
                id: 1,
                why: PopExit::UnderOneMove { to: vec![2] },
            }]
        );
    }

    /// Twenty-five households are past the shelter. The offer still leaves.
    #[test]
    fn internal_migration_still_sends_a_shop_at_the_founding_size() {
        let bread = Process::new(3, "make bread", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(3, 12.0, false));
        let factuals = Factuals::new().with_process(bread);
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut shop = mover(1, 25.0, 4.0, 40.0);
        shop.job = Job::new(1, vec![JobLine::new(3, Some(2.0), vec![])]);
        let mut town = mover(2, 25.0, 6.0, 40.0);
        town.job = Job::new(1, vec![JobLine::new(3, Some(2.0), vec![])]);
        seat(&mut market, &mut actors, shop);
        seat(&mut market, &mut actors, town);
        let mut card = MarketGood::new();
        card.sought_unmet = 30.0;
        market.goods.insert(3, card);

        market.internal_migration(&mut actors, &factuals);

        assert!(actors.pop(1).demographics.household.count < 24.0);
    }

    /// The market wants bread. The idle pop has no running line and no coin,
    /// so the whole count still leaves.
    #[test]
    fn internal_migration_still_evacuates_an_idle_pop_while_bread_is_sought() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut origin = mover(1, 8.0, 4.0, 0.0);
        origin.property.insert(2, PopPRow::new(80.0));
        origin
            .property
            .insert(OPENING_CURRENCY, PopPRow::new(0.4));
        origin.job = Job::new(1, vec![JobLine::new(9, Some(0.0), vec![])]);
        let mut dest = mover(2, 4.0, 6.0, 0.0);
        dest.property.insert(3, PopPRow::new(400.0));
        dest.job = Job::new(2, vec![JobLine::new(9, Some(2.0), vec![])]);
        seat(&mut market, &mut actors, origin);
        seat(&mut market, &mut actors, dest);
        let mut card = MarketGood::new();
        card.sought_unmet = 30.0;
        market.goods.insert(3, card);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!(actors.pops.get(&1).is_none());
        assert!((actors.pop(2).demographics.household.count - 12.0).abs() < 1e-9);
    }

    /// A working pop of one household still leaves when the remainder would be under one.
    #[test]
    fn internal_migration_records_a_short_move_when_work_is_still_running() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut low = mover(1, 1.0, 4.0, 0.0);
        low.job = Job::new(1, vec![JobLine::new(1, Some(1.0), Vec::new())]);
        let mut high = mover(2, 10.0, 6.0, 0.0);
        high.job = Job::new(1, vec![JobLine::new(1, Some(1.0), Vec::new())]);
        seat(&mut market, &mut actors, low);
        seat(&mut market, &mut actors, high);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!(actors.pops.get(&1).is_none());
        assert_eq!(
            market.pop_removals(),
            &[PopRemoval {
                id: 1,
                why: PopExit::UnderOneMove { to: vec![2] },
            }]
        );
    }

    /// A high-living pop that is about to be removed does not keep the people sent to it.
    #[test]
    fn internal_migration_does_not_deliver_into_a_pop_that_is_leaving() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut low = mover(1, 10.0, 3.0, 1000.0);
        low.job = Job::new(1, vec![]);
        let mut mid = mover(2, 1.0, 7.0, 2.0);
        mid.job = Job::new(1, vec![]);
        let mut high = mover(3, 10.0, 9.0, 1000.0);
        high.job = Job::new(1, vec![]);
        seat(&mut market, &mut actors, low);
        seat(&mut market, &mut actors, mid);
        seat(&mut market, &mut actors, high);

        market.internal_migration(&mut actors, &Factuals::new());

        assert!(actors.pops.get(&2).is_none());
        let kept =
            actors.pop(1).demographics.household.count + actors.pop(3).demographics.household.count;
        assert!((kept - 21.0).abs() < 1e-6);
    }

    /// Growth or the cap can leave a pop under one household. That pop is deleted.
    #[test]
    fn prune_removes_a_pop_under_one_household() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        seat(&mut market, &mut actors, mover(1, 0.4, 5.0, 80.0));
        seat(&mut market, &mut actors, mover(2, 4.0, 5.0, 10.0));

        market.prune_pops_below_one_household(&mut actors);

        assert!(actors.pops.get(&1).is_none());
        assert!(!market.pops.contains(&1));
        assert_eq!(
            market.pop_removals(),
            &[PopRemoval {
                id: 1,
                why: PopExit::UnderOneHousehold,
            }]
        );
        assert!((actors.pop(2).property[&1].quantity - 10.0).abs() < 1e-9);
    }

    #[test]
    fn migration_push_squares_the_gaps_and_weights_fear_over_anger() {
        assert!((super::push_score(1.0, 0.0, 0.0, 0.0, 0.0) - 1.0).abs() < 1e-12);
        assert!((super::push_score(1.0, 0.5, 0.0, 0.0, 0.0) - 1.25).abs() < 1e-12);
        assert!((super::push_score(0.0, 0.0, 2.0, 0.0, 0.0) - 4.0).abs() < 1e-12);
        assert!((super::push_score(1.0, 0.0, 0.0, 0.5, 0.0) - 1.5).abs() < 1e-12);
        assert!((super::push_score(1.0, 0.0, 0.0, 0.0, 0.5) - 1.25).abs() < 1e-12);
        assert!((super::pull_score(1.0, 0.0, 1.0) - 2.0).abs() < 1e-12);
        assert!((super::pull_score(0.0, 2.0, 1.0) - 8.0).abs() < 1e-12);
    }

    /// Two people are one copy of the paced share. Each doubling adds a copy.
    #[test]
    fn migration_size_scale_doubles_with_the_people() {
        assert!((super::migration_size_scale(2.0) - 1.0).abs() < 1e-12);
        assert!((super::migration_size_scale(8.0) - 3.0).abs() < 1e-12);
        assert_eq!(super::migration_size_scale(1.0), 0.0);
        assert_eq!(super::migration_size_scale(f64::NAN), 0.0);
    }

    /// A one-point score saturates at one half, then the pace keeps a twentieth.
    #[test]
    fn migration_fraction_stays_under_the_pace() {
        assert!((super::migration_fraction(1.0) - 0.05).abs() < 1e-12);
        assert!((super::migration_fraction(1.0e9) - super::MIGRATION_PACE).abs() < 1e-6);
        assert_eq!(super::migration_fraction(0.0), 0.0);
    }

    /// Make bread and make pot need a good the pop does not hold. Farm, water,
    /// and forage need time only.
    fn runnable_factuals() -> Factuals {
        let mut factuals = founding_factuals();
        factuals = factuals
            .with_process(
                Process::new(3, "make bread", 0)
                    .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(2, 1.0, false)),
            )
            .with_process(
                Process::new(4, "make pot", 0)
                    .with_input(ProcessInput::new(8, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(11, 1.0, false)),
            )
            .with_process(
                Process::new(30, "subsistence water", 0)
                    .with_input(ProcessInput::new(TIME, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(2, 3.0, false))
                    .with_complexity(0.25),
            )
            .with_process(
                Process::new(31, "subsistence forage", 0)
                    .with_input(ProcessInput::new(TIME, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(7, 3.0, false))
                    .with_complexity(0.25),
            );
        factuals
    }

    /// Grain at 32 cannot run without grain. Time at 33 can.
    fn recovery_factuals() -> Factuals {
        runnable_factuals()
            .with_process(
                Process::new(32, "make bread", 0)
                    .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(2, 1.0, false)),
            )
            .with_process(
                Process::new(33, "spin", 0)
                    .with_input(ProcessInput::new(TIME, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(13, 1.0, false)),
            )
    }

    /// Subsistence craft, make grain, make water from clay, and make time.
    fn founding_factuals() -> Factuals {
        // The stake and the edit are what these boards test. The wealth floor and the gap have their own.
        let mut factuals = Factuals::new();
        factuals.config.pop.founding_wealth = 0.0;
        factuals.config.pop.founding_gap = 1.0;
        factuals
            .with_process(
                Process::new(1, "make grain", 0).with_output(ProcessOutput::new(1, 1.0, false)),
            )
            .with_process(
                Process::new(2, "make water", 0)
                    .with_input(ProcessInput::new(8, 1.0, true, InputType::Consumed, false))
                    .with_output(ProcessOutput::new(2, 1.0, false)),
            )
            .with_process(
                Process::new(28, "make time", 0)
                    .with_output(ProcessOutput::new(TIME, 1.0, false))
                    .with_output(ProcessOutput::new(9, 0.01, true)),
            )
            .with_process(
                Process::new(29, "subsistence farm", 0)
                    .with_output(ProcessOutput::new(1, 3.0, false))
                    .with_complexity(0.25),
            )
            .with_craft(
                Craft::new(1, "subsistence")
                    .with_process(29)
                    .with_process(30)
                    .with_process(31),
            )
            .with_craft(
                Craft::new(5, "subsistence farming")
                    .with_process(29)
                    .with_process(30)
                    .with_process(31)
                    .with_process(1),
            )
    }

    /// Founding board plus a time-only pots process.
    fn pots_factuals() -> Factuals {
        founding_factuals().with_process(
            Process::new(5, "pots", 0).with_output(ProcessOutput::new(11, 1.0, false)),
        )
    }

    /// Farm and bread. Make grain and make water are not on this board.
    fn bakery_factuals() -> Factuals {
        let mut factuals = Factuals::new();
        factuals.config.pop.founding_wealth = 0.0;
        factuals.config.pop.founding_gap = 1.0;
        factuals
            .with_process(
                Process::new(29, "subsistence farm", 0)
                    .with_output(ProcessOutput::new(1, 3.0, false))
                    .with_complexity(0.25),
            )
            .with_process(
                // A point per run would price this bread under its inputs.
                Process::new(3, "make bread", 0)
                    .with_complexity(0.01)
                    .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(2, 1.0, false)),
            )
    }

    /// Bread spends grain. Pots spend clay. No time-only process sits beside them.
    fn bread_and_pot_factuals() -> Factuals {
        // Same wealth and gap waiver as [`founding_factuals`].
        let mut factuals = Factuals::new();
        factuals.config.pop.founding_wealth = 0.0;
        factuals.config.pop.founding_gap = 1.0;
        factuals
            .with_process(
                Process::new(3, "make bread", 0)
                    .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(2, 1.0, false)),
            )
            .with_process(
                Process::new(4, "make pot", 0)
                    .with_input(ProcessInput::new(8, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(11, 1.0, false)),
            )
    }

    /// One founding from a fresh seed.
    fn found(market: &mut Market, actors: &mut Actors, factuals: &Factuals) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        market.found_pop(actors, factuals, &mut rng);
    }

    fn subsistence_job() -> Job {
        Job::new(
            1,
            vec![
                JobLine::new(29, Some(0.0), vec![]),
                JobLine::new(30, Some(0.0), vec![]),
                JobLine::new(31, Some(0.0), vec![]),
            ],
        )
    }

    fn aim(pop: &mut Pop, good: usize) {
        let desire = &mut pop.desires[0][0];
        desire.target = vec![DesireTarget::new(good, DesireTargetType::Consume, 1.0)];
        desire.target_taken = vec![0.0];
    }

    /// The pop with more saved goods gives five households. Make grain
    /// replaces the subsistence farm that makes the same good. A second
    /// call the same day joins that pop instead of founding another.
    #[test]
    fn found_pop_gives_five_households_from_the_largest_savings() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut rich_living = mover(1, 25.0, 5.0, 100.0);
        rich_living.saved.insert(2, 1.0);
        rich_living.job = subsistence_job();
        aim(&mut rich_living, 1);
        let mut saver = mover(2, 30.0, 1.0, 40.0);
        saver.saved.insert(2, 40.0);
        saver.property.get_mut(&1).unwrap().used = 8.0;
        saver.demographics.species = 3;
        saver.demographics.culture = 4;
        saver.demographics.stratum = 5;
        saver.demographics.religion = 6;
        saver.job = subsistence_job();
        aim(&mut saver, 1);
        seat(&mut market, &mut actors, rich_living);
        seat(&mut market, &mut actors, saver);
        market.mass_closes.push(MassClose {
            good: 1,
            offered: 0.0,
            requested: 5.0,
            exchanged: 0.0,
            price: None,
            miss: None,
        });

        let factuals = founding_factuals();
        found(&mut market, &mut actors, &factuals);
        found(&mut market, &mut actors, &factuals);

        assert!((actors.pop(1).demographics.household.count - 25.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 20.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 80.0 / 3.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].used - 16.0 / 3.0).abs() < 1e-9);
        assert!((actors.pop(2).migration + 10.0).abs() < 1e-9);
        let child = actors.pop(3);
        assert!(market.pops.contains(&3));
        assert!((child.demographics.household.count - 10.0).abs() < 1e-9);
        assert_eq!(child.demographics.species, 3);
        assert_eq!(child.demographics.culture, 4);
        assert_eq!(child.demographics.stratum, 5);
        assert_eq!(child.demographics.religion, 6);
        assert!(child.population_cap.is_none());
        assert!((child.migration - 10.0).abs() < 1e-9);
        assert_eq!(child.job.craft, 0);
        let lines: Vec<usize> = child.job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![30, 31, 1]);
        assert!((child.property[&1].quantity - 40.0 / 3.0).abs() < 1e-9);
        assert!((child.property[&1].used - 8.0 / 3.0).abs() < 1e-9);
        assert!((child.property[&2].quantity - 1.1).abs() < 1e-9);
        assert!(actors.pops.get(&4).is_none());
    }

    /// Grain is already on the job. Water is profitable, new, and demanded, so it is added.
    #[test]
    fn found_pop_adds_a_profitable_process_for_a_new_demanded_good() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 40.0);
        founder.saved.insert(2, 10.0);
        founder.property.insert(8, PopPRow::new(20.0));
        founder.job = subsistence_job();
        founder.job.lines.push(JobLine::new(1, Some(0.0), vec![]));
        aim(&mut founder, 1);
        founder.desires[1].push(Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 2,
            priority: 0,
            target: vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            amount: 1.0,
            satisfaction: 0.0,
            target_taken: vec![0.0],
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        });
        seat(&mut market, &mut actors, founder);
        for good in [1, 2] {
            market.mass_closes.push(MassClose {
                good,
                offered: 0.0,
                requested: 4.0,
                exchanged: 0.0,
                price: None,
                miss: None,
            });
        }
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market.goods.insert(8, MarketGood::new().with_amv(1.0));

        found(&mut market, &mut actors, &founding_factuals());

        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
        let child = actors.pop(2);
        // Subsistence farming lists the farm, the water, the forage, and make grain.
        assert_eq!(child.job.craft, 5);
        let lines: Vec<usize> = child.job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![29, 30, 31, 1, 2]);
        assert!((child.property[&8].quantity - 4.0).abs() < 1e-9);
    }

    /// Make cotton is already on the job. The clothier list is filled in,
    /// subsistence cotton comes off, and the job is named clothier.
    /// Subsistence farming is not filled in. A set that contains it is named
    /// for that larger list.
    #[test]
    fn complete_craft_groups_fills_the_clothier_and_drops_subsistence_cotton() {
        let factuals = founding_factuals()
            .with_process(
                Process::new(32, "subsistence cotton", 0)
                    .with_complexity(0.25)
                    .with_output(ProcessOutput::new(39, 6.0, false)),
            )
            .with_process(
                Process::new(39, "make cotton", 0).with_output(ProcessOutput::new(39, 6.0, false)),
            )
            .with_process(
                Process::new(40, "make thread", 0)
                    .with_input(ProcessInput::new(39, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(40, 1.0, false)),
            )
            .with_process(
                Process::new(41, "make fabric", 0)
                    .with_input(ProcessInput::new(40, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(41, 1.0, false)),
            )
            .with_process(
                Process::new(42, "make clothes", 0)
                    .with_input(ProcessInput::new(41, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(42, 1.0, false)),
            )
            .with_craft(
                Craft::new(8, "clothier")
                    .with_process(39)
                    .with_process(40)
                    .with_process(41)
                    .with_process(42)
                    .with_complexity_modifier(0.5),
            );
        let mut next = vec![29, 30, 31, 32, 39];
        super::complete_craft_groups(&factuals, &mut next);
        assert_eq!(next, vec![29, 30, 31, 39, 40, 41, 42]);
        assert_eq!(super::craft_id_for(&factuals, &next), 8);

        let mut farm = vec![29, 30, 31];
        super::complete_craft_groups(&factuals, &mut farm);
        assert_eq!(farm, vec![29, 30, 31]);
        assert_eq!(super::craft_id_for(&factuals, &[29, 30, 31, 1, 2]), 5);
    }

    /// The farm loses holding. Pots can run on the grain stake, so the child keeps pots.
    #[test]
    fn found_pop_drops_a_losing_subsistence_line_before_adding() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 40.0);
        founder.saved.insert(2, 10.0);
        founder.job = subsistence_job();
        founder.job.lines.push(JobLine::new(5, Some(0.0), vec![]));
        aim(&mut founder, 2);
        seat(&mut market, &mut actors, founder);
        market.goods.insert(1, MarketGood::new().with_amv(-2.0));
        market.goods.insert(11, MarketGood::new().with_amv(2.0));
        market.mass_closes.push(MassClose {
            good: 2,
            offered: 0.0,
            requested: 4.0,
            exchanged: 0.0,
            price: None,
            miss: None,
        });

        found(&mut market, &mut actors, &pots_factuals());

        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
        let child = actors.pop(2);
        assert_eq!(child.job.craft, 0);
        let lines: Vec<usize> = child.job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![30, 31, 5]);
    }

    /// No open bid. The farm loses holding, and pots is the line that remains.
    #[test]
    fn found_pop_drops_a_losing_subsistence_line_when_nothing_is_bid() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 40.0);
        founder.saved.insert(2, 10.0);
        founder.job = Job::new(
            1,
            vec![
                JobLine::new(29, Some(0.0), vec![]),
                JobLine::new(5, Some(0.0), vec![]),
            ],
        );
        seat(&mut market, &mut actors, founder);
        market.goods.insert(1, MarketGood::new().with_amv(-2.0));
        market.goods.insert(11, MarketGood::new().with_amv(2.0));

        found(&mut market, &mut actors, &pots_factuals());

        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
        let child = actors.pop(2);
        assert_eq!(child.job.craft, 0);
        let lines: Vec<usize> = child.job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![5]);
    }

    /// Dropping the only line would leave an empty job, so nobody is founded.
    #[test]
    fn found_pop_does_not_drop_the_last_process() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 40.0);
        founder.saved.insert(2, 10.0);
        founder.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        seat(&mut market, &mut actors, founder);
        market.goods.insert(1, MarketGood::new().with_amv(-2.0));

        found(&mut market, &mut actors, &founding_factuals());

        assert!(actors.pops.get(&2).is_none());
        assert!((actors.pop(1).demographics.household.count - 25.0).abs() < 1e-9);
    }

    /// The shed set is already that demographic's job, so the five households join it.
    #[test]
    fn found_pop_joins_the_pop_that_already_has_the_dropped_set() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 40.0);
        founder.saved.insert(2, 40.0);
        founder.job = subsistence_job();
        founder.job.lines.push(JobLine::new(5, Some(0.0), vec![]));
        aim(&mut founder, 2);
        let mut taken = mover(2, 2.0, 1.0, 4.0);
        taken.job = Job::new(
            0,
            vec![
                JobLine::new(30, Some(0.0), vec![]),
                JobLine::new(31, Some(0.0), vec![]),
                JobLine::new(5, Some(0.0), vec![]),
            ],
        );
        seat(&mut market, &mut actors, founder);
        seat(&mut market, &mut actors, taken);
        market.goods.insert(1, MarketGood::new().with_amv(-2.0));
        market.goods.insert(11, MarketGood::new().with_amv(2.0));
        market.mass_closes.push(MassClose {
            good: 2,
            offered: 0.0,
            requested: 4.0,
            exchanged: 0.0,
            price: None,
            miss: None,
        });

        found(&mut market, &mut actors, &pots_factuals());

        assert!(actors.pops.get(&3).is_none());
        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 7.0).abs() < 1e-9);
        assert!((actors.pop(1).migration + 5.0).abs() < 1e-9);
        assert!((actors.pop(2).migration - 5.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 32.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 12.0).abs() < 1e-9);
        let lines: Vec<usize> = actors.pop(2).job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![30, 31, 5]);
    }

    /// Bread pays and the farm loses holding. One roll in five drops that
    /// farm in the same founding that adds the bread. The other rolls keep it.
    #[test]
    fn found_pop_drops_a_losing_line_when_the_both_roll_lands() {
        let factuals = pots_factuals().with_process(
            Process::new(3, "make bread", 0)
                .with_complexity(0.01)
                .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(2, 1.0, false)),
        );
        let lines_for = |seed: u64| -> Vec<usize> {
            let mut market = Market::new(1);
            let mut actors = Actors::new();
            let mut founder = mover(1, 25.0, 0.0, 20.0);
            founder.job = Job::new(
                0,
                vec![
                    JobLine::new(29, Some(0.0), vec![]),
                    JobLine::new(5, Some(0.0), vec![]),
                ],
            );
            aim(&mut founder, 2);
            seat(&mut market, &mut actors, founder);
            market.goods.insert(1, MarketGood::new().with_amv(-2.0));
            market
                .goods
                .insert(11, MarketGood::new().with_amv(2.0).with_salability(1.0));
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            market.found_pop(&mut actors, &factuals, &mut rng);
            actors
                .pop(2)
                .job
                .lines
                .iter()
                .map(|line| line.process)
                .collect()
        };
        assert_eq!(lines_for(3), vec![5, 3]);
        assert_eq!(lines_for(1), vec![29, 5, 3]);
    }

    /// The farm loses holding, and water is an open bid that does not pay.
    /// One roll in five adds that water in the same founding that drops the
    /// farm. The other rolls only drop the farm.
    #[test]
    fn found_pop_adds_a_process_when_the_both_roll_lands_on_a_drop() {
        let factuals = pots_factuals();
        let lines_for = |seed: u64| -> Vec<usize> {
            let mut market = Market::new(1);
            let mut actors = Actors::new();
            let mut founder = mover(1, 25.0, 0.0, 20.0);
            founder.property.insert(8, PopPRow::new(20.0));
            founder.job = Job::new(
                0,
                vec![
                    JobLine::new(29, Some(0.0), vec![]),
                    JobLine::new(5, Some(0.0), vec![]),
                ],
            );
            aim(&mut founder, 11);
            seat(&mut market, &mut actors, founder);
            market.goods.insert(1, MarketGood::new().with_amv(-2.0));
            market
                .goods
                .insert(11, MarketGood::new().with_amv(2.0).with_salability(1.0));
            market.mass_closes.push(MassClose {
                good: 2,
                offered: 0.0,
                requested: 4.0,
                exchanged: 0.0,
                price: None,
                miss: None,
            });
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            market.found_pop(&mut actors, &factuals, &mut rng);
            actors
                .pop(2)
                .job
                .lines
                .iter()
                .map(|line| line.process)
                .collect()
        };
        assert_eq!(lines_for(0), vec![5, 2]);
        assert_eq!(lines_for(4), vec![5]);
    }

    /// Three runs of bread fit in the grain that travels, so the child bakes.
    #[test]
    fn found_pop_adds_bread_when_the_stake_covers_three_runs() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 20.0);
        founder.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        aim(&mut founder, 2);
        seat(&mut market, &mut actors, founder);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));

        found(&mut market, &mut actors, &bakery_factuals());

        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 16.0).abs() < 1e-9);
        let child = actors.pop(2);
        assert!((child.property[&1].quantity - 4.0).abs() < 1e-9);
        let lines: Vec<usize> = child.job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![29, 3]);
        assert!(child.job.lines.iter().all(|line| line.target.is_none()));
    }

    /// Bread and pots both pay. Grain is already farmed, and the unmet desire
    /// still wants bread, so most seeds bake. A seed can still throw pots.
    #[test]
    fn found_pop_prefers_the_stuck_desire_without_locking_it() {
        let mut factuals = Factuals::new();
        factuals.config.pop.founding_wealth = 0.0;
        factuals.config.pop.founding_gap = 1.0;
        let factuals = factuals
            .with_process(
                Process::new(29, "farm", 0)
                    .with_complexity(0.25)
                    .with_output(ProcessOutput::new(1, 3.0, false)),
            )
            .with_process(
                Process::new(3, "make bread", 0)
                    .with_complexity(1.0)
                    .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(2, 1.0, false)),
            )
            .with_process(
                Process::new(4, "make pot", 0)
                    .with_complexity(1.0)
                    .with_input(ProcessInput::new(8, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(11, 1.0, false)),
            );
        let mut bread = 0;
        let mut pots = 0;
        for seed in 0..64 {
            let mut market = Market::new(1);
            let mut actors = Actors::new();
            let mut founder = mover(1, 25.0, 1.0, 20.0);
            founder.property.insert(8, PopPRow::new(20.0));
            founder.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
            founder.desires[1].push(Desire {
                source: DemographicSource::Species(0),
                demo_desire_id: 2,
                priority: 0,
                target: vec![
                    DesireTarget::new(1, DesireTargetType::Consume, 1.0),
                    DesireTarget::new(2, DesireTargetType::Consume, 1.0),
                ],
                amount: 1.0,
                satisfaction: 0.0,
                target_taken: vec![0.0, 0.0],
                category: None,
                effect: vec![],
                scalar: ScalingFactor::Fixed(1.0),
                decay: 0.0,
            });
            seat(&mut market, &mut actors, founder);
            market
                .goods
                .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
            market
                .goods
                .insert(2, MarketGood::new().with_amv(5.0).with_salability(1.0));
            market
                .goods
                .insert(8, MarketGood::new().with_amv(1.0).with_salability(1.0));
            market
                .goods
                .insert(11, MarketGood::new().with_amv(5.0).with_salability(1.0));
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            market.found_pop(&mut actors, &factuals, &mut rng);
            let Some(child) = actors.pops.get(&2) else {
                continue;
            };
            let added = child
                .job
                .lines
                .iter()
                .map(|line| line.process)
                .find(|id| *id != 29);
            match added {
                Some(3) => bread += 1,
                Some(4) => pots += 1,
                _ => {}
            }
        }
        assert!(bread > pots * 2, "bread {bread} pots {pots}");
        assert!(pots >= 1, "bread {bread} pots {pots}");
    }

    /// The traveling grain cannot run bread three times, so nobody is founded.
    #[test]
    fn found_pop_waits_when_the_stake_cannot_run_the_process() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 2.0);
        founder.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        aim(&mut founder, 2);
        seat(&mut market, &mut actors, founder);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));

        found(&mut market, &mut actors, &bakery_factuals());

        assert!(actors.pops.get(&2).is_none());
        assert!((actors.pop(1).demographics.household.count - 25.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 2.0).abs() < 1e-9);
    }

    /// Fifteen water are parked. Founding does not see them until the hold
    /// is released. The traveling share of twenty-five households is then three.
    #[test]
    fn stake_hold_is_quantity_when_founding_reads_it() {
        let mut founder = mover(1, 25.0, 1.0, 0.0);
        founder.job.stake_hold.insert(2, 15.0);
        let history = Market::new(1).history();

        let before = super::founding_stake(&founder, &history, 0.0);
        assert!(before.get(&2).copied().unwrap_or(0.0).abs() < 1e-9);

        founder.release_stake_hold();

        let after = super::founding_stake(&founder, &history, 0.0);
        assert!((after.get(&2).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
        assert!(founder.job.stake_hold.is_empty());
        assert!(
            (founder
                .property
                .get(&2)
                .map(|row| row.quantity)
                .unwrap_or(0.0)
                - 15.0)
                .abs()
                < 1e-9
        );
    }

    /// The larger set-aside cannot run bread, so the smaller pop leaves.
    #[test]
    fn found_pop_asks_the_next_pop_when_the_largest_cannot_leave() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut rich = mover(1, 25.0, 1.0, 2.0);
        rich.saved.insert(9, 50.0);
        rich.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        let mut next = mover(2, 25.0, 1.0, 20.0);
        next.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        aim(&mut next, 2);
        seat(&mut market, &mut actors, rich);
        seat(&mut market, &mut actors, next);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market
            .goods
            .insert(9, MarketGood::new().with_amv(1.0).with_salability(1.0));

        found(&mut market, &mut actors, &bakery_factuals());

        assert!((actors.pop(1).demographics.household.count - 25.0).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 20.0).abs() < 1e-9);
        let lines: Vec<usize> = actors
            .pop(3)
            .job
            .lines
            .iter()
            .map(|line| line.process)
            .collect();
        assert_eq!(lines, vec![29, 3]);
    }

    /// Twenty grain can start bread three times. Nothing is saved, so the
    /// wealth floor keeps the households home. Two saved units at one of
    /// holding are two days, and then the child bakes.
    #[test]
    fn found_pop_waits_until_savings_cover_the_wealth_floor() {
        let mut factuals = bakery_factuals();
        factuals.config.pop.founding_wealth = 2.0;
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 20.0);
        founder.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        aim(&mut founder, 2);
        seat(&mut market, &mut actors, founder);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market
            .goods
            .insert(9, MarketGood::new().with_amv(1.0).with_salability(1.0));

        found(&mut market, &mut actors, &factuals);

        assert!(actors.pops.get(&2).is_none());
        assert!((actors.pop(1).demographics.household.count - 25.0).abs() < 1e-9);

        actors.pop_mut(1).saved.insert(9, 2.0);
        found(&mut market, &mut actors, &factuals);

        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
        let lines: Vec<usize> = actors
            .pop(2)
            .job
            .lines
            .iter()
            .map(|line| line.process)
            .collect();
        assert_eq!(lines, vec![29, 3]);
    }

    /// One day of bread is the meal. Two units past that day are two days
    /// of set-aside, and then the child bakes.
    #[test]
    fn found_pop_counts_meal_past_one_day() {
        let mut factuals = bakery_factuals();
        factuals.config.pop.founding_wealth = 2.0;
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 20.0);
        founder.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        aim(&mut founder, 2);
        founder.saved.insert(2, 1.0);
        founder.meal.insert(2, 1.0);
        seat(&mut market, &mut actors, founder);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));

        found(&mut market, &mut actors, &factuals);

        assert!(actors.pops.get(&2).is_none());
        assert!((actors.pop(1).demographics.household.count - 25.0).abs() < 1e-9);

        actors.pop_mut(1).saved.insert(2, 3.0);
        actors.pop_mut(1).meal.insert(2, 3.0);
        found(&mut market, &mut actors, &factuals);

        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
        assert!((actors.pop(1).saved[&2] - 2.4).abs() < 1e-9);
        assert!((actors.pop(2).property[&2].quantity - 0.6).abs() < 1e-9);
        let lines: Vec<usize> = actors
            .pop(2)
            .job
            .lines
            .iter()
            .map(|line| line.process)
            .collect();
        assert_eq!(lines, vec![29, 3]);
    }

    /// Two saved units at one of holding are two days, so twenty-five
    /// households may leave. One unit is one day. Twenty-four households
    /// cannot leave.
    #[test]
    fn is_founding_ready_follows_the_household_and_wealth_gates() {
        let mut market = Market::new(1);
        market
            .goods
            .insert(9, MarketGood::new().with_amv(1.0).with_salability(1.0));
        let mut pop = mover(1, 25.0, 1.0, 0.0);
        aim(&mut pop, 9);
        pop.saved.insert(9, 2.0);
        assert!(market.is_founding_ready(&pop, 2.0));

        pop.saved.insert(9, 1.0);
        assert!(!market.is_founding_ready(&pop, 2.0));

        pop.saved.insert(9, 2.0);
        pop.demographics.household.count = 24.0;
        assert!(!market.is_founding_ready(&pop, 0.0));
        assert_eq!(market.founding_wait(), 0);
    }

    /// # More Days Found Ahead Of A Larger Pile
    ///
    /// One hundred households hold two hundred coins against a basket of
    /// one hundred, so two days. Twenty-five households hold ten coins
    /// against a basket of one, so ten days. The smaller pile has more
    /// days, and it is asked first.
    #[test]
    fn founding_candidates_ask_the_pop_with_more_days_first() {
        let mut market = Market::new(1);
        market
            .goods
            .insert(9, MarketGood::new().with_amv(1.0).with_salability(1.0));
        let mut actors = Actors::new();
        let mut wide = mover(1, 100.0, 1.0, 0.0);
        aim(&mut wide, 9);
        wide.desires[0][0].amount = 100.0;
        wide.saved.insert(9, 200.0);
        let mut ready = mover(2, 25.0, 1.0, 0.0);
        aim(&mut ready, 9);
        ready.saved.insert(9, 10.0);
        seat(&mut market, &mut actors, wide);
        seat(&mut market, &mut actors, ready);

        let order = super::founding_candidates(&market, &actors, 1.5);

        assert_eq!(order, vec![2, 1]);
    }

    /// # Working Currency Counts Toward The Founding Gate
    ///
    /// Two working grain cover two days, so the gate opens. One grain is one
    /// day. Wood is not a currency, so a pile of it does not open the gate.
    #[test]
    fn working_currency_counts_toward_the_founding_gate() {
        let mut market = Market::new(1);
        market.currencies = [Some(1), None, None, None];
        market
            .goods
            .insert(1, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(3, MarketGood::new().with_amv(1.0).with_salability(1.0));
        let mut pop = mover(1, 25.0, 1.0, 2.0);
        aim(&mut pop, 1);
        assert!(market.is_founding_ready(&pop, 1.5));

        pop.property.insert(1, PopPRow::new(1.0));
        assert!(!market.is_founding_ready(&pop, 1.5));

        pop.property.insert(3, PopPRow::new(100.0));
        assert!(!market.is_founding_ready(&pop, 1.5));
    }

    /// One founding then blocks the next four market days. A call that
    /// founds nobody does not start that wait.
    #[test]
    fn found_pop_waits_out_the_gap() {
        let mut factuals = bakery_factuals();
        factuals.config.pop.founding_wealth = 2.0;
        factuals.config.pop.founding_gap = 5.0;
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 20.0);
        founder.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        aim(&mut founder, 2);
        seat(&mut market, &mut actors, founder);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market
            .goods
            .insert(9, MarketGood::new().with_amv(1.0).with_salability(1.0));

        found(&mut market, &mut actors, &factuals);

        assert!(actors.pops.get(&2).is_none());
        assert_eq!(market.founding_rest, 0);

        actors.pop_mut(1).saved.insert(9, 2.0);
        found(&mut market, &mut actors, &factuals);

        assert!(actors.pops.get(&2).is_some());
        assert_eq!(market.founding_wait(), 4);
        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);

        for _ in 0..4 {
            found(&mut market, &mut actors, &factuals);
        }

        assert!(actors.pops.get(&3).is_none());
        assert_eq!(market.founding_wait(), 0);
        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
    }

    /// Bread loses holding. The pot line remains, and the five households join the pop that has it.
    #[test]
    fn found_pop_joins_the_pop_that_keeps_the_line_it_can_run() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 0.0);
        founder.property.insert(8, PopPRow::new(20.0));
        founder.job = Job::new(
            0,
            vec![
                JobLine::new(3, Some(1.0), vec![]),
                JobLine::new(4, Some(1.0), vec![]),
            ],
        );
        let mut twin = mover(2, 2.0, 1.0, 0.0);
        twin.job = Job::new(0, vec![JobLine::new(4, Some(1.0), vec![])]);
        seat(&mut market, &mut actors, founder);
        seat(&mut market, &mut actors, twin);
        market.goods.insert(1, MarketGood::new().with_amv(1.0));
        market.goods.insert(2, MarketGood::new().with_amv(0.0));
        market.goods.insert(11, MarketGood::new().with_amv(2.0));

        found(&mut market, &mut actors, &bread_and_pot_factuals());

        assert!(actors.pops.get(&3).is_none());
        assert!((actors.pop(1).demographics.household.count - 20.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 7.0).abs() < 1e-9);
        let lines: Vec<usize> = actors.pop(2).job.lines.iter().map(|line| line.process).collect();
        assert_eq!(lines, vec![4]);
        assert_eq!(actors.pop(2).job.lines[0].target, Some(1.0));
    }

    /// A time bid, and a bid that was offered, found nobody.
    #[test]
    fn found_pop_skips_time_and_a_good_that_was_offered() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 25.0, 1.0, 40.0);
        founder.saved.insert(2, 10.0);
        founder.job = subsistence_job();
        aim(&mut founder, TIME);
        seat(&mut market, &mut actors, founder);
        market.goods.insert(1, MarketGood::new().with_amv(-2.0));
        market.mass_closes.push(MassClose {
            good: TIME,
            offered: 0.0,
            requested: 8.0,
            exchanged: 0.0,
            price: None,
            miss: None,
        });
        market.mass_closes.push(MassClose {
            good: 1,
            offered: 3.0,
            requested: 8.0,
            exchanged: 0.0,
            price: None,
            miss: None,
        });

        found(&mut market, &mut actors, &founding_factuals());

        assert!(actors.pops.get(&2).is_none());
        assert!((actors.pop(1).demographics.household.count - 25.0).abs() < 1e-9);
    }

    /// Twenty-four households cannot give five and still keep twenty.
    #[test]
    fn found_pop_leaves_a_pop_under_twenty_five_households_in_place() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut founder = mover(1, 24.0, 1.0, 40.0);
        founder.saved.insert(2, 10.0);
        founder.job = subsistence_job();
        aim(&mut founder, 1);
        seat(&mut market, &mut actors, founder);
        market.mass_closes.push(MassClose {
            good: 1,
            offered: 0.0,
            requested: 4.0,
            exchanged: 0.0,
            price: None,
            miss: None,
        });

        found(&mut market, &mut actors, &founding_factuals());

        assert!(actors.pops.get(&2).is_none());
        assert!((actors.pop(1).demographics.household.count - 24.0).abs() < 1e-9);
    }

    /// Five households over the count. That share is the working goods.
    #[test]
    fn founding_goods_fraction_is_the_leaving_share() {
        assert!((super::founding_goods_fraction(10.0) - 0.5).abs() < 1e-12);
        assert!((super::founding_goods_fraction(5.0) - 1.0).abs() < 1e-12);
        assert!((super::founding_goods_fraction(20.0) - 0.25).abs() < 1e-12);
        assert!((super::founding_goods_fraction(25.0) - 0.2).abs() < 1e-12);
        assert!(super::founding_goods_fraction(0.0).abs() < 1e-12);
    }

    #[test]
    fn founding_add_weight_doubles_for_a_new_good_and_again_for_demand() {
        assert!((super::founding_add_weight(false, 0.0, false) - 1.0).abs() < 1e-12);
        assert!((super::founding_add_weight(true, 0.0, false) - 2.0).abs() < 1e-12);
        assert!((super::founding_add_weight(false, 1.0, false) - 2.0).abs() < 1e-12);
        assert!((super::founding_add_weight(true, 1.0, false) - 4.0).abs() < 1e-12);
        assert!((super::founding_add_weight(true, 1.0, true) - 16.0).abs() < 1e-12);
        let one = super::founding_add_weight(true, 1.0, false);
        let many = super::founding_add_weight(true, 50.0, false);
        assert!(many > one * 2.0, "one {one} many {many}");
        assert!(many < one * 8.0, "one {one} many {many}");
    }

    /// A covered offer raises a drop by half. A short offer leaves it at 1.
    #[test]
    fn founding_drop_weight_rises_when_the_offer_covers_the_request() {
        let bread = Process::new(3, "bread", 0).with_output(ProcessOutput::new(2, 1.0, false));
        let covered = [MassClose {
            good: 2,
            offered: 10.0,
            requested: 4.0,
            exchanged: 4.0,
            price: None,
            miss: None,
        }];
        let short = [MassClose {
            good: 2,
            offered: 4.0,
            requested: 10.0,
            exchanged: 4.0,
            price: None,
            miss: None,
        }];
        assert!((super::founding_drop_weight(&bread, &covered) - 1.5).abs() < 1e-12);
        assert!((super::founding_drop_weight(&bread, &short) - 1.0).abs() < 1e-12);
        assert!((super::founding_drop_weight(&bread, &[]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn pick_weighted_walks_the_weights_in_order() {
        let items = [
            (super::FoundingEdit::Add(1), 4.0),
            (super::FoundingEdit::Add(2), 2.0),
        ];
        assert_eq!(
            super::pick_weighted(0.1, &items),
            Some(super::FoundingEdit::Add(1))
        );
        assert_eq!(
            super::pick_weighted(0.7, &items),
            Some(super::FoundingEdit::Add(2))
        );
    }

    #[test]
    fn choose_useful_falls_through_when_one_pool_is_empty() {
        assert!(super::choose_useful(0.79, 0.8, true, true));
        assert!(!super::choose_useful(0.8, 0.8, true, true));
        assert!(super::choose_useful(0.99, 0.8, true, false));
        assert!(!super::choose_useful(0.0, 0.8, false, true));
    }

    /// Working quantity travels in the leaving share. Three days for the five
    /// households come from the set-aside. Time stays out. Used stock is not subtracted.
    #[test]
    fn founding_stake_takes_quantity_and_saved() {
        let mut pop = mover(1, 25.0, 1.0, 40.0);
        pop.saved.insert(2, 8.0);
        pop.property.get_mut(&1).unwrap().used = 10.0;
        pop.property.insert(TIME, PopPRow::new(100.0));
        aim(&mut pop, 2);
        let mut history = MarketHistory::new();
        // AMV 2 at the default salability of 0.5 is one of holding.
        history.prices.insert(2, 2.0);
        let stake = super::founding_stake(&pop, &history, 3.0);
        assert!((stake[&1] - 8.0).abs() < 1e-9);
        assert!((stake[&2] - 0.6).abs() < 1e-9);
        assert!(!stake.contains_key(&TIME));
    }

    /// A time-only process needs the stake's total, not a pile of its own product.
    #[test]
    fn a_time_only_process_needs_three_units_in_the_stake() {
        let pots = Process::new(5, "pots", 0).with_output(ProcessOutput::new(11, 1.0, false));
        let grain = Process::new(1, "make grain", 0).with_output(ProcessOutput::new(1, 1.0, false));
        let time =
            Process::new(28, "make time", 0).with_output(ProcessOutput::new(TIME, 1.0, false));
        let short = HashMap::from([(1, 2.0)]);
        let enough = HashMap::from([(1, 3.0)]);
        assert!(!super::is_process_stocked(&pots, &short, 3.0));
        assert!(super::is_process_stocked(&pots, &enough, 3.0));
        assert!(super::is_process_stocked(&grain, &enough, 3.0));
        assert!(!super::is_process_stocked(&time, &enough, 3.0));
    }

    /// A consumed input has to cover every run. Capital has to be present once.
    #[test]
    fn a_consumed_input_must_cover_the_runs() {
        let bread = Process::new(3, "make bread", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, false));
        let mill = Process::new(6, "mill", 0)
            .with_input(ProcessInput::new(8, 1.0, true, InputType::Capital, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Consumed, false))
            .with_output(ProcessOutput::new(2, 1.0, false));
        let short = HashMap::from([(1, 2.0), (9, 5.0)]);
        let grain = HashMap::from([(1, 3.0), (8, 1.0)]);
        let no_mill = HashMap::from([(1, 3.0), (8, 0.5)]);
        assert!(!super::is_process_stocked(&bread, &short, 3.0));
        assert!(super::is_process_stocked(&bread, &grain, 3.0));
        assert!(super::is_process_stocked(&mill, &grain, 3.0));
        assert!(!super::is_process_stocked(&mill, &no_mill, 3.0));
    }

    /// The floor still adds subsistence when no loaded process in the set can run.
    #[test]
    fn floor_processes_adds_subsistence_when_the_set_cannot_run() {
        let factuals = bakery_factuals();
        assert_eq!(
            super::floor_processes(&factuals, &[3], &HashMap::new()),
            vec![29]
        );
        assert!(super::floor_processes(&factuals, &[3], &HashMap::from([(1, 3.0)])).is_empty());
    }
}
