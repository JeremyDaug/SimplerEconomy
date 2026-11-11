#![allow(dead_code)]

use crate::game::config::PopConfig;
use crate::game::demographic_source::DemographicSource;
use crate::game::pop::Pop;
use crate::game::scalingfactor::ScalingFactor;

pub use crate::game::effects::DesireEffect;

/// # Platonic Desire
///
/// Platonic Desires are the 'maximalist' form of pre-existing desires that can be
/// used by Cultures, Religions, Strata, and Species to generate their own variants.
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
    /// Fraction of satisfaction lost each day.
    ///
    /// 0.0 keeps the satisfaction. 1.0 clears it. Same scale as a good's
    /// decay rate. In `[0.0, 1.0]`.
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
    /// The demographics which may use this platonic desire.
    pub users: Vec<DemographicSource>,
}

/// # Demographic Desire
///
/// A desire as it exists in a Species, Culture, Stratum, or Religion.
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
    /// [`Self::set_amount`] rewrites it from the platonic magnitudes.
    pub effects: Vec<DesireEffect>,
    /// Platonic effect magnitudes, before the amount curve.
    base_effects: Vec<DesireEffect>,
    /// Curve copied from the platonic desire. [`Self::set_amount`] uses it.
    pub effect_rate: DesireEffectRate,
    /// The units of satisfaction needed to fully satisfy this desire.
    /// This is multiplied by the Scaling factor to produce the final target per whatever.
    ///
    /// Effectively equivalent to 1 per Scalar.
    ///
    /// Change it through [`Self::set_amount`] so `effects` stay matched to it.
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

/// # Desire
///
/// A Desire is things or groups of things that are desired by a pop.
#[derive(Debug, Clone)]
pub struct Desire {
    /// The demographic this desire comes from.
    pub source: DemographicSource,

    /// Id of the `DemoDesire` this was created from.
    pub demo_desire_id: usize,

    /// Ordering priority within a tier. Lower values come first when sorting.
    ///
    /// Lifecycle under `Pop::update_desires`:
    /// 1. Set from the parent `DemoDesire.priority` for placement sorting.
    /// 2. After the tier is sorted, rewritten to the desire's index in that tier.
    ///
    /// Once baked to index, later re-sorts can use `priority` alone without re-reading
    /// the source or [`Self::demo_desire_id`].
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

    /// Fraction of satisfaction lost each day. Copied from the demographic
    /// desire.
    ///
    /// Satisfaction kept is multiplied by `1.0 - decay`.
    pub decay: f64,
}

/// # Desire Target
///
/// A good and the efficiency of that good at satisfying our desires.
#[derive(Debug, Clone)]
pub struct DesireTarget {
    /// Whe ID of the good which can satisfy this desire.
    ///
    /// This should be unique within a bucket.
    /// TODO: Edit this to allow for good categories instead of just specific goods.
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
    /// The minimum proportion of the wider desire that can be satisfied by this specific good.
    pub min: f64,
    /// Whether this desire target is high priority and is always satisfied first.
    pub high_priority: bool,
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

impl PlatonicDesire {
    /// # New
    ///
    /// Creates a platonic desire with the given id.
    ///
    /// Bucket, effects, categories, class, and user are all empty.
    /// Scalar is [`ScalingFactor::All`] of 1.0.
    /// Effect rate is linear at 1.0.
    /// Decay is 0.0.
    /// Tier starts at 1. Tier should never be empty.
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

    /// # With User
    ///
    /// Appends a demographic that may use this desire.
    pub fn with_user(mut self, user: DemographicSource) -> Self {
        self.users.push(user);
        self
    }

    /// # With Tier
    ///
    /// Appends a tier. Panics unless `tier` is 0, 1, or 2.
    pub fn with_tier(mut self, tier: usize) -> Self {
        assert!(tier < 3, "Tier must be 0, 1, or 2.");
        self.tiers.push(tier);
        self
    }

    /// # With Class
    ///
    /// Appends a good class this desire may draw.
    pub fn with_class(mut self, class: usize) -> Self {
        self.classes.push(class);
        self
    }

    /// # With Category
    ///
    /// Appends a good category this desire may draw.
    pub fn with_category(mut self, category: String) -> Self {
        self.categories.push(category);
        self
    }

    /// # With Decay
    ///
    /// Sets the satisfaction decay.
    pub fn with_decay(mut self, decay: f64) -> Self {
        self.decay = decay;
        self
    }

    /// # With Effect Rate
    ///
    /// Sets the curve from amount to effect.
    pub fn with_effect_rate(mut self, effect: DesireEffectRate) -> Self {
        self.effect_rate = effect;
        self
    }

    /// # With Scalar
    ///
    /// Sets which part of the household the amount scales with.
    pub fn with_scalar(mut self, scalar: ScalingFactor) -> Self {
        self.scalar = scalar;
        self
    }

    /// # With Good
    ///
    /// Appends a satisfaction target.
    pub fn with_good(mut self, target_good: DesireTarget) -> Self {
        self.bucket.push(target_good);
        self
    }

    /// # With Effect
    ///
    /// Appends one platonic effect.
    pub fn with_effect(mut self, effect: DesireEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// # Create Empty Demo Desire
    ///
    /// Creates a demographic desire from this platonic desire with an empty bucket.
    ///
    /// `id` is the new desire's id. `tier` must be one this platonic desire
    /// lists. Priority is 1. Scalar, decay, and the effect rate are copied.
    /// Amount starts at 1, and the platonic effects are scaled to that amount.
    pub fn create_empty_demo_desire(&self, id: usize, tier: usize) -> DemoDesire {
        debug_assert!(
            self.tiers.contains(&tier),
            "Tier must be a valid tier for the Platonic Desire."
        );
        let mut demo = DemoDesire {
            id,
            platonic_id: self.id,
            bucket: vec![],
            effects: vec![],
            base_effects: self.effects.clone(),
            effect_rate: self.effect_rate,
            amount: 1.0,
            scalar: self.scalar,
            decay: self.decay,
            tier,
            category: "".into(),
            priority: 1,
        };
        demo.scale_effects_to_amount();
        demo
    }

    /// # Derive Demographic Desire
    ///
    /// Builds a demographic desire from this platonic desire.
    ///
    /// `id` is the new desire's id. `tier` must be one this platonic desire
    /// lists. `amount` is the satisfaction target and must be `>= 1.0`.
    /// `desire_targets` keeps bucket goods whose id is in that list.
    /// `priority` is stored on the result. Scalar, decay, and the effect rate
    /// are copied. [`DemoDesire::set_amount`] then writes `amount` and the effects.
    pub fn derive_demographic_desire(
        &self,
        id: usize,
        tier: usize,
        amount: f64,
        desire_targets: Vec<usize>,
        priority: isize,
    ) -> DemoDesire {
        debug_assert!(
            self.tiers.contains(&tier),
            "Tier must be a valid tier for the Platonic Desire."
        );
        // Keep only the platonic targets this demo selected.
        let first_desire = self
            .bucket
            .iter()
            .filter(|target| desire_targets.contains(&target.good))
            .cloned()
            .collect();
        let mut demo = DemoDesire {
            id,
            platonic_id: self.id,
            bucket: first_desire,
            effects: vec![],
            base_effects: self.effects.clone(),
            effect_rate: self.effect_rate,
            amount: 1.0,
            scalar: self.scalar,
            decay: self.decay,
            tier,
            category: "".into(),
            priority,
        };
        demo.set_amount(amount);
        demo
    }
}

impl DemoDesire {
    /// # New
    ///
    /// A demographic desire with this id and no goods or effects.
    ///
    /// Amount starts at 1. Scalar is one household. Tier and priority start at 1.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            platonic_id: 0,
            bucket: vec![],
            effects: vec![],
            base_effects: vec![],
            effect_rate: DesireEffectRate::Linear(1.0),
            amount: 1.0,
            scalar: ScalingFactor::Household(1.0),
            decay: 0.0,
            tier: 1,
            category: "".into(),
            priority: 1,
        }
    }

    /// # With Id
    ///
    /// Sets the demographic desire's unique ID.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// # With Platonic Id
    ///
    /// Sets the platonic desire this demo desire is based on (0 if open-ended).
    pub fn with_platonic_id(mut self, platonic_id: usize) -> Self {
        self.platonic_id = platonic_id;
        self
    }

    /// # With Good
    ///
    /// Adds a good target to the satisfaction bucket.
    pub fn with_good(mut self, target_good: DesireTarget) -> Self {
        self.bucket.push(target_good);
        self
    }

    /// # With Effect
    ///
    /// Adds `effect` at its platonic magnitude and rescales `effects` to the
    /// current amount.
    pub fn with_effect(mut self, effect: DesireEffect) -> Self {
        self.base_effects.push(effect);
        self.scale_effects_to_amount();
        self
    }

    /// # With Effect Rate
    ///
    /// Sets the curve and rescales `effects` to the current amount.
    pub fn with_effect_rate(mut self, effect_rate: DesireEffectRate) -> Self {
        self.effect_rate = effect_rate;
        self.scale_effects_to_amount();
        self
    }

    /// # With Amount
    ///
    /// Sets the satisfaction target through [`Self::set_amount`].
    pub fn with_amount(mut self, amount: f64) -> Self {
        self.set_amount(amount);
        self
    }

    /// # Set Amount
    ///
    /// Sets `amount` and rescales `effects` to that amount.
    ///
    /// `amount` is the satisfaction needed to fully meet this desire. It must
    /// be `>= 1.0`. A higher amount yields a higher effect. Scalar, decay,
    /// and the bucket stay.
    pub fn set_amount(&mut self, amount: f64) {
        debug_assert!(amount >= 1.0, "Amount must be 1.0 or greater.");
        self.amount = amount;
        self.scale_effects_to_amount();
    }

    /// # Scale Effects To Amount
    ///
    /// Rewrites `effects` from `base_effects` by [`DesireEffectRate::calculate`]
    /// of the current `amount`.
    fn scale_effects_to_amount(&mut self) {
        let scale = self.effect_rate.calculate(self.amount);
        self.effects = self
            .base_effects
            .iter()
            .map(|effect| effect.scale_by(scale))
            .collect();
    }

    /// # With Scalar
    ///
    /// Sets how the desire amount scales with the household or pop.
    pub fn with_scalar(mut self, scalar: ScalingFactor) -> Self {
        self.scalar = scalar;
        self
    }

    /// # With Decay
    ///
    /// Sets the satisfaction decay rate between days.
    /// Debug-asserts that `decay` is in `[0.0, 1.0]`.
    pub fn with_decay(mut self, decay: f64) -> Self {
        debug_assert!(
            0.0 <= decay && decay <= 1.0,
            "Decay must be between 0.0 and 1.0 inclusive."
        );
        self.decay = decay;
        self
    }

    /// # With Tier
    ///
    /// Sets the desire tier for the pop.
    /// Debug-asserts that `tier` is 0, 1, or 2.
    pub fn with_tier(mut self, tier: usize) -> Self {
        debug_assert!(tier <= 2, "Tier must be between 0 and 2 inclusive.");
        self.tier = tier;
        self
    }

    /// # With Priority
    ///
    /// Sets ordering priority within the demographic tier.
    pub fn with_priority(mut self, priority: isize) -> Self {
        self.priority = priority;
        self
    }

    /// # Scaled Effects
    ///
    /// Effects for a pop-sized copy of this desire.
    ///
    /// `scale` multiplies additive arms. Rate and percent arms stay at the
    /// demographic magnitudes. Returns that list.
    pub(crate) fn scaled_effects(&self, scale: f64) -> Vec<DesireEffect> {
        self.effects
            .iter()
            .map(|effect| {
                if effect.is_additive() {
                    effect.scale_by(scale)
                } else {
                    *effect
                }
            })
            .collect()
    }

    /// # Create Desire
    ///
    /// A pop-level desire copied from this demographic desire.
    ///
    /// `pop` scales the amount through [`Pop::get_scaling_factor`] and
    /// `scalar`. `source` is stored on the desire. Satisfaction starts at 0.
    /// Additive effects use the same scale. Other effects are copied.
    pub fn create_desire(&self, pop: &Pop, source: DemographicSource) -> Desire {
        let scale = pop.get_scaling_factor(self.scalar);
        Desire {
            source,
            demo_desire_id: self.id,
            priority: self.priority,
            target: self.bucket.clone(),
            amount: self.amount * scale,
            satisfaction: 0.0,
            category: None,
            effect: self.scaled_effects(scale),
            scalar: self.scalar,
            decay: self.decay,
        }
    }
}

impl Desire {
    /// # Tiers Satisfied
    ///
    /// `satisfaction` divided by `amount`.
    pub fn tiers_satisfied(&self) -> f64 {
        self.satisfaction / self.amount
    }

    /// # Cmp Order
    ///
    /// Orders one desire relative to another within a tier.
    ///
    /// Lower `priority` comes first. Source kind breaks a tie: Species,
    /// then Culture, then Stratum, then Religion. Demo desire id breaks
    /// the rest.
    pub fn cmp_order(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.source.order_rank().cmp(&other.source.order_rank()))
            .then_with(|| self.demo_desire_id.cmp(&other.demo_desire_id))
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
                b.efficiency
                    .partial_cmp(&a.efficiency)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        });
        targets
    }

    /// # Implicit Minimum
    ///
    /// Preferred share of this desire for each good in the bucket.
    ///
    /// `config` supplies [`PopConfig::desire_share_minimum_divisor`]. `0`
    /// turns this off and returns 0. At `1` or above, the share is
    /// `1 / (divisor * goods)`. `goods` is the number of targets. An empty
    /// bucket returns 0.
    pub fn implicit_minimum(&self, config: &PopConfig) -> f64 {
        let divisor = config.desire_share_minimum_divisor;
        debug_assert!(divisor.is_finite() && (divisor == 0.0 || divisor >= 1.0));
        let goods = self.target.len();
        if divisor == 0.0 || goods == 0 {
            0.0
        } else {
            1.0 / (divisor * goods as f64)
        }
    }

    /// # Implicit Cap
    ///
    /// Share of this desire one good may fill.
    ///
    /// `config` supplies the same divisor as [`Self::implicit_minimum`].
    /// `0` turns this off and returns 1. At `1` or above, the share is
    /// `1 / divisor` plus the implicit minimum. An empty bucket returns 1.
    pub fn implicit_cap(&self, config: &PopConfig) -> f64 {
        let divisor = config.desire_share_minimum_divisor;
        debug_assert!(divisor.is_finite() && (divisor == 0.0 || divisor >= 1.0));
        if divisor == 0.0 || self.target.is_empty() {
            1.0
        } else {
            (1.0 / divisor) + self.implicit_minimum(config)
        }
    }

    /// # Minimum
    ///
    /// Share of this desire `slot` is brought toward first.
    ///
    /// The higher of that target's stored minimum and [`Self::implicit_minimum`].
    /// `config` supplies the divisor.
    pub fn minimum(&self, slot: usize, config: &PopConfig) -> f64 {
        let defined = self.target[slot].min;
        debug_assert!(defined.is_finite() && defined >= 0.0);
        defined.max(self.implicit_minimum(config))
    }

    /// # Cap
    ///
    /// Share of this desire `slot` may fill.
    ///
    /// The lower of that target's stored cap and [`Self::implicit_cap`].
    /// A divisor of 0 keeps the stored cap. `config` supplies the divisor.
    pub fn cap(&self, slot: usize, config: &PopConfig) -> f64 {
        let defined = self.target[slot].cap;
        debug_assert!(defined.is_finite() && defined >= 0.0);
        if config.desire_share_minimum_divisor == 0.0 {
            defined
        } else {
            defined.min(self.implicit_cap(config))
        }
    }

    /// # Get Bonus Satisfaction
    ///
    /// Sums [`DesireEffect::Satisfaction`] attached to this desire.
    ///
    /// A bonus is multiplied by [`Self::tiers_satisfied`], so it grows as the
    /// desire is met and keeps growing on further levels.
    ///
    /// A malus is multiplied by the unmet share of one level,
    /// `(1 - tiers).max(0)`. It is strongest with nothing satisfied and reaches
    /// zero once one full level is met. Further levels leave it at zero.
    pub(crate) fn get_bonus_satisfaction(&self) -> f64 {
        let tiers = self.tiers_satisfied();
        self.effect.iter().fold(0.0, |sum, effect| {
            if matches!(effect, DesireEffect::Satisfaction(..)) {
                sum + effect.signed_strength_raw(tiers)
            } else {
                sum
            }
        })
    }
}

impl DesireEffectRate {
    /// # Linear
    ///
    /// Safe linear maker. Ensures values are positive. Panics otherwise.
    pub fn linear(v: f64) -> Self {
        assert!(v > 0.0, "V must be a Positive Value.");
        Self::Linear(v)
    }

    /// # Logarithmic
    ///
    /// Safe logarithmic maker. Ensures values are > 1.0. Panics otherwise.
    pub fn logarithmic(v: f64) -> Self {
        assert!(v > 1.0, "V must be greater than 1.0.");
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

impl DesireTarget {
    /// # New
    ///
    /// Creates a target. Efficiency must be positive (`> 0`).
    ///
    /// The cap starts at 1 and the target is not high priority.
    pub fn new(good: usize, desire_type: DesireTargetType, eff: f64) -> Self {
        debug_assert!(eff > 0.0, "Desire target efficiency must be positive");
        Self {
            good,
            desire_type,
            efficiency: eff,
            cap: 1.0,
            min: 0.0,
            high_priority: false,
        }
    }

    /// # With Cap
    ///
    /// Sets the share of the desire this good may fill.
    pub fn with_cap(mut self, cap: f64) -> Self {
        self.cap = cap;
        self
    }

    /// # With Min
    ///
    /// Sets the minimum share of the desire this good may fill.
    pub fn with_min(mut self, min: f64) -> Self {
        self.min = min;
        self
    }

    /// # With High Priority
    ///
    /// Sets whether this target is satisfied first.
    pub fn with_high_priority(mut self, high_priority: bool) -> Self {
        self.high_priority = high_priority;
        self
    }
}

#[cfg(test)]
mod share_should {
    use super::{DemographicSource, Desire, DesireTarget, DesireTargetType, ScalingFactor};
    use crate::game::config::PopConfig;

    fn bucket(targets: Vec<DesireTarget>) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: targets,
            amount: 1.0,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn config_with(divisor: f64) -> PopConfig {
        let mut config = PopConfig::default();
        config.desire_share_minimum_divisor = divisor;
        config
    }

    fn target(good: usize) -> DesireTarget {
        DesireTarget::new(good, DesireTargetType::Consume, 1.0)
    }

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
    }

    #[test]
    fn default_divisor_uses_half_the_bucket() {
        let desire = bucket(vec![target(1), target(2)]);
        let config = PopConfig::default();
        near(desire.implicit_minimum(&config), 0.25);
        near(desire.implicit_cap(&config), 0.75);
        near(desire.minimum(0, &config), 0.25);
        near(desire.cap(0, &config), 0.75);
    }

    #[test]
    fn one_good_at_the_default_covers_the_desire() {
        let desire = bucket(vec![target(1)]);
        let config = PopConfig::default();
        near(desire.implicit_minimum(&config), 0.5);
        near(desire.implicit_cap(&config), 1.0);
    }

    #[test]
    fn a_divisor_of_one_splits_two_goods_in_half() {
        let desire = bucket(vec![target(1), target(2)]);
        let config = config_with(1.0);
        near(desire.implicit_minimum(&config), 0.5);
        near(desire.implicit_cap(&config), 1.5);
        near(desire.cap(0, &config), 1.0);
    }

    #[test]
    fn a_tighter_stored_bound_wins() {
        let desire = bucket(vec![target(1).with_min(0.4).with_cap(0.5), target(2)]);
        let config = PopConfig::default();
        near(desire.minimum(0, &config), 0.4);
        near(desire.cap(0, &config), 0.5);
        near(desire.minimum(1, &config), 0.25);
        near(desire.cap(1, &config), 0.75);
    }

    #[test]
    fn zero_keeps_the_stored_shares() {
        let desire = bucket(vec![target(1).with_min(0.2).with_cap(0.8)]);
        let config = config_with(0.0);
        assert_eq!(desire.implicit_minimum(&config), 0.0);
        assert_eq!(desire.implicit_cap(&config), 1.0);
        near(desire.minimum(0, &config), 0.2);
        near(desire.cap(0, &config), 0.8);
    }
}
