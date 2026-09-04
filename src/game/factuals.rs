use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::game::{
    culture::Culture, desire::{DemoDesire, Desire, DesireSource}, good::Good, household::DemographicRates, pop::DemoRow, process::Process, religion::Religion, species::Species,
};

/// TOML world-data file of goods (factuals).
#[derive(Debug, Deserialize)]
struct GoodsFile {
    goods: Vec<Good>,
}

/// Failed to load factuals from a world-data file.
#[derive(Debug)]
pub enum FactualsLoadError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    DuplicateGood(usize),
}

impl fmt::Display for FactualsLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "read world goods: {err}"),
            Self::Toml(err) => write!(f, "parse world goods: {err}"),
            Self::DuplicateGood(id) => write!(f, "duplicate good id {id} in world goods"),
        }
    }
}

impl std::error::Error for FactualsLoadError {}

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

    /// Loads goods from a TOML world-data file into an empty [`Factuals`].
    /// Processes, species, cultures, and religions stay empty.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, FactualsLoadError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(FactualsLoadError::Io)?;
        Self::load_from_toml(&text)
    }

    /// Loads goods from TOML text into an empty [`Factuals`].
    pub fn load_from_toml(text: &str) -> Result<Self, FactualsLoadError> {
        let file: GoodsFile = toml::from_str(text).map_err(FactualsLoadError::Toml)?;
        let mut factuals = Factuals::new();
        for good in file.goods {
            if factuals.goods.contains_key(&good.id) {
                return Err(FactualsLoadError::DuplicateGood(good.id));
            }
            factuals.goods.insert(good.id, good);
        }
        Ok(factuals)
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
    /// After every pop has run [`crate::game::pop::Pop::update_desires`], clear
    /// the shared demographic `household_changed` flags so the next day does not
    /// rebuild households again.
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
    
    /// # Get Demographic Rates
    ///
    /// Resolve structural demographic rates for a pop's demographic ids:
    /// `baseline + species_demo_eff + culture_demo_eff + religion_demo_eff`
    /// (culture/religion id `0` means none and is skipped). Class is not folded in yet.
    ///
    /// ## Policy: recompute every call (no cache)
    ///
    /// Rates are **not** stored on the pop and are **not** memoized here. Each caller
    /// (typically once per pop per growth phase) recomputes from the current factual
    /// deltas. That keeps results always fresh under parallel `&Factuals` reads
    /// (e.g. rayon growth) without locks or invalidation.
    ///
    /// Cost is a few map lookups and a small `DemographicRates::add` chain. Unique
    /// demographic combos are usually far fewer than pop count; the same combo may
    /// be recomputed many times in one day when many pops share it.
    ///
    /// ## If this becomes too slow (large pop counts)
    ///
    /// Prefer a **day-fill cache of living combos only** (not the full species x
    /// culture x class x religion product):
    /// - Key: demographic ids only (not job, not household composition).
    /// - Sequential phase: ensure cache entries for every live key (or scan pops once).
    /// - Growth: `&self` lookup only (no interior mutability on the hot path).
    /// - Invalidate when any `*_demo_eff` / baseline changes.
    ///
    /// Lazy fill under parallel growth is also possible (`RwLock`/`DashMap`) but is
    /// more complex than day-fill for this turn loop. See
    /// `docs/proposals/household-population-refactor-primer.md`.
    pub(crate) fn get_demographic_rates(&self, demographics: DemoRow) -> DemographicRates {
        // Intentional: no cache. See doc above if profiling shows this hot.
        let mut rates = DemographicRates::baseline();
        if let Some(species) = self.species.get(&demographics.species) {
            rates = rates.add(&species.species_demo_eff);
        }
        if demographics.culture != 0 {
            if let Some(culture) = self.cultures.get(&demographics.culture) {
                rates = rates.add(&culture.culture_demo_eff);
            }
        }
        if demographics.religion != 0 {
            if let Some(religion) = self.religion.get(&demographics.religion) {
                rates = rates.add(&religion.religion_demo_eff);
            }
        }
        rates
    }
}

#[cfg(test)]
mod factuals_should {
    use super::*;
    use crate::game::good::GoodTag;
    use crate::game::{culture::Culture, religion::Religion, species::Species};
    use std::path::PathBuf;

    fn repo_goods_file() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world/goods.toml")
    }

    #[test]
    fn load_from_toml_reads_cli_goods() {
        let factuals = Factuals::load_from_toml(
            r#"
[[goods]]
id = 1
name = "grain"
mass = 1.0
volume = 1.0
"#,
        )
        .expect("toml");
        let grain = factuals.find_good(1);
        assert_eq!(grain.name, "grain");
        assert_eq!(grain.mass, 1.0);
        assert_eq!(grain.volume, 1.0);
        assert!(grain.tags.is_empty());
        assert!(factuals.processes.is_empty());
    }

    #[test]
    fn load_from_toml_reads_tags() {
        let factuals = Factuals::load_from_toml(
            r#"
[[goods]]
id = 9
name = "cargo"
mass = 0.0
volume = 0.0
tags = ["untradeable", { transport = 2.0 }]
"#,
        )
        .expect("toml");
        let cargo = factuals.find_good(9);
        assert!(cargo.tags.contains(&GoodTag::Untradeable));
        assert_eq!(cargo.transport_efficiency(), 2.0);
    }

    #[test]
    fn load_from_path_reads_the_world_goods_file() {
        let factuals = Factuals::load_from_path(repo_goods_file()).expect("world goods");
        assert_eq!(factuals.goods.len(), 6);
        assert_eq!(factuals.find_good(1).name, "grain");
        assert_eq!(factuals.find_good(2).name, "water");
        assert_eq!(factuals.find_good(3).name, "bread");
        assert_eq!(factuals.find_good(4).name, "gold");
        assert_eq!(factuals.find_good(5).name, "coin");
        assert_eq!(factuals.find_good(6).name, "jewelry");
    }

    #[test]
    fn load_from_toml_errors_on_duplicate_id() {
        let err = Factuals::load_from_toml(
            r#"
[[goods]]
id = 1
name = "grain"
mass = 1.0
volume = 1.0

[[goods]]
id = 1
name = "also grain"
mass = 1.0
volume = 1.0
"#,
        )
        .expect_err("duplicate");
        match err {
            FactualsLoadError::DuplicateGood(1) => {}
            other => panic!("expected DuplicateGood(1), got {other}"),
        }
    }

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