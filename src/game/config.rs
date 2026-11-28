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
        self.firm.validate(&mut problems);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigLoadError::Invalid(problems))
        }
    }
}

/// Share of morning time that covers every basic desire.
pub const SUBSISTENCE_ANCHOR: f64 = 0.75;

/// Common desires are covered at this share of the basic cover.
pub const SUBSISTENCE_COMMON_SHARE: f64 = 0.5;

/// Divisor for a desire's implicit minimum and cap.
pub const DESIRE_SHARE_MINIMUM_DIVISOR: f64 = 2.0;

/// Days of basic desires every pop holds in the principal currency when the game starts.
pub const STARTING_COIN_DAYS: f64 = 10.0;

/// Days of offers a pop looks back over before it changes how much it offers.
pub const OFFER_WINDOW_DAYS: u32 = 3;

/// Share of what a pop offered over the window it must sell to keep offering all of it.
pub const OFFER_SOLD_SHARE: f64 = 0.5;

/// Factor a pop's offer share is multiplied by after a window that sold too little.
pub const OFFER_CUT: f64 = 0.5;

/// Lowest share of its unreserved stock a pop offers.
pub const OFFER_SCALE_MIN: f64 = 0.01;

/// A good on a market's tender list is bought, and every bill is paid, in thousandths.
pub const CURRENCY_STEP: f64 = 0.001;

/// Satisfaction buys round the unfilled gap up by this share.
pub const BUY_OVERSHOOT: f64 = 0.02;

/// Flat transport cover for one meeting with one seller.
///
/// One tenth of a time unit. Bulk is added on top, and the bill is not
/// below this fee.
pub const TRANSACTION_COST: f64 = 0.1;

/// Share of bulk charged on a transaction.
///
/// The good keeps its full bulk. The haul adds this share of it, times
/// the market's friction. An immobile good still adds none.
pub const BULK_HAUL_SHARE: f64 = 0.25;

/// Salability above this votes to expand a job line.
///
/// A share from 0 to 1. Below [`PLAN_SALABILITY_NAY`] votes to contract.
/// The band between them casts no vote.
pub const PLAN_SALABILITY_YAY: f64 = 0.8;

/// Salability below this votes to contract a job line.
pub const PLAN_SALABILITY_NAY: f64 = 0.4;

/// Posted offer at least this many times the day's sale.
///
/// Demand votes to contract when the offer is above this times the units
/// exchanged. An unfilled bid does not vote to expand past that. No
/// exchange with a posted offer is the same vote.
pub const PLAN_OFFER_DWARF: f64 = 4.0;

/// Share of a finished run that luck counts as a strong sale.
///
/// At or above this share, luck votes to expand when the stock is within
/// [`PLAN_STOCK_COVER`] of those sales.
pub const PLAN_SELL_YAY: f64 = 0.8;

/// Share of a finished run below which luck votes to contract.
///
/// The band from here up to [`PLAN_SELL_YAY`] casts no vote, unless the
/// stock is past [`PLAN_STOCK_COVER`].
pub const PLAN_SELL_NAY: f64 = 0.4;

/// Stock larger than this many times the day's sales.
///
/// Luck votes to contract when the pile is above this, including when the
/// sell rate is high. No sales uses the finished run in place of the
/// sales: a pile bigger than that run votes to contract.
pub const PLAN_STOCK_COVER: f64 = 4.0;

/// Share of the asked iterations that still counts as meeting the plan.
///
/// Luck votes to contract when a line was asked to run and finished under
/// this share. At this share the sales check stands.
pub const PLAN_RUN_NEAR: f64 = 0.8;

/// One point of living against the mean of the earlier samples.
///
/// A rise of this size is one expand vote. A drop of this size is one
/// contract vote.
pub const PLAN_SOL_VOTE: f64 = 1.0;

/// Most contract votes a living drop can cast.
///
/// Each full [`PLAN_SOL_VOTE`] point under the earlier mean is one vote.
/// A deeper drop stops here.
pub const PLAN_SOL_NAY_CAP: i32 = 2;

/// Share of the morning grant that one planning step moves.
///
/// A desire or haul hold keeps this much beyond yesterday's use.
/// A craft or labor line moves by this much of the grant.
pub const PLAN_STEP_SHARE: f64 = 0.05;

/// Minimum gain on one iteration's input cost that a firm plan expands toward.
///
/// The gain is this share of the required destroyed or consumed inputs,
/// whatever that spend is. The organization that will set this is not written.
pub const FIRM_MIN_MARGIN: f64 = 0.05;

/// Lowest target a firm plan writes.
///
/// A day with no marks steps down by one iteration and stops here.
pub const FIRM_MIN_TARGET: f64 = 1.0;

/// Share of the asked iterations a finished run may miss.
pub const FIRM_FINISH_GRACE: f64 = 0.10;

/// Share of output value a sale must reach when that bar is the higher one.
///
/// The selling mark takes the higher of the input cost and this share of
/// the output value.
pub const FIRM_SELL_SHARE: f64 = 0.80;

/// Share added to a firm's uncovered input need before that buy rounds up.
pub const FIRM_BUY_PAD: f64 = 0.01;

/// Days of the owner pop's basic desires that one firm pays its owners as a wage.
pub const OWNER_WAGE_BASIC_DAYS: f64 = 5.0;

/// Share of a firm's daily profit paid to its owners as a dividend.
pub const OWNER_PROFIT_SHARE: f64 = 0.10;

/// Share of a firm's held output stock it offers each day, on top of the last day's output.
pub const FIRM_STOCK_OFFER_SHARE: f64 = 0.10;

/// Share of what pops eat of a good that their subsistence must make for the good to count as covered.
pub const SUBSISTENCE_COVER_SHARE: f64 = 0.5;

/// Days in a row a covered good must be short before firms make it.
pub const SHORT_DAYS_TO_COMPETE: u32 = 5;

/// Days a firm's average sales look back over. Each day moves the average one part in this many.
pub const FIRM_DEMAND_WINDOW_DAYS: f64 = 3.0;

/// Share a firm plans above its average sales, so a line can grow into new demand.
pub const FIRM_DEMAND_HEADROOM: f64 = 0.10;

/// Iterations a firm keeps on one line when demand would idle every line.
pub const FIRM_MIN_LINE_TARGET: f64 = 1.0;

/// Share of a firm's sold value set aside to pay the owner wage.
pub const WAGE_RESERVE_SHARE: f64 = 0.20;

/// Share of the pop moved into anger and into fear for each full share of
/// basic needs left unmet.
pub const BASIC_UNMET_SENTIMENT: f64 = 0.04;

/// Share of the pop moved into contentment for each full share of common
/// needs met. A share above one still counts.
pub const COMMON_SENTIMENT: f64 = 0.02;

/// Share of the pop moved into hope and into happiness for each luxury
/// level met. Further levels still count.
pub const LUXURY_SENTIMENT: f64 = 0.01;

/// Share of the pop moved per point of wealth per household.
///
/// Positive wealth moves that share into hope. Negative wealth moves the
/// absolute share into fear.
pub const WEALTH_SENTIMENT: f64 = 0.01;

/// Market days kept for the living average.
pub const SOL_AVERAGE_DAYS: usize = 5;

/// Share of the pop moved per point of living above or below its recent average.
pub const SOL_CHANGE_SENTIMENT: f64 = 0.25;

/// Fraction of each other emotion moved into contentment.
///
/// A larger share moves more. A content pop moves nothing.
pub const CONTENTMENT_PULL: f64 = 0.15;

/// How far a no-trade price sits above the dearest method's unit cost.
pub const IDLE_COST_MARGIN: f64 = 0.10;

/// Smallest abstract market value a pass writes.
pub const MIN_PRICE: f64 = 0.001;

/// Turns in one year. One V1 turn is one day.
pub const TURNS_PER_YEAR: f64 = 365.0;

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
    /// stock rates by this. The default is [`TURNS_PER_YEAR`].
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
    /// Days of basic desires each pop holds in the principal currency on
    /// the first day. Must be `>= 0`.
    ///
    /// [`crate::playstate::PlayState::advance_turn`] tops each pop up to
    /// this. The default is [`STARTING_COIN_DAYS`].
    pub starting_coin_days: f64,
    /// Days of offers a pop looks back over before it changes how much it
    /// offers. Must be `>= 1`.
    ///
    /// [`crate::game::pop::Pop::review_offers`] reads this and the other
    /// offer keys. The default is [`OFFER_WINDOW_DAYS`].
    pub offer_window_days: u32,
    /// Share of what it offered over the window a pop must sell to keep
    /// offering all of it. Must be from 0 to 1.
    ///
    /// The default is [`OFFER_SOLD_SHARE`].
    pub offer_sold_share: f64,
    /// Factor the offer share is multiplied by after a window that sold too
    /// little. Must be above 0 and at most 1.
    ///
    /// The default is [`OFFER_CUT`].
    pub offer_cut: f64,
    /// Lowest offer share. Must be above 0 and at most 1.
    ///
    /// The default is [`OFFER_SCALE_MIN`].
    pub offer_scale_min: f64,
}

impl Default for PopConfig {
    /// # Default
    ///
    /// Childhood is 20 years, adulthood is 40, births are half female, and
    /// the partnership step closes 0.13 of the gap in one year. One turn is
    /// one day, so a year is [`TURNS_PER_YEAR`] turns. The
    /// subsistence anchor is [`SUBSISTENCE_ANCHOR`] and the common share is
    /// [`SUBSISTENCE_COMMON_SHARE`]. The desire share minimum divisor is
    /// [`DESIRE_SHARE_MINIMUM_DIVISOR`]. Pops start with
    /// [`STARTING_COIN_DAYS`] of basic desires in coin. The offer keys are
    /// [`OFFER_WINDOW_DAYS`], [`OFFER_SOLD_SHARE`], [`OFFER_CUT`], and
    /// [`OFFER_SCALE_MIN`].
    fn default() -> Self {
        Self {
            childhood_years: 20.0,
            adulthood_years: 40.0,
            child_sex_ratio: 0.5,
            partnership_trend: 0.13,
            turn_length: TURNS_PER_YEAR,
            subsistence_anchor: SUBSISTENCE_ANCHOR,
            subsistence_common_share: SUBSISTENCE_COMMON_SHARE,
            desire_share_minimum_divisor: DESIRE_SHARE_MINIMUM_DIVISOR,
            starting_coin_days: STARTING_COIN_DAYS,
            offer_window_days: OFFER_WINDOW_DAYS,
            offer_sold_share: OFFER_SOLD_SHARE,
            offer_cut: OFFER_CUT,
            offer_scale_min: OFFER_SCALE_MIN,
        }
    }
}

impl PopConfig {
    /// # Validate
    ///
    /// Appends a problem when an age length or the subsistence anchor is
    /// not finite or not above 0, when a share is outside 0 to 1, when
    /// `turn_length` is not finite or below 1, when the desire share
    /// minimum divisor is not 0 and not at least 1, when
    /// `starting_coin_days` is not finite or below 0, when
    /// `offer_window_days` is 0, or when `offer_cut` or `offer_scale_min`
    /// is not above 0 and at most 1.
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
        at_least(problems, "pop.starting_coin_days", self.starting_coin_days, 0.0);
        if self.offer_window_days == 0 {
            problems.push("pop.offer_window_days must be at least 1".to_string());
        }
        unit_share(problems, "pop.offer_sold_share", self.offer_sold_share);
        for (name, value) in [
            ("pop.offer_cut", self.offer_cut),
            ("pop.offer_scale_min", self.offer_scale_min),
        ] {
            above(problems, name, value, 0.0);
            unit_share(problems, name, value);
        }
    }
}

/// Market tunables.
///
/// `salability_default` is the salability of a good with no record.
/// `amv_epsilon` is the half-width of the stored-AMV dead zone.
/// `price_gain` and `price_cap` scale the daily surplus and shortage step.
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
}

impl Default for MarketConfig {
    /// # Default
    ///
    /// The AMV dead zone is `1e-9`. Salability starts at one. Price
    /// gain is 0.10 and the price cap is 0.20.
    fn default() -> Self {
        Self {
            amv_epsilon: 1e-9,
            salability_default: 1.0,
            price_gain: 0.10,
            price_cap: 0.20,
        }
    }
}

impl MarketConfig {
    /// # Validate
    ///
    /// Appends a problem when `amv_epsilon` is not finite or not above 0,
    /// when `salability_default` is outside 0 to 1, when `price_gain` is not
    /// above 0, or when `price_cap` is not above 0 and at most 1.
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

/// Firm tunables.
///
/// `owner_wage_basic_days` and `owner_profit_share` set what a firm pays
/// its owner pops each morning, read by
/// [`crate::game::market::Market::pay_owners`]. `stock_offer_share` sets
/// how much held stock [`crate::game::firm::Firm::post_sale_offers`] lists.
/// `subsistence_cover_share` and `short_days_to_compete` decide when
/// [`crate::game::firm::Firm::planning`] idles a line that would compete
/// with subsistence.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct FirmConfig {
    /// Owner wage, in days of the owner pop's basic desires. Must be `>= 0`.
    ///
    /// The default is [`OWNER_WAGE_BASIC_DAYS`].
    pub owner_wage_basic_days: f64,
    /// Share of the firm's profit paid as the dividend. Must be from 0 to 1.
    ///
    /// The default is [`OWNER_PROFIT_SHARE`].
    pub owner_profit_share: f64,
    /// Share of held output stock offered each day. Must be from 0 to 1.
    ///
    /// The default is [`FIRM_STOCK_OFFER_SHARE`].
    pub stock_offer_share: f64,
    /// Share of what pops eat of a good that subsistence must make for it
    /// to count as covered. Must be from 0 to 1.
    ///
    /// The default is [`SUBSISTENCE_COVER_SHARE`].
    pub subsistence_cover_share: f64,
    /// Days in a row a covered good must be short before firms make it.
    ///
    /// The default is [`SHORT_DAYS_TO_COMPETE`].
    pub short_days_to_compete: u32,
    /// Days a firm's average sales look back over. Must be `>= 1`.
    ///
    /// The default is [`FIRM_DEMAND_WINDOW_DAYS`].
    pub demand_window_days: f64,
    /// Share a firm plans above its average sales. Must be `>= 0`.
    ///
    /// The default is [`FIRM_DEMAND_HEADROOM`].
    pub demand_headroom: f64,
    /// Iterations a firm keeps on one line when demand would idle every
    /// line. Must be `>= 0`.
    ///
    /// The default is [`FIRM_MIN_LINE_TARGET`].
    pub min_line_target: f64,
    /// Share of a firm's sold value set aside to pay the owner wage. Must
    /// be from 0 to 1.
    ///
    /// The default is [`WAGE_RESERVE_SHARE`].
    pub wage_reserve_share: f64,
}

impl Default for FirmConfig {
    /// # Default
    ///
    /// [`OWNER_WAGE_BASIC_DAYS`] is 5, [`OWNER_PROFIT_SHARE`] is 0.10,
    /// [`FIRM_STOCK_OFFER_SHARE`] is 0.10, [`SUBSISTENCE_COVER_SHARE`] is
    /// 0.5, [`SHORT_DAYS_TO_COMPETE`] is 5, [`FIRM_DEMAND_WINDOW_DAYS`] is
    /// 3, [`FIRM_DEMAND_HEADROOM`] is 0.10, [`FIRM_MIN_LINE_TARGET`] is 1,
    /// and [`WAGE_RESERVE_SHARE`] is 0.20.
    fn default() -> Self {
        Self {
            owner_wage_basic_days: OWNER_WAGE_BASIC_DAYS,
            owner_profit_share: OWNER_PROFIT_SHARE,
            stock_offer_share: FIRM_STOCK_OFFER_SHARE,
            subsistence_cover_share: SUBSISTENCE_COVER_SHARE,
            short_days_to_compete: SHORT_DAYS_TO_COMPETE,
            demand_window_days: FIRM_DEMAND_WINDOW_DAYS,
            demand_headroom: FIRM_DEMAND_HEADROOM,
            min_line_target: FIRM_MIN_LINE_TARGET,
            wage_reserve_share: WAGE_RESERVE_SHARE,
        }
    }
}

impl FirmConfig {
    /// # Validate
    ///
    /// Appends a problem when `owner_wage_basic_days` is not finite or is
    /// below 0, when `owner_profit_share`, `stock_offer_share`,
    /// `subsistence_cover_share`, or `wage_reserve_share` is outside 0 to 1,
    /// when `demand_window_days` is not finite or below 1, or when
    /// `demand_headroom` or `min_line_target` is not finite or below 0.
    fn validate(&self, problems: &mut Vec<String>) {
        at_least(
            problems,
            "firm.owner_wage_basic_days",
            self.owner_wage_basic_days,
            0.0,
        );
        unit_share(problems, "firm.owner_profit_share", self.owner_profit_share);
        unit_share(problems, "firm.stock_offer_share", self.stock_offer_share);
        unit_share(
            problems,
            "firm.subsistence_cover_share",
            self.subsistence_cover_share,
        );
        unit_share(problems, "firm.wage_reserve_share", self.wage_reserve_share);
        at_least(problems, "firm.demand_window_days", self.demand_window_days, 1.0);
        at_least(problems, "firm.demand_headroom", self.demand_headroom, 0.0);
        at_least(problems, "firm.min_line_target", self.min_line_target, 0.0);
    }
}

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
    fn owner_wage_must_be_at_least_zero_and_profit_share_a_unit_share() {
        let mut cfg = GameConfig::default();
        cfg.firm.owner_wage_basic_days = -1.0;
        let msg = message(cfg);
        assert!(msg.contains("firm.owner_wage_basic_days"), "{msg}");
        assert!(msg.contains("must be >= 0"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.firm.owner_profit_share = 1.5;
        let msg = message(cfg);
        assert!(msg.contains("firm.owner_profit_share"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.firm.stock_offer_share = -0.1;
        let msg = message(cfg);
        assert!(msg.contains("firm.stock_offer_share"), "{msg}");

        for (name, change) in [
            ("firm.wage_reserve_share", (|cfg: &mut GameConfig| cfg.firm.wage_reserve_share = 1.5) as fn(&mut GameConfig)),
            ("firm.demand_window_days", |cfg| cfg.firm.demand_window_days = 0.5),
            ("firm.demand_headroom", |cfg| cfg.firm.demand_headroom = -0.1),
            ("firm.min_line_target", |cfg| cfg.firm.min_line_target = -1.0),
        ] {
            let mut cfg = GameConfig::default();
            change(&mut cfg);
            let msg = message(cfg);
            assert!(msg.contains(name), "{msg}");
        }

        let mut cfg = GameConfig::default();
        cfg.firm.subsistence_cover_share = 2.0;
        let msg = message(cfg);
        assert!(msg.contains("firm.subsistence_cover_share"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.firm.owner_wage_basic_days = 0.0;
        cfg.firm.owner_profit_share = 1.0;
        cfg.firm.stock_offer_share = 1.0;
        cfg.validate().expect("no wage, all profit, and all stock");
    }

    #[test]
    fn offer_keys_must_keep_a_window_and_a_share_above_zero() {
        let mut cfg = GameConfig::default();
        cfg.pop.offer_window_days = 0;
        let msg = message(cfg);
        assert!(msg.contains("pop.offer_window_days"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.offer_cut = 0.0;
        let msg = message(cfg);
        assert!(msg.contains("pop.offer_cut"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.offer_scale_min = 1.5;
        let msg = message(cfg);
        assert!(msg.contains("pop.offer_scale_min"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.offer_sold_share = -0.1;
        let msg = message(cfg);
        assert!(msg.contains("pop.offer_sold_share"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.offer_window_days = 1;
        cfg.pop.offer_cut = 1.0;
        cfg.pop.offer_scale_min = 1.0;
        cfg.validate().expect("a one-day window that never cuts");
    }

    #[test]
    fn starting_coin_days_must_be_at_least_zero() {
        let mut cfg = GameConfig::default();
        cfg.pop.starting_coin_days = -1.0;
        let msg = message(cfg);
        assert!(msg.contains("pop.starting_coin_days"), "{msg}");

        let mut cfg = GameConfig::default();
        cfg.pop.starting_coin_days = 0.0;
        cfg.validate().expect("no starting coin");
    }

    #[test]
    fn validate_reports_every_failure() {
        let mut cfg = GameConfig::default();
        cfg.pop.adulthood_years = 0.0;
        cfg.market.price_gain = 0.0;
        let msg = message(cfg);
        assert!(msg.contains("pop.adulthood_years"), "{msg}");
        assert!(msg.contains("market.price_gain"), "{msg}");
    }
}
