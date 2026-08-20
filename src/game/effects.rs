//! # Effects
//!
//! Shared vocabulary of gameplay effects, plus **domain-specific** enums that
//! restrict which effects are legal in each context.
//!
//! - [`EffectKind`] — master catalog (for humans reading the design + conversion)
//! - Domain enums (`DesireEffect`, `PopEffect`, …) — typed storage at each site
//! - `to_kind` / `From` bridges — convert domain → catalog when applying/logging
//!
//! Process **recipe** modifiers ([`InputEffect`](crate::game::process::InputEffect))
//! stay on `process` for now: they change production math, not actor state. Their
//! birth / mortality arms bridge into [`ProcessEffect`] / [`EffectKind`].

use std::vec;

use crate::game::sentiment::SentimentKind;
use crate::game::household::HouseholdTarget;

// ---------------------------------------------------------------------------
// Master catalog
// ---------------------------------------------------------------------------

/// # Effect Kind
///
/// Master list of every payload the game can express. Domain enums are subsets
/// of this vocabulary with extra context (bonus/malus, scope, process yields).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectKind {
    // --- Population rates ---
    /// Additive birth-rate delta (e.g. `0.01` = +1%/turn).
    /// Applies to Birth-Per-Woman.
    BirthRate(f64),
    /// Additive mortality-rate delta. Applies to the demographic group
    /// specified in the [`HouseholdTarget`](crate::game::household::HouseholdTarget).
    MortalityRate(HouseholdTarget, f64),

    // --- Soft power / state ---
    Research(f64),
    Culture(f64),
    Faith(f64),
    Authority(f64),
    Legitimacy(f64),

    // --- Household structure / passive rates (demographic) ---
    AdultEfficiency(f64),
    ElderEfficiency(f64),
    ChildEfficiency(f64),
    /// Passive research generation per household (not tech points from processes).
    HouseholdResearchRate(f64),
    /// Passive culture generation per household.
    HouseholdCultureRate(f64),

    // --- Goods ---
    /// Extra goods granted (e.g. desire bonus transport).
    BonusGood { good: usize, amount: f64 },

    // --- Mood / satisfaction ---
    Satisfaction(f64),
    /// Flat share-of-pop shift into a sentiment axis (sign convention at site).
    SentimentFlat {
        kind: SentimentKind,
        amount: f64,
    },
    /// Relative scale of one sentiment axis (sign convention at site).
    SentimentRelative {
        kind: SentimentKind,
        relative: f64,
    },
}

// ---------------------------------------------------------------------------
// Scope (institution application)
// ---------------------------------------------------------------------------

/// Who an institution effect applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectScope {
    /// Everyone in markets/territory of the institution's owning player.
    OwnerRealm,
    /// Workers employed at firms owned/controlled by the institution.
    Members,
    // Regional / unlocked-region later if needed.
}

// ---------------------------------------------------------------------------
// Desire effects
// ---------------------------------------------------------------------------

/// Effects attached to a desire; applied from satisfaction (bonus) or lack (malus).
///
/// The `bool` is **true = bonus** (scales with satisfaction), **false = malus**
/// (scales with lack of satisfaction).
///
/// Player-resource arms (culture, research, faith, authority, legitimacy) are
/// harvested in [`crate::game::pop::Pop::extract_special_resources`]. Common
/// sat is clamped to `[0, 1]` at harvest; luxury sat is unclamped so extra
/// levels scale. A catch-all `PlayerResource` arm (id + amount + bonus) will
/// likely be added later if more non-good stocks appear.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DesireEffect {
    /// Mortality pressure on the targeted household subgroup when unmet
    /// (or bonus path when the bool says so).
    Mortality(HouseholdTarget, f64, bool),
    /// Growth via birth when met (or malus path when the bool says so).
    Birthrate(f64, bool),
    /// Extra goods granted from satisfaction path.
    BonusGood(usize, f64, bool),
    /// Extra satisfaction units applied to the **same tier** as the source desire.
    ///
    /// Applied early in [`crate::game::pop::Pop::update_sentiments`]. Never used
    /// on basic (tier 0). Common clamps to one full level; luxury may oversat.
    /// The `bool` is bonus (scales with sat) vs malus (scales with lack).
    Satisfaction(f64, bool),
    /// Flat share-of-pop shift into a sentiment axis (from other axes proportionally).
    ///
    /// Applied in [`crate::game::pop::Pop::update_sentiments`]. The `bool` is
    /// bonus (scales with sat) vs malus (scales with lack).
    SentimentFlat(SentimentKind, f64, bool),
    /// Relative scale of one sentiment axis, then renormalize.
    /// Applied in [`crate::game::pop::Pop::update_sentiments`].
    SentimentRelative(SentimentKind, f64, bool),
    /// Culture points for the owning player.
    Culture(f64, bool),
    /// Research points for the owning player.
    Research(f64, bool),
    /// Faith for the owning player (decays on the pool).
    Faith(f64, bool),
    /// Authority for the owning player.
    Authority(f64, bool),
    /// Legitimacy for the owning player (may be negative).
    Legitimacy(f64, bool),
}

impl DesireEffect {
    /// Catalog kind (ignores bonus/malus; use [`Self::is_bonus`] for that).
    pub fn to_kind(self) -> EffectKind {
        match self {
            DesireEffect::Mortality(target, v, _) => EffectKind::MortalityRate(target, v),
            DesireEffect::Birthrate(v, _) => EffectKind::BirthRate(v),
            DesireEffect::BonusGood(good, amount, _) => EffectKind::BonusGood { good, amount },
            DesireEffect::Satisfaction(amount, _) => EffectKind::Satisfaction(amount),
            DesireEffect::SentimentFlat(kind, amount, _) => {
                EffectKind::SentimentFlat { kind, amount }
            }
            DesireEffect::SentimentRelative(kind, relative, _) => {
                EffectKind::SentimentRelative { kind, relative }
            }
            DesireEffect::Culture(v, _) => EffectKind::Culture(v),
            DesireEffect::Research(v, _) => EffectKind::Research(v),
            DesireEffect::Faith(v, _) => EffectKind::Faith(v),
            DesireEffect::Authority(v, _) => EffectKind::Authority(v),
            DesireEffect::Legitimacy(v, _) => EffectKind::Legitimacy(v),
        }
    }

    /// True if this effect scales with satisfaction (bonus), false if with lack (malus).
    pub fn is_bonus(self) -> bool {
        match self {
            DesireEffect::Mortality(_, _, b)
            | DesireEffect::Birthrate(_, b)
            | DesireEffect::BonusGood(_, _, b)
            | DesireEffect::Satisfaction(_, b)
            | DesireEffect::SentimentFlat(_, _, b)
            | DesireEffect::SentimentRelative(_, _, b)
            | DesireEffect::Culture(_, b)
            | DesireEffect::Research(_, b)
            | DesireEffect::Faith(_, b)
            | DesireEffect::Authority(_, b)
            | DesireEffect::Legitimacy(_, b) => b,
        }
    }

    /// Magnitude after applying satisfaction in `[0, 1]`.
    ///
    /// Bonus → `+rate * sat`; malus → `-rate * (1 - sat)`.
    pub fn signed_strength(self, sat01: f64) -> f64 {
        self.signed_strength_raw(sat01.clamp(0.0, 1.0))
    }

    /// Like [`Self::signed_strength`], but `sat` is not clamped to 1.
    ///
    /// Floor at 0. Malus uses `max(0, 1 - sat)` so luxury oversat does not
    /// invert the malus. Use this for luxury player-resource harvest; keep
    /// [`Self::signed_strength`] for growth / sentiment / common.
    pub fn signed_strength_raw(self, sat: f64) -> f64 {
        debug_assert!(sat.is_finite(), "satisfaction must be finite");
        let sat = sat.max(0.0);
        let lack = (1.0 - sat).max(0.0);
        let rate = match self {
            DesireEffect::Mortality(_, v, _) | DesireEffect::Birthrate(v, _) => v,
            DesireEffect::BonusGood(_, amount, _) => amount,
            DesireEffect::Satisfaction(amount, _) => amount,
            DesireEffect::SentimentFlat(_, amount, _) => amount,
            DesireEffect::SentimentRelative(_, relative, _) => relative,
            DesireEffect::Culture(v, _)
            | DesireEffect::Research(v, _)
            | DesireEffect::Faith(v, _)
            | DesireEffect::Authority(v, _)
            | DesireEffect::Legitimacy(v, _) => v,
        };
        if self.is_bonus() {
            rate * sat
        } else {
            -rate * lack
        }
    }

    /// True for culture / research / faith / authority / legitimacy arms.
    pub fn is_player_resource(self) -> bool {
        matches!(
            self,
            DesireEffect::Culture(..)
                | DesireEffect::Research(..)
                | DesireEffect::Faith(..)
                | DesireEffect::Authority(..)
                | DesireEffect::Legitimacy(..)
        )
    }

    /// Growth arms left for growth phase.
    pub fn is_growth(self) -> bool {
        matches!(
            self,
            DesireEffect::Birthrate(..) | DesireEffect::Mortality(..)
        )
    }

    /// Goods paid out at decay.
    pub fn is_bonus_good(self) -> bool {
        matches!(self, DesireEffect::BonusGood(..))
    }
    
    /// # Scale By
    /// 
    /// Scales our Desire Effect by a given scalar, returning the new version.
    pub(crate) fn scale_by(&self, effect_scale: f64) -> Self {
        match self {
            DesireEffect::Mortality(household_target, v, b) => 
                DesireEffect::Mortality(*household_target, v*effect_scale, *b),
            DesireEffect::Birthrate(v, b) => 
                DesireEffect::Birthrate(v*effect_scale, *b),
            DesireEffect::BonusGood(g, v, b) => 
                DesireEffect::BonusGood(*g, v*effect_scale, *b),
            DesireEffect::Satisfaction(v, b) => 
                DesireEffect::Satisfaction(v*effect_scale, *b),
            DesireEffect::SentimentFlat(sentiment_kind, v, b) => 
                DesireEffect::SentimentFlat(*sentiment_kind, v*effect_scale, *b),
            DesireEffect::SentimentRelative(sentiment_kind, v, b) => 
                DesireEffect::SentimentRelative(*sentiment_kind, v*effect_scale, *b),
            DesireEffect::Culture(v, b) => 
                DesireEffect::Culture(v*effect_scale, *b),
            DesireEffect::Research(v, b) => 
                DesireEffect::Research(v*effect_scale, *b),
            DesireEffect::Faith(v, b) => 
                DesireEffect::Faith(v*effect_scale, *b),
            DesireEffect::Authority(v, b) => 
                DesireEffect::Authority(v*effect_scale, *b),
            DesireEffect::Legitimacy(v, b) => 
                DesireEffect::Authority(v*effect_scale, *b),
        }
    }
}

// ---------------------------------------------------------------------------
// Pop effects (ephemeral, same-day)
// ---------------------------------------------------------------------------

/// Same-day effects stored on a pop (environment, events, process spillover, …).
/// Must not survive past end of day.
///
/// - Growth arms ([`PopEffect::Birthrate`] / [`PopEffect::Mortality`]) →
///   [`crate::game::pop::Pop::growth_phase`] (applied and removed there).
/// - Satisfaction boosts + mood/sentiment → [`crate::game::pop::Pop::update_sentiments`].
/// - [`PopEffect::BonusGood`] → [`crate::game::pop::Pop::decay_goods`].
/// - Player-resource arms (already scaled) →
///   [`crate::game::pop::Pop::extract_special_resources`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PopEffect {
    Birthrate(f64),
    /// Mortality delta for the targeted household subgroup.
    Mortality(HouseholdTarget, f64),
    /// Extra satisfaction units for a **non-basic** desire tier (`tier` 1 = common,
    /// 2 = luxury). Applied early in update_sentiments. Common clamps to one
    /// full level; luxury may oversat. `amount` is already scaled.
    Satisfaction { tier: usize, amount: f64 },
    /// Extra goods already scaled; granted at end-of-day decay.
    BonusGood { good: usize, amount: f64 },
    /// Flat share-of-pop into an axis (donors unspecified — other axes scale down).
    SentimentFlat { kind: SentimentKind, delta: f64 },
    /// Relative scale of one axis, then renormalize.
    SentimentRelative { kind: SentimentKind, relative: f64 },
    /// Culture points already scaled; harvested at extract.
    Culture(f64),
    /// Research points already scaled; harvested at extract.
    Research(f64),
    /// Faith already scaled; harvested at extract.
    Faith(f64),
    /// Authority already scaled; harvested at extract.
    Authority(f64),
    /// Legitimacy already scaled; harvested at extract.
    Legitimacy(f64),
}

impl PopEffect {
    pub fn to_kind(self) -> EffectKind {
        match self {
            PopEffect::Birthrate(v) => EffectKind::BirthRate(v),
            PopEffect::Mortality(target, v) => EffectKind::MortalityRate(target, v),
            PopEffect::Satisfaction { amount, .. } => EffectKind::Satisfaction(amount),
            PopEffect::BonusGood { good, amount } => EffectKind::BonusGood { good, amount },
            PopEffect::SentimentFlat { kind, delta } => {
                EffectKind::SentimentFlat { kind, amount: delta }
            }
            PopEffect::SentimentRelative { kind, relative } => {
                EffectKind::SentimentRelative { kind, relative }
            }
            PopEffect::Culture(v) => EffectKind::Culture(v),
            PopEffect::Research(v) => EffectKind::Research(v),
            PopEffect::Faith(v) => EffectKind::Faith(v),
            PopEffect::Authority(v) => EffectKind::Authority(v),
            PopEffect::Legitimacy(v) => EffectKind::Legitimacy(v),
        }
    }

    /// Growth arms left for growth phase.
    pub fn is_growth(self) -> bool {
        matches!(self, PopEffect::Birthrate(..) | PopEffect::Mortality(..))
    }

    /// Goods paid out at decay.
    pub fn is_bonus_good(self) -> bool {
        matches!(self, PopEffect::BonusGood { .. })
    }

    /// True for culture / research / faith / authority / legitimacy arms.
    pub fn is_player_resource(self) -> bool {
        matches!(
            self,
            PopEffect::Culture(_)
                | PopEffect::Research(_)
                | PopEffect::Faith(_)
                | PopEffect::Authority(_)
                | PopEffect::Legitimacy(_)
        )
    }
}

impl From<PopEffect> for EffectKind {
    fn from(value: PopEffect) -> Self {
        value.to_kind()
    }
}

// ---------------------------------------------------------------------------
// Demographic effects (structural household mods)
// ---------------------------------------------------------------------------

/// Structural modifiers from species/culture/religion (and similar).
/// Intended to fold into [`crate::game::household::DemographicRates`] (and labor
/// fields on [`crate::game::household::Household`]); mapping is still evolving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DemographicEffect {
    /// Discourage large positive values; adults already have 1.0 baseline efficiency.
    AdultEfficiency(f64),
    ElderEfficiency(f64),
    ChildEfficiency(f64),
    BirthRate(f64),
    MortalityRate(HouseholdTarget, f64),
    /// Passive research per household. Harvested at extract.
    /// TODO, expand to include rates for each part of the household.
    ResearchRate(f64),
    /// Passive culture per household. Harvested at extract.
    /// TODO, expand to include rates for each part of the household.
    CultureRate(f64),
}

impl DemographicEffect {
    pub fn to_kind(self) -> EffectKind {
        match self {
            DemographicEffect::AdultEfficiency(v) => EffectKind::AdultEfficiency(v),
            DemographicEffect::ElderEfficiency(v) => EffectKind::ElderEfficiency(v),
            DemographicEffect::ChildEfficiency(v) => EffectKind::ChildEfficiency(v),
            DemographicEffect::BirthRate(v) => EffectKind::BirthRate(v),
            DemographicEffect::MortalityRate(target, v) => EffectKind::MortalityRate(target, v),
            DemographicEffect::ResearchRate(v) => EffectKind::HouseholdResearchRate(v),
            DemographicEffect::CultureRate(v) => EffectKind::HouseholdCultureRate(v),
        }
    }
}

impl From<DemographicEffect> for EffectKind {
    fn from(value: DemographicEffect) -> Self {
        value.to_kind()
    }
}

// ---------------------------------------------------------------------------
// Institution effects
// ---------------------------------------------------------------------------

/// Effects an institution can apply, with an explicit [`EffectScope`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstitutionEffect {
    BirthRate { rate: f64, scope: EffectScope },
    MortalityRate {
        target: HouseholdTarget,
        rate: f64,
        scope: EffectScope,
    },
}

impl InstitutionEffect {
    pub fn member_birthrate(rate: f64) -> Self {
        Self::BirthRate {
            rate,
            scope: EffectScope::Members,
        }
    }

    /// Member mortality on adults (legacy default; matches prior adult-only push).
    pub fn member_mortality(rate: f64) -> Self {
        Self::MortalityRate {
            target: HouseholdTarget::ADULT,
            rate,
            scope: EffectScope::Members,
        }
    }

    pub fn member_mortality_on(target: HouseholdTarget, rate: f64) -> Self {
        Self::MortalityRate {
            target,
            rate,
            scope: EffectScope::Members,
        }
    }

    pub fn realm_birthrate(rate: f64) -> Self {
        Self::BirthRate {
            rate,
            scope: EffectScope::OwnerRealm,
        }
    }

    /// Realm mortality on adults (legacy default; matches prior adult-only push).
    pub fn realm_mortality(rate: f64) -> Self {
        Self::MortalityRate {
            target: HouseholdTarget::ADULT,
            rate,
            scope: EffectScope::OwnerRealm,
        }
    }

    pub fn realm_mortality_on(target: HouseholdTarget, rate: f64) -> Self {
        Self::MortalityRate {
            target,
            rate,
            scope: EffectScope::OwnerRealm,
        }
    }

    pub fn to_kind(self) -> EffectKind {
        match self {
            InstitutionEffect::BirthRate { rate, .. } => EffectKind::BirthRate(rate),
            InstitutionEffect::MortalityRate { target, rate, .. } => {
                EffectKind::MortalityRate(target, rate)
            }
        }
    }

    pub fn scope(self) -> EffectScope {
        match self {
            InstitutionEffect::BirthRate { scope, .. }
            | InstitutionEffect::MortalityRate { scope, .. } => scope,
        }
    }
}

// ---------------------------------------------------------------------------
// Process effects (yields from completing a process)
// ---------------------------------------------------------------------------

/// Additional effects a process produces when run (scaled by iterations).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum ProcessEffect {
    /// Research points → firm.
    Research(f64),
    /// Culture → worker cultures.
    Culture(f64),
    /// Faith → worker religions.
    Faith(f64),
    /// Authority → territory owner.
    Authority(f64),
    /// Legitimacy → territory owner.
    Legitimacy(f64),
    /// Birth-per-woman delta on worker populace (scaling vs iterations is site-defined).
    BirthRate(f64),
    /// Mortality delta on worker populace for the targeted subgroup.
    MortalityRate(HouseholdTarget, f64),
}

impl ProcessEffect {
    pub fn to_kind(self) -> EffectKind {
        match self {
            ProcessEffect::Research(v) => EffectKind::Research(v),
            ProcessEffect::Culture(v) => EffectKind::Culture(v),
            ProcessEffect::Faith(v) => EffectKind::Faith(v),
            ProcessEffect::Authority(v) => EffectKind::Authority(v),
            ProcessEffect::Legitimacy(v) => EffectKind::Legitimacy(v),
            ProcessEffect::BirthRate(v) => EffectKind::BirthRate(v),
            ProcessEffect::MortalityRate(target, v) => EffectKind::MortalityRate(target, v),
        }
    }

    /// Scales the effect by the given multiplier, returning a new effect.
    pub fn scale(self, multiplier: f64) -> Self {
        match self {
            ProcessEffect::Research(v) => ProcessEffect::Research(v * multiplier),
            ProcessEffect::Culture(v) => ProcessEffect::Culture(v * multiplier),
            ProcessEffect::Faith(v) => ProcessEffect::Faith(v * multiplier),
            ProcessEffect::Authority(v) => ProcessEffect::Authority(v * multiplier),
            ProcessEffect::Legitimacy(v) => ProcessEffect::Legitimacy(v * multiplier),
            ProcessEffect::BirthRate(v) => ProcessEffect::BirthRate(v * multiplier),
            ProcessEffect::MortalityRate(target, v) => {
                ProcessEffect::MortalityRate(target, v * multiplier)
            }
        }
    }

    pub fn add(&self, other: &ProcessEffect) -> Option<Self> {
        match (self, other) {
            (ProcessEffect::Research(v1), ProcessEffect::Research(v2)) => {
                Some(ProcessEffect::Research(v1 + v2))
            }
            (ProcessEffect::Culture(v1), ProcessEffect::Culture(v2)) => {
                Some(ProcessEffect::Culture(v1 + v2))
            }
            (ProcessEffect::Faith(v1), ProcessEffect::Faith(v2)) => {
                Some(ProcessEffect::Faith(v1 + v2))
            }
            (ProcessEffect::Authority(v1), ProcessEffect::Authority(v2)) => {
                Some(ProcessEffect::Authority(v1 + v2))
            }
            (ProcessEffect::Legitimacy(v1), ProcessEffect::Legitimacy(v2)) => {
                Some(ProcessEffect::Legitimacy(v1 + v2))
            }
            (ProcessEffect::BirthRate(v1), ProcessEffect::BirthRate(v2)) => {
                Some(ProcessEffect::BirthRate(v1 + v2))
            }
            (
                ProcessEffect::MortalityRate(t1, v1),
                ProcessEffect::MortalityRate(t2, v2),
            ) if t1 == t2 => Some(ProcessEffect::MortalityRate(*t1, v1 + v2)),
            _ => None,
        }
    }
}

impl From<ProcessEffect> for EffectKind {
    fn from(value: ProcessEffect) -> Self {
        value.to_kind()
    }
}
