use rand::RngCore;

/// Index into a non-empty list. `len` of 0 returns 0.
pub fn random_index(rng: &mut dyn RngCore, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (rng.next_u64() as usize) % len
    }
}

/// Linearly interpolate between `a` and `b` by `t`.
/// `a` is the value at `t=0`, and `b` is the value at `t=1`.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
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