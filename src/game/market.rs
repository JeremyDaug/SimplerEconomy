use std::collections::{HashMap, HashSet};

use super::pop::Pop;

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
    /// The goods in the market and records of them available to all.
    /// 
    /// If needed, this will have to be culled and cleaned out of old goods periodically.
    /// 
    /// The key is the ID of the good.
    pub goods: HashMap<usize, MarketGood>,
}

impl Market {
}

/// # Market History
/// 
/// A saved record of minimal data for passing around.
pub struct MarketHistory {
    /// The last known prices of the good in AMV.
    pub prices: HashMap<usize, f64>,
}

impl MarketHistory {
    pub(crate) fn new() -> Self {
        Self { 
            prices: HashMap::new(), 
        }
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