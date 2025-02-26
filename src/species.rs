use crate::{desire::Desire, household::{Household, HouseholdMod}};

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

    /// The 'base' species household. What is most likely to occur in nature.
    /// This should have a size of 0.0, and a positive household size.
    /// 
    /// It's broken up into adults, children, and elders which define the makeup.
    pub household: HouseholdMod,

    /// The desires that the species naturally desires.
    /// Desires are sorted by their starting value, lowest to highest.
    pub desires: Vec<Desire>,

    // TODO: Placeholder spot for Species modifiers
    // TODO: Placeholder spot for Tech tied to species.
}

impl Species {
    pub fn new(id: usize, name: String) -> Self {
        Species {
            id,
            name,
            household: HouseholdMod::default_household(),
            desires: vec![],
        }
    }

    /// # With Household
    /// 
    /// Sets the household of the species using household mod.
    pub fn with_household(mut self, household: HouseholdMod) -> Self {
        self.household = household;
        self
    }

    /// # With Desire
    /// 
    /// Inserts desire into proper place.
    pub fn with_desire(mut self, desire: Desire) -> Self {
        // TODO: Consider adding checks to filter out bad desires here!
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