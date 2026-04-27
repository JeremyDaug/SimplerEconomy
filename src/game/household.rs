/// # Household Definition
/// 
/// Defines a a baseline for a household in a culture, and includes additional
/// effects applied to a household.
/// 
/// Adults/Elders/Children is how many of that category are in a household.
/// 
/// Efficiency is how much labor 1 member of that group adds to the total daily labor.
/// 
/// Birth Rate and Mortality rate is the rate of change (Positive and negative) a 
/// household each turn (change is the sum of the two values). This rate may be modified
/// up or down to make growth feel good without becoming overwhelming.
/// 
/// Culture and Research Rates are the passive generation of each in the household.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HouseholdDef {
    /// The number of adults in the household.
    pub adults: f64,
    /// Labor Efficiency of the adults.
    pub adult_eff: f64,
    
    /// Number of Elders in the household.
    pub elders: f64,
    /// Elder labor Efficiency
    pub elder_eff: f64,

    /// Number of Children in the household.
    pub children: f64,
    /// Child Labor Efficiency.
    pub child_eff: f64,

    /// Birth Rate
    pub birth_rate: f64,
    /// Mortality Rate
    pub mortality_rate: f64,

    /// Passive Research Rate
    pub research_rate: f64,
    /// Passive Culture Rate
    pub culture_rate: f64,
}

impl HouseholdDef {
    /// # Size
    /// 
    /// The total size of the house, adults, elders, and children.
    pub fn size(&self) -> f64 {
        self.adults + self.elders + self.children
    }

    /// # Weighted Average
    /// 
    /// Calculates the weighted average between 2 HouseholdDefs. 
    /// 
    /// NOTE: Not Tested
    pub fn weighted_average(&self, weight1: f64, other: &Self, weight2: f64) -> Self {
        assert!(weight1 > 0.0, "Weight1 Must be Positive Value.");
        assert!(weight2 > 0.0, "Weight2 Must be Positive Value.");
        let sum = weight1 + weight2;
        let w_self = weight1 / sum;
        let w_other = weight2 / sum;

        Self {
            adults: self.adults * w_self + other.adults * w_other,
            adult_eff: self.adult_eff * w_self + other.adult_eff * w_other,

            elders: self.elders * w_self + other.elders * w_other,
            elder_eff: self.elder_eff * w_self + other.elder_eff * w_other,

            children: self.children * w_self + other.children * w_other,
            child_eff: self.child_eff * w_self + other.child_eff * w_other,

            birth_rate: self.birth_rate * w_self + other.birth_rate * w_other,
            mortality_rate: self.mortality_rate * w_self + other.mortality_rate * w_other,

            research_rate: self.research_rate * w_self + other.research_rate * w_other,
            culture_rate: self.culture_rate * w_self + other.culture_rate * w_other,
        }
    }

    /// # Labor
    /// 
    /// The labor produced by the house
    pub fn labor(&self) -> f64 {
        self.adults * self.adult_eff + 
        self.elders * self.elder_eff +
        self.children * self.child_eff
    }
}

impl Default for HouseholdDef {
    /// # Default
    /// 
    /// Produces the default household Definition.
    /// 
    /// 2 Adults, 2.5 Children, 0.5 Elders
    /// 1.0 Adult, 0.3 Child, and 0.5 Elder Efficiency
    /// 
    /// Birth rate of 2.5%
    /// Mortality of 0.5%
    /// 
    /// Passive Research and Culture of 0.5 each.
    fn default() -> Self {
        Self {
            adults: 2.0,
            adult_eff: 1.0,
            elders: 0.5,
            elder_eff: 0.5,
            children: 2.5,
            child_eff: 0.3,
            birth_rate: 0.025,
            mortality_rate: 0.005,
            research_rate: 0.25,
            culture_rate: 0.25,
        }
    }

}

/// # Household
/// 
/// This is a functional household, used by pops to define the households that make 
/// them up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Household {
    pub def: HouseholdDef,
    pub count: f64,
}


impl Household {
    /// # New
    /// 
    /// New's up a household with the inputted count and a default HouseholdDef.
    pub fn new(count: f64) -> Self {
        Self {
            count,
            ..Default::default()
        }
    }

    /// # With Household
    /// 
    /// Fluent household setter. This can increase the total member count of the 
    /// Household.
    /// 
    /// Meant to be used immediately after newing a household up.
    pub fn with_household(mut self, household_def: HouseholdDef) -> Self {
        self.def = household_def;
        self
    }

    /// # Add Household
    /// 
    /// Adds two household together. The result should be a household that has the same
    /// number of members as the originals, and the household def should be the weighted
    /// average.
    pub fn add_household(&self, household: Household) -> Self {
        let members = self.members() + household.members();
        let new_house_def = self.def.weighted_average(self.members(), 
            &household.def, household.members());
        Self::new(members / new_house_def.size())
            .with_household(new_house_def)
    }

    /// # Add Members
    /// 
    /// Adds members to the household, increasing count scaled by household size.
    pub fn add_members(&mut self, member: f64) {
        let a = member / self.def.size();
        self.count += a;
    }

    /// # Alter household, Maintain Members
    /// 
    /// Alters the household definition, but also alters the count so the total members
    /// stay the same.
    pub fn alter_household_maintain_members(&self, household_def: HouseholdDef) -> Self {
        // get the total members at start
        let new_count = self.members() / household_def.size();
        Self::new(new_count).with_household(household_def)
    }

    /// # Members
    /// 
    /// Gets the members of the houusehold. This is the size of the household
    /// times the count of households.
    pub fn members(&self) -> f64 {
        self.count * self.def.size()
    }

    /// # Labor
    /// 
    /// Gets the total labor of the household produced in a turn.
    pub fn labor(&self) -> f64 {
        self.count * self.def.labor()
    }

    /// # Adults
    /// 
    /// The number of adults in this household.
    pub fn adults(&self) -> f64 {
        self.count * self.def.adults
    }

    /// # Elders
    /// 
    /// The number of elders in this household.
    pub fn elders(&self) -> f64 {
        self.count * self.def.elders
    }

    /// # Children
    /// 
    /// The number of children in this household.
    pub fn children(&self) -> f64 {
        self.count * self.def.children
    }
}

impl Default for Household {
    fn default() -> Self {
        Self { 
            def: Default::default(), 
            count: Default::default() 
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