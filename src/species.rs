use crate::{desire::Desire, household::{Household, HouseholdMod}};

/// # Species
/// 
/// This is the physical representation of a pop. The biological needs and
/// realities they have.
/// 
/// Currently, this is very simple and focuses on just getting the bones 
/// in place for later.
/// 
/// ## Desire Notes
/// 
/// For a life-need desire, it is advised to put it so that most of it's steps are
/// satisfied before any non-life need desire. This just ensures the pop doesn't
/// slowly starve to death because it wanted new shoes instead of a necissary meal.
pub struct Species {
    /// Unique ID of the species.
    pub id: usize,
    /// Unique name of the species.
    pub name: String,

    /// The 'base' species household, in the form of a household Mod.
    /// 
    /// It should not be below the minimum of of 1.0 adult, 0.0 Childern, and 0.0 Elders.
    /// 
    /// Assume this is being added to a house with no members at all.
    pub household_mod: HouseholdMod,

    /// The desires that the species biologically needs or wants.
    /// Desires are sorted by their starting value, lowest to highest.
    pub desires: Vec<Desire>,

    // TODO: Placeholder spot for Species modifiers
    // TODO: Placeholder spot for Tech tied to species.
    // TODO: Placeholder for Additional rule tags.
}

impl Species {
    pub fn new(id: usize, name: String) -> Self {
        Species {
            id,
            name,
            household_mod: HouseholdMod::default_household(),
            desires: vec![],
        }
    }

    /// # With Household
    /// 
    /// Sets the household of the species using household mod.
    pub fn with_household(mut self, household: HouseholdMod) -> Self {
        self.household_mod = household;
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
            if des.starting_value < desire.starting_value {
                index = idx;
            }
        }
        self.desires.insert(index, desire);
        self
    }
}