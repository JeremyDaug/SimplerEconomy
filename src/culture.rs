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
    /// Base efficiency, a flat multiplier of time produced at the start of 
    /// each market turn.
    pub base_efficiency: f64,
    // TODO: Culture Modifiers
    // TODO: Culture Tech Storage
}

impl Culture {
    pub fn new(id: usize, name: String) -> Culture {
        Culture {
            id,
            name,
            base_efficiency: 1.0,
            desires: vec![],
        }
    }

    /// # With Efficiency
    /// 
    /// Sets Efficiency for culture.
    /// 
    /// # Panics
    /// 
    /// Efficiency must be a positive value.
    pub fn with_efficiency(mut self, eff: f64) -> Self {
        assert!(eff > 0.0, "Base Efficiency must be positive.");
        self.base_efficiency = eff;
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