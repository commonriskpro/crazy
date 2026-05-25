// Tests for option and result exec handlers — arity errors, type errors, and
// negative execution paths through call_pure_stdlib.
//
// Spec: STDLIB-EXEC-OPT-1..6, STDLIB-EXEC-RES-1..6, STDLIB-EXEC-OPT-OK-OR-1..4
// These tests provide verified execution evidence for StabilityTier::Stable
// entries std.core.option.* and std.core.result.*.

use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib};

// ── Helper ────────────────────────────────────────────────────────────────

fn double(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    match v {
        StdlibValue::Int(n) => Ok(StdlibValue::Int(n * 2)),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

fn some_double(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    double(v).map(|x| StdlibValue::Option(Some(Box::new(x))))
}

fn ok_double(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    double(v).map(|x| StdlibValue::Result(Ok(Box::new(x))))
}

// ── STDLIB-EXEC-OPT-1: option.map arity error ────────────────────────────

#[test]
fn option_map_arity_error_too_few_args() {
    let result = call_pure_stdlib(
        "std.core.option.map",
        &[StdlibValue::Option(Some(Box::new(StdlibValue::Int(1))))],
    );
    assert_eq!(
        result,
        Err(StdlibExecError::Arity {
            expected: 2,
            actual: 1
        })
    );
}

// STDLIB-EXEC-OPT-2: option.map type error — non-Option first arg

#[test]
fn option_map_type_error_not_option() {
    let result = call_pure_stdlib(
        "std.core.option.map",
        &[StdlibValue::Int(1), StdlibValue::Function(double)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Option" }));
}

// STDLIB-EXEC-OPT-3: option.map None propagates without calling f

#[test]
fn option_map_none_returns_none_without_calling_f() {
    let result = call_pure_stdlib(
        "std.core.option.map",
        &[StdlibValue::Option(None), StdlibValue::Function(double)],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// STDLIB-EXEC-OPT-4: option.and_then None short-circuits

#[test]
fn option_and_then_none_short_circuits() {
    let result = call_pure_stdlib(
        "std.core.option.and_then",
        &[
            StdlibValue::Option(None),
            StdlibValue::Function(some_double),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// STDLIB-EXEC-OPT-5: option.unwrap_or type error — non-Option first arg

#[test]
fn option_unwrap_or_type_error_not_option() {
    let result = call_pure_stdlib(
        "std.core.option.unwrap_or",
        &[StdlibValue::Int(42), StdlibValue::Int(0)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Option" }));
}

// STDLIB-EXEC-OPT-6: option.unwrap_or None returns default

#[test]
fn option_unwrap_or_none_returns_default() {
    let result = call_pure_stdlib(
        "std.core.option.unwrap_or",
        &[
            StdlibValue::Option(None),
            StdlibValue::Text("fallback".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text("fallback".to_string())));
}

// ── STDLIB-EXEC-RES-1: result.map arity error ─────────────────────────────

#[test]
fn result_map_arity_error_too_few_args() {
    let result = call_pure_stdlib(
        "std.core.result.map",
        &[StdlibValue::Result(Ok(Box::new(StdlibValue::Int(1))))],
    );
    assert_eq!(
        result,
        Err(StdlibExecError::Arity {
            expected: 2,
            actual: 1
        })
    );
}

// STDLIB-EXEC-RES-2: result.map type error — non-Result first arg

#[test]
fn result_map_type_error_not_result() {
    let result = call_pure_stdlib(
        "std.core.result.map",
        &[StdlibValue::Int(1), StdlibValue::Function(double)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Result" }));
}

// STDLIB-EXEC-RES-3: result.map Err passes through unchanged

#[test]
fn result_map_err_passes_through_unchanged() {
    let result = call_pure_stdlib(
        "std.core.result.map",
        &[
            StdlibValue::Result(Err(Box::new(StdlibValue::Text("oops".to_string())))),
            StdlibValue::Function(double),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Err(Box::new(StdlibValue::Text(
            "oops".to_string()
        )))))
    );
}

// STDLIB-EXEC-RES-4: result.and_then Err short-circuits

#[test]
fn result_and_then_err_short_circuits() {
    let result = call_pure_stdlib(
        "std.core.result.and_then",
        &[
            StdlibValue::Result(Err(Box::new(StdlibValue::Text("e".to_string())))),
            StdlibValue::Function(ok_double),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Err(Box::new(StdlibValue::Text(
            "e".to_string()
        )))))
    );
}

// STDLIB-EXEC-RES-5: result.unwrap_or Err returns default

#[test]
fn result_unwrap_or_err_returns_default() {
    let result = call_pure_stdlib(
        "std.core.result.unwrap_or",
        &[
            StdlibValue::Result(Err(Box::new(StdlibValue::Text("e".to_string())))),
            StdlibValue::Int(99),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(99)));
}

// STDLIB-EXEC-RES-6: result.unwrap_or type error — non-Result first arg

#[test]
fn result_unwrap_or_type_error_not_result() {
    let result = call_pure_stdlib(
        "std.core.result.unwrap_or",
        &[StdlibValue::Int(1), StdlibValue::Int(0)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Result" }));
}

// ── STDLIB-EXEC-OPT-OK-OR-1: ok_or Some(v) returns Ok(v) ─────────────────
//
// Contract: Some(v) → Ok(v) — the inner value is promoted, no copy of err.

#[test]
fn ok_or_some_returns_ok() {
    let result = call_pure_stdlib(
        "std.core.option.ok_or",
        &[
            StdlibValue::Option(Some(Box::new(StdlibValue::Int(42)))),
            StdlibValue::Text("unused".to_string()),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(42)))))
    );
}

// STDLIB-EXEC-OPT-OK-OR-2: ok_or None returns Err(err)
//
// Contract: None → Err(err) — the provided error value is wrapped.

#[test]
fn ok_or_none_returns_err() {
    let result = call_pure_stdlib(
        "std.core.option.ok_or",
        &[
            StdlibValue::Option(None),
            StdlibValue::Text("missing".to_string()),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Err(Box::new(StdlibValue::Text(
            "missing".to_string()
        )))))
    );
}

// STDLIB-EXEC-OPT-OK-OR-3: ok_or arity error — too few arguments

#[test]
fn ok_or_arity_error_too_few_args() {
    let result = call_pure_stdlib(
        "std.core.option.ok_or",
        &[StdlibValue::Option(Some(Box::new(StdlibValue::Int(1))))],
    );
    assert_eq!(
        result,
        Err(StdlibExecError::Arity {
            expected: 2,
            actual: 1
        })
    );
}

// STDLIB-EXEC-OPT-OK-OR-4: ok_or type error — non-Option first arg

#[test]
fn ok_or_type_error_not_option() {
    let result = call_pure_stdlib(
        "std.core.option.ok_or",
        &[StdlibValue::Int(1), StdlibValue::Text("e".to_string())],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Option" }));
}
