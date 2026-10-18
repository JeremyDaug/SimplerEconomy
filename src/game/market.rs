use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::actors::Actors;
use crate::game::deal::{matched_on, DealResponse, Meeting, MeetingOutcome, ProposedDeal, SellerBook};
use crate::game::marketorder::MarketOrder;
use crate::game::factuals::Factuals;
use crate::game::good::GoodTag;
use crate::game::pop::MigrantGroup;

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

/// Largest one-night holding move from the print and the unsold ease, as a share of the payment scale.
const AMV_STEP_CAP: f64 = 0.05;

/// Flat share of the payment scale subtracted when a good was offered and nothing sold.
const AMV_EXCESS_NUDGE: f64 = 0.005;

/// Largest one-night production-flow fraction.
const AMV_FLOW_CAP: f64 = 0.005;

/// Flat share of the payment scale added when buy orders were still open.
const AMV_UNMET_NUDGE: f64 = 0.01;

/// Physical rot is divided by this before it cuts absolute AMV.
const AMV_ROT_DIVISOR: f64 = 4.0;

/// Largest share of absolute AMV that one night of rot may remove.
const AMV_ROT_CAP: f64 = 1.0;

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
    /// Production flow is a share of [`Self::payment_scale`]. When
    /// `sought_unmet` is positive, that night's rot is added back and
    /// [`AMV_UNMET_NUDGE`] times the payment scale is added on top.
    /// `factuals` is unused: rot is added through [`Self::note_decay`]
    /// before this runs.
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
            let mut rot_cut = 0.0;
            if decay_base > 0.0 && decayed > 0.0 {
                let fraction = (decayed / decay_base / AMV_ROT_DIVISOR).clamp(0.0, AMV_ROT_CAP);
                rot_cut = next.abs() * fraction;
                next -= rot_cut;
            }
            // Open buys. That rot goes back on, then the unmet step.
            if good.sought_unmet > 0.0 {
                next += rot_cut + scale * AMV_UNMET_NUDGE;
            }
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

    /// # History
    ///
    /// Snapshot of stored AMV, salability, units still sought, and production.
    ///
    /// Sought is copied from [`MarketGood::sought_unmet`]. Production is
    /// copied from [`MarketGood::production`]. A snapshot taken before
    /// exchange has no sought units. The night plan reads it.
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
        history.friction = self.friction;
        history
    }

    /// # Market Day
    ///
    /// One day for the actors registered on this market.
    ///
    /// Clears yesterday's exchange tape and rot, and each member's
    /// [`crate::game::deal::DealMaker::reset_day`],
    /// then reserve, produce, a second pop satisfy for what was just made,
    /// a resettle of each pop's input buys, [`Self::match_deals`], consume,
    /// [`crate::game::pop::Pop::update_sentiments`], pop growth,
    /// [`Self::rebalance_population_cap`],
    /// [`Self::prune_pops_below_one_household`],
    /// [`Self::internal_migration`], decay,
    /// [`crate::game::pop::Pop::rescale_desires`], then actor
    /// record keeping and planning, then [`Self::record_keeping`].
    ///
    /// Sentiment runs before growth, so today's mood scales this year's births.
    /// Rescale is after decay, so this day's desire effects stay at the
    /// previous size. It also runs after migration, so satisfaction and
    /// targets match the size still on the market. Planning then reads those targets.
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
        // Reserve what this morning made, so the buy is the desire still short.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).satisfy_made(rng);
            }
        }
        // Buy only the input that make left short.
        for actor in &members {
            if let Actor::Pop(id) = *actor {
                actors.pop_mut(id).settle_shopping(factuals);
            }
        }
        // Exchange Phase
        let meetings = self.match_deals(actors, factuals, rng);
        // Consumption Phase
        for actor in &members {
            actors.get_mut(*actor).consume();
        }
        // Sentiment. Consume has written today's satisfaction. Growth reads it.
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
    /// have a buy, and a seller from the valid matches in proportion to the
    /// units that seller offers.
    /// The buyer sees that seller's offers and requests and either proposes
    /// a basket or abandons. A pop who cannot propose one unit against that
    /// seller is not given a meeting. That buy stays posted, and the pair
    /// is left unmarked, so a later meeting in this call can still match
    /// them once the buyer's stock covers the unit. The seller accepts or
    /// rejects. An accepted basket is finalized, freight included. The buyer
    /// pays that freight from transport they already hold and from transport
    /// the basket buys.
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
            let mut offers_of: HashMap<Actor, Vec<MarketOrder>> = HashMap::new();
            for order in &sells {
                offers_of.entry(order.origin).or_default().push(order.clone());
            }
            let mut seller_buys: HashMap<Actor, Vec<MarketOrder>> = HashMap::new();
            let mut candidates = Vec::new();
            for buyer in &members {
                for buy in actors.get(*buyer).buy_orders(&history) {
                    if buy.target_amount < 1.0 || !tradeable(factuals, buy.target) {
                        continue;
                    }
                    let matches: Vec<MarketOrder> = sells
                        .iter()
                        .filter(|sell| {
                            if !matched_on(&buy, sell)
                                || tried.contains(&(*buyer, sell.origin, buy.target))
                            {
                                return false;
                            }
                            // A pop who cannot propose one unit against this seller is not a meeting yet.
                            let Actor::Pop(id) = *buyer else {
                                return true;
                            };
                            let requests = seller_buys.entry(sell.origin).or_insert_with(|| {
                                actors.get(sell.origin).buy_orders(&history)
                            });
                            let offers = offers_of
                                .get(&sell.origin)
                                .map(Vec::as_slice)
                                .unwrap_or(&[]);
                            actors.pop(id).can_propose_one(
                                buy.target,
                                offers,
                                requests,
                                &history,
                                factuals,
                            )
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
            // A larger offer is drawn more often, in proportion to its units.
            let sell = pick_seller(&matches, rng).clone();
            tried.insert((buyer, sell.origin, buy.target));
            let meeting = self.meet(actors, buyer, &sell, &history, factuals, rng);
            if meeting.outcome == MeetingOutcome::Accepted {
                if let Some(deal) = meeting.proposal.clone() {
                    accepted.push(deal);
                }
            }
            meetings.push(meeting);
            // Both piles may have changed. Read sell orders again so a new surplus is offered.
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
    /// households. A one-point gap, with no fear or anger, is a tenth.
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

/// # Pick Seller
///
/// One order from `matches`, drawn in proportion to the units it offers.
///
/// `rng` supplies the draw. `matches` is not empty. [`offered_units`] is
/// the weight. A larger pile is drawn more often by that many units. An
/// order with no whole unit to sell is skipped. When every weight is zero,
/// the draw is uniform over `matches`.
fn pick_seller<'a>(matches: &'a [MarketOrder], rng: &mut impl rand::RngCore) -> &'a MarketOrder {
    let weights: Vec<f64> = matches.iter().map(offered_units).collect();
    let total: f64 = weights.iter().copied().sum();
    if total <= 0.0 {
        return &matches[crate::game::util::random_index(rng, matches.len())];
    }
    let mut mark = unit_interval(rng) * total;
    for (order, weight) in matches.iter().zip(weights) {
        if weight <= 0.0 {
            continue;
        }
        mark -= weight;
        if mark < 0.0 {
            return order;
        }
    }
    matches
        .iter()
        .rev()
        .find(|order| offered_units(order) > 0.0)
        .unwrap_or(&matches[0])
}

/// # Offered Units
///
/// Whole units `order` is selling.
///
/// A buy, a non-finite amount, and a sell under one unit return 0. The
/// weight [`pick_seller`] uses.
fn offered_units(order: &MarketOrder) -> f64 {
    if !order.target_amount.is_finite() || order.target_amount >= 0.0 {
        return 0.0;
    }
    (-order.target_amount).floor().max(0.0)
}

/// # Unit Interval
///
/// A draw in `[0, 1)` from `rng`.
///
/// Uses 53 bits so the value lands on an `f64` mantissa. [`pick_seller`]
/// scales it by the total weight.
fn unit_interval(rng: &mut impl rand::RngCore) -> f64 {
    let bits = rng.next_u64() >> 11;
    (bits as f64) / ((1_u64 << 53) as f64)
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
    /// Units still on buy orders after the day's meetings.
    ///
    /// Empty on a snapshot taken before exchange.
    pub sought: HashMap<usize, f64>,
    /// Units produced today, per good.
    ///
    /// Copied from [`MarketGood::production`]. Empty when nothing was made.
    pub produced: HashMap<usize, f64>,
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
            sought: HashMap::new(),
            produced: HashMap::new(),
            friction: 0.0,
            default_salability: SALABILITY_DEFAULT,
        }
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

/// Anger weighs this much beside fear on the way out.
const MIGRATION_ANGER_WEIGHT: f64 = 0.5;
/// Share of a saturated push or pull that moves in one year.
///
/// A one-point living gap saturates at one half, so this pace moves a
/// tenth of the households. One year stays under this share of the pop.
const MIGRATION_PACE: f64 = 0.2;
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
    if weight == 0.0 { f64::NAN } else { sum / weight }
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
            if part == 0.0 { None } else { Some((good, part)) }
        })
        .collect()
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
            if left == 0.0 { None } else { Some((good, left)) }
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
    use std::collections::HashMap;

    use super::{
        amv_scale, monetary_rating, pick_seller, Market, MarketGood, MarketHistory, AMV_EPSILON,
        SALABILITY_DEFAULT, SALABILITY_MAX, SALABILITY_MIN,
    };
    use rand::SeedableRng;

    use crate::game::actor::Actor;
    use crate::game::actors::Actors;
    use crate::game::culture::Culture;
    use crate::game::deal::ProposedDeal;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::Desire;
    use crate::game::factuals::Factuals;
    use crate::game::job::Job;
    use crate::game::marketorder::MarketOrder;
    use crate::game::pop::{Pop, PopPRow};
    use crate::game::religion::Religion;
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::sentiment::Sentiment;

    /// Nine of the ten units sit on one seller. That seller is drawn most of the time.
    #[test]
    fn pick_seller_favors_the_larger_offer() {
        let matches = vec![
            MarketOrder::sell(Actor::Pop(1), 1, 1.0),
            MarketOrder::sell(Actor::Pop(2), 1, 9.0),
        ];
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut large = 0;
        let draws = 1000;
        for _ in 0..draws {
            if pick_seller(&matches, &mut rng).origin == Actor::Pop(2) {
                large += 1;
            }
        }
        assert!(large > 800, "large seller chosen {large} of {draws}");
    }

    /// A buy and a sell under one unit have no weight. The whole offer is drawn.
    #[test]
    fn pick_seller_skips_orders_with_no_whole_unit() {
        let matches = vec![
            MarketOrder::buy(Actor::Pop(1), 1, 5.0),
            MarketOrder::sell(Actor::Pop(2), 1, 0.4),
            MarketOrder::sell(Actor::Pop(3), 1, 4.0),
        ];
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        for _ in 0..20 {
            assert_eq!(pick_seller(&matches, &mut rng).origin, Actor::Pop(3));
        }
    }

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
        assert!((market.goods[&1].amv - 1.995).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.9975).abs() < 1e-9);
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

    /// Half the pile rots, so the price loses an eighth.
    #[test]
    fn record_keeping_cuts_amv_by_a_quarter_of_the_rot_share() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market.goods.insert(1, MarketGood::new().with_amv(2.0).with_salability(1.5));
        market.note_decay(1, 5.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.75).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.49).abs() < 1e-9);
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
        assert!((market.goods[&1].salability - 1.49).abs() < 1e-9);
    }

    /// Rot that takes the whole price bounces through zero.
    ///
    /// `40 / 10 / 4` is 1, and the rot cap is the whole price. The dead zone
    /// stores `-AMV_EPSILON`. Salability loses only the loss cap.
    #[test]
    fn record_keeping_total_rot_bounces_through_zero() {
        let mut market = Market::new(1);
        insert_salability_anchor(&mut market);
        market.goods.insert(1, MarketGood::new().with_amv(2.0).with_salability(1.5));
        market.note_decay(1, 40.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - -AMV_EPSILON).abs() < 1e-15);
        assert!((market.goods[&1].salability - 1.49).abs() < 1e-9);
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

        assert!((market.goods[&1].amv - 1.995).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.4975).abs() < 1e-9);
        assert_eq!(market.goods[&1].production, 0.0);
        assert_eq!(market.goods[&1].decayed, 0.0);
        assert_eq!(market.goods[&1].traded, 0.0);
    }

    /// Open buys rise by the unmet step. Salability still picks the unit.
    ///
    /// The coin was paid, so its salability rises, and a negative-AMV payment
    /// cannot be the unit. The unpaid good at the salability cap is restated
    /// at 1. The payment scale is 4, so each sought good gains 0.04 before
    /// that restatement.
    #[test]
    fn record_keeping_raises_unmet_buys_and_pegs_the_unit() {
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
        assert!((market.goods[&1].amv - 4.005).abs() < 1e-9);
        assert!((market.goods[&2].amv - 0.03).abs() < 1e-9);
        assert!((market.goods[&3].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&9].amv - 0.5).abs() < 1e-9);
        assert!((market.goods[&8].amv - -0.25).abs() < 1e-9);
    }

    /// Open buys put the night's rot back and add a step of the payment scale.
    ///
    /// Half of this pile would take an eighth of the price. Nothing was paid,
    /// so the scale is 1 and the step is 0.01.
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
    /// The coin's holding value is 4, so the cap is 0.2. The coin is then
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
        assert!((market.goods[&2].amv - 0.3).abs() < 1e-9);
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
        assert!((market.goods[&1].amv - 0.49875).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.9975).abs() < 1e-9);
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

    /// One point under the average offers a tenth of the households.
    /// A twentieth of the stock goes along.
    #[test]
    fn internal_migration_moves_a_tenth_for_one_point_of_living() {
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
        assert!((origin.demographics.household.count - 9.0).abs() < 1e-9);
        assert!((dest.demographics.household.count - 11.0).abs() < 1e-9);
        assert!((origin.property[&1].quantity - 190.0).abs() < 1e-9);
        assert!((dest.property[&1].quantity - 210.0).abs() < 1e-9);
        assert!((origin.property[&1].used - 7.6).abs() < 1e-9);
        assert!((dest.property[&1].used - 0.4).abs() < 1e-9);
        assert!((origin.property[&1].fresh - 38.0).abs() < 1e-9);
        assert_eq!(dest.property[&1].fresh, 0.0);
        assert!((origin.demographics.household.adult - 4.0).abs() < 1e-9);
        assert!((dest.demographics.household.adult - 24.0 / 11.0).abs() < 1e-9);
        assert!((dest.demographics.household.adult_mf - 14.0 / 24.0).abs() < 1e-9);
        assert!((dest.demographics.household.elder - 5.0 / 11.0).abs() < 1e-9);
        assert!((dest.demographics.household.child - 25.0 / 11.0).abs() < 1e-9);
        assert!((dest.demographics.household.adult_labor - 10.0 / 11.0).abs() < 1e-9);
        assert!((dest.demographics.household.partnership_rate - 29.0 / 11.0).abs() < 1e-9);
        assert!((origin.migration - -1.0).abs() < 1e-9);
        assert!((dest.migration - 1.0).abs() < 1e-9);
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

        assert!((actors.pop(1).demographics.household.count - 8.8).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 11.2).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 94.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 106.0).abs() < 1e-9);
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

        assert!((actors.pop(1).demographics.household.count - 9.5).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.5).abs() < 1e-9);
        assert!((actors.pop(1).property[&1].quantity - 195.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 405.0).abs() < 1e-9);
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
        // Cap is 2 * 1/4 = 0.5, so half of the 1-household room.
        assert!((actors.pop(1).demographics.household.count - 9.5).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.5).abs() < 1e-9);
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

        assert!((actors.pop(1).demographics.household.count - 9.25).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.75).abs() < 1e-9);
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

        assert!((actors.pop(1).demographics.household.count - 9.25).abs() < 1e-9);
        assert!((actors.pop(2).demographics.household.count - 10.75).abs() < 1e-9);
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
        let kept = actors.pop(1).demographics.household.count + actors.pop(3).demographics.household.count;
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

    /// A one-point score saturates at one half, then the pace keeps a tenth.
    #[test]
    fn migration_fraction_stays_under_the_pace() {
        assert!((super::migration_fraction(1.0) - 0.1).abs() < 1e-12);
        assert!((super::migration_fraction(1.0e9) - super::MIGRATION_PACE).abs() < 1e-6);
        assert_eq!(super::migration_fraction(0.0), 0.0);
    }
}
