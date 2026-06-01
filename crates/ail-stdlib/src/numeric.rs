// ── ail-stdlib::numeric ───────────────────────────────────────────────────
//
// Checked, wrapping, saturating arithmetic and narrowing conversions for the
// AIL `std.numeric` module.
//
// # Contracts (from docs/stdlib.md)
//
// - no silent overflow
// - no silent narrowing: narrowing conversions return `Result`
// - rounding explicit when needed

// ── Checked arithmetic ────────────────────────────────────────────────────

/// Return the smaller of two `i64` values.
pub fn min(a: i64, b: i64) -> i64 {
    a.min(b)
}

/// Return the larger of two `i64` values.
pub fn max(a: i64, b: i64) -> i64 {
    a.max(b)
}

/// Clamp an `i64` value between low and high bounds.
///
/// This intentionally mirrors the existing AIL `int_clamp` compiler helper:
/// first return `low` when `value < low`, otherwise return `high` when
/// `value > high`, otherwise return `value`.
pub fn clamp(value: i64, low: i64, high: i64) -> i64 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// Add two `i64` values, returning `None` on overflow.
pub fn checked_add(a: i64, b: i64) -> Option<i64> {
    a.checked_add(b)
}

/// Subtract `b` from `a`, returning `None` on underflow or overflow.
pub fn checked_sub(a: i64, b: i64) -> Option<i64> {
    a.checked_sub(b)
}

/// Multiply two `i64` values, returning `None` on overflow.
pub fn checked_mul(a: i64, b: i64) -> Option<i64> {
    a.checked_mul(b)
}

/// Return `abs(value)`, or `fallback` when `value == i64::MIN`.
pub fn abs_or(value: i64, fallback: i64) -> i64 {
    value.checked_abs().unwrap_or(fallback)
}

/// Return `-value`, or `fallback` when `value == i64::MIN`.
pub fn neg_or(value: i64, fallback: i64) -> i64 {
    value.checked_neg().unwrap_or(fallback)
}

/// Add two `i64` values, returning `fallback` on overflow.
pub fn add_or(a: i64, b: i64, fallback: i64) -> i64 {
    a.checked_add(b).unwrap_or(fallback)
}

/// Subtract two `i64` values, returning `fallback` on underflow or overflow.
pub fn sub_or(a: i64, b: i64, fallback: i64) -> i64 {
    a.checked_sub(b).unwrap_or(fallback)
}

/// Multiply two `i64` values, returning `fallback` on overflow.
pub fn mul_or(a: i64, b: i64, fallback: i64) -> i64 {
    a.checked_mul(b).unwrap_or(fallback)
}

/// Divide two `i64` values, returning `fallback` on divide-by-zero or overflow.
pub fn div_or(value: i64, divisor: i64, fallback: i64) -> i64 {
    value.checked_div(divisor).unwrap_or(fallback)
}

/// Remainder of two `i64` values, returning `fallback` on divide-by-zero or overflow.
pub fn rem_or(value: i64, divisor: i64, fallback: i64) -> i64 {
    value.checked_rem(divisor).unwrap_or(fallback)
}

// ── Wrapping arithmetic ───────────────────────────────────────────────────

/// Add two `i64` values with defined two's-complement wrapping.
///
/// Wrapping behavior is explicit: `i64::MAX + 1 == i64::MIN`.
pub fn wrapping_add(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

/// Subtract two `i64` values with defined two's-complement wrapping.
pub fn wrapping_sub(a: i64, b: i64) -> i64 {
    a.wrapping_sub(b)
}

/// Multiply two `i64` values with defined two's-complement wrapping.
pub fn wrapping_mul(a: i64, b: i64) -> i64 {
    a.wrapping_mul(b)
}

/// Negate an `i64` value with defined two's-complement wrapping.
pub fn wrapping_neg(value: i64) -> i64 {
    value.wrapping_neg()
}

// ── Saturating arithmetic ─────────────────────────────────────────────────

/// Add two `i64` values, clamping to `i64::MAX` or `i64::MIN` on overflow.
pub fn saturating_add(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}

/// Subtract two `i64` values, clamping to `i64::MAX` or `i64::MIN` on overflow.
pub fn saturating_sub(a: i64, b: i64) -> i64 {
    a.saturating_sub(b)
}

/// Multiply two `i64` values, clamping to `i64::MAX` or `i64::MIN` on overflow.
pub fn saturating_mul(a: i64, b: i64) -> i64 {
    a.saturating_mul(b)
}

/// Negate an `i64` value, clamping `i64::MIN` to `i64::MAX`.
pub fn saturating_neg(value: i64) -> i64 {
    value.saturating_neg()
}

// ── Narrowing conversions returning Result ────────────────────────────────

/// Narrowing conversion error: value out of target range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NarrowError {
    pub value: i64,
    pub target: &'static str,
}

impl std::fmt::Display for NarrowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "value {} out of range for {}", self.value, self.target)
    }
}

impl std::error::Error for NarrowError {}

/// Narrow an `i64` to `i32`, returning `Err` if the value does not fit.
pub fn narrow_i64_to_i32(v: i64) -> Result<i32, NarrowError> {
    i32::try_from(v).map_err(|_| NarrowError {
        value: v,
        target: "i32",
    })
}

/// Narrow an `i64` to `u64`, returning `Err` if the value is negative.
pub fn narrow_i64_to_u64(v: i64) -> Result<u64, NarrowError> {
    u64::try_from(v).map_err(|_| NarrowError {
        value: v,
        target: "u64",
    })
}

/// Narrow an `i64` to `u32`, returning `Err` if the value does not fit.
pub fn narrow_i64_to_u32(v: i64) -> Result<u32, NarrowError> {
    u32::try_from(v).map_err(|_| NarrowError {
        value: v,
        target: "u32",
    })
}

/// Narrow an `i64` to `i16`, returning `Err` if the value does not fit.
pub fn narrow_i64_to_i16(v: i64) -> Result<i16, NarrowError> {
    i16::try_from(v).map_err(|_| NarrowError {
        value: v,
        target: "i16",
    })
}

/// Narrow an `i64` to `u8`, returning `Err` if the value does not fit.
pub fn narrow_i64_to_u8(v: i64) -> Result<u8, NarrowError> {
    u8::try_from(v).map_err(|_| NarrowError {
        value: v,
        target: "u8",
    })
}

// ── Rounding policies ─────────────────────────────────────────────────────

/// Rounding policy for floating-point to integer conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingPolicy {
    /// Round toward zero (truncate).
    Truncate,
    /// Round toward positive infinity (ceiling).
    Ceiling,
    /// Round toward negative infinity (floor).
    Floor,
    /// Round to nearest, with half rounding away from zero.
    HalfAwayFromZero,
    /// Round to nearest, with half rounding toward even (banker's rounding).
    HalfToEven,
}

/// Round an `f64` to `i64` using the given policy.
///
/// Returns `None` if the result does not fit in `i64` or if the input is NaN
/// or infinite — no silent overflow.
pub fn round_f64_to_i64(v: f64, policy: RoundingPolicy) -> Option<i64> {
    if !v.is_finite() {
        return None;
    }
    let rounded = match policy {
        RoundingPolicy::Truncate => v.trunc(),
        RoundingPolicy::Ceiling => v.ceil(),
        RoundingPolicy::Floor => v.floor(),
        RoundingPolicy::HalfAwayFromZero => {
            if v >= 0.0 {
                (v + 0.5).floor()
            } else {
                (v - 0.5).ceil()
            }
        }
        RoundingPolicy::HalfToEven => {
            let floored = v.floor();
            let diff = v - floored;
            if (diff - 0.5).abs() < f64::EPSILON {
                // exactly 0.5 — round to even
                if floored as i64 % 2 == 0 {
                    floored
                } else {
                    floored + 1.0
                }
            } else {
                v.round()
            }
        }
    };
    if rounded >= i64::MIN as f64 && rounded <= i64::MAX as f64 {
        Some(rounded as i64)
    } else {
        None
    }
}
