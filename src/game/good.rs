use std::collections::{HashMap, HashSet};

use itertools::Itertools;

/// # Good
/// 
/// Goods are things that are bought, sold, and traded in the economy.
#[derive(Debug, Clone)]
pub struct Good {
    /// Unique ID of the good
    pub id: usize,
    /// The name of the good, should be unique.
    pub name: String,

    /// What class the good this is in.
    /// 
    /// This is another good which exists. it points to the 'ideal' example of the 
    /// class of good. Think generic bread vs wonder bread.
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

    /// The mass(kg) of the object, used as part of transportation, storage, and friction
    /// cost calculations.
    pub mass: f64,
    /// The volume (m^3) needed to store the object, used as part of transportation, 
    /// storage, and friction cost calculations.
    pub volume: f64,

    /// Tags which modify how the good is treated in markets.
    pub tags: HashSet<GoodTag>,

    /// Categories that a Good belongs to. A tool for searching, sorting, and refining 
    /// goods into various sections. For example, a bucket of desires should all be
    /// goods which share a primary category.
    pub categories: Vec<String>,
}
impl Good {
    pub fn is_buyable(&self) -> bool {
        !self.tags.iter().contains(&GoodTag::Untradeable)
    }

    /// # Bulk
    /// 
    /// Calculates the bulk of an item.
    /// Equal to mass + 400 * Volume.
    /// 
    /// This is meant to be scaled up or down to match friction scaling and so
    /// it may be added here later.
    pub fn bulk(&self) -> f64 {
        self.mass + self.volume * 400.0
    }
}

/// # Good Tag
/// 
/// Tags for goods.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum GoodTag {
    /// Good cannot be transported between markets.
    Fixed,
    /// Good only decays when unowned.
    Exposure,
    /// The good cannot be bought or sold.
    Untradeable,
}