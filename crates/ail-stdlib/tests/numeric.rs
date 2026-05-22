// Tests for ail-stdlib::numeric — checked/wrapping/saturating arithmetic.
//
// TDD cycle: all tests written before implementation.
// Spec: G26 stdlib-impl, Requirements R1.1–R1.5.

use ail_stdlib::numeric::{checked_add, checked_mul, checked_sub, saturating_add, wrapping_add};

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
