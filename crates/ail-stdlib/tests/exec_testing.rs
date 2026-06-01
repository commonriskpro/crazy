use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib};

#[test]
fn assert_eq_pass_returns_ok_unit() {
    let result = call_pure_stdlib(
        "std.testing.assert_eq",
        &[
            StdlibValue::Text("same".to_string()),
            StdlibValue::Text("same".to_string()),
            StdlibValue::Text("string equality".to_string()),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Unit))))
    );
}

#[test]
fn assert_eq_fail_returns_err_message() {
    let result = call_pure_stdlib(
        "std.testing.assert_eq",
        &[
            StdlibValue::Int(1),
            StdlibValue::Int(2),
            StdlibValue::Text("integer equality".to_string()),
        ],
    );
    let Ok(StdlibValue::Result(Err(message))) = result else {
        panic!("assert_eq failure must return Result::Err, got {result:?}");
    };
    assert!(
        matches!(&*message, StdlibValue::Text(text) if text.contains("integer equality")),
        "failure message must preserve assertion context, got {message:?}"
    );
}

#[test]
fn assert_approx_pass_returns_ok_unit() {
    let result = call_pure_stdlib(
        "std.testing.assert_approx",
        &[
            StdlibValue::Float(1.0),
            StdlibValue::Float(1.0 + 1e-10),
            StdlibValue::Float(1e-9),
            StdlibValue::Text("close enough".to_string()),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Unit))))
    );
}

#[test]
fn assert_approx_fail_returns_err_message() {
    let result = call_pure_stdlib(
        "std.testing.assert_approx",
        &[
            StdlibValue::Float(1.0),
            StdlibValue::Float(2.0),
            StdlibValue::Float(0.5),
            StdlibValue::Text("not close".to_string()),
        ],
    );
    let Ok(StdlibValue::Result(Err(message))) = result else {
        panic!("assert_approx failure must return Result::Err, got {result:?}");
    };
    assert!(
        matches!(&*message, StdlibValue::Text(text) if text.contains("not close")),
        "failure message must preserve assertion context, got {message:?}"
    );
}

#[test]
fn assert_approx_type_error_reports_expected_shape() {
    let result = call_pure_stdlib(
        "std.testing.assert_approx",
        &[
            StdlibValue::Int(1),
            StdlibValue::Float(1.0),
            StdlibValue::Float(1e-9),
            StdlibValue::Text("wrong type".to_string()),
        ],
    );
    assert_eq!(
        result,
        Err(StdlibExecError::Type {
            expected: "Float, Float, Float, Text"
        })
    );
}

#[test]
fn expect_error_passes_on_result_err() {
    let result = call_pure_stdlib(
        "std.testing.expect_error",
        &[
            StdlibValue::Result(Err(Box::new(StdlibValue::Text("boom".to_string())))),
            StdlibValue::Text("must fail".to_string()),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Unit))))
    );
}

#[test]
fn expect_error_fails_on_result_ok() {
    let result = call_pure_stdlib(
        "std.testing.expect_error",
        &[
            StdlibValue::Result(Ok(Box::new(StdlibValue::Int(42)))),
            StdlibValue::Text("must fail".to_string()),
        ],
    );
    let Ok(StdlibValue::Result(Err(message))) = result else {
        panic!("expect_error failure must return Result::Err, got {result:?}");
    };
    assert!(
        matches!(&*message, StdlibValue::Text(text) if text.contains("must fail")),
        "failure message must preserve assertion context, got {message:?}"
    );
}

#[test]
fn expect_error_type_error_reports_expected_shape() {
    let result = call_pure_stdlib(
        "std.testing.expect_error",
        &[
            StdlibValue::Int(1),
            StdlibValue::Text("wrong type".to_string()),
        ],
    );
    assert_eq!(
        result,
        Err(StdlibExecError::Type {
            expected: "Result<T, E>, Text"
        })
    );
}
