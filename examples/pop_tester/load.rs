//! Opening market and pops for the pop tester, read from TOML.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use simpler_economy::game::actors::Actors;
use simpler_economy::game::culture::Culture;
use simpler_economy::game::desire::{DemoDesire, DesireTarget, DesireTargetType};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::household::Household;
use simpler_economy::game::job::{Job, JobLine};
use simpler_economy::game::market::{Market, MarketGood};
use simpler_economy::game::pop::{Pop, PopPRow};
use simpler_economy::game::religion::Religion;
use simpler_economy::game::scalingfactor::ScalingFactor;
use simpler_economy::game::species::Species;
use simpler_economy::game::stratum::Stratum;

/// # Loaded Scenario
///
/// The market and the pops from one scenario folder.
pub struct Scenario {
    pub market: Market,
    pub actors: Actors,
}

#[derive(Debug, Deserialize)]
struct MarketFile {
    #[serde(default)]
    market: Vec<MarketRow>,
}

#[derive(Debug, Deserialize)]
struct MarketRow {
    good: String,
    #[serde(default = "one")]
    amv: f64,
    #[serde(default = "one")]
    salability: f64,
}

#[derive(Debug, Deserialize)]
struct PopFile {
    #[serde(default)]
    pops: Vec<PopRow>,
}

#[derive(Debug, Deserialize)]
struct PopRow {
    id: usize,
    #[serde(default)]
    craft: String,
    #[serde(default)]
    species: String,
    #[serde(default)]
    culture: String,
    #[serde(default)]
    stratum: String,
    #[serde(default)]
    religion: String,
    #[serde(default)]
    household: HouseholdRow,
    #[serde(default)]
    stock: Vec<StockRow>,
    #[serde(default)]
    lines: Vec<LineRow>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct HouseholdRow {
    count: f64,
    adult: f64,
    elder: f64,
    child: f64,
}

impl Default for HouseholdRow {
    fn default() -> Self {
        let household = Household::new();
        Self {
            count: household.count,
            adult: household.adult,
            elder: household.elder,
            child: household.child,
        }
    }
}

#[derive(Debug, Deserialize)]
struct StockRow {
    good: String,
    quantity: f64,
}

#[derive(Debug, Deserialize)]
struct LineRow {
    process: String,
    target: f64,
}

#[derive(Debug, Deserialize)]
struct SpeciesFile {
    #[serde(default)]
    species: Vec<GroupRow>,
}

#[derive(Debug, Deserialize)]
struct CultureFile {
    #[serde(default)]
    cultures: Vec<GroupRow>,
}

#[derive(Debug, Deserialize)]
struct ReligionFile {
    #[serde(default)]
    religions: Vec<GroupRow>,
}

#[derive(Debug, Deserialize)]
struct StratumFile {
    #[serde(default)]
    strata: Vec<StratumRow>,
}

#[derive(Debug, Deserialize)]
struct StratumRow {
    id: usize,
    name: String,
    culture: String,
    #[serde(default)]
    desires: Vec<DesireRow>,
}

#[derive(Debug, Deserialize)]
struct GroupRow {
    id: usize,
    name: String,
    #[serde(default)]
    desires: Vec<DesireRow>,
}

#[derive(Debug, Deserialize)]
struct DesireRow {
    id: usize,
    tier: usize,
    #[serde(default)]
    good: String,
    #[serde(default)]
    goods: Vec<DesireGoodRow>,
    amount: f64,
    #[serde(default = "one")]
    efficiency: f64,
    #[serde(default = "one")]
    cap: f64,
    #[serde(default = "fixed_scalar")]
    scalar: String,
    #[serde(default = "one")]
    scale: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct DesireGoodRow {
    good: String,
    #[serde(default = "one")]
    efficiency: f64,
    #[serde(default = "one")]
    cap: f64,
}

fn one() -> f64 {
    1.0
}

fn fixed_scalar() -> String {
    "fixed".to_string()
}

/// # Load Scenario
///
/// Reads `dir` against `factuals` and returns the opening market and pops.
///
/// `market.toml` is the opening board. `pops.toml` is each pop's craft,
/// household, stock, and first-morning lines. `species.toml`,
/// `cultures.toml`, `strata.toml`, and `religions.toml` are read when
/// present. Their desires are stored on `factuals`, and
/// [`Pop::update_desires`] copies them onto each pop. Good, process, and
/// craft names are the world names. An empty craft name is craft `0`. An
/// empty species name is species `0`. An empty culture, stratum, or
/// religion name is none.
pub fn load_scenario(dir: &Path, factuals: &mut Factuals) -> Result<Scenario, String> {
    let goods = names(&factuals.goods, |good| &good.name, "good")?;
    let processes = names(&factuals.processes, |process| &process.name, "process")?;
    let mut open_crafts = HashMap::new();
    for ((id, origin), craft) in &factuals.crafts {
        if origin.is_none() {
            open_crafts.insert(*id, craft);
        }
    }
    let crafts = names(&open_crafts, |craft| &craft.name, "craft")?;

    let species = load_species(dir, factuals, &goods)?;
    let cultures = load_cultures(dir, factuals, &goods)?;
    let strata = load_strata(dir, factuals, &goods, &cultures)?;
    let religions = load_religions(dir, factuals, &goods)?;

    let market_file: MarketFile = read_toml(&dir.join("market.toml"))?;
    let mut market = Market::new(1).with_friction(factuals.config.market.friction);
    let mut seen_goods = HashSet::new();
    for row in &market_file.market {
        let id = lookup(&goods, "good", &row.good)?;
        if !seen_goods.insert(id) {
            return Err(format!("market lists {} twice", row.good));
        }
        if !row.amv.is_finite() || !row.salability.is_finite() {
            return Err(format!("market good {} needs a finite amv and salability", row.good));
        }
        market.goods.insert(
            id,
            MarketGood::new()
                .with_amv(row.amv)
                .with_salability(row.salability),
        );
    }

    let pop_file: PopFile = read_toml(&dir.join("pops.toml"))?;
    let mut actors = Actors::new();
    let mut seen_pops = HashSet::new();
    for row in pop_file.pops {
        if !seen_pops.insert(row.id) {
            return Err(format!("pop {} is listed twice", row.id));
        }
        let craft = if row.craft.is_empty() {
            0
        } else {
            lookup(&crafts, "craft", &row.craft)?
        };
        let mut lines = Vec::new();
        for line in &row.lines {
            if !line.target.is_finite() || line.target < 0.0 {
                return Err(format!(
                    "pop {} process {} needs a target >= 0",
                    row.id, line.process
                ));
            }
            let process = lookup(&processes, "process", &line.process)?;
            lines.push(JobLine::new(process, Some(line.target), Vec::new()));
        }
        let mut pop = Pop::new(row.id);
        pop.job = Job::new(craft, lines);
        let mut household = Household::new();
        household.count = row.household.count;
        household.adult = row.household.adult;
        household.elder = row.household.elder;
        household.child = row.household.child;
        pop.demographics.household = household;
        pop.demographics.species = if row.species.is_empty() {
            0
        } else {
            lookup(&species, "species", &row.species)?
        };
        pop.demographics.culture = if row.culture.is_empty() {
            0
        } else {
            lookup(&cultures, "culture", &row.culture)?
        };
        pop.demographics.stratum = if row.stratum.is_empty() {
            0
        } else {
            lookup(&strata, "stratum", &row.stratum)?
        };
        pop.demographics.religion = if row.religion.is_empty() {
            0
        } else {
            lookup(&religions, "religion", &row.religion)?
        };
        let mut seen_stock = HashSet::new();
        for stock in &row.stock {
            if !stock.quantity.is_finite() || stock.quantity < 0.0 {
                return Err(format!(
                    "pop {} stock {} needs a quantity >= 0",
                    row.id, stock.good
                ));
            }
            let good = lookup(&goods, "good", &stock.good)?;
            if !seen_stock.insert(good) {
                return Err(format!("pop {} lists {} twice", row.id, stock.good));
            }
            pop.property.insert(good, PopPRow::new(stock.quantity));
        }
        pop.update_desires(factuals);
        market.pops.insert(pop.id);
        actors.pops.insert(pop.id, pop);
    }

    Ok(Scenario { market, actors })
}

/// # Load Species
///
/// Reads `species.toml` when `dir` has one, and stores each species on `factuals`.
///
/// `goods` resolves desire goods. Returns the name-to-id map. A missing file
/// leaves species empty. A repeated id or name is an error. Culture,
/// stratum, and religion use the same desire rows.
fn load_species(
    dir: &Path,
    factuals: &mut Factuals,
    goods: &HashMap<String, usize>,
) -> Result<HashMap<String, usize>, String> {
    let path = dir.join("species.toml");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let file: SpeciesFile = read_toml(&path)?;
    let mut names = HashMap::new();
    for row in file.species {
        if factuals.species.contains_key(&row.id) {
            return Err(format!("species {} is listed twice", row.id));
        }
        if names.insert(row.name.clone(), row.id).is_some() {
            return Err(format!("species name {} is listed twice", row.name));
        }
        let desires = demo_desires(&row.name, &row.desires, goods)?;
        let mut species = Species::new(row.id, row.name);
        species.desires = desires;
        factuals.species.insert(species.id, species);
    }
    Ok(names)
}

/// # Load Cultures
///
/// Reads `cultures.toml` when `dir` has one, and stores each culture on `factuals`.
///
/// `goods` resolves desire goods. Returns the name-to-id map. Id `0` is
/// rejected because a pop treats that id as no culture.
fn load_cultures(
    dir: &Path,
    factuals: &mut Factuals,
    goods: &HashMap<String, usize>,
) -> Result<HashMap<String, usize>, String> {
    let path = dir.join("cultures.toml");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let file: CultureFile = read_toml(&path)?;
    let mut names = HashMap::new();
    for row in file.cultures {
        if row.id == 0 {
            return Err(format!("culture {} uses id 0, which means none", row.name));
        }
        if factuals.cultures.contains_key(&row.id) {
            return Err(format!("culture {} is listed twice", row.id));
        }
        if names.insert(row.name.clone(), row.id).is_some() {
            return Err(format!("culture name {} is listed twice", row.name));
        }
        let desires = demo_desires(&row.name, &row.desires, goods)?;
        let mut culture = Culture::new(row.id, row.name);
        culture.desires = desires;
        factuals.cultures.insert(culture.id, culture);
    }
    Ok(names)
}

/// # Load Strata
///
/// Reads `strata.toml` when `dir` has one, and stores each stratum on `factuals`.
///
/// `goods` resolves desire goods. `cultures` resolves the culture each
/// stratum derives from. Returns the name-to-id map. Id `0` is rejected
/// because a pop treats that id as no stratum. The culture's list records
/// the new id.
fn load_strata(
    dir: &Path,
    factuals: &mut Factuals,
    goods: &HashMap<String, usize>,
    cultures: &HashMap<String, usize>,
) -> Result<HashMap<String, usize>, String> {
    let path = dir.join("strata.toml");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let file: StratumFile = read_toml(&path)?;
    let mut names = HashMap::new();
    for row in file.strata {
        if row.id == 0 {
            return Err(format!("stratum {} uses id 0, which means none", row.name));
        }
        if factuals.strata.contains_key(&row.id) {
            return Err(format!("stratum {} is listed twice", row.id));
        }
        if names.insert(row.name.clone(), row.id).is_some() {
            return Err(format!("stratum name {} is listed twice", row.name));
        }
        let culture = lookup(cultures, "culture", &row.culture)?;
        let desires = demo_desires(&row.name, &row.desires, goods)?;
        let mut stratum = Stratum::new(row.id, row.name, culture);
        stratum.desires = desires;
        factuals.add_stratum(stratum);
    }
    Ok(names)
}

/// # Load Religions
///
/// Reads `religions.toml` when `dir` has one, and stores each religion on `factuals`.
///
/// `goods` resolves desire goods. Returns the name-to-id map. Id `0` is
/// rejected because a pop treats that id as no religion.
fn load_religions(
    dir: &Path,
    factuals: &mut Factuals,
    goods: &HashMap<String, usize>,
) -> Result<HashMap<String, usize>, String> {
    let path = dir.join("religions.toml");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let file: ReligionFile = read_toml(&path)?;
    let mut names = HashMap::new();
    for row in file.religions {
        if row.id == 0 {
            return Err(format!("religion {} uses id 0, which means none", row.name));
        }
        if factuals.religion.contains_key(&row.id) {
            return Err(format!("religion {} is listed twice", row.id));
        }
        if names.insert(row.name.clone(), row.id).is_some() {
            return Err(format!("religion name {} is listed twice", row.name));
        }
        let desires = demo_desires(&row.name, &row.desires, goods)?;
        let mut religion = Religion::new(row.id, row.name);
        religion.desires = desires;
        factuals.religion.insert(religion.id, religion);
    }
    Ok(names)
}

/// # Demo Desires
///
/// Builds the desire map for one species, culture, stratum, or religion.
///
/// `owner` is that group's name, used in errors. `rows` is the file order.
/// `goods` resolves each target. The map is keyed by desire id. One row is
/// one desire. Its bucket is the single `good`, or every entry in `goods`.
/// Each target keeps that entry's efficiency and cap. Priority follows the
/// file order.
fn demo_desires(
    owner: &str,
    rows: &[DesireRow],
    goods: &HashMap<String, usize>,
) -> Result<HashMap<usize, DemoDesire>, String> {
    let mut desires = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        if row.tier > 2 {
            return Err(format!("{owner} desire {} tier {} is past luxury", row.id, row.tier));
        }
        if !row.amount.is_finite() || row.amount <= 0.0 {
            return Err(format!("{owner} desire {} needs an amount > 0", row.id));
        }
        let scalar = scaling(&row.scalar, row.scale).map_err(|err| {
            format!("{owner} desire {} {err}", row.id)
        })?;
        let mut desire = DemoDesire::new(row.id)
            .with_tier(row.tier)
            .with_amount(row.amount)
            .with_priority(index as isize)
            .with_scalar(scalar);
        // One row, one bucket. A repeated good on that row is an error.
        let mut seen = HashSet::new();
        for target in desire_targets(owner, row)? {
            if !target.efficiency.is_finite() || target.efficiency <= 0.0 {
                return Err(format!(
                    "{owner} desire {} good {} needs an efficiency > 0",
                    row.id, target.good
                ));
            }
            if !target.cap.is_finite() || target.cap <= 0.0 || target.cap > 1.0 {
                return Err(format!(
                    "{owner} desire {} good {} needs a cap > 0 and <= 1",
                    row.id, target.good
                ));
            }
            let good = lookup(goods, "good", &target.good)?;
            if !seen.insert(good) {
                return Err(format!("{owner} desire {} lists {} twice", row.id, target.good));
            }
            desire = desire.with_good(
                DesireTarget::new(good, DesireTargetType::Consume, target.efficiency)
                    .with_cap(target.cap),
            );
        }
        if desires.insert(row.id, desire).is_some() {
            return Err(format!("{owner} lists desire {} twice", row.id));
        }
    }
    Ok(desires)
}

/// # Desire Targets
///
/// The goods that satisfy one desire row, in file order.
///
/// `owner` names the demographic in errors. A row sets `good` or `goods`.
/// Each entry is the good name, its efficiency, and its cap. Listing both,
/// or neither, is an error.
fn desire_targets(owner: &str, row: &DesireRow) -> Result<Vec<DesireGoodRow>, String> {
    if !row.good.is_empty() && !row.goods.is_empty() {
        return Err(format!(
            "{owner} desire {} lists a good and a goods list",
            row.id
        ));
    }
    if !row.good.is_empty() {
        return Ok(vec![DesireGoodRow {
            good: row.good.clone(),
            efficiency: row.efficiency,
            cap: row.cap,
        }]);
    }
    if row.goods.is_empty() {
        return Err(format!("{owner} desire {} needs a good", row.id));
    }
    Ok(row.goods.clone())
}

/// # Scaling
///
/// The [`ScalingFactor`] named by `name`, multiplied by `scale`.
///
/// `fixed`, `household`, `all`, `adults`, `children`, `elders`, and `labor`
/// are the names. `scale` must be finite.
fn scaling(name: &str, scale: f64) -> Result<ScalingFactor, String> {
    if !scale.is_finite() {
        return Err("needs a finite scale".to_string());
    }
    match name {
        "fixed" => Ok(ScalingFactor::Fixed(scale)),
        "household" => Ok(ScalingFactor::Household(scale)),
        "all" => Ok(ScalingFactor::All(scale)),
        "adults" => Ok(ScalingFactor::Adults(scale)),
        "children" => Ok(ScalingFactor::Children(scale)),
        "elders" => Ok(ScalingFactor::Elders(scale)),
        "labor" => Ok(ScalingFactor::Labor(scale)),
        other => Err(format!("has unknown scalar {other}")),
    }
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    toml::from_str(&text).map_err(|err| format!("parse {}: {err}", path.display()))
}

fn names<T>(
    rows: &HashMap<usize, T>,
    name_of: impl Fn(&T) -> &String,
    kind: &str,
) -> Result<HashMap<String, usize>, String> {
    let mut names = HashMap::new();
    for (id, row) in rows {
        let name = name_of(row);
        if names.insert(name.clone(), *id).is_some() {
            return Err(format!("world data has two {kind}s named {name}"));
        }
    }
    Ok(names)
}

fn lookup(names: &HashMap<String, usize>, kind: &str, name: &str) -> Result<usize, String> {
    names
        .get(name)
        .copied()
        .ok_or_else(|| format!("no {kind} named {name}"))
}
