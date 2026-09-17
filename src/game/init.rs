//! Kickoff actors for a new game (initialization data).
//!
//! Human-editable scenario TOML: pops, firms, starting property. Separate
//! from world factuals and from a later compressed save. Defaults keep the
//! files short (shared desires/starter, remainder owner, name from output).
//! Firm `target` is process iterations; opening stock is three decay-adjusted
//! days of each output (`OPENING_COVER_DAYS`) so day 1 can sell. Required
//! non-Time inputs get four days (`OPENING_INPUT_DAYS`) with `use_target` set
//! to one day's recipe use so they are not sold. Live remainder fence is still
//! `firm.operations_cover`. Hours default to the sum of each line's
//! target * Time input (specialty plus auto-attached subsistence).

/// Days of output stocked at kickoff (decay-adjusted). Live `stock_target`
/// still follows `operations_cover` after the first plan.
pub const OPENING_COVER_DAYS: f64 = 3.0;

/// Days of required non-Time inputs stocked at kickoff. Quantity and
/// `stock_target` are this many days of recipe use; `use_target` is one day.
pub const OPENING_INPUT_DAYS: f64 = 4.0;

/// Iterations for each auto-attached subsistence line.
pub const SUBSISTENCE_LINE_TARGET: f64 = 2.0;

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use hexx::Hex;
use serde::Deserialize;

use crate::game::actor::Actor;
use crate::game::config::PopConfig;
use crate::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
use crate::game::factuals::Factuals;
use crate::game::firm::{Firm, FirmPRow, ProductionLine};
use crate::game::good::TIME;
use crate::game::household::Household;
use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
use crate::game::process::Process;
use crate::game::scalingfactor::ScalingFactor;
use crate::game::sentiment::Sentiment;
use crate::game::workforce::Workforce;

/// Loaded kickoff pops and firms.
#[derive(Debug, Clone)]
pub struct InitData {
    pub pops: Vec<Pop>,
    pub firms: Vec<Firm>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PopsFile {
    #[serde(default)]
    desires: Vec<DesireFile>,
    #[serde(default)]
    starter: Vec<PropertyFile>,
    #[serde(default)]
    pops: Vec<PopFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FirmsFile {
    #[serde(default)]
    firms: Vec<FirmFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesireFile {
    tier: usize,
    id: usize,
    name: String,
    #[serde(default = "default_one")]
    amount: f64,
    #[serde(default)]
    scalar: ScalarFile,
    #[serde(default = "default_one")]
    weight: f64,
    targets: Vec<TargetFile>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScalarFile {
    #[default]
    All,
    Household,
    Adults,
    Children,
    Elders,
    Labor,
    Fixed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetFile {
    good: GoodRef,
    #[serde(default = "default_one")]
    efficiency: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyFile {
    good: GoodRef,
    #[serde(default)]
    quantity: f64,
    #[serde(default)]
    shop: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PopFile {
    id: usize,
    #[serde(default = "default_one")]
    household: f64,
    #[serde(default)]
    property: Vec<PropertyFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FirmFile {
    id: usize,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    owner: Option<usize>,
    #[serde(default = "default_true")]
    remainder: bool,
    process: GoodRef,
    #[serde(default)]
    hours: Option<f64>,
    #[serde(default = "default_one")]
    workers: f64,
    #[serde(default)]
    target: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GoodRef {
    Id(usize),
    Name(String),
}

fn default_one() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

/// Failed to load initialization data.
#[derive(Debug)]
pub enum InitLoadError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    DuplicatePop(usize),
    DuplicateFirm(usize),
    UnknownGood(String),
    UnknownProcess(String),
    Invalid(String),
}

impl fmt::Display for InitLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "read init data: {err}"),
            Self::Toml(err) => write!(f, "parse init data: {err}"),
            Self::DuplicatePop(id) => write!(f, "duplicate pop id {id} in init data"),
            Self::DuplicateFirm(id) => write!(f, "duplicate firm id {id} in init data"),
            Self::UnknownGood(name) => write!(f, "unknown good {name} in init data"),
            Self::UnknownProcess(name) => write!(f, "unknown process {name} in init data"),
            Self::Invalid(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for InitLoadError {}

impl InitData {
    /// Loads `pops.toml` and `firms.toml` from an init-data folder.
    pub fn load_from_path(
        dir: impl AsRef<Path>,
        factuals: &Factuals,
    ) -> Result<Self, InitLoadError> {
        let dir = dir.as_ref();
        let pops_text = std::fs::read_to_string(dir.join("pops.toml")).map_err(InitLoadError::Io)?;
        let firms_text =
            std::fs::read_to_string(dir.join("firms.toml")).map_err(InitLoadError::Io)?;
        Self::load_from_toml(&pops_text, &firms_text, factuals)
    }

    /// Loads pops and firms from TOML text.
    pub fn load_from_toml(
        pops_text: &str,
        firms_text: &str,
        factuals: &Factuals,
    ) -> Result<Self, InitLoadError> {
        let pops_file: PopsFile = toml::from_str(pops_text).map_err(InitLoadError::Toml)?;
        let firms_file: FirmsFile = toml::from_str(firms_text).map_err(InitLoadError::Toml)?;
        let pop_cfg = &factuals.config.pop;
        let mut pops = Vec::new();
        let mut seen_pops = HashMap::new();
        for file in pops_file.pops {
            if seen_pops.insert(file.id, ()).is_some() {
                return Err(InitLoadError::DuplicatePop(file.id));
            }
            pops.push(build_pop(file, &pops_file.desires, &pops_file.starter, factuals, pop_cfg)?);
        }
        pops.sort_by_key(|pop| pop.id);
        let mut firms = Vec::new();
        let mut seen_firms = HashMap::new();
        for file in firms_file.firms {
            if seen_firms.insert(file.id, ()).is_some() {
                return Err(InitLoadError::DuplicateFirm(file.id));
            }
            firms.push(build_firm(file, factuals)?);
        }
        firms.sort_by_key(|firm| firm.id);
        grant_opening_specialty(&mut pops, &firms, factuals);
        Ok(Self { pops, firms })
    }
}

/// One day of the matching remainder firm's daily output, so pops can tender
/// on day 1. Time output is skipped (untradeable).
fn grant_opening_specialty(pops: &mut [Pop], firms: &[Firm], factuals: &Factuals) {
    for pop in pops.iter_mut() {
        let Some(firm) = firms.iter().find(|firm| firm.id == pop.id) else {
            continue;
        };
        let Some(line) = firm.production_line.first() else {
            continue;
        };
        let Some(process) = factuals.processes.get(&line.process) else {
            continue;
        };
        let target = line.target.unwrap_or(0.0);
        if target <= 0.0 {
            continue;
        }
        for output in &process.outputs {
            if output.good == TIME {
                continue;
            }
            let qty = output.amount * target;
            if qty <= 0.0 {
                continue;
            }
            if pop.property.contains_key(&output.good) {
                continue;
            }
            pop.property.insert(output.good, PopPRow::new(qty));
        }
    }
}

fn build_pop(
    file: PopFile,
    desires: &[DesireFile],
    starter: &[PropertyFile],
    factuals: &Factuals,
    pop_cfg: &PopConfig,
) -> Result<Pop, InitLoadError> {
    if file.id == 0 {
        return Err(InitLoadError::Invalid("pop id 0 is none".into()));
    }
    if file.household < 0.0 {
        return Err(InitLoadError::Invalid(format!(
            "pop {} household must be >= 0",
            file.id
        )));
    }
    let mut household = Household::new();
    household.count = file.household;
    let mut pop = Pop {
        id: file.id,
        job: 0,
        property: HashMap::new(),
        desires: vec![vec![]; 3],
        working_desires: vec![],
        demographics: DemoRow {
            household,
            species: 0,
            culture: 0,
            class: 0,
            religion: 0,
        },
        current_orders: vec![],
        stored_effects: vec![],
        sentiment: Sentiment::new(),
        records: PopRecords::from_config(pop_cfg),
    };
    for row in starter {
        insert_pop_property(&mut pop, row, factuals)?;
    }
    for row in &file.property {
        insert_pop_property(&mut pop, row, factuals)?;
    }
    for desire in desires {
        if desire.tier > 2 {
            return Err(InitLoadError::Invalid(format!(
                "desire {} tier must be 0..=2",
                desire.id
            )));
        }
        if desire.amount <= 0.0 {
            return Err(InitLoadError::Invalid(format!(
                "desire {} amount must be > 0",
                desire.id
            )));
        }
        let scalar = desire.scalar.to_scaling(desire.weight);
        let amount = pop.get_scaling_factor(scalar) * desire.amount;
        let mut targets = Vec::new();
        for target in &desire.targets {
            if target.efficiency <= 0.0 {
                return Err(InitLoadError::Invalid(format!(
                    "desire {} target efficiency must be > 0",
                    desire.id
                )));
            }
            let good = resolve_good(&target.good, factuals)?;
            targets.push(DesireTarget::new(good, DesireTargetType::Consume, target.efficiency));
        }
        pop.desires[desire.tier].push(Desire {
            source: DesireSource::Species(0, desire.id),
            priority: desire.id as isize,
            target: targets,
            amount,
            satisfaction: 0.0,
            category: Some(desire.name.clone()),
            effect: vec![],
            scalar,
            decay: 0.0,
        });
    }
    Ok(pop)
}

fn insert_pop_property(
    pop: &mut Pop,
    row: &PropertyFile,
    factuals: &Factuals,
) -> Result<(), InitLoadError> {
    if row.quantity < 0.0 || row.shop < 0.0 {
        return Err(InitLoadError::Invalid(format!(
            "pop {} property amounts must be >= 0",
            pop.id
        )));
    }
    let good = resolve_good(&row.good, factuals)?;
    pop.property
        .insert(good, PopPRow::new(row.quantity).with_target(row.shop));
    Ok(())
}

fn build_firm(file: FirmFile, factuals: &Factuals) -> Result<Firm, InitLoadError> {
    if file.id == 0 {
        return Err(InitLoadError::Invalid("firm id 0 is none".into()));
    }
    if file.workers < 0.0
        || file.hours.map(|hours| hours < 0.0).unwrap_or(false)
        || file.target.map(|target| target < 0.0).unwrap_or(false)
    {
        return Err(InitLoadError::Invalid(format!(
            "firm {} hours, workers, and target must be >= 0",
            file.id
        )));
    }
    let process = resolve_process(&file.process, factuals)?;
    let time_in = process
        .inputs
        .iter()
        .find(|input| input.good == TIME)
        .map(|input| input.amount)
        .unwrap_or(1.0);
    if time_in <= 0.0 {
        return Err(InitLoadError::Invalid(format!(
            "process {} Time input must be > 0",
            process.id
        )));
    }
    let target = match (file.target, file.hours) {
        (Some(target), _) => target,
        (None, Some(hours)) => hours / time_in,
        (None, None) => 0.0,
    };
    let output = process.outputs.first();
    let output_good = output.map(|row| row.good);
    let output_name = output_good
        .and_then(|id| factuals.goods.get(&id).map(|g| g.name.as_str()))
        .unwrap_or("good");
    let name = file
        .name
        .clone()
        .unwrap_or_else(|| format!("firm{}-{output_name}", file.id));
    let owner = file.owner.unwrap_or(file.id);
    let mut firm = Firm::new(file.id, name, 1, Hex::new(0, 0));
    firm = firm.with_owner(Actor::Pop(owner));
    if file.remainder {
        firm = firm.with_owner_remainder();
    }
    add_production_line(&mut firm, process, target, factuals);
    attach_subsistence_lines(&mut firm, factuals);
    let hours = file.hours.unwrap_or_else(|| line_hours(&firm, factuals));
    firm = firm.with_workforce(
        Workforce::new(owner)
            .with_workers(file.workers, file.workers)
            .with_hours(hours),
    );
    Ok(firm)
}

fn add_production_line(firm: &mut Firm, process: &Process, target: f64, factuals: &Factuals) {
    let inputs: Vec<usize> = process
        .inputs
        .iter()
        .filter(|input| !input.is_optional())
        .map(|input| input.good)
        .collect();
    let succeeded = target > 0.0;
    firm.production_line.push(ProductionLine {
        process: process.id,
        target: Some(target),
        inputs,
        historical_productivity: 0.0,
        aim: target,
        last_success_rate: if succeeded { 1.0 } else { 0.0 },
        last_iterations: if succeeded { target } else { 0.0 },
        last_effects: vec![],
        last_missing_goods: vec![],
        last_amv_consumed: 0.0,
        last_amv_produced: 0.0,
        idle_days: 0,
    });
    for output in &process.outputs {
        if output.good == TIME {
            continue;
        }
        let daily = output.amount * target;
        if daily <= 0.0 {
            continue;
        }
        let decay = factuals
            .goods
            .get(&output.good)
            .map(|g| g.decay_rate)
            .unwrap_or(1.0);
        let opening = FirmPRow::operations_opening(daily, OPENING_COVER_DAYS, decay);
        let row = firm.property.entry(output.good).or_insert_with(FirmPRow::new);
        row.quantity += opening.quantity;
        row.stock_target += opening.stock_target;
        row.sell_target += opening.sell_target;
    }
    for input in process.requirements() {
        if input.good == TIME {
            continue;
        }
        let daily = input.amount * target;
        if daily <= 0.0 {
            continue;
        }
        let qty = daily * OPENING_INPUT_DAYS;
        let row = firm.property.entry(input.good).or_insert_with(FirmPRow::new);
        row.quantity += qty;
        row.use_target += daily;
        row.stock_target += qty;
    }
}

fn attach_subsistence_lines(firm: &mut Firm, factuals: &Factuals) {
    let mut tagged: Vec<&Process> = factuals
        .processes
        .values()
        .filter(|process| process.is_subsistence())
        .collect();
    tagged.sort_by_key(|process| process.id);
    for process in tagged {
        if firm
            .production_line
            .iter()
            .any(|line| line.process == process.id)
        {
            continue;
        }
        add_production_line(firm, process, SUBSISTENCE_LINE_TARGET, factuals);
    }
}

fn line_hours(firm: &Firm, factuals: &Factuals) -> f64 {
    firm.production_line.iter().fold(0.0, |hours, line| {
        let time_in = factuals
            .processes
            .get(&line.process)
            .and_then(|process| {
                process
                    .inputs
                    .iter()
                    .find(|input| input.good == TIME)
                    .map(|input| input.amount)
            })
            .unwrap_or(1.0);
        hours + line.target.unwrap_or(0.0) * time_in
    })
}

fn resolve_good(r: &GoodRef, factuals: &Factuals) -> Result<usize, InitLoadError> {
    match r {
        GoodRef::Id(id) => {
            if factuals.goods.contains_key(id) {
                Ok(*id)
            } else {
                Err(InitLoadError::UnknownGood(id.to_string()))
            }
        }
        GoodRef::Name(name) => factuals
            .goods
            .values()
            .find(|g| g.name.eq_ignore_ascii_case(name))
            .map(|g| g.id)
            .ok_or_else(|| InitLoadError::UnknownGood(name.clone())),
    }
}

fn resolve_process<'a>(r: &GoodRef, factuals: &'a Factuals) -> Result<&'a Process, InitLoadError> {
    match r {
        GoodRef::Id(id) => factuals
            .processes
            .get(id)
            .ok_or_else(|| InitLoadError::UnknownProcess(id.to_string())),
        GoodRef::Name(name) => factuals
            .processes
            .values()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| InitLoadError::UnknownProcess(name.clone())),
    }
}

impl ScalarFile {
    fn to_scaling(self, weight: f64) -> ScalingFactor {
        match self {
            Self::All => ScalingFactor::All(weight),
            Self::Household => ScalingFactor::Household(weight),
            Self::Adults => ScalingFactor::Adults(weight),
            Self::Children => ScalingFactor::Children(weight),
            Self::Elders => ScalingFactor::Elders(weight),
            Self::Labor => ScalingFactor::Labor(weight),
            Self::Fixed => ScalingFactor::Fixed(weight),
        }
    }
}

#[cfg(test)]
mod init_should {
    use super::*;

    fn subsistence_output_opening(factuals: &Factuals, good: usize) -> (f64, f64, f64) {
        let decay = factuals.goods.get(&good).map(|g| g.decay_rate).unwrap_or(1.0);
        let mut qty = 0.0;
        let mut stock = 0.0;
        let mut sell = 0.0;
        for process in factuals.processes.values() {
            if !process.is_subsistence() {
                continue;
            }
            for output in &process.outputs {
                if output.good != good {
                    continue;
                }
                let daily = output.amount * SUBSISTENCE_LINE_TARGET;
                let opening =
                    FirmPRow::operations_opening(daily, OPENING_COVER_DAYS, decay);
                qty += opening.quantity;
                stock += opening.stock_target;
                sell += opening.sell_target;
            }
        }
        (qty, stock, sell)
    }

    fn tiny_factuals() -> Factuals {
        Factuals::load_from_toml(
            r#"
[[goods]]
id = 0
name = "time"
mass = 0.0
volume = 0.0
tags = ["untradeable", { transport = 1.0 }]

[[goods]]
id = 1
name = "grain"
mass = 1.0
volume = 1.0

[[processes]]
id = 1
name = "make grain"
inputs = [{ good = 0, amount = 1.0 }]
outputs = [{ good = 1, amount = 15.0 }]
"#,
        )
        .expect("tiny factuals")
    }

    #[test]
    fn load_from_toml_reads_a_pop_and_firm() {
        let factuals = tiny_factuals();
        let data = InitData::load_from_toml(
            r#"
[[desires]]
tier = 0
id = 0
name = "food"
targets = [{ good = "grain", efficiency = 1.0 }]

[[starter]]
good = "grain"
quantity = 1.0
shop = 2.0

[[pops]]
id = 1
"#,
            r#"
[[firms]]
id = 1
process = "make grain"
target = 10.0
"#,
            &factuals,
        )
        .expect("init");
        assert_eq!(data.pops.len(), 1);
        assert_eq!(data.firms.len(), 1);
        let pop = &data.pops[0];
        assert_eq!(pop.id, 1);
        assert_eq!(pop.desires[0].len(), 1);
        assert_eq!(pop.desires[0][0].category.as_deref(), Some("food"));
        assert!((pop.property[&1].quantity - 1.0).abs() < 1e-12);
        let firm = &data.firms[0];
        assert_eq!(firm.id, 1);
        assert_eq!(firm.owners.owner, Actor::Pop(1));
        assert!(firm.owners.remainder);
        assert!((firm.workforce[0].hours - 10.0).abs() < 1e-12);
        assert_eq!(firm.production_line[0].process, 1);
        assert!((firm.production_line[0].target.unwrap() - 10.0).abs() < 1e-12);
        assert!((firm.production_line[0].last_iterations - 10.0).abs() < 1e-12);
        assert!((firm.production_line[0].last_success_rate - 1.0).abs() < 1e-12);
        // tiny grain has default decay 1.0: hold nothing overnight.
        assert!((firm.property[&1].quantity - 0.0).abs() < 1e-12);
        assert!((firm.property[&1].stock_target - 0.0).abs() < 1e-12);
        assert!((firm.property[&1].sell_target - 150.0).abs() < 1e-12);
    }

    #[test]
    fn load_from_path_reads_scenario_init() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/world");
        let factuals = Factuals::load_from_path(&dir).expect("world");
        let init_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/init");
        let data = InitData::load_from_path(&init_dir, &factuals).expect("init dir");
        let pop = data.pops.iter().find(|p| p.id == 1).expect("pop 1");
        assert_eq!(pop.desires[0].len(), 3);
        assert_eq!(pop.desires[1].len(), 2);
        assert_eq!(pop.desires[2].len(), 1);
        assert_eq!(pop.desires[0][0].category.as_deref(), Some("food"));
        assert_eq!(pop.desires[1][0].category.as_deref(), Some("housing"));
        assert_eq!(pop.desires[2][0].category.as_deref(), Some("shiny"));
        assert!((pop.desires[1][0].amount - 1.0).abs() < 1e-12);
        let firm = data.firms.iter().find(|f| f.id == 1).expect("firm 1");
        assert_eq!(firm.name, "firm1-grain");
        let grain_target = firm.production_line[0].target.unwrap();
        assert!((grain_target - 8.0).abs() < 1e-12);
        assert!((firm.production_line[0].last_iterations - grain_target).abs() < 1e-12);
        assert!((firm.production_line[0].last_success_rate - 1.0).abs() < 1e-12);
        let grain_decay = factuals.goods[&1].decay_rate;
        let hold = FirmPRow::operations_hold_days(OPENING_COVER_DAYS, grain_decay);
        let grain_daily = factuals.processes[&1].outputs[0].amount * grain_target;
        let farm_daily =
            factuals.processes[&29].outputs[0].amount * SUBSISTENCE_LINE_TARGET;
        assert!((pop.property[&1].quantity - grain_daily).abs() < 1e-12);
        assert_eq!(firm.production_line.len(), 4);
        assert!(firm.production_line.iter().any(|line| line.process == 29));
        assert!(
            (firm.workforce[0].hours - (grain_target * 0.5 + 2.4)).abs() < 1e-12
        );
        assert!(
            (firm.property[&1].quantity - (grain_daily + farm_daily) * hold).abs() < 1e-12
        );
        assert!(
            (firm.property[&1].stock_target - (grain_daily + farm_daily) * hold).abs()
                < 1e-12
        );
        assert!(
            (firm.property[&1].sell_target - (grain_daily + farm_daily)).abs() < 1e-12
        );
        assert!((firm.property[&1].use_target - 0.0).abs() < 1e-12);
        let bread = data.firms.iter().find(|f| f.id == 3).expect("firm 3");
        let bread_target = bread.production_line[0].target.unwrap();
        assert!((bread_target - 5.0).abs() < 1e-12);
        assert!(bread.production_line[0].inputs.contains(&TIME));
        assert!(bread.production_line[0].inputs.contains(&1));
        assert!(bread.production_line[0].inputs.contains(&2));
        let bread_proc = &factuals.processes[&bread.production_line[0].process];
        for input in bread_proc.requirements() {
            if input.good == TIME {
                continue;
            }
            let daily = input.amount * bread_target;
            let row = bread.property.get(&input.good).expect("opening input");
            assert!((row.use_target - daily).abs() < 1e-12);
            let extra = subsistence_output_opening(&factuals, input.good);
            assert!((row.quantity - daily * OPENING_INPUT_DAYS - extra.0).abs() < 1e-12);
            assert!(
                (row.stock_target - daily * OPENING_INPUT_DAYS - extra.1).abs() < 1e-12
            );
            assert!((row.sell_target - extra.2).abs() < 1e-12);
        }
        assert_eq!(data.pops.len(), 8);
        assert_eq!(data.firms.len(), 8);
        let grain2 = data.firms.iter().find(|f| f.id == 5).expect("firm 5");
        assert_eq!(grain2.name, "firm5-grain");
        assert_eq!(grain2.owners.owner, Actor::Pop(5));
        assert!((grain2.production_line[0].target.unwrap() - 8.0).abs() < 1e-12);
        let cabin = data.firms.iter().find(|f| f.id == 8).expect("firm 8");
        assert!((cabin.production_line[0].target.unwrap() - 2.0).abs() < 1e-12);
        let water2 = data.firms.iter().find(|f| f.id == 6).expect("firm 6");
        assert_eq!(water2.name, "firm6-water");
        assert_eq!(water2.owners.owner, Actor::Pop(6));
    }

    #[test]
    fn load_from_toml_errors_when_root_keys_follow_desires() {
        let factuals = tiny_factuals();
        let err = InitData::load_from_toml(
            r#"
[[desires]]
tier = 0
id = 0
name = "food"
targets = [{ good = "grain" }]

pops = [{ id = 1 }]
"#,
            "",
            &factuals,
        )
        .expect_err("swallowed root keys");
        match err {
            InitLoadError::Toml(_) => {}
            other => panic!("expected Toml, got {other}"),
        }
    }

    #[test]
    fn load_from_toml_errors_on_duplicate_pop() {
        let factuals = tiny_factuals();
        let err = InitData::load_from_toml(
            "[[pops]]\nid = 1\n\n[[pops]]\nid = 1\n",
            "",
            &factuals,
        )
        .expect_err("duplicate");
        match err {
            InitLoadError::DuplicatePop(1) => {}
            other => panic!("expected DuplicatePop(1), got {other}"),
        }
    }

    #[test]
    fn load_from_toml_errors_on_unknown_good() {
        let factuals = tiny_factuals();
        let err = InitData::load_from_toml(
            r#"
[[starter]]
good = "nope"
quantity = 1.0

[[pops]]
id = 1
"#,
            "",
            &factuals,
        )
        .expect_err("unknown");
        match err {
            InitLoadError::UnknownGood(name) => assert_eq!(name, "nope"),
            other => panic!("expected UnknownGood, got {other}"),
        }
    }
}
