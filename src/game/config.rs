//! Gameplay tunables — one place for simulation knobs.
//!
//! Treat this as a constants catalog for now. Do not re-hardcode the same
//! numbers in systems; import from here. Later this can grow into load/mod
//! overrides without changing call sites that already read these values.

/// Living-standard score history, trend, and related mood knobs.
pub mod pop_constants {
    // History Length
    /// Compile-time max ring slots (array size on [`crate::game::pop_property::LivingStandardHistory`]).
    pub const HISTORY_MAX: usize = 16;

    // Living Standard Constants
    /// EMA blend for rolling average (higher = more weight on today).
    pub const ROLLING_AVG_WEIGHT: f64 = 0.25;
    /// Weight of basic tier sat in the composite living-standard score.
    pub const SCORE_WEIGHT_BASIC: f64 = 3.0;
    /// Weight of common-mood tier sat in the composite living-standard score.
    pub const SCORE_WEIGHT_COMMON: f64 = 1.5;
    /// Weight of luxury tier sat in the composite living-standard score.
    pub const SCORE_WEIGHT_LUXURY: f64 = 1.0;

    /// Rate at which anger sentiment is gained when 
    pub const ANGER_SENTIMENT_RATE: f64 = 0.08;

    /// Ignore |trend| below this when applying sentiment shifts.
    pub const SENTIMENT_TREND_DEADBAND: f64 = 0.05;
    /// Sentiment share gain scale when living standard is rising.
    pub const SENTIMENT_RISE_GAIN: f64 = 0.03;
    /// Sentiment share gain scale when living standard is falling (usually > rise).
    pub const SENTIMENT_FALL_GAIN: f64 = 0.05;

    /// Composite living-standard score from tier pieces prepared for mood
    /// (basic/luxury clamped, common via common-sat mood weight).
    pub fn score(basic: f64, common_mood: f64, luxury: f64) -> f64 {
        SCORE_WEIGHT_BASIC * basic
            + SCORE_WEIGHT_COMMON * common_mood
            + SCORE_WEIGHT_LUXURY * luxury
    }
}
