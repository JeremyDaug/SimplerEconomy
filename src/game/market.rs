use std::collections::{HashMap, HashSet};

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
    /// The current Abstract Market Value, an estimation of it's market value.
    pub amv: f64,

    // placeholder for AMV records.

    /// How many were made today.
    pub production: f64,
    /// How many were consumed today.
    pub consumption: f64,
    /// How many were brought in or out by traders. (Negative vaules means exports)
    pub imported: f64,
    /// How many of this good already existed in the market from yesterday.
    pub stock: f64,
    
    /* placeholder for exchange data. This data should include both how many times 
    it was exchanged, and how many were exchanged overall and at what 'concrete' prices 
    they were exchanged for.*/
}

impl MarketGood {
    /// # Default
    /// 
    /// Gets the default market good, AMV defaults to 1.0.
    pub fn default() -> Self {
        Self {
            amv: 1.0,
            production: 0.0,
            consumption: 0.0,
            imported: 0.0,
            stock: 0.0,
        }
    }
}