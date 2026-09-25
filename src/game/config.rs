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

/// Transport bill pieces. Not prices.
pub mod market_constants {
    /// Flat transport units charged per meeting. Not AMV.
    pub const TRANSACTION_COST: f64 = 1.0;
    /// Multiplier on deal bulk. `transport = TRANSACTION_COST + bulk * FRICTION`.
    pub const FRICTION: f64 = 1.0;
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
        self.market.validate(&mut problems);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigLoadError::Invalid(problems))
        }
    }
}

/// Pop-day tunables. Empty until the rebuilt pop needs them.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PopConfig {}

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
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            transaction_cost: market_constants::TRANSACTION_COST,
            friction: market_constants::FRICTION,
        }
    }
}

impl MarketConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        at_least(problems, "market.transaction_cost", self.transaction_cost, 0.0);
        at_least(problems, "market.friction", self.friction, 0.0);
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
/// species, culture, and religion, not here.
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
    fn load_from_toml_reads_market_transport() {
        let cfg = GameConfig::load_from_toml("[market]\nfriction = 2.0\n").expect("toml");
        assert_eq!(cfg.market.friction, 2.0);
        assert_eq!(cfg.market.transaction_cost, market_constants::TRANSACTION_COST);
    }

    #[test]
    fn load_from_toml_rejects_negative_friction() {
        let err = GameConfig::load_from_toml("[market]\nfriction = -1.0\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("market.friction"), "{msg}");
    }

    #[test]
    fn load_from_path_reads_world_config_file() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world/config.toml");
        let cfg = GameConfig::load_from_path(&path).expect("world config");
        assert_eq!(cfg, GameConfig::default());
    }

    #[test]
    fn default_config_passes_validate() {
        GameConfig::default().validate().expect("defaults are in bounds");
    }
}
