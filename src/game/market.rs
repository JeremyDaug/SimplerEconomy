use std::collections::{HashMap, HashSet};

use circular_buffer::CircularBuffer;
use rand::Rng;
use rand::seq::SliceRandom;

use crate::game::actor::Actor;
use crate::game::config::{market_constants, market_priority};
use crate::game::deal::{DealMaker, DealResponse};
use crate::game::firm::Firm;
use crate::game::marketorder::{pop_priority_from_wealth, MarketOrder};
use crate::game::pop::Pop;
use crate::game::util::{lerp, whole_units};
use crate::game::{actors::Actors, factuals::Factuals};

/// One buy/sell pair from [`Market::match_orders`].
///
/// Indices refer to the `buys` / `sells` slices passed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderMatch {
    pub buy_index: usize,
    pub sell_index: usize,
}

/// One matching pass: at most one deal, plus every front-group buy that has
/// no other-origin seller at all.
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

/// Length of the leading run of buys that share `buys[0].priority`.
fn front_priority_group_len(buys: &[MarketOrder]) -> usize {
    let best = buys[0].priority;
    let mut n = 1;
    while n < buys.len() && buys[n].priority == best {
        n += 1;
    }
    n
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

/// Sell selection weight for this pick only.
/// Matching `Some` counter-offer goods doubles the stored sell priority.
fn sell_match_weight(buy: &MarketOrder, sell: &MarketOrder) -> f64 {
    let mut weight = sell.priority.max(0.0);
    if matching_counter_offers(buy, sell) {
        weight *= market_priority::SELL_COINCIDENCE_WEIGHT;
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
        weights.push(sell_match_weight(buy, &sells[i]));
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
        .pay_transport(market_constants::TRANSACTION_COST, factuals);
    sells.push(sell_order);
    if let Some(renewed) = as_deal_maker(pops, firms, buy_order.origin).renew_buy(&buy_order) {
        buys.push(renewed);
        true
    } else {
        false
    }
}

/// Flat wash fee when the world has transport-tagged goods, else 0.
fn wash_transport(factuals: &Factuals) -> f64 {
    if factuals.goods.values().any(|good| good.is_transport()) {
        market_constants::TRANSACTION_COST
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

/// If `new` is inside the AMV dead zone, land `AMV_MIN_ABS` on the other side
/// of 0 from `old`. Otherwise return `new` unchanged.
fn bounce_away_from_zero(old: f64, new: f64) -> f64 {
    debug_assert!(new.is_finite(), "new must be finite");
    let min_abs = market_constants::AMV_MIN_ABS;
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
    /// 1. Collect orders from member pops and firms (`create_orders`). Pop
    ///    buy/request order priority is written from per-household wealth.
    ///    Institution and state orders are not collected yet.
    /// 2. Collate opening supply, demand, buyers, and suppliers onto
    ///    [`MarketGood`] rows.
    /// 3. Match loop, until no buy remains that can pair:
    ///    1. [`Market::match_orders`] (one pair, plus hopeless front-group buys).
    ///    2. Unmatched buys (no other-origin seller): mark the good on
    ///       [`Market::unavailable_goods`], no transport fee, no renew.
    ///    3. Matched pair: buyer `buy` (that basket is the buyer's accept),
    ///       seller `evaluate`. Accept -> [`DealMaker::finalize`] both
    ///       parties, buyer pays `transport_needed` after the map, leftover
    ///       orders reinserted. Reject / no proposal -> wash (flat
    ///       [`market_constants::TRANSACTION_COST`] from on-hand; buyer may
    ///       renew up to [`market_constants::BUY_TRY_LIMIT`] retries).
    ///    4. New orders after a fill (`Pop::next_shopping_trip`, firm re-emit)
    ///       are deferred.
    /// 4. Cleanup: clear member pops' `current_orders`. AMV is written on
    ///    [`MarketGood`] as meetings resolve (history stays the opening
    ///    snapshot). Salability updates from payment/tender after the loop.
    ///    Leftover book carry and re-planning are deferred.
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
        let mut report = MarketDayReport::default();

        let history = self.history();
        self.seed_amv_history();
        let (mut buys, mut sells) = self.collect_orders(&history, factuals, pops, firms);
        self.reset_day_exchange_stats();
        self.collate_order_books(&buys, &sells);

        let mut steps = 0usize;
        loop {
            steps += 1;
            debug_assert!(steps < 1_000_000, "market day failed to terminate");

            buys.sort_by(|a, b| {
                a.priority
                    .partial_cmp(&b.priority)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            sells.sort_by_key(|order| order.target);

            let batch = Self::match_orders(&buys, &sells, rng);
            if batch.is_empty() {
                break;
            }

            let matched = batch.matched.map(|pair| {
                (
                    buys[pair.buy_index].clone(),
                    sells[pair.sell_index].clone(),
                    pair.sell_index,
                )
            });

            for &i in &batch.unmatched_buys {
                self.unavailable_goods.insert(buys[i].target);
                report.unmatched_buys.push(buys[i].clone());
            }

            let mut remove_buys = batch.unmatched_buys.clone();
            if let Some(pair) = batch.matched {
                remove_buys.push(pair.buy_index);
            }
            remove_buys.sort_unstable();
            remove_buys.dedup();
            for i in remove_buys.into_iter().rev() {
                buys.remove(i);
            }
            if let Some((_, _, sell_index)) = matched {
                sells.remove(sell_index);
            }

            if let Some((buy_order, sell_order, _)) = matched {
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
        }

        for &id in &self.pops {
            pops.get_mut(&id)
                .unwrap_or_else(|| panic!("market pop {id} missing from pops"))
                .current_orders
                .clear();
        }

        self.update_salability();
        self.record_amv_closes();

        report.leftover_buys = buys;
        report.leftover_sells = sells;
        report
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

    /// Lerps each good's salability toward `payment / tender` when it was
    /// offered as payment today. Goods with no tender are left alone.
    fn update_salability(&mut self) {
        let blend = market_constants::SALABILITY_BLEND;
        for good in self.goods.values_mut() {
            if good.tender <= 0.0 {
                continue;
            }
            let accept = (good.payment / good.tender).clamp(0.0, 1.0);
            if !accept.is_finite() {
                continue;
            }
            good.set_salability(lerp(good.salability, accept, blend));
        }
    }

    /// Pulls AMV of the sold good and its tenders toward the midpoint of the
    /// basket totals. Uses live [`MarketGood::amv`], not the frozen history.
    fn drift_amv_on_accept(&mut self, target: usize, filled: f64, goods: &HashMap<usize, f64>) {
        if filled <= 0.0 {
            return;
        }
        let blend = market_constants::AMV_ACCEPT_BLEND;
        let target_amv = self.market_good_mut(target).amv;
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
            let amv = self.market_good_mut(id).amv;
            pays.push((id, qty, amv));
            pay_total += qty * amv;
        }
        if pay_total <= 0.0 || !pay_total.is_finite() {
            return;
        }

        let mid = 0.5 * (given_total + pay_total);
        let new_target = lerp(target_amv, mid / filled, blend);
        self.market_good_mut(target).set_amv(new_target);

        let scale = mid / pay_total;
        for (id, _, amv) in pays {
            let implied = amv * scale;
            self.market_good_mut(id).set_amv(lerp(amv, implied, blend));
        }
    }

    /// Raises the sought good's AMV and lowers each tender's AMV.
    /// Tender down-push scales with units offered per unit sought.
    fn drift_amv_on_reject(&mut self, target: usize, goods: &HashMap<usize, f64>) {
        let sought = goods.get(&target).copied().unwrap_or(0.0).abs();
        if sought <= 0.0 {
            return;
        }
        let blend = market_constants::AMV_REJECT_BLEND;
        let edge = market_constants::AMV_REJECT_DEMAND_EDGE;
        let old = self.market_good_mut(target).amv;
        self.market_good_mut(target).set_amv(lerp(old, old * edge, blend));

        for (&id, &qty) in goods {
            if id == target || qty <= 0.0 {
                continue;
            }
            let down_blend = (blend * (qty / sought)).min(1.0);
            let old = self.market_good_mut(id).amv;
            self.market_good_mut(id).set_amv(lerp(old, old / edge, down_blend));
        }
    }

    /// Raises the sought good's AMV when a meeting produced no basket.
    fn drift_amv_on_no_proposal(&mut self, target: usize) {
        let blend = market_constants::AMV_REJECT_BLEND;
        let edge = market_constants::AMV_REJECT_DEMAND_EDGE;
        let old = self.market_good_mut(target).amv;
        self.market_good_mut(target).set_amv(lerp(old, old * edge, blend));
    }

    /// Emits pop and firm orders for this market and splits them into buy and
    /// sell books. Pop buys get wealth-rank order priority.
    fn collect_orders(
        &self,
        history: &MarketHistory,
        factuals: &Factuals,
        pops: &HashMap<usize, Pop>,
        firms: &HashMap<usize, Firm>,
    ) -> (Vec<MarketOrder>, Vec<MarketOrder>) {
        let mut buys = Vec::new();
        let mut sells = Vec::new();

        let mut wealth = HashMap::new();
        let mut max_wealth = 0.0;
        for &id in &self.pops {
            let pop = pops.get(&id).expect("market pop missing from pops");
            let households = pop.demographics.household.count;
            let per_household = if households > 0.0 {
                pop.property_wealth_amv(history) / households
            } else {
                0.0
            };
            if per_household > max_wealth {
                max_wealth = per_household;
            }
            wealth.insert(id, per_household);
        }

        for &id in &self.pops {
            let pop = pops.get(&id).expect("market pop missing from pops");
            let per_household = wealth[&id];
            let mut orders = pop.create_orders(history, factuals, &self.unavailable_goods);
            for order in &mut orders {
                if order.target_amount > 0.0 {
                    order.set_priority(pop_priority_from_wealth(per_household, max_wealth));
                }
            }
            split_into_books(orders, &mut buys, &mut sells);
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
        for good in self.goods.values_mut() {
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
    fn collate_order_books(&mut self, buys: &[MarketOrder], sells: &[MarketOrder]) {
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
            let row = self.market_good_mut(good);
            row.set_demand(qty);
            row.set_buyers(n);
        }
        for (good, qty) in supply {
            let n = suppliers
                .get(&good)
                .map(|set| set.len() as f64)
                .unwrap_or(0.0);
            let row = self.market_good_mut(good);
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
    ///    basket midpoint, [`DealMaker::finalize`] both inventories, push
    ///    leftover order amounts back onto `buys` / `sells`.
    /// 4. Reject / no proposal: drift AMV (sought up, tenders down on reject),
    ///    wash. Charge [`market_constants::TRANSACTION_COST`]
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
            self.add_requests(target, sought);
        }

        let Some(proposal) = as_deal_maker(pops, firms, buy_order.origin)
            .buy(&buy_order, &sell_order, history, factuals)
        else {
            self.drift_amv_on_no_proposal(target);
            let transport = wash_transport(factuals);
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
            return;
        };
        for (&good, &qty) in &proposal.goods {
            if qty > 0.0 {
                self.add_tender(good, qty);
            }
        }

        let verdict = as_deal_maker(pops, firms, sell_order.origin)
            .evaluate(&proposal, &sell_order, &buy_order, history, factuals);
        if verdict != DealResponse::Accept {
            // TODO: Counteroffer haggling. The rewrite is seller-approved; the
            // buyer would then evaluate it (or a close-out). Wash for now.
            self.drift_amv_on_reject(target, &proposal.goods);
            let transport = wash_transport(factuals);
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
            return;
        }

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

        let payment_amv: f64 = proposal.goods.iter()
            .filter_map(|(&good, &qty)| (qty > 0.0).then_some(qty * history.price(good)))
            .sum();
        self.record_fill(target, filled, payment_amv / filled);
        for (&good, &qty) in &proposal.goods {
            if qty > 0.0 {
                self.add_payment(good, qty);
            }
        }
        self.drift_amv_on_accept(target, filled, &proposal.goods);

        as_deal_maker_mut(pops, firms, buy_order.origin).finalize(&proposal, history);
        as_deal_maker_mut(pops, firms, sell_order.origin).finalize(&proposal, history);
        as_deal_maker_mut(pops, firms, buy_order.origin)
            .pay_transport(proposal.transport_needed, factuals);

        if let Some(leftover) = leftover_order(buy_order, filled) {
            buys.push(leftover);
        }
        if let Some(mut leftover) = leftover_order(sell_order, filled) {
            leftover.add_sell_success_bonus();
            sells.push(leftover);
        }

        meetings.push(MarketMeeting {
            buy: buy_snap,
            sell: sell_snap,
            outcome: MeetingOutcome::Traded {
                goods: proposal.goods.clone(),
                transport_needed: proposal.transport_needed,
            },
        });
    }

    /// Returns the row for `good`, inserting a default if it is new.
    fn market_good_mut(&mut self, good: usize) -> &mut MarketGood {
        self.goods.entry(good).or_insert_with(MarketGood::new)
    }

    /// Adds `qty` to this good's deal-request total.
    fn add_requests(&mut self, good: usize, qty: f64) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good);
        row.set_requests(row.requests + qty);
    }

    /// Adds `qty` to this good's offered-as-payment total.
    fn add_tender(&mut self, good: usize, qty: f64) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good);
        row.set_tender(row.tender + qty);
    }

    /// Adds `qty` to this good's accepted-as-payment total.
    fn add_payment(&mut self, good: usize, qty: f64) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good);
        row.set_payment(row.payment + qty);
    }

    /// Records a successful purchase of `qty` at `unit_price` on the target
    /// good (purchased and rolling average price). Volume is derived.
    fn record_fill(&mut self, good: usize, qty: f64, unit_price: f64) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        debug_assert!(unit_price.is_finite(), "unit_price must be finite");
        let row = self.market_good_mut(good);
        let prev_qty = row.purchased;
        let prev_avg = row.average_price;
        let new_qty = prev_qty + qty;
        row.set_purchased(new_qty);
        if new_qty > 0.0 {
            row.set_average_price((prev_avg * prev_qty + unit_price * qty) / new_qty);
        }
    }

    /// # Match Orders
    ///
    /// One pass over the **front** buy-priority group. Does not mutate the
    /// lists; the caller removes, updates, or reinserts after.
    ///
    /// `buys` must be sorted by order priority (lowest first, FCFS). `sells`
    /// must be sorted by target good id. The front group is shuffled. At most
    /// **one** match (weighted sell of that good; coincidence doubles this pick
    /// only). Every front-group buy with no other-origin seller is listed in
    /// `unmatched_buys` so the caller can update those while the one deal
    /// runs. Later priority groups wait for the next call.
    pub fn match_orders<R: Rng + ?Sized>(
        buys: &[MarketOrder],
        sells: &[MarketOrder],
        rng: &mut R,
    ) -> OrderMatchBatch {
        if buys.is_empty() {
            return OrderMatchBatch::empty();
        }
        debug_assert!(
            buys.windows(2).all(|w| w[0].priority <= w[1].priority),
            "buys must be sorted by priority, lowest first"
        );
        debug_assert!(
            sells.windows(2).all(|w| w[0].target <= w[1].target),
            "sells must be sorted by target id"
        );

        let group_len = front_priority_group_len(buys);
        let mut group: Vec<usize> = (0..group_len).collect();
        group.shuffle(rng);

        let mut matched = None;
        let mut unmatched_buys = Vec::new();

        for buy_index in group {
            let buy = &buys[buy_index];
            debug_assert!(
                buy.target_amount > 0.0,
                "buy target_amount must be > 0.0"
            );

            let (available, weights, had_other) = classify_sells(sells, buy);
            if !had_other {
                unmatched_buys.push(buy_index);
                continue;
            }
            if matched.is_some() {
                continue;
            }
            if let Some(sell_index) = pick_available_sell(&available, &weights, rng) {
                matched = Some(OrderMatch {
                    buy_index,
                    sell_index,
                });
            }
        }

        unmatched_buys.sort_unstable();
        OrderMatchBatch {
            matched,
            unmatched_buys,
        }
    }

    /// # History
    ///
    /// Snapshot of current AMVs and salability for pop record keeping and
    /// sentiment wealth. Readers default missing prices to 1.0 and missing
    /// salability to [`market_constants::SALABILITY_DEFAULT`].
    pub fn history(&self) -> MarketHistory {
        let mut history = MarketHistory::new();
        for (&good_id, good) in &self.goods {
            history.prices.insert(good_id, good.amv);
            history.salability.insert(good_id, good.salability);
        }
        history.friction = self.friction;
        history
    }
}

/// # Market History
/// 
/// A saved record of minimal data for passing around.
#[derive(Debug, Clone, Default)]
pub struct MarketHistory {
    /// Last known AMV price per good.
    pub prices: HashMap<usize, f64>,
    /// Last known salability per good, typically in 0.0..=1.0.
    pub salability: HashMap<usize, f64>,
    /// Market friction factor copied from [`Market::friction`].
    pub friction: f64,
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

impl MarketHistory {
    pub(crate) fn new() -> Self {
        Self { 
            prices: HashMap::new(),
            salability: HashMap::new(),
            friction: 0.0,
        }
    }

    /// Price for `good_id`, or 1.0 if missing.
    pub fn price(&self, good_id: usize) -> f64 {
        self.prices.get(&good_id).copied().unwrap_or(1.0)
    }

    /// Salability for `good_id`, or [`market_constants::SALABILITY_DEFAULT`] if missing.
    pub fn salability(&self, good_id: usize) -> f64 {
        self.salability
            .get(&good_id)
            .copied()
            .unwrap_or(market_constants::SALABILITY_DEFAULT)
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

    /// Sets the current Abstract Market Value.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn set_amv(&mut self, amv: f64) {
        self.amv = bounce_away_from_zero(self.amv, amv);
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

    /// Sets salability, clamped to `0.0..=1.0`.
    pub fn set_salability(&mut self, salability: f64) {
        debug_assert!(salability.is_finite(), "salability must be finite");
        self.salability = salability.clamp(0.0, 1.0);
    }

    /// Sets salability, clamped to `0.0..=1.0`.
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
        self.average_price = bounce_away_from_zero(self.average_price, average_price);
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
    fn clamps_salability_to_unit_interval() {
        assert_eq!(MarketGood::new().with_salability(1.5).salability, 1.0);
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
        assert_eq!(good.salability, 1.0);
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
    fn later_priority_does_not_match_while_front_group_remains() {
        let buys = vec![
            request(1, 10, 3.0, market_priority::POP_START),
            request(2, 11, 3.0, 4.5),
        ];
        let sells = vec![offer(3, 11, 4.0, market_priority::POP_START)];
        let batch = Market::match_orders(&buys, &sells, &mut rng());
        assert_eq!(batch.unmatched_buys, vec![0]);
        assert!(batch.matched.is_none());
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
        assert!((sell_match_weight(&buy, &matching) - 3.0).abs() < 1e-12);
        assert!((sell_match_weight(&buy, &other_pay) - 1.5).abs() < 1e-12);
        assert!((sell_match_weight(&buy, &no_counter) - 1.5).abs() < 1e-12);
        assert_eq!(matching.priority, 1.5);
    }

    #[test]
    fn request_and_offer_without_counters_are_not_a_coincidence() {
        let buy = request(1, 10, 2.0, market_priority::POP_START);
        let sell = offer(2, 10, 2.0, 1.5);
        assert!((sell_match_weight(&buy, &sell) - 1.5).abs() < 1e-12);
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
    use crate::game::config::market_constants;
    use crate::game::factuals::Factuals;
    use crate::game::firm::{Firm, FirmPRow};
    use crate::game::good::Good;
    use crate::game::household::Household;
    use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
    use crate::game::sentiment::Sentiment;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    const GRAIN: usize = 1;
    const COIN: usize = 2;
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
        Factuals::new()
            .with_good(test_good(GRAIN, "grain"))
            .with_good(test_good(COIN, "coin"))
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

    fn shopper(id: usize, coin: f64, grain_shop: f64) -> Pop {
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
            .insert(GRAIN, PopPRow::new(0.0).with_target(grain_shop));
        pop
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
        pops.insert(1, shopper(1, 10.0, 4.0));
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
        assert!((pops[&1].property[&COIN].quantity - 6.0).abs() < 1e-12);
        assert!((firms[&1].property[&GRAIN].quantity - 6.0).abs() < 1e-12);
        assert!((firms[&1].property[&COIN].quantity - 4.0).abs() < 1e-12);
        assert!((firms[&1].property[&GRAIN].sold - 4.0).abs() < 1e-12);
        assert!(pops[&1].current_orders.is_empty());
        // Even AMV basket: no accept drift. Coin fully accepted: salability stays 1.
        assert!((market.goods[&GRAIN].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&COIN].salability - 1.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].salability - 0.5).abs() < 1e-12);
    }

    #[test]
    fn richer_pop_buys_first_when_supply_is_scarce() {
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

        assert!((pops[&1].property[&GRAIN].quantity - 2.0).abs() < 1e-12);
        assert!((pops[&2].property[&GRAIN].quantity).abs() < 1e-12);
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
        assert!(market.goods[&GRAIN].amv > 1.0);
        assert!(market.goods[&COIN].amv < 1.0);
        // Coin was tendered and never accepted.
        assert!(market.goods[&COIN].salability < 0.2);
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
    fn report_records_a_trade_and_leftover_sell() {
        let mut market = priced_market();
        market.pops.insert(1);
        market.firms.insert(1);

        let mut pops = HashMap::new();
        pops.insert(1, shopper(1, 10.0, 4.0));
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
        pops.insert(1, shopper(1, 10.0, 1.0));
        let mut firms = HashMap::new();
        firms.insert(1, farm(1, 10.0, 10.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        // 1 grain at 2.5 AMV ceils to 3 coin. Mid 2.75; grain rises, coin falls.
        assert!((pops[&1].property[&GRAIN].quantity - 1.0).abs() < 1e-12);
        let grain_amv = market.goods[&GRAIN].amv;
        let coin_amv = market.goods[&COIN].amv;
        assert!(grain_amv > 2.5);
        assert!(grain_amv < 2.75);
        assert!(coin_amv < 1.0);
        assert!(coin_amv > 0.9);
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

        // Coin fully accepted as payment: 0.5 -> lerp toward 1.0.
        let expected = lerp(0.5, 1.0, market_constants::SALABILITY_BLEND);
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
        firms.insert(1, farm(1, 10.0, 10.0));

        market.run_market_day(&factuals(), &mut pops, &mut firms, &mut rng());

        let grain = &market.goods[&GRAIN];
        let trail = grain.amv_trail();
        assert_eq!(trail.len(), 2);
        assert!((trail[0] - 2.5).abs() < 1e-12);
        assert!((trail[1] - grain.amv).abs() < 1e-12);
        assert!(trail[1] > trail[0]);

        let coin = &market.goods[&COIN];
        let trail = coin.amv_trail();
        assert_eq!(trail.len(), 2);
        assert!((trail[0] - 1.0).abs() < 1e-12);
        assert!((trail[1] - coin.amv).abs() < 1e-12);
        assert!(trail[1] < trail[0]);
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
        assert!((pops[&1].property[&CARGO].quantity).abs() < 1e-12);
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
            MarketGood::new().with_amv(1.0).with_salability(0.2),
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
}