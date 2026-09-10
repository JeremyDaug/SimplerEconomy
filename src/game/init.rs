//! Kickoff actors for a new game (initialization data).
//!
//! Human-editable scenario TOML: pops, firms, starting property. Separate
//! from world factuals and from a later compressed save. Defaults keep the
//! files short (shared desires/starter, remainder owner, name from output).
//! Firm `target` is process iterations; opening stock is each output times
//! that target (yesterday succeeded). Hours default to target * Time input.

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
        Ok(Self { pops, firms })
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
    let hours = file.hours.unwrap_or(target * time_in);
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
    firm = firm.with_workforce(
        Workforce::new(owner)
            .with_workers(file.workers, file.workers)
            .with_hours(hours),
    );
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
        last_success_rate: if succeeded { 1.0 } else { 0.0 },
        last_iterations: if succeeded { target } else { 0.0 },
        last_effects: vec![],
        last_missing_goods: vec![],
        last_amv_consumed: 0.0,
        last_amv_produced: 0.0,
    });
    for output in &process.outputs {
        if output.good == TIME {
            continue;
        }
        let qty = output.amount * target;
        if qty <= 0.0 {
            continue;
        }
        firm.property.insert(
            output.good,
            FirmPRow::new()
                .with_quantity(qty)
                .with_sell_target(qty),
        );
    }
    Ok(firm)
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
        assert!((firm.property[&1].quantity - 150.0).abs() < 1e-12);
        assert!((firm.property[&1].sell_target - 150.0).abs() < 1e-12);
    }

    #[test]
    fn load_from_path_reads_scenario_init() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/world");
        let factuals = Factuals::load_from_path(&dir).expect("world");
        let init_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/init");
        let data = InitData::load_from_path(&init_dir, &factuals).expect("init dir");
        let pop = data.pops.iter().find(|p| p.id == 1).expect("pop 1");
        assert_eq!(pop.desires[0].len(), 4);
        assert_eq!(pop.desires[1].len(), 4);
        assert_eq!(pop.desires[2].len(), 2);
        let firm = data.firms.iter().find(|f| f.id == 1).expect("firm 1");
        assert_eq!(firm.name, "firm1-grain");
        assert!((firm.production_line[0].target.unwrap() - 10.0).abs() < 1e-12);
        assert!((firm.production_line[0].last_iterations - 10.0).abs() < 1e-12);
        assert!((firm.production_line[0].last_success_rate - 1.0).abs() < 1e-12);
        assert!((firm.property[&1].quantity - 150.0).abs() < 1e-12);
        let time = data.firms.iter().find(|f| f.id == 28).expect("firm 28");
        assert!(time.property.is_empty());
        assert!((time.production_line[0].target.unwrap() - 10.0).abs() < 1e-12);
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
