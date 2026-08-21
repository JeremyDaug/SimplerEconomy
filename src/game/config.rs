//! Gameplay tunables — one place for simulation constants.
//!
//! Treat this as a constants catalog for now. Do not re-hardcode the same
//! numbers in systems; import from here. Later this can grow into load/mod
//! overrides without changing call sites that already read these values.

/// Living-standard score history, trend, and related mood rates.
pub mod pop_constants {
    // History Length
    /// Compile-time max ring slots for PopRecords histories (SOL, pop size, liquid wealth).
    pub const HISTORY_MAX: usize = 16;

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

    /// Legitimacy from the first common desire (scaled by average desire sat).
    /// Further desires add [`EXTRA_DESIRE_LEGITIMACY`] each, so extra wants are
    /// a weak legitimacy source.
    pub const FIRST_DESIRE_LEGITIMACY: f64 = 0.5;
    /// Legitimacy added per common desire after the first.
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
