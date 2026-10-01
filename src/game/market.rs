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

/// Salability clamp. `0..=1` is illiquid to par. Above 1 is at-par and currency.
pub const SALABILITY_MAX: f64 = 2.0;

/// Floor and ceiling of the factor that scales a positive AMV.
///
/// Salability below this floor still counts as the floor. Salability above 1
/// does not raise the factor past 1; that excess is [`monetary_rating`].
pub const AMV_SCALE_MIN: f64 = 0.05;

/// Largest one-night AMV move from unmet buys versus unsold offers, as a
/// fraction of the current absolute AMV.
const AMV_STEP_CAP: f64 = 0.25;

/// Largest one-night AMV move from production minus consumption, as a
/// fraction of the current absolute AMV.
const AMV_FLOW_CAP: f64 = 0.05;

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

    /// End-of-day market bookkeeping.
    ///
    /// Writes tomorrow's AMV and salability from each good's day record, then
    /// clears the exchange and flow counters. [`MarketGood::decayed`] and
    /// `volume` stay until the next morning's reset. Stock is left as it
    /// stands. `factuals` is unused: rot is added through [`Self::note_decay`]
    /// before this runs.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = factuals;
        let mut ids: Vec<usize> = self.goods.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let good = self.goods.get_mut(&id).expect("id was just copied from goods");
            let old = good.amv;
            let denom = good.sought_unmet + good.offered_unsold + good.traded + 1.0;
            let pressure = (good.sought_unmet - good.offered_unsold) / denom;
            let step = pressure.clamp(-AMV_STEP_CAP, AMV_STEP_CAP);
            let flow_denom = good.stock + good.production + good.consumption + 1.0;
            let flow = ((good.consumption - good.production) / flow_denom)
                .clamp(-AMV_FLOW_CAP, AMV_FLOW_CAP);
            let mut next = old + old.abs().max(AMV_EPSILON) * (step + flow);
            let decayed = good.decayed;
            let decay_base = if good.volume > 0.0 {
                good.volume
            } else {
                good.stock
            };
            if decay_base > 0.0 && decayed > 0.0 {
                let fraction = (decayed / decay_base).clamp(0.0, 1.0);
                next -= next.abs() * fraction;
            }
            let paid = good.paid;
            let informed = good.traded + good.paid + good.offered_unsold > 0.0;
            good.set_amv(next);
            let fell = good.amv < old;
            if fell && old.abs() >= AMV_EPSILON {
                let drop = ((old - good.amv) / old.abs()).min(SALABILITY_LOSS_CAP);
                good.set_salability(good.salability - drop);
            } else if informed && paid > 0.0 {
                good.set_salability(good.salability + SALABILITY_UP_STEP);
            }
            good.clear_exchange();
        }
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
    /// Clears yesterday's day-records and each member's
    /// [`crate::game::deal::DealMaker::reset_day`],
    /// then reserve, produce, [`Self::match_deals`], consume, decay, then actor
    /// record keeping and planning, then [`Self::record_keeping`].
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
        // Cleanup Phase. Drop yesterday before this day records anything.
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
        // Decay Phase
        for actor in &members {
            for (good, (lost, volume)) in actors.get_mut(*actor).decay_goods(factuals) {
                self.note_decay(good, lost, volume);
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
    /// `actors` supplies the members. AMV, salability, and stock stay.
    /// Pop satisfaction and reserves go to zero. Firms and institutions
    /// keep the empty [`crate::game::deal::DealMaker::reset_day`] default.
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

    /// Record units traded and what was still on the book when matching stopped.
    ///
    /// Does not write AMV or salability. Those move in [`Self::record_keeping`].
    fn record_match_tape(
        &mut self,
        actors: &Actors,
        factuals: &Factuals,
        history: &MarketHistory,
        deals: &[ProposedDeal],
    ) {
        for deal in deals {
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
    /// Clamped to `0.0..=`[`SALABILITY_MAX`].
    pub salability: f64,
    /// Units made today.
    pub production: f64,
    /// Units consumed today.
    pub consumption: f64,
    /// Units already in the market from yesterday.
    pub stock: f64,
    /// Units that changed hands in accepted deals today.
    pub traded: f64,
    /// Units that changed hands as payment today.
    pub paid: f64,
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

    /// Sets salability, clamped to `0.0..=`[`SALABILITY_MAX`].
    pub fn set_salability(&mut self, salability: f64) {
        debug_assert!(salability.is_finite(), "salability must be finite");
        self.salability = salability.clamp(0.0, SALABILITY_MAX);
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
    /// Zeros today's exchange and production flow.
    ///
    /// Leaves AMV, salability, stock, and rot (`decayed`, `volume`).
    fn clear_exchange(&mut self) {
        self.production = 0.0;
        self.consumption = 0.0;
        self.traded = 0.0;
        self.paid = 0.0;
        self.sought_unmet = 0.0;
        self.offered_unsold = 0.0;
    }

    /// # Clear Day
    ///
    /// Zeros today's flows, including rot.
    ///
    /// Leaves AMV, salability, and stock. The morning reset uses this so
    /// yesterday's rot does not feed the next night.
    fn clear_day(&mut self) {
        self.clear_exchange();
        self.decayed = 0.0;
        self.volume = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        amv_scale, monetary_rating, Market, MarketGood, MarketHistory,
        SALABILITY_DEFAULT,
    };

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

    #[test]
    fn record_keeping_lowers_an_unsold_good_and_clears_the_tape() {
        let mut market = Market::new(1);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.0);
        good.offered_unsold = 10.0;
        market.goods.insert(1, good);
        let before = market.history();
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((before.price(1) - 2.0).abs() < 1e-12);
        assert!((market.goods[&1].amv - 1.5).abs() < 1e-9);
        assert!((market.goods[&1].salability - 0.8).abs() < 1e-9);
        assert_eq!(market.goods[&1].offered_unsold, 0.0);
    }

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

    #[test]
    fn record_keeping_cuts_amv_by_the_share_that_rotted() {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_amv(2.0).with_salability(1.5));
        market.note_decay(1, 5.0, 10.0);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.3).abs() < 1e-9);
        assert!((market.goods[&1].decayed - 5.0).abs() < 1e-9);
        assert!((market.goods[&1].volume - 10.0).abs() < 1e-9);
    }

    #[test]
    fn record_keeping_production_lowers_amv_and_salability_with_it() {
        let mut market = Market::new(1);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(1.5);
        good.production = 10.0;
        market.goods.insert(1, good);
        market.record_keeping(&crate::game::factuals::Factuals::new());
        // flow cap 0.05 of |amv|: 2 - 0.1 = 1.9. That loss also ticks salability.
        assert!((market.goods[&1].amv - 1.9).abs() < 1e-9);
        assert!((market.goods[&1].salability - 1.45).abs() < 1e-9);
        assert_eq!(market.goods[&1].production, 0.0);
    }
}
