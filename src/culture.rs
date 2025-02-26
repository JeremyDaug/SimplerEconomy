use crate::{desire::Desire, household::{Household, HouseholdMod}};


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

    /// The effects on a household a culture has. This should be a cound 0.0 
    /// household.
    /// 
    /// # V 1.0 Note
    /// 
    /// This should never push any group below 0.0, or the household down below 1.0 size.
    pub household_mod: HouseholdMod,
    /// The desire track of the culture.
    pub desires: Vec<Desire>,
    // TODO: Culture Modifiers
    // TODO: Culture Tech Storage
}

impl Culture {
    pub fn new(id: usize, name: String) -> Culture {
        Culture {
            id,
            name,
            household_mod: HouseholdMod {
                adults: 0.0,
                elders: 0.0,
                children: 0.0,
            },
            desires: vec![],
        }
    }

    /// # With Household Modification
    /// 
    /// Sets the changes to a household that this culture makes.
    pub fn with_household_mod(mut self, household_mod: HouseholdMod) -> Self {
        self.household_mod = household_mod;
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