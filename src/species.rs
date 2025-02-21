use crate::desire::Desire;

/// # Species
/// 
/// This is the physical representation of a pop. The biological needs and
/// realities they have.
/// 
/// Currently, this is very simple and focuses on just getting the bones 
/// in place for later.
pub struct Species {
    /// Unique ID of the species.
    pub id: usize,
    /// Unique name of the species.
    pub name: String,
    /// The desires that the species naturally desires.
    /// Desires are sorted by their starting value, lowest to highest.
    pub desires: Vec<Desire>,
    /// The 'default' household size for a species. This is a measure of all
    /// the people in a household, adults, elders, and children.
    /// NOTE: This will need tobe broken up further into who is were 
    pub household: f64,
    /// The default birthrate of a species. Must be non-negative.
    pub birthrate: f64,
    /// The default mortality of a species. Must be non-negative.
    pub mortality: f64,
    // TODO: Placeholder spot for Species modifiers
    // TODO: Placeholder spot for Tech tied to species.
}

impl Species {
    pub fn new(id: usize, name: String) -> Self {
        Species {
            id,
            name,
            household: 1.0,
            birthrate: 0.0,
            mortality: 0.0,
            desires: vec![],
        }
    }

    /// # With Base Efficiency
    /// 
    /// Sets and checks base efficiency fluently.
    /// 
    /// # Panics
    /// 
    /// Base Efficiency must be greater than 0.0.
    pub fn with_household_size(mut self, base_eff: f64) -> Self {
        assert!(base_eff > 0.0, "Base Efficiency must be a positive number.");
        self.base_efficiency = base_eff;
        self
    }
    
    /// # With Desire
    /// 
    /// Inserts desire into proper place.
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