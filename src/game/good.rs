use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

/// Foundational good id. Time is 0; other goods start at 1.
/// Exception to "0 means none" for good ids.
pub const TIME: usize = 0;

fn default_decay_rate() -> f64 {
    1.0
}

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
    /// Missing world-data keys default to 1.0 (full daily decay).
    #[serde(default = "default_decay_rate")]
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
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_class(mut self, class: Option<usize>) -> Self {
        self.class = class;
        self
    }

    pub fn with_decay_rate(mut self, decay_rate: f64) -> Self {
        debug_assert!(decay_rate >= 0.0 && decay_rate <= 1.0);
        self.decay_rate = decay_rate;
        self
    }

    pub fn with_decay_result(mut self, decay_result: HashMap<usize, f64>) -> Self {
        self.decay_result = decay_result;
        self
    }

    pub fn with_mass(mut self, mass: f64) -> Self {
        self.mass = mass;
        self
    }

    pub fn with_volume(mut self, volume: f64) -> Self {
        self.volume = volume;
        self
    }

    pub fn with_tags(mut self, tags: HashSet<GoodTag>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_categories(mut self, categories: Vec<String>) -> Self {
        self.categories = categories;
        self
    }

    /// Sets the Transport tag to this efficiency, replacing any previous one.
    /// Must be `> 0.0`.
    pub fn with_transport_efficiency(mut self, efficiency: f64) -> Self {
        self.set_transport_efficiency(efficiency);
        self
    }

    /// Sets the Transport tag to this efficiency, replacing any previous one.
    /// Must be `> 0.0`.
    pub fn set_transport_efficiency(&mut self, efficiency: f64) -> &mut Self {
        self.tags.retain(|tag| tag.transport_efficiency().is_none());
        self.tags.insert(GoodTag::transport(efficiency));
        self
    }

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

    /// # Durability
    /// 
    /// Calculates the durability of an item (1 - decay_rate).
    /// 
    /// Also used for remaining goods after decay.
    pub fn durability(&self) -> f64 {
        1.0 - self.decay_rate
    }

    /// # Bulk
    /// 
    /// Calculates the bulk of an item.
    /// Equal to mass + 400 * Volume.
    /// 
    /// This is meant to be scaled up or down to match friction scaling and so
    /// it may be added here later.
    /// 
    /// Bulk may be negative, but it cannot reduce the transportation cost of a 
    /// transaction below the flat friction cost.
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
    pub fn transport_efficiency(&self) -> Option<f64> {
        match self {
            Self::Transport(efficiency) => Some(*efficiency),
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
