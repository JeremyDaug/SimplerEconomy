/// # Demographic Rates
/// 
/// The Demographic rates which modify the Household and simulate group and household 
/// growth dynamics. 
/// 
/// Values between demographic rates and effects that modify these rates are added together
/// directly. 
/// 
/// We do allow negative values, but clamp all of them to more appropriate minimums when calculating.
/// Birth and Mortality rates cap at 0%, except elders who cap at 0.00001% (near immortality).
/// Labor efficiency can be a negative value, though the sum result of labor values should never get
/// too low.
/// 
/// Negative Labor Rates should are allowed for Children and Elders, representing 
/// additional care needed to support them, Adults should never have negative labor 
/// rates. Also, the Sum of Labor across a pop should also never be negative.
/// 
/// When calculating age categories, we assume a turn is equal to 1 year, adulthood 
/// occurs at 20, and elderhood occurs at 60 for nice round numbers.
/// 
/// Since Elders and children can have labor efficiency values, child labor and 
/// retirement age is included second hand. Reducing `child_eff` is the same as reducing 
/// child labor. Reducing 'elder_eff' is the same as reducing the retirement age.
/// 
/// Culture and research should never go below 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemographicRates {
    // Birth and Death Rates
    /// Births Per Woman (adults * 0.5)
    /// Adds children to population.
    /// 
    /// Effect cap of 0.0%
    pub births_per_woman: f64,
    /// Infant mortality rate.
    /// Reduces Birthrate.
    /// Effect cap of 0.0%
    pub infant_mortality: f64,
    /// Maternal mortality rate. Chance of a woman dying in childbirth.
    /// Effect cap of 0.0%
    pub maternal_mortality: f64,
    /// Child Mortality Rate, chance a child dies each turn.
    /// Effect cap of 0.0%
    pub child_mortality: f64,
    /// Adult Mortality Rate, chance an adult dies each turn.
    /// Effect cap of 0.0%
    pub adult_mortality: f64,
    /// Elder Mortality Rate, chance an elder dies each turn.
    /// Effect cap of 0.000001%
    /// Allowing 0% minimum would require a mechanism to alter 'retirement' age.
    pub elder_mortality: f64,

    // labor efficiencies.
    /// Adult labor efficiency. (Turn Time * adult_eff = available time)
    pub adult_eff: f64,
    /// Elder labor efficiency. (Turn Time * elder_eff = available time)
    pub elder_eff: f64,
    /// Child labor efficiency. (Turn Time * child_eff = available time)
    pub child_eff: f64,

    // baseline household culture and research rates.
    /// Culture produced per household.
    pub culture: f64,
    /// Research produced per household.
    pub research: f64,

    // TODO: Consider breaking Culture and Research into category rates, not just household.
    // TODO: Consider adding Gender Balance in each group also.
}

impl DemographicRates {
    /// # Baseline Demographic Rates
    /// 
    /// This is the 'default' values that all pops should have game start,
    /// assuming no modifiers from other demographic factors.
    /// 
    /// Assuming no penalties or changes to the rates, a household should stabilize
    /// around 2.0 adults, 2.5 children, and 0.5 elders.
    /// 
    /// We should also have a growth rate of about 2.0% per turn.
    pub fn baseline() -> Self {
        Self {
            births_per_woman: 0.167,
            infant_mortality: 0.2,
            maternal_mortality: 0.015,
            child_mortality: 0.025,
            adult_mortality: 0.018,
            elder_mortality: 0.08,
            adult_eff: 1.0,
            elder_eff: 0.5,
            child_eff: 0.3,
            culture: 1.0,
            research: 1.0,
        }
    }

    /// # Add Demographic Rates
    /// 
    /// Adds two sets of demographic rates together. 
    /// 
    /// Remember, rates are uncappped in storage, capped in
    pub fn add(&self, other: &Self) -> Self {
        Self {
            births_per_woman: self.births_per_woman + other.births_per_woman,
            infant_mortality: self.infant_mortality + other.infant_mortality,
            maternal_mortality: self.maternal_mortality + other.maternal_mortality,
            child_mortality: self.child_mortality + other.child_mortality,
            adult_mortality: self.adult_mortality + other.adult_mortality,
            elder_mortality: self.elder_mortality + other.elder_mortality,
            adult_eff: self.adult_eff + other.adult_eff,
            elder_eff: self.elder_eff + other.elder_eff,
            child_eff: self.child_eff + other.child_eff,
            culture: self.culture + other.culture,
            research: self.research + other.research,
        }
    }

    // clamp values
    pub const BIRTHS_MINIMUM: f64 = 0.0;
    pub const INFANT_MORTALITY_MINIMUM: f64 = 0.0;
    pub const MATERNAL_MORTALITY_MINIMUM: f64 = 0.0;
    pub const CHILD_MORTALITY_MINIMUM: f64 = 0.0;
    pub const ADULT_MORTALITY_MINIMUM: f64 = 0.0;
    pub const ELDER_MORTALITY_MINIMUM: f64 = 0.0;
    pub const CULTURE_MINIMUM: f64 = 0.0;
    pub const RESEARCH_MINIMUM: f64 = 0.0;
    pub const ADULT_EFF_MINIMUM: f64 = 0.0;

    // clamp helper functions
    #[inline]
    pub fn clamp_births(&self) -> f64 {
        self.births_per_woman.max(Self::BIRTHS_MINIMUM)
    }

    #[inline]
    pub fn clamp_infant_mortality(&self) -> f64 {
        self.infant_mortality.max(Self::INFANT_MORTALITY_MINIMUM)
    }

    #[inline]
    pub fn clamp_maternal_mortality(&self) -> f64 {
        self.maternal_mortality.max(Self::MATERNAL_MORTALITY_MINIMUM)
    }

    #[inline]
    pub fn clamp_child_mortality(&self) -> f64 {
        self.child_mortality.max(Self::CHILD_MORTALITY_MINIMUM)
    }

    #[inline]
    pub fn clamp_adult_mortality(&self) -> f64 {
        self.adult_mortality.max(Self::ADULT_MORTALITY_MINIMUM)
    }

    #[inline]
    pub fn clamp_elder_mortality(&self) -> f64 {
        self.elder_mortality.max(Self::ELDER_MORTALITY_MINIMUM)
    }
}

/// # Household
/// 
/// This is a household, includes current demographic rates (updated as needed), total 
/// household count, and the age category breakdown.
/// 
/// Each turn is treated as a year (calculations are direct multiplications and additions).
/// 
/// Adulthood starts at 20, Elderhood starts at 60.
/// 
/// `Household::count` should always be >= 1.0 excluding immediately after growth phase.
/// If it reaches below 1.0 during the growth phase, the household 'dies' and is removed 
/// immediately thereafter.
/// 
/// TODO: Consider renaming to something more appropriate, maybe just `Households`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Household {
    /// The Demographic rates of the household. Updated as needed.
    pub rates: DemographicRates,
    /// The total number of households. Fractional values allowed.
    /// Total members = count * household_size()
    pub count: f64,

    // Household Breakdown
    /// Number of adults in a household (average).
    pub adults: f64,
    /// Number of elders in a household (average).
    pub elders: f64,
    /// Number of children in a household (average).
    pub children: f64,
}


impl Household {
    /// # New
    /// 
    /// New's up a household with the inputted count, baseline DemographicRates,
    /// and default household (2.0 Adults, 0.5 Elders, 2.5 Children).
    pub fn new(count: f64) -> Self {
        Self {
            count,
            rates: DemographicRates::baseline(),
            adults: 2.0,
            elders: 0.5,
            children: 2.5
        }
    }

    /// # With Household
    /// 
    /// Fluent household setter. This can increase the total member count of the 
    /// Household.
    /// 
    /// Meant to be used immediately after newing a household up.
    pub fn with_household(mut self, adults: f64, elders: f64, children: f64) -> Self {
        self.adults = adults;
        self.elders = elders;
        self.children = children;
        self
    }

    /// # With Demographic Rates
    /// 
    /// Sets the demographic rates for the household fluently.
    pub fn with_demographic_rates(mut self, demographic_rates: DemographicRates) -> Self {
        self.rates = demographic_rates;
        self
    }

    /// # Household Weighted Average
    /// 
    /// Returns the weighted average household member values, (adult, elder, children).
    pub fn household_weighted_average(&self, other: Self) -> (f64, f64, f64) {
        let total_members = self.total_population() + other.total_population();
        if total_members == 0.0 {
            return (0.0, 0.0, 0.0);
        }
        (
            (self.adults * self.count + other.adults * other.count) / total_members,
            (self.elders * self.count + other.elders * other.count) / total_members,
            (self.children * self.count + other.children * other.count) / total_members
        )
    }

    /// # Combine Households
    /// 
    /// Given two households, combine them. Should have the same number of members. Household
    /// weights are combined as a weighted average.
    /// 
    /// Keeps the demographic rates of the first household.
    pub fn combine_households(&self, other: &Self) -> Self {
        let (adults, elders, children) = self.household_weighted_average(other.clone());
        Self {
            count: self.count + other.count,
            rates: self.rates, // or combine if needed
            adults,
            elders,
            children
        }
    }

    /// # Household Size
    /// 
    /// Gets the size of a singular household, adults + elders + children.
    pub fn household_size(&self) -> f64 {
        self.adults + self.elders + self.children
    }

    /// # Members
    /// 
    /// Gets the members of the household. This is the size of the household
    /// times the count of households.
    pub fn total_population(&self) -> f64 {
        self.count * self.household_size()
    }

    /// # Household Labor
    /// 
    /// Gets the total labor of the household produced in a turn.
    pub fn household_labor(&self) -> f64 {
        self.adults * self.rates.adult_eff +
        self.elders * self.rates.elder_eff +
        self.children * self.rates.child_eff
    }

    /// # Total Labor
    /// 
    /// Gets the total labor of all households produced in a turn.
    pub fn total_labor(&self) -> f64 {
        self.household_labor() * self.count
    }

    /// # Total Adults
    /// 
    /// The number of adults in this household.
    pub fn total_adults(&self) -> f64 {
        self.count * self.adults
    }

    /// # Total Elders
    /// 
    /// The number of elders in this household.
    pub fn total_elders(&self) -> f64 {
        self.count * self.elders
    }

    /// # Total Children
    /// 
    /// The number of children in this household.
    pub fn total_children(&self) -> f64 {
        self.count * self.children
    }

    /// # Update
    /// 
    /// Updates the household based on the current demographic rates and other factors.
    /// 
    /// TODO: Consider removing Demographic rates from Household and passing it in here instead.
    pub fn update(&mut self) {
        if self.count <= 0.0 { 
            return;
        }
        
        // current totals
        let mut total_adults = self.total_adults();
        let mut total_elders = self.total_elders();
        let mut total_children = self.total_children();

        // 1. Birth and Maternal Mortality
        let women = total_adults * 0.5;
        let births = women * self.rates.births_per_woman 
            * (1.0 - self.rates.infant_mortality);
        let maternal_deaths = births * self.rates.maternal_mortality;

        // 2. Cagetory Deaths
        let child_deaths = total_children * self.rates.child_mortality;
        let adult_deaths = total_adults * self.rates.adult_mortality;
        let elder_deaths = total_elders * self.rates.elder_mortality;

        // 3. Age Flows
        let child_aging = total_children / 20.0;
        let adult_aging = total_adults / 40.0;

        // 4. Apply results
        total_children += births - child_deaths - child_aging;
        total_adults += child_aging - adult_deaths - adult_aging;
        total_elders += adult_aging - elder_deaths;

        // prevent negatives (may replace with debug_asserts)
        total_children = f64::max(0.0, total_children);
        total_adults = f64::max(0.0, total_adults);
        total_elders = f64::max(0.0, total_elders);

        let new_total = total_adults + total_children + total_elders;
        if new_total <= 0.0 { 
            // everyone died
            self.count = 0.0;
            self.adults = 0.0;
            self.elders = 0.0;
            self.children = 0.0;
            return;
        }

        // convert back to average household.
        let old_size = self.household_size();
        self.count = new_total / old_size;
        
        self.adults = total_adults / new_total;
        self.elders = total_elders / new_total;
        self.children = total_children / new_total;
        
    }
}

impl Default for Household {
    /// # Default
    /// 
    /// Defaults to 1.0 household,
    /// demographic rates equal to baseline,
    /// 2.0 adults, 0.5 elders, and 2.5 children.
    fn default() -> Self {
        Self { 
            count: 1.0,
            rates: DemographicRates::baseline(),
            adults: 2.0,
            elders: 0.5,
            children: 2.5,
        }
    }
}

/// # House Member
/// 
/// A helper enum to select between members of a household
pub enum HouseMember {
    Adult,
    Child,
    Elder
}