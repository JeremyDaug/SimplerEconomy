use crate::game::scalingfactor::ScalingFactor;


/// # Desire
/// 
/// A Desire is things or groups of things that are desired by a pop.
/// 
/// 
#[derive(Debug, Clone)]
pub struct Desire {
    /// An index value, for keeping desires order. 
    /// 
    /// This is set/reset when desires are added/rearranged, or
    pub idx: usize,

    /// Useful Identifier which points back to where this desire comes from.
    pub source: DesireSource,

    /// The goods beings desired. If of length 1, then it's a specific good,
    /// if it's multiple, then it's a bucket.
    pub target: Vec<DesireTarget>,

    /// The amount of units needed. Must always be a positive value.
    pub amount: f64,
    /// The current satisfaction of the desire in units. Does not differentiate goods.
    pub satisfaction: f64,

    /// Desires should have a category of good they are restricted to expanding into.
    pub category: Option<String>,

    /// The effects (typically one or none) which are generated when a desire is either
    /// satisfied (for bonuses) or unsatisfied (for maluses).
    pub effect: Vec<DesireEffect>,

    /// A Desire's Scalar is the factor by which the base amount of a desire is scaled
    /// by to match the pop and it's household(s).
    /// 
    /// In particular, it represents selecting either everyone, a member of the 
    /// household, or it is on a 'per house' basis.
    pub scalar: ScalingFactor,

    /// The rate at which the satisfaction of a desire over time.
    /// 
    /// The satisfaction is multiplied by this value each turn.
    /// 
    /// This should be within [0.0, 1.0).
    /// 
    /// 0.0 means the desire is reset each day.
    /// 
    /// It should never be 1.0, as that would mean the desire never get's unsatisfied.
    pub decay: f64,
}

impl Desire {
    /// # Tiers Satisfied
    /// 
    /// `self.satisfaciton` / `self.amount`, or the number of times it's been satisfied.
    pub fn tiers_satisfied(&self) -> f64 {
        self.satisfaction / self.amount
    }

    /// # Ordered Targets
    /// 
    /// Organizes the target goods in the bucket, putting high priority goods first, 
    /// then by efficiency.
    pub fn ordered_targets(&self) -> Vec<&DesireTarget> {
        let mut targets = self.target.iter().collect::<Vec<&DesireTarget>>();
        targets.sort_by(|a, b| {
            if a.high_priority && !b.high_priority {
                std::cmp::Ordering::Less
            } else if !a.high_priority && b.high_priority {
                std::cmp::Ordering::Greater
            } else {
                b.efficiency.partial_cmp(&a.efficiency).unwrap_or(std::cmp::Ordering::Equal)
            }
        });
        targets
    }
}

/// # Desire Target 
/// 
/// A good and the efficiency of that good at satisfying our desires.
#[derive(Debug, Clone)]
pub struct DesireTarget {
    /// Whe ID of the good which can satisfy this desire.
    pub good: usize,
    /// Whether the desire is Consumed or Used by the pop.
    pub desire_type: DesireTargetType,
    /// The efficiency at which the good can satisfy our desire.
    /// Units * effenciency = satisfaction.
    pub efficiency: f64,
    /// What proportion of the wider desire can be satisfied by this specific good.
    /// 
    /// Should be between 0.0 and 1.0 and should not go below 1 / number of goods, for 
    /// the desire.
    pub cap: f64,
    /// Whether this desire target is high priority and is always satisfied first.
    pub high_priority: bool,
}

impl DesireTarget {
    pub fn new(good: usize, desire_type: DesireTargetType, eff: f64) -> Self {
        Self {
            good,
            desire_type,
            efficiency: eff,
            cap: 1.0,
            high_priority: false,
        }
    }

    pub fn with_cap(mut self, cap: f64) -> Self {
        self.cap = cap;
        self
    }

    pub fn with_high_priority(mut self, high_priority: bool) -> Self {
        self.high_priority = high_priority;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DesireTargetType {
    /// The Desire is Consumed, producing the output goods of the good's decay.
    Consume,
    /// The Desire is to be used, not consumed. It is not decayed/destroyed, but instead used.
    /// Currently, Use goods have no time cost attached. That may change, but not just yet.
    Use,
}

/// # Desire Effect
/// 
/// When the condition of the effect is met (satisfaction or lack thereof) the effect
/// is generated and applied to the pop who owns the desire.
/// 
/// Most effects define the rate they apply to the pop, and a bool which defines how they
/// operate. True if the effect is treated as a 'bonus' and false if it is treated as a 
/// 'malus'. Bonuses scale positively with satisfaction, malus's with the lack of satisfaction.
/// 
/// As a note, Luxury desires, due to their infinite nature, should not have malus effects.
/// 
/// Note: This is currently not comprehensive.
#[derive(Debug, Clone)]
pub enum DesireEffect {
    /// When this desire is **not** met, it reduces growth by this value.
    Mortality(f64, bool),
    /// When this desire **is** met, it increases growth by this value.
    Birthrate(f64, bool),
    /// An additional good which is added to the pop's inventory.
    /// 
    /// Useful for things like transportation.
    BonusGood(usize, f64, bool),
}

/// # Desire Source
/// 
/// Where is the desire's definition derived from.
#[derive(Debug, Clone, Copy)]
pub enum DesireSource {
    /// Desire is sourced from the pop's biological needs.
    Species(usize),
    /// Desire is sourced from a Culture.
    Culture(usize),
    /// Desire is sourced from a class (Not currently used).
    Class(usize),
    /// Desire is sourced from a religion.
    Religion(usize),
}

impl DesireSource {
    /// # Unwrap
    /// 
    /// Gets the ID of the Desire Source.
    pub fn unwrap(&self) -> &usize {
        match self {
            DesireSource::Species(id) |
            DesireSource::Culture(id) |
            DesireSource::Class(id) |
            DesireSource::Religion(id) => id,
        }
    }
}