//! Gameplay tunables. Defaults live here. `data/world/config.toml` may override them.
//!
//! Missing keys keep [`Default`]. Call sites with [`crate::game::factuals::Factuals`]
//! should read `factuals.config`.
//!
//! Section structs stay so later pop, firm, and market work has a place to put knobs.

use std::fmt;
use std::path::Path;

use serde::Deserialize;

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
    /// # Load From Path
    ///
    /// Loads tunables from a TOML file. Missing keys keep [`Default`].
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigLoadError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(ConfigLoadError::Io)?;
        Self::load_from_toml(&text)
    }

    /// # Load From Toml
    ///
    /// Loads tunables from TOML text. Missing keys keep [`Default`].
    ///
    /// [`Self::validate`] checks the loaded values. A value outside its bound
    /// is [`ConfigLoadError::Invalid`].
    pub fn load_from_toml(text: &str) -> Result<Self, ConfigLoadError> {
        let cfg: Self = toml::from_str(text).map_err(ConfigLoadError::Toml)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// # Validate
    ///
    /// Checks loaded tunables against the bounds on each field.
    ///
    /// Returns every failure together. An empty list is `Ok`.
    pub fn validate(&self) -> Result<(), ConfigLoadError> {
        let mut problems = Vec::new();
        self.pop.validate(&mut problems);
        self.market.validate(&mut problems);
        self.labor.validate(&mut problems);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigLoadError::Invalid(problems))
        }
    }
}

/// Subsistence rate that covers every basic desire.
pub const SUBSISTENCE_ANCHOR: f64 = 0.6;

/// Common desires are covered at this share of the basic cover.
pub const SUBSISTENCE_COMMON_SHARE: f64 = 0.5;

/// Divisor for a desire's implicit minimum and cap.
pub const DESIRE_SHARE_MINIMUM_DIVISOR: f64 = 2.0;

/// Pop-day tunables.
///
/// [`crate::game::household::Household::update`] reads the age lengths, the
/// birth sex ratio, the partnership step, and [`Self::turn_length`].
/// [`crate::game::pop::Pop::run_subsistence`] reads the subsistence anchor
/// and the common share. [`crate::game::pop::Pop::consume`] reads the
/// desire share minimum divisor.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct PopConfig {
    /// Years in childhood before aging into adulthood. Must be `> 0`.
    pub childhood_years: f64,
    /// Years in adulthood before aging into elderhood. Must be `> 0`.
    pub adulthood_years: f64,
    /// Female fraction of live births. `0` is all male and `1` is all female.
    ///
    /// Must be from 0 to 1.
    pub child_sex_ratio: f64,
    /// Share of the partnership gap closed in one year. Must be from 0 to 1.
    pub partnership_trend: f64,
    /// Turns in one year. Must be `>= 1`.
    ///
    /// [`crate::game::household::Household::update`] divides the annual
    /// stock rates by this. The default is one turn per year.
    pub turn_length: f64,
    /// Subsistence rate that covers basic desires. Must be `> 0`.
    ///
    /// The default is [`SUBSISTENCE_ANCHOR`].
    pub subsistence_anchor: f64,
    /// Share of the basic cover that common desires receive. Must be from 0 to 1.
    ///
    /// The default is [`SUBSISTENCE_COMMON_SHARE`].
    pub subsistence_common_share: f64,
    /// Divisor for a desire's implicit minimum and cap. Must be `0` or at least `1`.
    ///
    /// [`crate::game::desire::Desire::implicit_minimum`] reads this value.
    /// The default is [`DESIRE_SHARE_MINIMUM_DIVISOR`].
    pub desire_share_minimum_divisor: f64,
}

impl Default for PopConfig {
    /// # Default
    ///
    /// Childhood is 20 years, adulthood is 40, births are half female, and
    /// the partnership step closes 0.13 of the gap in one year. One turn is
    /// one year. The
    /// subsistence anchor is [`SUBSISTENCE_ANCHOR`] and the common share is
    /// [`SUBSISTENCE_COMMON_SHARE`]. The desire share minimum divisor is
    /// [`DESIRE_SHARE_MINIMUM_DIVISOR`].
    fn default() -> Self {
        Self {
            childhood_years: 20.0,
            adulthood_years: 40.0,
            child_sex_ratio: 0.5,
            partnership_trend: 0.13,
            turn_length: 1.0,
            subsistence_anchor: SUBSISTENCE_ANCHOR,
            subsistence_common_share: SUBSISTENCE_COMMON_SHARE,
            desire_share_minimum_divisor: DESIRE_SHARE_MINIMUM_DIVISOR,
        }
    }
}

impl PopConfig {
    /// # Validate
    ///
    /// Appends a problem when an age length or the subsistence anchor is
    /// not finite or not above 0, when a share is outside 0 to 1, when
    /// `turn_length` is not finite or below 1, or when the desire share
    /// minimum divisor is not 0 and not at least 1.
    fn validate(&self, problems: &mut Vec<String>) {
        above(problems, "pop.childhood_years", self.childhood_years, 0.0);
        above(problems, "pop.adulthood_years", self.adulthood_years, 0.0);
        unit_share(problems, "pop.child_sex_ratio", self.child_sex_ratio);
        unit_share(problems, "pop.partnership_trend", self.partnership_trend);
        at_least(problems, "pop.turn_length", self.turn_length, 1.0);
        above(
            problems,
            "pop.subsistence_anchor",
            self.subsistence_anchor,
            0.0,
        );
        unit_share(
            problems,
            "pop.subsistence_common_share",
            self.subsistence_common_share,
        );
        zero_or_at_least(
            problems,
            "pop.desire_share_minimum_divisor",
            self.desire_share_minimum_divisor,
            1.0,
        );
    }
}

/// Player-score yields. Empty until those pools are wired again.
/// The pools themselves live on [`crate::game::player_resources::PlayerResources`].
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PlayerResourceConfig {}

/// Market tunables.
///
/// `salability_default` is the salability of a good with no record.
/// `amv_epsilon` is the half-width of the stored-AMV dead zone.
/// `price_gain` and `price_cap` scale the daily surplus and shortage step.
/// The migration and founding knobs are loaded here. Those passes are not
/// written yet.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct MarketConfig {
    /// Half-width of the AMV dead zone around zero. Must be `> 0`.
    ///
    /// A stored AMV inside this band is not kept. It lands on this value
    /// with the opposite sign from the previous AMV. This is not a price floor.
    pub amv_epsilon: f64,
    /// Salability of a good with no recorded value yet. Must be from 0 to 1.
    pub salability_default: f64,
    /// Fractional gain on a surplus or shortage. Must be `> 0`.
    ///
    /// The design band is 0.05 to 0.15. The default is 0.10.
    pub price_gain: f64,
    /// Largest fractional step from surplus or shortage. Must be `> 0` and `<= 1`.
    ///
    /// The design example is 0.20. The day's ratio is already within 1.
    pub price_cap: f64,
    /// Weight of anger beside fear on the way out. Must be `>= 0`.
    pub migration_anger_weight: f64,
    /// Share of a saturated push or pull that moves in one year.
    ///
    /// Must be from 0 to 1. The saturated fraction stays under this share.
    pub migration_pace: f64,
    /// Households one founding moves. Must be `> 0`.
    pub founding_households: f64,
    /// Households a pop needs before a founding may leave. Must be `> 0`.
    pub founding_min_households: f64,
    /// Chance added each open day that a pop founds when nobody clears the gate.
    ///
    /// Must be from 0 to 1. The first open day is one step. Each later open
    /// day adds another. The chance stops at 1.
    pub founding_drought_step: f64,
    /// Share of foundings that apply both an add and a drop. Must be from 0 to 1.
    pub founding_both_share: f64,
    /// Extra weight on a drop whose product already covered the request.
    ///
    /// Must be `>= 0`. `1` leaves the base weight.
    pub founding_available_drop: f64,
}

impl Default for MarketConfig {
    /// # Default
    ///
    /// The AMV dead zone is `1e-9`. Salability starts at one half. Price
    /// gain is 0.10 and the price cap is 0.20. Anger weighs half of fear.
    /// One tenth of a saturated push moves in a year.
    /// A founding moves 5 households out of a pop of at least 25. An open
    /// day adds one hundredth to the drought chance. One fifth of foundings
    /// both add and drop. A covered drop weighs half again its base.
    fn default() -> Self {
        Self {
            amv_epsilon: 1e-9,
            salability_default: 0.5,
            price_gain: 0.10,
            price_cap: 0.20,
            migration_anger_weight: 0.5,
            migration_pace: 0.1,
            founding_households: 5.0,
            founding_min_households: 25.0,
            founding_drought_step: 0.01,
            founding_both_share: 0.2,
            founding_available_drop: 1.5,
        }
    }
}

impl MarketConfig {
    /// # Validate
    ///
    /// Appends a problem when `amv_epsilon` or a household count is not
    /// finite or not above 0, when a weight is not finite or below 0, when
    /// a share is outside 0 to 1, when `price_gain` is not above 0, or when
    /// `price_cap` is not above 0 and at most 1.
    fn validate(&self, problems: &mut Vec<String>) {
        above(problems, "market.amv_epsilon", self.amv_epsilon, 0.0);
        unit_share(
            problems,
            "market.salability_default",
            self.salability_default,
        );
        above(problems, "market.price_gain", self.price_gain, 0.0);
        above(problems, "market.price_cap", self.price_cap, 0.0);
        if self.price_cap.is_finite() && self.price_cap > 1.0 {
            problems.push(format!(
                "market.price_cap must be <= 1, got {}",
                self.price_cap
            ));
        }
        at_least(
            problems,
            "market.migration_anger_weight",
            self.migration_anger_weight,
            0.0,
        );
        unit_share(problems, "market.migration_pace", self.migration_pace);
        above(
            problems,
            "market.founding_households",
            self.founding_households,
            0.0,
        );
        above(
            problems,
            "market.founding_min_households",
            self.founding_min_households,
            0.0,
        );
        unit_share(
            problems,
            "market.founding_drought_step",
            self.founding_drought_step,
        );
        unit_share(
            problems,
            "market.founding_both_share",
            self.founding_both_share,
        );
        at_least(
            problems,
            "market.founding_available_drop",
            self.founding_available_drop,
            0.0,
        );
    }
}

/// # Finite
///
/// Whether `value` is finite.
///
/// Appends a problem for `name` when it is not. Returns that check.
fn finite(problems: &mut Vec<String>, name: &str, value: f64) -> bool {
    if value.is_finite() {
        true
    } else {
        problems.push(format!("{name} must be finite"));
        false
    }
}

/// # Unit Share
///
/// Appends a problem when `value` is not a finite share from 0 to 1.
///
/// `name` is the key in the message. Nothing is returned.
fn unit_share(problems: &mut Vec<String>, name: &str, value: f64) {
    if !finite(problems, name, value) {
        return;
    }
    if !(0.0..=1.0).contains(&value) {
        problems.push(format!("{name} must be from 0 to 1, got {value}"));
    }
}

/// # At Least
///
/// Appends a problem when `value` is not finite or is below `min`.
///
/// `name` is the key in the message. Nothing is returned.
fn at_least(problems: &mut Vec<String>, name: &str, value: f64, min: f64) {
    if !finite(problems, name, value) {
        return;
    }
    if value < min {
        problems.push(format!("{name} must be >= {min}, got {value}"));
    }
}

/// # Zero Or At Least
///
/// Appends a problem when `value` is not finite, or when it is neither 0 nor at least `min`.
///
/// `name` is the key in the message. Nothing is returned.
fn zero_or_at_least(problems: &mut Vec<String>, name: &str, value: f64, min: f64) {
    if !finite(problems, name, value) {
        return;
    }
    if value != 0.0 && value < min {
        problems.push(format!("{name} must be 0 or >= {min}, got {value}"));
    }
}

/// # Above
///
/// Appends a problem when `value` is not finite or is not above `min`.
///
/// `name` is the key in the message. Nothing is returned.
fn above(problems: &mut Vec<String>, name: &str, value: f64, min: f64) {
    if !finite(problems, name, value) {
        return;
    }
    if value <= min {
        problems.push(format!("{name} must be > {min}, got {value}"));
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

/// Time created for one unit of household labor at day start.
pub const TIME_PER_LABOR: f64 = 16.0;

/// Labor tunables.
///
/// `time_per_labor` is the morning time grant. The share of time a pop may
/// sell lives on species, culture, stratum, and religion, not here.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct LaborConfig {
    /// Time created per unit of household labor at day start. Must be `> 0`.
    pub time_per_labor: f64,
}

impl Default for LaborConfig {
    /// # Default
    ///
    /// [`TIME_PER_LABOR`] is 16.
    fn default() -> Self {
        Self {
            time_per_labor: TIME_PER_LABOR,
        }
    }
}

impl LaborConfig {
    /// # Validate
    ///
    /// Appends a problem when `time_per_labor` is not finite or not above 0.
    fn validate(&self, problems: &mut Vec<String>) {
        above(problems, "labor.time_per_labor", self.time_per_labor, 0.0);
    }
}

/// Firm planning. Empty until firms plan again.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct FirmConfig {}

#[cfg(test)]
mod config_should {
    use super::*;

    fn message(cfg: GameConfig) -> String {
        cfg.validate().unwrap_err().to_string()
    }

    #[test]
    fn default_config_passes() {
        GameConfig::default().validate().expect("default");
    }

    #[test]
    fn above_rejects_zero_and_a_non_finite() {
        let mut cfg = GameConfig::default();
        cfg.pop.childhood_years = 0.0;
        let msg = message(cfg);
        assert!(msg.contains("pop.childhood_years"), "{msg}");
        assert!(msg.contains("must be > 0"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.labor.time_per_labor = f64::NAN;
        let msg = message(cfg);
        assert!(msg.contains("labor.time_per_labor"), "{msg}");
        assert!(msg.contains("must be finite"), "{msg}");
        assert!(!msg.contains("must be > 0"), "{msg}");
    }

    #[test]
    fn unit_share_rejects_outside_zero_to_one_and_accepts_the_ends() {
        let mut cfg = GameConfig::default();
        cfg.pop.child_sex_ratio = -0.1;
        let msg = message(cfg);
        assert!(msg.contains("pop.child_sex_ratio"), "{msg}");
        assert!(msg.contains("must be from 0 to 1"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.market.salability_default = 1.1;
        let msg = message(cfg);
        assert!(msg.contains("market.salability_default"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.child_sex_ratio = 0.0;
        cfg.pop.partnership_trend = 1.0;
        cfg.validate().expect("share ends");
    }

    #[test]
    fn desire_share_minimum_divisor_is_zero_or_at_least_one() {
        let mut cfg = GameConfig::default();
        cfg.pop.desire_share_minimum_divisor = 0.5;
        let msg = message(cfg);
        assert!(msg.contains("pop.desire_share_minimum_divisor"), "{msg}");
        assert!(msg.contains("must be 0 or >= 1"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.desire_share_minimum_divisor = -1.0;
        let msg = message(cfg);
        assert!(msg.contains("pop.desire_share_minimum_divisor"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.desire_share_minimum_divisor = f64::NAN;
        let msg = message(cfg);
        assert!(msg.contains("must be finite"), "{msg}");
        assert!(!msg.contains("must be 0 or >= 1"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.desire_share_minimum_divisor = 0.0;
        cfg.validate().expect("off");

        let mut cfg = GameConfig::default();
        cfg.pop.desire_share_minimum_divisor = 1.0;
        cfg.validate().expect("one");
    }

    #[test]
    fn turn_length_must_be_at_least_one() {
        let mut cfg = GameConfig::default();
        cfg.pop.turn_length = 0.5;
        let msg = message(cfg);
        assert!(msg.contains("pop.turn_length"), "{msg}");
        assert!(msg.contains("must be >= 1"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.turn_length = 1.0;
        cfg.validate().expect("one turn a year");
    }

    #[test]
    fn at_least_rejects_below_zero_and_accepts_zero() {
        let mut cfg = GameConfig::default();
        cfg.market.migration_anger_weight = -1.0;
        let msg = message(cfg);
        assert!(msg.contains("market.migration_anger_weight"), "{msg}");
        assert!(msg.contains("must be >= 0"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.market.founding_available_drop = 0.0;
        cfg.validate().expect("zero weight");
    }

    #[test]
    fn price_gain_must_be_above_zero_and_price_cap_at_most_one() {
        let mut cfg = GameConfig::default();
        cfg.market.price_gain = 0.0;
        let msg = message(cfg);
        assert!(msg.contains("market.price_gain"), "{msg}");
        assert!(msg.contains("must be > 0"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.market.price_cap = 1.1;
        let msg = message(cfg);
        assert!(msg.contains("market.price_cap"), "{msg}");
        assert!(msg.contains("must be <= 1"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.market.price_cap = 0.0;
        let msg = message(cfg);
        assert!(msg.contains("must be > 0"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.market.price_gain = 0.05;
        cfg.market.price_cap = 1.0;
        cfg.validate().expect("band ends");
    }

    #[test]
    fn validate_reports_every_failure() {
        let mut cfg = GameConfig::default();
        cfg.pop.adulthood_years = 0.0;
        cfg.market.migration_pace = 2.0;
        let msg = message(cfg);
        assert!(msg.contains("pop.adulthood_years"), "{msg}");
        assert!(msg.contains("market.migration_pace"), "{msg}");
    }
}
