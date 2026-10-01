//! Opening market and pops for the pop tester, read from TOML.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;
use simpler_economy::game::actors::Actors;
use simpler_economy::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::household::Household;
use simpler_economy::game::job::{Job, JobLine};
use simpler_economy::game::market::{Market, MarketGood};
use simpler_economy::game::pop::{Pop, PopPRow};
use simpler_economy::game::scalingfactor::ScalingFactor;

/// # Loaded Scenario
///
/// The market and the pops from one scenario file.
pub struct Scenario {
    pub market: Market,
    pub actors: Actors,
}

#[derive(Debug, Deserialize)]
struct ScenarioFile {
    #[serde(default)]
    market: Vec<MarketRow>,
    #[serde(default)]
    pops: Vec<PopRow>,
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
struct PopRow {
    id: usize,
    #[serde(default)]
    craft: String,
    #[serde(default)]
    household: HouseholdRow,
    #[serde(default)]
    stock: Vec<StockRow>,
    #[serde(default)]
    lines: Vec<LineRow>,
    #[serde(default)]
    desires: Vec<DesireRow>,
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
struct DesireRow {
    tier: usize,
    good: String,
    amount: f64,
}

fn one() -> f64 {
    1.0
}

/// # Load Scenario
///
/// Reads `path` against `factuals` and returns the opening market and pops.
///
/// Good, process, and craft names are the world names. A pop's household
/// comes from the file; omitted fields stay at the standard household, so
/// the day start can grant Time. A line target is that morning's iterations.
/// An empty craft name is craft `0`.
pub fn load_scenario(path: &Path, factuals: &Factuals) -> Result<Scenario, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let file: ScenarioFile = toml::from_str(&text).map_err(|err| format!("parse {}: {err}", path.display()))?;
    let goods = names(&factuals.goods, |good| &good.name, "good")?;
    let processes = names(&factuals.processes, |process| &process.name, "process")?;
    let crafts = names(&factuals.crafts, |craft| &craft.name, "craft")?;

    let mut market = Market::new(1).with_friction(factuals.config.market.friction);
    let mut seen_goods = HashSet::new();
    for row in &file.market {
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

    let mut actors = Actors::new();
    let mut seen_pops = HashSet::new();
    for row in file.pops {
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
        for (index, desire) in row.desires.iter().enumerate() {
            if desire.tier > 2 {
                return Err(format!("pop {} desire tier {} is past luxury", row.id, desire.tier));
            }
            if !desire.amount.is_finite() || desire.amount <= 0.0 {
                return Err(format!("pop {} desire {} needs an amount > 0", row.id, desire.good));
            }
            let good = lookup(&goods, "good", &desire.good)?;
            pop.desires[desire.tier].push(Desire {
                source: DesireSource::Species(0, index + 1),
                priority: index as isize,
                target: vec![DesireTarget::new(good, DesireTargetType::Consume, 1.0)],
                amount: desire.amount,
                satisfaction: 0.0,
                category: None,
                effect: vec![],
                scalar: ScalingFactor::Fixed(1.0),
                decay: 0.0,
            });
        }
        market.pops.insert(pop.id);
        actors.pops.insert(pop.id, pop);
    }

    Ok(Scenario { market, actors })
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
        .ok_or_else(|| format!("world data has no {kind} named {name}"))
}
