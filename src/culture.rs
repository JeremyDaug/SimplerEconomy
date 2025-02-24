use crate::desire::Desire;


/// # Culture
/// 
/// A common storage place for data used by pops. Currently only covers 
/// culture, species, and other factors are ignored and will need their own
/// storage most likely.
/// 
/// This currently only stores the desires of the pop.
pub struct Culture {
    /// The unique id of the culture.
    pub id: usize,
    /// The unique name of the culture.
    pub name: String,
    /// The desire track of the culture.
    pub desires: Vec<Desire>,
    /// A multiplier on household size.
    /// 
    /// This should be broken up into adult, child, and elder modifiers.
    pub household_modifier: f64,
    /// An additive bonus or malus on birthrate.
    pub birthrate: f64,
    /// An additive bonus or malus on mortality.
    pub mortality: f64,
    // TODO: Culture Modifiers
    // TODO: Culture Tech Storage
}

impl Culture {
    pub fn new(id: usize, name: String) -> Culture {
        Culture {
            id,
            name,
            household_modifier: 1.0,
            birthrate: 0.0,
            mortality: 0.0,
            desires: vec![],
        }
    }

    /// # With Household
    /// 
    /// Sets household size modification for culture.
    /// Values greater than 1.0 results in an increase.
    /// Negative in a decrease.
    /// 
    /// ## Note
    /// 
    /// This should never reduce the household below 1, but this is not enforced.
    /// 
    /// # Panics
    /// 
    /// Household must be a positive value.
    pub fn with_household(mut self, household: f64) -> Self {
        assert!(household > 0.0, "Base Efficiency must be positive.");
        self.household_modifier = household;
        self
    }

    /// # With Birthrate
    /// 
    /// Sets Birthrate modifier for culture.
    /// 
    /// # Notes
    /// 
    /// Birthrate mod should never result in a negative birthrate, but we 
    /// won't enforce that here.
    pub fn with_birthrate(mut self, birthrate_mod: f64) -> Self {
        self.birthrate = birthrate_mod;
        self
    }

    /// # With Mortality
    /// 
    /// Sets Mortality modifier for culture.
    /// 
    /// # Notes
    /// 
    /// Mortality mod should never result in a negative Mortality, but we 
    /// won't enforce that here.
    pub fn with_mortality(mut self, mortality_mod: f64) -> Self {
        self.mortality = mortality_mod;
        self
    }

    /// # With Desire
    /// 
    /// Inserts desire inot proper place.
    pub fn with_desire(mut self, desire: Desire) -> Self {
        // find where to insert it.
        let mut index = 0; 
        for (idx, des) in self.desires.iter().enumerate() {
            if des.start < desire.start {
                index = idx;
            }
        }
        self.desires.insert(index, desire);
        self
    }

    
}