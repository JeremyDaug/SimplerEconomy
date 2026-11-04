//! Gameplay tunables. Defaults live here. `data/world/config.toml` may override them.
//!
//! Missing keys keep [`Default`]. Load rejects values outside the bounds on each
//! field. Call sites with [`crate::game::factuals::Factuals`] should read
//! `factuals.config`.
//!
//! Section structs stay so later pop, firm, and market work has a place to put
//! knobs. Most of the old shopping, priority, and planning fields are gone.

use std::fmt;
use std::path::Path;

use serde::Deserialize;

/// Time goods granted per unit of household labor.
///
/// Adult labor 1.0, elder 0.7, child 0.3 are the household defaults.
/// 1 labor is a 16 hour day, counted as 64 quarter-hours.
pub const TIME_PER_LABOR: f64 = 64.0;

/// Pop cottage-work tunables.
pub mod pop_constants {
    /// Full weight of one extra process in [`crate::game::craft::Craft::craft_distance`].
    pub const CRAFT_DISTANCE: f64 = 0.1;
    /// Turns in one demographic year when config does not say. One turn is one year.
    pub const TURN_LENGTH: f64 = 1.0;
    /// Multiplier on the mean tier price saved per household at each level.
    pub const SAVINGS_AMV: f64 = 1.0;
    /// Runs the goods sent with a new pop must be able to start.
    pub const FOUNDING_RUNS: f64 = 3.0;
    /// Days of set-aside each leaving household takes.
    pub const FOUNDING_GOODS_WEIGHT: f64 = 3.0;
    /// Share of foundings that add a profitable process or drop an unprofitable one.
    pub const FOUNDING_USEFUL_SHARE: f64 = 0.8;
    /// Days of set-aside required before a pop may split.
    pub const FOUNDING_WEALTH: f64 = 1.5;
    /// Market days from one founding to the next. One is every day.
    pub const FOUNDING_GAP: f64 = 5.0;
}

/// Transport bill pieces. Not prices.
pub mod market_constants {
    /// Flat transport units charged per meeting. Not AMV.
    pub const TRANSACTION_COST: f64 = 1.0;
    /// Multiplier on deal bulk. `transport = TRANSACTION_COST + bulk * FRICTION`.
    pub const FRICTION: f64 = 1.0;
    /// Share of working currency surplus a holder lists.
    pub const CURRENCY_HOLDER_SHARE: f64 = 0.5;
    /// Share of working currency surplus a producer of that currency lists.
    pub const CURRENCY_PRODUCER_SHARE: f64 = 1.0;
}

/// Failed to load gameplay config from a world-data file.
#[derive(Debug)]
pub enum ConfigLoadError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    /// Every bound failure from one load, one line each.
    Invalid(Vec<String>),
}

impl fmt::Display for ConfigLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "read world config: {err}"),
            Self::Toml(err) => write!(f, "parse world config: {err}"),
            Self::Invalid(msgs) => {
                write!(f, "invalid world config:")?;
                for msg in msgs {
                    write!(f, "\n- {msg}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ConfigLoadError {}

/// Loaded gameplay tunables.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct GameConfig {
    pub pop: PopConfig,
    pub player_resources: PlayerResourceConfig,
    pub market: MarketConfig,
    pub deal: DealConfig,
    pub market_priority: MarketPriorityConfig,
    pub labor: LaborConfig,
    pub firm: FirmConfig,
}

impl GameConfig {
    /// Loads tunables from a TOML file. Missing keys keep [`Default`].
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigLoadError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(ConfigLoadError::Io)?;
        Self::load_from_toml(&text)
    }

    /// Loads tunables from TOML text. Missing keys keep [`Default`].
    /// Rejects values outside the bounds written on each field.
    pub fn load_from_toml(text: &str) -> Result<Self, ConfigLoadError> {
        let cfg: Self = toml::from_str(text).map_err(ConfigLoadError::Toml)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Returns `Err` when a loaded value is outside its stated bound.
    pub fn validate(&self) -> Result<(), ConfigLoadError> {
        let mut problems = Vec::new();
        self.pop.validate(&mut problems);
        self.market.validate(&mut problems);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigLoadError::Invalid(problems))
        }
    }
}

/// Pop-day tunables.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct PopConfig {
    /// Full per-process weight for [`crate::game::craft::Craft::craft_distance`].
    ///
    /// Must be `>= 0`.
    pub craft_distance: f64,
    /// Turns in one demographic year.
    ///
    /// [`crate::game::household::Household::update`] divides the annual stock
    /// rates by this. Must be `>= 1`.
    pub turn_length: f64,
    /// Multiplier on the mean tier price saved per household at each level.
    ///
    /// Must be `>= 0`. The levels add. [`crate::game::ladder`] is the buyer.
    pub savings_amv: f64,
    /// Runs of the new job the leaving goods must be able to start. Must be `>= 1`.
    ///
    /// [`crate::game::market::Market::found_pop`] reads it.
    pub founding_runs: f64,
    /// Days of set-aside each leaving household takes.
    ///
    /// Must be `>= 1`. The days are a share of the set-aside, capped at
    /// the whole pile. Working goods move separately, with the households.
    pub founding_goods_weight: f64,
    /// Share of foundings that add a profitable process or drop an unprofitable line.
    ///
    /// Must be from 0 to 1. The rest add the process for an open bid.
    pub founding_useful_share: f64,
    /// Days of set-aside required before a pop may split.
    ///
    /// Must be `>= 0`. A day is one pass through the desire basket.
    /// Zero accepts a pop with nothing set aside.
    /// [`crate::game::market::Market::found_pop`] reads it.
    pub founding_wealth: f64,
    /// Market days from one founding to the next.
    ///
    /// Must be `>= 1`. One allows a founding every day.
    /// [`crate::game::market::Market::found_pop`] reads it.
    pub founding_gap: f64,
}

impl Default for PopConfig {
    fn default() -> Self {
        Self {
            craft_distance: pop_constants::CRAFT_DISTANCE,
            turn_length: pop_constants::TURN_LENGTH,
            savings_amv: pop_constants::SAVINGS_AMV,
            founding_runs: pop_constants::FOUNDING_RUNS,
            founding_goods_weight: pop_constants::FOUNDING_GOODS_WEIGHT,
            founding_useful_share: pop_constants::FOUNDING_USEFUL_SHARE,
            founding_wealth: pop_constants::FOUNDING_WEALTH,
            founding_gap: pop_constants::FOUNDING_GAP,
        }
    }
}

impl PopConfig {
    /// # Validate
    ///
    /// Appends a problem when `craft_distance` is non-finite or below 0,
    /// when `turn_length` or `founding_runs` is non-finite or below 1,
    /// when `savings_amv` or `founding_wealth` is non-finite or below 0, when
    /// `founding_goods_weight` or `founding_gap` is non-finite or below 1, or when
    /// `founding_useful_share` is outside 0 to 1.
    fn validate(&self, problems: &mut Vec<String>) {
        at_least(problems, "pop.craft_distance", self.craft_distance, 0.0);
        at_least(problems, "pop.turn_length", self.turn_length, 1.0);
        at_least(problems, "pop.savings_amv", self.savings_amv, 0.0);
        at_least(problems, "pop.founding_wealth", self.founding_wealth, 0.0);
        at_least(problems, "pop.founding_gap", self.founding_gap, 1.0);
        at_least(problems, "pop.founding_runs", self.founding_runs, 1.0);
        at_least(
            problems,
            "pop.founding_goods_weight",
            self.founding_goods_weight,
            1.0,
        );
        unit_share(
            problems,
            "pop.founding_useful_share",
            self.founding_useful_share,
        );
    }
}

/// Player-score yields. Empty until those pools are wired again.
/// The pools themselves live on [`crate::game::player_resources::PlayerResources`].
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PlayerResourceConfig {}

/// Market transport knobs. Price movement is not configured here.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct MarketConfig {
    /// Flat transport units charged per meeting. Must be `>= 0`.
    pub transaction_cost: f64,
    /// Multiplier on deal bulk for the wagon bill. Must be `>= 0`.
    pub friction: f64,
    /// Share of working currency surplus a holder lists. Must be from 0 to 1.
    pub currency_holder_share: f64,
    /// Share of working currency surplus a producer of that currency lists.
    ///
    /// Must be from 0 to 1. A producer lists this instead of
    /// [`Self::currency_holder_share`].
    pub currency_producer_share: f64,
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            transaction_cost: market_constants::TRANSACTION_COST,
            friction: market_constants::FRICTION,
            currency_holder_share: market_constants::CURRENCY_HOLDER_SHARE,
            currency_producer_share: market_constants::CURRENCY_PRODUCER_SHARE,
        }
    }
}

impl MarketConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        at_least(
            problems,
            "market.transaction_cost",
            self.transaction_cost,
            0.0,
        );
        at_least(problems, "market.friction", self.friction, 0.0);
        unit_share(
            problems,
            "market.currency_holder_share",
            self.currency_holder_share,
        );
        unit_share(
            problems,
            "market.currency_producer_share",
            self.currency_producer_share,
        );
    }
}

/// Deal acceptance. Empty until the next matcher exists.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct DealConfig {}

/// Who goes first in a market day. Empty until that order is redesigned.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct MarketPriorityConfig {}

/// Wage settlement. Empty. The share of Time a pop may sell lives on
/// species, culture, stratum, and religion, not here.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct LaborConfig {}

/// Firm planning. Empty until firms plan again.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct FirmConfig {}

fn finite(problems: &mut Vec<String>, name: &str, value: f64) -> bool {
    if value.is_finite() {
        true
    } else {
        problems.push(format!("{name} must be finite"));
        false
    }
}

fn unit_share(problems: &mut Vec<String>, name: &str, value: f64) {
    if !finite(problems, name, value) {
        return;
    }
    if !(0.0..=1.0).contains(&value) {
        problems.push(format!("{name} must be from 0 to 1, got {value}"));
    }
}

fn at_least(problems: &mut Vec<String>, name: &str, value: f64, min: f64) {
    if !finite(problems, name, value) {
        return;
    }
    if value < min {
        problems.push(format!("{name} must be >= {min}, got {value}"));
    }
}

#[cfg(test)]
mod config_should {
    use super::*;

    #[test]
    fn load_from_toml_keeps_defaults_for_missing_keys() {
        let cfg = GameConfig::load_from_toml("").expect("toml");
        assert_eq!(cfg, GameConfig::default());
    }

    #[test]
    fn load_from_toml_reads_craft_distance() {
        let cfg = GameConfig::load_from_toml("[pop]\ncraft_distance = 0.2\n").expect("toml");
        assert!((cfg.pop.craft_distance - 0.2).abs() < 1e-12);
        assert_eq!(cfg.market.friction, market_constants::FRICTION);
    }

    #[test]
    fn load_from_toml_rejects_negative_craft_distance() {
        let err = GameConfig::load_from_toml("[pop]\ncraft_distance = -0.1\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pop.craft_distance"), "{msg}");
    }

    #[test]
    fn load_from_toml_reads_market_transport() {
        let cfg = GameConfig::load_from_toml("[market]\nfriction = 2.0\n").expect("toml");
        assert_eq!(cfg.market.friction, 2.0);
        assert_eq!(
            cfg.market.transaction_cost,
            market_constants::TRANSACTION_COST
        );
    }

    #[test]
    fn load_from_toml_rejects_negative_friction() {
        let err = GameConfig::load_from_toml("[market]\nfriction = -1.0\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("market.friction"), "{msg}");
    }

    #[test]
    fn load_from_toml_reads_turn_length() {
        let cfg = GameConfig::load_from_toml("[pop]\nturn_length = 12\n").expect("toml");
        assert_eq!(cfg.pop.turn_length, 12.0);
        assert_eq!(cfg.pop.craft_distance, pop_constants::CRAFT_DISTANCE);
    }

    #[test]
    fn load_from_toml_reads_the_founding_stake() {
        let cfg = GameConfig::load_from_toml(
            "[pop]\nfounding_runs = 4\nfounding_goods_weight = 2\nfounding_useful_share = 0.5\n",
        )
        .expect("toml");
        assert_eq!(cfg.pop.founding_runs, 4.0);
        assert_eq!(cfg.pop.founding_goods_weight, 2.0);
        assert_eq!(cfg.pop.founding_useful_share, 0.5);
        assert_eq!(cfg.pop.founding_wealth, pop_constants::FOUNDING_WEALTH);
        assert_eq!(cfg.pop.founding_gap, pop_constants::FOUNDING_GAP);
    }

    #[test]
    fn load_from_toml_rejects_a_founding_gap_below_one() {
        let err = GameConfig::load_from_toml("[pop]\nfounding_gap = 0\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pop.founding_gap"), "{msg}");
    }

    #[test]
    fn load_from_toml_rejects_negative_founding_wealth() {
        let err = GameConfig::load_from_toml("[pop]\nfounding_wealth = -1\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pop.founding_wealth"), "{msg}");
    }

    #[test]
    fn load_from_toml_rejects_a_founding_weight_below_one() {
        let err = GameConfig::load_from_toml("[pop]\nfounding_goods_weight = 0.5\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pop.founding_goods_weight"), "{msg}");
    }

    #[test]
    fn load_from_toml_rejects_founding_runs_below_one() {
        let err = GameConfig::load_from_toml("[pop]\nfounding_runs = 0\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pop.founding_runs"), "{msg}");
    }

    #[test]
    fn load_from_toml_rejects_a_founding_share_above_one() {
        let err = GameConfig::load_from_toml("[pop]\nfounding_useful_share = 1.5\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pop.founding_useful_share"), "{msg}");
    }

    #[test]
    fn load_from_toml_rejects_a_turn_length_below_one() {
        let err = GameConfig::load_from_toml("[pop]\nturn_length = 0.5\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pop.turn_length"), "{msg}");
    }

    #[test]
    fn load_from_path_reads_world_config_file() {
        let path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world/config.toml");
        let cfg = GameConfig::load_from_path(&path).expect("world config");
        let mut expected = GameConfig::default();
        expected.pop.turn_length = 4.0;
        expected.pop.savings_amv = 3.0;
        assert_eq!(cfg, expected);
    }

    #[test]
    fn default_config_passes_validate() {
        GameConfig::default()
            .validate()
            .expect("defaults are in bounds");
    }
}
