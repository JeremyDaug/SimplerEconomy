//! Gameplay tunables — defaults here, overrides from world-data `config.toml`.
//!
//! Compile-time buffer sizes ([`pop_constants::HISTORY_MAX`],
//! [`market_constants::AMV_HISTORY_MAX`]) stay as `const` because they size
//! ring buffers. Everything else can be loaded from `data/world/config.toml`
//! into [`GameConfig`]. Missing keys keep these defaults. Load rejects values
//! outside the bounds written on each field. Call sites with
//! [`crate::game::factuals::Factuals`] should read `factuals.config`.

/// Living-standard score history, trend, and related mood rates.
pub mod pop_constants {
    // History Length
    /// Compile-time max ring slots for PopRecords histories (SOL, pop size, liquid wealth).
    pub const HISTORY_MAX: usize = 16;

    /// Daily Time grant per unit of household labor (`ScalingFactor::Labor`).
    /// Adult labor 1.0, elder 0.7, child 0.3 (household defaults).
    pub const TIME_PER_LABOR: f64 = 48.0;

    /// Default days of basic+common consume-need to hold as a buffer (1.0 = 1 day).
    pub const DEFAULT_SAVINGS_RATIO: f64 = 0.20;
    /// Default required return (personal interest rate). Higher = more impatient.
    pub const DEFAULT_TIME_PREFERENCE: f64 = 0.05;
    /// Neutral risk appetite (fear/greed). Range intended: -1.0 (fear) ..= 1.0 (greed).
    pub const DEFAULT_RISK_APPETITE: f64 = 0.0;

    /// Daily lerp toward planning-variable targets (1.0 = snap).
    pub const PLANNING_LERP_RATE: f64 = 0.15;

    pub const RISK_APPETITE_MIN: f64 = -1.0;
    pub const RISK_APPETITE_MAX: f64 = 1.0;
    /// Hope raises risk appetite (more than happiness).
    pub const RISK_HOPE_WEIGHT: f64 = 1.0;
    /// Happiness raises risk appetite less than hope.
    pub const RISK_HAPPINESS_WEIGHT: f64 = 0.40;
    /// Fear lowers risk appetite (more than anger).
    pub const RISK_FEAR_WEIGHT: f64 = 1.0;
    /// Anger lowers risk appetite less than fear.
    pub const RISK_ANGER_WEIGHT: f64 = 0.45;
    /// How hard SOL trend pulls risk appetite (falling SOL -> more caution).
    pub const RISK_TREND_WEIGHT: f64 = 0.25;
    /// Contentment lowers risk appetite (keep what we have).
    pub const RISK_CONTENTMENT_WEIGHT: f64 = 0.50;

    /// Days-of-buffer clamp. 5.0 = five extra days of basic+common need.
    pub const SAVINGS_RATIO_MIN: f64 = 0.0;
    pub const SAVINGS_RATIO_MAX: f64 = 5.0;
    /// Greed (positive risk) lowers days of buffer; fear-side risk raises them.
    pub const SAVINGS_RISK_WEIGHT: f64 = 0.10;
    /// Extra days of buffer from the Fear sentiment axis.
    pub const SAVINGS_FEAR_WEIGHT: f64 = 0.05;
    /// Extra days of buffer from unmet basic tier sat (0 at full basic, 1 at none).
    pub const SAVINGS_UNMET_BASIC_WEIGHT: f64 = 0.10;
    /// Extra days of buffer from a falling living-standard trend.
    pub const SAVINGS_FALL_SOL_WEIGHT: f64 = 0.15;
    /// How much household growth inflates the savings pile (1.0 = full growth_f).
    pub const SAVINGS_GROWTH_BUFFER_WEIGHT: f64 = 1.0;
    /// At fear 0, this share of the buffer may be highly salable AMV instead of
    /// the specific goods in the basic+common basket. 1.0 = fully substitutable.
    pub const SAVINGS_SUBSTITUTABILITY_CALM: f64 = 1.0;
    /// At fear 1, this share may still be liquid AMV. 0.0 = insist on the goods.
    pub const SAVINGS_SUBSTITUTABILITY_FEAR: f64 = 0.0;

    pub const TIME_PREFERENCE_MIN: f64 = 0.0;
    pub const TIME_PREFERENCE_MAX: f64 = 1.0;
    pub const TIME_PREFERENCE_ANGER_WEIGHT: f64 = 0.03;
    pub const TIME_PREFERENCE_UNMET_BASIC_WEIGHT: f64 = 0.04;
    /// Contentment lowers time preference (more patient).
    pub const TIME_PREFERENCE_CONTENTMENT_WEIGHT: f64 = 0.02;

    // Living Standard Constants
    /// EMA blend for rolling average (higher = more weight on today).
    pub const ROLLING_AVG_WEIGHT: f64 = 0.25;
    /// Weight of basic tier sat in the composite living-standard score.
    pub const SCORE_WEIGHT_BASIC: f64 = 3.0;
    /// Weight of common-mood tier sat in the composite living-standard score.
    pub const SCORE_WEIGHT_COMMON: f64 = 1.5;
    /// Weight of luxury tier sat in the composite living-standard score.
    pub const SCORE_WEIGHT_LUXURY: f64 = 1.0;

    /// Rate at which anger sentiment is gained from low living standard.
    pub const ANGER_SENTIMENT_RATE: f64 = 0.08;
    // Rate at which fear sentiment is gained from low living standard.
    pub const FEAR_SENTIMENT_RATE: f64 = 0.04;
    // Rate at which Contentment is gained from moderate living standards.
    pub const CONTENTMENT_SENTIMENT_RATE: f64 = 0.05;
    /// Rate at which Happiness is gained from moderate Living Standards.
    pub const HAPPINESS_SENTIMENT_RATE: f64 = 0.03;
    /// Rate at which Hope is gained from high Living Standards.
    pub const HOPE_SENTIMENT_RATE: f64 = 0.02;

    /// Rate at which Contentment is gained from rising living standards.
    pub const TREND_CONTENTMENT_SENTIMENT_RATE: f64 = 0.02;
    /// Rate at which Happiness is gained from rising living standards.
    pub const TREND_HAPPINESS_SENTIMENT_RATE: f64 = 0.03;
    /// Rate at which Hope is gained from rising living standards.
    pub const TREND_HOPE_SENTIMENT_RATE: f64 = 0.05;
    /// Rate at which Anger is gained from falling living standards.
    pub const TREND_ANGER_SENTIMENT_RATE: f64 = 0.04;
    /// Rate at which Fear is gained from falling living standards.
    pub const TREND_FEAR_SENTIMENT_RATE: f64 = 0.03;

    /// Ignore |trend| below this when applying sentiment shifts.
    pub const SENTIMENT_TREND_DEADBAND: f64 = 0.5;
    /// Sentiment share gain scale when living standard is rising.
    pub const SENTIMENT_RISE_GAIN: f64 = 0.03;
    /// Sentiment share gain scale when living standard is falling (usually > rise).
    pub const SENTIMENT_FALL_GAIN: f64 = 0.05;
}

/// Daily player-resource yields from pops (culture, research, legitimacy, …).
/// Values are placeholders; retune after extract is in play.
pub mod player_resource_constants {
    /// Culture per 1.0 common **tier sat** satisfied, per pop.
    pub const COMMON_CULTURE_RATE: f64 = 1.0;
    /// Weaker, unclamped luxury ladder (same pop scale).
    pub const LUXURY_CULTURE_RATE: f64 = 0.35;

    /// Legitimacy from the first desire (scaled by average desire sat).
    /// Further desires add [`EXTRA_DESIRE_LEGITIMACY`] each, so extra wants are
    /// a weak legitimacy source.
    pub const FIRST_DESIRE_LEGITIMACY: f64 = 0.5;
    /// Legitimacy added per desire after the first.
    pub const EXTRA_DESIRE_LEGITIMACY: f64 = 0.1;
    /// Luxury legitimacy per 1.0 luxury tier-sat mass (unclamped).
    pub const LUXURY_LEGITIMACY_RATE: f64 = 0.40;
    /// Legitimacy Potential Modifier for Moods.
    pub const MOOD_POTENTIAL_MODIFIER: f64 = 0.75;
    /// Legitimacy Potential Modifier for Trends.
    pub const TREND_POTENTIAL_MODIFIER: f64 = 0.5;

    /// Mood shares (0-1) added into the legitimacy signed term, then * potential.
    /// Anger hurts more than fear; happiness/hope help, a bit weaker.
    pub const ANGER_LEGITIMACY_RATE: f64 = 0.40;
    pub const FEAR_LEGITIMACY_RATE: f64 = 0.22;
    pub const HAPPINESS_LEGITIMACY_RATE: f64 = 0.18;
    pub const HOPE_LEGITIMACY_RATE: f64 = 0.12;
    /// Rising SOL trend coefficient (people praise the rise).
    pub const TREND_LEGITIMACY_RISE: f64 = 0.03;
    /// Falling SOL trend coefficient (people hate the fall more than they praise a rise).
    pub const TREND_LEGITIMACY_FALL: f64 = 0.05;
}

/// Market valuation and trade tunables.
pub mod market_constants {
    /// Smallest allowed |AMV| and |average_price|.
    ///
    /// Zero is never stored. A setter that would land inside `(-AMV_MIN_ABS,
    /// AMV_MIN_ABS)` bounces that far past 0 from the previous sign
    /// (positive -> slightly negative, negative -> slightly positive).
    pub const AMV_MIN_ABS: f64 = 0.00001;

    /// Default salability for a new or unrecorded good.
    /// Below [`EXCHANGE_SALABILITY_MIN`], so unknown goods are not till money.
    pub const SALABILITY_DEFAULT: f64 = 0.4;
    /// Minimum salability to treat on-hand stock as exchange tender.
    pub const EXCHANGE_SALABILITY_MIN: f64 = 0.6;
    /// When a pile is both sold and exchanged, each side keeps at least this
    /// share (0.1 = 10%). Salability lerps the rest.
    pub const SELL_EXCHANGE_EDGE: f64 = 0.1;

    /// Failed-deal retries a buy/request may take. `tries` starts at 0.
    /// After this many renewals a further failure closes the order
    /// (the third close-out).
    pub const BUY_TRY_LIMIT: u32 = 2;

    /// Flat transport units charged per meeting (success or wash).
    /// 1 for the current town scale. Later: scale with size. Not AMV.
    pub const TRANSACTION_COST: f64 = 1.0;

    /// How hard a successful exchange pulls both sides' AMV toward the
    /// midpoint of the basket (0 = no move, 1 = snap).
    pub const AMV_ACCEPT_BLEND: f64 = 0.25;
    /// How hard a rejected meeting pulls AMV (sought up, tenders down).
    pub const AMV_REJECT_BLEND: f64 = 0.10;
    /// Sought-good up-push is this times the tender down-push (demand edge).
    pub const AMV_REJECT_DEMAND_EDGE: f64 = 1.1;
    /// Day-end leftover-book AMV added per leftover-to-volume multiple.
    /// Factor is `1 + blend * miss / purchased` (empty fill = 1 unit).
    pub const AMV_LEFTOVER_BLEND: f64 = 0.10;
    /// Skip leftover AMV when both books have leftover and
    /// `|buy - sell| / (buy + sell)` is below this (0.10 = 10%).
    pub const AMV_LEFTOVER_BAND: f64 = 0.10;
    /// Day-end lerp of salability toward payment/tender (0 = no move, 1 = snap).
    pub const SALABILITY_BLEND: f64 = 0.25;

    /// Compile-time max ring slots for [`crate::game::market::MarketGood`] AMV history.
    pub const AMV_HISTORY_MAX: usize = 16;
}

/// Deal-making AMV acceptance floors and tender cutoffs.
///
/// Values are **keep ratios** (`received AMV / given AMV`). A pop "75% max
/// loss" is keep `0.25`. Buyers still accept windfalls (`keep >= 1.0`).
pub mod deal_constants {
    /// Pop minimum AMV keep. `0.25` = accept up to 75% AMV loss.
    pub const POP_AMV_MIN_KEEP: f64 = 0.25;
    /// Firm minimum AMV keep. `0.50` = accept up to 50% AMV loss.
    pub const FIRM_AMV_MIN_KEEP: f64 = 0.50;
    /// When a firm deal cannot land in [`FIRM_AMV_MIN_KEEP`] but the firm
    /// needs the received goods (purchase or use target), fall back to this
    /// keep ratio (same as pop).
    pub const FIRM_AMV_NEED_KEEP: f64 = POP_AMV_MIN_KEEP;
    /// Salability at or above this is highly salable (money-like). Buy
    /// proposals fill from these (plus the seller's named counter) before
    /// offering lower-salability goods.
    pub const HIGH_SALABILITY: f64 = 0.8;
}

/// Named intramarket order-priority slots.
///
/// Lower values go first. Bands are half-open `[start, end)`. Equal values are
/// later broken at random by the matcher. See
/// `docs/proposals/market-order-priority.md`.
pub mod market_priority {
    /// Institution slot before all firms.
    pub const INSTITUTION_BEFORE_FIRMS: f64 = 1.0;
    /// Institution slot after both firm bands and before pops.
    pub const INSTITUTION_BETWEEN_FIRMS_AND_POPS: f64 = 3.0;
    /// Institution slot after the pop band.
    pub const INSTITUTION_AFTER_POPS: f64 = 5.0;

    /// Merchant / trader firm band start (inclusive).
    pub const FIRM_MERCHANT_START: f64 = 2.0;
    /// Merchant / trader firm band end (exclusive).
    pub const FIRM_MERCHANT_END: f64 = 2.5;
    /// Producer firm band start (inclusive).
    pub const FIRM_PRODUCER_START: f64 = 2.5;
    /// Producer firm band end (exclusive).
    pub const FIRM_PRODUCER_END: f64 = 3.0;

    /// Default merchant priority when the firm is not wealth-ranked.
    pub const FIRM_MERCHANT: f64 = FIRM_MERCHANT_START;
    /// Default producer priority when the firm is not wealth-ranked.
    pub const FIRM_PRODUCER: f64 = FIRM_PRODUCER_START;

    /// How far before a firm-band exclusive end the matching state slot sits.
    /// Ranked firms lerp toward this value and never reach it.
    pub const STATE_FIRM_SLOT_MARGIN: f64 = 0.01;

    /// Pop band start (inclusive). Unranked pop orders sit here until the
    /// market sets a wealth rank.
    pub const POP_START: f64 = 4.0;
    /// Pop band end (exclusive).
    pub const POP_END: f64 = 5.0;

    /// State / player: before everyone.
    pub const STATE_FIRST: f64 = 0.0;
    /// State / player: after institution-before-firms, before merchants.
    pub const STATE_BEFORE_FIRMS: f64 = 1.5;
    /// State / player: after ranked merchants (`FIRM_MERCHANT_END - margin`).
    pub const STATE_AFTER_MERCHANTS: f64 = FIRM_MERCHANT_END - STATE_FIRM_SLOT_MARGIN;
    /// State / player: after ranked producers (`FIRM_PRODUCER_END - margin`).
    pub const STATE_AFTER_PRODUCERS: f64 = FIRM_PRODUCER_END - STATE_FIRM_SLOT_MARGIN;
    /// State / player: after institution-between, before pops.
    pub const STATE_AFTER_FIRMS: f64 = 3.1;
    /// State / player: after institution-after-pops.
    pub const STATE_LAST: f64 = 5.1;

    /// Floor on actor-band priority when composing sell weight as `1 / p`.
    /// [`STATE_FIRST`] is `0.0`; without a floor that term is undefined.
    /// `1 / 0.5 = 2`, matching "priority 0.5 -> 2x weight" as the cap for
    /// the earliest slots.
    pub const SELL_ACTOR_PRIORITY_FLOOR: f64 = 0.01;

    /// Flat add to a sell order's priority after each successful fill.
    /// Small so repeat sales do not explode into a rich-get-richer spiral.
    pub const SUCCESSFUL_SELL_BONUS: f64 = 0.25;

    /// This-pick-only multiplier when buy and sell name the same counter-offer
    /// good. Does not change stored priority. Both sides must be `Some`.
    pub const SELL_COINCIDENCE_WEIGHT: f64 = 2.0;
}

/// Stand-in labor payouts until wage contracts exist.
///
/// Shares of on-hand coinage, rounded up to whole units. Living owners are
/// paid first; workers share what remains of their share. Missing owners
/// are not paid and do not drain the till. Producers with no process inputs
/// pay the whole till, split in this same owner:worker ratio.
pub mod labor_constants {
    /// Share of on-hand coinage paid to a living owner (ceil).
    pub const OWNER_SHARE: f64 = 0.30;
    /// Share of on-hand coinage paid to workers (ceil).
    pub const WORKER_SHARE: f64 = 0.30;
    /// Default share of a pop's on-hand Time that may be committed to wage hours.
    /// 0.5 is 12 work hours of a 24-hour (48 Time) grant.
    pub const WORK_TIME_FRACTION: f64 = 0.5;
    /// Days between labor-budget rewrites. 1 = every day. 0 skips.
    pub const BUDGET_INTERVAL: u32 = 1;
    /// Lowest whole wage-term amount after a labor budget rewrite. Never 0.
    pub const WAGE_AMOUNT_MIN: f64 = 1.0;
    /// Anger+fear share that lets the pop push the wage basket.
    pub const WAGE_PRESSURE_BAR: f64 = 0.25;
    /// Inclusive top of the "barely profitable" hold band. Above this, a calm
    /// firm may add a product bonus. Below 1.0 is unprofitable (trim flats).
    pub const WAGE_HOLD_MAX: f64 = 1.15;
}

/// Firm production-plan rewrite (end of day / planning phase).
pub mod firm_constants {
    /// Daily lerp toward new production, stock, and AMV targets (1.0 = snap).
    pub const PLANNING_LERP_RATE: f64 = 0.15;
    /// Grow a hit sell plan by this fraction of `sell_target`.
    pub const GROWTH_RATE: f64 = 0.10;
    /// Shrink an unprofitable line's restock aim by this fraction of its target.
    pub const SHRINK_RATE: f64 = 0.20;
    /// Input `stock_target` in days of `use_target` (2.0 = two days of inputs).
    pub const INPUT_COVER: f64 = 2.0;
    /// Output units kept on hand, in days of expected production.
    pub const OUTPUT_COVER: f64 = 0.5;
    /// Input `reserve_target` in days of `use_target` when supply is reliable.
    pub const RESERVE_COVER: f64 = 0.5;
    /// Extra reserve cover when today's purchases missed the old purchase target.
    pub const MISS_RESERVE_BONUS: f64 = 0.5;
    /// Fractional move of the firm's own `amv_target` on a miss or sell-out
    /// (0.05 = 5%). Not a lerp onto live market AMV.
    pub const AMV_NUDGE: f64 = 0.05;
    /// Default bid/ask margin on dual buy+sell rows when margin is still 0.
    pub const DEFAULT_MARGIN: f64 = 0.05;
    /// EMA blend for `FirmPRow.rolling_average` toward on-hand quantity.
    pub const ROLLING_AVG_WEIGHT: f64 = 0.25;
    /// Sell success (`sold / sell_target`) at or above this counts as strong demand.
    pub const SELL_SUCCESS_GROW: f64 = 0.80;
    /// Sell success below this counts as a miss (track `sold`, apply undersell pressure).
    pub const SELL_SUCCESS_SHRINK: f64 = 0.50;
    /// Starting [`crate::game::firm::FirmRecords::confidence`] (0 cautious .. 1 aggressive).
    pub const CONFIDENCE_DEFAULT: f64 = 0.5;
    /// Plan-pace multiplier at confidence 0 (half the advertised lerp/step).
    pub const CONFIDENCE_PACE_MIN: f64 = 0.5;
    /// Plan-pace multiplier at confidence 1 (one and a half times the advertised lerp/step).
    /// Mid confidence (0.5) keeps multiplier 1.0.
    pub const CONFIDENCE_PACE_MAX: f64 = 1.5;

    /// Peer band: line profit ratios within this fraction are "comparable".
    pub const PROFIT_PEER_BAND: f64 = 0.05;
    /// Below this AMV-out/AMV-in, profitability is low.
    pub const PROFIT_LOW: f64 = 0.80;
    /// Above this AMV-out/AMV-in, profitability is high.
    pub const PROFIT_HIGH: f64 = 1.20;
    /// Own quote vs market AMV band (0.10 = +/- 10%).
    pub const PRICE_BAND: f64 = 0.10;
    /// Market-share floor of the "respectable" band.
    pub const SHARE_LOW: f64 = 0.10;
    /// Market-share ceiling of the "respectable" band.
    pub const SHARE_HIGH: f64 = 0.40;
    /// |AMV trail slope| below this is a flat trend.
    pub const TREND_DEADBAND: f64 = 0.02;
    /// Relative AMV-trail stdev below this is low volatility.
    pub const VOLATILITY_LOW: f64 = 0.05;
    /// |sold/produced - 1| below this is reasonable turnover.
    pub const TURNOVER_BAND: f64 = 0.10;
    /// |stockpile / baseline - 1| below this is a normal leftover.
    pub const STOCKPILE_BAND: f64 = 0.10;
}

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

/// Loaded gameplay tunables. Defaults match the `*_constants` modules.
#[derive(Debug, Clone, Deserialize, PartialEq)]
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

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            pop: PopConfig::default(),
            player_resources: PlayerResourceConfig::default(),
            market: MarketConfig::default(),
            deal: DealConfig::default(),
            market_priority: MarketPriorityConfig::default(),
            labor: LaborConfig::default(),
            firm: FirmConfig::default(),
        }
    }
}

impl GameConfig {
    /// Loads tunables from a TOML file. Missing keys keep [`Default`].
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigLoadError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(ConfigLoadError::Io)?;
        Self::load_from_toml(&text)
    }

    /// Loads tunables from TOML text. Missing keys keep [`Default`].
    /// Rejects values outside the bounds written on each field.
    /// All bound failures are returned together.
    pub fn load_from_toml(text: &str) -> Result<Self, ConfigLoadError> {
        let cfg: Self = toml::from_str(text).map_err(ConfigLoadError::Toml)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Returns `Err` when a loaded value is outside its stated bound.
    /// Collects every failure so they can be fixed in one pass.
    pub fn validate(&self) -> Result<(), ConfigLoadError> {
        let mut problems = Vec::new();
        self.pop.validate(&mut problems);
        self.player_resources.validate(&mut problems);
        self.market.validate(&mut problems);
        self.deal.validate(&mut problems);
        self.market_priority.validate(&mut problems);
        self.labor.validate(&mut problems);
        self.firm.validate(&mut problems);
        if self.deal.high_salability < self.market.exchange_salability_min {
            problems.push(format!(
                "deal.high_salability ({}) must be >= market.exchange_salability_min ({})",
                self.deal.high_salability, self.market.exchange_salability_min
            ));
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigLoadError::Invalid(problems))
        }
    }
}

fn finite(problems: &mut Vec<String>, name: &str, value: f64) -> bool {
    if value.is_finite() {
        true
    } else {
        problems.push(format!("{name} must be finite"));
        false
    }
}

fn in_range(problems: &mut Vec<String>, name: &str, value: f64, min: f64, max: f64) {
    if !finite(problems, name, value) {
        return;
    }
    if !(min..=max).contains(&value) {
        problems.push(format!("{name} must be in {min}..={max}, got {value}"));
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

fn above(problems: &mut Vec<String>, name: &str, value: f64, min: f64) {
    if !finite(problems, name, value) {
        return;
    }
    if value <= min {
        problems.push(format!("{name} must be > {min}, got {value}"));
    }
}

fn ordered(problems: &mut Vec<String>, lo_name: &str, lo: f64, hi_name: &str, hi: f64) {
    let lo_ok = finite(problems, lo_name, lo);
    let hi_ok = finite(problems, hi_name, hi);
    if lo_ok && hi_ok && lo > hi {
        problems.push(format!("{lo_name} ({lo}) must be <= {hi_name} ({hi})"));
    }
}

fn band(problems: &mut Vec<String>, start_name: &str, start: f64, end_name: &str, end: f64) {
    let start_ok = finite(problems, start_name, start);
    let end_ok = finite(problems, end_name, end);
    if start_ok && end_ok && start >= end {
        problems.push(format!("{start_name} ({start}) must be < {end_name} ({end})"));
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct PopConfig {
    /// Days of basic+common consume-need to hold as a buffer. Default 0.20.
    /// Must sit in `savings_ratio_min..=savings_ratio_max`.
    pub default_savings_ratio: f64,
    /// Neutral personal interest rate. Default 0.05. Higher = more impatient.
    /// Must sit in `time_preference_min..=time_preference_max`.
    pub default_time_preference: f64,
    /// Neutral fear/greed. Default 0.0. Must sit in `risk_appetite_min..=max`.
    pub default_risk_appetite: f64,
    /// Daily lerp toward planning targets. Default 0.15 (1.0 = snap). Bound 0..=1.
    pub planning_lerp_rate: f64,
    /// Risk appetite floor. Default -1.0 (full fear). Must be <= max.
    pub risk_appetite_min: f64,
    /// Risk appetite ceiling. Default 1.0 (full greed).
    pub risk_appetite_max: f64,
    /// Hope weight into risk appetite. Default 1.0. Must be finite.
    pub risk_hope_weight: f64,
    /// Happiness weight into risk appetite. Default 0.40 (weaker than hope).
    pub risk_happiness_weight: f64,
    /// Fear weight into risk appetite. Default 1.0.
    pub risk_fear_weight: f64,
    /// Anger weight into risk appetite. Default 0.45 (weaker than fear).
    pub risk_anger_weight: f64,
    /// SOL-trend pull on risk appetite. Default 0.25 (falling SOL -> caution).
    pub risk_trend_weight: f64,
    /// Contentment pull that lowers risk appetite. Default 0.50.
    pub risk_contentment_weight: f64,
    /// Days-of-buffer floor. Default 0.0. Must be <= max.
    pub savings_ratio_min: f64,
    /// Days-of-buffer ceiling. Default 5.0 (five extra days of need).
    pub savings_ratio_max: f64,
    /// Greed lowers days of buffer; fear-side risk raises them. Default 0.10.
    pub savings_risk_weight: f64,
    /// Extra days of buffer from the Fear sentiment axis. Default 0.05.
    pub savings_fear_weight: f64,
    /// Extra days of buffer from unmet basic tier sat. Default 0.10.
    pub savings_unmet_basic_weight: f64,
    /// Extra days of buffer from a falling SOL trend. Default 0.15.
    pub savings_fall_sol_weight: f64,
    /// How much household growth inflates the savings pile. Default 1.0.
    pub savings_growth_buffer_weight: f64,
    /// At fear 0, this share of the buffer may be liquid AMV. Default 1.0. Bound 0..=1.
    pub savings_substitutability_calm: f64,
    /// At fear 1, this share may still be liquid AMV. Default 0.0. Bound 0..=1.
    pub savings_substitutability_fear: f64,
    /// Time-preference floor. Default 0.0. Must be <= max.
    pub time_preference_min: f64,
    /// Time-preference ceiling. Default 1.0.
    pub time_preference_max: f64,
    /// Anger raises time preference. Default 0.03.
    pub time_preference_anger_weight: f64,
    /// Unmet basic raises time preference. Default 0.04.
    pub time_preference_unmet_basic_weight: f64,
    /// Contentment lowers time preference. Default 0.02.
    pub time_preference_contentment_weight: f64,
    /// EMA blend for SOL rolling average. Default 0.25. Bound 0..=1.
    pub rolling_avg_weight: f64,
    /// Basic tier-sat weight in living-standard score. Default 3.0. Must be >= 0.
    pub score_weight_basic: f64,
    /// Common tier-sat weight in living-standard score. Default 1.5. Must be >= 0.
    pub score_weight_common: f64,
    /// Luxury tier-sat weight in living-standard score. Default 1.0. Must be >= 0.
    pub score_weight_luxury: f64,
    /// Anger from unmet basic. Default 0.08. Must be finite.
    pub anger_sentiment_rate: f64,
    /// Fear from unmet basic. Default 0.04.
    pub fear_sentiment_rate: f64,
    /// Contentment from met basic * common mood. Default 0.05.
    pub contentment_sentiment_rate: f64,
    /// Happiness from common mood. Default 0.03.
    pub happiness_sentiment_rate: f64,
    /// Hope from luxury sat. Default 0.02.
    pub hope_sentiment_rate: f64,
    /// Extra contentment from a rising SOL trend. Default 0.02.
    pub trend_contentment_sentiment_rate: f64,
    /// Extra happiness from a rising SOL trend. Default 0.03.
    pub trend_happiness_sentiment_rate: f64,
    /// Extra hope from a rising SOL trend. Default 0.05.
    pub trend_hope_sentiment_rate: f64,
    /// Extra anger from a falling SOL trend. Default 0.04.
    pub trend_anger_sentiment_rate: f64,
    /// Extra fear from a falling SOL trend. Default 0.03.
    pub trend_fear_sentiment_rate: f64,
    /// Ignore |SOL trend| below this. Default 0.5. Must be >= 0.
    pub sentiment_trend_deadband: f64,
    /// Rise-side sentiment share scale. Default 0.03. Must be >= 0.
    pub sentiment_rise_gain: f64,
    /// Fall-side sentiment share scale. Default 0.05 (usually > rise). Must be >= 0.
    pub sentiment_fall_gain: f64,
}

impl Default for PopConfig {
    fn default() -> Self {
        Self {
            default_savings_ratio: pop_constants::DEFAULT_SAVINGS_RATIO,
            default_time_preference: pop_constants::DEFAULT_TIME_PREFERENCE,
            default_risk_appetite: pop_constants::DEFAULT_RISK_APPETITE,
            planning_lerp_rate: pop_constants::PLANNING_LERP_RATE,
            risk_appetite_min: pop_constants::RISK_APPETITE_MIN,
            risk_appetite_max: pop_constants::RISK_APPETITE_MAX,
            risk_hope_weight: pop_constants::RISK_HOPE_WEIGHT,
            risk_happiness_weight: pop_constants::RISK_HAPPINESS_WEIGHT,
            risk_fear_weight: pop_constants::RISK_FEAR_WEIGHT,
            risk_anger_weight: pop_constants::RISK_ANGER_WEIGHT,
            risk_trend_weight: pop_constants::RISK_TREND_WEIGHT,
            risk_contentment_weight: pop_constants::RISK_CONTENTMENT_WEIGHT,
            savings_ratio_min: pop_constants::SAVINGS_RATIO_MIN,
            savings_ratio_max: pop_constants::SAVINGS_RATIO_MAX,
            savings_risk_weight: pop_constants::SAVINGS_RISK_WEIGHT,
            savings_fear_weight: pop_constants::SAVINGS_FEAR_WEIGHT,
            savings_unmet_basic_weight: pop_constants::SAVINGS_UNMET_BASIC_WEIGHT,
            savings_fall_sol_weight: pop_constants::SAVINGS_FALL_SOL_WEIGHT,
            savings_growth_buffer_weight: pop_constants::SAVINGS_GROWTH_BUFFER_WEIGHT,
            savings_substitutability_calm: pop_constants::SAVINGS_SUBSTITUTABILITY_CALM,
            savings_substitutability_fear: pop_constants::SAVINGS_SUBSTITUTABILITY_FEAR,
            time_preference_min: pop_constants::TIME_PREFERENCE_MIN,
            time_preference_max: pop_constants::TIME_PREFERENCE_MAX,
            time_preference_anger_weight: pop_constants::TIME_PREFERENCE_ANGER_WEIGHT,
            time_preference_unmet_basic_weight: pop_constants::TIME_PREFERENCE_UNMET_BASIC_WEIGHT,
            time_preference_contentment_weight: pop_constants::TIME_PREFERENCE_CONTENTMENT_WEIGHT,
            rolling_avg_weight: pop_constants::ROLLING_AVG_WEIGHT,
            score_weight_basic: pop_constants::SCORE_WEIGHT_BASIC,
            score_weight_common: pop_constants::SCORE_WEIGHT_COMMON,
            score_weight_luxury: pop_constants::SCORE_WEIGHT_LUXURY,
            anger_sentiment_rate: pop_constants::ANGER_SENTIMENT_RATE,
            fear_sentiment_rate: pop_constants::FEAR_SENTIMENT_RATE,
            contentment_sentiment_rate: pop_constants::CONTENTMENT_SENTIMENT_RATE,
            happiness_sentiment_rate: pop_constants::HAPPINESS_SENTIMENT_RATE,
            hope_sentiment_rate: pop_constants::HOPE_SENTIMENT_RATE,
            trend_contentment_sentiment_rate: pop_constants::TREND_CONTENTMENT_SENTIMENT_RATE,
            trend_happiness_sentiment_rate: pop_constants::TREND_HAPPINESS_SENTIMENT_RATE,
            trend_hope_sentiment_rate: pop_constants::TREND_HOPE_SENTIMENT_RATE,
            trend_anger_sentiment_rate: pop_constants::TREND_ANGER_SENTIMENT_RATE,
            trend_fear_sentiment_rate: pop_constants::TREND_FEAR_SENTIMENT_RATE,
            sentiment_trend_deadband: pop_constants::SENTIMENT_TREND_DEADBAND,
            sentiment_rise_gain: pop_constants::SENTIMENT_RISE_GAIN,
            sentiment_fall_gain: pop_constants::SENTIMENT_FALL_GAIN,
        }
    }
}

impl PopConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        ordered(
            problems,
            "pop.risk_appetite_min",
            self.risk_appetite_min,
            "pop.risk_appetite_max",
            self.risk_appetite_max,
        );
        in_range(
            problems,
            "pop.default_risk_appetite",
            self.default_risk_appetite,
            self.risk_appetite_min,
            self.risk_appetite_max,
        );
        in_range(problems, "pop.planning_lerp_rate", self.planning_lerp_rate, 0.0, 1.0);
        ordered(
            problems,
            "pop.savings_ratio_min",
            self.savings_ratio_min,
            "pop.savings_ratio_max",
            self.savings_ratio_max,
        );
        at_least(problems, "pop.savings_ratio_min", self.savings_ratio_min, 0.0);
        in_range(
            problems,
            "pop.default_savings_ratio",
            self.default_savings_ratio,
            self.savings_ratio_min,
            self.savings_ratio_max,
        );
        in_range(
            problems,
            "pop.savings_substitutability_calm",
            self.savings_substitutability_calm,
            0.0,
            1.0,
        );
        in_range(
            problems,
            "pop.savings_substitutability_fear",
            self.savings_substitutability_fear,
            0.0,
            1.0,
        );
        ordered(
            problems,
            "pop.time_preference_min",
            self.time_preference_min,
            "pop.time_preference_max",
            self.time_preference_max,
        );
        at_least(problems, "pop.time_preference_min", self.time_preference_min, 0.0);
        in_range(
            problems,
            "pop.default_time_preference",
            self.default_time_preference,
            self.time_preference_min,
            self.time_preference_max,
        );
        in_range(problems, "pop.rolling_avg_weight", self.rolling_avg_weight, 0.0, 1.0);
        at_least(problems, "pop.score_weight_basic", self.score_weight_basic, 0.0);
        at_least(problems, "pop.score_weight_common", self.score_weight_common, 0.0);
        at_least(problems, "pop.score_weight_luxury", self.score_weight_luxury, 0.0);
        at_least(problems, "pop.sentiment_trend_deadband", self.sentiment_trend_deadband, 0.0);
        at_least(problems, "pop.sentiment_rise_gain", self.sentiment_rise_gain, 0.0);
        at_least(problems, "pop.sentiment_fall_gain", self.sentiment_fall_gain, 0.0);
        for (name, value) in [
            ("pop.risk_hope_weight", self.risk_hope_weight),
            ("pop.risk_happiness_weight", self.risk_happiness_weight),
            ("pop.risk_fear_weight", self.risk_fear_weight),
            ("pop.risk_anger_weight", self.risk_anger_weight),
            ("pop.risk_trend_weight", self.risk_trend_weight),
            ("pop.risk_contentment_weight", self.risk_contentment_weight),
            ("pop.savings_risk_weight", self.savings_risk_weight),
            ("pop.savings_fear_weight", self.savings_fear_weight),
            ("pop.savings_unmet_basic_weight", self.savings_unmet_basic_weight),
            ("pop.savings_fall_sol_weight", self.savings_fall_sol_weight),
            ("pop.savings_growth_buffer_weight", self.savings_growth_buffer_weight),
            ("pop.time_preference_anger_weight", self.time_preference_anger_weight),
            ("pop.time_preference_unmet_basic_weight", self.time_preference_unmet_basic_weight),
            ("pop.time_preference_contentment_weight", self.time_preference_contentment_weight),
            ("pop.anger_sentiment_rate", self.anger_sentiment_rate),
            ("pop.fear_sentiment_rate", self.fear_sentiment_rate),
            ("pop.contentment_sentiment_rate", self.contentment_sentiment_rate),
            ("pop.happiness_sentiment_rate", self.happiness_sentiment_rate),
            ("pop.hope_sentiment_rate", self.hope_sentiment_rate),
            ("pop.trend_contentment_sentiment_rate", self.trend_contentment_sentiment_rate),
            ("pop.trend_happiness_sentiment_rate", self.trend_happiness_sentiment_rate),
            ("pop.trend_hope_sentiment_rate", self.trend_hope_sentiment_rate),
            ("pop.trend_anger_sentiment_rate", self.trend_anger_sentiment_rate),
            ("pop.trend_fear_sentiment_rate", self.trend_fear_sentiment_rate),
        ] {
            finite(problems, name, value);
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayerResourceConfig {
    /// Culture per 1.0 common tier sat, per person. Default 1.0. Must be finite.
    pub common_culture_rate: f64,
    /// Weaker unclamped luxury culture ladder. Default 0.35.
    pub luxury_culture_rate: f64,
    /// Legitimacy potential from the first desire. Default 0.5.
    pub first_desire_legitimacy: f64,
    /// Extra legitimacy potential per desire after the first. Default 0.1.
    pub extra_desire_legitimacy: f64,
    /// Luxury legitimacy per 1.0 luxury tier-sat mass. Default 0.40.
    pub luxury_legitimacy_rate: f64,
    /// Mood term scaled by potential. Default 0.75.
    pub mood_potential_modifier: f64,
    /// Trend term scaled by potential. Default 0.5.
    pub trend_potential_modifier: f64,
    /// Anger hurts legitimacy. Default 0.40.
    pub anger_legitimacy_rate: f64,
    /// Fear hurts legitimacy (weaker than anger). Default 0.22.
    pub fear_legitimacy_rate: f64,
    /// Happiness helps legitimacy. Default 0.18.
    pub happiness_legitimacy_rate: f64,
    /// Hope helps legitimacy (weaker than happiness). Default 0.12.
    pub hope_legitimacy_rate: f64,
    /// Rising SOL trend coefficient. Default 0.03.
    pub trend_legitimacy_rise: f64,
    /// Falling SOL trend coefficient (stronger than rise). Default 0.05.
    pub trend_legitimacy_fall: f64,
}

impl Default for PlayerResourceConfig {
    fn default() -> Self {
        Self {
            common_culture_rate: player_resource_constants::COMMON_CULTURE_RATE,
            luxury_culture_rate: player_resource_constants::LUXURY_CULTURE_RATE,
            first_desire_legitimacy: player_resource_constants::FIRST_DESIRE_LEGITIMACY,
            extra_desire_legitimacy: player_resource_constants::EXTRA_DESIRE_LEGITIMACY,
            luxury_legitimacy_rate: player_resource_constants::LUXURY_LEGITIMACY_RATE,
            mood_potential_modifier: player_resource_constants::MOOD_POTENTIAL_MODIFIER,
            trend_potential_modifier: player_resource_constants::TREND_POTENTIAL_MODIFIER,
            anger_legitimacy_rate: player_resource_constants::ANGER_LEGITIMACY_RATE,
            fear_legitimacy_rate: player_resource_constants::FEAR_LEGITIMACY_RATE,
            happiness_legitimacy_rate: player_resource_constants::HAPPINESS_LEGITIMACY_RATE,
            hope_legitimacy_rate: player_resource_constants::HOPE_LEGITIMACY_RATE,
            trend_legitimacy_rise: player_resource_constants::TREND_LEGITIMACY_RISE,
            trend_legitimacy_fall: player_resource_constants::TREND_LEGITIMACY_FALL,
        }
    }
}

impl PlayerResourceConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        for (name, value) in [
            ("player_resources.common_culture_rate", self.common_culture_rate),
            ("player_resources.luxury_culture_rate", self.luxury_culture_rate),
            ("player_resources.first_desire_legitimacy", self.first_desire_legitimacy),
            ("player_resources.extra_desire_legitimacy", self.extra_desire_legitimacy),
            ("player_resources.luxury_legitimacy_rate", self.luxury_legitimacy_rate),
            ("player_resources.mood_potential_modifier", self.mood_potential_modifier),
            ("player_resources.trend_potential_modifier", self.trend_potential_modifier),
            ("player_resources.anger_legitimacy_rate", self.anger_legitimacy_rate),
            ("player_resources.fear_legitimacy_rate", self.fear_legitimacy_rate),
            ("player_resources.happiness_legitimacy_rate", self.happiness_legitimacy_rate),
            ("player_resources.hope_legitimacy_rate", self.hope_legitimacy_rate),
            ("player_resources.trend_legitimacy_rise", self.trend_legitimacy_rise),
            ("player_resources.trend_legitimacy_fall", self.trend_legitimacy_fall),
        ] {
            finite(problems, name, value);
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct MarketConfig {
    /// Smallest stored |AMV| / |average_price|. Default 0.00001. Must be > 0.
    pub amv_min_abs: f64,
    /// Salability for a new or unrecorded good. Default 0.4. Bound 0..=1.
    /// Must stay below `exchange_salability_min` so unknown goods are not till money.
    pub salability_default: f64,
    /// Minimum salability to treat on-hand stock as exchange tender. Default 0.6.
    /// Bound 0..=1.
    pub exchange_salability_min: f64,
    /// When a pile is both sold and exchanged, each side keeps at least this share.
    /// Default 0.1 (10%). Bound 0..=0.5.
    pub sell_exchange_edge: f64,
    /// Failed-deal retries a buy/request may take. Default 2. `tries` starts at 0.
    pub buy_try_limit: u32,
    /// Flat transport units charged per meeting. Default 1.0. Must be >= 0. Not AMV.
    pub transaction_cost: f64,
    /// Successful-exchange AMV pull toward basket midpoint. Default 0.25. Bound 0..=1.
    pub amv_accept_blend: f64,
    /// Rejected-meeting AMV pull (sought up, tenders down). Default 0.10. Bound 0..=1.
    pub amv_reject_blend: f64,
    /// Sought-good up-push vs tender down-push. Default 1.1. Must be > 0.
    pub amv_reject_demand_edge: f64,
    /// Day-end leftover AMV added per leftover-to-volume multiple. Default 0.10.
    /// Bound 0..=1. Factor is `1 + blend * miss / purchased`.
    pub amv_leftover_blend: f64,
    /// Skip leftover AMV when both sides leftover and the imbalance is below this.
    /// Default 0.10. Bound 0..=1.
    pub amv_leftover_band: f64,
    /// Day-end lerp of salability toward payment/tender. Default 0.25. Bound 0..=1.
    pub salability_blend: f64,
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            amv_min_abs: market_constants::AMV_MIN_ABS,
            salability_default: market_constants::SALABILITY_DEFAULT,
            exchange_salability_min: market_constants::EXCHANGE_SALABILITY_MIN,
            sell_exchange_edge: market_constants::SELL_EXCHANGE_EDGE,
            buy_try_limit: market_constants::BUY_TRY_LIMIT,
            transaction_cost: market_constants::TRANSACTION_COST,
            amv_accept_blend: market_constants::AMV_ACCEPT_BLEND,
            amv_reject_blend: market_constants::AMV_REJECT_BLEND,
            amv_reject_demand_edge: market_constants::AMV_REJECT_DEMAND_EDGE,
            amv_leftover_blend: market_constants::AMV_LEFTOVER_BLEND,
            amv_leftover_band: market_constants::AMV_LEFTOVER_BAND,
            salability_blend: market_constants::SALABILITY_BLEND,
        }
    }
}

impl MarketConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        above(problems, "market.amv_min_abs", self.amv_min_abs, 0.0);
        in_range(problems, "market.salability_default", self.salability_default, 0.0, 1.0);
        in_range(
            problems,
            "market.exchange_salability_min",
            self.exchange_salability_min,
            0.0,
            1.0,
        );
        if self.salability_default.is_finite()
            && self.exchange_salability_min.is_finite()
            && self.salability_default >= self.exchange_salability_min
        {
            problems.push(format!(
                "market.salability_default ({}) must be < market.exchange_salability_min ({})",
                self.salability_default, self.exchange_salability_min
            ));
        }
        in_range(problems, "market.sell_exchange_edge", self.sell_exchange_edge, 0.0, 0.5);
        at_least(problems, "market.transaction_cost", self.transaction_cost, 0.0);
        in_range(problems, "market.amv_accept_blend", self.amv_accept_blend, 0.0, 1.0);
        in_range(problems, "market.amv_reject_blend", self.amv_reject_blend, 0.0, 1.0);
        above(problems, "market.amv_reject_demand_edge", self.amv_reject_demand_edge, 0.0);
        in_range(problems, "market.amv_leftover_blend", self.amv_leftover_blend, 0.0, 1.0);
        in_range(problems, "market.amv_leftover_band", self.amv_leftover_band, 0.0, 1.0);
        in_range(problems, "market.salability_blend", self.salability_blend, 0.0, 1.0);
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct DealConfig {
    /// Pop minimum AMV keep (`received / given`). Default 0.25 (up to 75% loss).
    /// Bound 0..=1. Buyers still accept windfalls (`keep >= 1.0`).
    pub pop_amv_min_keep: f64,
    /// Firm minimum AMV keep. Default 0.50 (up to 50% loss). Bound 0..=1.
    pub firm_amv_min_keep: f64,
    /// Looser keep when the firm needs a received good. Default 0.25 (same as pop).
    /// Bound 0..=1. Must be <= `firm_amv_min_keep`.
    pub firm_amv_need_keep: f64,
    /// Salability at or above this is money-like for buy tenders. Default 0.8.
    /// Bound 0..=1. Must be >= market `exchange_salability_min`.
    pub high_salability: f64,
}

impl Default for DealConfig {
    fn default() -> Self {
        Self {
            pop_amv_min_keep: deal_constants::POP_AMV_MIN_KEEP,
            firm_amv_min_keep: deal_constants::FIRM_AMV_MIN_KEEP,
            firm_amv_need_keep: deal_constants::FIRM_AMV_NEED_KEEP,
            high_salability: deal_constants::HIGH_SALABILITY,
        }
    }
}

impl DealConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        in_range(problems, "deal.pop_amv_min_keep", self.pop_amv_min_keep, 0.0, 1.0);
        in_range(problems, "deal.firm_amv_min_keep", self.firm_amv_min_keep, 0.0, 1.0);
        in_range(problems, "deal.firm_amv_need_keep", self.firm_amv_need_keep, 0.0, 1.0);
        ordered(
            problems,
            "deal.firm_amv_need_keep",
            self.firm_amv_need_keep,
            "deal.firm_amv_min_keep",
            self.firm_amv_min_keep,
        );
        in_range(problems, "deal.high_salability", self.high_salability, 0.0, 1.0);
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct MarketPriorityConfig {
    /// Institution slot before all firms. Default 1.0. Lower goes first.
    pub institution_before_firms: f64,
    /// Institution slot after both firm bands, before pops. Default 3.0.
    pub institution_between_firms_and_pops: f64,
    /// Institution slot after the pop band. Default 5.0.
    pub institution_after_pops: f64,
    /// Merchant firm buy-band start (inclusive). Default 2.0. Must be < end.
    pub firm_merchant_start: f64,
    /// Merchant firm buy-band end (exclusive). Default 2.5.
    pub firm_merchant_end: f64,
    /// Producer firm buy-band start (inclusive). Default 2.5. Must be >= merchant end.
    pub firm_producer_start: f64,
    /// Producer firm buy-band end (exclusive). Default 3.0.
    pub firm_producer_end: f64,
    /// How far before a firm-band exclusive end the matching state slot sits.
    /// Default 0.01. Must be > 0 and less than each firm-band width.
    pub state_firm_slot_margin: f64,
    /// Pop buy-band start (inclusive). Default 4.0. Unranked pop orders sit here.
    pub pop_start: f64,
    /// Pop buy-band end (exclusive). Default 5.0.
    pub pop_end: f64,
    /// State / player: before everyone. Default 0.0.
    pub state_first: f64,
    /// State / player: after institution-before-firms, before merchants. Default 1.5.
    pub state_before_firms: f64,
    /// State / player: after institution-between, before pops. Default 3.1.
    pub state_after_firms: f64,
    /// State / player: after institution-after-pops. Default 5.1.
    pub state_last: f64,
    /// Floor on actor-band priority when composing sell weight as `1 / p`.
    /// Default 0.01 so STATE_FIRST 0.0 is defined. Must be > 0.
    pub sell_actor_priority_floor: f64,
    /// Flat add to a sell order's priority after each successful fill. Default 0.25.
    /// Must be >= 0.
    pub successful_sell_bonus: f64,
    /// This-pick-only multiplier when buy and sell name the same counter-offer.
    /// Default 2.0. Must be > 0.
    pub sell_coincidence_weight: f64,
}

impl Default for MarketPriorityConfig {
    fn default() -> Self {
        Self {
            institution_before_firms: market_priority::INSTITUTION_BEFORE_FIRMS,
            institution_between_firms_and_pops: market_priority::INSTITUTION_BETWEEN_FIRMS_AND_POPS,
            institution_after_pops: market_priority::INSTITUTION_AFTER_POPS,
            firm_merchant_start: market_priority::FIRM_MERCHANT_START,
            firm_merchant_end: market_priority::FIRM_MERCHANT_END,
            firm_producer_start: market_priority::FIRM_PRODUCER_START,
            firm_producer_end: market_priority::FIRM_PRODUCER_END,
            state_firm_slot_margin: market_priority::STATE_FIRM_SLOT_MARGIN,
            pop_start: market_priority::POP_START,
            pop_end: market_priority::POP_END,
            state_first: market_priority::STATE_FIRST,
            state_before_firms: market_priority::STATE_BEFORE_FIRMS,
            state_after_firms: market_priority::STATE_AFTER_FIRMS,
            state_last: market_priority::STATE_LAST,
            sell_actor_priority_floor: market_priority::SELL_ACTOR_PRIORITY_FLOOR,
            successful_sell_bonus: market_priority::SUCCESSFUL_SELL_BONUS,
            sell_coincidence_weight: market_priority::SELL_COINCIDENCE_WEIGHT,
        }
    }
}

impl MarketPriorityConfig {
    pub fn firm_merchant(&self) -> f64 {
        self.firm_merchant_start
    }

    pub fn firm_producer(&self) -> f64 {
        self.firm_producer_start
    }

    pub fn state_after_merchants(&self) -> f64 {
        self.firm_merchant_end - self.state_firm_slot_margin
    }

    pub fn state_after_producers(&self) -> f64 {
        self.firm_producer_end - self.state_firm_slot_margin
    }

    fn validate(&self, problems: &mut Vec<String>) {
        for (name, value) in [
            ("market_priority.institution_before_firms", self.institution_before_firms),
            ("market_priority.institution_between_firms_and_pops", self.institution_between_firms_and_pops),
            ("market_priority.institution_after_pops", self.institution_after_pops),
            ("market_priority.state_first", self.state_first),
            ("market_priority.state_before_firms", self.state_before_firms),
            ("market_priority.state_after_firms", self.state_after_firms),
            ("market_priority.state_last", self.state_last),
        ] {
            finite(problems, name, value);
        }
        band(
            problems,
            "market_priority.firm_merchant_start",
            self.firm_merchant_start,
            "market_priority.firm_merchant_end",
            self.firm_merchant_end,
        );
        band(
            problems,
            "market_priority.firm_producer_start",
            self.firm_producer_start,
            "market_priority.firm_producer_end",
            self.firm_producer_end,
        );
        ordered(
            problems,
            "market_priority.firm_merchant_end",
            self.firm_merchant_end,
            "market_priority.firm_producer_start",
            self.firm_producer_start,
        );
        band(
            problems,
            "market_priority.pop_start",
            self.pop_start,
            "market_priority.pop_end",
            self.pop_end,
        );
        above(problems, "market_priority.state_firm_slot_margin", self.state_firm_slot_margin, 0.0);
        if self.firm_merchant_start < self.firm_merchant_end {
            let merchant_width = self.firm_merchant_end - self.firm_merchant_start;
            if self.state_firm_slot_margin >= merchant_width {
                problems.push(
                    "market_priority.state_firm_slot_margin must be < merchant band width".into(),
                );
            }
        }
        if self.firm_producer_start < self.firm_producer_end {
            let producer_width = self.firm_producer_end - self.firm_producer_start;
            if self.state_firm_slot_margin >= producer_width {
                problems.push(
                    "market_priority.state_firm_slot_margin must be < producer band width".into(),
                );
            }
        }
        above(
            problems,
            "market_priority.sell_actor_priority_floor",
            self.sell_actor_priority_floor,
            0.0,
        );
        at_least(problems, "market_priority.successful_sell_bonus", self.successful_sell_bonus, 0.0);
        above(
            problems,
            "market_priority.sell_coincidence_weight",
            self.sell_coincidence_weight,
            0.0,
        );
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct LaborConfig {
    /// Share of on-hand coinage paid to a living owner (ceil). Default 0.30.
    /// Bound 0..=1. Missing owners do not drain the till.
    pub owner_share: f64,
    /// Share of on-hand coinage paid to workers (ceil). Default 0.30.
    /// Bound 0..=1. Taken from what remains after the owner payout.
    pub worker_share: f64,
    /// Share of on-hand Time a pop may commit to wage hours. Default 0.5.
    /// Bound 0..=1. Caps [`crate::game::workforce::Workforce::hours`] at settle.
    /// Stand-in until culture, class, religion, and laws supply this as a cap.
    pub work_time_fraction: f64,
    /// Days between [`crate::game::firm::Firm::budget_labor`] rewrites.
    /// 1 = every day. 0 skips. No hire/fire; hours and wage amounts only.
    pub budget_interval: u32,
}

impl Default for LaborConfig {
    fn default() -> Self {
        Self {
            owner_share: labor_constants::OWNER_SHARE,
            worker_share: labor_constants::WORKER_SHARE,
            work_time_fraction: labor_constants::WORK_TIME_FRACTION,
            budget_interval: labor_constants::BUDGET_INTERVAL,
        }
    }
}

impl LaborConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        in_range(problems, "labor.owner_share", self.owner_share, 0.0, 1.0);
        in_range(problems, "labor.worker_share", self.worker_share, 0.0, 1.0);
        in_range(
            problems,
            "labor.work_time_fraction",
            self.work_time_fraction,
            0.0,
            1.0,
        );
    }
}

/// Firm planning rewrite tunables. Defaults match [`firm_constants`].
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct FirmConfig {
    /// Daily lerp toward new production, stock, and AMV targets. Default 0.15.
    /// Bound 0..=1.
    pub planning_lerp_rate: f64,
    /// Grow a hit sell plan by this fraction of `sell_target`. Default 0.10.
    /// Bound 0..=1.
    pub growth_rate: f64,
    /// Shrink an unprofitable line's restock aim by this fraction. Default 0.20.
    /// Bound 0..=1.
    pub shrink_rate: f64,
    /// Input stock target in days of use. Default 2.0. Must be >= 0.
    pub input_cover: f64,
    /// Output units kept on hand, in days of expected production. Default 0.5.
    /// Must be >= 0.
    pub output_cover: f64,
    /// Reliable-supply reserve in days of use. Default 0.5. Must be >= 0.
    pub reserve_cover: f64,
    /// Extra reserve cover when purchases missed. Default 0.5. Must be >= 0.
    pub miss_reserve_bonus: f64,
    /// Fractional move of the firm's own `amv_target` on a miss or sell-out.
    /// Default 0.05. Bound 0..=1.
    pub amv_nudge: f64,
    /// Default dual-row margin when margin is still 0. Default 0.05. Bound 0..=1.
    pub default_margin: f64,
    /// EMA blend for rolling average toward on-hand quantity. Default 0.25.
    /// Bound 0..=1.
    pub rolling_avg_weight: f64,
    /// Sell success (`sold / sell_target`) at or above this is strong demand.
    /// Default 0.80. Bound 0..=1.
    pub sell_success_grow: f64,
    /// Sell success below this is a miss. Default 0.50.
    /// Bound 0..=1. Must be <= `sell_success_grow`.
    pub sell_success_shrink: f64,
    /// Starting firm confidence. Default 0.5. Bound 0..=1.
    pub confidence_default: f64,
    /// Plan-pace multiplier at confidence 0. Default 0.5. Must be >= 0.
    pub confidence_pace_min: f64,
    /// Plan-pace multiplier at confidence 1. Default 1.5. Must be >= `confidence_pace_min`.
    pub confidence_pace_max: f64,
}

impl Default for FirmConfig {
    fn default() -> Self {
        Self {
            planning_lerp_rate: firm_constants::PLANNING_LERP_RATE,
            growth_rate: firm_constants::GROWTH_RATE,
            shrink_rate: firm_constants::SHRINK_RATE,
            input_cover: firm_constants::INPUT_COVER,
            output_cover: firm_constants::OUTPUT_COVER,
            reserve_cover: firm_constants::RESERVE_COVER,
            miss_reserve_bonus: firm_constants::MISS_RESERVE_BONUS,
            amv_nudge: firm_constants::AMV_NUDGE,
            default_margin: firm_constants::DEFAULT_MARGIN,
            rolling_avg_weight: firm_constants::ROLLING_AVG_WEIGHT,
            sell_success_grow: firm_constants::SELL_SUCCESS_GROW,
            sell_success_shrink: firm_constants::SELL_SUCCESS_SHRINK,
            confidence_default: firm_constants::CONFIDENCE_DEFAULT,
            confidence_pace_min: firm_constants::CONFIDENCE_PACE_MIN,
            confidence_pace_max: firm_constants::CONFIDENCE_PACE_MAX,
        }
    }
}

impl FirmConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        in_range(problems, "firm.planning_lerp_rate", self.planning_lerp_rate, 0.0, 1.0);
        in_range(problems, "firm.growth_rate", self.growth_rate, 0.0, 1.0);
        in_range(problems, "firm.shrink_rate", self.shrink_rate, 0.0, 1.0);
        at_least(problems, "firm.input_cover", self.input_cover, 0.0);
        at_least(problems, "firm.output_cover", self.output_cover, 0.0);
        at_least(problems, "firm.reserve_cover", self.reserve_cover, 0.0);
        at_least(problems, "firm.miss_reserve_bonus", self.miss_reserve_bonus, 0.0);
        in_range(problems, "firm.amv_nudge", self.amv_nudge, 0.0, 1.0);
        in_range(problems, "firm.default_margin", self.default_margin, 0.0, 1.0);
        in_range(problems, "firm.rolling_avg_weight", self.rolling_avg_weight, 0.0, 1.0);
        in_range(problems, "firm.sell_success_grow", self.sell_success_grow, 0.0, 1.0);
        in_range(problems, "firm.sell_success_shrink", self.sell_success_shrink, 0.0, 1.0);
        ordered(
            problems,
            "firm.sell_success_shrink",
            self.sell_success_shrink,
            "firm.sell_success_grow",
            self.sell_success_grow,
        );
        in_range(problems, "firm.confidence_default", self.confidence_default, 0.0, 1.0);
        at_least(problems, "firm.confidence_pace_min", self.confidence_pace_min, 0.0);
        at_least(problems, "firm.confidence_pace_max", self.confidence_pace_max, 0.0);
        ordered(
            problems,
            "firm.confidence_pace_min",
            self.confidence_pace_min,
            "firm.confidence_pace_max",
            self.confidence_pace_max,
        );
    }
}

#[cfg(test)]
mod config_should {
    use super::*;

    #[test]
    fn load_from_toml_keeps_defaults_for_missing_keys() {
        let cfg = GameConfig::load_from_toml("[labor]\nworker_share = 0.5\n").expect("toml");
        assert_eq!(cfg.labor.worker_share, 0.5);
        assert_eq!(cfg.labor.owner_share, labor_constants::OWNER_SHARE);
        assert_eq!(cfg.deal.high_salability, deal_constants::HIGH_SALABILITY);
    }

    #[test]
    fn load_from_path_reads_world_config_file() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data/world/config.toml");
        let cfg = GameConfig::load_from_path(&path).expect("world config");
        assert_eq!(cfg, GameConfig::default());
    }

    #[test]
    fn load_from_toml_rejects_salability_outside_unit_interval() {
        let err = GameConfig::load_from_toml("[market]\nsalability_default = 1.5\n")
            .expect_err("out of range");
        let msg = err.to_string();
        assert!(msg.contains("salability_default"), "{msg}");
    }

    #[test]
    fn load_from_toml_rejects_firm_lerp_outside_unit_interval() {
        let err = GameConfig::load_from_toml("[firm]\nplanning_lerp_rate = 1.5\n")
            .expect_err("out of range");
        let msg = err.to_string();
        assert!(msg.contains("planning_lerp_rate"), "{msg}");
    }

    #[test]
    fn load_from_toml_rejects_inverted_savings_bounds() {
        let err = GameConfig::load_from_toml(
            "[pop]\nsavings_ratio_min = 3.0\nsavings_ratio_max = 1.0\n",
        )
        .expect_err("inverted");
        let msg = err.to_string();
        assert!(msg.contains("savings_ratio"), "{msg}");
    }

    #[test]
    fn load_from_toml_rejects_zero_amv_min_abs() {
        let err = GameConfig::load_from_toml("[market]\namv_min_abs = 0.0\n")
            .expect_err("zero min abs");
        let msg = err.to_string();
        assert!(msg.contains("amv_min_abs"), "{msg}");
    }

    #[test]
    fn default_config_passes_validate() {
        GameConfig::default().validate().expect("defaults are in bounds");
    }

    #[test]
    fn load_from_toml_reports_every_bound_failure() {
        let err = GameConfig::load_from_toml(
            "[market]\nsalability_default = 1.5\namv_min_abs = 0.0\n\
             [labor]\nowner_share = -0.1\nworker_share = 2.0\n",
        )
        .expect_err("several bad values");
        let msg = err.to_string();
        assert!(msg.contains("salability_default"), "{msg}");
        assert!(msg.contains("amv_min_abs"), "{msg}");
        assert!(msg.contains("owner_share"), "{msg}");
        assert!(msg.contains("worker_share"), "{msg}");
        let dashes = msg.matches("\n- ").count();
        assert!(dashes >= 4, "expected a listed mass error, got:\n{msg}");
    }
}
