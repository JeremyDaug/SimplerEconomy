use std::collections::{HashMap, HashSet};

use circular_buffer::CircularBuffer;
use rand::Rng;

use crate::game::actor::Actor;
use crate::game::config::{market_constants, market_priority, MarketConfig};
use crate::game::deal::{DealMaker, DealResponse};
use crate::game::firm::Firm;
use crate::game::good::TIME;
use crate::game::marketorder::MarketOrder;
use crate::game::pop::Pop;
use crate::game::util::{lerp, whole_units};
use crate::game::workforce::LaborSettlement;
use crate::game::{actors::Actors, factuals::Factuals};

/// One buy/sell pair from [`Market::match_orders`].
///
/// Indices refer to the `buys` / `sells` slices passed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderMatch {
    pub buy_index: usize,
    pub sell_index: usize,
}

/// One matching pass: at most one deal. When none, `unmatched_buys` is every
/// remaining buy (no other-origin seller).
///
/// Indices refer to the slices passed in. The matcher does not remove them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderMatchBatch {
    pub matched: Option<OrderMatch>,
    pub unmatched_buys: Vec<usize>,
}

impl OrderMatchBatch {
    fn empty() -> Self {
        Self {
            matched: None,
            unmatched_buys: Vec::new(),
        }
    }

    /// True when there is nothing for the caller to deal or update.
    pub fn is_empty(&self) -> bool {
        self.matched.is_none() && self.unmatched_buys.is_empty()
    }
}

/// Why a matched pair washed instead of trading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WashReason {
    /// Buyer [`DealMaker::buy`] returned `None`.
    NoProposal,
    /// Seller did not [`DealResponse::Accept`].
    Rejected,
    /// Accepted map did not move the target good.
    EmptyFill,
}

/// Result of one matched buy/sell meeting.
#[derive(Debug, Clone, PartialEq)]
pub enum MeetingOutcome {
    /// Seller accepted. `goods` is the seller's inventory change.
    Traded {
        goods: HashMap<usize, f64>,
        transport_needed: f64,
    },
    /// No trade. Flat meeting fee from on-hand. `closed` means the buy was
    /// not renewed.
    Wash {
        reason: WashReason,
        transport: f64,
        closed: bool,
    },
}

/// One buy/sell pair that reached `settle_pair` (traded or washed).
#[derive(Debug, Clone, PartialEq)]
pub struct MarketMeeting {
    pub buy: MarketOrder,
    pub sell: MarketOrder,
    pub outcome: MeetingOutcome,
}

/// What [`Market::run_market_day`] did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarketDayReport {
    /// Buys with no other-origin seller. Marked unavailable. Not a meeting.
    pub unmatched_buys: Vec<MarketOrder>,
    /// Each matched pair, in day order, traded or washed.
    pub meetings: Vec<MarketMeeting>,
    /// Buys still in the book when the loop stopped.
    pub leftover_buys: Vec<MarketOrder>,
    /// Sells still in the book when the loop stopped.
    pub leftover_sells: Vec<MarketOrder>,
}

/// Walk `weights` with a `roll` in `[0, sum)`. Last index wins leftover float dust.
fn pick_weighted_index(weights: &[f64], mut roll: f64) -> usize {
    for (i, weight) in weights.iter().enumerate() {
        if *weight <= 0.0 {
            continue;
        }
        if roll < *weight {
            return i;
        }
        roll -= *weight;
    }
    weights.len().saturating_sub(1)
}

/// Sell selection weight for this pick: listed units, times coincidence
/// when both named counters match.
fn sell_match_weight_with(
    buy: &MarketOrder,
    sell: &MarketOrder,
    coincidence_weight: f64,
) -> f64 {
    let mut weight = (-sell.target_amount).max(0.0);
    if matching_counter_offers(buy, sell) {
        weight *= coincidence_weight;
    }
    weight
}

fn matching_counter_offers(buy: &MarketOrder, sell: &MarketOrder) -> bool {
    match (buy.counter_offer, sell.counter_offer) {
        (Some(buy_good), Some(sell_good)) => buy_good == sell_good,
        _ => false,
    }
}

/// Other-origin sells of `buy.target`. `had_other` is true if any exist.
fn classify_sells(
    sells: &[MarketOrder],
    buy: &MarketOrder,
    coincidence_weight: f64,
) -> (Vec<usize>, Vec<f64>, bool) {
    let start = sells.partition_point(|s| s.target < buy.target);
    let end = start + sells[start..].partition_point(|s| s.target == buy.target);
    let mut available = Vec::new();
    let mut weights = Vec::new();
    for i in start..end {
        if sells[i].origin == buy.origin || sells[i].target_amount >= 0.0 {
            continue;
        }
        available.push(i);
        weights.push(sell_match_weight_with(buy, &sells[i], coincidence_weight));
    }
    let had_other = !available.is_empty();
    (available, weights, had_other)
}

fn pick_available_sell<R: Rng + ?Sized>(
    available: &[usize],
    weights: &[f64],
    rng: &mut R,
) -> Option<usize> {
    if available.is_empty() {
        return None;
    }
    let total: f64 = weights.iter().sum();
    let pick = if total > 0.0 && total.is_finite() {
        pick_weighted_index(weights, rng.random_range(0.0..total))
    } else {
        rng.random_range(0..available.len())
    };
    Some(available[pick])
}



impl Market {
    fn note_buy_stops(
        &self,
        pops: &mut HashMap<usize, Pop>,
        factuals: &Factuals,
        history: &MarketHistory,
        door: f64,
    ) {
        let mut ids: Vec<usize> = self.pops.iter().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let pop = pops
                .get_mut(&id)
                .unwrap_or_else(|| panic!("market pop {id} missing from pops"));
            pop.records.buy_stop = pop.classify_buy_stop(
                factuals,
                history,
                &self.unavailable_goods,
                door,
            );
        }
    }
}

/// Pushes each order into the buy book (`target_amount` > 0) or the sell book
/// (`target_amount` < 0). Zero-amount orders are dropped.
fn split_into_books(
    orders: Vec<MarketOrder>,
    buys: &mut Vec<MarketOrder>,
    sells: &mut Vec<MarketOrder>,
) {
    for order in orders {
        if order.target_amount > 0.0 {
            buys.push(order);
        } else if order.target_amount < 0.0 {
            sells.push(order);
        }
    }
}

/// Returns `order` with `filled` units removed, or `None` if nothing remains.
/// Scales a named counter amount by the same remaining/original ratio.
/// Leftover target and counter amounts are whole units.
fn leftover_order(mut order: MarketOrder, filled: f64) -> Option<MarketOrder> {
    debug_assert!(filled >= 0.0, "filled must be >= 0.0");
    let original = order.target_amount;
    if original > 0.0 {
        order.target_amount = whole_units(order.target_amount - filled);
        if order.target_amount <= 0.0 {
            return None;
        }
    } else {
        order.target_amount = whole_units(order.target_amount + filled);
        if order.target_amount >= 0.0 {
            return None;
        }
    }
    if let Some(counter) = order.counter_offer_amount.as_mut() {
        if original != 0.0 {
            let scaled = *counter * order.target_amount / original;
            let mut qty = whole_units(scaled);
            if qty == 0.0 && scaled != 0.0 {
                qty = scaled.signum();
            }
            *counter = qty;
        }
    }
    Some(order)
}

/// Returns on-hand quantity of `good` for `actor`, or 0 if missing.
fn actor_on_hand(
    pops: &HashMap<usize, Pop>,
    firms: &HashMap<usize, Firm>,
    actor: Actor,
    good: usize,
) -> f64 {
    match actor {
        Actor::Pop(id) => pops
            .get(&id)
            .and_then(|pop| pop.property.get(&good))
            .map(|row| row.quantity)
            .unwrap_or(0.0),
        Actor::Firm(id) => firms
            .get(&id)
            .and_then(|firm| firm.property.get(&good))
            .map(|row| row.shelf())
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Shrinks leftover sell/offer orders so they do not exceed this actor's
/// on-hand stock. A buy can tender a good that is also listed for sale;
/// without this, the later sell still asks for the morning amount.
fn clamp_sells_to_on_hand(
    sells: &mut Vec<MarketOrder>,
    actor: Actor,
    pops: &HashMap<usize, Pop>,
    firms: &HashMap<usize, Firm>,
) {
    let mut i = 0;
    while i < sells.len() {
        if sells[i].origin != actor {
            i += 1;
            continue;
        }
        let have = whole_units(actor_on_hand(pops, firms, actor, sells[i].target).max(0.0));
        let listed = -sells[i].target_amount;
        if have <= 0.0 {
            sells.remove(i);
            continue;
        }
        if listed > have {
            match leftover_order(sells[i].clone(), listed - have) {
                Some(order) => sells[i] = order,
                None => {
                    sells.remove(i);
                    continue;
                }
            }
        }
        i += 1;
    }
}

/// Charges the flat transport meeting fee from on-hand, pushes `sell_order`
/// back onto `sells`, and asks the buyer to [`DealMaker::renew_buy`].
/// Returns true if the buy was renewed.
fn wash_pair(
    buy_order: MarketOrder,
    sell_order: MarketOrder,
    factuals: &Factuals,
    pops: &mut HashMap<usize, Pop>,
    firms: &mut HashMap<usize, Firm>,
    buys: &mut Vec<MarketOrder>,
    sells: &mut Vec<MarketOrder>,
) -> bool {
    as_deal_maker_mut(pops, firms, buy_order.origin)
        .pay_transport(factuals.config.market.transaction_cost, factuals);
    sells.push(sell_order);
    if let Some(renewed) = as_deal_maker(pops, firms, buy_order.origin)
        .renew_buy_with_limit(&buy_order, factuals.config.market.buy_try_limit)
    {
        buys.push(renewed);
        true
    } else {
        false
    }
}

/// Flat wash fee when the world has transport-tagged goods, else 0.
fn wash_transport(factuals: &Factuals) -> f64 {
    if factuals.goods.values().any(|good| good.is_transport()) {
        factuals.config.market.transaction_cost
    } else {
        0.0
    }
}

/// Looks up `actor` as a [`DealMaker`]. Pops and firms must be in the maps.
/// Institution and state DealMaker impls are not wired yet.
fn as_deal_maker<'a>(
    pops: &'a HashMap<usize, Pop>,
    firms: &'a HashMap<usize, Firm>,
    actor: Actor,
) -> &'a dyn DealMaker {
    match actor {
        Actor::Pop(id) => {
            pops.get(&id).unwrap_or_else(|| panic!("market pop {id} missing from pops"))
        }
        Actor::Firm(id) => {
            firms.get(&id).unwrap_or_else(|| panic!("market firm {id} missing from firms"))
        }
        Actor::Institution(_) | Actor::State(_) => {
            panic!("DealMaker not wired for {actor:?}")
        }
    }
}

/// Looks up `actor` as a mutable [`DealMaker`]. Pops and firms must be in the maps.
/// Institution and state DealMaker impls are not wired yet.
fn as_deal_maker_mut<'a>(
    pops: &'a mut HashMap<usize, Pop>,
    firms: &'a mut HashMap<usize, Firm>,
    actor: Actor,
) -> &'a mut dyn DealMaker {
    match actor {
        Actor::Pop(id) => {
            pops.get_mut(&id).unwrap_or_else(|| panic!("market pop {id} missing from pops"))
        }
        Actor::Firm(id) => {
            firms.get_mut(&id).unwrap_or_else(|| panic!("market firm {id} missing from firms"))
        }
        Actor::Institution(_) | Actor::State(_) => {
            panic!("DealMaker not wired for {actor:?}")
        }
    }
}

fn record_firm_sell_meet(
    firms: &mut HashMap<usize, Firm>,
    origin: Actor,
    good: usize,
    fills: f64,
    rejects: f64,
    no_proposal: f64,
) {
    let Actor::Firm(id) = origin else {
        return;
    };
    if let Some(firm) = firms.get_mut(&id) {
        firm.note_sell_meet(good, fills, rejects, no_proposal);
    }
}

/// If `new` is inside the AMV dead zone, land `min_abs` on the other side
/// of 0 from `old`. Otherwise return `new` unchanged.
fn bounce_away_from_zero(old: f64, new: f64, min_abs: f64) -> f64 {
    debug_assert!(new.is_finite(), "new must be finite");
    debug_assert!(min_abs > 0.0, "min_abs must be > 0.0");
    if new.abs() >= min_abs {
        new
    } else if old >= 0.0 {
        -min_abs
    } else {
        min_abs
    }
}

/// # Market
/// 
/// The market holds the actors and acts on stuff. It is what abstracts physical details
/// from the region away and consolidates it into the goods that can be bought, sold,
/// and traded.
/// 
/// Things which can't be moved out of a region can't be moved out of a market, and so on.
#[derive(Debug, Clone)]
pub struct Market {
    /// The unique ID of the market, should match the ID of the region it represents.
    pub id: usize,
    /// The pops in the market.
    pub pops: HashSet<usize>,
    /// The firms in the market.
    pub firms: HashSet<usize>,
    /// Institutions present in this market (membership only; actors own the data).
    ///
    /// An institution may appear in multiple markets via its own `markets` list.
    pub institution_ids: HashSet<usize>,
    /// The goods in the market and records of them available to all.
    /// 
    /// If needed, this will have to be culled and cleaned out of old goods periodically.
    /// 
    /// The key is the ID of the good.
    pub goods: HashMap<usize, MarketGood>,
    /// Distance / size multiplier on deal bulk. 0 on a one-hex market.
    /// Transport bill is `TRANSACTION_COST + bulk * friction`.
    pub friction: f64,
    /// Goods with no other-origin seller today. Passed into `create_orders`.
    /// Cleared at market-day start; unmatched buys insert here.
    pub unavailable_goods: HashSet<usize>,
    /// Market days completed. Incremented at the end of each
    /// [`Market::run_market_day`]. AMV rescale uses this against
    /// [`MarketConfig::amv_rescale_period`].
    pub market_days: u32,
    /// Unfilled buy/request units by good at market close (leftover book plus
    /// unmatched). Plan treats this as remaining demand, not a sell miss.
    pub leftover_buy: HashMap<usize, f64>,
}

impl Market {
    /// Empty market with this id. No pops, firms, institutions, or goods.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            pops: HashSet::new(),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            friction: 0.0,
            unavailable_goods: HashSet::new(),
            market_days: 0,
            leftover_buy: HashMap::new(),
        }
    }

    /// Sets the market friction factor. Must be `>= 0.0`.
    pub fn with_friction(mut self, friction: f64) -> Self {
        debug_assert!(friction >= 0.0, "friction must be >= 0.0");
        self.friction = friction;
        self
    }

    /// End-of-day market bookkeeping (prices, volume history, clear day locals, …).
    /// Only external input is factuals; does not touch actors.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Market record keeping")
    }

    /// Aggregate pop emigration and firm hiring pressures for this market region.
    /// Reads actor pressures already computed; does not move actors between markets.
    pub fn sum_migratory_pressure(&mut self, actors: &Actors, factuals: &Factuals) {
        let _ = (self, actors, factuals);
        todo!("Market sum migratory pressure (positive / negative / net, migrant pool)")
    }

    /// # Run Market Day
    ///
    /// Runs this market's intramarket day.
    ///
    /// 1. Collect orders from member pops and firms (`create_orders`) once.
    ///    Institution and state orders are not collected yet.
    /// 2. Collate opening supply, demand, buyers, and suppliers onto
    ///    [`MarketGood`] rows.
    /// 3. Match until quiet: pick a buy at random among those with an
    ///    other-origin sell, pick that sell by listed amount (coincidence
    ///    multiplies). Same-origin pairs are skipped. Matched pair: buyer
    ///    `buy`, seller `evaluate`. Accept -> finalize + wagon bill;
    ///    leftover orders scale down and stay. Wash: flat door fee; buyer
    ///    may renew up to `BUY_TRY_LIMIT`. Buys with no seller become
    ///    unmatched / [`Market::unavailable_goods`].
    /// 4. Cleanup: clear member pops' `current_orders`. AMV is written on
    ///    [`MarketGood`] as meetings resolve (history stays the opening
    ///    snapshot). Leftover books do not move AMV. Salability updates
    ///    from payment/tender after the loop. Then live AMV is rescaled so
    ///    one unit of each tradeable good averages `amv_rescale_mean` (daily
    ///    by default). Time is skipped (labor deals already wrote it). Firm
    ///    AMV quotes are scaled by the same factor, and
    ///    the close is recorded. The AMV trail is not rewritten. Leftover
    ///    rot cap is a later caller ([`Market::cap_salability_from_decay`])
    ///    after decay, not this method. Leftover book carry and re-planning
    ///    are deferred.
    ///
    /// Returns a [`MarketDayReport`] of unmatched buys, each meeting, and
    /// leftover book orders.
    pub fn run_market_day<R: Rng + ?Sized>(
        &mut self,
        factuals: &Factuals,
        pops: &mut HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
        rng: &mut R,
    ) -> MarketDayReport {
        self.unavailable_goods.clear();
        for &id in &self.firms {
            firms
                .get_mut(&id)
                .unwrap_or_else(|| panic!("market firm {id} missing from firms"))
                .refresh_household_needs(pops);
        }
        for &id in &self.pops {
            pops.get_mut(&id)
                .unwrap_or_else(|| panic!("market pop {id} missing from pops"))
                .records
                .buy_stop = None;
        }
        let mut report = MarketDayReport::default();

        let history = self.history_with(&factuals.config.market);
        self.seed_amv_history();
        let (mut buys, mut sells) = self.collect_orders(&history, factuals, pops, firms);
        self.reset_day_exchange_stats();
        self.collate_order_books(&buys, &sells, &factuals.config.market);

        let mut steps = 0usize;
        loop {
            steps += 1;
            debug_assert!(steps < 1_000_000, "market day failed to terminate");

            sells.sort_by_key(|order| order.target);

            let batch = Self::match_orders_with_coincidence(
                &buys,
                &sells,
                rng,
                factuals.config.market_priority.sell_coincidence_weight,
            );
            let Some(pair) = batch.matched else {
                for order in buys.drain(..) {
                    self.unavailable_goods.insert(order.target);
                    report.unmatched_buys.push(order);
                }
                break;
            };

            let buy_order = buys[pair.buy_index].clone();
            let sell_order = sells[pair.sell_index].clone();
            buys.remove(pair.buy_index);
            sells.remove(pair.sell_index);

            self.settle_pair(
                buy_order,
                sell_order,
                &history,
                factuals,
                pops,
                firms,
                &mut buys,
                &mut sells,
                &mut report.meetings,
            );
        }

        self.note_buy_stops(pops, factuals, &history, wash_transport(factuals));

        for &id in &self.pops {
            pops.get_mut(&id)
                .unwrap_or_else(|| panic!("market pop {id} missing from pops"))
                .current_orders
                .clear();
        }

        report.leftover_buys = buys;
        report.leftover_sells = sells;
        self.stamp_leftover_buy(&report.leftover_buys, &report.unmatched_buys);
        self.nudge_amv_from_imbalance(&factuals.config.market);
        self.market_days = self.market_days.saturating_add(1);
        self.update_salability(&factuals.config.market);
        let period = factuals.config.market.amv_rescale_period;
        let mean = factuals.config.market.amv_rescale_mean;
        let min_abs = factuals.config.market.amv_min_abs;
        if period > 0 && self.market_days % period == 0 {
            let scale = self.rescale_amv_to_mean(mean, min_abs);
            for firm in firms.values_mut() {
                firm.scale_amv_unit(scale);
            }
        }
        self.record_amv_closes();
        report
    }

    /// # Rescale AMV To Mean
    ///
    /// Multiplies live AMV and average price so the unweighted mean of one
    /// unit of each tradeable good equals `target`. Time is skipped.
    ///
    /// Returns the scale applied, or `1.0` when this is a no-op. Does not
    /// rewrite the AMV trail: recorded closes already live in that day's
    /// mean units, so scaling them again compounds the unit change into
    /// diff/trend. Does not change salability. Callers scale firm AMV
    /// quotes by the same factor ([`Firm::scale_amv_unit`]).
    pub fn rescale_amv_to_mean(&mut self, target: f64, min_abs: f64) -> f64 {
        if !(target.is_finite() && target > 0.0) {
            return 1.0;
        }
        let n = self.goods.keys().filter(|id| **id != TIME).count() as f64;
        if n <= 0.0 {
            return 1.0;
        }
        let mean: f64 = self
            .goods
            .iter()
            .filter(|(id, _)| **id != TIME)
            .map(|(_, good)| good.amv)
            .sum::<f64>()
            / n;
        if !mean.is_finite() || mean.abs() < min_abs {
            return 1.0;
        }
        let scale = target / mean;
        if !scale.is_finite() {
            return 1.0;
        }
        for (&id, good) in self.goods.iter_mut() {
            if id == TIME {
                continue;
            }
            good.set_amv_min(good.amv * scale, min_abs);
            good.set_average_price_min(good.average_price * scale, min_abs);
        }
        scale
    }

    /// Pushes current AMV into an empty history ring (the opening AMV).
    fn seed_amv_history(&mut self) {
        for good in self.goods.values_mut() {
            if good.amv_history.is_empty() {
                good.record_amv();
            }
        }
    }

    /// Pushes each good's current AMV as today's close.
    fn record_amv_closes(&mut self) {
        for good in self.goods.values_mut() {
            good.record_amv();
        }
    }

    /// Morning labor settle for member firms. Does not post Time on
    /// `MarketOrder`s. Each pop-firm settle is one signed goods map on the
    /// workforce contract; the market records it as an accept when Time was
    /// given and goods were received.
    pub fn settle_labor(
        &mut self,
        pops: &mut HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
        factuals: &Factuals,
    ) -> Vec<(usize, LaborSettlement)> {
        let history = self.history_with(&factuals.config.market);
        let work_fraction = factuals.config.labor.work_time_fraction.clamp(0.0, 1.0);
        let mut firm_ids: Vec<usize> = self.firms.iter().copied().collect();
        firm_ids.sort_unstable();

        let mut demand = 0.0;
        let mut supply = 0.0;
        let mut buyers = 0.0;
        let mut suppliers: HashSet<usize> = HashSet::new();
        for &id in &firm_ids {
            let Some(firm) = firms.get(&id) else {
                continue;
            };
            let mut firm_claims = false;
            for worker in &firm.workforce {
                if worker.id == 0 {
                    continue;
                }
                let Some(pop) = pops.get(&worker.id) else {
                    continue;
                };
                demand += worker.hours.max(0.0);
                supply += work_fraction * pop.on_hand_time().max(0.0);
                suppliers.insert(worker.id);
                firm_claims = true;
            }
            if firm_claims {
                buyers += 1.0;
            }
        }

        let mut wages = Vec::new();
        for &id in &firm_ids {
            let Some(firm) = firms.get_mut(&id) else {
                continue;
            };
            let settlement = firm.settle_labor_contracts(pops, &history, factuals);
            wages.push((id, settlement));
        }

        let cfg = &factuals.config.market;
        for (id, settlement) in &wages {
            let Some(firm) = firms.get_mut(id) else {
                continue;
            };
            settlement.write_last_exchanges(firm);
            for worker in &firm.workforce {
                self.record_labor_deal(&worker.last_exchange, cfg);
            }
            if let Some(owner) = &settlement.owner {
                if !firm.workforce.iter().any(|row| row.id == owner.pop) {
                    self.record_labor_deal(&settlement.exchange_for(owner.pop), cfg);
                }
            }
        }
        self.stamp_time_from_labor(
            0.0,
            demand,
            supply,
            buyers,
            suppliers.len() as f64,
            None,
            cfg,
        );
        wages
    }

    /// Labor budget for member firms. Rewrites hours and wage baskets.
    /// Does not restamp Time AMV (labor deals already did).
    pub fn budget_labor(
        &mut self,
        pops: &HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
        factuals: &Factuals,
        day: u32,
    ) {
        let history = self.history_with(&factuals.config.market);
        let mut firm_ids: Vec<usize> = self.firms.iter().copied().collect();
        firm_ids.sort_unstable();
        for &id in &firm_ids {
            let Some(firm) = firms.get_mut(&id) else {
                continue;
            };
            firm.budget_labor(factuals, &history, pops, day);
        }

        let work_fraction = factuals.config.labor.work_time_fraction.clamp(0.0, 1.0);
        let mut demand = 0.0;
        let mut supply = 0.0;
        let mut buyers = 0.0;
        let mut suppliers: HashSet<usize> = HashSet::new();
        for &id in &firm_ids {
            let Some(firm) = firms.get(&id) else {
                continue;
            };
            let mut firm_claims = false;
            for worker in &firm.workforce {
                if worker.id == 0 {
                    continue;
                }
                let h = worker.hours.max(0.0);
                demand += h;
                firm_claims = true;
                suppliers.insert(worker.id);
                if let Some(pop) = pops.get(&worker.id) {
                    supply += work_fraction * pop.on_hand_time().max(0.0);
                }
            }
            if firm_claims {
                buyers += 1.0;
            }
        }
        self.stamp_time_from_labor(
            0.0,
            demand,
            supply,
            buyers,
            suppliers.len() as f64,
            None,
            &factuals.config.market,
        );
    }

    /// One labor settle as a market accept: Time given vs goods received.
    fn record_labor_deal(&mut self, goods: &HashMap<usize, f64>, cfg: &MarketConfig) {
        let filled = (-goods.get(&TIME).copied().unwrap_or(0.0)).max(0.0);
        if filled <= 0.0 {
            return;
        }
        let mut pay_amv = 0.0;
        for (&good, &qty) in goods {
            if good == TIME || qty <= 0.0 {
                continue;
            }
            pay_amv += qty * self.goods.get(&good).map(|row| row.amv.max(0.0)).unwrap_or(0.0);
        }
        if pay_amv <= 0.0 || !pay_amv.is_finite() {
            return;
        }
        self.record_fill(TIME, filled, pay_amv / filled, cfg);
        for (&good, &qty) in goods {
            if good == TIME || qty <= 0.0 {
                continue;
            }
            self.add_tender(good, qty, cfg);
            self.add_payment(good, qty, cfg);
        }
        self.drift_amv_on_accept(TIME, filled, goods, cfg);
    }

    /// Writes Time's labor book (demand = claimed hours, supply =
    /// work-fraction Time). Leaves AMV unchanged when `unit_amv` is 0.
    /// `purchased` is Some at settle (Time given); None at budget keeps
    /// this morning's fill.
    fn stamp_time_from_labor(
        &mut self,
        unit_amv: f64,
        demand: f64,
        supply: f64,
        buyers: f64,
        suppliers: f64,
        purchased: Option<f64>,
        cfg: &MarketConfig,
    ) {
        let row = self.market_good_mut(TIME, cfg);
        if unit_amv > 0.0 {
            row.set_amv_min(unit_amv, cfg.amv_min_abs);
            row.set_average_price_min(unit_amv, cfg.amv_min_abs);
        }
        row.set_demand(demand.max(0.0));
        row.set_supply(supply.max(0.0));
        row.set_buyers(buyers.max(0.0));
        row.set_suppliers(suppliers.max(0.0));
        if let Some(qty) = purchased {
            row.set_purchased(qty.max(0.0));
        }
    }

    /// Lerps each good's salability toward `payment / tender` when it was
    /// offered as payment today. Goods with no tender are left alone.
    fn update_salability(&mut self, cfg: &crate::game::config::MarketConfig) {
        let blend = cfg.salability_blend;
        for good in self.goods.values_mut() {
            if good.tender <= 0.0 {
                continue;
            }
            let accept = (good.payment / good.tender).clamp(0.0, 1.0);
            if !accept.is_finite() {
                continue;
            }
            let target = accept * market_constants::SALABILITY_MAX;
            good.set_salability(lerp(good.salability, target, blend));
        }
    }

    /// # Cap Salability From Decay
    ///
    /// Caps live salability at `1 - decayed / volume` per good.
    ///
    /// `rot` is `(decayed, volume)` from [`Pop::decay_goods`] /
    /// [`Firm::decay_goods`]. Volume is leftover on-hand after `used` is
    /// returned, plus `consumed`. Eaten stock counts as volume, not rot, so a
    /// good that is fully consumed and never leftover-decays is not capped.
    /// Zero or missing volume leaves salability unchanged. Does not raise
    /// salability. Unknown market goods are skipped.
    ///
    /// Call after decay, before record keeping so save ranking sees the cap.
    pub fn cap_salability_from_decay(&mut self, rot: &HashMap<usize, (f64, f64)>) {
        for (&id, &(decayed, volume)) in rot {
            if volume <= 0.0 {
                debug_assert!(decayed <= 0.0, "decayed must be <= 0.0 when volume <= 0.0");
                continue;
            }
            debug_assert!(decayed.is_finite() && volume.is_finite(), "decay rot must be finite");
            debug_assert!(decayed >= 0.0, "decayed must be >= 0.0");
            let rot_frac = (decayed / volume).clamp(0.0, 1.0);
            let cap = market_constants::SALABILITY_MAX * (1.0 - rot_frac);
            let Some(good) = self.goods.get_mut(&id) else {
                continue;
            };
            if good.salability > cap {
                good.set_salability(cap);
            }
        }
    }

    /// Pulls AMV of the sold good and its tenders toward the midpoint of the
    /// basket totals. Uses live [`MarketGood::amv`], not the frozen history.
    fn drift_amv_on_accept(
        &mut self,
        target: usize,
        filled: f64,
        goods: &HashMap<usize, f64>,
        cfg: &crate::game::config::MarketConfig,
    ) {
        if filled <= 0.0 {
            return;
        }
        let blend = cfg.amv_accept_blend;
        let target_amv = self.market_good_mut(target, cfg).amv;
        let given_total = filled * target_amv;
        if !given_total.is_finite() {
            return;
        }

        let mut pays: Vec<(usize, f64, f64)> = Vec::new();
        let mut pay_total = 0.0;
        for (&id, &qty) in goods {
            if id == target || qty <= 0.0 {
                continue;
            }
            let amv = self.market_good_mut(id, cfg).amv;
            pays.push((id, qty, amv));
            pay_total += qty * amv;
        }
        if pay_total <= 0.0 || !pay_total.is_finite() {
            return;
        }

        let s_sold = self.market_good_mut(target, cfg).salability.max(1e-9);
        let pay_qty: f64 = pays.iter().map(|(_, qty, _)| *qty).sum();
        let s_pay = if pay_qty > 0.0 {
            pays.iter()
                .map(|(id, qty, _)| {
                    *qty * self.market_good_mut(*id, cfg).salability.max(1e-9)
                })
                .sum::<f64>()
                / pay_qty
        } else {
            s_sold
        };

        let mid = 0.5 * (given_total + pay_total);
        let sold_blend = (blend * 2.0 * s_pay / (s_sold + s_pay)).min(1.0);
        let new_target = lerp(target_amv, mid / filled, sold_blend);
        self.market_good_mut(target, cfg).set_amv_min(new_target, cfg.amv_min_abs);

        let scale = mid / pay_total;
        for (id, _, amv) in pays {
            let s_i = self.market_good_mut(id, cfg).salability.max(1e-9);
            let pay_blend = (blend * 2.0 * s_sold / (s_i + s_sold)).min(1.0);
            let implied = amv * scale;
            self.market_good_mut(id, cfg)
                .set_amv_min(lerp(amv, implied, pay_blend), cfg.amv_min_abs);
        }
    }

    /// Lowers salability on goods that failed as payment. Does not move AMV.
    /// `blend` is the lerp toward 0 (pop reject uses `salability_blend`;
    /// firm reject uses that times `salability_firm_reject_scale`).
    fn drift_salability_on_reject(
        &mut self,
        target: usize,
        goods: &HashMap<usize, f64>,
        cfg: &crate::game::config::MarketConfig,
        blend: f64,
    ) {
        let blend = blend.clamp(0.0, 1.0);
        if blend <= 0.0 {
            return;
        }
        for (&id, &qty) in goods {
            if id == target || qty <= 0.0 {
                continue;
            }
            let old = self.market_good_mut(id, cfg).salability;
            self.market_good_mut(id, cfg)
                .set_salability(lerp(old, 0.0, blend));
        }
    }

    /// ±`amv_imbalance_kick` on each tradeable good toward heavier opening
    /// demand vs supply. Live kick is **flat ±1 AMV**. Tie (including both 0)
    /// does not move. Time skipped.
    ///
    /// Deferred if flat ±1 is too strong on cheap goods: (1) ±1% of |AMV|
    /// (`old * 0.01`) — scale-invariant, but the bottom sagged in a seed-1
    /// 60-day compare; (2) asymmetric +1 demand / −1% supply — rescue cheap
    /// goods people still want, don't hammer a glut by a flat dollar.
    fn nudge_amv_from_imbalance(&mut self, cfg: &MarketConfig) {
        let kick = cfg.amv_imbalance_kick;
        if kick <= 0.0 {
            return;
        }
        let mut ids: Vec<usize> = self.goods.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            if id == TIME {
                continue;
            }
            let (demand, supply, old) = {
                let Some(row) = self.goods.get(&id) else {
                    continue;
                };
                (row.demand, row.supply, row.amv)
            };
            if (demand - supply).abs() <= 0.0 {
                continue;
            }
            let delta = if demand > supply { kick } else { -kick };
            self.market_good_mut(id, cfg)
                .set_amv_min(old + delta, cfg.amv_min_abs);
        }
    }

    /// Emits pop and firm orders for this market and splits them into buy and
    /// sell books.
    fn collect_orders(
        &self,
        history: &MarketHistory,
        factuals: &Factuals,
        pops: &mut HashMap<usize, Pop>,
        firms: &HashMap<usize, Firm>,
    ) -> (Vec<MarketOrder>, Vec<MarketOrder>) {
        let mut buys = Vec::new();
        let mut sells = Vec::new();

        for &id in &self.pops {
            let pop = pops.get_mut(&id).expect("market pop missing from pops");
            split_into_books(
                pop.create_orders(history, factuals, &self.unavailable_goods),
                &mut buys,
                &mut sells,
            );
        }

        for &id in &self.firms {
            let firm = firms.get(&id).expect("market firm missing from firms");
            split_into_books(
                firm.create_orders(history, factuals, &self.unavailable_goods),
                &mut buys,
                &mut sells,
            );
        }

        (buys, sells)
    }

    /// Zeros today's exchange counters on every recorded good. Leaves AMV,
    /// salability, average price, stock, production, consumption, and imports.
    fn reset_day_exchange_stats(&mut self) {
        for (&id, good) in self.goods.iter_mut() {
            if id == TIME {
                continue;
            }
            good.set_supply(0.0);
            good.set_suppliers(0.0);
            good.set_demand(0.0);
            good.set_buyers(0.0);
            good.set_requests(0.0);
            good.set_purchased(0.0);
            good.set_tender(0.0);
            good.set_payment(0.0);
        }
    }

    /// Writes opening supply, demand, unique buyers, and unique suppliers
    /// from the current books onto [`MarketGood`] rows.
    fn collate_order_books(
        &mut self,
        buys: &[MarketOrder],
        sells: &[MarketOrder],
        cfg: &MarketConfig,
    ) {
        let mut demand: HashMap<usize, f64> = HashMap::new();
        let mut supply: HashMap<usize, f64> = HashMap::new();
        let mut buyers: HashMap<usize, HashSet<Actor>> = HashMap::new();
        let mut suppliers: HashMap<usize, HashSet<Actor>> = HashMap::new();

        for order in buys {
            *demand.entry(order.target).or_insert(0.0) += order.target_amount;
            buyers.entry(order.target).or_default().insert(order.origin);
        }
        for order in sells {
            *supply.entry(order.target).or_insert(0.0) += -order.target_amount;
            suppliers
                .entry(order.target)
                .or_default()
                .insert(order.origin);
        }

        for (good, qty) in demand {
            let n = buyers.get(&good).map(|set| set.len() as f64).unwrap_or(0.0);
            let row = self.market_good_mut(good, cfg);
            row.set_demand(qty);
            row.set_buyers(n);
        }
        for (good, qty) in supply {
            let n = suppliers
                .get(&good)
                .map(|set| set.len() as f64)
                .unwrap_or(0.0);
            let row = self.market_good_mut(good, cfg);
            row.set_supply(qty);
            row.set_suppliers(n);
        }
    }

    /// # Settle Pair
    ///
    /// Runs one matched buy/sell through propose, seller judge, and apply.
    /// The matched orders were already taken off `buys` / `sells` by the caller.
    ///
    /// 1. Buyer [`DealMaker::buy`] names a basket. That proposal is the buyer's accept.
    /// 2. Seller [`DealMaker::evaluate`]s it.
    /// 3. Accept: record fill stats, drift AMV on [`MarketGood`] toward the
    ///    basket midpoint (more salable goods move less), [`DealMaker::finalize`]
    ///    both inventories, push leftover order amounts back onto `buys` / `sells`.
    /// 4. Reject: lower salability on tenders (not AMV). No-proposal: no AMV
    ///    and no salability change. Wash. On reject, cut the sell/offer weight by
    ///    `sell_reject_weight` (same day only). Charge
    ///    [`market_constants::TRANSACTION_COST`]
    ///    transport from on-hand. Push `sell_order` back onto `sells`.
    ///    Buyer [`DealMaker::renew_buy`] may put the buy back with `tries`
    ///    incremented; after [`market_constants::BUY_TRY_LIMIT`] retries the
    ///    order closes.
    /// 5. Counteroffer haggling is later (seller-approved rewrite, then buyer
    ///    evaluates). Unused verdicts wash like a close-out for now.
    ///
    /// `pops` / `firms` are the live actor maps; inventory moves here on accept.
    /// `buys` / `sells` are this day's leftover books.
    fn settle_pair(
        &mut self,
        buy_order: MarketOrder,
        sell_order: MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
        pops: &mut HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
        buys: &mut Vec<MarketOrder>,
        sells: &mut Vec<MarketOrder>,
        meetings: &mut Vec<MarketMeeting>,
    ) {
        let buy_snap = buy_order.clone();
        let sell_snap = sell_order.clone();
        let target = buy_order.target;
        let sought = buy_order.target_amount.min(-sell_order.target_amount);
        if sought > 0.0 {
            self.add_requests(target, sought, &factuals.config.market);
        }

        let Some(proposal) = as_deal_maker(pops, firms, buy_order.origin)
            .buy(&buy_order, &sell_order, history, factuals)
        else {
            let transport = wash_transport(factuals);
            let origin = sell_order.origin;
            let renewed = wash_pair(buy_order, sell_order, factuals, pops, firms, buys, sells);
            meetings.push(MarketMeeting {
                buy: buy_snap,
                sell: sell_snap,
                outcome: MeetingOutcome::Wash {
                    reason: WashReason::NoProposal,
                    transport,
                    closed: !renewed,
                },
            });
            record_firm_sell_meet(firms, origin, target, 0.0, 0.0, 1.0);
            return;
        };

        let verdict = as_deal_maker(pops, firms, sell_order.origin)
            .evaluate(&proposal, &sell_order, &buy_order, history, factuals);
        if verdict != DealResponse::Accept {
            // TODO: Counteroffer haggling. The rewrite is seller-approved; the
            // buyer would then evaluate it (or a close-out). Wash for now.
            let cfg = &factuals.config.market;
            let blend = if matches!(sell_order.origin, Actor::Pop(_)) {
                cfg.salability_blend
            } else {
                cfg.salability_blend * cfg.salability_firm_reject_scale
            };
            self.drift_salability_on_reject(target, &proposal.goods, cfg, blend);
            let transport = wash_transport(factuals);
            let mut sell_order = sell_order;
            sell_order.apply_reject_weight_penalty(
                factuals.config.market_priority.sell_reject_weight,
                factuals.config.market_priority.sell_actor_priority_floor,
            );
            let origin = sell_order.origin;
            let renewed = wash_pair(buy_order, sell_order, factuals, pops, firms, buys, sells);
            meetings.push(MarketMeeting {
                buy: buy_snap,
                sell: sell_snap,
                outcome: MeetingOutcome::Wash {
                    reason: WashReason::Rejected,
                    transport,
                    closed: !renewed,
                },
            });
            record_firm_sell_meet(firms, origin, target, 0.0, 1.0, 0.0);
            return;
        }

        let proposal = as_deal_maker(pops, firms, sell_order.origin).sell(
            &proposal,
            &sell_order,
            &buy_order,
            history,
            factuals,
        );

        let filled = proposal.goods.get(&target).copied().unwrap_or(0.0).abs();
        if filled <= 0.0 {
            debug_assert!(false, "accepted deal must move the target good");
            let transport = wash_transport(factuals);
            let renewed = wash_pair(buy_order, sell_order, factuals, pops, firms, buys, sells);
            meetings.push(MarketMeeting {
                buy: buy_snap,
                sell: sell_snap,
                outcome: MeetingOutcome::Wash {
                    reason: WashReason::EmptyFill,
                    transport,
                    closed: !renewed,
                },
            });
            return;
        }

        for (&good, &qty) in &proposal.goods {
            if qty > 0.0 {
                self.add_tender(good, qty, &factuals.config.market);
            }
        }

        let payment_amv: f64 = proposal.goods.iter()
            .filter_map(|(&good, &qty)| (qty > 0.0).then_some(qty * history.price(good)))
            .sum();
        self.record_fill(
            target,
            filled,
            payment_amv / filled,
            &factuals.config.market,
        );
        for (&good, &qty) in &proposal.goods {
            if qty > 0.0 {
                self.add_payment(good, qty, &factuals.config.market);
            }
        }
        self.drift_amv_on_accept(target, filled, &proposal.goods, &factuals.config.market);

        as_deal_maker_mut(pops, firms, buy_order.origin).finalize(&proposal, history);
        as_deal_maker_mut(pops, firms, sell_order.origin).finalize(&proposal, history);
        record_firm_sell_meet(firms, sell_order.origin, target, 1.0, 0.0, 0.0);
        as_deal_maker_mut(pops, firms, buy_order.origin)
            .pay_transport(proposal.transport_needed, factuals);

        if let Some(leftover) = leftover_order(buy_order, filled) {
            buys.push(leftover);
        }
        if let Some(mut leftover) = leftover_order(sell_order, filled) {
            leftover.add_successful_sell_bonus_amount(
                factuals.config.market_priority.successful_sell_bonus,
            );
            sells.push(leftover);
        }
        clamp_sells_to_on_hand(sells, buy_snap.origin, pops, firms);
        clamp_sells_to_on_hand(sells, sell_snap.origin, pops, firms);

        meetings.push(MarketMeeting {
            buy: buy_snap,
            sell: sell_snap,
            outcome: MeetingOutcome::Traded {
                goods: proposal.goods.clone(),
                transport_needed: proposal.transport_needed,
            },
        });
    }

    /// Returns the row for `good`, inserting a config-default good if it is new.
    fn market_good_mut(&mut self, good: usize, cfg: &MarketConfig) -> &mut MarketGood {
        self.goods
            .entry(good)
            .or_insert_with(|| MarketGood::from_config(cfg))
    }

    /// Adds `qty` to this good's deal-request total.
    fn add_requests(&mut self, good: usize, qty: f64, cfg: &MarketConfig) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good, cfg);
        row.set_requests(row.requests + qty);
    }

    /// Adds `qty` to this good's offered-as-payment total.
    fn add_tender(&mut self, good: usize, qty: f64, cfg: &MarketConfig) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good, cfg);
        row.set_tender(row.tender + qty);
    }

    /// Adds `qty` to this good's accepted-as-payment total.
    fn add_payment(&mut self, good: usize, qty: f64, cfg: &MarketConfig) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good, cfg);
        row.set_payment(row.payment + qty);
    }

    /// Records a successful purchase of `qty` at `unit_price` on the target
    /// good (purchased and rolling average price). Volume is derived.
    fn record_fill(&mut self, good: usize, qty: f64, unit_price: f64, cfg: &MarketConfig) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        debug_assert!(unit_price.is_finite(), "unit_price must be finite");
        let row = self.market_good_mut(good, cfg);
        let prev_qty = row.purchased;
        let prev_avg = row.average_price;
        let new_qty = prev_qty + qty;
        row.set_purchased(new_qty);
        if new_qty > 0.0 {
            row.set_average_price_min(
                (prev_avg * prev_qty + unit_price * qty) / new_qty,
                cfg.amv_min_abs,
            );
        }
    }

    /// # Match Orders
    ///
    /// One pass: pick a buy at random among those with an other-origin sell,
    /// then pick that sell by listed amount (coincidence multiplies). Same
    /// origin never pairs. Does not mutate the lists. `sells` must be sorted
    /// by target good id. At most one match. When none, `unmatched_buys` is
    /// every remaining buy.
    pub fn match_orders<R: Rng + ?Sized>(
        buys: &[MarketOrder],
        sells: &[MarketOrder],
        rng: &mut R,
    ) -> OrderMatchBatch {
        Self::match_orders_with_coincidence(
            buys,
            sells,
            rng,
            market_priority::SELL_COINCIDENCE_WEIGHT,
        )
    }

    /// One matching pass using a loaded coincidence-weight multiplier.
    pub fn match_orders_with_coincidence<R: Rng + ?Sized>(
        buys: &[MarketOrder],
        sells: &[MarketOrder],
        rng: &mut R,
        coincidence_weight: f64,
    ) -> OrderMatchBatch {
        if buys.is_empty() {
            return OrderMatchBatch::empty();
        }
        debug_assert!(
            sells.windows(2).all(|w| w[0].target <= w[1].target),
            "sells must be sorted by target id"
        );

        let mut matchable = Vec::new();
        let mut unmatched_buys = Vec::new();
        for (buy_index, buy) in buys.iter().enumerate() {
            debug_assert!(
                buy.target_amount > 0.0,
                "buy target_amount must be > 0.0"
            );
            let (_, _, had_other) = classify_sells(sells, buy, coincidence_weight);
            if had_other {
                matchable.push(buy_index);
            } else {
                unmatched_buys.push(buy_index);
            }
        }
        if matchable.is_empty() {
            unmatched_buys.sort_unstable();
            return OrderMatchBatch {
                matched: None,
                unmatched_buys,
            };
        }

        let buy_index = matchable[rng.random_range(0..matchable.len())];
        let buy = &buys[buy_index];
        let (available, weights, _) = classify_sells(sells, buy, coincidence_weight);
        let sell_index = pick_available_sell(&available, &weights, rng)
            .expect("matchable buy has an other-origin sell");
        OrderMatchBatch {
            matched: Some(OrderMatch {
                buy_index,
                sell_index,
            }),
            unmatched_buys: Vec::new(),
        }
    }

    /// # History
    ///
    /// Snapshot of current AMVs and salability for pop record keeping and
    /// sentiment wealth. Readers default missing prices to 1.0 and missing
    /// salability to [`market_constants::SALABILITY_DEFAULT`].
    pub fn history(&self) -> MarketHistory {
        self.history_with_salability(market_constants::SALABILITY_DEFAULT)
    }

    /// Snapshot using a loaded missing-salability default.
    pub fn history_with(&self, cfg: &MarketConfig) -> MarketHistory {
        self.history_with_salability(cfg.salability_default)
    }

    fn history_with_salability(&self, default_salability: f64) -> MarketHistory {
        let mut history = MarketHistory::new();
        history.default_salability = default_salability;
        for (&good_id, good) in &self.goods {
            history.prices.insert(good_id, good.amv);
            history.salability.insert(good_id, good.salability);
            history.purchased.insert(good_id, good.purchased);
            history.amv_trails.insert(good_id, good.amv_trail());
        }
        history.leftover_buy = self.leftover_buy.clone();
        history.friction = self.friction;
        history
    }

    fn stamp_leftover_buy(&mut self, leftover: &[MarketOrder], unmatched: &[MarketOrder]) {
        self.leftover_buy.clear();
        for order in leftover.iter().chain(unmatched) {
            if order.target_amount > 0.0 {
                *self.leftover_buy.entry(order.target).or_insert(0.0) += order.target_amount;
            }
        }
    }
}

/// # Market History
/// 
/// A saved record of minimal data for passing around.
#[derive(Debug, Clone)]
pub struct MarketHistory {
    /// Last known AMV price per good.
    pub prices: HashMap<usize, f64>,
    /// Last known salability per good, typically in 0.0..=1.0.
    pub salability: HashMap<usize, f64>,
    /// Market friction factor copied from [`Market::friction`].
    pub friction: f64,
    /// Used when a good has no recorded salability. Default 0.4.
    pub default_salability: f64,
    /// Units purchased today as the sought good. Missing = unknown share.
    pub purchased: HashMap<usize, f64>,
    /// Oldest-to-newest AMV closes. Empty = unknown trend and volatility.
    pub amv_trails: HashMap<usize, Vec<f64>>,
    /// Unfilled buy/request units by good at market close. Missing = 0.
    pub leftover_buy: HashMap<usize, f64>,
}

impl Default for MarketHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-market AMV snapshots plus pop-to-market membership.
/// Histories are day-static; rebuild membership after pops move.
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
        Self::from_markets_with(markets, market_constants::SALABILITY_DEFAULT)
    }

    /// Same as [`Self::from_markets`], using a loaded missing-salability default.
    pub fn from_markets_with(
        markets: &HashMap<usize, Market>,
        default_salability: f64,
    ) -> Self {
        let mut histories = HashMap::new();
        let mut pop_to_market = HashMap::new();
        for market in markets.values() {
            let mut history = market.history();
            history.default_salability = default_salability;
            histories.insert(market.id, history);
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

impl MarketHistory {
    pub(crate) fn new() -> Self {
        Self { 
            prices: HashMap::new(),
            salability: HashMap::new(),
            friction: 0.0,
            default_salability: market_constants::SALABILITY_DEFAULT,
            purchased: HashMap::new(),
            amv_trails: HashMap::new(),
            leftover_buy: HashMap::new(),
        }
    }

    /// Unfilled buy/request units for `good_id` at market close, or 0.
    pub fn leftover_buy(&self, good_id: usize) -> f64 {
        self.leftover_buy.get(&good_id).copied().unwrap_or(0.0)
    }

    /// Price for `good_id`, or 1.0 if missing.
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

    /// Highest recorded salability among tradeable goods in this snapshot.
    /// Time is skipped. Returns 0.0 when the snapshot has no other goods.
    pub fn max_salability(&self) -> f64 {
        let mut ids: HashSet<usize> = self.prices.keys().copied().collect();
        ids.extend(self.salability.keys().copied());
        ids.remove(&TIME);
        ids.iter()
            .map(|&id| self.salability(id))
            .fold(0.0, f64::max)
    }
}


/// # Market Good
/// 
/// Publically available data for a maret good.
/// 
/// Records overall production, consumption, buying/selling (both times traded and 
/// quantity traded), and the current estimated Abstract Market Value (AMV).
/// 
/// Note: Consumption means both consumed by a pop for their needs, and consumed by a 
/// firm for production purposes. It does not currently distinguish between the two.
#[derive(Debug, Clone)]
pub struct MarketGood {
    // Valuation data. Key features of exchange data.
    /// The current Abstract Market Value, an estimation of it's market value.
    pub amv: f64,
    /// The current salability of the good. Must be bound between [0.0 and 1.0].
    /// 
    /// Low Salability means it's hard to sell and generally illiquid.
    /// High Salability means its easy to sell, generally liquid.
    /// 
    /// Salability factors:
    /// - Salability is pushed up or down relative to the history of being accepted or 
    /// rejected.
    /// - The total Volume Moved and Velocity of trades of the good. 
    /// - External effects (Culture, Institutions, State action, etc)
    /// - Safety of Value (how often does it lose value and how much does it lose).
    /// 
    /// Possible Additional Factors:
    /// - Price Impact, 
    pub salability: f64,

    // placeholder for AMV Historical records.
    /// Closing AMVs for the last [`market_constants::AMV_HISTORY_MAX`] market
    /// days, oldest first. The first sample is the opening AMV on the day the
    /// ring was seeded; later samples are end-of-day closes.
    pub amv_history: CircularBuffer<{ market_constants::AMV_HISTORY_MAX }, f64>,

    // Physical data. End-of-Day-Stock = Stock + imported + production - Consumption.
    /// How many were made today.
    pub production: f64,
    /// How many were consumed today.
    pub consumption: f64,
    /// How many were brought in or out by traders. (Negative vaules means exports)
    pub imported: f64,
    /// How many of this good already existed in the market from yesterday.
    pub stock: f64,

    // Market Data. What was actually shown to the market fully.
    /// How many units of the good were offered in sale. The Sum of all Sell and Offer 
    /// orders.
    pub supply: f64,
    /// How many unique sellers there were.
    pub suppliers: f64,
    /// How many units of the good were requested in sale. The Sum of all Buy and 
    /// Request orders.
    pub demand: f64,
    /// How many unique buyers their were.
    pub buyers: f64,

    // Deal Records. When Buyer and Seller are matched, what happened.
    /// How many units of the good were sought out in all deals.
    pub requests: f64,
    /// How many requested goods were successfully purchased.
    pub purchased: f64,
    /// How many units of the good were offered as payment in all deals.
    pub tender: f64,
    /// How many units of the good were actually accepted as payment in all deals.
    pub payment: f64,
    /// The average price the good traded for.
    /// Average Price = (average_price * purchased + deal's price * deals purchase amount) 
    ///     / (purchased + deals purchase amount).
    /// Alternatively may be updated at days end instead.
    pub average_price: f64,
}

impl Default for MarketGood {
    /// AMV defaults to 1.0, average price to 1.0, and salability to
    /// [`market_constants::SALABILITY_DEFAULT`]. All others default to 0.0.
    fn default() -> Self {
        Self {
            amv: 1.0,
            salability: market_constants::SALABILITY_DEFAULT,
            amv_history: CircularBuffer::new(),
            production: 0.0,
            consumption: 0.0,
            imported: 0.0,
            stock: 0.0,
            supply: 0.0,
            suppliers: 0.0,
            demand: 0.0,
            buyers: 0.0,
            requests: 0.0,
            purchased: 0.0,
            tender: 0.0,
            payment: 0.0,
            average_price: 1.0,
        }
    }
}

impl MarketGood {
    /// # New
    ///
    /// Same defaults as [`Default`]: AMV 1.0, salability
    /// [`market_constants::SALABILITY_DEFAULT`], average price 1.0, all others 0.0.
    pub fn new() -> Self {
        Self::default()
    }

    /// New row using loaded salability default. AMV still starts at 1.0.
    pub fn from_config(cfg: &MarketConfig) -> Self {
        let mut good = Self::new();
        good.salability = cfg.salability_default.clamp(0.0, market_constants::SALABILITY_MAX);
        good
    }

    /// Sets the current Abstract Market Value.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn set_amv(&mut self, amv: f64) {
        self.set_amv_min(amv, market_constants::AMV_MIN_ABS);
    }

    /// Sets AMV using a loaded bounce floor.
    pub fn set_amv_min(&mut self, amv: f64, min_abs: f64) {
        self.amv = bounce_away_from_zero(self.amv, amv, min_abs);
    }

    /// Sets the current Abstract Market Value.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn with_amv(mut self, amv: f64) -> Self {
        self.set_amv(amv);
        self
    }

    /// Pushes the current AMV onto `amv_history`.
    pub fn record_amv(&mut self) {
        self.amv_history.push_back(self.amv);
    }

    /// Oldest-to-newest AMV samples currently in the ring.
    pub fn amv_trail(&self) -> Vec<f64> {
        self.amv_history.iter().copied().collect()
    }

    /// Sets salability, clamped to `0.0..=`[`market_constants::SALABILITY_MAX`].
    pub fn set_salability(&mut self, salability: f64) {
        debug_assert!(salability.is_finite(), "salability must be finite");
        self.salability = salability.clamp(0.0, market_constants::SALABILITY_MAX);
    }

    /// Sets salability, clamped to `0.0..=`[`market_constants::SALABILITY_MAX`].
    pub fn with_salability(mut self, salability: f64) -> Self {
        self.set_salability(salability);
        self
    }

    /// Sets how many units were produced today.
    /// Must be `>= 0.0`.
    pub fn set_production(&mut self, production: f64) {
        debug_assert!(production >= 0.0, "production must be >= 0.0");
        self.production = production;
    }

    /// Sets how many units were produced today.
    /// Must be `>= 0.0`.
    pub fn with_production(mut self, production: f64) -> Self {
        self.set_production(production);
        self
    }

    /// Sets how many units were consumed today.
    /// Must be `>= 0.0`.
    pub fn set_consumption(&mut self, consumption: f64) {
        debug_assert!(consumption >= 0.0, "consumption must be >= 0.0");
        self.consumption = consumption;
    }

    /// Sets how many units were consumed today.
    /// Must be `>= 0.0`.
    pub fn with_consumption(mut self, consumption: f64) -> Self {
        self.set_consumption(consumption);
        self
    }

    /// Sets net imports today. Negative values are exports.
    pub fn set_imported(&mut self, imported: f64) {
        self.imported = imported;
    }

    /// Sets net imports today. Negative values are exports.
    pub fn with_imported(mut self, imported: f64) -> Self {
        self.set_imported(imported);
        self
    }

    /// Sets yesterday's leftover stock.
    /// Must be `>= 0.0`.
    pub fn set_stock(&mut self, stock: f64) {
        debug_assert!(stock >= 0.0, "stock must be >= 0.0");
        self.stock = stock;
    }

    /// Sets yesterday's leftover stock.
    /// Must be `>= 0.0`.
    pub fn with_stock(mut self, stock: f64) -> Self {
        self.set_stock(stock);
        self
    }

    /// Sets units offered in sale (sum of sell and offer orders).
    /// Must be `>= 0.0`.
    pub fn set_supply(&mut self, supply: f64) {
        debug_assert!(supply >= 0.0, "supply must be >= 0.0");
        self.supply = supply;
    }

    /// Sets units offered in sale (sum of sell and offer orders).
    /// Must be `>= 0.0`.
    pub fn with_supply(mut self, supply: f64) -> Self {
        self.set_supply(supply);
        self
    }

    /// Sets how many unique sellers there were.
    /// Must be `>= 0.0`.
    pub fn set_suppliers(&mut self, suppliers: f64) {
        debug_assert!(suppliers >= 0.0, "suppliers must be >= 0.0");
        self.suppliers = suppliers;
    }

    /// Sets how many unique sellers there were.
    /// Must be `>= 0.0`.
    pub fn with_suppliers(mut self, suppliers: f64) -> Self {
        self.set_suppliers(suppliers);
        self
    }

    /// Sets units requested (sum of buy and request orders).
    /// Must be `>= 0.0`.
    pub fn set_demand(&mut self, demand: f64) {
        debug_assert!(demand >= 0.0, "demand must be >= 0.0");
        self.demand = demand;
    }

    /// Sets units requested (sum of buy and request orders).
    /// Must be `>= 0.0`.
    pub fn with_demand(mut self, demand: f64) -> Self {
        self.set_demand(demand);
        self
    }

    /// Sets how many unique buyers there were.
    /// Must be `>= 0.0`.
    pub fn set_buyers(&mut self, buyers: f64) {
        debug_assert!(buyers >= 0.0, "buyers must be >= 0.0");
        self.buyers = buyers;
    }

    /// Sets how many unique buyers there were.
    /// Must be `>= 0.0`.
    pub fn with_buyers(mut self, buyers: f64) -> Self {
        self.set_buyers(buyers);
        self
    }

    /// Local units that changed hands today: `purchased + payment`.
    /// Does not include imports and exports. Total volume with trade is
    /// `volume() + imported.abs()`.
    pub fn volume(&self) -> f64 {
        self.purchased + self.payment
    }

    /// Sets units sought out across all deals.
    /// Must be `>= 0.0`.
    pub fn set_requests(&mut self, requests: f64) {
        debug_assert!(requests >= 0.0, "requests must be >= 0.0");
        self.requests = requests;
    }

    /// Sets units sought out across all deals.
    /// Must be `>= 0.0`.
    pub fn with_requests(mut self, requests: f64) -> Self {
        self.set_requests(requests);
        self
    }

    /// Sets units successfully purchased across all deals.
    /// Must be `>= 0.0`.
    pub fn set_purchased(&mut self, purchased: f64) {
        debug_assert!(purchased >= 0.0, "purchased must be >= 0.0");
        self.purchased = purchased;
    }

    /// Sets units successfully purchased across all deals.
    /// Must be `>= 0.0`.
    pub fn with_purchased(mut self, purchased: f64) -> Self {
        self.set_purchased(purchased);
        self
    }

    /// Sets units offered as payment across all deals.
    /// Must be `>= 0.0`.
    pub fn set_tender(&mut self, tender: f64) {
        debug_assert!(tender >= 0.0, "tender must be >= 0.0");
        self.tender = tender;
    }

    /// Sets units offered as payment across all deals.
    /// Must be `>= 0.0`.
    pub fn with_tender(mut self, tender: f64) -> Self {
        self.set_tender(tender);
        self
    }

    /// Sets units accepted as payment across all deals.
    /// Must be `>= 0.0`.
    pub fn set_payment(&mut self, payment: f64) {
        debug_assert!(payment >= 0.0, "payment must be >= 0.0");
        self.payment = payment;
    }

    /// Sets units accepted as payment across all deals.
    /// Must be `>= 0.0`.
    pub fn with_payment(mut self, payment: f64) -> Self {
        self.set_payment(payment);
        self
    }

    /// Sets the average price the good traded for.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn set_average_price(&mut self, average_price: f64) {
        self.set_average_price_min(average_price, market_constants::AMV_MIN_ABS);
    }

    /// Sets average price using a loaded bounce floor.
    pub fn set_average_price_min(&mut self, average_price: f64, min_abs: f64) {
        self.average_price = bounce_away_from_zero(self.average_price, average_price, min_abs);
    }

    /// Sets the average price the good traded for.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn with_average_price(mut self, average_price: f64) -> Self {
        self.set_average_price(average_price);
        self
    }
}

#[cfg(test)]
mod market_lookups_should {
    use super::*;

    #[test]
    fn snapshots_one_history_per_market_and_maps_pops() {
        let mut market = Market {
            id: 7,
            pops: HashSet::from([10, 11]),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            friction: 0.0,
            unavailable_goods: HashSet::new(),
            market_days: 0,
            leftover_buy: HashMap::new(),
        };
        market.goods.insert(5, MarketGood::new().with_amv(3.0));
        let mut markets = HashMap::new();
        markets.insert(7, market);

        let lookups = MarketLookups::from_markets(&markets);
        let empty = MarketHistory::new();

        assert_eq!(lookups.histories.len(), 1);
        assert_eq!(lookups.history_for_pop(10, &empty).price(5), 3.0);
        assert_eq!(lookups.history_for_pop(11, &empty).price(5), 3.0);
        assert_eq!(lookups.history_for_pop(99, &empty).price(5), 1.0);
    }
}

#[cfg(test)]
mod market_good_should {
    use super::*;
    use crate::game::config::market_constants;

    #[test]
    fn default_to_unit_amv_default_salability_and_zero_flow() {
        let good = MarketGood::new();
        assert_eq!(good.amv, 1.0);
        assert_eq!(good.salability, market_constants::SALABILITY_DEFAULT);
        assert_eq!(good.average_price, 1.0);
        assert_eq!(good.production, 0.0);
        assert_eq!(good.consumption, 0.0);
        assert_eq!(good.imported, 0.0);
        assert_eq!(good.stock, 0.0);
        assert_eq!(good.supply, 0.0);
        assert_eq!(good.suppliers, 0.0);
        assert_eq!(good.demand, 0.0);
        assert_eq!(good.buyers, 0.0);
        assert_eq!(good.volume(), 0.0);
        assert_eq!(good.requests, 0.0);
        assert_eq!(good.purchased, 0.0);
        assert_eq!(good.tender, 0.0);
        assert_eq!(good.payment, 0.0);
        assert!(good.amv_history.is_empty());
    }

    #[test]
    fn fluent_setters_override_defaults() {
        let good = MarketGood::new()
            .with_amv(2.5)
            .with_salability(0.8)
            .with_production(4.0)
            .with_consumption(1.0)
            .with_imported(-0.5)
            .with_stock(10.0)
            .with_supply(3.0)
            .with_suppliers(2.0)
            .with_demand(5.0)
            .with_buyers(3.0)
            .with_requests(5.0)
            .with_purchased(2.0)
            .with_tender(6.0)
            .with_payment(4.0)
            .with_average_price(1.5);

        assert_eq!(good.amv, 2.5);
        assert_eq!(good.salability, 0.8);
        assert_eq!(good.production, 4.0);
        assert_eq!(good.consumption, 1.0);
        assert_eq!(good.imported, -0.5);
        assert_eq!(good.stock, 10.0);
        assert_eq!(good.supply, 3.0);
        assert_eq!(good.suppliers, 2.0);
        assert_eq!(good.demand, 5.0);
        assert_eq!(good.buyers, 3.0);
        assert_eq!(good.volume(), 6.0);
        assert_eq!(good.requests, 5.0);
        assert_eq!(good.purchased, 2.0);
        assert_eq!(good.tender, 6.0);
        assert_eq!(good.payment, 4.0);
        assert_eq!(good.average_price, 1.5);
        assert_eq!(good.volume(), good.purchased + good.payment);
    }

    #[test]
    fn bounces_positive_amv_past_zero_to_negative() {
        let min = market_constants::AMV_MIN_ABS;
        let good = MarketGood::new().with_amv(0.0);
        assert_eq!(good.amv, -min);

        let good = MarketGood::new().with_amv(min / 10.0);
        assert_eq!(good.amv, -min);
    }

    #[test]
    fn bounces_negative_amv_past_zero_to_positive() {
        let min = market_constants::AMV_MIN_ABS;
        let good = MarketGood::new().with_amv(-1.0).with_amv(0.0);
        assert_eq!(good.amv, min);

        let good = MarketGood::new().with_amv(-1.0).with_amv(-min / 10.0);
        assert_eq!(good.amv, min);
    }

    #[test]
    fn keeps_amv_outside_the_dead_zone() {
        let min = market_constants::AMV_MIN_ABS;
        assert_eq!(MarketGood::new().with_amv(min).amv, min);
        assert_eq!(MarketGood::new().with_amv(-min).amv, -min);
        assert_eq!(MarketGood::new().with_amv(-2.5).amv, -2.5);
    }

    #[test]
    fn average_price_uses_the_same_zero_bounce() {
        let min = market_constants::AMV_MIN_ABS;
        assert_eq!(
            MarketGood::new().with_average_price(0.0).average_price,
            -min
        );
        assert_eq!(
            MarketGood::new()
                .with_average_price(-1.0)
                .with_average_price(0.0)
                .average_price,
            min
        );
    }

    #[test]
    fn clamps_salability_to_max() {
        assert_eq!(MarketGood::new().with_salability(1.5).salability, 1.5);
        assert_eq!(
            MarketGood::new().with_salability(2.5).salability,
            market_constants::SALABILITY_MAX
        );
        assert_eq!(MarketGood::new().with_salability(-0.2).salability, 0.0);
        assert_eq!(MarketGood::new().with_salability(0.4).salability, 0.4);
        assert_eq!(MarketGood::new().with_salability(0.0).salability, 0.0);
        assert_eq!(MarketGood::new().with_salability(1.0).salability, 1.0);
    }

    #[test]
    fn mutating_setters_share_the_same_invariants() {
        let min = market_constants::AMV_MIN_ABS;
        let mut good = MarketGood::new();
        good.set_amv(0.0);
        assert_eq!(good.amv, -min);
        good.set_salability(2.0);
        assert_eq!(good.salability, 2.0);
        good.set_production(3.0);
        assert_eq!(good.production, 3.0);
    }

    #[test]
    fn record_amv_pushes_current_value() {
        let mut good = MarketGood::new().with_amv(2.5);
        assert!(good.amv_history.is_empty());
        good.record_amv();
        good.set_amv(2.75);
        good.record_amv();
        assert_eq!(good.amv_trail(), vec![2.5, 2.75]);
    }
}

#[cfg(test)]
mod cap_salability_from_decay_should {
    use super::*;

    const GRAIN: usize = 1;
    const COIN: usize = 2;

    fn market_with(grain: f64, coin: f64) -> Market {
        let mut market = Market::new(1);
        market.goods.insert(GRAIN, MarketGood::new().with_salability(grain));
        market.goods.insert(COIN, MarketGood::new().with_salability(coin));
        market
    }

    #[test]
    fn full_rot_caps_at_zero() {
        let mut market = market_with(1.0, 1.0);
        let mut rot = HashMap::new();
        rot.insert(GRAIN, (10.0, 10.0));
        market.cap_salability_from_decay(&rot);
        assert_eq!(market.goods[&GRAIN].salability, 0.0);
        assert_eq!(market.goods[&COIN].salability, 1.0);
    }

    #[test]
    fn consumed_only_volume_leaves_salability() {
        let mut market = market_with(0.8, 1.0);
        let mut rot = HashMap::new();
        rot.insert(GRAIN, (0.0, 10.0));
        market.cap_salability_from_decay(&rot);
        assert!((market.goods[&GRAIN].salability - 0.8).abs() < 1e-12);
    }

    #[test]
    fn ten_percent_rot_caps_at_max_times_kept() {
        let mut market = market_with(2.0, 1.0);
        let mut rot = HashMap::new();
        rot.insert(GRAIN, (1.0, 10.0));
        market.cap_salability_from_decay(&rot);
        assert!((market.goods[&GRAIN].salability - 1.8).abs() < 1e-12);
    }

    #[test]
    fn does_not_raise_salability_already_below_cap() {
        let mut market = market_with(0.2, 1.0);
        let mut rot = HashMap::new();
        rot.insert(GRAIN, (1.0, 10.0));
        market.cap_salability_from_decay(&rot);
        assert!((market.goods[&GRAIN].salability - 0.2).abs() < 1e-12);
    }

    #[test]
    fn skips_unknown_goods_and_zero_volume() {
        let mut market = market_with(0.7, 1.0);
        let mut rot = HashMap::new();
        rot.insert(99, (5.0, 5.0));
        rot.insert(COIN, (0.0, 0.0));
        market.cap_salability_from_decay(&rot);
        assert!((market.goods[&GRAIN].salability - 0.7).abs() < 1e-12);
        assert_eq!(market.goods[&COIN].salability, 1.0);
    }
}

#[cfg(test)]
mod rescale_amv_to_mean_should {
    use super::*;
    use crate::game::config::market_constants;
    use crate::game::good::TIME;

    const GRAIN: usize = 1;
    const COIN: usize = 2;
    const BREAD: usize = 3;

    #[test]
    fn scales_unweighted_mean_of_one_unit_each_to_target() {
        let mut market = Market::new(1);
        market.goods.insert(GRAIN, MarketGood::new().with_amv(1.0).with_average_price(1.0));
        market.goods.insert(COIN, MarketGood::new().with_amv(2.0).with_average_price(2.0));
        market.goods.insert(BREAD, MarketGood::new().with_amv(3.0).with_average_price(3.0));
        market.rescale_amv_to_mean(10.0, market_constants::AMV_MIN_ABS);
        // Mean was 2; scale 5.
        assert!((market.goods[&GRAIN].amv - 5.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv - 10.0).abs() < 1e-12);
        assert!((market.goods[&BREAD].amv - 15.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].average_price - 5.0).abs() < 1e-12);
        let mean: f64 = (5.0 + 10.0 + 15.0) / 3.0;
        assert!((mean - 10.0).abs() < 1e-12);
    }

    #[test]
    fn does_not_rescale_recorded_closes() {
        let mut market = Market::new(1);
        let mut grain = MarketGood::new().with_amv(10.0);
        grain.record_amv();
        grain.set_amv(30.0);
        market.goods.insert(GRAIN, grain);
        market.goods.insert(COIN, MarketGood::new().with_amv(10.0));
        // Mean (30+10)/2 = 20; scale 0.5 to target 10.
        let scale = market.rescale_amv_to_mean(10.0, market_constants::AMV_MIN_ABS);
        assert!((scale - 0.5).abs() < 1e-12);
        assert_eq!(market.goods[&GRAIN].amv_trail(), vec![10.0]);
        assert!((market.goods[&GRAIN].amv - 15.0).abs() < 1e-12);
        market.goods.get_mut(&GRAIN).unwrap().record_amv();
        assert_eq!(market.goods[&GRAIN].amv_trail(), vec![10.0, 15.0]);
    }

    #[test]
    fn skips_when_mean_is_too_close_to_zero() {
        let mut market = Market::new(1);
        market.goods.insert(GRAIN, MarketGood::new().with_amv(1.0));
        market.goods.insert(COIN, MarketGood::new().with_amv(-1.0));
        market.rescale_amv_to_mean(10.0, market_constants::AMV_MIN_ABS);
        assert!((market.goods[&GRAIN].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv + 1.0).abs() < 1e-12);
    }

    #[test]
    fn does_not_scale_time() {
        let mut market = Market::new(1);
        market.goods.insert(TIME, MarketGood::new().with_amv(1.0));
        market.goods.insert(GRAIN, MarketGood::new().with_amv(2.0));
        market.goods.insert(COIN, MarketGood::new().with_amv(8.0));
        market.rescale_amv_to_mean(10.0, market_constants::AMV_MIN_ABS);
        assert!((market.goods[&TIME].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].amv - 4.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv - 16.0).abs() < 1e-12);
    }
}

#[cfg(test)]
mod match_orders_should {
    use super::*;
    use crate::game::config::market_priority;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(1)
    }

    fn request(pop: usize, good: usize, amount: f64, priority: f64) -> MarketOrder {
        MarketOrder::request_order(Actor::Pop(pop), good, amount, priority)
    }

    fn firm_request(firm: usize, good: usize, amount: f64, priority: f64) -> MarketOrder {
        MarketOrder::request_order(Actor::Firm(firm), good, amount, priority)
    }

    fn offer(pop: usize, good: usize, amount: f64, priority: f64) -> MarketOrder {
        MarketOrder::offer_order(Actor::Pop(pop), good, -amount, priority)
    }

    fn pair(buy_index: usize, sell_index: usize) -> OrderMatch {
        OrderMatch {
            buy_index,
            sell_index,
        }
    }

    #[test]
    fn empty_buys_are_an_empty_batch() {
        let sells = vec![offer(2, 10, 1.0, market_priority::POP_START)];
        let batch = Market::match_orders(&[], &sells, &mut rng());
        assert!(batch.is_empty());
    }

    #[test]
    fn pairs_a_buy_with_a_sell_of_the_same_good() {
        let buys = vec![request(1, 10, 3.0, market_priority::POP_START)];
        let sells = vec![offer(2, 10, 4.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert_eq!(batch.matched, Some(pair(0, 0)));
        assert!(batch.unmatched_buys.is_empty());
    }

    #[test]
    fn unmatched_buy_when_no_one_offers_that_good() {
        let buys = vec![request(1, 10, 3.0, market_priority::POP_START)];
        let sells = vec![offer(2, 11, 4.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert!(batch.matched.is_none());
        assert_eq!(batch.unmatched_buys, vec![0]);
    }

    #[test]
    fn skips_self_trade_and_reports_unmatched() {
        let buys = vec![request(1, 10, 3.0, market_priority::POP_START)];
        let sells = vec![offer(1, 10, 4.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert!(batch.matched.is_none());
        assert_eq!(batch.unmatched_buys, vec![0]);
    }

    #[test]
    fn a_later_buy_can_match_while_another_has_no_seller() {
        let buys = vec![
            request(1, 10, 3.0, market_priority::POP_START),
            request(2, 11, 3.0, 4.5),
        ];
        let sells = vec![offer(3, 11, 4.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert_eq!(batch.matched, Some(pair(1, 0)));
        assert!(batch.unmatched_buys.is_empty());
    }

    #[test]
    fn pop_or_firm_buy_may_match_the_same_sell() {
        let buys = vec![
            request(1, 10, 1.0, market_priority::POP_START),
            firm_request(1, 10, 1.0, market_priority::FIRM_PRODUCER),
        ];
        let sells = vec![offer(2, 10, 1.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        let m = batch.matched.expect("a buy should match");
        assert_eq!(m.sell_index, 0);
        assert!(m.buy_index == 0 || m.buy_index == 1);
        assert!(batch.unmatched_buys.is_empty());
    }

    #[test]
    fn hopeless_buy_does_not_block_another_good() {
        let buys = vec![
            firm_request(1, 10, 1.0, market_priority::FIRM_PRODUCER),
            request(1, 11, 1.0, market_priority::FIRM_PRODUCER),
        ];
        let sells = vec![offer(2, 11, 1.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert_eq!(batch.matched, Some(pair(1, 0)));
        assert!(batch.unmatched_buys.is_empty());
    }

    #[test]
    fn reports_every_hopeless_front_buy_in_one_pass() {
        let buys = vec![
            request(1, 10, 1.0, market_priority::POP_START),
            request(2, 11, 1.0, market_priority::POP_START),
        ];
        let sells = vec![offer(3, 12, 1.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert!(batch.matched.is_none());
        assert_eq!(batch.unmatched_buys, vec![0, 1]);
    }

    #[test]
    fn one_match_even_when_two_disjoint_pairs_exist() {
        let buys = vec![
            request(1, 10, 1.0, market_priority::POP_START),
            request(2, 20, 1.0, market_priority::POP_START),
        ];
        let sells = vec![
            offer(3, 10, 1.0, market_priority::POP_START),
            offer(4, 20, 1.0, market_priority::POP_START),
        ];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert!(batch.unmatched_buys.is_empty());
        assert!(batch.matched.is_some());
        let m = batch.matched.unwrap();
        assert!((m.buy_index == 0 && m.sell_index == 0) || (m.buy_index == 1 && m.sell_index == 1));
    }

    #[test]
    fn two_buys_one_sell_matches_one_and_skips_the_other() {
        let buys = vec![
            request(1, 10, 1.0, market_priority::POP_START),
            request(2, 10, 1.0, market_priority::POP_START),
        ];
        let sells = vec![offer(3, 10, 1.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert_eq!(batch.matched.unwrap().sell_index, 0);
        assert!(batch.unmatched_buys.is_empty());
    }

    #[test]
    fn picks_among_sells_of_the_target_good_only() {
        let buys = vec![request(1, 20, 1.0, market_priority::POP_START)];
        let sells = vec![
            offer(2, 10, 1.0, market_priority::POP_START),
            offer(3, 20, 1.0, market_priority::POP_START),
            offer(4, 30, 1.0, market_priority::POP_START),
        ];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert_eq!(batch.matched, Some(pair(0, 1)));
        assert!(batch.unmatched_buys.is_empty());
    }

    #[test]
    fn matching_counters_double_sell_weight_this_pick_only() {
        let buy = MarketOrder::buy_order(
            Actor::Firm(1),
            10,
            2.0,
            1.0,
            99,
            -2.0,
            market_priority::FIRM_MERCHANT,
        );
        let matching = MarketOrder::sell_order(
            Actor::Firm(2),
            10,
            -2.0,
            1.0,
            99,
            2.0,
            1.5,
        );
        let other_pay = MarketOrder::sell_order(
            Actor::Firm(3),
            10,
            -2.0,
            1.0,
            50,
            2.0,
            1.5,
        );
        let no_counter = offer(4, 10, 2.0, 1.5);
        let w = market_priority::SELL_COINCIDENCE_WEIGHT;
        assert!((sell_match_weight_with(&buy, &matching, w) - 4.0).abs() < 1e-12);
        assert!((sell_match_weight_with(&buy, &other_pay, w) - 2.0).abs() < 1e-12);
        assert!((sell_match_weight_with(&buy, &no_counter, w) - 2.0).abs() < 1e-12);
        assert_eq!(matching.priority, 1.5);
    }

    #[test]
    fn request_and_offer_without_counters_are_not_a_coincidence() {
        let buy = request(1, 10, 2.0, market_priority::POP_START);
        let sell = offer(2, 10, 2.0, 1.5);
        assert!(
            (sell_match_weight_with(&buy, &sell, market_priority::SELL_COINCIDENCE_WEIGHT)
                - 2.0)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn request_and_offer_with_the_same_named_counter_are_a_coincidence() {
        let buy = request(1, 10, 2.0, market_priority::POP_START).with_counter_offer(99);
        let sell = offer(2, 10, 2.0, 1.5).with_counter_offer(99);
        let w = market_priority::SELL_COINCIDENCE_WEIGHT;
        assert!((sell_match_weight_with(&buy, &sell, w) - 4.0).abs() < 1e-12);
        assert!(buy.is_request_order());
        assert!(sell.is_offer_order());
    }

    #[test]
    fn pick_weighted_index_walks_the_roll() {
        assert_eq!(pick_weighted_index(&[1.0, 9.0], 0.0), 0);
        assert_eq!(pick_weighted_index(&[1.0, 9.0], 0.999), 0);
        assert_eq!(pick_weighted_index(&[1.0, 9.0], 1.0), 1);
        assert_eq!(pick_weighted_index(&[1.0, 9.0], 9.5), 1);
    }
}

#[cfg(test)]
mod run_market_day_should {
    use super::*;
    use crate::game::actor::Actor;
    use crate::game::config::{market_constants, market_priority};
    use crate::game::factuals::Factuals;
    use crate::game::firm::{Firm, FirmPRow};
    use crate::game::good::{Good, TIME};
    use crate::game::workforce::{PaymentTerm, Workforce};
    use crate::game::household::Household;
    use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
    use crate::game::sentiment::Sentiment;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    const GRAIN: usize = 1;
    const COIN: usize = 2;
    const BREAD: usize = 3;
    const CARGO: usize = 9;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(1)
    }

    fn test_good(id: usize, name: &str) -> Good {
        Good {
            id,
            name: name.to_string(),
            class: None,
            decay_rate: 0.0,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 1.0,
            tags: HashSet::new(),
            categories: vec![],
        }
    }

    fn factuals() -> Factuals {
        let mut facts = Factuals::new()
            .with_good(test_good(GRAIN, "grain"))
            .with_good(test_good(COIN, "coin"));
        facts.config.market.amv_rescale_period = 0;
        facts.config.market.amv_imbalance_kick = 0.0;
        facts
    }

    fn priced_market() -> Market {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(1.0).with_salability(0.5),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(1.0),
        );
        market
    }

    fn extra_desire(good: usize, amount: f64) -> crate::game::desire::Desire {
        use crate::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
        use crate::game::scalingfactor::ScalingFactor;
        Desire {
            source: DesireSource::Species(0, 1),
            priority: 0,
            target: vec![DesireTarget::new(good, DesireTargetType::Consume, 1.0)],
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Household(1.0),
            decay: 0.0,
        }
    }

    fn shopper(id: usize, coin: f64, grain_shop: f64) -> Pop {
        shopper_for(id, coin, GRAIN, grain_shop)
    }

    fn shopper_for(id: usize, coin: f64, good: usize, shop: f64) -> Pop {
        let mut pop = Pop {
            id,
            job: 0,
            property: HashMap::new(),
            desires: vec![vec![]; 3],
            working_desires: vec![],
            demographics: DemoRow {
                household: Household::with_count(10.0),
                species: 0,
                culture: 0,
                class: 0,
                religion: 0,
            },
            current_orders: vec![],
            stored_effects: vec![],
            sentiment: Sentiment::new(),
            records: PopRecords::default(),
        };
        pop.property.insert(COIN, PopPRow::new(coin));
        pop.property
            .insert(good, PopPRow::new(0.0).with_target(shop));
        pop
    }

    fn accept_time_amv(
        time_amv: f64,
        time_sal: f64,
        hours: f64,
        pay_amv: f64,
        pay_sal: f64,
        blend: f64,
    ) -> f64 {
        let given_total = hours * time_amv;
        let mid = 0.5 * (given_total + pay_amv);
        let sold_blend = (blend * 2.0 * pay_sal / (time_sal.max(1e-9) + pay_sal.max(1e-9))).min(1.0);
        lerp(time_amv, mid / hours, sold_blend)
    }

    fn farm(id: usize, grain: f64, sell: f64) -> Firm {
        let mut firm = Firm::new(id, "farm".into(), 1, hexx::Hex::new(0, 0));
        firm.property.insert(
            GRAIN,
            FirmPRow::new()
                .with_quantity(grain)
                .with_sell_target(sell),
        );
        firm
    }

    #[test]
    fn empty_books_do_nothing() {
        let mut market = priced_market();
        let mut pops = HashMap::new();
        let mut firms = HashMap::new();
        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());
        assert_eq!(market.goods[&GRAIN].purchased, 0.0);
    }

    #[test]
    fn collates_opening_books_and_moves_stock_on_accept() {
        let mut market = priced_market();
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 4.0, 4.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        let grain = &market.goods[&GRAIN];
        assert!((grain.demand - 4.0).abs() < 1e-12);
        assert!((grain.supply - 10.0).abs() < 1e-12);
        assert!((grain.buyers - 1.0).abs() < 1e-12);
        assert!((grain.suppliers - 1.0).abs() < 1e-12);
        assert!((grain.purchased - 4.0).abs() < 1e-12);
        assert!((grain.volume() - 4.0).abs() < 1e-12);
        assert!((grain.requests - 4.0).abs() < 1e-12);

        let coin = &market.goods[&COIN];
        assert!((coin.tender - 4.0).abs() < 1e-12);
        assert!((coin.payment - 4.0).abs() < 1e-12);
        assert!((coin.volume() - 4.0).abs() < 1e-12);

        assert!((pops[&1].property[&GRAIN].quantity - 4.0).abs() < 1e-12);
        assert!((pops[&1].property[&COIN].quantity).abs() < 1e-12);
        assert!((firms[&1].property[&GRAIN].quantity - 6.0).abs() < 1e-12);
        assert!((firms[&1].property[&COIN].quantity - 4.0).abs() < 1e-12);
        assert!((firms[&1].property[&GRAIN].sold - 4.0).abs() < 1e-12);
        assert!(pops[&1].current_orders.is_empty());
        // Even AMV basket: no accept drift. Coin fully accepted: S lerps toward max.
        // Leftover grain sell does not move AMV (no leftover-book pressure).
        assert!((market.goods[&GRAIN].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv - 1.0).abs() < 1e-12);
        let coin_s = lerp(1.0, market_constants::SALABILITY_MAX, market_constants::SALABILITY_BLEND);
        assert!((market.goods[&COIN].salability - coin_s).abs() < 1e-12);
        assert!((market.goods[&GRAIN].salability - 0.5).abs() < 1e-12);
    }

    #[test]
    fn tendering_a_sell_good_does_not_overdraw_later_sell() {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(1.0).with_salability(0.4),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(1.0),
        );
        market.goods.insert(
            BREAD,
            MarketGood::new().with_amv(1.0).with_salability(0.6),
        );
        market.pops.insert(1);
        market.firms.insert(1);
        market.firms.insert(2);

        let mut pops = HashMap::new();
        let mut pop = shopper(1, 20.0, 0.0);
        pop.property
            .insert(BREAD, PopPRow::new(0.0).with_target(12.0));
        pops.insert(1, pop);

        let mut firms = HashMap::new();
        let mut farm_firm = farm(1, 10.0, 10.0);
        farm_firm
            .property
            .insert(COIN, FirmPRow::new().with_quantity(1.0));
        firms.insert(1, farm_firm);

        let mut bakery = Firm::new(2, "bakery".into(), 1, hexx::Hex::new(0, 0));
        bakery.property.insert(
            BREAD,
            FirmPRow::new().with_quantity(15.0).with_sell_target(12.0),
        );
        bakery.property.insert(
            GRAIN,
            FirmPRow::new().with_purchase_target(4.0),
        );
        firms.insert(2, bakery);

        let factuals = Factuals::new()
            .with_good(test_good(GRAIN, "grain"))
            .with_good(test_good(COIN, "coin"))
            .with_good(test_good(BREAD, "bread"));
        market.run_market_day(&factuals, &mut pops, &mut firms, &mut rng());

        assert!(firms[&2].property[&BREAD].quantity >= 0.0);
        assert!(firms[&2].property[&GRAIN].quantity >= 0.0);
    }

    #[test]
    fn scarce_supply_fills_one_of_two_buys() {
        let mut market = priced_market();
        market.pops.insert(1);
        market.pops.insert(2);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 20.0, 2.0));
        pops.insert(2, shopper(2, 5.0, 2.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 2.0, 2.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        let got = pops[&1].property[&GRAIN].quantity + pops[&2].property[&GRAIN].quantity;
        assert!((got - 2.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].purchased - 2.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].demand - 4.0).abs() < 1e-12);
    }

    #[test]
    fn leftover_sell_stays_after_a_partial_fill() {
        let mut market = priced_market();
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 10.0, 4.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 2.0, 2.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        assert!((pops[&1].property[&GRAIN].quantity - 2.0).abs() < 1e-12);
        assert!((firms[&1].property[&GRAIN].quantity).abs() < 1e-12);
        assert!((market.goods[&GRAIN].purchased - 2.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].supply - 2.0).abs() < 1e-12);
    }

    #[test]
    fn wash_leaves_stock_put_when_seller_rejects() {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(1.0).with_salability(0.5),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(0.2),
        );
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 10.0, 4.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        assert!((pops[&1].property[&GRAIN].quantity).abs() < 1e-12);
        assert!((pops[&1].property[&COIN].quantity - 10.0).abs() < 1e-12);
        assert!((firms[&1].property[&GRAIN].quantity - 10.0).abs() < 1e-12);
        assert!(!firms[&1].property.contains_key(&COIN));
        assert!(market.goods[&GRAIN].purchased.abs() < 1e-12);
        // Three deal attempts (initial + two auto-renews), then close-out.
        assert!((market.goods[&GRAIN].requests - 12.0).abs() < 1e-12);
        // Rejected meetings do not move AMV (rescale still equalizes).
        assert!((market.goods[&GRAIN].amv - market.goods[&COIN].amv).abs() < 1e-9);
        // Firm reject is weaker than a pop reject (retries apply the small blend).
        let pop_once = lerp(0.2, 0.0, 0.25);
        assert!(market.goods[&COIN].salability < 0.2);
        assert!(market.goods[&COIN].salability > pop_once);
    }

    #[test]
    fn reject_cuts_same_day_sell_weight() {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(1.0).with_salability(0.5),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(0.2),
        );
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 10.0, 4.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        let report = market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());
        let first = report
            .meetings
            .iter()
            .find(|m| matches!(m.outcome, MeetingOutcome::Wash { reason: WashReason::Rejected, .. }))
            .expect("reject");
        let leftover = report
            .leftover_sells
            .iter()
            .find(|o| o.origin == Actor::Firm(1) && o.target == GRAIN)
            .expect("leftover sell");
        let rejects = report
            .meetings
            .iter()
            .filter(|m| matches!(m.outcome, MeetingOutcome::Wash { reason: WashReason::Rejected, .. }))
            .count() as i32;
        let keep = 1.0 - market_priority::SELL_REJECT_WEIGHT;
        let want = first.sell.priority * keep.powi(rejects);
        assert!(
            (leftover.priority - want).abs() < 1e-9,
            "leftover {} want {want} after {rejects} rejects",
            leftover.priority
        );
        assert!(rejects >= 1);
    }

    #[test]
    fn unmatched_buy_marks_the_good_unavailable() {
        let mut market = priced_market();
        market.pops.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 10.0, 4.0));
        let mut firms = HashMap::new();

        let report = market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        assert!(market.unavailable_goods.contains(&GRAIN));
        assert!((pops[&1].property[&GRAIN].quantity).abs() < 1e-12);
        assert!(market.goods[&GRAIN].purchased.abs() < 1e-12);
        assert_eq!(report.unmatched_buys.len(), 1);
        assert_eq!(report.unmatched_buys[0].target, GRAIN);
        assert!(report.meetings.is_empty());
        assert!((market.goods[&GRAIN].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].salability - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_buy_with_no_seller_does_not_block_another_good() {
        let mut market = priced_market();
        market.goods.insert(
            BREAD,
            MarketGood::new().with_amv(1.0).with_salability(0.5),
        );
        market.pops.insert(1);
        market.pops.insert(2);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper_for(1, 100.0, BREAD, 4.0));
        pops.insert(2, shopper(2, 4.0, 4.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        let report = market.run_market_day(
            &factuals().with_good(test_good(BREAD, "bread")),
            &mut pops,
            &mut firms,
            &mut rng(),
        );

        assert!(report.meetings.iter().any(|m| {
            matches!(m.outcome, MeetingOutcome::Traded { .. }) && m.buy.target == GRAIN
        }));
        let leftover_grain_buys = report
            .leftover_buys
            .iter()
            .any(|order| order.target == GRAIN);
        let leftover_grain_sells = report
            .leftover_sells
            .iter()
            .any(|order| order.target == GRAIN);
        assert!(
            !(leftover_grain_buys && leftover_grain_sells),
            "grain buy and grain sell should have met, not both leftover"
        );
        assert!(report.unmatched_buys.iter().any(|order| order.target == BREAD));
    }

    #[test]
    fn extra_desire_is_not_a_second_trip() {
        let mut market = priced_market();
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pop = shopper(1, 20.0, 4.0);
        pop.desires[0].push(extra_desire(BREAD, 10.0));
        let mut pops = HashMap::new();
        pops.insert(1, pop);
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        let report = market.run_market_day(
            &factuals().with_good(test_good(BREAD, "bread")),
            &mut pops,
            &mut firms,
            &mut rng(),
        );
        assert!((pops[&1].property[&GRAIN].quantity - 4.0).abs() < 1e-12);
        assert!(report.unmatched_buys.iter().all(|order| order.target != BREAD));
        assert!(!market.unavailable_goods.contains(&BREAD));
    }

    #[test]
    fn report_records_a_trade_and_leftover_sell() {
        let mut market = priced_market();
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 4.0, 4.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        let report = market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());
        assert_eq!(report.unmatched_buys.len(), 0);
        assert_eq!(report.meetings.len(), 1);
        match &report.meetings[0].outcome {
            MeetingOutcome::Traded { goods, transport_needed } => {
                assert!((goods[&GRAIN] + 4.0).abs() < 1e-12);
                assert!((goods[&COIN] - 4.0).abs() < 1e-12);
                assert_eq!(*transport_needed, 0.0);
            }
            other => panic!("expected trade, got {other:?}"),
        }
        assert_eq!(report.leftover_buys.len(), 0);
        assert_eq!(report.leftover_sells.len(), 1);
        assert!((report.leftover_sells[0].target_amount + 6.0).abs() < 1e-12);
    }

    #[test]
    fn accept_whole_unit_overpay_pulls_amvs_together() {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(2.5).with_salability(0.5),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(1.0),
        );
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 3.0, 1.0));
        let mut firms = HashMap::new();
        // Sell equals the 1-unit fill so leftover 10% does not reverse accept lerp.
        firms.insert(1, farm(1, 1.0, 1.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        // 3 coin proposed; firm make-change returns 1 (keep 3/2.5 -> 2/2.5).
        assert!((pops[&1].property[&GRAIN].quantity - 1.0).abs() < 1e-12);
        assert!((pops[&1].property[&COIN].quantity - 1.0).abs() < 1e-12);
        assert!((firms[&1].property.get(&COIN).map(|r| r.quantity).unwrap_or(0.0) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn salability_lerps_toward_payment_over_tender() {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(1.0).with_salability(0.5),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(0.5),
        );
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 10.0, 4.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        // Coin fully accepted as payment: 0.5 -> lerp toward SALABILITY_MAX.
        let expected = lerp(0.5, market_constants::SALABILITY_MAX, market_constants::SALABILITY_BLEND);
        assert!((market.goods[&COIN].salability - expected).abs() < 1e-12);
        assert!((market.goods[&GRAIN].salability - 0.5).abs() < 1e-12);
    }

    #[test]
    fn records_opening_amv_and_each_days_close() {
        let mut market = priced_market();
        let mut pops = HashMap::new();
        let mut firms = HashMap::new();

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());
        let grain = &market.goods[&GRAIN];
        assert_eq!(grain.amv_trail(), vec![1.0, 1.0]);

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());
        let grain = &market.goods[&GRAIN];
        assert_eq!(grain.amv_trail(), vec![1.0, 1.0, 1.0]);
        assert!((grain.amv - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rescales_unweighted_mean_to_ten_every_day() {
        let mut market = priced_market();
        let mut pops = HashMap::new();
        let mut firms = HashMap::new();
        let mut facts = factuals();
        facts.config.market.amv_rescale_period = 1;

        market.run_market_day(&facts, &mut pops, &mut firms, &mut rng());
        assert_eq!(market.market_days, 1);
        assert!((market.goods[&GRAIN].amv - 100.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv - 100.0).abs() < 1e-12);
        let trail = market.goods[&GRAIN].amv_trail();
        assert!((trail[0] - 1.0).abs() < 1e-12);
        assert!((trail[trail.len() - 1] - 100.0).abs() < 1e-12);
    }

    #[test]
    fn rescale_does_not_scale_time() {
        let mut market = priced_market();
        market.goods.insert(TIME, MarketGood::new().with_amv(2.0));
        let mut pops = HashMap::new();
        let mut firms = HashMap::new();
        let mut facts = factuals();
        facts = facts.with_good(test_good(TIME, "time"));
        facts.config.market.amv_rescale_period = 1;

        market.run_market_day(&facts, &mut pops, &mut firms, &mut rng());
        assert!((market.goods[&GRAIN].amv - 100.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv - 100.0).abs() < 1e-12);
        assert!(
            (market.goods[&TIME].amv - 2.0).abs() < 1e-12,
            "time {}",
            market.goods[&TIME].amv
        );
    }

    #[test]
    fn rescales_firm_amv_quotes_with_the_market() {
        let mut market = priced_market();
        let mut facts = factuals();
        facts.config.market.amv_rescale_period = 1;
        let mut firm = farm(1, 10.0, 10.0);
        firm.property.get_mut(&GRAIN).unwrap().amv_target = 2.0;
        firm.property.get_mut(&GRAIN).unwrap().average_cost = 2.0;
        market.firms.insert(1);
        let mut firms = HashMap::new();
        firms.insert(1, firm);
        let mut pops = HashMap::new();
        market.run_market_day(&facts, &mut pops, &mut firms, &mut rng());
        let row = &firms[&1].property[&GRAIN];
        assert!((row.amv_target - 200.0).abs() < 1e-12);
        assert!((row.average_cost - 200.0).abs() < 1e-12);
    }

    #[test]
    fn reject_lowers_tender_salability_not_amv() {
        let mut market = priced_market();
        market.goods.get_mut(&GRAIN).unwrap().set_amv(20.0);
        market.goods.get_mut(&COIN).unwrap().set_amv(1.0);
        market.goods.get_mut(&COIN).unwrap().set_salability(1.0);
        let grain_amv = market.goods[&GRAIN].amv;
        let cfg = crate::game::config::MarketConfig::default();
        let mut goods = HashMap::new();
        goods.insert(GRAIN, -1.0);
        goods.insert(COIN, 20.0);
        market.drift_salability_on_reject(GRAIN, &goods, &cfg, cfg.salability_blend);
        assert!((market.goods[&GRAIN].amv - grain_amv).abs() < 1e-12);
        let expected = lerp(1.0, 0.0, cfg.salability_blend);
        assert!((market.goods[&COIN].salability - expected).abs() < 1e-12);
    }

    #[test]
    fn firm_reject_does_not_lower_tender_salability() {
        let mut market = priced_market();
        market.goods.get_mut(&COIN).unwrap().set_salability(0.3);
        market.pops.insert(1);
        market.firms.insert(1);
        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 10.0, 4.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));
        let before = market.goods[&COIN].salability;
        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());
        assert_eq!(market.goods[&GRAIN].purchased, 0.0);
        let pop_once = lerp(before, 0.0, 0.25);
        assert!(market.goods[&COIN].salability < before);
        assert!(market.goods[&COIN].salability > pop_once);
    }

    #[test]
    fn close_records_the_drifted_amv() {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(2.5).with_salability(0.5),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(1.0),
        );
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 10.0, 1.0));
        let mut firms = HashMap::new();
        // Sell equals the 1-unit fill so leftover 10% does not reverse accept lerp.
        firms.insert(1, farm(1, 1.0, 1.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        let grain = &market.goods[&GRAIN];
        let trail = grain.amv_trail();
        assert_eq!(trail.len(), 2);
        assert!((trail[0] - 2.5).abs() < 1e-12);
        assert!((trail[1] - grain.amv).abs() < 1e-12);
        assert!((market.goods[&GRAIN].purchased - 1.0).abs() < 1e-12);

        let coin = &market.goods[&COIN];
        let trail = coin.amv_trail();
        assert_eq!(trail.len(), 2);
        assert!((trail[0] - 1.0).abs() < 1e-12);
        assert!((trail[1] - coin.amv).abs() < 1e-12);
    }

    fn cargo_good() -> Good {
        let mut good = test_good(CARGO, "cargo");
        good.mass = 0.0;
        good.volume = 0.0;
        good.with_transport_efficiency(1.0)
    }

    fn factuals_with_cargo() -> Factuals {
        factuals().with_good(cargo_good())
    }

    fn shopper_with_cargo(id: usize, coin: f64, grain_shop: f64, cargo: f64) -> Pop {
        let mut pop = shopper(id, coin, grain_shop);
        pop.property.insert(CARGO, PopPRow::new(cargo));
        pop
    }

    #[test]
    fn success_spends_the_flat_transport_fee() {
        let mut market = priced_market();
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper_with_cargo(1, 10.0, 4.0, 25.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        market.run_market_day(&factuals_with_cargo(), &mut pops, &mut firms, &mut rng());

        assert!((pops[&1].property[&GRAIN].quantity - 4.0).abs() < 1e-12);
        assert!(
            (pops[&1].property[&CARGO].quantity - (25.0 - market_constants::TRANSACTION_COST))
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn success_spends_transport_by_efficiency() {
        let mut market = priced_market();
        market.pops.insert(1);
        market.firms.insert(1);

        let cargo = cargo_good().with_transport_efficiency(2.0);
        let factuals = factuals().with_good(cargo);

        let mut pops = HashMap::new();
        pops.insert(1, shopper_with_cargo(1, 10.0, 4.0, 5.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        market.run_market_day(&factuals, &mut pops, &mut firms, &mut rng());

        assert!((pops[&1].property[&GRAIN].quantity - 4.0).abs() < 1e-12);
        // 5 cargo at efficiency 2.0; fee 1 spends 0.5 units.
        assert!((pops[&1].property[&CARGO].quantity - 4.5).abs() < 1e-12);
    }

    #[test]
    fn wash_spends_the_flat_fee_each_meeting() {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(1.0).with_salability(0.5),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(0.0),
        );
        market.goods.insert(
            CARGO,
            MarketGood::new().with_amv(1.0).with_salability(0.0),
        );
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper_with_cargo(1, 10.0, 4.0, 40.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        market.run_market_day(&factuals_with_cargo(), &mut pops, &mut firms, &mut rng());

        assert!((pops[&1].property[&GRAIN].quantity).abs() < 1e-12);
        let spent = 3.0 * market_constants::TRANSACTION_COST;
        assert!((pops[&1].property[&CARGO].quantity - (40.0 - spent)).abs() < 1e-12);
    }

    #[test]
    fn leftover_buy_order_scales_the_counter() {
        let order = MarketOrder::buy_order(
            Actor::Firm(1),
            GRAIN,
            4.0,
            1.0,
            COIN,
            -4.0,
            market_priority::FIRM_PRODUCER,
        );
        let leftover = leftover_order(order, 2.0).expect("remaining");
        assert!((leftover.target_amount - 2.0).abs() < 1e-12);
        assert!((leftover.counter_offer_amount.unwrap() + 2.0).abs() < 1e-12);
        assert!(leftover_order(
            MarketOrder::offer_order(
                Actor::Firm(1),
                GRAIN,
                -2.0,
                1.0,
            ),
            2.0
        )
        .is_none());
    }

    #[test]
    fn leftover_buy_order_snaps_a_fractional_counter() {
        let order = MarketOrder::buy_order(
            Actor::Firm(1),
            GRAIN,
            5.0,
            1.0,
            COIN,
            -9.0,
            market_priority::FIRM_PRODUCER,
        );
        let leftover = leftover_order(order, 2.0).expect("remaining");
        assert_eq!(leftover.target_amount, 3.0);
        // 9 * 3/5 = 5.4, trunc to 5
        assert_eq!(leftover.counter_offer_amount, Some(-5.0));
    }

    #[test]
    fn leftover_sell_with_no_buyers_does_not_move_amv() {
        let mut market = priced_market();
        market.firms.insert(1);
        let mut pops = HashMap::new();
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        let report = market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        assert!(report.leftover_sells.iter().any(|o| o.target == GRAIN));
        assert!(market.goods[&GRAIN].purchased.abs() < 1e-12);
        assert!((market.goods[&GRAIN].amv - 1.0).abs() < 1e-12);
    }

    #[test]
    fn settle_labor_stamps_time_amv_from_paid_wages() {
        let mut market = Market::new(1);
        market.pops.insert(2);
        market.firms.insert(1);
        market.goods.insert(TIME, MarketGood::new().with_amv(1.0));
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(0.21).with_salability(1.0),
        );

        let worker = Workforce::new(2)
            .with_hours(10.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.property
            .insert(COIN, FirmPRow::new().with_quantity(100.0));
        let mut pop = shopper(2, 0.0, 0.0);
        pop.property.insert(TIME, PopPRow::new(48.0));

        let mut pops = HashMap::from([(2, pop)]);
        let mut firms = HashMap::from([(1, firm)]);
        let mut facts = factuals();
        facts = facts.with_good(test_good(TIME, "time"));

        let wages = market.settle_labor(&mut pops, &mut firms, &facts);
        assert_eq!(wages.len(), 1);
        assert!((wages[0].1.workers[0].time_given - 10.0).abs() < 1e-12);
        let exchange = &firms[&1].workforce[0].last_exchange;
        assert!((exchange[&TIME] + 10.0).abs() < 1e-12);
        assert!(exchange.get(&COIN).copied().unwrap_or(0.0) > 0.0);
        let coins = exchange[&COIN];
        let time = &market.goods[&TIME];
        let expected = accept_time_amv(
            1.0,
            time.salability,
            10.0,
            coins * 0.21,
            1.0,
            facts.config.market.amv_accept_blend,
        );
        assert!((time.amv - expected).abs() < 1e-9, "time amv {}", time.amv);
        assert!((time.purchased - 10.0).abs() < 1e-12);
        assert!(time.demand > 0.0);
        assert!(time.supply > 0.0);
    }

    #[test]
    fn unpaid_hours_keep_the_going_time_amv() {
        let mut market = Market::new(1);
        market.pops.insert(2);
        market.firms.insert(1);
        market.goods.insert(TIME, MarketGood::new().with_amv(100.0));
        let worker = Workforce::new(2).with_hours(10.0);
        let firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(crate::game::actor::Actor::Pop(2))
            .with_owner_liability()
            .with_workforce(worker);
        let mut pop = shopper(2, 0.0, 0.0);
        pop.property.insert(TIME, PopPRow::new(48.0));
        let mut pops = HashMap::from([(2, pop)]);
        let mut firms = HashMap::from([(1, firm)]);
        let mut facts = factuals();
        facts = facts.with_good(test_good(TIME, "time"));
        market.settle_labor(&mut pops, &mut firms, &facts);
        assert!((market.goods[&TIME].amv - 100.0).abs() < 1e-9);
    }

    #[test]
    fn remainder_in_kind_payout_prices_time() {
        let mut market = Market::new(1);
        market.pops.insert(2);
        market.firms.insert(1);
        market.goods.insert(TIME, MarketGood::new().with_amv(100.0));
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(10.0).with_salability(0.5),
        );
        let worker = Workforce::new(2).with_hours(10.0);
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(crate::game::actor::Actor::Pop(2))
            .with_owner_liability()
            .with_workforce(worker);
        firm.property
            .insert(GRAIN, FirmPRow::new().with_quantity(10.0));
        let mut pop = shopper(2, 0.0, 0.0);
        pop.property.insert(TIME, PopPRow::new(48.0));
        let mut pops = HashMap::from([(2, pop)]);
        let mut firms = HashMap::from([(1, firm)]);
        let mut facts = factuals();
        facts = facts.with_good(test_good(TIME, "time"));

        let wages = market.settle_labor(&mut pops, &mut firms, &facts);
        let paid = wages[0]
            .1
            .owner
            .as_ref()
            .map(|owner| owner.paid_amv)
            .unwrap_or(0.0);
        assert!(paid > 0.0, "remainder should move grain");
        let given = wages[0].1.workers[0].time_given;
        assert!((given - 10.0).abs() < 1e-12);
        let exchange = &firms[&1].workforce[0].last_exchange;
        assert!((exchange[&TIME] + given).abs() < 1e-12);
        assert!(exchange.get(&GRAIN).copied().unwrap_or(0.0) > 0.0);
        let grain_qty = exchange[&GRAIN];
        let time_sal = MarketGood::new().salability;
        let expected = accept_time_amv(
            100.0,
            time_sal,
            given,
            grain_qty * 10.0,
            0.5,
            facts.config.market.amv_accept_blend,
        );
        assert!(
            (market.goods[&TIME].amv - expected).abs() < 1e-9,
            "time {} want {}",
            market.goods[&TIME].amv,
            expected
        );
    }

    #[test]
    fn recap_is_on_the_contract_and_does_not_price_time() {
        let mut market = Market::new(1);
        market.pops.insert(2);
        market.firms.insert(1);
        market.goods.insert(TIME, MarketGood::new().with_amv(100.0));
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(10.0).with_salability(0.5),
        );
        let worker = Workforce::new(2).with_hours(10.0);
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(crate::game::actor::Actor::Pop(2))
            .with_owner_liability()
            .with_workforce(worker);
        firm.property.insert(
            GRAIN,
            FirmPRow::new()
                .with_quantity(0.0)
                .with_use_target(5.0)
                .with_stock_target(5.0),
        );
        let mut pop = shopper(2, 0.0, 0.0);
        pop.property.insert(TIME, PopPRow::new(48.0));
        pop.property.insert(GRAIN, PopPRow::new(10.0));
        let mut pops = HashMap::from([(2, pop)]);
        let mut firms = HashMap::from([(1, firm)]);
        let mut facts = factuals();
        facts = facts.with_good(test_good(TIME, "time"));

        let wages = market.settle_labor(&mut pops, &mut firms, &facts);
        let recap = wages[0]
            .1
            .owner
            .as_ref()
            .map(|owner| owner.recap.get(&GRAIN).copied().unwrap_or(0.0))
            .unwrap_or(0.0);
        assert!(recap > 0.0, "owner should recap grain");
        let given = wages[0].1.workers[0].time_given;
        assert!(given > 0.0);
        let exchange = &firms[&1].workforce[0].last_exchange;
        assert!((exchange[&TIME] + given).abs() < 1e-12);
        assert!(exchange.get(&GRAIN).copied().unwrap_or(0.0) < 0.0);
        assert!(
            (market.goods[&TIME].amv - 100.0).abs() < 1e-9,
            "recap-only settle is not a Time sale, time {}",
            market.goods[&TIME].amv
        );
    }

    #[test]
    fn imbalance_kick_raises_amv_when_demand_exceeds_supply() {
        let mut market = priced_market();
        market.goods.get_mut(&GRAIN).unwrap().set_demand(10.0);
        market.goods.get_mut(&GRAIN).unwrap().set_supply(4.0);
        let cfg = crate::game::config::MarketConfig::default();
        market.nudge_amv_from_imbalance(&cfg);
        assert!((market.goods[&GRAIN].amv - 2.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv - 1.0).abs() < 1e-12);
    }

    #[test]
    fn imbalance_kick_cuts_amv_when_supply_exceeds_demand() {
        let mut market = priced_market();
        market.goods.get_mut(&GRAIN).unwrap().set_amv(10.0);
        market.goods.get_mut(&GRAIN).unwrap().set_demand(4.0);
        market.goods.get_mut(&GRAIN).unwrap().set_supply(10.0);
        let cfg = crate::game::config::MarketConfig::default();
        market.nudge_amv_from_imbalance(&cfg);
        assert!((market.goods[&GRAIN].amv - 9.0).abs() < 1e-12);
    }

    #[test]
    fn imbalance_kick_skips_ties_and_time() {
        let mut market = priced_market();
        market.goods.insert(TIME, MarketGood::new().with_amv(5.0));
        market.goods.get_mut(&TIME).unwrap().set_demand(9.0);
        market.goods.get_mut(&TIME).unwrap().set_supply(1.0);
        market.goods.get_mut(&GRAIN).unwrap().set_demand(3.0);
        market.goods.get_mut(&GRAIN).unwrap().set_supply(3.0);
        let cfg = crate::game::config::MarketConfig::default();
        market.nudge_amv_from_imbalance(&cfg);
        assert!((market.goods[&TIME].amv - 5.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].amv - 1.0).abs() < 1e-12);
    }
}