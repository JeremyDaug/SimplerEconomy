// TODO: Much of this is likely to be turned into a config file that is set on game load.

//# Game Universe units/scales.

/// The number of units of time in a single market day.
/// 
/// You can think of this as the number of 'shifts' or 'hours' in a day. 
pub const TIME_UNITS_PER_DAY: f64 = 24.0;

/// How many days are in a single turn (Market Day) of the game.
/// 
/// Acts as a multiplier so the length of a market day can be increased
pub const DAYS_PER_TURN: f64 = 1.0;

// NOTE: Fixed Good IDs.

/// ID for time good. Fixed in place as it's always going to be needed.
pub const TIME_ID: usize = 0;

// NOTE: Market Constants

/// The threshold of Salability which turns a good into a Money.
pub const MONEY_SALABILITY_THRESHOLD: f64 = 0.9;

// NOTE: Pop Constants

/// The minimum size a want can take in storage. Anything less than this should decay to 0.0.
/// 
/// This is to help keep want storage in pops down.
pub const MINIMUM_WANT_THRESHOLD: f64 = 0.001;

/// The Pop AMV Hard Loss Threshold, used in checking if an offer is valid. 
/// 
/// The AMV Gained should be greater than the Loss times this threshold.
pub const POP_AMV_HARD_LOSS_THRESHOLD: f64 = 0.25;

/// Per market day, how many days of labor they get.
pub const ADULT_LABOR_EFFICIENCY: f64 = 1.0;
/// Per market day, how many days of labor children are good for.
pub const CHILDREN_LABOR_EFFICIENCY: f64 = 0.3;
/// Per market day, how many days of labor Elders are good for.
pub const ELDER_LABOR_EFFICIENCY: f64 = 0.5;