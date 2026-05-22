// Tests for ail-stdlib::result — Result<T, E> combinators.
//
// TDD cycle: tests written before implementation.
// Spec: G26 stdlib-impl, Requirements R3.1–R3.4.

use ail_stdlib::result::{result_and_then, result_map, result_transpose, result_unwrap_or};

// ── R3.1: result_map ──────────────────────────────────────────────────────

// S3.1: maps Ok value
#[test]
fn result_map_ok() {
    let r: Result<i32, &str> = Ok(2);
    assert_eq!(result_map(r, |x| x * 3), Ok(6));
}

// S3.2: Err passes through unchanged
#[test]
fn result_map_err_unchanged() {
    let r: Result<i32, &str> = Err("e");
    let mapped: Result<i32, &str> = result_map(r, |x| x * 3);
    assert_eq!(mapped, Err("e"));
}

// Triangulate: type transformation
#[test]
fn result_map_type_change() {
    let r: Result<i32, &str> = Ok(42);
    assert_eq!(result_map(r, |x| x.to_string()), Ok("42".to_string()));
}

// ── R3.2: result_and_then ─────────────────────────────────────────────────

// S3.3: and_then on Ok with success predicate
#[test]
fn result_and_then_ok_to_ok() {
    let r: Result<i32, &str> = Ok(2);
    assert_eq!(
        result_and_then(r, |x| if x > 1 { Ok(x) } else { Err("too small") }),
        Ok(2)
    );
}

// and_then on Ok with failure predicate
#[test]
fn result_and_then_ok_to_err() {
    let r: Result<i32, &str> = Ok(0);
    assert_eq!(
        result_and_then(r, |x| if x > 1 { Ok(x) } else { Err("too small") }),
        Err("too small")
    );
}

// Triangulate: Err propagates without calling f
#[test]
fn result_and_then_err_propagates() {
    let r: Result<i32, &str> = Err("original");
    let called = std::cell::Cell::new(false);
    let result = result_and_then(r, |x| {
        called.set(true);
        Ok::<i32, &str>(x)
    });
    assert_eq!(result, Err("original"));
    assert!(!called.get(), "f must not be called on Err");
}

// ── R3.3: result_unwrap_or ────────────────────────────────────────────────

// S3.4: Err returns default
#[test]
fn result_unwrap_or_err_returns_default() {
    let r: Result<i32, &str> = Err("e");
    assert_eq!(result_unwrap_or(r, 42), 42);
}

// Triangulate: Ok returns value
#[test]
fn result_unwrap_or_ok_returns_value() {
    let r: Result<i32, &str> = Ok(7);
    assert_eq!(result_unwrap_or(r, 42), 7);
}

// ── R3.4: result_transpose ───────────────────────────────────────────────

// S3.5: Ok(Some(v)) → Some(Ok(v))
#[test]
fn result_transpose_ok_some() {
    let r: Result<Option<i32>, &str> = Ok(Some(1));
    assert_eq!(result_transpose(r), Some(Ok(1)));
}

// S3.6: Ok(None) → None
#[test]
fn result_transpose_ok_none() {
    let r: Result<Option<i32>, &str> = Ok(None);
    assert_eq!(result_transpose(r), None);
}

// S3.7: Err(e) → Some(Err(e))
#[test]
fn result_transpose_err() {
    let r: Result<Option<i32>, &str> = Err("e");
    assert_eq!(result_transpose(r), Some(Err("e")));
}
