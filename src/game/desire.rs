
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
    pub scalar: DesireScalar,
}

impl Desire {
    /// # Tiers Satisfied
    /// 
    /// `self.satisfaciton` / `self.amount`, or the number of times it's been satisfied.
    pub fn tiers_satisfied(&self) -> f64 {
        self.satisfaction / self.amount
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
}

impl DesireTarget {
    pub fn new(good: usize, desire_type: DesireTargetType, eff: f64) -> Self {
        Self {
            good,
            desire_type,
            efficiency: eff,
            cap: 1.0
        }
    }
}

/// # Desire Scalar
/// 
/// When a base amount targeted by a desire is being scaled, what part of the pop
/// does it scale off of. 
#[derive(Debug, Clone)]
pub enum DesireScalar {
    /// Scales with all of the members of a house.
    All,
    /// Scales by household, not members.
    Household,
    /// Scales by adults only.
    Adults,
    /// Scaled by children only.
    Children,
    /// Scaled by Elders only.
    Elders,
    /// Scaled by the effective labor output of the household.
    Labor
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
/// Note: This is currently not comprehensive.
#[derive(Debug, Clone)]
pub enum DesireEffect {
    /// When this desire is **not** met, it reduces growth by this value.
    Mortality(f64),
    /// When this desire **is** met, it increases growth by this value.
    Birthrate(f64)
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