//! Numeric conversions between Lua numbers (`f64`) and the integer and
//! `f32` parameters of GTK/VTE.

/// Clamps a negative value to 0.
pub(crate) fn non_negative(i: i32) -> u32 {
    u32::try_from(i).unwrap_or(0)
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "`as` saturates out-of-range values and maps NaN to 0, the wanted clamp"
)]
pub(crate) fn saturating_i32(f: f64) -> i32 {
    f as i32
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "`as` saturates out-of-range values and maps NaN to 0, the wanted clamp"
)]
pub(crate) fn saturating_i64(f: f64) -> i64 {
    f as i64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "`as` saturates out-of-range values and maps NaN and negatives to 0, the wanted clamp"
)]
pub(crate) fn saturating_u64(f: f64) -> u64 {
    f as u64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "GTK takes `f32` alignments and scales; the lost precision is invisible"
)]
pub(crate) fn to_f32(f: f64) -> f32 {
    f as f32
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "Lua numbers are `f64`; integers beyond 2^53 round, as in Lua itself"
)]
pub(crate) fn i64_to_f64(i: i64) -> f64 {
    i as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "Lua numbers are `f64`; integers beyond 2^53 round, as in Lua itself"
)]
pub(crate) fn u64_to_f64(u: u64) -> f64 {
    u as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps() {
        assert_eq!(non_negative(-3), 0);
        assert_eq!(non_negative(7), 7);
        assert_eq!(saturating_i32(1e20), i32::MAX);
        assert_eq!(saturating_i32(f64::NAN), 0);
        assert_eq!(saturating_u64(-1.0), 0);
    }
}
