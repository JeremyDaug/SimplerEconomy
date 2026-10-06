use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::game::{
    config::{ConfigLoadError, GameConfig}, craft::Craft, culture::Culture, demographic_source::DemographicSource, desire::{DemoDesire, Desire}, effects::ProcessEffect, good::Good, household::DemographicRates, pop::DemoRow, process::{InputEffect, InputType, Process, ProcessInput, ProcessOutput}, religion::Religion, species::Species, stratum::Stratum,
};

/// TOML world-data file of goods, processes, and crafts (factuals).
#[derive(Debug, Deserialize)]
struct WorldFile {
    #[serde(default)]
    goods: Vec<Good>,
    #[serde(default)]
    processes: Vec<ProcessFile>,
    #[serde(default)]
    crafts: Vec<CraftFile>,
    #[serde(default)]
    culture_crafts: Vec<CultureCraftFile>,
    #[serde(default)]
    religion_crafts: Vec<ReligionCraftFile>,
}

#[derive(Debug, Deserialize)]
struct CraftFile {
    id: usize,
    name: String,
    #[serde(default)]
    processes: Vec<usize>,
    #[serde(default = "default_complexity_modifier")]
    complexity_modifier: f64,
}

/// # Default Complexity Modifier
///
/// A craft that omits `complexity_modifier` loads as 1.0.
fn default_complexity_modifier() -> f64 {
    1.0
}

#[derive(Debug, Deserialize)]
struct CultureCraftFile {
    culture: usize,
    #[serde(default)]
    name: String,
    craft: usize,
    #[serde(default)]
    add: Vec<usize>,
    #[serde(default)]
    remove: Vec<usize>,
    #[serde(default = "default_complexity_modifier")]
    complexity_modifier: f64,
}

#[derive(Debug, Deserialize)]
struct ReligionCraftFile {
    religion: usize,
    #[serde(default)]
    name: String,
    craft: usize,
    #[serde(default)]
    add: Vec<usize>,
    #[serde(default)]
    remove: Vec<usize>,
    #[serde(default = "default_complexity_modifier")]
    complexity_modifier: f64,
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
    /// Management overhead. Omitted world data is 1.0. Below 1.0 is subsistence.
    #[serde(default = "default_process_complexity")]
    complexity: f64,
}

/// # Default Process Complexity
///
/// World data that omits complexity loads as 1.0, which is not subsistence.
fn default_process_complexity() -> f64 {
    1.0
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
    DuplicateCraft(usize),
    DuplicateCultureCraft { culture: usize, craft: usize },
    DuplicateReligionCraft { religion: usize, craft: usize },
    InvalidProcess(String),
    InvalidCraft(String),
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
            Self::DuplicateCraft(id) => write!(f, "duplicate craft id {id} in world data"),
            Self::DuplicateCultureCraft { culture, craft } => {
                write!(f, "culture {culture} repeats craft {craft}")
            }
            Self::DuplicateReligionCraft { religion, craft } => {
                write!(f, "religion {religion} repeats craft {craft}")
            }
            Self::InvalidProcess(msg) => write!(f, "{msg}"),
            Self::InvalidCraft(msg) => write!(f, "{msg}"),
            Self::Config(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for FactualsLoadError {}

impl ProcessFile {
    fn into_process(self, factuals: &Factuals) -> Result<Process, FactualsLoadError> {
        let id = self.id;
        check_process_complexity(id, self.complexity)?;
        let mut seen_inputs = HashSet::new();
        let mut process = Process::new(id, self.name, self.tech_source)
            .with_complexity(self.complexity);
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

/// # Check Process Complexity
///
/// `process` is the process id. `complexity` is the value from world data.
/// Accepts a finite number greater than 0. Anything else is an invalid process.
fn check_process_complexity(process: usize, complexity: f64) -> Result<(), FactualsLoadError> {
    if complexity > 0.0 && complexity.is_finite() {
        Ok(())
    } else {
        Err(FactualsLoadError::InvalidProcess(format!(
            "process {process} complexity must be finite and > 0"
        )))
    }
}

/// # Check Complexity Modifier
///
/// `label` names the row. `modifier` is the value from world data.
///
/// A finite modifier is kept, including a negative one. NaN and infinity
/// are an invalid craft.
fn check_complexity_modifier(label: &str, modifier: f64) -> Result<(), FactualsLoadError> {
    if modifier.is_finite() {
        Ok(())
    } else {
        Err(FactualsLoadError::InvalidCraft(format!(
            "{label} complexity modifier must be finite"
        )))
    }
}

/// # Reject Added And Removed
///
/// `label` names the row. `add` and `remove` are its process ids.
///
/// An id in both lists is invalid. Returns the first such id.
fn reject_added_and_removed(
    label: &str,
    add: &[usize],
    remove: &[usize],
) -> Result<(), FactualsLoadError> {
    for id in add {
        if remove.contains(id) {
            return Err(FactualsLoadError::InvalidCraft(format!(
                "{label} adds and removes process {id}"
            )));
        }
    }
    Ok(())
}

/// # Duplicate Craft
///
/// The error for craft `id` already stored at `origin`.
///
/// An open craft is [`FactualsLoadError::DuplicateCraft`]. Culture and
/// religion use their duplicate errors. Species and stratum name the
/// demographic in [`FactualsLoadError::InvalidCraft`].
fn duplicate_craft(id: usize, origin: Option<DemographicSource>) -> FactualsLoadError {
    match origin {
        None => FactualsLoadError::DuplicateCraft(id),
        Some(DemographicSource::Culture(culture)) => {
            FactualsLoadError::DuplicateCultureCraft { culture, craft: id }
        }
        Some(DemographicSource::Religion(religion)) => {
            FactualsLoadError::DuplicateReligionCraft { religion, craft: id }
        }
        Some(DemographicSource::Species(species)) => FactualsLoadError::InvalidCraft(format!(
            "species {species} already has craft {id}"
        )),
        Some(DemographicSource::Stratum(stratum)) => FactualsLoadError::InvalidCraft(format!(
            "stratum {stratum} already has craft {id}"
        )),
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
/// This should include Goods, Processes, Crafts, Game Rules, etc.
/// 
/// This is as compared to 'game state' which is the current state fo the world in a 
/// given game, such as the map, players, goods in the market, prices, etc.
#[derive(Debug, Clone)]
pub struct Factuals {
    pub goods: HashMap<usize, Good>,
    pub processes: HashMap<usize, Process>,
    /// Crafts keyed by craft id and [`Craft::origin`].
    ///
    /// Goods and processes stay as loaded. Crafts may be added or removed
    /// during play. Pop jobs are not retargeted here.
    pub crafts: HashMap<(usize, Option<DemographicSource>), Craft>,
    pub species: HashMap<usize, Species>,
    pub cultures: HashMap<usize, Culture>,
    /// Strata keyed by id. Id `0` is empty and is not stored.
    pub strata: HashMap<usize, Stratum>,
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
            crafts: HashMap::new(),
            cultures: HashMap::new(),
            species: HashMap::new(),
            strata: HashMap::new(),
            religion: HashMap::new(),
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
    /// Reads `goods.toml`, then `processes.toml` when that file is present,
    /// then `crafts.toml` when present, then `config.toml` when present.
    /// Each table file is merged by [`Self::insert_world_file`]. `config.toml`
    /// replaces [`Self::config`]. Returns the [`Factuals`], or the first error.
    fn load_from_dir(dir: &Path) -> Result<Self, FactualsLoadError> {
        let goods_path = dir.join("goods.toml");
        let text = std::fs::read_to_string(&goods_path).map_err(FactualsLoadError::Io)?;
        let mut factuals = Self::load_from_toml(&text)?;
        let processes_path = dir.join("processes.toml");
        if processes_path.exists() {
            let text = std::fs::read_to_string(&processes_path).map_err(FactualsLoadError::Io)?;
            factuals.insert_world_file(&text)?;
        }
        let crafts_path = dir.join("crafts.toml");
        if crafts_path.exists() {
            let text = std::fs::read_to_string(&crafts_path).map_err(FactualsLoadError::Io)?;
            factuals.insert_world_file(&text)?;
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
    /// Loads goods, then processes, then open crafts, then culture crafts,
    /// then religion crafts. A culture or religion row is stored by
    /// [`Self::insert_attached_craft`]. Returns the first error.
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
        for craft in file.crafts {
            self.insert_craft_file(craft)?;
        }
        for craft in file.culture_crafts {
            self.insert_culture_craft(craft)?;
        }
        for craft in file.religion_crafts {
            self.insert_religion_craft(craft)?;
        }
        Ok(())
    }

    /// # Insert Craft File
    ///
    /// Stores one open craft from world data.
    ///
    /// `file` is the row. Craft id `0` is rejected. A craft id already stored
    /// as open is a duplicate. Each process id is kept once, and only when
    /// [`Self::get_process`] already has that process. A rejected row is not
    /// stored.
    fn insert_craft_file(&mut self, file: CraftFile) -> Result<(), FactualsLoadError> {
        if file.id == 0 {
            return Err(FactualsLoadError::InvalidCraft(
                "craft id 0 is no craft".into(),
            ));
        }
        if self.get_craft(file.id).is_some() {
            return Err(FactualsLoadError::DuplicateCraft(file.id));
        }
        let label = format!("craft {}", file.id);
        self.require_craft_processes(&label, &file.processes)?;
        check_complexity_modifier(&label, file.complexity_modifier)?;
        self.add_craft(Craft {
            id: file.id,
            name: file.name,
            origin: None,
            processes: file.processes,
            remove: Vec::new(),
            complexity_modifier: file.complexity_modifier,
        })
    }

    /// # Insert Culture Craft
    ///
    /// `file` is one culture craft row.
    ///
    /// Storage is [`Self::insert_attached_craft`] for that culture. Returns
    /// that result.
    fn insert_culture_craft(&mut self, file: CultureCraftFile) -> Result<(), FactualsLoadError> {
        self.insert_attached_craft(
            DemographicSource::Culture(file.culture),
            file.name,
            file.craft,
            file.add,
            file.remove,
            file.complexity_modifier,
        )
    }

    /// # Insert Religion Craft
    ///
    /// `file` is one religion craft row.
    ///
    /// Storage is [`Self::insert_attached_craft`] for that religion. Returns
    /// that result.
    fn insert_religion_craft(&mut self, file: ReligionCraftFile) -> Result<(), FactualsLoadError> {
        self.insert_attached_craft(
            DemographicSource::Religion(file.religion),
            file.name,
            file.craft,
            file.add,
            file.remove,
            file.complexity_modifier,
        )
    }

    /// # Insert Attached Craft
    ///
    /// Stores one demographic craft from world data.
    ///
    /// `origin` is the demographic. `holder_name` names a culture or religion
    /// that is not loaded yet. `craft_id` is the open craft. `add` and
    /// `remove` are process ids. `modifier` is stored on the attached craft.
    ///
    /// A non-finite modifier is rejected before a holder is created. Origin
    /// id `0` is rejected. The open craft must already be stored. Add and
    /// remove ids must be loaded and unique, and an id cannot be in both.
    /// The holder is created after those checks. The stored craft's
    /// `processes` are `add`.
    fn insert_attached_craft(
        &mut self,
        origin: DemographicSource,
        holder_name: String,
        craft_id: usize,
        add: Vec<usize>,
        remove: Vec<usize>,
        modifier: f64,
    ) -> Result<(), FactualsLoadError> {
        let kind = match origin {
            DemographicSource::Culture(_) => "culture",
            DemographicSource::Religion(_) => "religion",
            DemographicSource::Species(_) => "species",
            DemographicSource::Stratum(_) => "stratum",
        };
        let label = format!("{kind} {} craft {craft_id}", origin.id());
        check_complexity_modifier(&label, modifier)?;
        if origin.id() == 0 {
            return Err(FactualsLoadError::InvalidCraft(format!(
                "{kind} 0 is no {kind}"
            )));
        }
        let Some(open) = self.get_craft(craft_id) else {
            return Err(FactualsLoadError::InvalidCraft(format!(
                "{label} has no open craft"
            )));
        };
        let name = open.name.clone();
        self.require_craft_processes(&label, &add)?;
        self.require_craft_processes(&label, &remove)?;
        reject_added_and_removed(&label, &add, &remove)?;
        match origin {
            DemographicSource::Culture(id) => {
                self.cultures
                    .entry(id)
                    .or_insert_with(|| Culture::new(id, holder_name));
            }
            DemographicSource::Religion(id) => {
                self.religion
                    .entry(id)
                    .or_insert_with(|| Religion::new(id, holder_name));
            }
            DemographicSource::Species(_) | DemographicSource::Stratum(_) => {}
        }
        self.add_craft(Craft {
            id: craft_id,
            name,
            origin: Some(origin),
            processes: add,
            remove,
            complexity_modifier: modifier,
        })
    }

    /// # Require Craft Processes
    ///
    /// `label` names the row. `ids` are the process ids on that row.
    ///
    /// A repeated id is invalid. An id [`Self::get_process`] does not return
    /// is invalid. Returns the first failure.
    fn require_craft_processes(
        &self,
        label: &str,
        ids: &[usize],
    ) -> Result<(), FactualsLoadError> {
        let mut seen = HashSet::new();
        for process in ids {
            if !seen.insert(*process) {
                return Err(FactualsLoadError::InvalidCraft(format!(
                    "{label} repeats process {process}"
                )));
            }
            if self.get_process(*process).is_none() {
                return Err(FactualsLoadError::InvalidCraft(format!(
                    "{label} process {process} is not loaded"
                )));
            }
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

    /// # With Stratum
    ///
    /// Stores `stratum` and returns these factuals.
    ///
    /// Storage is [`Self::add_stratum`].
    pub fn with_stratum(mut self, stratum: Stratum) -> Self {
        self.add_stratum(stratum);
        self
    }

    /// # Add Stratum
    ///
    /// Stores `stratum` and records its id on the culture it derives from.
    ///
    /// Id `0` is the empty stratum and panics. A culture of `0` panics.
    /// A missing culture panics. A stratum id already stored panics. The
    /// culture's list gains the id when it is not already there.
    pub fn add_stratum(&mut self, stratum: Stratum) {
        let id = stratum.id;
        if id == 0 {
            panic!("Stratum 0 is empty.");
        }
        if self.strata.contains_key(&id) {
            panic!("Stratum ID {id} already exists in factuals.");
        }
        let culture_id = stratum.culture;
        if culture_id == 0 {
            panic!("Stratum {id} derives from culture 0, which is empty.");
        }
        let culture = self
            .cultures
            .get_mut(&culture_id)
            .unwrap_or_else(|| panic!("Culture {culture_id} missing from factuals."));
        culture.push_stratum(id);
        self.strata.insert(id, stratum);
    }

    /// # With Craft
    ///
    /// Adds `craft` and returns these factuals.
    ///
    /// Panics when [`Self::add_craft`] rejects the craft.
    pub fn with_craft(mut self, craft: Craft) -> Self {
        if let Err(err) = self.add_craft(craft) {
            panic!("{err}");
        }
        self
    }

    /// # Add Craft
    ///
    /// Stores `craft` under its id and origin.
    ///
    /// Id `0` is invalid. A craft already stored for that id and origin is a
    /// duplicate. Process ids are not checked. A rejected craft is not stored.
    pub fn add_craft(&mut self, craft: Craft) -> Result<(), FactualsLoadError> {
        if craft.id == 0 {
            return Err(FactualsLoadError::InvalidCraft(
                "craft id 0 is no craft".into(),
            ));
        }
        let key = (craft.id, craft.origin);
        if self.crafts.contains_key(&key) {
            return Err(duplicate_craft(craft.id, craft.origin));
        }
        self.crafts.insert(key, craft);
        Ok(())
    }

    /// # Remove Craft
    ///
    /// Removes the craft stored under `id` and `origin`.
    ///
    /// Returns that craft when one was stored.
    pub fn remove_craft(&mut self, id: usize, origin: Option<DemographicSource>) -> Option<Craft> {
        self.crafts.remove(&(id, origin))
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

    /// # Get Good
    ///
    /// The good stored under `id`.
    ///
    /// Returns `None` when the world has no good with that id.
    pub fn get_good(&self, id: usize) -> Option<&Good> {
        self.goods.get(&id)
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

    /// # Get Craft
    ///
    /// The open craft stored under `id`.
    ///
    /// Returns `None` when that open craft is not stored.
    pub fn get_craft(&self, id: usize) -> Option<&Craft> {
        self.craft(id, None)
    }

    /// # Craft
    ///
    /// The craft stored under `id` and `origin`.
    ///
    /// Returns `None` when that pair is not stored.
    pub fn craft(&self, id: usize, origin: Option<DemographicSource>) -> Option<&Craft> {
        self.crafts.get(&(id, origin))
    }

    /// # Effective Craft
    ///
    /// The open craft after the culture attachment and then the religion attachment.
    ///
    /// Starts from the open craft. Each attachment replaces the process list
    /// with [`Craft::apply_to`] and multiplies [`Craft::complexity_modifier`].
    /// Craft id `0`, or a missing open craft, returns `None`. Culture or
    /// religion id `0`, or a missing attachment, skips that part.
    pub fn effective_craft(&self, craft: usize, culture: usize, religion: usize) -> Option<Craft> {
        if craft == 0 {
            return None;
        }
        let mut resolved = self.get_craft(craft)?.clone();
        if culture != 0 {
            if let Some(attached) = self.craft(craft, Some(DemographicSource::Culture(culture))) {
                resolved.processes = attached.apply_to(&resolved.processes);
                resolved.complexity_modifier *= attached.complexity_modifier;
            }
        }
        if religion != 0 {
            if let Some(attached) = self.craft(craft, Some(DemographicSource::Religion(religion)))
            {
                resolved.processes = attached.apply_to(&resolved.processes);
                resolved.complexity_modifier *= attached.complexity_modifier;
            }
        }
        Some(resolved)
    }

    /// # Craft Processes
    ///
    /// Process ids of [`Self::effective_craft`] for these ids.
    ///
    /// An absent craft returns an empty list.
    pub fn craft_processes(&self, craft: usize, culture: usize, religion: usize) -> Vec<usize> {
        self.effective_craft(craft, culture, religion)
            .map(|craft| craft.processes)
            .unwrap_or_default()
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
        for stratum in self.strata.values_mut() {
            stratum.household_changed = false;
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

    /// # Get Stratum
    ///
    /// The stratum stored under `id`.
    ///
    /// Returns `None` when that id is not stored. Id `0` is empty and is
    /// not stored.
    pub fn get_stratum(&self, id: usize) -> Option<&Stratum> {
        self.strata.get(&id)
    }

    /// # Find Stratum
    ///
    /// The stratum stored under `id`.
    ///
    /// Panics when that id is missing.
    pub fn find_stratum(&self, id: usize) -> &Stratum {
        self.get_stratum(id)
            .unwrap_or_else(|| panic!("Stratum {id} missing from factuals."))
    }

    /// Looks up a religion by id. Panics if missing.
    pub fn find_religion(&self, id: usize) -> &Religion {
        self.religion.get(&id)
            .unwrap_or_else(|| panic!("Religion {id} missing from factuals."))
    }

    /// # Source Demo Desire
    ///
    /// Resolves the demographic desire behind a pop `Desire` via
    /// [`Desire::source`] and [`Desire::demo_desire_id`].
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
            DemographicSource::Stratum(source_id) => self
                .get_stratum(source_id)
                .and_then(|stratum| stratum.find_desire(demo_id)),
            DemographicSource::Religion(source_id) => self
                .religion
                .get(&source_id)
                .and_then(|religion| religion.find_desire(demo_id)),
        }
    }

    pub(crate) fn find_good(&self, id: usize) -> &Good {
        self.goods.get(&id)
            .unwrap_or_else(|| panic!("Good {id} missing from factuals."))
    }
    
    /// # Get Demographic Rates
    ///
    /// Resolve structural demographic rates for a pop's demographic ids:
    /// `baseline + species_demo_eff + culture_demo_eff + stratum_demo_eff + religion_demo_eff`
    /// (culture, stratum, and religion id `0` means none and is skipped).
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
    /// culture x stratum x religion product):
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
        if demographics.stratum != 0 {
            if let Some(stratum) = self.get_stratum(demographics.stratum) {
                rates = rates.add(&stratum.stratum_demo_eff);
            }
        }
        if demographics.religion != 0 {
            if let Some(religion) = self.religion.get(&demographics.religion) {
                rates = rates.add(&religion.religion_demo_eff);
            }
        }
        rates
    }

    /// Share of on-hand Time this demographic may commit to wage work.
    ///
    /// Species supplies the base (default `0.5` when the species is missing).
    /// Culture, stratum, and religion add their fractions. Id `0` on any of
    /// those is skipped. The result is clamped to `0..=1`.
    pub fn work_time_fraction(&self, demographics: DemoRow) -> f64 {
        let mut fraction = self
            .species
            .get(&demographics.species)
            .map(|species| species.work_time_fraction)
            .unwrap_or(0.5);
        if demographics.culture != 0 {
            if let Some(culture) = self.cultures.get(&demographics.culture) {
                fraction += culture.work_time_fraction;
            }
        }
        if demographics.stratum != 0 {
            if let Some(stratum) = self.get_stratum(demographics.stratum) {
                fraction += stratum.work_time_fraction;
            }
        }
        if demographics.religion != 0 {
            if let Some(religion) = self.religion.get(&demographics.religion) {
                fraction += religion.work_time_fraction;
            }
        }
        fraction.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod factuals_should {
    use super::*;
    use super::{CraftFile, CultureCraftFile, ReligionCraftFile};
    use crate::game::config::GameConfig;
    use crate::game::craft::Craft;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::effects::ProcessEffect;
    use crate::game::good::{GoodTag, TIME};
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
        assert_eq!(factuals.find_good(1).name, "grain");
        assert!((factuals.find_good(1).decay_rate - 0.12).abs() < 1e-12);
        assert!((factuals.find_good(1).mass - 1.0).abs() < 1e-12);
        assert!((factuals.find_good(1).volume - 0.0015).abs() < 1e-12);
        assert!((factuals.find_good(1).bulk() - 1.6).abs() < 1e-12);
        assert!(factuals.find_good(0).bulk().abs() < 1e-12);
        assert_eq!(factuals.find_good(5).name, "gold_token");
        assert!((factuals.find_good(5).decay_rate - 0.005).abs() < 1e-12);
    }

    #[test]
    fn load_from_toml_reads_crafts_and_overlays() {
        let factuals = Factuals::load_from_toml(
            r#"
[[processes]]
id = 2
name = "mill"

[[processes]]
id = 3
name = "bake"

[[processes]]
id = 29
name = "farm"

[[processes]]
id = 30
name = "water"

[[crafts]]
id = 1
name = "subsistence"
processes = [29, 30]

[[culture_crafts]]
culture = 4
name = "welsh"
craft = 1
remove = [30]
add = [3]

[[religion_crafts]]
religion = 5
name = "old faith"
craft = 1
add = [30, 2]
"#,
        )
        .expect("toml");

        assert_eq!(factuals.get_craft(1).expect("subsistence").name, "subsistence");
        assert_eq!(factuals.craft_processes(1, 0, 0), vec![29, 30]);
        assert_eq!(factuals.craft_processes(1, 4, 0), vec![29, 3]);
        assert_eq!(factuals.craft_processes(1, 4, 5), vec![29, 3, 30, 2]);
        assert!(factuals.craft_processes(0, 4, 5).is_empty());
        assert!(factuals.craft_processes(9, 4, 5).is_empty());
        assert_eq!(factuals.find_culture(4).name, "welsh");
        assert_eq!(factuals.find_religion(5).name, "old faith");
    }

    #[test]
    fn effective_craft_stacks_processes_and_modifiers() {
        let factuals = Factuals::new()
            .with_craft(
                Craft::new(1, "subsistence")
                    .with_process(1)
                    .with_process(2)
                    .with_complexity_modifier(0.4),
            )
            .with_craft(
                Craft::new(1, "subsistence")
                    .with_origin(Some(DemographicSource::Culture(2)))
                    .with_remove(2)
                    .with_process(3)
                    .with_complexity_modifier(0.5),
            )
            .with_craft(
                Craft::new(1, "subsistence")
                    .with_origin(Some(DemographicSource::Religion(3)))
                    .with_complexity_modifier(0.5),
            );

        let craft = factuals.effective_craft(1, 2, 3).expect("craft");
        assert_eq!(craft.processes, vec![1, 3]);
        assert!((craft.complexity_modifier - 0.1).abs() < 1e-12);
        assert_eq!(factuals.craft_processes(1, 2, 3), vec![1, 3]);
        assert!(factuals.effective_craft(0, 2, 3).is_none());
        assert!(factuals.effective_craft(9, 2, 3).is_none());
    }

    #[test]
    fn load_from_toml_reads_craft_complexity_modifiers() {
        let factuals = Factuals::load_from_toml(
            r#"
[[processes]]
id = 1
name = "mill"

[[processes]]
id = 29
name = "farm"

[[processes]]
id = 30
name = "water"

[[crafts]]
id = 1
name = "subsistence"
processes = [29, 30]
complexity_modifier = 0.4

[[crafts]]
id = 2
name = "plain"
processes = [1]

[[culture_crafts]]
culture = 4
name = "welsh"
craft = 1
complexity_modifier = 0.5

[[culture_crafts]]
culture = 4
craft = 2

[[religion_crafts]]
religion = 5
name = "old faith"
craft = 1
complexity_modifier = 0.5
"#,
        )
        .expect("toml");

        assert!((factuals.get_craft(1).expect("subsistence").complexity_modifier - 0.4).abs() < 1e-12);
        assert!((factuals.get_craft(2).expect("plain").complexity_modifier - 1.0).abs() < 1e-12);
        assert!(
            (factuals
                .craft(1, Some(DemographicSource::Culture(4)))
                .expect("welsh")
                .complexity_modifier
                - 0.5)
                .abs()
                < 1e-12
        );
        assert!(
            (factuals
                .craft(2, Some(DemographicSource::Culture(4)))
                .expect("plain attachment")
                .complexity_modifier
                - 1.0)
                .abs()
                < 1e-12
        );
        assert!(
            (factuals
                .craft(1, Some(DemographicSource::Religion(5)))
                .expect("old faith")
                .complexity_modifier
                - 0.5)
                .abs()
                < 1e-12
        );

        let stacked = factuals.effective_craft(1, 4, 5).expect("stacked");
        assert!((stacked.complexity_modifier - 0.1).abs() < 1e-12);
    }

    #[test]
    fn load_rejects_a_non_finite_craft_modifier() {
        let mut factuals = Factuals::new();
        let err = factuals
            .insert_craft_file(CraftFile {
                id: 1,
                name: "bad".into(),
                processes: vec![],
                complexity_modifier: f64::NAN,
            })
            .expect_err("nan");
        assert!(matches!(err, FactualsLoadError::InvalidCraft(_)));
        assert!(factuals.get_craft(1).is_none());

        let err = factuals
            .insert_culture_craft(CultureCraftFile {
                culture: 4,
                name: "welsh".into(),
                craft: 1,
                add: vec![],
                remove: vec![],
                complexity_modifier: f64::INFINITY,
            })
            .expect_err("infinity");
        assert!(matches!(err, FactualsLoadError::InvalidCraft(_)));
        assert!(factuals.cultures.get(&4).is_none());
    }

    #[test]
    fn load_from_toml_rejects_craft_zero_and_duplicates() {
        let zero = Factuals::load_from_toml(
            r#"
[[crafts]]
id = 0
name = "none"
"#,
        );
        assert!(zero.is_err());

        let repeated = Factuals::load_from_toml(
            r#"
[[processes]]
id = 29
name = "farm"

[[crafts]]
id = 1
name = "subsistence"
processes = [29, 29]
"#,
        );
        assert!(repeated.is_err());

        let duplicate = Factuals::load_from_toml(
            r#"
[[crafts]]
id = 1
name = "subsistence"

[[crafts]]
id = 1
name = "again"
"#,
        );
        assert!(duplicate.is_err());
    }

    #[test]
    fn load_from_toml_rejects_a_craft_process_that_is_not_loaded() {
        let missing = Factuals::load_from_toml(
            r#"
[[crafts]]
id = 1
name = "subsistence"
processes = [32]
"#,
        );
        let msg = missing.expect_err("missing process").to_string();
        assert!(msg.contains("craft 1"), "{msg}");
        assert!(msg.contains("32"), "{msg}");

        let loaded = Factuals::load_from_toml(
            r#"
[[processes]]
id = 32
name = "known"

[[crafts]]
id = 1
name = "subsistence"
processes = [32]
"#,
        )
        .expect("known process");
        assert_eq!(loaded.get_craft(1).expect("craft").processes, vec![32]);
    }

    #[test]
    fn load_rejects_unknown_or_repeated_attached_processes() {
        let mut factuals = Factuals::load_from_toml(
            r#"
[[processes]]
id = 3
name = "bake"

[[processes]]
id = 29
name = "farm"

[[crafts]]
id = 1
name = "subsistence"
processes = [29]
"#,
        )
        .expect("base");

        let unknown_add = factuals
            .insert_culture_craft(CultureCraftFile {
                culture: 4,
                name: "welsh".into(),
                craft: 1,
                add: vec![32],
                remove: vec![],
                complexity_modifier: 1.0,
            })
            .expect_err("unknown add");
        let msg = unknown_add.to_string();
        assert!(msg.contains("craft 1"), "{msg}");
        assert!(msg.contains("32"), "{msg}");
        assert!(factuals.cultures.get(&4).is_none());
        assert!(factuals.craft(1, Some(DemographicSource::Culture(4))).is_none());

        let repeated = factuals
            .insert_culture_craft(CultureCraftFile {
                culture: 4,
                name: "welsh".into(),
                craft: 1,
                add: vec![3, 3],
                remove: vec![],
                complexity_modifier: 1.0,
            })
            .expect_err("repeated add");
        assert!(repeated.to_string().contains("repeats process 3"));
        assert!(factuals.cultures.get(&4).is_none());

        let unknown_remove = factuals
            .insert_religion_craft(ReligionCraftFile {
                religion: 5,
                name: "old faith".into(),
                craft: 1,
                add: vec![],
                remove: vec![39],
                complexity_modifier: 1.0,
            })
            .expect_err("unknown remove");
        let msg = unknown_remove.to_string();
        assert!(msg.contains("39"), "{msg}");
        assert!(factuals.religion.get(&5).is_none());

        let both = factuals
            .insert_culture_craft(CultureCraftFile {
                culture: 4,
                name: "welsh".into(),
                craft: 1,
                add: vec![3],
                remove: vec![3],
                complexity_modifier: 1.0,
            })
            .expect_err("add and remove");
        assert!(both.to_string().contains("adds and removes process 3"));
        assert!(factuals.cultures.get(&4).is_none());
        assert_eq!(factuals.get_craft(1).expect("open").processes, vec![29]);
    }

    #[test]
    fn load_from_path_reads_world_crafts() {
        let factuals = Factuals::load_from_path(repo_world_dir()).expect("world dir");
        let subsistence = factuals.get_craft(1).expect("subsistence");
        assert_eq!(subsistence.name, "subsistence");
        assert_eq!(subsistence.processes, vec![29, 30, 31]);
        assert!((subsistence.complexity_modifier - 1.0).abs() < 1e-12);
        assert_eq!(
            factuals.get_craft(5).expect("subsistence farming").processes,
            vec![29, 30, 31, 1]
        );
        assert_eq!(
            factuals.get_craft(6).expect("subsistence watering").processes,
            vec![29, 30, 31, 2]
        );
        assert_eq!(
            factuals.get_craft(7).expect("subsistence baking").processes,
            vec![29, 30, 31, 3]
        );
    }

    #[test]
    fn load_from_path_reads_world_dir_goods_and_processes() {
        let factuals = Factuals::load_from_path(repo_world_dir()).expect("world dir");
        assert!(!factuals.goods.is_empty());
        assert!(factuals.processes.len() >= factuals.goods.len());
        const RAW_EXTRACTS: &[&str] = &[
            "grain", "water", "gold", "wood", "iron", "copper", "tin", "bronze", "coal",
            "clay",
        ];
        for process in factuals.processes.values() {
            let time_in = process
                .inputs
                .iter()
                .find(|input| input.good == TIME)
                .expect("Time input");
            assert!(time_in.amount > 0.0);
            // Subsistence feeds the household and may take more than one time unit.
            if !process.is_subsistence() {
                assert!(
                    time_in.amount <= 1.0,
                    "{} time {} exceeds 1",
                    process.name,
                    time_in.amount
                );
            }
            assert!(matches!(time_in.input_type, InputType::Destroyed));
            assert!(!time_in.is_optional());
            assert_eq!(process.outputs.len(), 1);
            assert!(process.outputs[0].amount > 0.0 && process.outputs[0].amount <= 8.0);
            assert!(process.complexity > 0.0);
            let output_name = factuals
                .goods
                .get(&process.outputs[0].good)
                .map(|good| good.name.as_str())
                .unwrap_or("");
            let required_material = process.inputs.iter().any(|input| {
                input.good != TIME && !input.is_optional()
            });
            if process.is_subsistence() || process.outputs[0].good == TIME {
                assert!(!required_material, "{} should be Time-only", process.name);
                if process.is_subsistence() {
                    assert!(process.complexity < 1.0);
                    assert!((process.complexity - 0.25).abs() < 1e-12);
                }
            } else if RAW_EXTRACTS.contains(&output_name) {
                assert!(
                    !required_material,
                    "{} extract should not require a material",
                    process.name
                );
            } else {
                assert!(required_material, "{} needs a material input", process.name);
            }
        }
        let grain = factuals.processes.get(&1).expect("make grain");
        assert_eq!(grain.name, "make grain");
        assert_eq!(grain.outputs[0].good, 1);
        assert!(!grain.is_subsistence());
        assert!((grain.complexity - 1.0).abs() < 1e-12);
        assert_eq!(grain.inputs.len(), 3);
        assert!((grain.inputs[0].amount - 0.5).abs() < 1e-12);
        assert!(grain.inputs[1].is_optional());
        assert!(grain.inputs[2].is_optional());
        assert!((grain.outputs[0].amount - 6.0).abs() < 1e-12);
        let farm = factuals.processes.get(&29).expect("subsistence farm");
        assert!(farm.is_subsistence());
        assert!((farm.complexity - 0.25).abs() < 1e-12);
        assert_eq!(farm.outputs[0].good, 1);
        assert_eq!(farm.inputs.len(), 1);
        let pots = factuals.processes.get(&27).expect("make pots");
        assert_eq!(pots.name, "make pots");
        assert_eq!(pots.outputs[0].good, 27);
        assert_eq!(pots.inputs.len(), 3);
        assert_eq!(pots.inputs[1].good, 26);
        assert_eq!(pots.inputs[2].good, 23);
        assert!((pots.outputs[0].amount - 1.0).abs() < 1e-12);
        let time = factuals.processes.get(&28).expect("make time");
        assert_eq!(time.name, "make time");
        assert_eq!(time.outputs[0].good, 0);
        assert_eq!(time.inputs.len(), 1);
        assert!((time.outputs[0].amount - 1.0).abs() < 1e-12);
        assert_eq!(factuals.config, GameConfig::default());
        assert_eq!(factuals.config.market.friction, 1.0);
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
        let mut stratum = Stratum::new(3, "tenants", 1);
        stratum.household_changed = true;

        let mut factuals = Factuals::new()
            .with_species(species)
            .with_culture(culture)
            .with_stratum(stratum)
            .with_religion(religion);

        factuals.clear_household_changed_flags();

        assert!(!factuals.species[&0].household_changed);
        assert!(!factuals.cultures[&1].household_changed);
        assert!(!factuals.strata[&3].household_changed);
        assert!(!factuals.religion[&2].household_changed);
    }

    #[test]
    fn add_stratum_records_the_id_on_its_culture() {
        let factuals = Factuals::new()
            .with_culture(Culture::new(1, "farmers"))
            .with_stratum(Stratum::new(2, "tenants", 1));

        assert_eq!(factuals.find_culture(1).strata, vec![2]);
        assert_eq!(factuals.find_stratum(2).culture, 1);
    }

    #[test]
    #[should_panic(expected = "Stratum 0 is empty.")]
    fn add_stratum_rejects_the_empty_id() {
        let _ = Factuals::new()
            .with_culture(Culture::new(1, "farmers"))
            .with_stratum(Stratum::new(0, "none", 1));
    }

    #[test]
    #[should_panic(expected = "derives from culture 0")]
    fn add_stratum_rejects_an_empty_culture() {
        let _ = Factuals::new().with_stratum(Stratum::new(1, "tenants", 0));
    }

    #[test]
    fn work_time_fraction_adds_the_stratum() {
        use crate::game::household::Household;
        use crate::game::pop::DemoRow;

        let mut culture = Culture::new(1, "farmers");
        culture.work_time_fraction = 0.1;
        let mut stratum = Stratum::new(2, "tenants", 1);
        stratum.work_time_fraction = 0.05;
        let factuals = Factuals::new()
            .with_culture(culture)
            .with_stratum(stratum);
        let row = DemoRow {
            household: Household::new(),
            species: 0,
            culture: 1,
            stratum: 2,
            religion: 0,
        };

        // Missing species uses 0.5. Culture and stratum add their fractions.
        assert!((factuals.work_time_fraction(row) - 0.65).abs() < 1e-12);
    }

    #[test]
    fn get_demographic_rates_stacks_species_culture_religion() {
        use crate::game::household::{DemographicRates, Household};
        use crate::game::pop::DemoRow;

        let mut species = Species::new(0, "Human");
        let mut species_mod = DemographicRates::zero();
        species_mod.birth_per_woman = 0.01;
        species.species_demo_eff = species_mod;

        let mut culture = Culture::new(1, "Test");
        let mut culture_mod = DemographicRates::zero();
        culture_mod.infant_mortality = 0.05;
        culture.culture_demo_eff = culture_mod;

        let mut stratum = Stratum::new(3, "tenants", 1);
        let mut stratum_mod = DemographicRates::zero();
        stratum_mod.maternal_mortality = 0.02;
        stratum.stratum_demo_eff = stratum_mod;

        let mut religion = Religion::new(2, "Faith");
        let mut religion_mod = DemographicRates::zero();
        religion_mod.adult_mortality.0 = -0.001;
        religion.religion_demo_eff = religion_mod;

        let factuals = Factuals::new()
            .with_species(species)
            .with_culture(culture)
            .with_stratum(stratum)
            .with_religion(religion);
        let row = DemoRow {
            household: Household::new(),
            species: 0,
            culture: 1,
            stratum: 3,
            religion: 2,
        };
        let rates = factuals.get_demographic_rates(row);
        let expected = DemographicRates::baseline()
            .add(&{
                let mut m = DemographicRates::zero();
                m.birth_per_woman = 0.01;
                m
            })
            .add(&{
                let mut m = DemographicRates::zero();
                m.infant_mortality = 0.05;
                m
            })
            .add(&{
                let mut m = DemographicRates::zero();
                m.maternal_mortality = 0.02;
                m
            })
            .add(&{
                let mut m = DemographicRates::zero();
                m.adult_mortality.0 = -0.001;
                m
            });
        assert_eq!(rates, expected);
    }
}