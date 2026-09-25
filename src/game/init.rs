//! Kickoff actors for a new game (initialization data).
//!
//! Human-editable scenario TOML: pops, firms, starting property. Separate
//! from world factuals and from a later compressed save. Defaults keep the
//! files short (shared desires/starter, remainder owner, name from output).
//! Firm `target` is process iterations; opening stock is three decay-adjusted
//! days of each output (`OPENING_COVER_DAYS`) so day 1 can sell. Required
//! non-Time inputs get four days (`OPENING_INPUT_DAYS`) with `use_target` set
//! to one day's recipe use so they are not sold. Live remainder fence is still
//! `firm.operations_cover`. Hours default to recipe Time plus the
//! complexity tax (specialty plus auto-attached subsistence).

/// Days of output stocked at kickoff (decay-adjusted). Live `stock_target`
/// still follows `operations_cover` after the first plan.
pub const OPENING_COVER_DAYS: f64 = 3.0;

/// Days of required non-Time inputs stocked at kickoff. Quantity and
/// `stock_target` are this many days of recipe use; `use_target` is one day.
pub const OPENING_INPUT_DAYS: f64 = 4.0;

/// Iterations for each auto-attached subsistence line.
pub const SUBSISTENCE_LINE_TARGET: f64 = 2.0;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::game::factuals::Factuals;
use crate::game::firm::Firm;
use crate::game::pop::Pop;
use crate::game::process::Process;
use crate::game::scalingfactor::ScalingFactor;

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
        }
        let mut firms = Vec::new();
        let mut seen_firms = HashMap::new();
        for file in firms_file.firms {
            if seen_firms.insert(file.id, ()).is_some() {
                return Err(InitLoadError::DuplicateFirm(file.id));
            }
        }
        Ok(Self { pops, firms })
    }

    /// Drops goods this roster does not reference from `factuals`. Time is
    /// always kept. Processes that mention a dropped good are removed;
    /// remaining processes lose optional inputs of dropped goods.
    pub fn unload_unused_goods(&self, factuals: &mut Factuals) {
        todo!()
    }
}

/// True when every required input and every output is in `keep`.
fn process_goods_kept(process: &Process, keep: &HashSet<usize>) -> bool {
    process.outputs.iter().all(|row| keep.contains(&row.good))
        && process
            .inputs
            .iter()
            .all(|input| input.is_optional() || keep.contains(&input.good))
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
