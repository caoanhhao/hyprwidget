//! Numeric conversions the compiler cannot prove lossless, in one place so
//! each kind of lossy cast is justified once.

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "float-to-int `as` saturates and maps NaN to 0, the clamp we want"
)]
mod float_to_unsigned {
    pub(crate) fn f64_to_u8(x: f64) -> u8 {
        x as u8
    }

    pub(crate) fn f64_to_u64(x: f64) -> u64 {
        x as u64
    }

    pub(crate) fn f64_to_usize(x: f64) -> usize {
        x as usize
    }
}

pub(crate) use float_to_unsigned::{f64_to_u8, f64_to_u64, f64_to_usize};

/// Saturates at the bounds of `i64`; NaN becomes 0.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "float-to-int `as` saturates and maps NaN to 0, the clamp we want"
)]
pub(crate) fn f64_to_i64(x: f64) -> i64 {
    x as i64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "only values above 2^53 round, far beyond the counters and \
              timestamps converted here"
)]
mod int_to_float {
    pub(crate) fn i64_to_f64(x: i64) -> f64 {
        x as f64
    }

    pub(crate) fn u64_to_f64(x: u64) -> f64 {
        x as f64
    }

    pub(crate) fn usize_to_f64(x: usize) -> f64 {
        x as f64
    }
}

pub(crate) use int_to_float::{i64_to_f64, u64_to_f64, usize_to_f64};
