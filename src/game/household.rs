/// # Household Definition
/// 
/// Defines a a baseline for a household in a culture, and includes additional
/// effects applied to a household.
#[derive(Debug, Clone, Copy)]
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
    pub fn default() -> Self {
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

    /// # Size
    /// 
    /// The total size of the house, adults, elders, and children.
    pub fn size(&self) -> f64 {
        self.adults + self.elders + self.children
    }
}

/// # Household
/// 
/// This is a functional household, used by pops to define the households that make 
/// them up.
#[derive(Debug, Clone, Copy)]
pub struct Household {
    pub def: HouseholdDef,
    pub count: f64,
}