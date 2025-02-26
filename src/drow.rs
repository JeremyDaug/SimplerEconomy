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
    /// How many households have this data.
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

    pub fn birthrate(self, data: &Data) -> f64 {
        let base = data.get_species(self.species).birthrate;
        let culture = if let Some(culture_id) = self.culture {
            data.get_culture(culture_id).birthrate
        } else { 0.0 };
        (base + culture).max(0.0)
    }

    pub fn mortality(self, data: &Data) -> f64 {
        let base = data.get_species(self.species).mortality;
        let culture = if let Some(culture_id) = self.culture {
            data.get_culture(culture_id).mortality
        } else { 0.0 };
        (base + culture).max(0.0)
    }
    
    pub fn household_mult(self, data: &Data) -> f64 {
        let base = data.get_species(self.species).household;
        let culture = if let Some(culture_id) = self.culture {
            data.get_culture(culture_id).household_modifier
        } else { 1.0 };
        (base * culture).max(1.0)
    }
}