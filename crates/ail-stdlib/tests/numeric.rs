// Tests for ail-stdlib::numeric — checked/wrapping/saturating arithmetic.
//
// TDD cycle: all tests written before implementation.
// Spec: G26 stdlib-impl, Requirements R1.1–R1.5.

use ail_stdlib::numeric::{
    abs_or, add_or, bit_and, bit_not, bit_or, bit_xor, checked_add, checked_mul, checked_sub,
    clamp, div_or, max, min, mul_or, neg_or, rem_or, saturating_add, saturating_mul,
    saturating_neg, saturating_sub, shift_left, shift_right, shift_right_unsigned, sub_or,
    wrapping_add, wrapping_mul, wrapping_neg, wrapping_sub,
};

// ── Bounds helpers ───────────────────────────────────────────────────────

#[test]
fn min_returns_smaller_signed_int() {
    assert_eq!(min(10, -2), -2);
    assert_eq!(min(-5, -8), -8);
}

#[test]
fn max_returns_larger_signed_int() {
    assert_eq!(max(10, -2), 10);
    assert_eq!(max(-5, -8), -5);
}

#[test]
fn clamp_bounds_signed_int() {
    assert_eq!(clamp(-5, 0, 10), 0);
    assert_eq!(clamp(15, 0, 10), 10);
    assert_eq!(clamp(7, 0, 10), 7);
}

#[test]
fn fallback_helpers_return_operation_or_fallback() {
    assert_eq!(abs_or(-7, 99), 7);
    assert_eq!(abs_or(i64::MIN, 99), 99);
    assert_eq!(neg_or(-5, 99), 5);
    assert_eq!(neg_or(i64::MIN, 99), 99);
    assert_eq!(add_or(40, 2, -1), 42);
    assert_eq!(add_or(i64::MAX, 1, 19), 19);
    assert_eq!(sub_or(50, 8, -1), 42);
    assert_eq!(sub_or(i64::MIN, 1, 23), 23);
    assert_eq!(mul_or(6, 7, -1), 42);
    assert_eq!(mul_or(i64::MAX, 2, 29), 29);
    assert_eq!(div_or(21, 3, -1), 7);
    assert_eq!(div_or(1, 0, 5), 5);
    assert_eq!(div_or(i64::MIN, -1, 11), 11);
    assert_eq!(rem_or(22, 5, -1), 2);
    assert_eq!(rem_or(1, 0, 6), 6);
    assert_eq!(rem_or(i64::MIN, -1, 13), 13);
}

#[test]
fn bit_and_shift_helpers_match_compiler_semantics() {
    assert_eq!(bit_and(6, 3), 2);
    assert_eq!(bit_and(-1, 42), 42);
    assert_eq!(bit_or(4, 1), 5);
    assert_eq!(bit_or(8, 3), 11);
    assert_eq!(bit_xor(6, 3), 5);
    assert_eq!(bit_xor(-1, 42), -43);
    assert_eq!(bit_not(0), -1);
    assert_eq!(bit_not(-1), 0);
    assert_eq!(shift_left(1, 3), 8);
    assert_eq!(shift_left(-1, 1), -2);
    assert_eq!(shift_right(16, 1), 8);
    assert_eq!(shift_right(-8, 1), -4);
    assert_eq!(shift_right_unsigned(16, 1), 8);
    assert_eq!(shift_right_unsigned(-8, 1), 9223372036854775804);
}

// ── R1.1 + R1.2: checked_add ──────────────────────────────────────────────

// S1.2: normal addition succeeds
#[test]
fn checked_add_normal() {
    assert_eq!(checked_add(1, 2), Some(3));
    assert_eq!(checked_add(0, 0), Some(0));
    assert_eq!(checked_add(-5, 3), Some(-2));
}

// S1.1: overflow returns None
#[test]
fn checked_add_overflow_returns_none() {
    assert_eq!(checked_add(i64::MAX, 1), None);
    assert_eq!(checked_add(i64::MAX, i64::MAX), None);
}

// Triangulate: negative underflow
#[test]
fn checked_add_underflow_returns_none() {
    assert_eq!(checked_add(i64::MIN, -1), None);
}

// ── R1.2: wrapping_add ────────────────────────────────────────────────────

// S1.3: wraps to MIN on MAX + 1
#[test]
fn wrapping_add_overflow_wraps() {
    assert_eq!(wrapping_add(i64::MAX, 1), i64::MIN);
}

#[test]
fn wrapping_add_normal() {
    assert_eq!(wrapping_add(1, 2), 3);
    assert_eq!(wrapping_add(-1, 1), 0);
}

#[test]
fn wrapping_sub_overflow_wraps() {
    assert_eq!(wrapping_sub(i64::MIN, 1), i64::MAX);
}

#[test]
fn wrapping_mul_overflow_wraps() {
    assert_eq!(wrapping_mul(i64::MAX, 2), -2);
}

#[test]
fn wrapping_neg_min_wraps_to_min() {
    assert_eq!(wrapping_neg(i64::MIN), i64::MIN);
    assert_eq!(wrapping_neg(5), -5);
}

// ── R1.3: saturating_add ─────────────────────────────────────────────────

// S1.4: clamps to MAX on overflow
#[test]
fn saturating_add_clamps_to_max() {
    assert_eq!(saturating_add(i64::MAX, 1), i64::MAX);
    assert_eq!(saturating_add(i64::MAX, 100), i64::MAX);
}

#[test]
fn saturating_add_clamps_to_min() {
    assert_eq!(saturating_add(i64::MIN, -1), i64::MIN);
}

#[test]
fn saturating_add_normal() {
    assert_eq!(saturating_add(10, 20), 30);
}

#[test]
fn saturating_sub_clamps_to_min() {
    assert_eq!(saturating_sub(i64::MIN, 1), i64::MIN);
}

#[test]
fn saturating_mul_clamps_to_max() {
    assert_eq!(saturating_mul(i64::MAX, 2), i64::MAX);
}

#[test]
fn saturating_neg_min_clamps_to_max() {
    assert_eq!(saturating_neg(i64::MIN), i64::MAX);
    assert_eq!(saturating_neg(5), -5);
}

// ── R1.4: checked_sub ────────────────────────────────────────────────────

// S1.5: underflow returns None
#[test]
fn checked_sub_underflow_returns_none() {
    assert_eq!(checked_sub(i64::MIN, 1), None);
}

#[test]
fn checked_sub_normal() {
    assert_eq!(checked_sub(5, 3), Some(2));
    assert_eq!(checked_sub(0, 0), Some(0));
}

#[test]
fn checked_sub_overflow_returns_none() {
    // i64::MAX - (-1) overflows
    assert_eq!(checked_sub(i64::MAX, -1), None);
}

// ── R1.5: checked_mul ────────────────────────────────────────────────────

// S1.6: overflow returns None
#[test]
fn checked_mul_overflow_returns_none() {
    assert_eq!(checked_mul(i64::MAX, 2), None);
    assert_eq!(checked_mul(i64::MIN, 2), None);
}

#[test]
fn checked_mul_normal() {
    assert_eq!(checked_mul(3, 4), Some(12));
    assert_eq!(checked_mul(-3, 4), Some(-12));
    assert_eq!(checked_mul(0, i64::MAX), Some(0));
}
