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
    // NOTE: Placeholder spot for Species modifiers
}

impl Species {
    pub fn new(id: usize, name: String) -> Self {
        Species {
            id,
            name,
            desires: vec![],
        }
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