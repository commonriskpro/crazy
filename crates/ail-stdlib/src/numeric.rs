// ── ail-stdlib::numeric ───────────────────────────────────────────────────
//
// Checked, wrapping, and saturating arithmetic for the AIL `std.numeric`
// module.  All operations are defined on i64 (the canonical AIL integer type).
//
// # Contracts (from docs/stdlib.md)
//
// - no silent overflow: callers receive an explicit signal (None / wrap / clamp)
// - no silent narrowing: narrowing conversions are out of scope for this module
// - rounding explicit when needed: not applicable to integer ops
//
// # No dependencies
//
// This module uses only Rust primitives. No additional crates are required.

// ── Checked arithmetic ────────────────────────────────────────────────────

/// Add two `i64` values, returning `None` on overflow.
///
/// Embodies the `std.numeric` contract: "no silent overflow."
/// Callers must handle the `None` case explicitly.
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

// ── Wrapping arithmetic ───────────────────────────────────────────────────

/// Add two `i64` values with defined two's-complement wrapping.
///
/// Unlike unchecked addition, the wrapping behavior is explicit and
/// documented: `i64::MAX + 1 == i64::MIN`.  This is NOT silent overflow —
/// the caller chose wrapping semantics.
pub fn wrapping_add(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

// ── Saturating arithmetic ─────────────────────────────────────────────────

/// Add two `i64` values, clamping to `i64::MAX` or `i64::MIN` on overflow.
///
/// Result is always in the valid `i64` range; the saturation boundary is
/// explicit, not silent.
pub fn saturating_add(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}
