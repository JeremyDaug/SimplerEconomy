
use crate::game::{pop::Pop, scalingfactor::ScalingFactor};

/// # Platonic Desire
/// 
/// Platonic Desires are the 'maximalist' form of pre-existing desires that can be
/// used by Cultures/Religions/Classes/Species to generate their own variants.
/// 
/// These should be defined at game start and never changed.
#[derive(Debug, Clone)]
pub struct PlatonicDesire {
    /// Unique ID of the desire. 0 Is reserved as a 'blank spot'.
    pub id: usize,
    /// What goods this desire seeks out, their efficiency, and how they satisfy.
    pub bucket: Vec<DesireTarget>,
    /// The effects produced by satisfying the desire.
    pub effects: Vec<DesireEffect>,
    /// What part of a household this scales with.
    pub scalar: ScalingFactor,
    /// The rate that satisfaction is converted into the end effect(s).
    /// 
    /// This always assumes that 1.0 satisfaction => 1.0 effects.
    pub effect_rate: DesireEffectRate,
    /// The rate of decay for this desire once satisfied.
    /// 0.0 means total decay between days, 1.0 would mean none. 
    /// Bounded between [0.0, 1.0), decay likely shouldn't go above 0.8 or so.
    pub decay: f64,
    /// What category(s) of goods should go into this desire.
    pub categories: Vec<String>,
    /// What class(es) of goods should go into this desire.
    pub classes: Vec<usize>,
    /// What Tiers this desire may go into. (Few go into 0, most will be 1 or 2)
    pub tiers: Vec<usize>,
    /// The Desire Sources which are valid for using this Platonic Desire.
    /// 
    /// The ID is ignored in this case, instead focusing on the determinent.
    pub users: Vec<DesireSource>
}

impl PlatonicDesire {
    /// Creates new Platonic Desire with the given id.
    /// 
    /// Bucket, Effects, Categories, Class, Tier, and User are all empty.
    /// Scalar is set to a factor of All(1.0)
    /// effect rate is set to Linear(1.0)
    /// decay is set to 0.0.
    pub fn new(id: usize) -> Self {
        PlatonicDesire {
            id,
            bucket: vec![],
            effects: vec![],
            scalar: ScalingFactor::All(1.0),
            effect_rate: DesireEffectRate::Linear(1.0),
            decay: 0.0,
            categories: vec![],
            classes: vec![],
            tiers: vec![],
            users: vec![],
        }
    }

    pub fn with_user(mut self, user: DesireSource) -> Self {
        self.users.push(user);
        self
    }

    pub fn with_tier(mut self, tier: usize) -> Self {
        assert!(tier < 3, "Tier must be 0, 1, or 2.");
        self.tiers.push(tier);
        self
    }

    pub fn with_class(mut self, class: usize) -> Self {
        self.classes.push(class);
        self
    }

    pub fn with_category(mut self, category: String) -> Self {
        self.categories.push(category);
        self
    }

    pub fn with_decay(mut self, decay: f64) -> Self {
        self.decay = decay;
        self
    }

    pub fn with_effect_rate(mut self, effect: DesireEffectRate) -> Self {
        self.effect_rate = effect;
        self
    }

    pub fn with_scalar(mut self, scalar: ScalingFactor) -> Self {
        self.scalar = scalar;
        self
    }

    pub fn with_good(mut self, target_good: DesireTarget) -> Self {
        self.bucket.push(target_good);
        self
    }

    pub fn with_effect(mut self, effect: DesireEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// # Create Empty Demo Desire
    /// 
    /// Creates and initializes a demographic desire based on this Platonic Desire.
    /// 
    /// Gives it the Platonic ID, base effects, scalar, and decay.
    /// 
    /// Sets priority to 1.
    pub fn create_empty_demo_desire(&self, id: usize, tier: usize) -> DemoDesire {
        debug_assert!(self.tiers.contains(&tier), 
            "Tier must be a valid tier for the Platonic Desire.");
        DemoDesire {
            id,
            platonic_id: self.id,
            bucket: vec![],
            effects: self.effects.clone(),
            amount: 1.0,
            scalar: self.scalar,
            decay: self.decay,
            tier,
            priority: 1,
        }
    }
}

/// # Demographic Desire
/// 
/// A desire as it exists in a Species, Culture, Class, or Religion.
/// 
/// This includes a targeted amount as though it were for a singular household.
/// 
/// This can be modified by players during the game. 
/// 
/// If a DemoDesire is 'new' and just to increase consumption, its `platonic_id` points 
/// to 0.
#[derive(Debug, Clone)]
pub struct DemoDesire {
    /// The ID of the Demographic Desire. Preferably unique to all demographics, but
    /// being unique to just the demographic which contains it should be good enough.
    pub id: usize,
    /// The ID of the Platonic Desire it is based off of. This should be a subset of the
    /// Platonic Desire. If 0, then it is not based on any Platonic Desire and thus is
    /// open ended.
    pub platonic_id: usize,
    /// The Bucket of goods which satisfy this desire, as well as how they are used and
    /// the efficiency they have in satisfying it.
    /// 
    /// This should be a subset of the Platonic Desire's Targets. Desire Targets should be
    /// equivalent in terms of details.
    pub bucket: Vec<DesireTarget>,
    /// The effects produced by satisfaction. This is scaled with the amount targeted and
    /// the rate defined by the Platonic Desire's Desire Effect Rate.
    /// 
    /// This is the result of meeting the amount fully.
    pub effects: Vec<DesireEffect>,
    /// The units of satisfaction needed to fully satisfy this desire.
    /// This is multiplied by the Scaling factor to produce the final target per whatever.
    /// 
    /// Effectively equivalent to 1 per Scalar.
    pub amount: f64,
    /// The Scaling Factor of the Desire. Multiply this value by the amount for the
    /// actual target amount.
    pub scalar: ScalingFactor,
    /// The rate of decay for the Desire. Should be directly inherited from the 
    /// Platonic Desire.
    pub decay: f64,
    /// The Desire Tier for the pop.
    pub tier: usize,
    /// The Priority of the desire in Demographic Tier it's in. Used for organization 
    /// both here and when consolidating into the pop at the end.
    pub priority: isize,
}

impl DemoDesire {
    /// # New
    /// 
    ///  Simple New, creates with no frills.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            platonic_id: 0,
            bucket: vec![],
            effects: vec![],
            amount: 1.0,
            scalar: ScalingFactor::Household(1.0),
            decay: 0.0,
            tier: 1,
            priority: 1,
        }
    }

    /// Sets the demographic desire's unique ID.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// Sets the platonic desire this demo desire is based on (0 if open-ended).
    pub fn with_platonic_id(mut self, platonic_id: usize) -> Self {
        self.platonic_id = platonic_id;
        self
    }

    /// Adds a good target to the satisfaction bucket.
    pub fn with_good(mut self, target_good: DesireTarget) -> Self {
        self.bucket.push(target_good);
        self
    }

    /// Adds an effect produced when this desire is satisfied.
    pub fn with_effect(mut self, effect: DesireEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// Sets the units of satisfaction needed to fully satisfy this desire.
    /// Debug-asserts that `amount` is positive.
    pub fn with_amount(mut self, amount: f64) -> Self {
        debug_assert!(amount > 0.0, "Amount must be positive.");
        self.amount = amount;
        self
    }

    /// Sets how the desire amount scales with the household/pop.
    pub fn with_scalar(mut self, scalar: ScalingFactor) -> Self {
        self.scalar = scalar;
        self
    }

    /// Sets the satisfaction decay rate between days.
    /// Debug-asserts that `decay` is in `[0.0, 1.0)`.
    pub fn with_decay(mut self, decay: f64) -> Self {
        debug_assert!(0.0 <= decay && decay < 1.0, "Decay must be between 0.0 inclusive and 1.0 exclusive.");
        self.decay = decay;
        self
    }

    /// Sets the desire tier for the pop.
    /// Debug-asserts that `tier` is 0, 1, or 2.
    pub fn with_tier(mut self, tier: usize) -> Self {
        debug_assert!(tier <= 2, "Tier must be between 0 and 1 inclusive.");
        self.tier = tier;
        self
    }

    /// Sets ordering priority within the demographic tier.
    pub fn with_priority(mut self, priority: isize) -> Self {
        self.priority = priority;
        self
    }

    /// # Create Desire
    /// 
    /// Creates a pop-level `Desire` from this demographic desire.
    /// 
    /// Copies bucket, effects, scalar, and decay. Satisfaction starts at 0.0 and
    /// category is left empty. `idx` is set to this demo desire's `id` so
    /// `Factuals::source_demo_desire` can resolve it later.
    /// 
    /// The target `amount` is this desire's base amount multiplied by the pop via
    /// `Pop::get_scaling_factor` and `self.scalar`.
    pub fn create_desire(&self, pop: &Pop, source: DesireSource) -> Desire {
        Desire {
            idx: self.id,
            source,
            target: self.bucket.clone(),
            amount: self.amount * pop.get_scaling_factor(self.scalar),
            satisfaction: 0.0,
            category: None,
            effect: self.effects.clone(),
            scalar: self.scalar,
            decay: self.decay,
        }
    }
}

/// # Desire Effect Rate
/// 
/// As a desire's target amount increases, the effects/benefits it recieves alter as 
/// well.
/// 
/// This effect alters the bonuses given to a pop by satisfying a desire, however pops
/// will always linearly connect between 0 and the bonus effect given by the Culture.
/// They will not follow the curve.
#[derive(Debug, Clone, Copy)]
pub enum DesireEffectRate {
    /// (input - 1.0) * v + 1.0
    /// Ensures that at 1, we get 1 of the effect.
    /// Value must be positive.
    Linear(f64),
    /// input.sqrt()
    SqareRoot,
    /// Log_v (input) + 1.0
    /// Ensures that 1.0 gives 1.0 effect.
    /// Value must be a valid basis for a log.
    Logarithmic(f64),
}

impl DesireEffectRate {
    /// Safe Linear Maker. Ensures values are positive. Panics Otherwise.
    pub fn linear(v: f64) -> Self {
        assert!(v > 0.0, "V must be a Positive Value.");
        Self::Linear(v)
    }

    /// Safe Logarithmic maker. Ensures values are > 1.0. Panic otherwise.
    pub fn logarithmic(v: f64) -> Self {
        assert!(v > 1.0, "V must be greater that 1.0.");
        Self::Logarithmic(v)
    }

    /// # Calculate
    /// 
    /// Given an input value `v`, it calculates what the desire's effect rate is.
    /// 
    /// v must be >= 1.0. V should never be below 1.0.
    pub fn calculate(&self, v: f64) -> f64 {
        assert!(v >= 1.0, "Input value must be 1.0 or greater.");
        match self {
            DesireEffectRate::Linear(c) => (v - 1.0) * c + 1.0,
            DesireEffectRate::SqareRoot => v.sqrt(),
            DesireEffectRate::Logarithmic(b) => v.log(*b),
        }
    }
}

/// # Desire
/// 
/// A Desire is things or groups of things that are desired by a pop.
#[derive(Debug, Clone)]
pub struct Desire {
    /// Links back to the source `DemoDesire.id` (set by `DemoDesire::create_desire`).
    /// 
    /// May also be used for ordering when desires are rearranged.
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

/// # Desire Target Type
/// 
/// What kind of desire the target is. 
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
#[derive(Debug, Clone, Copy)]
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
    /// Desire is sourced from a class.
    /// 
    /// TODO: Class demographics / desires are not implemented yet.
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