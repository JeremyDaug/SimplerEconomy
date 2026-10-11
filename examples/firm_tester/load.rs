//! Firms for the firm tester, read from TOML.
//!
//! The market and pops come from the pop tester's loader. This file adds
//! the firms on top of them.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use hexx::Hex;
use serde::Deserialize;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::actors::Actors;
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::{Firm, FirmPRow, ProductionLine};
use simpler_economy::game::market::Market;

use crate::pop_load::{lookup, names, read_toml};

#[derive(Debug, Deserialize)]
struct FirmFile {
    #[serde(default)]
    firms: Vec<FirmRow>,
}

#[derive(Debug, Deserialize)]
struct FirmRow {
    id: usize,
    name: String,
    #[serde(default)]
    owner: usize,
    #[serde(default)]
    profit_share: Option<f64>,
    #[serde(default)]
    liable: bool,
    #[serde(default)]
    stock: Vec<FirmStockRow>,
    #[serde(default)]
    lines: Vec<FirmLineRow>,
}

#[derive(Debug, Deserialize)]
struct FirmStockRow {
    good: String,
    quantity: f64,
    #[serde(default)]
    reserve: f64,
}

#[derive(Debug, Deserialize)]
struct FirmLineRow {
    process: String,
    #[serde(default)]
    target: Option<f64>,
}

/// # Load Firms
///
/// Reads `firms.toml` from `dir` and adds each firm to `actors` and `market`.
///
/// `factuals` resolves good and process names. Each firm gets its owner,
/// stock, and production lines and sits on hex `(0, 0)` of `market`. An
/// owner is a pop id already listed on `market`, and `0` is no owner. The owner
/// takes `profit_share`, or carries the residual when `liable` is set.
/// Setting both, a repeated firm id, or a repeated stock good is an error.
/// A reserve must be between 0 and that row's quantity.
pub fn load_firms(
    dir: &Path,
    factuals: &Factuals,
    market: &mut Market,
    actors: &mut Actors,
) -> Result<(), String> {
    let goods = names(&factuals.goods, |good| &good.name, "good")?;
    let processes = names(&factuals.processes, |process| &process.name, "process")?;
    let file: FirmFile = read_toml(&dir.join("firms.toml"))?;
    let mut seen_firms = HashSet::new();
    for row in file.firms {
        if !seen_firms.insert(row.id) {
            return Err(format!("firm {} is listed twice", row.id));
        }
        let mut firm = Firm::new(row.id, row.name.clone(), market.id, Hex::new(0, 0));
        firm = owned(firm, &row, &market.pops)?;
        firm.property = stock(&row, &goods)?;
        for line in &row.lines {
            if let Some(target) = line.target {
                if !target.is_finite() || target < 0.0 {
                    return Err(format!(
                        "firm {} process {} needs a target >= 0",
                        row.id, line.process
                    ));
                }
            }
            let process = lookup(&processes, "process", &line.process)?;
            firm.production_line.push(ProductionLine {
                process,
                target: line.target,
                inputs: Vec::new(),
            });
        }
        market.firms.insert(firm.id);
        actors.firms.insert(firm.id, firm);
    }
    Ok(())
}

/// # Owned
///
/// Returns `firm` with the owner terms from `row`.
///
/// `pops` is the market's pop list and must hold the owner. Owner `0`
/// leaves the blank owner. A `profit_share` outside `0..=1`, or one set beside `liable`, is
/// an error.
fn owned(firm: Firm, row: &FirmRow, pops: &HashSet<usize>) -> Result<Firm, String> {
    if row.owner == 0 {
        return Ok(firm);
    }
    if !pops.contains(&row.owner) {
        return Err(format!("firm {} owner pop {} is not in pops.toml", row.id, row.owner));
    }
    let firm = firm.with_owner(Actor::Pop(row.owner));
    match (row.profit_share, row.liable) {
        (Some(_), true) => Err(format!(
            "firm {} sets both profit_share and liable",
            row.id
        )),
        (Some(share), false) if !(0.0..=1.0).contains(&share) => Err(format!(
            "firm {} needs a profit_share from 0 to 1",
            row.id
        )),
        (Some(share), false) => Ok(firm.with_owner_profit_share(share)),
        (None, true) => Ok(firm.with_owner_liability()),
        (None, false) => Ok(firm),
    }
}

/// # Stock
///
/// The opening property rows for one firm, keyed by good id.
///
/// `goods` resolves each name. Quantity must be `>= 0`. Reserve must be
/// `>= 0` and no more than that quantity.
fn stock(row: &FirmRow, goods: &HashMap<String, usize>) -> Result<HashMap<usize, FirmPRow>, String> {
    let mut property = HashMap::new();
    for stock in &row.stock {
        if !stock.quantity.is_finite() || stock.quantity < 0.0 {
            return Err(format!(
                "firm {} stock {} needs a quantity >= 0",
                row.id, stock.good
            ));
        }
        if !stock.reserve.is_finite() || stock.reserve < 0.0 || stock.reserve > stock.quantity {
            return Err(format!(
                "firm {} stock {} needs a reserve from 0 to its quantity",
                row.id, stock.good
            ));
        }
        let good = lookup(goods, "good", &stock.good)?;
        let entry = FirmPRow::new()
            .with_quantity(stock.quantity)
            .with_reserve(stock.reserve);
        if property.insert(good, entry).is_some() {
            return Err(format!("firm {} lists {} twice", row.id, stock.good));
        }
    }
    Ok(property)
}
