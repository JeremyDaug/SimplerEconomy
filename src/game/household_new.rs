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
    /// Female fraction among elders.
    pub elder_mf: f64,
    /// Female fraction among children.
    pub child_mf: f64,

    /// Adult labor rate per person per day.
    pub adult_labor: f64,
    /// Elder labor rate per person per day.
    pub elder_labor: f64,
    /// Child labor rate per person per day.
    pub child_labor: f64,
}

/// Years spent in childhood before aging into adulthood.
const CHILDHOOD_YEARS: f64 = 20.0;
/// Years spent in adulthood before aging into elderhood.
const ADULTHOOD_YEARS: f64 = 40.0;

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
        household
    }

    pub fn with_sex_breakdown(adult_mf: f64, elder_mf: f64, child_mf: f64) -> Self {
        let mut household = Household::new();
        household.adult_mf = adult_mf;
        household.elder_mf = elder_mf;
        household.child_mf = child_mf;
        household
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
    /// Flow (all rates taken from the start-of-turn snapshot):
    /// 1. Live births from adult women, reduced by infant mortality; newborns 50/50 sex.
    /// 2. Maternal deaths remove adult women (fraction of live births).
    /// 3. Age-band deaths: for each sex, rate = `total + sex_specific` (stacked), floored at 0.
    /// 4. Aging: children / 20 -> adults, adults / 40 -> elders (sex preserved into the next band).
    /// 5. `count = (total_adults + total_elders) / partnership_rate` when adults+elders remain;
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
        debug_assert!(
            self.adult_mf.is_finite()
                && self.elder_mf.is_finite()
                && self.child_mf.is_finite(),
            "sex fractions must be finite"
        );

        // --- Start-of-turn totals by sex (female fraction) -------------------
        let adult_f_frac = self.adult_mf.clamp(0.0, 1.0);
        let elder_f_frac = self.elder_mf.clamp(0.0, 1.0);
        let child_f_frac = self.child_mf.clamp(0.0, 1.0);

        let start_adults = self.total_adults().max(0.0);
        let start_elders = self.total_elders().max(0.0);
        let start_children = self.total_children().max(0.0);

        let start_adults_f = start_adults * adult_f_frac;
        let start_adults_m = start_adults - start_adults_f;
        let start_elders_f = start_elders * elder_f_frac;
        let start_elders_m = start_elders - start_elders_f;
        let start_children_f = start_children * child_f_frac;
        let start_children_m = start_children - start_children_f;

        // --- Clamped scalar rates -------------------------------------------
        let birth_per_woman = rates.birth_per_woman.max(0.0);
        let infant_mortality = rates.infant_mortality.clamp(0.0, 1.0);
        let maternal_mortality = rates.maternal_mortality.max(0.0);

        // --- 1. Births and maternal mortality -------------------------------
        // Live infants after infant mortality; maternal risk on those live births.
        let live_births = start_adults_f * birth_per_woman * (1.0 - infant_mortality);
        let maternal_deaths = (live_births * maternal_mortality).min(start_adults_f);
        let births_m = live_births * 0.5;
        let births_f = live_births - births_m;

        // --- 2. Age-band deaths (total + sex-specific, stacked) --------------
        let (child_death_m, child_death_f) = sex_band_deaths(
            start_children_m,
            start_children_f,
            rates.child_mortality,
        );
        let (adult_death_m, adult_death_f) = sex_band_deaths(
            start_adults_m,
            start_adults_f,
            rates.adult_mortality,
        );
        let (elder_death_m, elder_death_f) = sex_band_deaths(
            start_elders_m,
            start_elders_f,
            rates.elder_mortality,
        );

        // --- 3. Aging flows (fraction of stage per year) --------------------
        let child_aging_m = start_children_m / CHILDHOOD_YEARS;
        let child_aging_f = start_children_f / CHILDHOOD_YEARS;
        let adult_aging_m = start_adults_m / ADULTHOOD_YEARS;
        let adult_aging_f = start_adults_f / ADULTHOOD_YEARS;

        // --- 4. Apply simultaneous flows ------------------------------------
        let mut end_children_m =
            start_children_m + births_m - child_death_m - child_aging_m;
        let mut end_children_f =
            start_children_f + births_f - child_death_f - child_aging_f;

        let mut end_adults_m =
            start_adults_m + child_aging_m - adult_death_m - adult_aging_m;
        let mut end_adults_f = start_adults_f + child_aging_f
            - adult_death_f
            - maternal_deaths
            - adult_aging_f;

        let mut end_elders_m = start_elders_m + adult_aging_m - elder_death_m;
        let mut end_elders_f = start_elders_f + adult_aging_f - elder_death_f;

        end_children_m = end_children_m.max(0.0);
        end_children_f = end_children_f.max(0.0);
        end_adults_m = end_adults_m.max(0.0);
        end_adults_f = end_adults_f.max(0.0);
        end_elders_m = end_elders_m.max(0.0);
        end_elders_f = end_elders_f.max(0.0);

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

        // --- 5. Fold into count via partnership (adults + elders target) ----
        let partners = end_adults + end_elders;
        let partnership = rates.partnership_rate.max(f64::EPSILON);
        if partners > 0.0 {
            self.count = partners / partnership;
        } else {
            // Children only: keep a positive count from previous average size so
            // averages stay well-defined until adults reappear or the pop dies.
            let old_size = self.household_size().max(0.1);
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
    }
}

/// Deaths for one age band: each sex uses stacked `total + sex` rate (floored at 0).
/// Rate above 1.0 wipes that sex in the band.
fn sex_band_deaths(males: f64, females: f64, rates: (f64, f64, f64)) -> (f64, f64) {
    let (total, male_r, female_r) = rates;
    let m_rate = (total + male_r).max(0.0);
    let f_rate = (total + female_r).max(0.0);
    let death_m = (males * m_rate).min(males.max(0.0));
    let death_f = (females * f_rate).min(females.max(0.0));
    (death_m, death_f)
}

/// Female fraction from female headcount and band total; `0.5` if the band is empty.
fn female_fraction(females: f64, total: f64) -> f64 {
    if total > 0.0 {
        (females / total).clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// # Demographic Rates
///
/// Birth, mortality, and household-formation parameters for [`Household::update`].
/// Values may be stored uncapped from modifiers; [`Household::update`] clamps
/// infant mortality to `0..=1` and floors other rates at 0 where needed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemographicRates {
    /// Live-birth attempts per adult woman per year (before infant mortality).
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
        Self {
            birth_per_woman: 0.167,
            infant_mortality: 0.20,
            maternal_mortality: 0.015,
            child_mortality: (0.025, 0.0, 0.0),
            adult_mortality: (0.018, 0.0, 0.0),
            elder_mortality: (0.08, 0.0, 0.0),
            partnership_rate: 2.5,
        }
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
    fn zero_count_is_noop() {
        let mut h = Household::with_count(0.0);
        h.adult = 2.0;
        let rates = DemographicRates::baseline();
        h.update(&rates);
        assert_eq!(h.count, 0.0);
        assert_eq!(h.adult, 2.0);
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