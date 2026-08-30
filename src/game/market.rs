use std::collections::{HashMap, HashSet};

use rand::Rng;
use rand::seq::SliceRandom;

use crate::game::actor::Actor;
use crate::game::config::{market_constants, market_priority};
use crate::game::firm::Firm;
use crate::game::marketorder::MarketOrder;
use crate::game::pop::Pop;
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
}

impl Market {
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
    /// Runs the market's day with 3 primary steps.
    /// 
    /// 1. Collect orders, go through each actor (Pop, Firm, Institution, State) and 
    /// have them create their market orders for the day. Sorting into purchase and selling orders.
    /// 2. Match Loop
    ///   1. Find a match between buyers and sellers as well as unsuccessful buyers/sellers.
    /// 3. 
    pub fn run_market_day(&mut self, factuals: &Factuals, 
        pops: &mut Vec<Pop>,
        firms: &mut Vec<Firm>, 
        // placeholder note possibly for later including institutions and states.
    ) {
        
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
    /// How many units changed hands (were bought and sold) today.
    /// Does **NOT** include imports and exports, only local movement.
    /// Total Volume = volume + |imported|
    /// Volume = Purchased + Sold
    pub volume: f64,

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
            production: 0.0,
            consumption: 0.0,
            imported: 0.0,
            stock: 0.0,
            supply: 0.0,
            suppliers: 0.0,
            demand: 0.0,
            buyers: 0.0,
            volume: 0.0,
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

    /// Sets units that changed hands locally today.
    /// Must be `>= 0.0`.
    pub fn set_volume(&mut self, volume: f64) {
        debug_assert!(volume >= 0.0, "volume must be >= 0.0");
        self.volume = volume;
    }

    /// Sets units that changed hands locally today.
    /// Must be `>= 0.0`.
    pub fn with_volume(mut self, volume: f64) -> Self {
        self.set_volume(volume);
        self
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
        assert_eq!(good.volume, 0.0);
        assert_eq!(good.requests, 0.0);
        assert_eq!(good.purchased, 0.0);
        assert_eq!(good.tender, 0.0);
        assert_eq!(good.payment, 0.0);
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
            .with_volume(2.0)
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
        assert_eq!(good.volume, 2.0);
        assert_eq!(good.requests, 5.0);
        assert_eq!(good.purchased, 2.0);
        assert_eq!(good.tender, 6.0);
        assert_eq!(good.payment, 4.0);
        assert_eq!(good.average_price, 1.5);
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