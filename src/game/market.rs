use std::collections::{HashMap, HashSet};

use bevy::utils::default;

use crate::game::{actors::Actors, factuals::Factuals};

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

    /// # History
    ///
    /// Snapshot of current AMVs for pop record keeping and sentiment wealth.
    /// Salability is not stored on [`MarketGood`] yet, so it stays empty
    /// (readers default missing salability to 1.0).
    pub fn history(&self) -> MarketHistory {
        let mut history = MarketHistory::new();
        for (&good_id, good) in &self.goods {
            history.prices.insert(good_id, good.amv);
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

    /// Salability for `good_id`, or 1.0 if missing.
    pub fn salability(&self, good_id: usize) -> f64 {
        self.salability.get(&good_id).copied().unwrap_or(1.0)
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

impl MarketGood {
    /// # Default
    /// 
    /// Gets the default market good, AMV defaults to 1.0,
    /// average price defaults to 1.0, and salability defaults to 0.6.
    /// 
    /// All others default to 0.0.
    pub fn default() -> Self {
        Self {
            amv: 1.0,
            salability: 0.6,
            average_price: 1.0,
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
        }
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
        market.goods.insert(5, MarketGood { amv: 3.0, ..MarketGood::default() });
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