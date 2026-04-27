use std::collections::{HashMap, HashSet};

/// # Good
/// 
/// Goods are things that are bought, sold, and traded in the economy.
/// 
#[derive(Debug, Clone)]
pub struct Good {
    /// Unique ID of the good
    pub id: usize,
    /// The name of the good, should be unique.
    pub name: String,

    /// What class the good this is in.
    pub class: Option<usize>,

    /// The rate that the good decays.
    /// 
    /// [0, 1]
    /// 
    /// Decay rate of 0, means no decay, decay of 1 means it always decays.
    pub decay_rate: f64,
    /// What the good decays into.
    /// 
    /// General Rule 1, what it decays into should be of similar mass .
    /// 
    /// General Rule 2, it should only decay into goods that are either indestructable
    /// or decay into nothing.
    pub decay_result: HashMap<usize, f64>,

    /// Tags which modify how the good is treated in markets.
    pub tags: HashSet<GoodTag>,
}

/// # Good Tag
/// 
/// Tags for goods.
#[derive(Debug, Clone, Copy, Hash)]
pub enum GoodTag {
    /// Good cannot be transported between markets.
    Fixed,
    /// Good only decays when unowned.
    Exposure,
    /// The good cannot be bought or sold.
    Untradeable,
}