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

#[cfg(test)]
mod util_should {
    use super::*;

    #[test]
    fn whole_units_drops_the_fraction_toward_zero() {
        assert_eq!(whole_units(4.7), 4.0);
        assert_eq!(whole_units(-4.7), -4.0);
        assert_eq!(whole_units(2.0), 2.0);
        assert_eq!(whole_units(0.9), 0.0);
    }

    #[test]
    fn whole_units_up_rounds_away_from_zero() {
        assert_eq!(whole_units_up(2.1), 3.0);
        assert_eq!(whole_units_up(-2.1), -3.0);
        assert_eq!(whole_units_up(2.0), 2.0);
        assert_eq!(whole_units_up(0.0), 0.0);
    }

    #[test]
    fn is_whole_unit_rejects_fractions() {
        assert!(is_whole_unit(3.0));
        assert!(is_whole_unit(-4.0));
        assert!(!is_whole_unit(2.5));
        assert!(!is_whole_unit(f64::NAN));
    }
}
