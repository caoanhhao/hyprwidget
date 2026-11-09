//! Lossy numeric conversions that are intended: pixel coordinates,
//! graphene floats and wrapping protocol timestamps.

/// Logical pixel coordinate → integer pixel (truncates toward zero,
/// saturates at the `i32` range, NaN becomes 0).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "truncating a pixel coordinate is the intent; `as` saturates"
)]
pub(crate) fn px(v: f64) -> i32 {
    v as i32
}

/// `f64` → `f32` for graphene / GSK, which only take single precision.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "GSK geometry is f32; the precision loss is irrelevant for pixels"
)]
pub(crate) fn gsk(v: f64) -> f32 {
    v as f32
}

/// Monotonic µs → wrapping millisecond timestamp (`wl_callback.done`,
/// input events), which the protocol defines as a `u32` that wraps.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Wayland timestamps are u32 milliseconds that wrap around"
)]
pub(crate) fn time_ms(us: i64) -> u32 {
    (us / 1000) as u32
}

/// Count or index → `i32`, saturating (never reached for real sizes).
pub(crate) fn sat_i32(v: usize) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

/// Count or index → `u32`, saturating (never reached for real sizes).
pub(crate) fn sat_u32(v: usize) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}
