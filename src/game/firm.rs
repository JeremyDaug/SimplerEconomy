use std::collections::HashMap;

use hexx::Hex;

use crate::game::market::Market;

/// # Firm 
/// 
/// A firm is the smallest unit of business. It deals with Production and local economic
/// calculation
#[derive(Debug, Clone)]
pub struct Firm {
    /// Unique Id for the Firm.
    pub id: usize,
    /// The unique name of the firm. If a child of another firm, this is it's 
    /// regional/sub name.
    pub name: String,

    /// Which market this firm is attached to and operating in.
    pub market: usize,
    /// The Specific Hex Tile the firm is centered in, for market splits and joins.
    pub hq: Hex,
    /// The Parent firm to this firm (if it has one).
    pub parent: Option<usize>,
    /// The Children Firm for 
    pub children: Vec<usize>,

    /// The Property owned by the firm. In some cases, this can be shared with the owner
    /// if it's an especially small business, but for most purposes, this is separate 
    /// and distinct.
    pub property: HashMap<usize, FirmPRow>,

    /// The details of the processes and work the firm will do.
    /// 
    /// Production lines are ordered by priority, those first in the list get run
    /// first. This should be noted for production lines that feed into each other.
    pub production_line: Vec<ProductionLine>,

    /// Information on who own's the firm, profits and risk distribution, and other
    /// such information.
    pub owners: Owners,
}

#[derive(Debug, Clone)]
pub struct Owners {
    
}

/// # Production Line
/// 
/// A Production line is a process and the information around it. This includes
/// targets, and input restrictions.
#[derive(Debug, Clone)]
pub struct ProductionLine {
    /// The process being run.
    pub process: usize,
    /// The target being sought. If None, then the firm wants to do as many as possible.
    pub target: Option<usize>,
    /// What goods are going to go into the process. Used to restrict optional inputs.
    pub inputs: Vec<usize>,
    /// A record of the average productivity (amv out / amv in) of the process.
    pub historical_productivity: f64,
}

/// # Firm Property Row
/// 
/// A row of property data for a Firm. Includes data for management, oversight, and 
/// targeting for both purchasing and use in production.
#[derive(Debug, Clone)]
pub struct FirmPRow {
    /// The amount currently owned.
    pub quantity: f64,
    /// The average ownership of the good over the last 30 days.
    pub rolling_average: f64,
    /// The target amount the firm wants to have after shopping.
    /// This should be before production processes.
    pub target: f64,
    /// If trading the good, this is the amount that the firm will never willingly 
    /// part with.
    pub reserve: f64,

    /// The average cost to get these good so far. Updated after each purchase and
    /// productive process.
    /// Used for value production efficiency calculations.
    pub average_cost: f64,
}