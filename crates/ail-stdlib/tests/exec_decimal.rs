use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib};

fn decimal(mantissa: i64, scale: i64) -> StdlibValue {
    StdlibValue::Tuple(vec![StdlibValue::Int(mantissa), StdlibValue::Int(scale)])
}

fn ok_decimal(result: Result<StdlibValue, StdlibExecError>) -> StdlibValue {
    let Ok(StdlibValue::Result(Ok(value))) = result else {
        panic!("expected Result::Ok(decimal), got {result:?}");
    };
    *value
}

#[test]
fn decimal_from_int_returns_scale_zero_tuple() {
    let result = call_pure_stdlib("std.decimal.from_int", &[StdlibValue::Int(42)]);
    assert_eq!(result, Ok(decimal(42, 0)));
}

#[test]
fn decimal_add_returns_decimal_result() {
    let result = call_pure_stdlib("std.decimal.add", &[decimal(100, 2), decimal(50, 2)]);
    assert_eq!(ok_decimal(result), decimal(150, 2));
}

#[test]
fn decimal_sub_returns_decimal_result() {
    let result = call_pure_stdlib("std.decimal.sub", &[decimal(200, 2), decimal(75, 2)]);
    assert_eq!(ok_decimal(result), decimal(125, 2));
}

#[test]
fn decimal_mul_returns_decimal_result_with_combined_scale() {
    let result = call_pure_stdlib("std.decimal.mul", &[decimal(10, 1), decimal(20, 1)]);
    assert_eq!(ok_decimal(result), decimal(200, 2));
}

#[test]
fn decimal_rescale_returns_decimal_result() {
    let result = call_pure_stdlib("std.decimal.rescale", &[decimal(1, 0), StdlibValue::Int(2)]);
    assert_eq!(ok_decimal(result), decimal(100, 2));
}

#[test]
fn decimal_predicates_return_bool() {
    assert_eq!(
        call_pure_stdlib("std.decimal.is_negative", &[decimal(-1, 0)]),
        Ok(StdlibValue::Bool(true))
    );
    assert_eq!(
        call_pure_stdlib("std.decimal.is_negative", &[decimal(0, 0)]),
        Ok(StdlibValue::Bool(false))
    );
    assert_eq!(
        call_pure_stdlib("std.decimal.is_zero", &[decimal(0, 2)]),
        Ok(StdlibValue::Bool(true))
    );
    assert_eq!(
        call_pure_stdlib("std.decimal.is_zero", &[decimal(5, 2)]),
        Ok(StdlibValue::Bool(false))
    );
}

#[test]
fn decimal_non_negative_returns_ok_for_zero_and_positive() {
    let zero = call_pure_stdlib("std.decimal.non_negative", &[decimal(0, 0)]);
    assert_eq!(ok_decimal(zero), decimal(0, 0));

    let positive = call_pure_stdlib("std.decimal.non_negative", &[decimal(5, 2)]);
    assert_eq!(ok_decimal(positive), decimal(5, 2));
}

#[test]
fn decimal_non_negative_rejects_negative() {
    let result = call_pure_stdlib("std.decimal.non_negative", &[decimal(-1, 0)]);
    let Ok(StdlibValue::Result(Err(message))) = result else {
        panic!("negative decimal must return Result::Err, got {result:?}");
    };
    assert!(
        matches!(&*message, StdlibValue::Text(text) if text.contains("negative value")),
        "error must mention negative value, got {message:?}"
    );
}

#[test]
fn decimal_scale_mismatch_returns_err_text() {
    let result = call_pure_stdlib("std.decimal.add", &[decimal(100, 2), decimal(50, 1)]);
    let Ok(StdlibValue::Result(Err(message))) = result else {
        panic!("scale mismatch must return Result::Err, got {result:?}");
    };
    assert!(
        matches!(&*message, StdlibValue::Text(text) if text.contains("scale mismatch")),
        "error must mention scale mismatch, got {message:?}"
    );
}

#[test]
fn decimal_invalid_shape_returns_type_error() {
    let result = call_pure_stdlib("std.decimal.add", &[StdlibValue::Int(1), decimal(1, 0)]);
    assert_eq!(
        result,
        Err(StdlibExecError::Type {
            expected: "Decimal"
        })
    );
}
