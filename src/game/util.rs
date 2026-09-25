/// Linearly interpolate between `a` and `b` by `t`.
/// `a` is the value at `t=0`, and `b` is the value at `t=1`.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Drops the fractional part toward zero (`4.7` -> `4`, `-4.7` -> `-4`).
pub fn whole_units(qty: f64) -> f64 {
    qty.trunc()
}

/// Rounds away from zero to the next whole unit (`2.1` -> `3`, `-2.1` -> `-3`).
/// Whole values are unchanged.
pub fn whole_units_up(qty: f64) -> f64 {
    if qty > 0.0 {
        qty.ceil()
    } else if qty < 0.0 {
        qty.floor()
    } else {
        0.0
    }
}

/// Returns true when `qty` is a finite whole number.
pub fn is_whole_unit(qty: f64) -> bool {
    qty.is_finite() && qty == qty.trunc()
}

/// Rounds a non-negative unit count half-up to a whole number (`4.5` -> `5`).
pub fn round_units(amount: f64) -> f64 {
    debug_assert!(amount >= 0.0, "amount must be >= 0.0");
    (amount + 0.5).floor()
}