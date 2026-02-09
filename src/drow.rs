use crate::{data::Data, household::Household};

/// # Demographic Row
/// 
/// A Demographic row is used by pops to define the amount of a population in a
/// pop group.
/// 
/// Has a column for each 'layer' of information a pop can have.
/// 
/// Currently, we only use one.
#[derive(Debug, Clone, Copy)]
pub struct DRow {
    /// The Household(s) of this Demographic row.
    pub household: Household,
    /// All pops need a base species at all times.
    pub species: usize,
    /// Culture is currently the only additional layer of info for a pop.
    pub culture: Option<usize>,
    // Placeholder for later columns.
}

impl DRow {
    pub fn new(count: f64, species: usize) -> Self {
        let household = Household::zeroed_household().add_count(count);
        Self {
            household,
            species,
            culture: None,
        }
    }

    pub fn has_culture(mut self, culture: usize) -> Self {
        self.culture = Some(culture);
        self
    }

    /// # Update Household
    /// 
    /// Used after setting the demographic parts, updates the household to match species.
    /// 
    /// This does change population as household count is maintained.
    pub fn update_household(&mut self, data: &Data) {
        let mut mods = vec![];
        mods.push(data.get_species(self.species).household_mod);
        if let Some(id) = self.culture {
            mods.push(data.get_culture(id).household_mod);
        }
        self.household.add_mods(mods);
    }
}