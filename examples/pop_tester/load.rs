//! The pop tester's scenario folder.
//!
//! `desires.toml` holds platonic desires. `species.toml` and `cultures.toml`
//! derive demographic desires from those names. `pops.toml` and `market.toml`
//! name goods and processes from the loaded world.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use simpler_economy::game::culture::Culture;
use simpler_economy::game::demographic_source::DemographicSource;
use simpler_economy::game::desire::{DemoDesire, DesireTarget, DesireTargetType, PlatonicDesire};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::job::{Job, JobLine};
use simpler_economy::game::market::Market;
use simpler_economy::game::pop::Pop;
use simpler_economy::game::scalingfactor::ScalingFactor;
use simpler_economy::game::species::Species;

/// Species, cultures, pops, and markets loaded from one scenario folder.
pub(crate) struct Scenario {
    pub(crate) species: Vec<Species>,
    pub(crate) cultures: Vec<Culture>,
    pub(crate) pops: Vec<Pop>,
    pub(crate) markets: Vec<Market>,
}

/// A scenario file that could not be read or applied.
#[derive(Debug)]
pub(crate) struct ScenarioError {
    message: String,
}

impl ScenarioError {
    /// # New
    ///
    /// An error whose text is `message`.
    ///
    /// Returns that error.
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// # Default Tier
///
/// The tier [`DemoDesire::new`] starts on.
///
/// Returns 1.
fn default_tier() -> usize {
    1
}

/// # Default Priority
///
/// The priority [`DemoDesire::new`] starts on.
///
/// Returns 1.
fn default_priority() -> isize {
    1
}

/// # Default One
///
/// A missing desire scale or good efficiency.
///
/// Returns 1.
fn default_one() -> f64 {
    1.0
}

#[derive(Deserialize)]
struct DesireFile {
    #[serde(default)]
    desires: Vec<PlatonicRow>,
}

#[derive(Deserialize)]
struct PlatonicRow {
    id: usize,
    name: String,
    #[serde(default)]
    decay: f64,
    #[serde(default)]
    scalar: Option<String>,
    #[serde(default = "default_one")]
    scale: f64,
    #[serde(default)]
    goods: Vec<GoodRow>,
}

#[derive(Deserialize)]
struct GoodRow {
    good: String,
    #[serde(default = "default_one")]
    efficiency: f64,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct SpeciesFile {
    #[serde(default)]
    species: Vec<SpeciesRow>,
}

#[derive(Deserialize)]
struct SpeciesRow {
    id: usize,
    name: String,
    #[serde(default)]
    desires: Vec<DemoLink>,
}

#[derive(Deserialize)]
struct CultureFile {
    #[serde(default)]
    cultures: Vec<CultureRow>,
}

#[derive(Deserialize)]
struct CultureRow {
    id: usize,
    name: String,
    #[serde(default)]
    desires: Vec<DemoLink>,
}

#[derive(Deserialize)]
struct DemoLink {
    id: usize,
    desire: String,
    #[serde(default = "default_tier")]
    tier: usize,
    #[serde(default = "default_priority")]
    priority: isize,
    amount: f64,
}

#[derive(Deserialize)]
struct PopFile {
    #[serde(default)]
    pops: Vec<PopRow>,
}

#[derive(Deserialize)]
struct PopRow {
    id: usize,
    species: String,
    #[serde(default)]
    culture: Option<String>,
    #[serde(default)]
    lines: Vec<LineRow>,
}

#[derive(Deserialize)]
struct LineRow {
    process: String,
    #[serde(default)]
    target: Option<f64>,
    #[serde(default)]
    inputs: Vec<String>,
}

#[derive(Deserialize)]
struct MarketFile {
    id: usize,
    #[serde(default)]
    currencies: Vec<String>,
    #[serde(default)]
    pops: Option<Vec<usize>>,
}

/// # Load Scenario
///
/// Species, cultures, pops, and the market in `dir`, named against `factuals`.
///
/// `dir` holds `desires.toml`, `species.toml`, `cultures.toml`, `pops.toml`,
/// and `market.toml`. Good and process names are world names. A species or
/// culture desire is derived from the platonic desire it names. A pop copies
/// both. A job of more than two lines is an error. Returns that stand-up.
pub(crate) fn load_scenario(dir: &Path, factuals: &Factuals) -> Result<Scenario, ScenarioError> {
    let goods = name_index(
        factuals.goods.iter().map(|(id, good)| (*id, good.name.as_str())),
        "good",
    )?;
    let processes = name_index(
        factuals.processes.iter().map(|(id, process)| (*id, process.name.as_str())),
        "process",
    )?;
    // Platonic desires first, so a species or culture can derive from a name.
    let desires = load_platonic(&dir.join("desires.toml"), &goods)?;
    let species = load_species(&dir.join("species.toml"), &desires)?;
    let cultures = load_cultures(&dir.join("cultures.toml"), &desires)?;
    let pops = load_pops(&dir.join("pops.toml"), &species, &cultures, &processes, &goods)?;
    let market = load_market(&dir.join("market.toml"), &goods, &pops)?;
    Ok(Scenario { species, cultures, pops, markets: vec![market] })
}

/// # Load Platonic
///
/// The platonic desires in `path`, keyed by name.
///
/// `goods` resolves bucket names. Id 0 is reserved. A repeated id or name,
/// or an empty bucket, is an error. Returns that map.
fn load_platonic(path: &Path, goods: &HashMap<String, usize>) -> Result<HashMap<String, PlatonicDesire>, ScenarioError> {
    let file: DesireFile = read_toml(path)?;
    let mut desires = HashMap::new();
    for row in file.desires {
        if desires.values().any(|desire: &PlatonicDesire| desire.id == row.id) {
            return Err(ScenarioError::new(format!("{}: desire id {} is repeated", path.display(), row.id)));
        }
        if desires.contains_key(&row.name) {
            return Err(ScenarioError::new(format!("{}: desire name {} is used twice", path.display(), row.name)));
        }
        let name = row.name.clone();
        desires.insert(name, build_platonic(path, row, goods)?);
    }
    Ok(desires)
}

/// # Build Platonic
///
/// One platonic desire.
///
/// `path` names the file in an error. `goods` resolves the bucket. Decay is
/// from 0 to 1. An omitted scalar is [`ScalingFactor::All`]. Returns that desire.
fn build_platonic(path: &Path, row: PlatonicRow, goods: &HashMap<String, usize>) -> Result<PlatonicDesire, ScenarioError> {
    if row.id == 0 {
        return Err(ScenarioError::new(format!("{}: desire id 0 is reserved", path.display())));
    }
    if row.name.is_empty() {
        return Err(ScenarioError::new(format!("{}: a desire needs a name", path.display())));
    }
    if !row.decay.is_finite() || !(0.0..=1.0).contains(&row.decay) {
        return Err(ScenarioError::new(format!(
            "{}: desire {} decay must be from 0 to 1",
            path.display(), row.name
        )));
    }
    if !row.scale.is_finite() {
        return Err(ScenarioError::new(format!("{}: desire {} scale is not finite", path.display(), row.name)));
    }
    if row.goods.is_empty() {
        return Err(ScenarioError::new(format!("{}: desire {} needs a good", path.display(), row.name)));
    }
    let scalar = match &row.scalar {
        Some(name) => scaling(path, &row.name, name, row.scale)?,
        None => ScalingFactor::All(row.scale),
    };
    let mut desire = PlatonicDesire::new(row.id).with_decay(row.decay).with_scalar(scalar);
    for good in &row.goods {
        let id = lookup(goods, &good.good, "good")
            .map_err(|err| ScenarioError::new(format!("{}: {err}", path.display())))?;
        if desire.bucket.iter().any(|target| target.good == id) {
            return Err(ScenarioError::new(format!(
                "{}: desire {} repeats {}",
                path.display(), row.name, good.good
            )));
        }
        if !good.efficiency.is_finite() || good.efficiency <= 0.0 {
            return Err(ScenarioError::new(format!(
                "{}: desire {} good {} needs a positive efficiency",
                path.display(), row.name, good.good
            )));
        }
        desire = desire.with_good(DesireTarget::new(id, desire_kind(path, &row.name, &good.kind)?, good.efficiency));
    }
    Ok(desire)
}

/// # Load Species
///
/// The species in `path`.
///
/// `desires` resolves each demographic desire by platonic name. A repeated
/// species id or name is an error. Returns those species.
fn load_species(path: &Path, desires: &HashMap<String, PlatonicDesire>) -> Result<Vec<Species>, ScenarioError> {
    let file: SpeciesFile = read_toml(path)?;
    let mut species: Vec<Species> = Vec::new();
    for row in file.species {
        if row.name.is_empty() {
            return Err(ScenarioError::new(format!("{}: a species needs a name", path.display())));
        }
        if species.iter().any(|other| other.id == row.id) {
            return Err(ScenarioError::new(format!("{}: species id {} is repeated", path.display(), row.id)));
        }
        if species.iter().any(|other| other.name == row.name) {
            return Err(ScenarioError::new(format!("{}: species name {} is used twice", path.display(), row.name)));
        }
        let mut built = Species::new(row.id, row.name);
        for desire in demo_desires(path, row.desires, desires)? {
            built = built.with_desire(desire);
        }
        species.push(built);
    }
    Ok(species)
}

/// # Load Cultures
///
/// The cultures in `path`.
///
/// `desires` resolves each demographic desire by platonic name. A repeated
/// culture id or name is an error. Returns those cultures.
fn load_cultures(path: &Path, desires: &HashMap<String, PlatonicDesire>) -> Result<Vec<Culture>, ScenarioError> {
    let file: CultureFile = read_toml(path)?;
    let mut cultures: Vec<Culture> = Vec::new();
    for row in file.cultures {
        if row.name.is_empty() {
            return Err(ScenarioError::new(format!("{}: a culture needs a name", path.display())));
        }
        if cultures.iter().any(|other| other.id == row.id) {
            return Err(ScenarioError::new(format!("{}: culture id {} is repeated", path.display(), row.id)));
        }
        if cultures.iter().any(|other| other.name == row.name) {
            return Err(ScenarioError::new(format!("{}: culture name {} is used twice", path.display(), row.name)));
        }
        let mut built = Culture::new(row.id, row.name);
        for desire in demo_desires(path, row.desires, desires)? {
            built = built.with_desire(desire);
        }
        cultures.push(built);
    }
    Ok(cultures)
}

/// # Demo Desires
///
/// Demographic desires derived from `rows`.
///
/// `path` names the file in an error. `desires` is the platonic map. A
/// repeated demographic id, or a name that is not platonic, is an error.
/// Returns those desires in file order.
fn demo_desires(
    path: &Path, rows: Vec<DemoLink>, desires: &HashMap<String, PlatonicDesire>,
) -> Result<Vec<DemoDesire>, ScenarioError> {
    let mut made = Vec::new();
    for row in rows {
        if made.iter().any(|desire: &DemoDesire| desire.id == row.id) {
            return Err(ScenarioError::new(format!("{}: desire id {} is repeated", path.display(), row.id)));
        }
        let platonic = desires.get(&row.desire).ok_or_else(|| {
            ScenarioError::new(format!("{}: desire {} is not in desires.toml", path.display(), row.desire))
        })?;
        made.push(derive_demo(path, &row, platonic)?);
    }
    Ok(made)
}

/// # Derive Demo
///
/// The demographic desire `row` takes from `platonic`.
///
/// The bucket is the whole platonic bucket. `tier` is recorded on the
/// platonic desire when it does not already allow that tier. Amount is at
/// least 1. Returns that demographic desire.
fn derive_demo(path: &Path, row: &DemoLink, platonic: &PlatonicDesire) -> Result<DemoDesire, ScenarioError> {
    if !row.amount.is_finite() || row.amount < 1.0 {
        return Err(ScenarioError::new(format!(
            "{}: desire {} amount must be at least 1",
            path.display(), row.desire
        )));
    }
    if row.tier > 2 {
        return Err(ScenarioError::new(format!(
            "{}: desire {} tier must be 0, 1, or 2",
            path.display(), row.desire
        )));
    }
    // The platonic list is the tiers this desire is allowed to use.
    let mut source = platonic.clone();
    if !source.tiers.contains(&row.tier) {
        source = source.with_tier(row.tier);
    }
    let goods: Vec<usize> = source.bucket.iter().map(|target| target.good).collect();
    Ok(source.derive_demographic_desire(row.id, row.tier, row.amount, goods, row.priority))
}

/// # Scaling
///
/// The scalar named `name` at `scale`.
///
/// `path` and `desire` name the file and desire in an error. `fixed`,
/// `all`, `household`, `adults`, `children`, `elders`, and `labor` are
/// the names. Returns that factor.
fn scaling(path: &Path, desire: &str, name: &str, scale: f64) -> Result<ScalingFactor, ScenarioError> {
    match name {
        "fixed" => Ok(ScalingFactor::Fixed(scale)),
        "all" => Ok(ScalingFactor::All(scale)),
        "household" => Ok(ScalingFactor::Household(scale)),
        "adults" => Ok(ScalingFactor::Adults(scale)),
        "children" => Ok(ScalingFactor::Children(scale)),
        "elders" => Ok(ScalingFactor::Elders(scale)),
        "labor" => Ok(ScalingFactor::Labor(scale)),
        other => Err(ScenarioError::new(format!(
            "{}: desire {desire} scalar {other} is not a known scalar",
            path.display()
        ))),
    }
}

/// # Desire Kind
///
/// Consume or use for one desire good.
///
/// An omitted kind is consume. `path` and `desire` name the file and
/// desire in an error. Returns that kind.
fn desire_kind(path: &Path, desire: &str, kind: &Option<String>) -> Result<DesireTargetType, ScenarioError> {
    match kind.as_deref().unwrap_or("consume") {
        "consume" => Ok(DesireTargetType::Consume),
        "use" => Ok(DesireTargetType::Use),
        other => Err(ScenarioError::new(format!(
            "{}: desire {desire} type {other} is not consume or use",
            path.display()
        ))),
    }
}

/// # Load Pops
///
/// The pops in `path`.
///
/// `species` and `cultures` supply desires. `processes` and `goods` resolve
/// the job. A repeated pop id is an error. More than two lines is an error.
/// Returns those pops.
fn load_pops(
    path: &Path, species: &[Species], cultures: &[Culture], processes: &HashMap<String, usize>,
    goods: &HashMap<String, usize>,
) -> Result<Vec<Pop>, ScenarioError> {
    let file: PopFile = read_toml(path)?;
    let mut pops: Vec<Pop> = Vec::new();
    for row in file.pops {
        if pops.iter().any(|other| other.id == row.id) {
            return Err(ScenarioError::new(format!("{}: pop id {} is repeated", path.display(), row.id)));
        }
        let kind = species.iter().find(|species| species.name == row.species).ok_or_else(|| {
            ScenarioError::new(format!("{}: species {} is not in species.toml", path.display(), row.species))
        })?;
        if row.lines.len() > 2 {
            return Err(ScenarioError::new(format!(
                "{}: pop {} has {} job lines and a job holds 2",
                path.display(), row.id, row.lines.len()
            )));
        }
        let mut lines = [JobLine::new(0, None, Vec::new()), JobLine::new(0, None, Vec::new())];
        for (slot, line) in row.lines.iter().enumerate() {
            lines[slot] = build_line(path, line, processes, goods)?;
        }
        let mut pop = Pop::new(row.id);
        pop.demographics.species = kind.id;
        copy_desires(&mut pop, DemographicSource::Species(kind.id), &kind.desires);
        if let Some(name) = &row.culture {
            let culture = cultures.iter().find(|culture| culture.name == *name).ok_or_else(|| {
                ScenarioError::new(format!("{}: culture {name} is not in cultures.toml", path.display()))
            })?;
            pop.demographics.culture = culture.id;
            copy_desires(&mut pop, DemographicSource::Culture(culture.id), &culture.desires);
        }
        pop.job = Job::new(lines);
        pops.push(pop);
    }
    Ok(pops)
}

/// # Build Line
///
/// One job line.
///
/// `path` names the file in an error. `processes` and `goods` resolve the
/// process and any optional inputs. Returns that line.
fn build_line(
    path: &Path, line: &LineRow, processes: &HashMap<String, usize>, goods: &HashMap<String, usize>,
) -> Result<JobLine, ScenarioError> {
    let process = processes.get(&line.process).copied().ok_or_else(|| {
        ScenarioError::new(format!("{}: process {} is not in the world", path.display(), line.process))
    })?;
    let mut inputs = Vec::new();
    for name in &line.inputs {
        inputs.push(lookup(goods, name, "good").map_err(|err| {
            ScenarioError::new(format!("{}: {err}", path.display()))
        })?);
    }
    Ok(JobLine::new(process, line.target, inputs))
}

/// # Copy Desires
///
/// Puts `desires` onto `pop` from `source`.
///
/// Desires are ordered by tier, then priority, then id. Each is appended
/// to its tier.
fn copy_desires(pop: &mut Pop, source: DemographicSource, desires: &HashMap<usize, DemoDesire>) {
    let mut ordered: Vec<_> = desires.values().collect();
    ordered.sort_by_key(|desire| (desire.tier, desire.priority, desire.id));
    let copied: Vec<_> = ordered
        .iter()
        .map(|desire| (desire.tier, desire.create_desire(pop, source)))
        .collect();
    for (tier, desire) in copied {
        pop.desires[tier].push(desire);
    }
}

/// # Load Market
///
/// The market in `path`.
///
/// `goods` resolves currency names. `pops` are the loaded pops. An omitted
/// pop list is every pop, lowest id first. A named pop that was not loaded
/// is an error. Returns that market.
fn load_market(path: &Path, goods: &HashMap<String, usize>, pops: &[Pop]) -> Result<Market, ScenarioError> {
    let file: MarketFile = read_toml(path)?;
    let members = match file.pops {
        Some(ids) => ids,
        None => {
            let mut ids: Vec<usize> = pops.iter().map(|pop| pop.id).collect();
            ids.sort_unstable();
            ids
        }
    };
    let mut market = Market::new(file.id);
    for name in &file.currencies {
        market.currencies.push(lookup(goods, name, "good").map_err(|err| {
            ScenarioError::new(format!("{}: {err}", path.display()))
        })?);
    }
    for id in members {
        if !pops.iter().any(|pop| pop.id == id) {
            return Err(ScenarioError::new(format!("{}: pop {id} is not in pops.toml", path.display())));
        }
        market.pops.insert(id);
    }
    Ok(market)
}

/// # Name Index
///
/// `pairs` of id and name, keyed by name.
///
/// A repeated name is an error naming `kind`. Returns that map.
fn name_index<'a>(pairs: impl Iterator<Item = (usize, &'a str)>, kind: &str) -> Result<HashMap<String, usize>, ScenarioError> {
    let mut index = HashMap::new();
    for (id, name) in pairs {
        if index.insert(name.to_string(), id).is_some() {
            return Err(ScenarioError::new(format!("{kind} name {name} is used twice")));
        }
    }
    Ok(index)
}

/// # Lookup
///
/// The id of `name` in `index`.
///
/// A missing name is an error naming `kind`. Returns that id.
fn lookup(index: &HashMap<String, usize>, name: &str, kind: &str) -> Result<usize, ScenarioError> {
    index.get(name).copied().ok_or_else(|| ScenarioError::new(format!("{kind} {name} is not in the world")))
}

/// # Read Toml
///
/// `path` parsed as `T`.
///
/// A missing file or a document that does not match `T` is an error.
/// Returns that value.
fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ScenarioError> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| ScenarioError::new(format!("read {}: {err}", path.display())))?;
    toml::from_str(&text).map_err(|err| ScenarioError::new(format!("parse {}: {err}", path.display())))
}
