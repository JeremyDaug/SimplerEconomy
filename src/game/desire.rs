
use std::debug_assert;

use itertools::Itertools;

use crate::game::{pop::Pop, scalingfactor::ScalingFactor};

pub use crate::game::effects::DesireEffect;

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
    /// 
    /// Currently not in use.
    pub categories: Vec<String>,
    /// What class(es) of goods should go into this desire.
    pub classes: Vec<usize>,
    /// What Tiers this desire may go into. (Few go into 0, most will be 1 or 2)
    /// 
    /// This should Never be empty.
    pub tiers: Vec<usize>,
    /// The Desire Sources which are valid for using this Platonic Desire.
    /// 
    /// The ID is ignored in this case, instead focusing on the determinent.
    pub users: Vec<DesireSource>
}

impl PlatonicDesire {
    /// Creates new Platonic Desire with the given id.
    /// 
    /// Bucket, Effects, Categories, Class, and User are all empty.
    /// Scalar is set to a factor of All(1.0)
    /// effect rate is set to Linear(1.0)
    /// decay is set to 0.0.
    /// Tier is set to 1, as a default. Tier should never be empty.
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
            category: "".into(),
            priority: 1,
        }
    }

    /// # Derive Demographic Desire
    pub fn derive_demographic_desire(&self, id: usize, tier: usize, amount: f64,
        desire_targets: Vec<usize>, priority: isize) -> DemoDesire {
        debug_assert!(self.tiers.contains(&tier),
            "Tier must be a valid tier for the Platonic Desire.");
        // This is gross, rework this if possible.
        let first_desire = self.bucket.iter()
            .filter(|x| desire_targets.contains(&x.good))
            .map(|x| x.clone())
            .collect_vec();
        let effect_scale = self.effect_rate.calculate(amount);
        let effects = self.effects.iter()
            .map(|x| {
                x.scale_by(effect_scale)
            }).collect_vec();
        DemoDesire {
            id,
            platonic_id: self.id,
            bucket: first_desire,
            effects,
            amount,
            scalar: self.scalar,
            decay: self.decay,
            tier,
            category: "".into(),
            priority,
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
    /// This should be a subset of the Platonic Desire's Targets goods.
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
    /// The category the desire is restricting itself to.
    pub category: String,
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
            category: "".into(),
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
        debug_assert!(tier <= 2, "Tier must be between 0 and 2 inclusive.");
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
    /// category is left empty. `source` is stored with this demo desire's `id`
    /// filled in (second field of `DesireSource`) so lookups can resolve it later.
    /// 
    /// The target `amount` is this desire's base amount multiplied by the pop via
    /// `Pop::get_scaling_factor` and `self.scalar`.
    pub fn create_desire(&self, pop: &Pop, source: DesireSource) -> Desire {
        Desire {
            source: source.with_demo_desire_id(self.id),
            priority: self.priority,
            target: self.bucket.clone(),
            amount: self.amount * pop.get_scaling_factor(self.scalar),
            satisfaction: 0.0,
            category: None,
            effect: self.effects.clone(),
            scalar: self.scalar,
            decay: self.decay,
        }
    }

    /// # Derive Desire
    /// 
    /// Given the current demographic desire, create a desire for a pop and adjust it 
    /// to them apropriately.
    /// 
    /// The amount on the output should be scaled to match the pop, and the effect
    /// should scale apropriately as well to ensure it outputs the correct amount as a
    /// percent of total success (satisfaction / amount)
    /// 
    /// Does not set the satisfaction.
    ///
    /// Additive effects (culture, research, faith, authority, legitimacy, bonus goods)
    /// are multiplied by the same pop scale as amount. Birth, mortality, sentiment,
    /// and satisfaction arms are left as demo rates / percents.
    pub fn derive_desire(&self, source: DesireSource, pop: &Pop) -> Desire {
        // amount per unit times that number in the pop.
        let scale = pop.get_scaling_factor(self.scalar);
        let amount = self.amount * scale;
        // Additive arms (player resources, bonus goods) are per-scalar on the demo
        // and must grow with household/adult/etc. count. Rate and percent arms stay.
        let effect = self
            .effects
            .iter()
            .map(|e| {
                if e.is_additive() {
                    e.scale_by(scale)
                } else {
                    *e
                }
            })
            .collect();
        Desire {
            source: source.with_demo_desire_id(self.id),
            priority: self.priority,
            target: self.bucket.clone(),
            amount,
            satisfaction: 0.0,
            category: None,
            effect,
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
    SquareRoot,
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
            DesireEffectRate::SquareRoot => v.sqrt(),
            DesireEffectRate::Logarithmic(b) => v.log(*b),
        }
    }
}

/// # Desire
/// 
/// A Desire is things or groups of things that are desired by a pop.
/// 
/// ## Contracts
/// 
/// Teh ordering of DesireTarget in here should be in the same order as the 
#[derive(Debug, Clone)]
pub struct Desire {
    /// Where this desire comes from, including the source demographic id and the
    /// linked `DemoDesire.id` (see `DesireSource`).
    pub source: DesireSource,

    /// Ordering priority within a tier. Lower values come first when sorting.
    /// 
    /// Lifecycle under `Pop::update_desires`:
    /// 1. Set from the parent `DemoDesire.priority` for placement sorting.
    /// 2. After the tier is sorted, rewritten to the desire's index in that tier.
    /// 
    /// Once baked to index, later re-sorts can use `priority` alone without re-reading
    /// `DesireSource`.
    pub priority: isize,

    /// The goods beings desired. If of length 1, then it's a specific good,
    /// if it's multiple, then it's a bucket.
    pub target: Vec<DesireTarget>,

    /// The amount of units needed. Must always be a positive value and should always 
    /// be >= 1.0.
    pub amount: f64,
    /// The current satisfaction of the desire in units. Does not differentiate goods.
    pub satisfaction: f64,

    /// Desires should have a category of good they are restricted to expanding into.
    /// 
    /// Not Currently Used.
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

    /// # Cmp Order
    /// 
    /// Orders one desire relative to another within a tier.
    /// 
    /// Used during `update_desires` **after** priorities have been loaded from each
    /// parent `DemoDesire`, and for any later re-sort once priorities have been baked
    /// to tier indices (then `priority` alone decides order).
    /// 
    /// 1. `priority` ascending (lower first) — demo priority, or post-update index
    /// 2. source kind: Species → Culture → Class → Religion (tie-break only)
    /// 3. demo desire id ascending (tie-break only)
    /// 
    /// Provisional; may be reworked after playtesting.
    pub fn cmp_order(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
            .then_with(|| self.source.order_rank().cmp(&other.source.order_rank()))
            .then_with(|| self.source.demo_desire_id().cmp(&other.source.demo_desire_id()))
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
    /// 
    /// This should be unique within a bucket.
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
    /// Creates a target. Efficiency must be positive (`> 0`).
    pub fn new(good: usize, desire_type: DesireTargetType, eff: f64) -> Self {
        debug_assert!(eff > 0.0, "Desire target efficiency must be positive");
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

/// # Desire Source
/// 
/// Where is the desire's definition derived from.
/// 
/// Each variant is `(source_id, demo_desire_id)`:
/// - `source_id`: Species / Culture / Class / Religion id
/// - `demo_desire_id`: id of the `DemoDesire` within that demographic
/// 
/// For platonic / user lists that only care about the determinant, `demo_desire_id`
/// may be `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesireSource {
    /// Desire is sourced from the pop's biological needs. `(species_id, demo_desire_id)`
    Species(usize, usize),
    /// Desire is sourced from a Culture. `(culture_id, demo_desire_id)`
    Culture(usize, usize),
    /// Desire is sourced from a class. `(class_id, demo_desire_id)`
    /// 
    /// TODO: Class demographics / desires are not implemented yet.
    Class(usize, usize),
    /// Desire is sourced from a religion. `(religion_id, demo_desire_id)`
    Religion(usize, usize),
}

impl DesireSource {
    /// # Desire Source ID
    /// 
    /// Gets the demographic source id (species/culture/class/religion).
    pub fn desire_source_id(&self) -> &usize {
        match self {
            DesireSource::Species(id, _) |
            DesireSource::Culture(id, _) |
            DesireSource::Class(id, _) |
            DesireSource::Religion(id, _) => id,
        }
    }

    /// Gets the linked `DemoDesire.id`.
    pub fn demo_desire_id(&self) -> &usize {
        match self {
            DesireSource::Species(_, id) |
            DesireSource::Culture(_, id) |
            DesireSource::Class(_, id) |
            DesireSource::Religion(_, id) => id,
        }
    }

    /// Returns a copy of this source with the demo desire id set.
    pub fn with_demo_desire_id(self, demo_desire_id: usize) -> Self {
        match self {
            DesireSource::Species(source_id, _) => DesireSource::Species(source_id, demo_desire_id),
            DesireSource::Culture(source_id, _) => DesireSource::Culture(source_id, demo_desire_id),
            DesireSource::Class(source_id, _) => DesireSource::Class(source_id, demo_desire_id),
            DesireSource::Religion(source_id, _) => DesireSource::Religion(source_id, demo_desire_id),
        }
    }

    /// # Order Rank
    /// 
    /// Sort key for desire ordering: Species → Culture → Class → Religion.
    pub fn order_rank(&self) -> u8 {
        match self {
            DesireSource::Species(_, _) => 0,
            DesireSource::Culture(_, _) => 1,
            DesireSource::Class(_, _) => 2,
            DesireSource::Religion(_, _) => 3,
        }
    }
}