use std::collections::HashMap;

use crate::game::{
    culture::Culture,
    desire::{DemoDesire, Desire, DesireSource},
    good::Good,
    process::Process,
    religion::Religion,
    species::Species,
};

/// # Factuals
/// 
/// This is where all the 'facts' of the world are stored, such as what goods and 
/// processes exist, these rarely, if ever change, and should even be mostly the same
/// between games.
/// 
/// This should include Goods, Processes, Game Rules, etc.
/// 
/// This is as compared to 'game state' which is the current state fo the world in a 
/// given game, such as the map, players, goods in the market, prices, etc.
#[derive(Debug, Clone)]
pub struct Factuals {
    pub goods: HashMap<usize, Good>,
    pub processes: HashMap<usize, Process>,
    pub species: HashMap<usize, Species>,
    pub cultures: HashMap<usize, Culture>,
    pub religion: HashMap<usize, Religion>,
}

impl Factuals {
    /// # New
    /// 
    /// News up empty data.
    pub fn new() -> Self {
        Factuals {
            goods: HashMap::new(),
            processes: HashMap::new(),
            cultures: HashMap::new(),
            species: HashMap::new(),
            religion: HashMap::new(),
        }
    }

    /// Adds a good; panics if its ID is already present.
    pub fn with_good(mut self, good: Good) -> Self {
        let id = good.id;
        if self.goods.contains_key(&id) {
            panic!("Good ID {} already exists in factuals.", id);
        }
        self.goods.insert(id, good);
        self
    }

    /// Adds a process; panics if its ID is already present.
    pub fn with_process(mut self, process: Process) -> Self {
        let id = process.id;
        if self.processes.contains_key(&id) {
            panic!("Process ID {} already exists in factuals.", id);
        }
        self.processes.insert(id, process);
        self
    }

    /// Adds a species; panics if its ID is already present.
    pub fn with_species(mut self, species: Species) -> Self {
        let id = species.id;
        if self.species.contains_key(&id) {
            panic!("Species ID {} already exists in factuals.", id);
        }
        self.species.insert(id, species);
        self
    }

    /// Adds a culture; panics if its ID is already present.
    pub fn with_culture(mut self, culture: Culture) -> Self {
        let id = culture.id;
        if self.cultures.contains_key(&id) {
            panic!("Culture ID {} already exists in factuals.", id);
        }
        self.cultures.insert(id, culture);
        self
    }

    /// Adds a religion; panics if its ID is already present.
    pub fn with_religion(mut self, religion: Religion) -> Self {
        let id = religion.id;
        if self.religion.contains_key(&id) {
            panic!("Religion ID {} already exists in factuals.", id);
        }
        self.religion.insert(id, religion);
        self
    }

    /// Looks up a species by id. Panics if missing.
    pub fn find_species(&self, id: usize) -> &Species {
        self.species.get(&id)
            .unwrap_or_else(|| panic!("Species {id} missing from factuals."))
    }

    /// # Clear Household Changed Flags
    ///
    /// After every pop has run [`crate::game::pop::Pop::demographic_update`], clear
    /// the shared demographic `household_changed` flags so the next day does not
    /// rebuild households from a stale signal.
    pub fn clear_household_changed_flags(&mut self) {
        for species in self.species.values_mut() {
            species.household_changed = false;
        }
        for culture in self.cultures.values_mut() {
            culture.household_changed = false;
        }
        for religion in self.religion.values_mut() {
            religion.household_changed = false;
        }
    }

    /// Looks up a culture by id. Panics if missing.
    pub fn find_culture(&self, id: usize) -> &Culture {
        self.cultures.get(&id)
            .unwrap_or_else(|| panic!("Culture {id} missing from factuals."))
    }

    /// Looks up a religion by id. Panics if missing.
    pub fn find_religion(&self, id: usize) -> &Religion {
        self.religion.get(&id)
            .unwrap_or_else(|| panic!("Religion {id} missing from factuals."))
    }

    /// # Source Demo Desire
    ///
    /// Resolves the demographic desire behind a pop `Desire` via `desire.source`
    /// (`source_id`, `demo_desire_id`). Class is not implemented yet.
    pub fn source_demo_desire(&self, desire: &Desire) -> Option<&DemoDesire> {
        match desire.source {
            DesireSource::Species(source_id, demo_id) => {
                self.find_species(source_id).find_desire(demo_id)
            }
            DesireSource::Culture(source_id, demo_id) => {
                self.find_culture(source_id).find_desire(demo_id)
            }
            DesireSource::Religion(source_id, demo_id) => {
                self.find_religion(source_id).find_desire(demo_id)
            }
            DesireSource::Class(source_id, _demo_id) => {
                todo!("Class desires are not supported yet (class id {source_id}).");
                #[allow(unreachable_code)]
                None
            }
        }
    }

    pub(crate) fn find_good(&self, id: usize) -> &Good {
        self.goods.get(&id)
            .unwrap_or_else(|| panic!("Good {id} missing from factuals."))
    }
}

#[cfg(test)]
mod factuals_should {
    use super::*;
    use crate::game::{culture::Culture, religion::Religion, species::Species};

    #[test]
    fn clear_household_changed_flags_resets_all_demographics() {
        let mut species = Species::new(0, "Human");
        species.household_changed = true;
        let mut culture = Culture::new(1, "C");
        culture.household_changed = true;
        let mut religion = Religion::new(2, "R");
        religion.household_changed = true;

        let mut factuals = Factuals::new()
            .with_species(species)
            .with_culture(culture)
            .with_religion(religion);

        factuals.clear_household_changed_flags();

        assert!(!factuals.species[&0].household_changed);
        assert!(!factuals.cultures[&1].household_changed);
        assert!(!factuals.religion[&2].household_changed);
    }
}