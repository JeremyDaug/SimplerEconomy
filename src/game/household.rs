use crate::game::util::lerp;
use bitflags::*;

/// # Household
///
/// Storage for a pop's household block: household count, per-household average
/// members (adult / elder / child), female fraction in each age band, and labor
/// rates per person-day in each band.
///
/// ## Assumptions
///
/// Age ranges are fixed for round numbers: childhood 20 years, adulthood 40 years.
/// Each game turn is one year.
///
/// Sex fields (`*_mf`) are **female fractions** in `0.0..=1.0`
/// (`0.0` = all male, `1.0` = all female). Birth math uses adult women =
/// `total_adults * adult_mf`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Household {
    /// Count of households in the pop; fractional values are allowed and carry over.
    pub count: f64,

    // members (per-household averages)
    /// Average number of adults in a household.
    pub adult: f64,
    /// Average number of elders in a household.
    pub elder: f64,
    /// Average number of children in a household.
    pub child: f64,

    // sex breakdown (female fraction)
    /// Female fraction among adults (`0.0` all male, `1.0` all female).
    pub adult_mf: f64,
    /// Female fraction among elders (`0.0` all male, `1.0` all female)..
    pub elder_mf: f64,
    /// Female fraction among children (`0.0` all male, `1.0` all female)..
    pub child_mf: f64,

    /// Adult labor rate per person per day.
    pub adult_labor: f64,
    /// Elder labor rate per person per day.
    pub elder_labor: f64,
    /// Child labor rate per person per day.
    pub child_labor: f64,

    /// The Current Partnership rate of the households (household Adults + children).
    pub partnership_rate: f64,
}

/// Years spent in childhood before aging into adulthood.
const CHILDHOOD_YEARS: f64 = 20.0;
/// Years spent in adulthood before aging into elderhood.
const ADULTHOOD_YEARS: f64 = 40.0;
/// The default ratio of females to males in births (1.0 all female).
const CHILD_SEX_RATIO: f64 = 0.5;
/// The rate partnership moves closer to the targeted partnership rate.
const PARTNERSHIP_TREND: f64 = 0.13;

impl Household {
    /// # New
    /// 
    /// Creates a new household with default values. 
    /// 
    /// 1 Household of 2 adults, 2.5 Children, 0.5 elders. All 50% male/female.
    /// Adult labor is 1.0, elder labor is 0.7, and child labor is 0.3.
    pub fn new() -> Self {
        Household {
            count: 1.0,
            adult: 2.0,
            elder: 0.5,
            child: 2.5,
            adult_mf: 0.5,
            elder_mf: 0.5,
            child_mf: 0.5,
            adult_labor: 1.0,
            elder_labor: 0.7,
            child_labor: 0.3,
            partnership_rate: 2.5,
        }
    }

    pub fn with_count(count: f64) -> Self {
        let mut household = Household::new();
        household.count = count;
        household
    }

    pub fn with_members(adult: f64, elder: f64, child: f64) -> Self {
        let mut household = Household::new();
        household.adult = adult;
        household.elder = elder;
        household.child = child;
        household.partnership_rate = adult + elder;
        household.debug_assert_member_sizes_valid();
        household
    }

    pub fn with_sex_breakdown(adult_mf: f64, elder_mf: f64, child_mf: f64) -> Self {
        let mut household = Household::new();
        household.adult_mf = adult_mf;
        household.elder_mf = elder_mf;
        household.child_mf = child_mf;
        household.debug_assert_sex_ratios_valid();
        household
    }

    /// Debug-only: each `*_mf` female fraction is finite and in `0.0..=1.0`.
    #[inline]
    fn debug_assert_sex_ratios_valid(&self) {
        debug_assert!(
            sex_ratio_in_unit(self.adult_mf),
            "adult_mf must be finite and in 0.0..=1.0, got {}",
            self.adult_mf
        );
        debug_assert!(
            sex_ratio_in_unit(self.elder_mf),
            "elder_mf must be finite and in 0.0..=1.0, got {}",
            self.elder_mf
        );
        debug_assert!(
            sex_ratio_in_unit(self.child_mf),
            "child_mf must be finite and in 0.0..=1.0, got {}",
            self.child_mf
        );
    }

    /// Debug-only: average adult / elder / child sizes are finite and >= 0.
    #[inline]
    fn debug_assert_member_sizes_valid(&self) {
        debug_assert!(
            nonneg_finite(self.adult),
            "adult average must be finite and >= 0, got {}",
            self.adult
        );
        debug_assert!(
            nonneg_finite(self.elder),
            "elder average must be finite and >= 0, got {}",
            self.elder
        );
        debug_assert!(
            nonneg_finite(self.child),
            "child average must be finite and >= 0, got {}",
            self.child
        );
    }

    pub fn with_labor(adult_labor: f64, elder_labor: f64, child_labor: f64) -> Self {
        let mut household = Household::new();
        household.adult_labor = adult_labor;
        household.elder_labor = elder_labor;
        household.child_labor = child_labor;
        household
    }

    /// # Total Adults
    /// 
    /// Returns the total number of adults in the household.
    pub fn total_adults(&self) -> f64 {
        self.count * self.adult
    }

    /// # Total Elders
    /// 
    /// Returns the total numbers of elders in the household.
    pub fn total_elders(&self) -> f64 {
        self.count * self.elder
    }

    /// # Total Children
    /// 
    /// Returns the total numbers of children in the household.
    pub fn total_children(&self) -> f64 {
        self.count * self.child
    }

    /// # Total Count
    ///
    /// Returns the total number of people in the household group.
    /// `self.count * self.household_size()`.
    pub fn total_count(&self) -> f64 {
        self.count * self.household_size()
    }

    /// # Total Labor
    ///
    /// Total labor from all people in all households this turn.
    pub fn total_labor(&self) -> f64 {
        self.total_adults() * self.adult_labor
            + self.total_children() * self.child_labor
            + self.total_elders() * self.elder_labor
    }

    /// # Household Size
    ///
    /// Average people per household (adult + elder + child averages).
    pub fn household_size(&self) -> f64 {
        self.adult + self.elder + self.child
    }

    /// # Update
    ///
    /// Advance this household group by one turn (one year) under `rates`.
    ///
    /// `rates` is the **final** rate bundle for this tick: baseline demographic rates
    /// plus any modifiers (desires, institutions, events, stored effects, etc.) already
    /// folded in by the caller. This method does not apply a second modifier pass.
    /// 
    /// All rates are allowed to be negative, but negative values go to zero. 
    /// Some rates are capped at 1.0 to ensure no funny overflow occurs, infant 
    /// mortality, 
    ///
    /// Flow (start-of-turn composition is non-negative by invariant):
    /// 1. Live births from adult women, reduced by infant mortality; newborns 50/50 sex.
    /// 2. Age-band deaths (rate = total + sex, stacked), then maternal deaths on remaining women.
    /// 3. Aging from **survivors** (children/20, adults/40) so bands cannot go negative.
    /// 4. `count = (total_adults + total_elders) / partnership_rate` when adults+elders remain;
    ///    averages and female fractions are rebuilt from end-of-turn totals.
    ///
    /// Labor rates are not modified. Empty / dead groups zero out composition and count.
    pub fn update(&mut self, rates: &DemographicRates) {
        debug_assert!(
            self.count >= 1.0,
            "Household must have 1 or more households at the start."
        );
        debug_assert!(
            rates.partnership_rate > 0.0 && rates.partnership_rate.is_finite(),
            "partnership_rate must be finite and > 0, got {}",
            rates.partnership_rate
        );
        self.debug_assert_sex_ratios_valid();
        self.debug_assert_member_sizes_valid();

        // --- Start-of-turn totals by sex (female fraction) -------------------
        let curr_adults = self.total_adults();
        let curr_elders = self.total_elders();
        let curr_children = self.total_children();
        let curr_partnership = self.partnership_rate;

        let curr_adults_f = curr_adults * self.adult_mf;
        let curr_adults_m = curr_adults - curr_adults_f;
        let curr_elders_f = curr_elders * self.elder_mf;
        let curr_elders_m = curr_elders - curr_elders_f;
        let curr_children_f = curr_children * self.child_mf;
        let curr_children_m = curr_children - curr_children_f;

        // --- Rate floors (modifiers may push stored rates negative) ----------
        let birth_per_woman = rates.birth_per_woman.max(0.0);
        let infant_mortality = rates.infant_mortality.clamp(0.0, 1.0);
        let maternal_mortality = rates.maternal_mortality.clamp(0.0, 1.0); 

        // --- 1. Births ------------------------------------------------------
        let live_births = curr_adults_f * birth_per_woman * (1.0 - infant_mortality);
        let births_m = live_births * CHILD_SEX_RATIO;
        let births_f = live_births - births_m;

        // --- 2. Deaths, then maternal on remaining adult women --------------
        let (child_death_m, child_death_f) =
            sex_band_deaths(curr_children_m, curr_children_f, rates.child_mortality);
        let (adult_death_m, adult_death_f) =
            sex_band_deaths(curr_adults_m, curr_adults_f, rates.adult_mortality);
        let (elder_death_m, elder_death_f) =
            sex_band_deaths(curr_elders_m, curr_elders_f, rates.elder_mortality);

        let remain_children_m = curr_children_m - child_death_m; 
        let remain_children_f = curr_children_f - child_death_f;
        let remain_adults_m = curr_adults_m - adult_death_m;
        let mut remain_adults_f = curr_adults_f - adult_death_f;
        let remain_elders_m = curr_elders_m - elder_death_m;
        let remain_elders_f = curr_elders_f - elder_death_f;

        let maternal_deaths = (live_births * maternal_mortality).min(remain_adults_f);
        remain_adults_f -= maternal_deaths;

        // --- 3. Aging from survivors (keeps end bands non-negative) ---------
        let child_aging_m = remain_children_m / CHILDHOOD_YEARS;
        let child_aging_f = remain_children_f / CHILDHOOD_YEARS;
        let adult_aging_m = remain_adults_m / ADULTHOOD_YEARS;
        let adult_aging_f = remain_adults_f / ADULTHOOD_YEARS;

        let end_children_m = remain_children_m + births_m - child_aging_m;
        let end_children_f = remain_children_f + births_f - child_aging_f;
        let end_adults_m = remain_adults_m + child_aging_m - adult_aging_m;
        let end_adults_f = remain_adults_f + child_aging_f - adult_aging_f;
        let end_elders_m = remain_elders_m + adult_aging_m;
        let end_elders_f = remain_elders_f + adult_aging_f;

        debug_assert!(nonneg_finite(end_children_m) && nonneg_finite(end_children_f));
        debug_assert!(nonneg_finite(end_adults_m) && nonneg_finite(end_adults_f));
        debug_assert!(nonneg_finite(end_elders_m) && nonneg_finite(end_elders_f));

        let end_adults = end_adults_m + end_adults_f;
        let end_elders = end_elders_m + end_elders_f;
        let end_children = end_children_m + end_children_f;
        let end_total = end_adults + end_elders + end_children;

        if end_total <= 0.0 {
            self.count = 0.0;
            self.adult = 0.0;
            self.elder = 0.0;
            self.child = 0.0;
            // leave sex / labor fields as-is for inspection of the last living shape
            return;
        }

        // 4. Tug household parntership factor and size to be closer to the parntership rate given.
        let new_partnership = lerp(curr_partnership, rates.partnership_rate, PARTNERSHIP_TREND);
        let partners = end_adults + end_elders;
        if partners > 0.0 {
            self.count = partners / new_partnership;
        } else {
            // Children only: keep a positive count from previous average size.
            let old_size = self.household_size();
            debug_assert!(
                old_size > 0.0,
                "children-only fold requires positive prior household_size"
            );
            self.count = end_total / old_size;
        }

        debug_assert!(
            self.count > 0.0 && self.count.is_finite(),
            "household count must be finite and > 0 after update, got {}",
            self.count
        );

        self.adult = end_adults / self.count;
        self.elder = end_elders / self.count;
        self.child = end_children / self.count;

        self.adult_mf = female_fraction(end_adults_f, end_adults);
        self.elder_mf = female_fraction(end_elders_f, end_elders);
        self.child_mf = female_fraction(end_children_f, end_children);

        self.debug_assert_member_sizes_valid();
        self.debug_assert_sex_ratios_valid();
    }
}

/// True when `v` is a valid female-fraction sex ratio: finite and in `0.0..=1.0`.
#[inline]
fn sex_ratio_in_unit(v: f64) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}

/// True when `v` is finite and >= 0.
#[inline]
fn nonneg_finite(v: f64) -> bool {
    v.is_finite() && v >= 0.0
}

/// Deaths for one age band: each sex uses stacked `total + sex` rate (floored at 0).
/// Rate above 1.0 wipes that sex in the band. Callers pass non-negative headcounts.
fn sex_band_deaths(males: f64, females: f64, rates: (f64, f64, f64)) -> (f64, f64) {
    debug_assert!(nonneg_finite(males) && nonneg_finite(females));
    let (total, male_r, female_r) = rates;
    let m_rate = (total + male_r).max(0.0);
    let f_rate = (total + female_r).max(0.0);
    let death_m = (males * m_rate).min(males);
    let death_f = (females * f_rate).min(females);
    (death_m, death_f)
}

/// Female fraction from female headcount and band total; `0.5` if the band is empty.
fn female_fraction(females: f64, total: f64) -> f64 {
    let result = females / total;
    debug_assert!(0.0 <= result && result <= 1.0, "Female Fraction somehow left bounds!");
    result
}

/// # Demographic Rates
///
/// Birth, mortality, and household-formation parameters for [`Household::update`].
/// Values may be stored uncapped from modifiers; [`Household::update`] clamps
/// infant mortality to `0..=1` and floors other rates at 0 where needed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemographicRates {
    /// Live-birth attempts per adult woman per year (before infant mortality).
    /// TODO: Separate this value to distinquish between 'children born' and 'women who gave birth' to account for multichild births.
    pub birth_per_woman: f64,
    /// Fraction of births that die in infancy (`0.2` => 20% die, 80% become children).
    pub infant_mortality: f64,
    /// Adult women who die per live birth (maternal mortality).
    pub maternal_mortality: f64,

    /// Child death chance per year: `(total, male, female)`. Effective rate per sex
    /// is `total + sex` (stacked).
    pub child_mortality: (f64, f64, f64),
    /// Adult death chance per year: `(total, male, female)`, stacked as above.
    pub adult_mortality: (f64, f64, f64),
    /// Elder death chance per year: `(total, male, female)`, stacked as above.
    pub elder_mortality: (f64, f64, f64),

    /// Preferred adults+elders per household. After flows,
    /// `count = (total_adults + total_elders) / partnership_rate`.
    ///
    /// TODO: Change name to something more appropriate.
    pub partnership_rate: f64,
}

impl DemographicRates {
    /// # Baseline
    ///
    /// Rough starting rates aimed near ~2 adults, ~2.5 children, ~0.5 elders
    /// and positive growth when sex balance is even. Partnership target is
    /// adults+elders = 2.5 (2.0 + 0.5).
    pub fn baseline() -> Self {
        let maternal_mortality = 0.01;
        let birth_per_woman = 0.30;
        Self {
            birth_per_woman,
            infant_mortality: 0.10,
            maternal_mortality,
            child_mortality: (0.025, 0.0, 0.0),
            adult_mortality: (0.013, maternal_mortality * birth_per_woman, 0.0),
            elder_mortality: (0.09, 0.0, 0.0),
            partnership_rate: 2.5,
        }
    }

    /// # Zero
    ///
    /// All-zero delta (safe to add onto baseline or other rate bundles).
    /// Partnership target is 0 so additive mods do not shift household size
    /// unless the mod sets a non-zero partnership delta.
    pub fn zero() -> Self {
        Self {
            birth_per_woman: 0.0,
            infant_mortality: 0.0,
            maternal_mortality: 0.0,
            child_mortality: (0.0, 0.0, 0.0),
            adult_mortality: (0.0, 0.0, 0.0),
            elder_mortality: (0.0, 0.0, 0.0),
            partnership_rate: 0.0,
        }
    }

    /// # Add
    ///
    /// Field-wise sum of two rate bundles (including mortality triples and
    /// partnership target). Used to stack baseline + species + culture + religion
    /// + same-day modifiers.
    pub fn add(&self, other: &Self) -> Self {
        Self {
            birth_per_woman: self.birth_per_woman + other.birth_per_woman,
            infant_mortality: self.infant_mortality + other.infant_mortality,
            maternal_mortality: self.maternal_mortality + other.maternal_mortality,
            child_mortality: (
                self.child_mortality.0 + other.child_mortality.0,
                self.child_mortality.1 + other.child_mortality.1,
                self.child_mortality.2 + other.child_mortality.2,
            ),
            adult_mortality: (
                self.adult_mortality.0 + other.adult_mortality.0,
                self.adult_mortality.1 + other.adult_mortality.1,
                self.adult_mortality.2 + other.adult_mortality.2,
            ),
            elder_mortality: (
                self.elder_mortality.0 + other.elder_mortality.0,
                self.elder_mortality.1 + other.elder_mortality.1,
                self.elder_mortality.2 + other.elder_mortality.2,
            ),
            partnership_rate: self.partnership_rate + other.partnership_rate,
        }
    }

    /// # Apply Mortality
    ///
    /// Add `rate` into the mortality fields selected by `target`.
    ///
    /// Mapping:
    /// - `INFANTS` -> `infant_mortality`
    /// - `MATERNAL` -> `maternal_mortality`
    /// - Age bits (`CHILD` / `ADULT` / `ELDER`) -> that band's mortality triple
    /// - Sex bits on an age band: neither or both -> total component (`.0`);
    ///   only `MALE` -> male (`.1`); only `FEMALE` -> female (`.2`)
    /// - No age bits and no infant/maternal bits -> all three age bands
    /// - Infant and/or maternal only (no age bits) -> does not touch age bands
    pub fn apply_mortality(&mut self, target: HouseholdTarget, rate: f64) {
        debug_assert!(rate.is_finite(), "mortality rate delta must be finite");

        if target.contains(HouseholdTarget::INFANTS) {
            self.infant_mortality += rate;
        }
        if target.contains(HouseholdTarget::MATERNAL) {
            self.maternal_mortality += rate;
        }

        let has_age = target.intersects(HouseholdTarget::ANY_AGE);
        let has_special =
            target.intersects(HouseholdTarget::INFANTS | HouseholdTarget::MATERNAL);
        // Pure infant/maternal: leave age-band triples alone.
        // Otherwise (any age bit, or neither age nor special) apply to ages.
        let apply_ages = has_age || !has_special;
        if !apply_ages {
            return;
        }

        let ages = if has_age {
            target & HouseholdTarget::ANY_AGE
        } else {
            HouseholdTarget::ANY_AGE
        };

        if ages.contains(HouseholdTarget::CHILD) {
            apply_sex_band_mortality(&mut self.child_mortality, target, rate);
        }
        if ages.contains(HouseholdTarget::ADULT) {
            apply_sex_band_mortality(&mut self.adult_mortality, target, rate);
        }
        if ages.contains(HouseholdTarget::ELDER) {
            apply_sex_band_mortality(&mut self.elder_mortality, target, rate);
        }
    }
}

/// Route a mortality delta into a `(total, male, female)` triple from sex flags.
fn apply_sex_band_mortality(
    band: &mut (f64, f64, f64),
    target: HouseholdTarget,
    rate: f64,
) {
    let male = target.contains(HouseholdTarget::MALE);
    let female = target.contains(HouseholdTarget::FEMALE);
    match (male, female) {
        (false, false) | (true, true) => band.0 += rate,
        (true, false) => band.1 += rate,
        (false, true) => band.2 += rate,
    }
}

bitflags! {
    /// Compact flags for age/sex subgroups of a household as well as maternal and
    /// infant mortality rates.
    /// Combinable so an effect, desire, or filter can target
    /// "adult females", "all children", "elders of either sex", etc.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct HouseholdTarget: u8 {
        const CHILD  = 0b0000_0001;
        const ADULT  = 0b0000_0010;
        const ELDER  = 0b0000_0100;

        const MALE   = 0b0001_0000;
        const FEMALE = 0b0010_0000;

        const MATERNAL = 0b0100_0000;
        const INFANTS = 0b1000_0000;

        // Convenience composites
        const ANY_AGE = Self::CHILD.bits() | Self::ADULT.bits() | Self::ELDER.bits();
        const NOT_CHILD = Self::ADULT.bits() | Self::ELDER.bits();
        const NOT_ADULT = Self::CHILD.bits() | Self::ELDER.bits();
        const NOT_ELDER = Self::CHILD.bits() | Self::ADULT.bits();
        const ANY_SEX = Self::MALE.bits() | Self::FEMALE.bits();
        const ANY     = Self::ANY_AGE.bits() | Self::ANY_SEX.bits();
    }
}

#[cfg(test)]
mod household_new_update_should {
    use super::*;

    fn assert_finite_household(h: &Household) {
        assert!(h.count.is_finite());
        assert!(h.adult.is_finite() && h.elder.is_finite() && h.child.is_finite());
        assert!(h.adult_mf.is_finite() && h.elder_mf.is_finite() && h.child_mf.is_finite());
    }

    #[test]
    #[should_panic(expected = "Household must have 1 or more households")]
    fn update_requires_count_at_least_one() {
        let mut h = Household::with_count(0.0);
        h.update(&DemographicRates::baseline());
    }

    #[test]
    #[should_panic(expected = "adult_mf must be finite and in 0.0..=1.0")]
    fn sex_ratio_outside_unit_is_rejected() {
        let _ = Household::with_sex_breakdown(1.5, 0.5, 0.5);
    }

    #[test]
    #[should_panic(expected = "adult average must be finite and >= 0")]
    fn negative_member_size_is_rejected() {
        let _ = Household::with_members(-1.0, 0.5, 2.5);
    }

    #[test]
    fn baseline_keeps_positive_population_and_partnership_count() {
        let mut h = Household::with_count(10.0);
        let rates = DemographicRates::baseline();
        let before = h.total_count();
        h.update(&rates);
        assert_finite_household(&h);
        assert!(h.count > 0.0);
        assert!(h.total_count() > 0.0);
        // Partnership: adults+elders averages should sum to partnership_rate.
        let partners_per_house = h.adult + h.elder;
        assert!(
            (partners_per_house - rates.partnership_rate).abs() < 1e-9,
            "adult+elder average {partners_per_house} should equal partnership_rate"
        );
        // Not a wipe.
        assert!(h.total_count() > before * 0.5);
    }

    #[test]
    fn sex_specific_mortality_shifts_female_fraction() {
        let mut h = Household::with_count(10.0);
        // Even start.
        h.adult_mf = 0.5;
        h.child_mf = 0.5;
        h.elder_mf = 0.5;
        let mut rates = DemographicRates::baseline();
        // Heavy extra female adult mortality, no extra male.
        rates.adult_mortality = (0.0, 0.0, 0.5);
        rates.birth_per_woman = 0.0;
        rates.maternal_mortality = 0.0;
        h.update(&rates);
        assert!(
            h.adult_mf < 0.5,
            "female-heavy adult mortality should lower adult female fraction, got {}",
            h.adult_mf
        );
    }

    #[test]
    fn everyone_dead_zeros_composition() {
        // Simultaneous flows still age survivors into the next band, so a full wipe
        // needs no younger band to feed adults/elders (no children, no adults).
        let mut h = Household::with_count(5.0);
        h.adult = 0.0;
        h.child = 0.0;
        h.elder = 1.0;
        let rates = DemographicRates {
            birth_per_woman: 0.0,
            infant_mortality: 0.0,
            maternal_mortality: 0.0,
            child_mortality: (0.0, 0.0, 0.0),
            adult_mortality: (0.0, 0.0, 0.0),
            elder_mortality: (2.0, 0.0, 0.0),
            partnership_rate: 2.5,
        };
        h.update(&rates);
        assert_eq!(h.count, 0.0);
        assert_eq!(h.adult, 0.0);
        assert_eq!(h.elder, 0.0);
        assert_eq!(h.child, 0.0);
    }

    #[test]
    fn newborns_are_split_evenly_when_starting_childless() {
        let mut h = Household::with_count(10.0);
        h.child = 0.0;
        h.child_mf = 0.0; // will be replaced by births
        let mut rates = DemographicRates::baseline();
        rates.child_mortality = (0.0, 0.0, 0.0);
        // No aging out of empty children; suppress other noise.
        rates.adult_mortality = (0.0, 0.0, 0.0);
        rates.elder_mortality = (0.0, 0.0, 0.0);
        rates.maternal_mortality = 0.0;
        rates.infant_mortality = 0.0;
        rates.birth_per_woman = 1.0; // one birth per woman
        h.update(&rates);
        assert!(h.child > 0.0);
        assert!(
            (h.child_mf - 0.5).abs() < 1e-9,
            "newborns should be 50/50, child_mf={}",
            h.child_mf
        );
    }
}

#[cfg(test)]
mod apply_mortality_should {
    use super::*;

    #[test]
    fn infants_and_maternal_only_touch_special_fields() {
        let mut rates = DemographicRates::zero();
        rates.apply_mortality(HouseholdTarget::INFANTS, 0.1);
        rates.apply_mortality(HouseholdTarget::MATERNAL, 0.05);
        assert!((rates.infant_mortality - 0.1).abs() < 1e-12);
        assert!((rates.maternal_mortality - 0.05).abs() < 1e-12);
        assert_eq!(rates.child_mortality, (0.0, 0.0, 0.0));
        assert_eq!(rates.adult_mortality, (0.0, 0.0, 0.0));
        assert_eq!(rates.elder_mortality, (0.0, 0.0, 0.0));
    }

    #[test]
    fn adult_female_hits_adult_female_component() {
        let mut rates = DemographicRates::zero();
        rates.apply_mortality(HouseholdTarget::ADULT | HouseholdTarget::FEMALE, 0.2);
        assert_eq!(rates.adult_mortality, (0.0, 0.0, 0.2));
        assert_eq!(rates.child_mortality, (0.0, 0.0, 0.0));
    }

    #[test]
    fn no_age_bits_apply_to_all_age_bands_total() {
        let mut rates = DemographicRates::zero();
        rates.apply_mortality(HouseholdTarget::empty(), 0.03);
        assert_eq!(rates.child_mortality, (0.03, 0.0, 0.0));
        assert_eq!(rates.adult_mortality, (0.03, 0.0, 0.0));
        assert_eq!(rates.elder_mortality, (0.03, 0.0, 0.0));
    }

    #[test]
    fn male_only_without_age_hits_all_bands_male() {
        let mut rates = DemographicRates::zero();
        rates.apply_mortality(HouseholdTarget::MALE, 0.04);
        assert_eq!(rates.child_mortality, (0.0, 0.04, 0.0));
        assert_eq!(rates.adult_mortality, (0.0, 0.04, 0.0));
        assert_eq!(rates.elder_mortality, (0.0, 0.04, 0.0));
    }
}