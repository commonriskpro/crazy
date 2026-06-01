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

#[test]
fn wrapping_sub_wraps_at_min() {
    let result = call_pure_stdlib(
        "std.numeric.wrapping_sub",
        &[StdlibValue::Int(i64::MIN), StdlibValue::Int(1)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(i64::MAX)));
}

#[test]
fn wrapping_mul_wraps_on_overflow() {
    let result = call_pure_stdlib(
        "std.numeric.wrapping_mul",
        &[StdlibValue::Int(i64::MAX), StdlibValue::Int(2)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(-2)));
}

#[test]
fn wrapping_neg_wraps_min_to_min() {
    let result = call_pure_stdlib("std.numeric.wrapping_neg", &[StdlibValue::Int(i64::MIN)]);
    assert_eq!(result, Ok(StdlibValue::Int(i64::MIN)));
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

#[test]
fn saturating_sub_clamps_at_min() {
    let result = call_pure_stdlib(
        "std.numeric.saturating_sub",
        &[StdlibValue::Int(i64::MIN), StdlibValue::Int(1)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(i64::MIN)));
}

#[test]
fn saturating_mul_clamps_at_max() {
    let result = call_pure_stdlib(
        "std.numeric.saturating_mul",
        &[StdlibValue::Int(i64::MAX), StdlibValue::Int(2)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(i64::MAX)));
}

#[test]
fn saturating_neg_min_clamps_to_max() {
    let result = call_pure_stdlib("std.numeric.saturating_neg", &[StdlibValue::Int(i64::MIN)]);
    assert_eq!(result, Ok(StdlibValue::Int(i64::MAX)));
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

// ── STDLIB-EXEC-NUM-7: bounds helpers match source/compiler helpers ──────

#[test]
fn min_returns_smaller_signed_int() {
    let result = call_pure_stdlib(
        "std.numeric.min",
        &[StdlibValue::Int(10), StdlibValue::Int(-2)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(-2)));
}

#[test]
fn max_returns_larger_signed_int() {
    let result = call_pure_stdlib(
        "std.numeric.max",
        &[StdlibValue::Int(10), StdlibValue::Int(-2)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(10)));
}

#[test]
fn clamp_returns_low_when_value_is_below_bounds() {
    let result = call_pure_stdlib(
        "std.numeric.clamp",
        &[
            StdlibValue::Int(-5),
            StdlibValue::Int(0),
            StdlibValue::Int(10),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(0)));
}

#[test]
fn clamp_returns_high_when_value_is_above_bounds() {
    let result = call_pure_stdlib(
        "std.numeric.clamp",
        &[
            StdlibValue::Int(15),
            StdlibValue::Int(0),
            StdlibValue::Int(10),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(10)));
}

#[test]
fn clamp_returns_value_when_value_is_inside_bounds() {
    let result = call_pure_stdlib(
        "std.numeric.clamp",
        &[
            StdlibValue::Int(7),
            StdlibValue::Int(0),
            StdlibValue::Int(10),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(7)));
}

#[test]
fn clamp_type_error_returns_err() {
    let result = call_pure_stdlib(
        "std.numeric.clamp",
        &[
            StdlibValue::Text("x".to_string()),
            StdlibValue::Int(0),
            StdlibValue::Int(10),
        ],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Int" }));
}

#[test]
fn fallback_helpers_return_operation_or_fallback() {
    for (id, args, expected) in [
        (
            "std.numeric.abs_or",
            vec![StdlibValue::Int(-7), StdlibValue::Int(99)],
            7,
        ),
        (
            "std.numeric.abs_or",
            vec![StdlibValue::Int(i64::MIN), StdlibValue::Int(99)],
            99,
        ),
        (
            "std.numeric.neg_or",
            vec![StdlibValue::Int(-5), StdlibValue::Int(99)],
            5,
        ),
        (
            "std.numeric.neg_or",
            vec![StdlibValue::Int(i64::MIN), StdlibValue::Int(99)],
            99,
        ),
        (
            "std.numeric.add_or",
            vec![
                StdlibValue::Int(i64::MAX),
                StdlibValue::Int(1),
                StdlibValue::Int(19),
            ],
            19,
        ),
        (
            "std.numeric.sub_or",
            vec![
                StdlibValue::Int(i64::MIN),
                StdlibValue::Int(1),
                StdlibValue::Int(23),
            ],
            23,
        ),
        (
            "std.numeric.mul_or",
            vec![
                StdlibValue::Int(i64::MAX),
                StdlibValue::Int(2),
                StdlibValue::Int(29),
            ],
            29,
        ),
        (
            "std.numeric.div_or",
            vec![
                StdlibValue::Int(1),
                StdlibValue::Int(0),
                StdlibValue::Int(5),
            ],
            5,
        ),
        (
            "std.numeric.rem_or",
            vec![
                StdlibValue::Int(i64::MIN),
                StdlibValue::Int(-1),
                StdlibValue::Int(13),
            ],
            13,
        ),
    ] {
        assert_eq!(call_pure_stdlib(id, &args), Ok(StdlibValue::Int(expected)));
    }
}

#[test]
fn bit_and_shift_helpers_match_compiler_semantics() {
    for (id, args, expected) in [
        (
            "std.numeric.bit_and",
            vec![StdlibValue::Int(6), StdlibValue::Int(3)],
            2,
        ),
        (
            "std.numeric.bit_or",
            vec![StdlibValue::Int(8), StdlibValue::Int(3)],
            11,
        ),
        (
            "std.numeric.bit_xor",
            vec![StdlibValue::Int(-1), StdlibValue::Int(42)],
            -43,
        ),
        ("std.numeric.bit_not", vec![StdlibValue::Int(0)], -1),
        (
            "std.numeric.shift_left",
            vec![StdlibValue::Int(-1), StdlibValue::Int(1)],
            -2,
        ),
        (
            "std.numeric.shift_right",
            vec![StdlibValue::Int(-8), StdlibValue::Int(1)],
            -4,
        ),
        (
            "std.numeric.shift_right_unsigned",
            vec![StdlibValue::Int(-8), StdlibValue::Int(1)],
            9223372036854775804,
        ),
    ] {
        assert_eq!(call_pure_stdlib(id, &args), Ok(StdlibValue::Int(expected)));
    }
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

// ── STDLIB-EXEC-NUM-8: narrow_to_i32 value fits → Ok ─────────────────────

#[test]
fn narrow_to_i32_in_range_returns_ok() {
    let result = call_pure_stdlib(
        "std.numeric.narrow_to_i32",
        &[StdlibValue::Int(i32::MAX as i64)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(
            i32::MAX as i64
        )))))
    );
}

// STDLIB-EXEC-NUM-9: narrow_to_i32 overflow → Err

#[test]
fn narrow_to_i32_overflow_returns_err() {
    let result = call_pure_stdlib("std.numeric.narrow_to_i32", &[StdlibValue::Int(i64::MAX)]);
    assert!(
        matches!(result, Ok(StdlibValue::Result(Err(_)))),
        "narrow_to_i32(i64::MAX) must return Err variant, got: {result:?}"
    );
}

// Triangulate: negative value out of i32 range → Err

#[test]
fn narrow_to_i32_underflow_returns_err() {
    let result = call_pure_stdlib("std.numeric.narrow_to_i32", &[StdlibValue::Int(i64::MIN)]);
    assert!(
        matches!(result, Ok(StdlibValue::Result(Err(_)))),
        "narrow_to_i32(i64::MIN) must return Err variant, got: {result:?}"
    );
}

// ── STDLIB-EXEC-NUM-10: narrow_to_u32 value fits → Ok ────────────────────

#[test]
fn narrow_to_u32_in_range_returns_ok() {
    let result = call_pure_stdlib(
        "std.numeric.narrow_to_u32",
        &[StdlibValue::Int(u32::MAX as i64)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(
            u32::MAX as i64
        )))))
    );
}

// STDLIB-EXEC-NUM-11: narrow_to_u32 negative → Err (no implicit coercion)

#[test]
fn narrow_to_u32_negative_returns_err() {
    let result = call_pure_stdlib("std.numeric.narrow_to_u32", &[StdlibValue::Int(-1)]);
    assert!(
        matches!(result, Ok(StdlibValue::Result(Err(_)))),
        "narrow_to_u32(-1) must return Err variant, got: {result:?}"
    );
}

#[test]
fn narrow_to_u64_in_range_returns_ok() {
    let result = call_pure_stdlib("std.numeric.narrow_to_u64", &[StdlibValue::Int(i64::MAX)]);
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(
            i64::MAX
        )))))
    );
}

#[test]
fn narrow_to_u64_negative_returns_err() {
    let result = call_pure_stdlib("std.numeric.narrow_to_u64", &[StdlibValue::Int(-1)]);
    assert!(
        matches!(result, Ok(StdlibValue::Result(Err(_)))),
        "narrow_to_u64(-1) must return Err variant, got: {result:?}"
    );
}

#[test]
fn narrow_to_i16_bounds_return_ok_or_err() {
    let min = call_pure_stdlib(
        "std.numeric.narrow_to_i16",
        &[StdlibValue::Int(i16::MIN as i64)],
    );
    assert_eq!(
        min,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(
            i16::MIN as i64
        )))))
    );

    let overflow = call_pure_stdlib(
        "std.numeric.narrow_to_i16",
        &[StdlibValue::Int(i16::MAX as i64 + 1)],
    );
    assert!(
        matches!(overflow, Ok(StdlibValue::Result(Err(_)))),
        "narrow_to_i16(i16::MAX + 1) must return Err variant, got: {overflow:?}"
    );
}

#[test]
fn narrow_to_u8_bounds_return_ok_or_err() {
    let max = call_pure_stdlib(
        "std.numeric.narrow_to_u8",
        &[StdlibValue::Int(u8::MAX as i64)],
    );
    assert_eq!(
        max,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(
            u8::MAX as i64
        )))))
    );

    let negative = call_pure_stdlib("std.numeric.narrow_to_u8", &[StdlibValue::Int(-1)]);
    assert!(
        matches!(negative, Ok(StdlibValue::Result(Err(_)))),
        "narrow_to_u8(-1) must return Err variant, got: {negative:?}"
    );
}

// STDLIB-EXEC-NUM-12: narrow_to_u32 arity error

#[test]
fn narrow_to_u32_arity_error() {
    let result = call_pure_stdlib("std.numeric.narrow_to_u32", &[]);
    assert_eq!(
        result,
        Err(StdlibExecError::Arity {
            expected: 1,
            actual: 0
        })
    );
}
