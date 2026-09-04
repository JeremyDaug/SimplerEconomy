use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

/// # Good
/// 
/// Goods are things that are bought, sold, and traded in the economy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Good {
    /// Unique ID of the good
    pub id: usize,
    /// The name of the good, should be unique.
    pub name: String,

    /// What class the good this is in.
    /// 
    /// This is another good which exists. it points to the 'ideal' example of the 
    /// class of good. Think generic bread vs wonder bread.
    #[serde(default)]
    pub class: Option<usize>,

    /// The rate that the good decays.
    /// 
    /// [0, 1]
    /// 
    /// Decay rate of 0, means no decay, decay of 1 means it always decays.
    #[serde(default)]
    pub decay_rate: f64,
    /// What the good decays into.
    /// 
    /// General Rule 1, what it decays into should be of similar mass .
    /// 
    /// General Rule 2, it should only decay into goods that are either indestructable
    /// or decay into nothing.
    #[serde(default)]
    pub decay_result: HashMap<usize, f64>,

    /// The mass(kg) of the object, used as part of transportation, storage, and friction
    /// cost calculations.
    pub mass: f64,
    /// The volume (m^3) needed to store the object, used as part of transportation, 
    /// storage, and friction cost calculations.
    pub volume: f64,

    /// Tags which modify how the good is treated in markets.
    #[serde(default)]
    pub tags: HashSet<GoodTag>,

    /// Categories that a Good belongs to. A tool for searching, sorting, and refining 
    /// goods into various sections. For example, a bucket of desires should all be
    /// goods which share a primary category.
    #[serde(default)]
    pub categories: Vec<String>,
}
impl Good {
    pub fn is_buyable(&self) -> bool {
        !self.tags.iter().contains(&GoodTag::Untradeable)
    }

    /// True if this good can pay intramarket transport / friction.
    pub fn is_transport(&self) -> bool {
        self.tags.iter().any(|tag| tag.transport_efficiency().is_some())
    }

    /// Friction cover per unit from the Transport tag, or 0.0 if none.
    pub fn transport_efficiency(&self) -> f64 {
        self.tags
            .iter()
            .find_map(|tag| tag.transport_efficiency())
            .unwrap_or(0.0)
    }

    /// Friction cover from `qty` units. 0 if this is not a transport good.
    pub fn transport_cover(&self, qty: f64) -> f64 {
        qty * self.transport_efficiency()
    }

    /// Sets the Transport tag to this efficiency, replacing any previous one.
    /// Must be `> 0.0`.
    pub fn with_transport_efficiency(mut self, efficiency: f64) -> Self {
        self.tags.retain(|tag| tag.transport_efficiency().is_none());
        self.tags.insert(GoodTag::transport(efficiency));
        self
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoodTag {
    /// Good cannot be transported between markets.
    Fixed,
    /// Good only decays when unowned.
    Exposure,
    /// The good cannot be bought or sold.
    Untradeable,
    /// Pays intramarket friction (time, cargo, shipping). The value is
    /// friction cover per unit (1.0 = time baseline). Spent by the buyer
    /// after a completed deal, and on a washed meeting for the flat fee.
    Transport(f64),
}

impl GoodTag {
    /// Transport tag with this efficiency. Must be `> 0.0`.
    pub fn transport(efficiency: f64) -> Self {
        debug_assert!(
            efficiency > 0.0 && efficiency.is_finite(),
            "transport efficiency must be > 0.0"
        );
        Self::Transport(efficiency)
    }

    /// Efficiency if this is a Transport tag.
    pub fn transport_efficiency(self) -> Option<f64> {
        match self {
            Self::Transport(efficiency) => Some(efficiency),
            _ => None,
        }
    }
}

impl PartialEq for GoodTag {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fixed, Self::Fixed)
            | (Self::Exposure, Self::Exposure)
            | (Self::Untradeable, Self::Untradeable) => true,
            (Self::Transport(a), Self::Transport(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

impl Eq for GoodTag {}

impl Hash for GoodTag {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        if let Self::Transport(efficiency) = self {
            efficiency.to_bits().hash(state);
        }
    }
}