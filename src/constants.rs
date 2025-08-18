/// ID for time good. Fixed in place as it's always going to be needed.
pub const TIME_ID: usize = 0;




/// The minimum size a want can take in storage. Anything less than this should decay to 0.0.
/// 
/// This is to help keep want storage in pops down.
pub const MINIMUM_WANT_THRESHOLD: f64 = 0.001;

/// The Pop AMV Hard Loss Threshold, used in checking if an offer is valid. 
/// 
/// The AMV Gained should be greater than the Loss times this threshold.
pub const POP_AMV_HARD_LOSS_THRESHOLD: f64 = 0.25;