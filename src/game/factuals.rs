use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::game::{
    config::{ConfigLoadError, GameConfig}, culture::Culture, desire::{DemoDesire, Desire, DesireSource}, effects::ProcessEffect, good::Good, household::DemographicRates, pop::DemoRow, process::{InputEffect, InputType, Process, ProcessInput, ProcessOutput}, religion::Religion, species::Species,
};

/// TOML world-data file of goods and/or processes (factuals).
#[derive(Debug, Deserialize)]
struct WorldFile {
    #[serde(default)]
    goods: Vec<Good>,
    #[serde(default)]
    processes: Vec<ProcessFile>,
}

#[derive(Debug, Deserialize)]
struct ProcessFile {
    id: usize,
    name: String,
    #[serde(default)]
    tech_source: usize,
    #[serde(default)]
    inputs: Vec<ProcessInputFile>,
    #[serde(default)]
    outputs: Vec<ProcessOutputFile>,
    #[serde(default)]
    effects: Vec<ProcessEffectFile>,
}

#[derive(Debug, Deserialize)]
struct ProcessInputFile {
    good: usize,
    amount: f64,
    #[serde(default)]
    fixed: bool,
    #[serde(default, rename = "type")]
    kind: InputTypeFile,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    optional_effects: Vec<InputEffectFile>,
}

#[derive(Debug, Deserialize)]
struct ProcessOutputFile {
    good: usize,
    amount: f64,
    #[serde(default)]
    fixed: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum InputTypeFile {
    #[default]
    Destroyed,
    Consumed,
    Capital,
    Factor,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum InputEffectFile {
    Throughput(f64),
    Input(f64),
    Output(f64),
    ExtraOutput { good: usize, amount: f64 },
    BirthRate(f64),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProcessEffectFile {
    Research(f64),
    Culture(f64),
    Faith(f64),
    Authority(f64),
    Legitimacy(f64),
    BirthRate(f64),
}

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
            Self::DuplicateProcessInput { process, good } => {
                write!(f, "process {process} repeats input good {good}")
            }
            Self::InvalidProcess(msg) => write!(f, "{msg}"),
            Self::Config(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for FactualsLoadError {}

impl ProcessFile {
    fn into_process(self, factuals: &Factuals) -> Result<Process, FactualsLoadError> {
        let id = self.id;
        let mut seen_inputs = HashSet::new();
        let mut process = Process::new(id, self.name, self.tech_source);
        for input in self.inputs {
            if !seen_inputs.insert(input.good) {
                return Err(FactualsLoadError::DuplicateProcessInput {
                    process: id,
                    good: input.good,
                });
            }
            process = process.with_input(input.into_input(id, factuals)?);
        }
        for output in self.outputs {
            process = process.with_output(output.into_output(id, factuals)?);
        }
        for effect in self.effects {
            process = process.with_effect(effect.into());
        }
        Ok(process)
    }
}

impl ProcessInputFile {
    fn into_input(self, process: usize, factuals: &Factuals) -> Result<ProcessInput, FactualsLoadError> {
        check_process_amount(process, "input", self.amount)?;
        check_process_good(process, self.good, factuals)?;
        let optional = self.optional || !self.optional_effects.is_empty();
        let mut input = ProcessInput::new(
            self.good,
            self.amount,
            self.fixed,
            self.kind.into(),
            optional,
        );
        for effect in self.optional_effects {
            input = input.with_optional(effect.into());
        }
        Ok(input)
    }
}

impl ProcessOutputFile {
    fn into_output(self, process: usize, factuals: &Factuals) -> Result<ProcessOutput, FactualsLoadError> {
        check_process_amount(process, "output", self.amount)?;
        check_process_good(process, self.good, factuals)?;
        Ok(ProcessOutput::new(self.good, self.amount, self.fixed))
    }
}

fn check_process_amount(process: usize, kind: &str, amount: f64) -> Result<(), FactualsLoadError> {
    if amount > 0.0 && amount.is_finite() {
        Ok(())
    } else {
        Err(FactualsLoadError::InvalidProcess(format!(
            "process {process} {kind} amount must be finite and > 0"
        )))
    }
}

fn check_process_good(
    process: usize,
    good: usize,
    factuals: &Factuals,
) -> Result<(), FactualsLoadError> {
    if factuals.goods.is_empty() || factuals.goods.contains_key(&good) {
        Ok(())
    } else {
        Err(FactualsLoadError::InvalidProcess(format!(
            "process {process} references missing good {good}"
        )))
    }
}

impl From<InputTypeFile> for InputType {
    fn from(kind: InputTypeFile) -> Self {
        match kind {
            InputTypeFile::Destroyed => InputType::Destroyed,
            InputTypeFile::Consumed => InputType::Consumed,
            InputTypeFile::Capital => InputType::Capital,
            InputTypeFile::Factor => InputType::Factor,
        }
    }
}

impl From<InputEffectFile> for InputEffect {
    fn from(effect: InputEffectFile) -> Self {
        match effect {
            InputEffectFile::Throughput(v) => InputEffect::Throughput(v),
            InputEffectFile::Input(v) => InputEffect::Input(v),
            InputEffectFile::Output(v) => InputEffect::Output(v),
            InputEffectFile::ExtraOutput { good, amount } => InputEffect::ExtraOutput(good, amount),
            InputEffectFile::BirthRate(v) => InputEffect::BirthRate(v),
        }
    }
}

impl From<ProcessEffectFile> for ProcessEffect {
    fn from(effect: ProcessEffectFile) -> Self {
        match effect {
            ProcessEffectFile::Research(v) => ProcessEffect::Research(v),
            ProcessEffectFile::Culture(v) => ProcessEffect::Culture(v),
            ProcessEffectFile::Faith(v) => ProcessEffect::Faith(v),
            ProcessEffectFile::Authority(v) => ProcessEffect::Authority(v),
            ProcessEffectFile::Legitimacy(v) => ProcessEffect::Legitimacy(v),
            ProcessEffectFile::BirthRate(v) => ProcessEffect::BirthRate(v),
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
    pub religion: HashMap<usize, Religion>,
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
            religion: HashMap::new(),
            config: GameConfig::default(),
        }
    }

    /// Loads world data from `path`.
    ///
    /// A directory loads `goods.toml`, `processes.toml` if present, and
    /// `config.toml` if present. A file is treated as a single TOML document
    /// (goods and/or processes). Species, cultures, and religions stay empty.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, FactualsLoadError> {
        let path = path.as_ref();
        if path.is_dir() {
            Self::load_from_dir(path)
        } else {
            let text = std::fs::read_to_string(path).map_err(FactualsLoadError::Io)?;
            Self::load_from_toml(&text)
        }
    }

    /// Loads `goods.toml` plus optional `processes.toml` from a world-data folder.
    fn load_from_dir(dir: &Path) -> Result<Self, FactualsLoadError> {
        let goods_path = dir.join("goods.toml");
        let text = std::fs::read_to_string(&goods_path).map_err(FactualsLoadError::Io)?;
        let mut factuals = Self::load_from_toml(&text)?;
        let processes_path = dir.join("processes.toml");
        if processes_path.exists() {
            let text = std::fs::read_to_string(&processes_path).map_err(FactualsLoadError::Io)?;
            factuals.insert_world_file(&text)?;
        }
        let config_path = dir.join("config.toml");
        if config_path.exists() {
            factuals.config =
                GameConfig::load_from_path(&config_path).map_err(FactualsLoadError::Config)?;
        }
        Ok(factuals)
    }

    /// Loads goods and processes from TOML text into an empty [`Factuals`].
    pub fn load_from_toml(text: &str) -> Result<Self, FactualsLoadError> {
        let mut factuals = Factuals::new();
        factuals.insert_world_file(text)?;
        Ok(factuals)
    }

    fn insert_world_file(&mut self, text: &str) -> Result<(), FactualsLoadError> {
        let file: WorldFile = toml::from_str(text).map_err(FactualsLoadError::Toml)?;
        for good in file.goods {
            if self.goods.contains_key(&good.id) {
                return Err(FactualsLoadError::DuplicateGood(good.id));
            }
            self.goods.insert(good.id, good);
        }
        for process in file.processes {
            self.insert_process_file(process)?;
        }
        Ok(())
    }

    fn insert_process_file(&mut self, file: ProcessFile) -> Result<(), FactualsLoadError> {
        if self.processes.contains_key(&file.id) {
            return Err(FactualsLoadError::DuplicateProcess(file.id));
        }
        let process = file.into_process(self)?;
        self.processes.insert(process.id, process);
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
    use crate::game::config::GameConfig;
    use crate::game::effects::ProcessEffect;
    use crate::game::good::GoodTag;
    use crate::game::process::InputType;
    use crate::game::{culture::Culture, religion::Religion, species::Species};
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
        assert!(!factuals.find_good(0).is_buyable());
        assert_eq!(factuals.find_good(1).name, "grain");
        assert!((factuals.find_good(1).decay_rate - 0.4).abs() < 1e-12);
        assert_eq!(factuals.find_good(5).name, "gold_token");
        assert!((factuals.find_good(5).decay_rate - 0.01).abs() < 1e-12);
    }

    #[test]
    fn load_from_path_reads_world_dir_goods_and_processes() {
        let factuals = Factuals::load_from_path(repo_world_dir()).expect("world dir");
        assert!(!factuals.goods.is_empty());
        assert_eq!(factuals.processes.len(), factuals.goods.len());
        for process in factuals.processes.values() {
            assert_eq!(process.inputs.len(), 1);
            assert_eq!(process.inputs[0].good, 0);
            assert!((process.inputs[0].amount - 1.0).abs() < 1e-12);
            assert!(matches!(process.inputs[0].input_type, InputType::Destroyed));
            assert_eq!(process.outputs.len(), 1);
            assert!((process.outputs[0].amount - 15.0).abs() < 1e-12);
        }
        let grain = factuals.processes.get(&1).expect("make grain");
        assert_eq!(grain.name, "make grain");
        assert_eq!(grain.outputs[0].good, 1);
        let pots = factuals.processes.get(&27).expect("make pots");
        assert_eq!(pots.name, "make pots");
        assert_eq!(pots.outputs[0].good, 27);
        let time = factuals.processes.get(&28).expect("make time");
        assert_eq!(time.name, "make time");
        assert_eq!(time.outputs[0].good, 0);
        assert_eq!(factuals.config, GameConfig::default());
        assert_eq!(factuals.config.labor.worker_share, 0.30);
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
effects = [{ research = 4.0 }]
"#,
        )
        .expect("toml");
        let mill = factuals.processes.get(&10).expect("mill");
        assert_eq!(mill.name, "test mill");
        assert_eq!(mill.tech_source, 2);
        assert!(mill.inputs[0].is_optional());
        assert!(matches!(mill.inputs[0].input_type, InputType::Consumed));
        assert!(mill.outputs[0].fixed);
        assert!(matches!(mill.effects[0], ProcessEffect::Research(v) if v == 4.0));
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
            FactualsLoadError::DuplicateProcessInput { process: 1, good: 1 } => {}
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