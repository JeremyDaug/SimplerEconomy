use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::actors::Actors;
use crate::game::deal::{DealMaker, ProposedDeal, freight_bill};
use crate::game::factuals::Factuals;
use crate::game::good::Good;
use crate::game::job::listed_decay;
use crate::game::marketorder::MarketOrder;
use crate::game::pop::{MigrantGroup, SaveHold};

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

/// Flat share of the payment scale added when buy orders were still open.
const AMV_UNMET_NUDGE: f64 = 0.01;

/// Full share of absolute AMV one side of a mass book adds or removes.
///
/// [`mass_idle_fraction`] takes this share times the part of the larger
/// side that did not meet the other. Surplus subtracts it. Shortage adds it.
const MASS_IDLE_STEP: f64 = 0.03;

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
        }
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
    /// clears the exchange and flow counters and restates the card so the
    /// filled currencies average 1 ([`Self::peg_currencies`]).
    /// [`MarketGood::decayed`] and `volume`
    /// stay until the next morning's reset. Stock is left as it stands.
    /// The print move is [`print_push`], applied in holding space.
    /// Production flow is a share of [`Self::payment_scale`]. Decay is
    /// [`decay_cut`]: a positive price falls, and it stops at
    /// [`AMV_EPSILON`]. When `sought_unmet` is positive, that night's rot
    /// is added back and [`AMV_UNMET_NUDGE`] times the payment scale is
    /// added on top.
    /// A mass book that [`mass_book_can_move`] allows then moves by
    /// [`mass_idle_fraction`] of the absolute AMV this card opened at.
    /// [`use_pull`] then moves that price by the day's trade
    /// and process shares. The process share stops at [`AMV_STEP_CAP`] of
    /// the payment scale.
    /// Desire consumption and rot are in the share
    /// and do not add a second step. [`valued_price`] then keeps the result at
    /// [`AMV_EPSILON`] unless a seller priced that good below zero today.
    /// `factuals` is that tradeable check. Rot itself was added
    /// through [`Self::note_decay`] before this runs.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let history = self.history();
        let scale = self.payment_scale();
        let currencies = self.currencies();
        let idle = mass_idle_by_good(&self.mass_closes, &currencies, factuals, &history);
        let mut ids: Vec<usize> = self.goods.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let good = self
                .goods
                .get_mut(&id)
                .expect("id was just copied from goods");
            let old = good.amv;
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
            // Open buys. That rot goes back on, then the unmet step.
            if good.sought_unmet > 0.0 {
                next += rot_cut + scale * AMV_UNMET_NUDGE;
            }
            // Mass book. Unsold offer down, unmet bid up, by that share.
            if let Some(step) = idle.get(&id) {
                next += old.abs() * step;
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
            // A positive ask keeps the stored price positive.
            next = valued_price(self.negative_value.contains(&id), next);
            // Store the new AMV.
            good.set_amv(next);
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
            }
            // Clear today's exchange and flow.
            good.clear_exchange();
        }
        // Restate every AMV so the filled currencies average 1.
        self.peg_currencies();
        // The peg can shrink a positive price into the dead zone.
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
    /// Output value is each output's current AMV times its units, and only an
    /// output already on the market counts. When at least one output is priced
    /// and the input itself is on the market, the units and that value are
    /// stored for [`use_pull`]. Nothing is returned.
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

    /// # Peg Currencies
    ///
    /// Divides every stored AMV so the filled currencies average 1.
    ///
    /// Each filled slot counts once. A slot whose good is missing, or whose
    /// AMV is not finite, is left out of the mean. The mean has to be finite
    /// and above [`AMV_EPSILON`]. Every stored AMV, including a negative one,
    /// is then divided by that mean. Salability is unchanged. No such mean
    /// leaves the card as written.
    fn peg_currencies(&mut self) {
        let mut seen = HashSet::new();
        let mut sum = 0.0;
        let mut count = 0.0;
        for slot in self.currencies {
            let Some(id) = slot else {
                continue;
            };
            if !seen.insert(id) {
                continue;
            }
            let Some(good) = self.goods.get(&id) else {
                continue;
            };
            if !good.amv.is_finite() {
                continue;
            }
            sum += good.amv;
            count += 1.0;
        }
        if count == 0.0 {
            return;
        }
        let mean = sum / count;
        if !mean.is_finite() || mean <= AMV_EPSILON {
            return;
        }
        let mut ids: Vec<usize> = self.goods.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let good = self
                .goods
                .get_mut(&id)
                .expect("id was just copied from goods");
            good.set_amv(good.amv / mean);
        }
    }

    /// # Keep Positive Asks
    ///
    /// Lifts each stored AMV that no seller valued below zero.
    ///
    /// [`Self::peg_currencies`] has already divided the card.
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
    /// [`crate::game::deal::DealMaker::reset_day`]. Each pop then locks a
    /// meal and writes its purchase targets, [`Self::buy_ladder`] runs the
    /// sell book and the buy ladder, held inputs return to property, then
    /// produce, consume, [`crate::game::deal::DealMaker::rest`],
    /// [`crate::game::pop::Pop::update_sentiments`], pop growth,
    /// [`Self::rebalance_population_cap`],
    /// [`Self::prune_pops_below_one_household`],
    /// [`Self::internal_migration`], decay,
    /// [`crate::game::pop::Pop::rescale_desires`], actor record keeping,
    /// [`Self::gather_use`], and [`Self::record_keeping`].
    ///
    /// Sentiment runs before growth, so today's mood scales this year's births.
    /// Rescale is after decay, so this day's desire effects stay at the
    /// previous size. It also runs after migration, so satisfaction and
    /// targets match the size still on the market.
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
        rng: &mut impl rand::RngCore,
    ) {
        // Cleanup Phase. Drop yesterday's tape and rot. Production stays.
        // The plan still reads yesterday's offer and exchange.
        let prior = self.history();
        self.reset_day(actors);
        let mut history = self.history();
        history.mass_offered = prior.mass_offered;
        history.mass_exchanged = prior.mass_exchanged;
        let members = self.members();
        // Meal first, then the runs those leftovers can still fund.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                let pop = actors.pop_mut(id);
                let locked = pop.meal_to_keep();
                let gaps = pop.basic_gaps();
                let property = pop.property.clone();
                let hold = pop
                    .job
                    .plan_purchases(&property, &locked, factuals, &history, &gaps);
                for (good, units) in locked {
                    pop.stash_working(good, units, SaveHold::Meal);
                }
                for (good, units) in hold {
                    pop.stash_working(good, units, SaveHold::Input);
                }
            }
        }
        // One sell book, then each pop buys in household-liquidity order.
        self.buy_ladder(actors, factuals, rng);
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
        // Decay Phase. Membership is whoever migration left.
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
        self.record_keeping(factuals);
    }

    /// # Reset Day
    ///
    /// Clears this market's day tape and each member's yesterday.
    ///
    /// `actors` supplies the members. AMV, salability, stock, and production
    /// stay. Today's mass-market rows are dropped, and so is any sell
    /// priced below zero. The refused-ask walk stays. Production stays so
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

    /// # Remember Mass Close
    ///
    /// Stores one mass-market row for `good`.
    ///
    /// `offered` and `requested` are the whole units quoted. `exchanged` is
    /// what changed hands. `price` is the clearing AMV when that exchange
    /// happened. `miss` is why both sides quoted and nothing moved. A later
    /// pass for the same good keeps the larger quote, adds the exchange, and
    /// keeps a price that traded. A pass that exchanges a unit clears `miss`.
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
    /// the night steps its card toward that AMV. `traded` takes `exchanged`.
    /// Each tender's `traded` and `paid` take that tender's units.
    /// `clear_units` and `clear_value` take the good's own exchange.
    /// Nothing is returned.
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
                pay.traded += qty;
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
    /// a pop here. A count of 1 stays.
    pub(crate) fn prune_pops_below_one_household(&mut self, actors: &mut Actors) {
        let doomed: Vec<usize> = self
            .pops
            .iter()
            .copied()
            .filter(|&id| actors.pop(id).demographics.household.count < 1.0)
            .collect();
        for id in doomed {
            self.pops.remove(&id);
            actors.pops.remove(&id);
        }
    }

    /// # Internal Migration
    ///
    /// Moves households between the pops on this market, then clears the pool.
    ///
    /// The search space is these pops. Standard of living and wealth per
    /// household are household-weighted averages. A non-finite living standard
    /// is left out of that average and out of the living comparison. One point
    /// of living is the same gap at any scale. The gap is squared. Fear adds
    /// its full share and anger adds half of its share, on top of that level.
    /// Wealth below the average adds the square of its relative shortfall.
    /// A pop above the average living standard pulls, squared the same way,
    /// and hope adds its full share. The offer and the room are
    /// [`MIGRATION_PACE`] times `score / (1 + score)` of that pop's
    /// households. A one-point gap, with no fear or anger, is a twentieth.
    ///
    /// An offer splits across open destinations in proportion to each one's
    /// gated room. A destination keeps at most its room, and each pair is
    /// also capped by that gate. Households with nowhere to sit stay home.
    /// A move that would leave the origin under one household takes the whole
    /// count instead, and the receivers may pass their room. The origin is
    /// then removed. Flows into a pop that is itself leaving are dropped first.
    ///
    /// The gate is demographic distance times wealth. A different species is
    /// closed. Distance otherwise counts mismatches of culture, stratum, and
    /// religion, and a matching species is one of the four ids. A distance
    /// of 0 is open. Otherwise [`assimilation_cap`] limits the pair. That cap
    /// averages culture assimilation with religion conversion. Moving to a
    /// richer pop is harder by `origin / destination` wealth. A different
    /// craft closes once the destination is more than 10% richer. Craft `0`
    /// is not a shared craft. An origin with wealth at or below 0 has no
    /// upward move. A poorer destination stays open.
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
                sol: pop.standard_of_living(),
                wealth: pop.wealth_per_household(&history),
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
            });
        }
        // One pop, or none, has no other pop on this market to move to.
        if rows.len() < 2 {
            return;
        }
        // Offer and room are paced shares of each pop, against the household-weighted averages.
        let avg_sol = weighted_sol(&rows);
        let avg_wealth = weighted_wealth(&rows);
        for row in &mut rows {
            let push = push_score(
                sol_gap(row.sol, avg_sol),
                wealth_shortfall(row.wealth, avg_wealth),
                row.fear,
                row.anger,
            );
            let pull = pull_score(sol_surplus(row.sol, avg_sol), row.hope);
            row.offer = row.count * migration_fraction(push);
            row.room = row.count * migration_fraction(pull);
        }

        // Weight each destination by its room and by the wealth and demographic gate.
        let weight = pair_weights(&rows, factuals);
        // Split each offer across those weights, then hold each destination to its room.
        let mut flow = seat_offers(&rows, &weight);
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
        for (i, row) in rows.iter().enumerate() {
            if leaving[i] {
                self.pops.remove(&row.id);
                actors.pops.remove(&row.id);
            }
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
        }
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
/// A fraction of 0, and a good that cannot trade, are left out.
/// Returns [`mass_idle_fraction`] per good.
fn mass_idle_by_good(
    closes: &[MassClose],
    currencies: &[Option<usize>],
    factuals: &Factuals,
    history: &MarketHistory,
) -> HashMap<usize, f64> {
    let mut steps = HashMap::new();
    for close in closes {
        let step = mass_idle_fraction(close);
        if step == 0.0 || !mass_book_can_move(factuals, currencies, history, close.good) {
            continue;
        }
        steps.insert(close.good, step);
    }
    steps
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

/// # Mass Idle Fraction
///
/// The AMV fraction one mass book applies from the side that was left over.
///
/// `close` is that book. A book with both sides quoted that missed on
/// [`MassMiss::NoCross`] or [`MassMiss::Haul`] returns 0: those days did
/// not show the good unwanted, and [`step_ask_walk`] is what pulls the ask
/// to the card. A larger offer returns `-[MASS_IDLE_STEP]` times the unsold
/// share of that offer. A larger request returns [`MASS_IDLE_STEP`] times
/// the unfilled share of that request. Nothing on one side is the full
/// step. Equal sides return 0.
fn mass_idle_fraction(close: &MassClose) -> f64 {
    let both = close.offered >= 1.0 && close.requested >= 1.0;
    if both && matches!(close.miss, Some(MassMiss::NoCross | MassMiss::Haul)) {
        return 0.0;
    }
    if close.offered > close.requested {
        let share = (close.offered - close.requested) / close.offered;
        -MASS_IDLE_STEP * share
    } else if close.requested > close.offered {
        let share = (close.requested - close.offered) / close.requested;
        MASS_IDLE_STEP * share
    } else {
        0.0
    }
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
    /// Units that changed hands in accepted deals today.
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
/// A one-point living gap saturates at one half, so this pace moves a
/// twentieth of the households. One year stays under this share of the pop.
const MIGRATION_PACE: f64 = 0.1;
/// A different craft closes above this multiple of the origin's wealth.
const MIGRATION_WEALTH_CLOSE: f64 = 1.10;
/// Species, culture, stratum, and religion.
const DEMO_AXES: usize = 4;

/// One pop's inputs to [`Market::internal_migration`].
struct Mover {
    id: usize,
    count: f64,
    sol: f64,
    wealth: f64,
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
}

/// # Push Score
///
/// Squared living gap plus squared wealth shortfall, scaled by fear and anger.
///
/// `sol_gap` is points of living below the market. `wealth_short` is the
/// relative wealth shortfall. Fear adds its full share. Anger adds
/// [`MIGRATION_ANGER_WEIGHT`] of its share. A zero level stays zero.
fn push_score(sol_gap: f64, wealth_short: f64, fear: f64, anger: f64) -> f64 {
    let level = sol_gap.max(0.0).powi(2) + wealth_short.max(0.0).powi(2);
    level * (1.0 + fear + MIGRATION_ANGER_WEIGHT * anger)
}

/// # Pull Score
///
/// Squared living surplus, scaled by hope.
///
/// `sol_surplus` is points of living above the market. Hope adds its full share.
fn pull_score(sol_surplus: f64, hope: f64) -> f64 {
    sol_surplus.max(0.0).powi(2) * (1.0 + hope)
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
/// Points of living below `avg`. A non-finite input is 0.
fn sol_gap(sol: f64, avg: f64) -> f64 {
    if !sol.is_finite() || !avg.is_finite() {
        0.0
    } else {
        (avg - sol).max(0.0)
    }
}

/// # Sol Surplus
///
/// Points of living above `avg`. A non-finite input is 0.
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
/// Household-weighted wealth average over `rows`.
///
/// An empty weight returns 0.
fn weighted_wealth(rows: &[Mover]) -> f64 {
    let weight: f64 = rows.iter().map(|row| row.count).sum();
    if weight == 0.0 {
        return 0.0;
    }
    rows.iter().map(|row| row.count * row.wealth).sum::<f64>() / weight
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
/// A destination at or below the origin's wealth is `1`. A richer
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

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::{
        AMV_EPSILON, CURRENCY_SLOTS, Market, MarketGood, MarketHistory, MassClose, MassMiss,
        OPENING_CURRENCY, SALABILITY_DEFAULT, SALABILITY_MAX, SALABILITY_MIN, amv_scale,
        clearing_price, make_change, mix_change, monetary_rating, payment_tender, payment_tenders,
        ration,
    };
    use rand::SeedableRng;

    use crate::game::actor::Actor;
    use crate::game::actors::Actors;
    use crate::game::culture::Culture;
    use crate::game::deal::DealMaker;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag};
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

    /// A sell priced below zero lets that night's decline cross zero.
    ///
    /// The price sits just above [`AMV_EPSILON`]. Rot's 5 percent uses up
    /// that room. The unsold surplus crosses into the dead zone, so the
    /// stored price is -[AMV_EPSILON]. The ask of -1 is not the price.
    #[test]
    fn clear_mass_good_lets_a_negative_sell_cross_zero_at_night() {
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
        assert!((market.goods[&1].amv - -AMV_EPSILON).abs() < 1e-15);
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

    /// No currency is stored, so the unsold good's step stays on the card.
    #[test]
    fn record_keeping_lowers_an_unsold_good_and_clears_the_tape() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.0);
        good.offered_unsold = 10.0;
        market.goods.insert(1, good);
        let before = market.history();
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((before.price(1) - 2.0).abs() < 1e-12);
        assert!((market.goods[&1].amv - 1.995).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.9975).abs() < 1e-9);
        assert_eq!(market.goods[&1].offered_unsold, 0.0);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// A mass surplus that did not trade cuts the opening AMV by 3 percent.
    /// No currency is stored, so the step stays on good 1.
    #[test]
    fn record_keeping_lowers_a_mass_surplus_that_did_not_trade() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.94).abs() < 1e-9);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// A mass shortage raises the opening AMV by the unmet share of the step.
    /// Nothing offered is the full 3 percent. Good 2 offered one against six
    /// bids, so five sixths of that step lifts its negative card.
    #[test]
    fn record_keeping_raises_a_mass_shortage_that_did_not_trade() {
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
        assert!((market.goods[&1].amv - 2.06).abs() < 1e-9);
        let short = -4.0 + 4.0 * 0.03 * (5.0 / 6.0);
        assert!((market.goods[&2].amv - short).abs() < 1e-9);
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

    /// A partial surplus steps down by the unsold share.
    /// A balanced book, an untradeable good, and the only tender do not.
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
        // Good 1 offered 9 against 1 bid. The unsold share is 8/9 of the
        // 3 percent step. The currency opened at 4, so the restatement
        // divides by that mean. The other books did not step.
        let stepped = 1.0 - 0.03 * (8.0 / 9.0);
        assert!((market.goods[&1].amv - stepped).abs() < 1e-9);
        assert!((market.goods[&2].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&3].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&OPENING_CURRENCY].amv - 1.0).abs() < 1e-9);
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

    /// A tenth of the pile rots, so the price loses a quarter of that share.
    ///
    /// That quarter is 2.5 percent, under the 5 percent night cap. 2.0
    /// becomes 1.95. Salability loses only the loss cap.
    #[test]
    fn record_keeping_takes_a_quarter_of_the_rot_share() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.5));
        market.note_decay(1, 1.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.95).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.49).abs() < 1e-9);
        assert!((market.goods[&1].decayed - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].volume - 10.0).abs() < 1e-9);
    }

    /// A pile that rots completely stops at 5 percent of the price.
    ///
    /// A quarter of a total rot would be 25 percent. The night cap leaves
    /// 5 percent, so 2.0 becomes 1.90.
    #[test]
    fn record_keeping_caps_a_total_rot_at_five_percent() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.5));
        market.note_decay(1, 10.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.9).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.49).abs() < 1e-9);
    }

    /// A rot cut that would pass epsilon stops on the positive side of zero.
    ///
    /// The price sits just above [`AMV_EPSILON`]. Five percent of it is more
    /// than that gap, so decay leaves [`AMV_EPSILON`]. Salability loses only
    /// the loss cap.
    #[test]
    fn record_keeping_stops_rot_at_epsilon() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market.goods.insert(
            1,
            MarketGood::new()
                .with_amv(AMV_EPSILON * 1.02)
                .with_salability(1.5),
        );
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

    /// Full rot on a price just above epsilon, plus an unsold surplus, stops
    /// on the positive side of zero.
    ///
    /// Five percent uses up the room above [`AMV_EPSILON`]. The idle step
    /// then takes 3 percent of the opening price. No seller priced the good
    /// below zero, so the night is put back at [`AMV_EPSILON`].
    #[test]
    fn record_keeping_stops_a_rotted_surplus_at_epsilon() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market.goods.insert(
            1,
            MarketGood::new()
                .with_amv(AMV_EPSILON * 1.02)
                .with_salability(1.0),
        );
        market.note_decay(1, 10.0, 10.0);
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - AMV_EPSILON).abs() < 1e-15);
    }

    /// The same rot and surplus cross zero when a seller priced below zero.
    ///
    /// Five percent uses up the room above [`AMV_EPSILON`]. The idle step
    /// crosses into the dead zone, so the stored price is -[AMV_EPSILON].
    /// The negative minimum is not the new price.
    #[test]
    fn record_keeping_lets_a_rotted_surplus_cross_after_a_negative_value() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market.goods.insert(
            1,
            MarketGood::new()
                .with_amv(AMV_EPSILON * 1.02)
                .with_salability(1.0),
        );
        market.note_negative_value(1);
        market.note_decay(1, 10.0, 10.0);
        market.mass_closes.push(idle_close(1, 10.0, 0.0, 0.0));
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - -AMV_EPSILON).abs() < 1e-15);
    }

    #[test]
    fn record_keeping_production_lowers_amv_and_salability_with_it() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.5);
        good.production = 10.0;
        market.goods.insert(1, good);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        // Nothing was paid, so the scale is 1. Flow cap 0.005 of that scale.
        assert!((market.goods[&1].amv - 1.995).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.4975).abs() < 1e-9);
        assert_eq!(market.goods[&1].production, 0.0);
    }

    /// Production written before the day is still the night's flow.
    ///
    /// The morning clear drops rot and the exchange tape. It leaves
    /// production, and the night write then zeros it.
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

        assert!((market.goods[&1].amv - 1.995).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.4975).abs() < 1e-9);
        assert_eq!(market.goods[&1].production, 0.0);
        assert_eq!(market.goods[&1].decayed, 0.0);
        assert_eq!(market.goods[&1].traded, 0.0);
    }

    /// Open buys rise by the unmet step, then the currency mean restates the card.
    ///
    /// The coin is the only currency. It was paid, so its salability rises.
    /// The payment scale is 4, so each sought good gains 0.04 before that
    /// restatement. Dividing by the coin's 4 puts the coin at 1. Good 8
    /// was valued below zero, so the restatement keeps its sign.
    #[test]
    fn record_keeping_raises_unmet_buys_and_restates_the_currency() {
        let mut market = Market::new(1);
        market.currencies = [Some(9), None, None, None];
        let mut dear = MarketGood::new().with_amv(32.0).with_salability(1.0);
        dear.sought_unmet = 10.0;
        let mut cheap = MarketGood::new().with_amv(0.2).with_salability(1.0);
        cheap.sought_unmet = 10.0;
        let mut coin = MarketGood::new().with_amv(4.0).with_salability(1.0);
        coin.paid = 3.0;
        let mut debt = MarketGood::new().with_amv(-2.0).with_salability(1.0);
        debt.paid = 100.0;
        market.note_negative_value(8);
        let jewel = MarketGood::new()
            .with_amv(8.0)
            .with_salability(SALABILITY_MAX);
        market.goods.insert(1, dear);
        market.goods.insert(2, cheap);
        market.goods.insert(3, jewel);
        market.goods.insert(9, coin);
        market.goods.insert(8, debt);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 8.01).abs() < 1e-9);
        assert!((market.goods[&2].amv - 0.06).abs() < 1e-9);
        assert!((market.goods[&3].amv - 2.0).abs() < 1e-9);
        assert!((market.goods[&9].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&8].amv - -0.5).abs() < 1e-9);
        assert!((market.goods[&9].salability - 1.05).abs() < 1e-9);
    }

    /// Open buys put the night's rot back and add a step of the payment scale.
    ///
    /// Half of this pile is past the night cap, so rot would take 5 percent.
    /// Nothing was paid, so the scale is 1 and the step is 0.01.
    #[test]
    fn record_keeping_unmet_buys_outweigh_rot() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.0);
        good.sought_unmet = 4.0;
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
    /// The coin's holding value is 4, so the cap is 0.2. The coin is the
    /// currency, and the bread step is restated by that mean.
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
        assert!((market.goods[&2].amv - 0.3).abs() < 1e-9);
        assert!((market.goods[&9].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&9].salability - 1.05).abs() < 1e-9);
    }

    /// Nothing was paid and no currency is stored, so a high salability
    /// does not restate the card. The unsold good still takes its ease.
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
        assert!((market.goods[&1].amv - 1.995).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.9975).abs() < 1e-9);
    }

    /// The stored currency is restated at 1. The other good keeps the ratio.
    #[test]
    fn record_keeping_restates_the_card_in_the_stored_currency() {
        let mut market = Market::new(1);
        market
            .goods
            .insert(5, MarketGood::new().with_amv(8.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(4.0).with_salability(1.0));
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&5].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 0.5).abs() < 1e-9);
    }

    /// Two currencies average 1. Payment still raises both salabilities.
    #[test]
    fn record_keeping_restates_the_card_so_the_currencies_average_one() {
        let mut market = Market::new(1);
        market.currencies = [Some(5), Some(2), None, None];
        let mut small = MarketGood::new().with_amv(4.0).with_salability(1.0);
        small.paid = 1.0;
        let mut large = MarketGood::new().with_amv(8.0).with_salability(1.0);
        large.paid = 3.0;
        market.goods.insert(2, small);
        market.goods.insert(5, large);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&5].amv - 8.0 / 6.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 4.0 / 6.0).abs() < 1e-9);
        assert!((market.goods[&5].salability - 1.05).abs() < 1e-9);
        assert!((market.goods[&2].salability - 1.05).abs() < 1e-9);
        let mean = (market.goods[&5].amv + market.goods[&2].amv) / 2.0;
        assert!((mean - 1.0).abs() < 1e-9);
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

    /// Process use of the whole day moves toward the output value, and stops
    /// at the print cap.
    ///
    /// Ten units carry forty of output value, so the target is 4. The opening
    /// price is 0.2. The gap is past the cap, so the night lands 0.05 higher.
    #[test]
    fn record_keeping_pulls_process_use_onto_the_output_value() {
        let mut market = Market::new(1);
        let mut input = MarketGood::new().with_amv(0.2).with_salability(1.0);
        input.process_used = 10.0;
        input.process_value = 40.0;
        market.goods.insert(1, input);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 0.25).abs() < 1e-9);
        assert_eq!(market.goods[&1].process_used, 0.0);
    }

    /// An equal desire share keeps half of a process gap that fits under the cap.
    ///
    /// The target is 1.06. Half of the 0.06 gap is 0.03, so 1.0 lands on 1.03.
    #[test]
    fn record_keeping_lets_desire_use_hold_half_the_process_gap() {
        let mut market = Market::new(1);
        let mut input = MarketGood::new().with_amv(1.0).with_salability(1.0);
        input.process_used = 10.0;
        input.process_value = 10.6;
        input.desire_used = 10.0;
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

    /// Rot still takes its cut. The decayed units then shrink the process pull.
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
        market.goods.insert(1, input);
        market.note_decay(1, 10.0, 1000.0);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 2.015).abs() < 1e-9);
    }

    /// Desire use alone leaves the rot cut as the only move.
    #[test]
    fn record_keeping_holds_a_price_when_the_day_was_desire() {
        let mut market = Market::new(1);
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market.goods.get_mut(&1).expect("good").desire_used = 5.0;
        market.note_decay(1, 1.0, 10.0);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&1].amv - 1.95).abs() < 1e-9);
    }

    /// One grain becomes one bread priced at 4. The night stores that credit.
    /// The gap is past the print cap, so grain moves 0.05 and lands on 0.25.
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
        market
            .goods
            .insert(2, MarketGood::new().with_amv(4.0).with_salability(1.0));
        market.pops.insert(1);
        let mut actors = Actors::new();
        actors.pops.insert(1, pop);

        market.gather_use(&actors);

        assert!((market.goods[&1].process_used - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].process_value - 4.0).abs() < 1e-9);
        assert!(market.goods[&1].consumption.abs() < 1e-9);
        market.record_keeping(&factuals);
        assert!((market.goods[&1].amv - 0.25).abs() < 1e-9);
        assert!((market.goods[&2].amv - 4.0).abs() < 1e-9);
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
    /// Good 0 is not a currency. With no currency stored, the peg does not
    /// restate the card, so the other good keeps the AMV this night's step wrote.
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

    /// One point under the average offers a twentieth of the households.
    /// A fortieth of the stock goes along.
    #[test]
    fn internal_migration_moves_a_twentieth_for_one_point_of_living() {
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
        assert!((origin.demographics.household.count - 9.5).abs() < 1e-9);
        assert!((dest.demographics.household.count - 10.5).abs() < 1e-9);
        assert!((origin.property[&1].quantity - 195.0).abs() < 1e-9);
        assert!((dest.property[&1].quantity - 205.0).abs() < 1e-9);
        assert!((origin.property[&1].used - 7.8).abs() < 1e-9);
        assert!((dest.property[&1].used - 0.2).abs() < 1e-9);
        assert!((origin.property[&1].fresh - 39.0).abs() < 1e-9);
        assert_eq!(dest.property[&1].fresh, 0.0);
        assert!((origin.demographics.household.adult - 4.0).abs() < 1e-9);
        assert!((dest.demographics.household.adult - 22.0 / 10.5).abs() < 1e-9);
        assert!((dest.demographics.household.adult_mf - 12.0 / 22.0).abs() < 1e-9);
        assert!((dest.demographics.household.elder - 5.0 / 10.5).abs() < 1e-9);
        assert!((dest.demographics.household.child - 25.0 / 10.5).abs() < 1e-9);
        assert!((dest.demographics.household.adult_labor - 10.0 / 10.5).abs() < 1e-9);
        assert!((dest.demographics.household.partnership_rate - 27.0 / 10.5).abs() < 1e-9);
        assert!((origin.migration - -0.5).abs() < 1e-9);
        assert!((dest.migration - 0.5).abs() < 1e-9);
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

    /// Fear raises the offer when the destination has room for it.
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

        assert!((actors.pop(1).demographics.household.count - 9.4).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.6).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 97.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 103.0).abs() < 1e-9);
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

        assert!((actors.pop(1).demographics.household.count - 9.75).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.25).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 197.5).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 402.5).abs() < 1e-9);
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
        // Cap is 2 * 1/4 = 0.5, so half of the half-household room.
        assert!((actors.pop(1).demographics.household.count - 9.75).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.25).abs() < 1e-9);
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

        assert!((actors.pop(1).demographics.household.count - 9.625).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.375).abs() < 1e-9);
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

        assert!((actors.pop(1).demographics.household.count - 9.625).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.375).abs() < 1e-9);
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
        assert!((actors.pop(2).demographics.household.count - 11.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 20.0).abs() < 1e-9);
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
        assert!((actors.pop(2).property[&1].quantity - 10.0).abs() < 1e-9);
    }

    #[test]
    fn migration_push_squares_the_gaps_and_weights_fear_over_anger() {
        assert!((super::push_score(1.0, 0.0, 0.0, 0.0) - 1.0).abs() < 1e-12);
        assert!((super::push_score(1.0, 0.5, 0.0, 0.0) - 1.25).abs() < 1e-12);
        assert!((super::push_score(1.0, 0.0, 0.5, 0.0) - 1.5).abs() < 1e-12);
        assert!((super::push_score(1.0, 0.0, 0.0, 0.5) - 1.25).abs() < 1e-12);
        assert!((super::pull_score(1.0, 1.0) - 2.0).abs() < 1e-12);
    }

    /// A one-point score saturates at one half, then the pace keeps a twentieth.
    #[test]
    fn migration_fraction_stays_under_the_pace() {
        assert!((super::migration_fraction(1.0) - 0.05).abs() < 1e-12);
        assert!((super::migration_fraction(1.0e9) - super::MIGRATION_PACE).abs() < 1e-6);
        assert_eq!(super::migration_fraction(0.0), 0.0);
    }
}
