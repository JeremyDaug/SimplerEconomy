use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use crate::game::{
    config::{ConfigLoadError, GameConfig},
    culture::Culture,
    demographic_source::DemographicSource,
    desire::{DemoDesire, Desire},
    good::{Good, GoodLoadError},
    household::{BirthSentiment, DemographicRates},
    pop_property::DemoRow,
    process::{Process, ProcessLoadError},
    species::Species,
};

/// Failed to load factuals from a world-data file.
#[derive(Debug)]
pub enum FactualsLoadError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    DuplicateGood(usize),
    DuplicateProcess(usize),
    DuplicateProcessInput { process: usize, good: usize },
    InvalidProcess(String),
    Config(ConfigLoadError),
}

impl fmt::Display for FactualsLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "read world data: {err}"),
            Self::Toml(err) => write!(f, "parse world data: {err}"),
            Self::DuplicateGood(id) => write!(f, "duplicate good id {id} in world data"),
            Self::DuplicateProcess(id) => write!(f, "duplicate process id {id} in world data"),
            Self::DuplicateProcessInput { process, good } => write!(f, "process {process} repeats input good {good}"),
            Self::InvalidProcess(msg) => write!(f, "{msg}"),
            Self::Config(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for FactualsLoadError {}

impl From<GoodLoadError> for FactualsLoadError {
    fn from(err: GoodLoadError) -> Self {
        match err {
            GoodLoadError::Io(err) => Self::Io(err),
            GoodLoadError::Toml(err) => Self::Toml(err),
            GoodLoadError::Duplicate(id) => Self::DuplicateGood(id),
        }
    }
}

impl From<ProcessLoadError> for FactualsLoadError {
    fn from(err: ProcessLoadError) -> Self {
        match err {
            ProcessLoadError::Io(err) => Self::Io(err),
            ProcessLoadError::Toml(err) => Self::Toml(err),
            ProcessLoadError::Duplicate(id) => Self::DuplicateProcess(id),
            ProcessLoadError::DuplicateInput { process, good } => {
                Self::DuplicateProcessInput { process, good }
            }
            ProcessLoadError::Invalid(msg) => Self::InvalidProcess(msg),
        }
    }
}

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
    /// Gameplay tunables. Loaded from `config.toml` when reading a world folder.
    pub config: GameConfig,
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
            config: GameConfig::default(),
        }
    }

    /// # Load From Path
    ///
    /// `path` is a world-data file or directory.
    ///
    /// A directory is loaded by [`Self::load_from_dir`]. A file is read and
    /// loaded by [`Self::load_from_toml`]. Returns those [`Factuals`], or the
    /// first error. Species stay empty.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, FactualsLoadError> {
        let path = path.as_ref();
        if path.is_dir() {
            Self::load_from_dir(path)
        } else {
            let text = std::fs::read_to_string(path).map_err(FactualsLoadError::Io)?;
            Self::load_from_toml(&text)
        }
    }

    /// # Load From Dir
    ///
    /// `dir` is a world-data folder.
    ///
    /// Reads `goods.toml` through [`Good::load_from_path`], then
    /// `processes.toml` through [`Process::load_from_path`] when that file
    /// is present, then `config.toml` when present. `config.toml` replaces
    /// [`Self::config`]. Returns the [`Factuals`], or the first error.
    fn load_from_dir(dir: &Path) -> Result<Self, FactualsLoadError> {
        let mut factuals = Self::new();
        factuals.goods = Good::load_from_path(dir.join("goods.toml"))?;
        let processes_path = dir.join("processes.toml");
        if processes_path.exists() {
            factuals.processes = Process::load_from_path(&processes_path, &factuals.goods)?;
        }
        let config_path = dir.join("config.toml");
        if config_path.exists() {
            factuals.config =
                GameConfig::load_from_path(&config_path).map_err(FactualsLoadError::Config)?;
        }
        Ok(factuals)
    }

    /// # Load From Toml
    ///
    /// `text` is one TOML document.
    ///
    /// Starts from [`Self::new`] and merges that document through
    /// [`Self::insert_world_file`]. Returns the [`Factuals`], or the first error.
    pub fn load_from_toml(text: &str) -> Result<Self, FactualsLoadError> {
        let mut factuals = Factuals::new();
        factuals.insert_world_file(text)?;
        Ok(factuals)
    }

    /// # Insert World File
    ///
    /// `text` is one TOML document merged into these factuals.
    ///
    /// Loads goods through [`Good::load_from_toml`], then processes through
    /// [`Process::load_from_toml`]. A duplicate against goods or processes
    /// already stored here is an error. Returns the first error.
    fn insert_world_file(&mut self, text: &str) -> Result<(), FactualsLoadError> {
        let goods = Good::load_from_toml(text)?;
        for id in goods.keys() {
            if self.goods.contains_key(id) {
                return Err(FactualsLoadError::DuplicateGood(*id));
            }
        }
        // Validate processes against goods already stored plus this document,
        // before either map is changed.
        let mut known = self.goods.clone();
        known.extend(goods.clone());
        let processes = Process::load_from_toml(text, &known)?;
        for id in processes.keys() {
            if self.processes.contains_key(id) {
                return Err(FactualsLoadError::DuplicateProcess(*id));
            }
        }
        self.goods.extend(goods);
        self.processes.extend(processes);
        Ok(())
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

    /// # With Species
    ///
    /// Stores `species` and returns these factuals.
    ///
    /// Panics when that id is already stored.
    pub fn with_species(mut self, species: Species) -> Self {
        let id = species.id;
        if self.species.contains_key(&id) {
            panic!("Species ID {} already exists in factuals.", id);
        }
        self.species.insert(id, species);
        self
    }

    /// # With Culture
    ///
    /// Stores `culture` and returns these factuals.
    ///
    /// Panics when that id is already stored.
    pub fn with_culture(mut self, culture: Culture) -> Self {
        let id = culture.id;
        if self.cultures.contains_key(&id) {
            panic!("Culture ID {} already exists in factuals.", id);
        }
        self.cultures.insert(id, culture);
        self
    }

    /// # Get Good
    ///
    /// The good stored under `id`.
    ///
    /// Returns `None` when the world has no good with that id.
    pub fn get_good(&self, id: usize) -> Option<&Good> {
        self.goods.get(&id)
    }

    /// # Durability
    ///
    /// [`Good::durability`] for `good`.
    ///
    /// A missing good is 0. Returns that durability.
    pub fn durability(&self, good: usize) -> f64 {
        self.get_good(good).map(|good| good.durability()).unwrap_or(0.0)
    }

    /// # Is For Sale
    ///
    /// Whether `good` can be bought.
    ///
    /// A missing good cannot. [`Good::is_buyable`] decides a loaded good.
    /// Returns that check.
    pub fn is_for_sale(&self, good: usize) -> bool {
        self.get_good(good).is_some_and(|good| good.is_buyable())
    }

    /// # May Leave Market
    ///
    /// Whether a shipment may carry `good` to another market.
    ///
    /// A missing good may not. A loaded good uses [`Good::may_leave_market`].
    /// A trade on the good's own tile does not ask this. Returns that check.
    pub fn may_leave_market(&self, good: usize) -> bool {
        self.get_good(good).is_some_and(|good| good.may_leave_market())
    }

    /// # Has Transport
    ///
    /// Whether any stored good pays transport.
    ///
    /// [`Good::is_transport`] decides a good. Returns that check.
    pub fn has_transport(&self) -> bool {
        self.goods.values().any(|good| good.is_transport())
    }

    /// # Get Process
    ///
    /// The process stored under `id`.
    ///
    /// Returns `None` when the world has no process with that id. A job line
    /// can name a process that is not loaded, and that line does not run.
    pub fn get_process(&self, id: usize) -> Option<&Process> {
        self.processes.get(&id)
    }

    /// # Each Process
    ///
    /// Every process stored in this world.
    ///
    /// Map order. One id is [`Self::get_process`].
    pub fn each_process(&self) -> impl Iterator<Item = &Process> {
        self.processes.values()
    }

    /// # Each Good
    ///
    /// Every good stored in this world.
    ///
    /// Map order. One id is [`Self::get_good`].
    pub fn each_good(&self) -> impl Iterator<Item = &Good> {
        self.goods.values()
    }

    /// # Find Good
    ///
    /// The good stored under `id`.
    ///
    /// Panics when the world has no good with that id.
    pub(crate) fn find_good(&self, id: usize) -> &Good {
        self.goods
            .get(&id)
            .unwrap_or_else(|| panic!("Good {id} missing from factuals."))
    }

    /// # Find Species
    ///
    /// The species stored under `id`. Panics if it is missing.
    pub fn find_species(&self, id: usize) -> &Species {
        self.species
            .get(&id)
            .unwrap_or_else(|| panic!("Species {id} missing from factuals."))
    }

    /// # Find Culture
    ///
    /// The culture stored under `id`. Panics if it is missing.
    pub fn find_culture(&self, id: usize) -> &Culture {
        self.cultures
            .get(&id)
            .unwrap_or_else(|| panic!("Culture {id} missing from factuals."))
    }

    /// # Source Demo Desire
    ///
    /// The demographic desire behind a pop `desire`, from its source and
    /// demo desire id.
    ///
    /// Returns `None` when that holder or that desire is not stored.
    pub fn source_demo_desire(&self, desire: &Desire) -> Option<&DemoDesire> {
        let demo_id = desire.demo_desire_id;
        match desire.source {
            DemographicSource::Species(source_id) => self
                .species
                .get(&source_id)
                .and_then(|species| species.find_desire(demo_id)),
            DemographicSource::Culture(source_id) => self
                .cultures
                .get(&source_id)
                .and_then(|culture| culture.find_desire(demo_id)),
            // Religion records are not loaded.
            DemographicSource::Religion(_) => None,
        }
    }

    /// # Get Demographic Rates
    ///
    /// Structural rates for `demographics`: baseline, then species and culture.
    ///
    /// Culture id `0` is skipped. A missing record is skipped. Each call adds
    /// the stored rate bundles. Nothing is cached.
    pub fn get_demographic_rates(&self, demographics: DemoRow) -> DemographicRates {
        let mut rates = DemographicRates::baseline();
        if let Some(species) = self.species.get(&demographics.species) {
            rates = rates.add(&species.species_demo_eff);
        }
        if demographics.culture != 0 {
            if let Some(culture) = self.cultures.get(&demographics.culture) {
                rates = rates.add(&culture.culture_demo_eff);
            }
        }
        rates
    }

    /// # Birth Sentiment
    ///
    /// Species and culture birth-sentiment rates, added.
    ///
    /// `demographics` names those records. Culture id `0` is skipped. A missing
    /// species adds nothing. The result is that stacked schedule.
    pub fn birth_sentiment(&self, demographics: DemoRow) -> BirthSentiment {
        let mut mods = BirthSentiment::zero();
        if let Some(species) = self.species.get(&demographics.species) {
            mods = mods.add(species.birth_sentiment);
        }
        if demographics.culture != 0 {
            if let Some(culture) = self.cultures.get(&demographics.culture) {
                mods = mods.add(culture.birth_sentiment);
            }
        }
        mods
    }

    /// # Clear Household Changed Flags
    ///
    /// Sets `household_changed` to false on every species and culture stored here.
    pub fn clear_household_changed_flags(&mut self) {
        for species in self.species.values_mut() {
            species.household_changed = false;
        }
        for culture in self.cultures.values_mut() {
            culture.household_changed = false;
        }
    }
}

#[cfg(test)]
mod factuals_should {
    use super::*;
    use crate::game::config::GameConfig;
    use crate::game::effects::ProcessEffect;
    use crate::game::good::{GoodTag, TIME};
    use crate::game::process::InputType;
    use std::path::PathBuf;

    fn repo_goods_file() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world/goods.toml")
    }

    fn repo_world_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world")
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
        assert!((grain.decay_rate - 1.0).abs() < 1e-12);
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
        assert!(!factuals.goods.is_empty());
        assert_eq!(factuals.find_good(0).name, "time");
        assert!((factuals.find_good(0).decay_rate - 1.0).abs() < 1e-12);
        assert!((factuals.find_good(0).transport_efficiency() - 1.0).abs() < 1e-12);
        assert_eq!(factuals.find_good(1).name, "grain");
        assert!((factuals.find_good(1).decay_rate - 0.05).abs() < 1e-12);
        assert!((factuals.find_good(1).mass - 1.0).abs() < 1e-12);
        assert!((factuals.find_good(1).volume - 0.0015).abs() < 1e-12);
        assert!((factuals.find_good(1).bulk() - 1.6).abs() < 1e-12);
        assert!(factuals.find_good(0).bulk().abs() < 1e-12);
        assert!(factuals.may_leave_market(1));
        let cabin = factuals.find_good(8);
        assert!(cabin.is_restricted());
        assert!(cabin.is_immobile());
        assert!(!cabin.may_leave_market());
        assert!(!factuals.may_leave_market(8));
        assert!((cabin.bulk() - 40.0).abs() < 1e-12);
        let hut = factuals.find_good(44);
        assert!(hut.is_restricted());
        assert!(hut.is_immobile());
        assert!((hut.bulk() - 22.0).abs() < 1e-12);
        assert_eq!(factuals.find_good(5).name, "gold_token");
        assert!((factuals.find_good(5).decay_rate - 0.005).abs() < 1e-12);
    }

    #[test]
    fn load_from_path_reads_world_dir_goods_and_processes() {
        let factuals = Factuals::load_from_path(repo_world_dir()).expect("world dir");
        assert!(!factuals.goods.is_empty());
        assert!(!factuals.processes.is_empty());
        let survival = factuals.goods.values().find(|good| good.name == "survival");
        let survival = survival.expect("survival");
        assert_eq!(survival.mass, 0.0);
        assert_eq!(survival.volume, 0.0);
        assert!((survival.decay_rate - 0.05).abs() < 1e-12);
        assert!(survival.is_skill());
        assert!(survival.tags.contains(&GoodTag::Untradeable));
        let grain = factuals.processes.get(&1).expect("make grain");
        assert_eq!(grain.name, "make grain");
        assert!(!grain.is_subsistence());
        assert!((grain.complexity - 1.0).abs() < 1e-12);
        assert_eq!(grain.inputs[0].good, TIME);
        assert!((grain.inputs[0].amount - 5.0).abs() < 1e-12);
        assert_eq!(grain.outputs[0].good, 1);
        assert!((grain.outputs[0].amount - 40.0).abs() < 1e-12);
        assert_eq!(factuals.config, GameConfig::default());
    }

    #[test]
    fn load_from_toml_reads_a_process() {
        let factuals = Factuals::load_from_toml(
            r#"
[[processes]]
id = 10
name = "test mill"
tech_source = 2
inputs = [{ good = 1, amount = 2.0, type = "consumed", optional = true }]
outputs = [{ good = 3, amount = 1.0, fixed = true }]
effects = [{ development = 4.0 }]
"#,
        )
        .expect("toml");
        let mill = factuals.processes.get(&10).expect("mill");
        assert_eq!(mill.name, "test mill");
        assert_eq!(mill.tech_source, 2);
        assert!(mill.inputs[0].is_optional());
        assert!(matches!(mill.inputs[0].input_type, InputType::Consumed));
        assert!(mill.outputs[0].fixed);
        assert!(matches!(mill.effects[0], ProcessEffect::Development(v) if v == 4.0));
        assert!(!mill.is_subsistence());
        assert!((mill.complexity - 1.0).abs() < 1e-12);
    }

    #[test]
    fn load_from_toml_reads_complexity_and_rejects_non_positive() {
        let factuals = Factuals::load_from_toml(
            r#"
[[processes]]
id = 40
name = "camp"
complexity = 0.25
inputs = [{ good = 0, amount = 0.5 }]
outputs = [{ good = 1, amount = 1.0 }]

[[processes]]
id = 42
name = "specialized"
complexity = 1.0
outputs = [{ good = 1, amount = 1.0 }]

[[processes]]
id = 43
name = "works"
complexity = 2.5
outputs = [{ good = 1, amount = 1.0 }]
"#,
        )
        .expect("toml");
        let camp = factuals.processes.get(&40).expect("camp");
        assert!(camp.is_subsistence());
        assert!((camp.complexity - 0.25).abs() < 1e-12);
        let specialized = factuals.processes.get(&42).expect("specialized");
        assert!(!specialized.is_subsistence());
        assert!((specialized.complexity - 1.0).abs() < 1e-12);
        let works = factuals.processes.get(&43).expect("works");
        assert!(!works.is_subsistence());
        assert!((works.complexity - 2.5).abs() < 1e-12);

        let err = Factuals::load_from_toml(
            r#"
[[processes]]
id = 41
name = "bad"
complexity = 0.0
outputs = [{ good = 1, amount = 1.0 }]
"#,
        )
        .expect_err("zero complexity");
        match err {
            FactualsLoadError::InvalidProcess(msg) => {
                assert!(msg.contains("complexity"));
            }
            other => panic!("expected InvalidProcess, got {other}"),
        }
    }

    #[test]
    fn load_from_toml_errors_on_duplicate_process_id() {
        let err = Factuals::load_from_toml(
            r#"
[[processes]]
id = 1
name = "a"
outputs = [{ good = 1, amount = 1.0 }]

[[processes]]
id = 1
name = "b"
outputs = [{ good = 1, amount = 1.0 }]
"#,
        )
        .expect_err("duplicate");
        match err {
            FactualsLoadError::DuplicateProcess(1) => {}
            other => panic!("expected DuplicateProcess(1), got {other}"),
        }
    }

    #[test]
    fn load_from_toml_errors_on_duplicate_process_input() {
        let err = Factuals::load_from_toml(
            r#"
[[processes]]
id = 1
name = "a"
inputs = [
  { good = 1, amount = 1.0 },
  { good = 1, amount = 2.0 },
]
outputs = [{ good = 2, amount = 1.0 }]
"#,
        )
        .expect_err("duplicate input");
        match err {
            FactualsLoadError::DuplicateProcessInput {
                process: 1,
                good: 1,
            } => {}
            other => panic!("expected DuplicateProcessInput, got {other}"),
        }
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
}
