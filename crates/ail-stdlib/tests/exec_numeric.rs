// Tests for numeric overflow exec entries via call_pure_stdlib.
//
// TDD: these tests are written BEFORE the implementation (T7).
// They will fail with UnknownFunction until T7 is applied.
// Spec: STDLIB-EXEC-NUM-1..7

use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib};

// ── STDLIB-EXEC-NUM-1: checked_add overflow returns Option(None) ──────────

#[test]
fn checked_add_overflow_returns_option_none() {
    let result = call_pure_stdlib(
        "std.numeric.checked_add",
        &[StdlibValue::Int(i64::MAX), StdlibValue::Int(1)],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// ── STDLIB-EXEC-NUM-2: checked_add normal returns Option(Some(value)) ─────

#[test]
fn checked_add_normal_returns_option_some() {
    let result = call_pure_stdlib(
        "std.numeric.checked_add",
        &[StdlibValue::Int(10), StdlibValue::Int(20)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(30)))))
    );
}

// ── STDLIB-EXEC-NUM-3: wrapping_add wraps at MAX ──────────────────────────

#[test]
fn wrapping_add_wraps_at_max() {
    let result = call_pure_stdlib(
        "std.numeric.wrapping_add",
        &[StdlibValue::Int(i64::MAX), StdlibValue::Int(1)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(i64::MIN)));
}

// Triangulate: wrapping_add normal case
#[test]
fn wrapping_add_normal_no_wrap() {
    let result = call_pure_stdlib(
        "std.numeric.wrapping_add",
        &[StdlibValue::Int(5), StdlibValue::Int(3)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(8)));
}

// ── STDLIB-EXEC-NUM-4: saturating_add clamps at MAX ──────────────────────

#[test]
fn saturating_add_clamps_at_max() {
    let result = call_pure_stdlib(
        "std.numeric.saturating_add",
        &[StdlibValue::Int(i64::MAX), StdlibValue::Int(1)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(i64::MAX)));
}

// Triangulate: saturating_add normal case
#[test]
fn saturating_add_normal_no_clamp() {
    let result = call_pure_stdlib(
        "std.numeric.saturating_add",
        &[StdlibValue::Int(100), StdlibValue::Int(200)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(300)));
}

// ── STDLIB-EXEC-NUM-5: checked_sub underflow returns Option(None) ─────────

#[test]
fn checked_sub_underflow_returns_option_none() {
    let result = call_pure_stdlib(
        "std.numeric.checked_sub",
        &[StdlibValue::Int(i64::MIN), StdlibValue::Int(1)],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// Triangulate: checked_sub normal case
#[test]
fn checked_sub_normal_returns_option_some() {
    let result = call_pure_stdlib(
        "std.numeric.checked_sub",
        &[StdlibValue::Int(10), StdlibValue::Int(3)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(7)))))
    );
}

// ── STDLIB-EXEC-NUM-6: checked_mul overflow returns Option(None) ──────────

#[test]
fn checked_mul_overflow_returns_option_none() {
    let result = call_pure_stdlib(
        "std.numeric.checked_mul",
        &[StdlibValue::Int(i64::MAX), StdlibValue::Int(2)],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// Triangulate: checked_mul normal case
#[test]
fn checked_mul_normal_returns_option_some() {
    let result = call_pure_stdlib(
        "std.numeric.checked_mul",
        &[StdlibValue::Int(6), StdlibValue::Int(7)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(42)))))
    );
}

// ── STDLIB-EXEC-NUM-7: type error returns Err(Type) ───────────────────────

#[test]
fn checked_add_type_error_returns_err() {
    let result = call_pure_stdlib(
        "std.numeric.checked_add",
        &[StdlibValue::Text("x".to_string()), StdlibValue::Int(1)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Int" }));
}
