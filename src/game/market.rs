use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::actors::Actors;
use crate::game::deal::{matched_on, DealResponse, Meeting, MeetingOutcome, ProposedDeal, SellerBook};
use crate::game::marketorder::MarketOrder;
use crate::game::factuals::Factuals;
use crate::game::good::GoodTag;

/// Dead zone around zero for a stored AMV.
///
/// AMV may be negative. A value inside `(-AMV_EPSILON, AMV_EPSILON)` is not
/// stored: it bounces to `AMV_EPSILON` on the other side of zero from the
/// previous sign. This is not a configured price floor.
pub const AMV_EPSILON: f64 = 1e-9;

/// Salability of a good with no recorded value yet.
pub const SALABILITY_DEFAULT: f64 = 0.1;

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

/// Largest one-night holding move from the print and the unsold ease, as a share of the payment scale.
const AMV_STEP_CAP: f64 = 0.1;

/// Flat share of the payment scale subtracted when a good was offered and nothing sold.
const AMV_EXCESS_NUDGE: f64 = 0.05;

/// Largest one-night production-flow fraction.
const AMV_FLOW_CAP: f64 = 0.05;

/// Physical rot is divided by this before it cuts absolute AMV.
const AMV_ROT_DIVISOR: f64 = 4.0;

/// Largest share of absolute AMV that one night of rot may remove.
const AMV_ROT_CAP: f64 = 0.95;

/// Salability gained on a night the good was taken in payment and its AMV
/// did not fall.
const SALABILITY_UP_STEP: f64 = 0.05;

/// Largest salability lost on a night the AMV fell.
const SALABILITY_LOSS_CAP: f64 = 0.2;

/// Factor applied to a positive AMV. Clamped to [`AMV_SCALE_MIN`]..=1.
pub fn amv_scale(salability: f64) -> f64 {
    salability.clamp(AMV_SCALE_MIN, 1.0)
}

/// Monetary standing above par. Zero while salability is at or below 1.
pub fn monetary_rating(salability: f64) -> f64 {
    (salability - 1.0).max(0.0)
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
}

impl Market {
    /// Empty market with this id.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            pops: HashSet::new(),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            friction: 0.0,
        }
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
    /// clears the exchange and flow counters and restates the card in the
    /// unit good ([`Self::peg_unit`]). [`MarketGood::decayed`] and `volume`
    /// stay until the next morning's reset. Stock is left as it stands.
    /// The print move is [`print_push`], applied in holding space.
    /// Production flow is a share of [`Self::payment_scale`]. `factuals`
    /// is unused: rot is added through [`Self::note_decay`] before this runs.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = factuals;
        let history = self.history();
        let scale = self.payment_scale();
        let mut paid = Vec::new();
        let mut ids: Vec<usize> = self.goods.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let good = self.goods.get_mut(&id).expect("id was just copied from goods");
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
            if decay_base > 0.0 && decayed > 0.0 {
                let fraction = (decayed / decay_base / AMV_ROT_DIVISOR).clamp(0.0, AMV_ROT_CAP);
                next -= next.abs() * fraction;
            }
            // Store the new AMV.
            good.set_amv(next);
            // Salability. A fall lowers it. Payment without a fall raises it.
            let paid_units = good.paid;
            let informed = good.traded + good.paid + good.offered_unsold > 0.0;
            let fell = good.amv < old;
            if fell && old.abs() >= AMV_EPSILON {
                let drop = ((old - good.amv) / old.abs()).min(SALABILITY_LOSS_CAP);
                good.set_salability(good.salability - drop);
            } else if informed && paid_units > 0.0 {
                good.set_salability(good.salability + SALABILITY_UP_STEP);
            }
            if paid_units > 0.0 {
                paid.push((id, paid_units));
            }
            // Clear today's exchange and flow.
            good.clear_exchange();
        }
        // Restate every AMV in the unit good.
        self.peg_unit(&paid, &history);
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
        if units > 0.0 {
            value / units
        } else {
            1.0
        }
    }

    /// # Peg Unit
    ///
    /// Divides every stored AMV by [`Self::unit_good`]'s AMV.
    ///
    /// `paid` and `history` are passed through to that choice. The unit lands
    /// on 1. Every other AMV, including a negative one, is divided by that
    /// same price. Salability is unchanged. No positive AMV leaves the card
    /// as written.
    fn peg_unit(&mut self, paid: &[(usize, f64)], history: &MarketHistory) {
        let Some(unit) = self.unit_good(paid, history) else {
            return;
        };
        let divisor = self.goods[&unit].amv;
        let mut ids: Vec<usize> = self.goods.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let good = self.goods.get_mut(&id).expect("id was just copied from goods");
            good.set_amv(good.amv / divisor);
        }
    }

    /// # Unit Good
    ///
    /// The good [`Self::peg_unit`] restates at 1.
    ///
    /// `paid` is `(good id, units paid)` from before the tape clear.
    /// `history` is the card from before this night's AMV write. A good with
    /// a negative or zero AMV cannot be the unit. Payment value is units paid
    /// times [`MarketHistory::holding_per_unit`] on `history`. Ranking is
    /// [`Self::outranks_as_unit`]. Returns `None` when no AMV is positive.
    fn unit_good(&self, paid: &[(usize, f64)], history: &MarketHistory) -> Option<usize> {
        let mut paid_value: HashMap<usize, f64> = HashMap::new();
        for &(id, units) in paid {
            if units <= 0.0 {
                continue;
            }
            let holding = history.holding_per_unit(id);
            if holding <= 0.0 {
                continue;
            }
            paid_value.insert(id, holding * units);
        }
        let mut best: Option<usize> = None;
        for (&id, good) in &self.goods {
            if good.amv <= 0.0 {
                continue;
            }
            let replace = match best {
                None => true,
                Some(best_id) => self.outranks_as_unit(id, best_id, &paid_value),
            };
            if replace {
                best = Some(id);
            }
        }
        best
    }

    /// # Outranks As Unit
    ///
    /// Whether `id` wins over `other` inside [`Self::unit_good`].
    ///
    /// Higher salability wins. Equal salability goes to the larger
    /// `paid_value`. A remaining tie goes to the lower id.
    fn outranks_as_unit(
        &self,
        id: usize,
        other: usize,
        paid_value: &HashMap<usize, f64>,
    ) -> bool {
        let salability = self.goods[&id].salability;
        let other_salability = self.goods[&other].salability;
        match salability.partial_cmp(&other_salability) {
            Some(std::cmp::Ordering::Greater) => true,
            Some(std::cmp::Ordering::Equal) => {
                let value = paid_value.get(&id).copied().unwrap_or(0.0);
                let other_value = paid_value.get(&other).copied().unwrap_or(0.0);
                match value.partial_cmp(&other_value) {
                    Some(std::cmp::Ordering::Greater) => true,
                    Some(std::cmp::Ordering::Equal) => id < other,
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Snapshot of stored AMV and salability.
    pub fn history(&self) -> MarketHistory {
        let mut history = MarketHistory::new();
        for (&good_id, good) in &self.goods {
            history.prices.insert(good_id, good.amv);
            history.salability.insert(good_id, good.salability);
        }
        history.friction = self.friction;
        history
    }

    /// # Market Day
    ///
    /// One day for the actors registered on this market.
    ///
    /// Clears yesterday's exchange tape and rot, and each member's
    /// [`crate::game::deal::DealMaker::reset_day`],
    /// then reserve, produce, [`Self::match_deals`], consume, pop growth,
    /// decay, [`crate::game::pop::Pop::rescale_desires`], then actor
    /// record keeping and planning, then [`Self::record_keeping`].
    ///
    /// Rescale is after decay, so this day's desire effects stay at the
    /// previous size. Planning then reads the new targets.
    ///
    /// A pop's reserve applies its craft before the job reserves inputs.
    ///
    /// Institutions and states use the empty defaults, so their unimplemented
    /// decay and record-keeping methods stay uncalled.
    ///
    /// Returns every meeting from the exchange. Today's rot stays on
    /// [`MarketGood::decayed`]. Inter-market work stays outside this call.
    pub fn market_day(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        rng: &mut impl rand::RngCore,
    ) -> Vec<Meeting> {
        // Cleanup Phase. Drop yesterday's tape and rot. Production already recorded stays.
        self.reset_day(actors);
        let members = self.members();
        // Day start reservations
        for actor in &members {
            actors.get_mut(*actor).reserve(factuals, rng);
        }
        // Production Phase
        for actor in &members {
            actors.get_mut(*actor).produce(factuals);
        }
        // Exchange Phase
        let meetings = self.match_deals(actors, factuals, rng);
        // Consumption Phase
        for actor in &members {
            actors.get_mut(*actor).consume();
        }
        // Growth. The new size is applied to desires after decay.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).growth_phase(factuals);
            }
        }
        // Decay Phase
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
        // Record Keeping and Planning phase.
        let history = self.history();
        for actor in &members {
            actors.get_mut(*actor).record_keeping(factuals, &history);
            actors.get_mut(*actor).plan(factuals, &history);
        }
        self.record_keeping(factuals);
        meetings
    }

    /// # Reset Day
    ///
    /// Clears this market's day tape and each member's yesterday.
    ///
    /// `actors` supplies the members. AMV, salability, stock, and production
    /// stay. Production stays so [`crate::game::actors::Actors::start_day`],
    /// which runs before this, still counts at the night write. Pop
    /// satisfaction and reserves go to zero. Firms run
    /// [`crate::game::firm::Firm::reset_day`]. Institutions keep the empty
    /// [`crate::game::deal::DealMaker::reset_day`] default.
    fn reset_day(&mut self, actors: &mut Actors) {
        for good in self.goods.values_mut() {
            good.clear_day();
        }
        for actor in self.members() {
            actors.get_mut(actor).reset_day();
        }
    }

    /// # Match Deals
    ///
    /// Collects sell orders, then picks a buyer at random from those who still
    /// have a buy, and a seller at random from the valid matches.
    /// The buyer sees that seller's offers and requests and either proposes
    /// a basket or abandons. The seller accepts or rejects. An accepted
    /// basket is finalized, freight included. The buyer pays that freight
    /// from transport they already hold and from transport the basket buys.
    ///
    /// After every meeting both sides reevaluate their orders. A rejected or
    /// abandoned pair is not tried again this call.
    ///
    /// Returns every meeting. Only [`MeetingOutcome::Accepted`] moves goods.
    pub fn match_deals(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        rng: &mut impl rand::RngCore,
    ) -> Vec<Meeting> {
        let history = self.history();
        let members = self.members();
        let mut sells = Vec::new();
        for actor in &members {
            sells.extend(listed_sells(actors, *actor, &history, factuals));
        }

        let mut tried: HashSet<(Actor, Actor, usize)> = HashSet::new();
        let mut meetings = Vec::new();
        let mut accepted = Vec::new();
        loop {
            let mut candidates = Vec::new();
            for buyer in &members {
                for buy in actors.get(*buyer).buy_orders(&history) {
                    if buy.target_amount < 1.0 || !tradeable(factuals, buy.target) {
                        continue;
                    }
                    let matches: Vec<MarketOrder> = sells
                        .iter()
                        .filter(|sell| {
                            matched_on(&buy, sell)
                                && !tried.contains(&(*buyer, sell.origin, buy.target))
                        })
                        .cloned()
                        .collect();
                    if !matches.is_empty() {
                        candidates.push((*buyer, buy, matches));
                    }
                }
            }
            if candidates.is_empty() {
                break;
            }
            let pick = crate::game::util::random_index(rng, candidates.len());
            let (buyer, buy, matches) = candidates.swap_remove(pick);
            let sell = matches[crate::game::util::random_index(rng, matches.len())].clone();
            tried.insert((buyer, sell.origin, buy.target));
            let meeting = self.meet(actors, buyer, &sell, &history, factuals, rng);
            if meeting.outcome == MeetingOutcome::Accepted {
                if let Some(deal) = meeting.proposal.clone() {
                    accepted.push(deal);
                }
            }
            meetings.push(meeting);
            replace_sells(&mut sells, actors, buyer, &history, factuals);
            replace_sells(&mut sells, actors, sell.origin, &history, factuals);
        }
        self.record_match_tape(actors, factuals, &history, &accepted);
        meetings
    }

    /// Record units traded, payment, the print, and what was still on the book.
    ///
    /// Does not write AMV or salability. Those move in [`Self::record_keeping`].
    /// The print itself is [`Self::note_prints`].
    fn record_match_tape(
        &mut self,
        actors: &Actors,
        factuals: &Factuals,
        history: &MarketHistory,
        deals: &[ProposedDeal],
    ) {
        for deal in deals {
            self.note_prints(history, deal);
            for (&good, &qty) in &deal.goods {
                if qty == 0.0 {
                    continue;
                }
                let row = self.goods.entry(good).or_insert_with(MarketGood::new);
                row.traded += qty.abs();
                if qty < 0.0 {
                    row.paid += -qty;
                }
            }
        }
        for row in self.goods.values_mut() {
            row.sought_unmet = 0.0;
            row.offered_unsold = 0.0;
        }
        let members = self.members();
        let mut sought: HashMap<usize, f64> = HashMap::new();
        let mut offered: HashMap<usize, f64> = HashMap::new();
        for actor in &members {
            for buy in actors.get(*actor).buy_orders(history) {
                if buy.target_amount >= 1.0 && tradeable(factuals, buy.target) {
                    *sought.entry(buy.target).or_insert(0.0) += buy.target_amount.floor();
                }
            }
            for sell in listed_sells(actors, *actor, history, factuals) {
                *offered.entry(sell.target).or_insert(0.0) += (-sell.target_amount).floor();
            }
        }
        for (good, qty) in sought {
            self.goods
                .entry(good)
                .or_insert_with(MarketGood::new)
                .sought_unmet += qty;
        }
        for (good, qty) in offered {
            self.goods
                .entry(good)
                .or_insert_with(MarketGood::new)
                .offered_unsold += qty;
        }
    }

    /// # Note Prints
    ///
    /// Adds this deal's paid holding onto the goods the buyer received.
    ///
    /// `history` is the morning card. Payment holding is the morning holding
    /// value of every good the buyer gave. That sum is split across received
    /// lines whose morning holding is positive, in proportion to holding
    /// times units. Each line adds its share to [`MarketGood::print_holding`]
    /// and its units to [`MarketGood::print_units`]. A deal with no such
    /// line records nothing.
    fn note_prints(&mut self, history: &MarketHistory, deal: &ProposedDeal) {
        let mut payment = 0.0;
        let mut received = Vec::new();
        for (&good, &qty) in &deal.goods {
            if qty < 0.0 {
                payment += history.holding_per_unit(good) * -qty;
            } else if qty > 0.0 {
                let holding = history.holding_per_unit(good);
                if holding > 0.0 {
                    received.push((good, qty, holding * qty));
                }
            }
        }
        let weight: f64 = received.iter().map(|(_, _, line)| *line).sum();
        if weight <= 0.0 {
            return;
        }
        for (good, qty, line) in received {
            let row = self.goods.entry(good).or_insert_with(MarketGood::new);
            row.print_holding += payment * (line / weight);
            row.print_units += qty;
        }
    }

    /// One meeting. The buyer proposes from the seller's book. The seller
    /// accepts or rejects. Both sides then rewrite their orders.
    fn meet(
        &self,
        actors: &mut Actors,
        buyer: Actor,
        sell: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
        rng: &mut impl rand::RngCore,
    ) -> Meeting {
        let seller = sell.origin;
        let match_good = sell.target;
        let book = SellerBook {
            seller,
            offers: listed_sells(actors, seller, history, factuals),
            requests: actors.get(seller).buy_orders(history),
        };
        let mut proposal = actors.get(buyer).propose(match_good, &book, history, factuals);
        if let Some(deal) = &mut proposal {
            // The basket is chosen. Record each giver's fresh fraction before anyone moves.
            deal.stamp_fresh_shares(|actor, good| actors.get(actor).fresh_share(good));
        }
        let outcome = if let Some(proposal) = &proposal {
            if actors.get(seller).evaluate(proposal, history, factuals) == DealResponse::Accept
                && is_valid_exchange(actors, proposal)
            {
                actors.get_mut(buyer).finalize(proposal, factuals);
                actors.get_mut(seller).finalize(proposal, factuals);
                MeetingOutcome::Accepted
            } else {
                MeetingOutcome::Rejected
            }
        } else {
            MeetingOutcome::Abandoned
        };
        actors.get_mut(buyer).reevaluate(history, rng);
        actors.get_mut(seller).reevaluate(history, rng);
        Meeting {
            buyer,
            seller,
            match_good,
            proposal,
            outcome,
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

fn listed_sells(
    actors: &Actors,
    actor: Actor,
    history: &MarketHistory,
    factuals: &Factuals,
) -> Vec<MarketOrder> {
    actors
        .get(actor)
        .sell_orders(history)
        .into_iter()
        .filter(|order| order.target_amount < 0.0 && tradeable(factuals, order.target))
        .collect()
}

/// Both sides can spare the goods the basket moves.
fn is_valid_exchange(actors: &Actors, proposal: &ProposedDeal) -> bool {
    proposal.goods.iter().all(|(&good, &qty)| {
        if qty > 0.0 {
            actors.get(proposal.seller).free_units(good) >= qty
        } else if qty < 0.0 {
            actors.get(proposal.buyer).free_units(good) >= -qty
        } else {
            true
        }
    })
}

fn replace_sells(
    sells: &mut Vec<MarketOrder>,
    actors: &Actors,
    actor: Actor,
    history: &MarketHistory,
    factuals: &Factuals,
) {
    sells.retain(|order| order.origin != actor);
    sells.extend(listed_sells(actors, actor, history, factuals));
}

fn tradeable(factuals: &Factuals, good: usize) -> bool {
    factuals
        .goods
        .get(&good)
        .is_none_or(|row| !row.tags.contains(&GoodTag::Untradeable))
}

/// A saved price snapshot for one market.
#[derive(Debug, Clone)]
pub struct MarketHistory {
    /// Last known AMV per good.
    pub prices: HashMap<usize, f64>,
    /// Last known salability per good.
    pub salability: HashMap<usize, f64>,
    /// Copied from [`Market::friction`].
    pub friction: f64,
    /// Used when a good has no recorded salability.
    pub default_salability: f64,
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
            friction: 0.0,
            default_salability: SALABILITY_DEFAULT,
        }
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
    /// Zeros today's exchange, print, and production flow.
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
    }

    /// # Clear Day
    ///
    /// Zeros today's exchange tape, print, and rot.
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
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        amv_scale, monetary_rating, Market, MarketGood, MarketHistory,
        SALABILITY_DEFAULT, SALABILITY_MAX, SALABILITY_MIN,
    };
    use crate::game::actor::Actor;
    use crate::game::actors::Actors;
    use crate::game::deal::ProposedDeal;
    use crate::game::factuals::Factuals;

    #[test]
    fn amv_scale_clamps_and_monetary_rating_starts_at_par() {
        assert!((amv_scale(0.0) - 0.05).abs() < 1e-12);
        assert!((amv_scale(0.1) - 0.1).abs() < 1e-12);
        assert!((amv_scale(1.5) - 1.0).abs() < 1e-12);
        assert!((monetary_rating(1.5) - 0.5).abs() < 1e-12);
        assert_eq!(monetary_rating(0.1), 0.0);
        assert!((SALABILITY_DEFAULT - 0.1).abs() < 1e-12);
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

    /// Good 0 is already the unit, so the unsold good's step stays on the card.
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
        assert!((market.goods[&1].amv - 1.95).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.975).abs() < 1e-9);
        assert_eq!(market.goods[&1].offered_unsold, 0.0);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    /// Payment without a fall raises salability. That good is then the unit.
    #[test]
    fn record_keeping_raises_salability_when_payment_does_not_drop_amv() {
        let mut market = Market::new(1);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(0.5);
        good.traded = 4.0;
        good.paid = 4.0;
        market.goods.insert(1, good);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.55).abs() < 1e-9);
    }

    /// Half the pile rots, so the price loses an eighth.
    #[test]
    fn record_keeping_cuts_amv_by_a_quarter_of_the_rot_share() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market.goods.insert(1, MarketGood::new().with_amv(2.0).with_salability(1.5));
        market.note_decay(1, 5.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.75).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.375).abs() < 1e-9);
        assert!((market.goods[&1].decayed - 5.0).abs() < 1e-9);
        assert!((market.goods[&1].volume - 10.0).abs() < 1e-9);
    }

    /// A pile that rots completely loses a quarter of the price.
    #[test]
    fn record_keeping_takes_a_quarter_of_a_total_rot() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market.goods.insert(1, MarketGood::new().with_amv(2.0).with_salability(1.5));
        market.note_decay(1, 10.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.5).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.3).abs() < 1e-9);
    }

    /// A quartered rot share above 95% still leaves 5% of the price.
    #[test]
    fn record_keeping_caps_total_rot_at_ninety_five_percent() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market.goods.insert(1, MarketGood::new().with_amv(2.0).with_salability(1.5));
        market.note_decay(1, 40.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 0.1).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.3).abs() < 1e-9);
        assert!((market.goods[&0].amv - 1.0).abs() < 1e-9);
    }

    #[test]
    fn record_keeping_production_lowers_amv_and_salability_with_it() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.5);
        good.production = 10.0;
        market.goods.insert(1, good);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        // Nothing was paid, so the scale is 1. Flow cap 0.05 of that scale.
        assert!((market.goods[&1].amv - 1.95).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.475).abs() < 1e-9);
        assert_eq!(market.goods[&1].production, 0.0);
    }

    /// Production written before the day is still the night's flow.
    ///
    /// The morning clear drops rot and the exchange tape. It leaves
    /// production, and the night write then zeros it.
    #[test]
    fn market_day_keeps_production_recorded_before_the_reset() {
        use rand::rngs::StdRng;
        use rand::SeedableRng;

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

        assert!((market.goods[&1].amv - 1.95).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.475).abs() < 1e-9);
        assert_eq!(market.goods[&1].production, 0.0);
        assert_eq!(market.goods[&1].decayed, 0.0);
        assert_eq!(market.goods[&1].traded, 0.0);
    }

    /// Unmet buys do not move AMV. Salability still picks the unit.
    ///
    /// The coin was paid, so its salability rises, and a negative-AMV payment
    /// cannot be the unit. The unpaid good at the salability cap is restated
    /// at 1. Sought goods keep the ratio they had to that good's AMV.
    #[test]
    fn record_keeping_leaves_unmet_buys_unmoved() {
        let mut market = Market::new(1);
        let mut dear = MarketGood::new().with_amv(32.0).with_salability(1.0);
        dear.sought_unmet = 10.0;
        let mut cheap = MarketGood::new().with_amv(0.2).with_salability(1.0);
        cheap.sought_unmet = 10.0;
        let mut coin = MarketGood::new().with_amv(4.0).with_salability(1.0);
        coin.paid = 3.0;
        let mut debt = MarketGood::new().with_amv(-2.0).with_salability(1.0);
        debt.paid = 100.0;
        let jewel = MarketGood::new().with_amv(8.0).with_salability(SALABILITY_MAX);
        market.goods.insert(1, dear);
        market.goods.insert(2, cheap);
        market.goods.insert(3, jewel);
        market.goods.insert(9, coin);
        market.goods.insert(8, debt);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 4.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 0.025).abs() < 1e-9);
        assert!((market.goods[&3].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&9].amv - 0.5).abs() < 1e-9);
        assert!((market.goods[&8].amv - -0.25).abs() < 1e-9);
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
        assert!((market.goods[&2].amv - 2.2).abs() < 1e-9);
        assert!((market.goods[&2].salability - 0.5).abs() < 1e-9);
    }

    /// The print step cannot exceed a tenth of the payment scale.
    ///
    /// The coin's holding value is 4, so the cap is 0.4. The coin is then
    /// the unit, and the bread step is restated in that coin.
    #[test]
    fn record_keeping_caps_the_print_step_at_the_payment_scale() {
        let mut market = Market::new(1);
        let mut bread = MarketGood::new().with_amv(1.0).with_salability(1.0);
        bread.traded = 1.0;
        bread.print_units = 1.0;
        bread.print_holding = 100.0;
        let mut coin = MarketGood::new().with_amv(4.0).with_salability(1.0);
        coin.paid = 3.0;
        market.goods.insert(2, bread);
        market.goods.insert(9, coin);
        market.record_keeping(&Factuals::new());
        assert!((market.goods[&2].amv - 0.35).abs() < 1e-9);
        assert!((market.goods[&9].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&9].salability - 1.05).abs() < 1e-9);
    }

    /// Payment holding is split by morning holding. A negative price is left out.
    #[test]
    fn note_prints_splits_payment_by_morning_holding() {
        let mut market = Market::new(1);
        market
            .goods
            .insert(2, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(1, MarketGood::new().with_amv(2.0).with_salability(1.0));
        market
            .goods
            .insert(9, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(8, MarketGood::new().with_amv(-2.0).with_salability(1.0));
        let history = market.history();
        let deal = ProposedDeal {
            buyer: Actor::Pop(1),
            seller: Actor::Pop(2),
            match_good: 2,
            goods: HashMap::from([(2, 4.0), (1, 1.0), (8, 1.0), (9, -6.0)]),
            fresh: HashMap::new(),
            freight: 0.0,
        };
        market.record_match_tape(&Actors::new(), &Factuals::new(), &history, &[deal]);
        assert!((market.goods[&2].print_holding - 4.0).abs() < 1e-9);
        assert!((market.goods[&2].print_units - 4.0).abs() < 1e-9);
        assert!((market.goods[&1].print_holding - 2.0).abs() < 1e-9);
        assert!((market.goods[&1].print_units - 1.0).abs() < 1e-9);
        assert_eq!(market.goods[&8].print_units, 0.0);
        assert!((market.goods[&9].paid - 6.0).abs() < 1e-9);
        assert!((market.goods[&2].traded - 4.0).abs() < 1e-9);
    }

    /// Nothing was paid, so the highest salability is restated at 1.
    #[test]
    fn record_keeping_pegs_the_highest_salability_when_nothing_was_paid() {
        let mut market = Market::new(1);
        let mut grain = MarketGood::new().with_amv(2.0).with_salability(1.0);
        grain.offered_unsold = 10.0;
        market.goods.insert(1, grain);
        market
            .goods
            .insert(2, MarketGood::new().with_amv(4.0).with_salability(1.5));
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&2].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&2].salability - 1.5).abs() < 1e-9);
        assert!((market.goods[&1].amv - 0.4875).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.975).abs() < 1e-9);
    }

    /// Equal salability and no payment goes to the lower id.
    #[test]
    fn record_keeping_breaks_an_unpaid_tie_toward_the_lower_id() {
        let mut market = Market::new(1);
        market
            .goods
            .insert(5, MarketGood::new().with_amv(8.0).with_salability(1.0));
        market
            .goods
            .insert(2, MarketGood::new().with_amv(4.0).with_salability(1.0));
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&2].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&5].amv - 2.0).abs() < 1e-9);
    }

    /// Equal salability goes to the good that was paid for more.
    ///
    /// Both are paid and neither price falls, so both salabilities rise by
    /// the same step and stay tied. The higher id carries the larger
    /// payment value and is restated at 1.
    #[test]
    fn record_keeping_breaks_a_salability_tie_toward_the_paid_value() {
        let mut market = Market::new(1);
        let mut small = MarketGood::new().with_amv(4.0).with_salability(1.0);
        small.paid = 1.0;
        let mut large = MarketGood::new().with_amv(8.0).with_salability(1.0);
        large.paid = 3.0;
        market.goods.insert(2, small);
        market.goods.insert(5, large);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&5].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 0.5).abs() < 1e-9);
        assert!((market.goods[&5].salability - 1.05).abs() < 1e-9);
        assert!((market.goods[&2].salability - 1.05).abs() < 1e-9);
    }

    /// # Insert Salability Anchor
    ///
    /// Inserts good 0 at AMV 1 and [`SALABILITY_MAX`].
    ///
    /// The other good then keeps the AMV this night's step wrote.
    fn insert_salability_anchor(market: &mut Market) {
        market.goods.insert(
            0,
            MarketGood::new()
                .with_amv(1.0)
                .with_salability(SALABILITY_MAX),
        );
    }
}
