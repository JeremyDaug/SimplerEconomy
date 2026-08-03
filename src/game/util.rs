/// Linearly interpolate between `a` and `b` by `t`.
/// `a` is the value at `t=0`, and `b` is the value at `t=1`.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}