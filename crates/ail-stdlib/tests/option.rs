// Tests for ail-stdlib::option — Option<T> combinators.
//
// TDD cycle: tests written before implementation.
// Spec: G26 stdlib-impl, Requirements R2.1–R2.5.

use ail_stdlib::option::{
    collect_option_results, option_and_then, option_map, option_transpose, option_unwrap_or,
};

// ── R2.1: option_map ──────────────────────────────────────────────────────

// S2.1: maps Some value
#[test]
fn option_map_some() {
    assert_eq!(option_map(Some(2), |x| x * 3), Some(6));
}

// S2.2: maps None produces None
#[test]
fn option_map_none() {
    let result: Option<i32> = option_map(None, |x: i32| x * 3);
    assert_eq!(result, None);
}

// Triangulate: type transformation
#[test]
fn option_map_type_change() {
    assert_eq!(option_map(Some(42), |x| x.to_string()), Some("42".to_string()));
}

// ── R2.2: option_and_then ─────────────────────────────────────────────────

// S2.3: and_then with predicate returning Some
#[test]
fn option_and_then_some_to_some() {
    assert_eq!(
        option_and_then(Some(2), |x| if x > 1 { Some(x) } else { None }),
        Some(2)
    );
}

// S2.4: and_then with predicate returning None
#[test]
fn option_and_then_some_to_none() {
    assert_eq!(
        option_and_then(Some(0), |x| if x > 1 { Some(x) } else { None }),
        None
    );
}

// Triangulate: None propagates through and_then
#[test]
fn option_and_then_none_propagates() {
    let result: Option<i32> = option_and_then(None, |x: i32| Some(x + 1));
    assert_eq!(result, None);
}

// ── R2.3: option_unwrap_or ────────────────────────────────────────────────

// S2.5: unwrap_or on None returns default
#[test]
fn option_unwrap_or_none_returns_default() {
    assert_eq!(option_unwrap_or(None, 42), 42);
}

// Triangulate: unwrap_or on Some returns value
#[test]
fn option_unwrap_or_some_returns_value() {
    assert_eq!(option_unwrap_or(Some(7), 42), 7);
}

// ── R2.4: option_transpose ────────────────────────────────────────────────

// S2.6: Some(Ok(v)) → Ok(Some(v))
#[test]
fn option_transpose_some_ok() {
    let input: Option<Result<i32, &str>> = Some(Ok(1));
    assert_eq!(option_transpose(input), Ok(Some(1)));
}

// S2.7: Some(Err(e)) → Err(e)
#[test]
fn option_transpose_some_err() {
    let input: Option<Result<i32, &str>> = Some(Err("e"));
    assert_eq!(option_transpose(input), Err("e"));
}

// S2.8: None → Ok(None)
#[test]
fn option_transpose_none() {
    let input: Option<Result<i32, &str>> = None;
    assert_eq!(option_transpose(input), Ok(None));
}

// ── R2.5: collect_option_results ─────────────────────────────────────────

// S2.9: all Ok → Ok(Vec)
#[test]
fn collect_option_results_all_ok() {
    let items: Vec<Result<i32, &str>> = vec![Ok(1), Ok(2), Ok(3)];
    assert_eq!(collect_option_results(items), Ok(vec![1, 2, 3]));
}

// S2.10: first Err → Err
#[test]
fn collect_option_results_first_err() {
    let items: Vec<Result<i32, &str>> = vec![Ok(1), Err("e"), Ok(3)];
    assert_eq!(collect_option_results(items), Err("e"));
}

// Triangulate: empty vec → Ok(empty)
#[test]
fn collect_option_results_empty() {
    let items: Vec<Result<i32, &str>> = vec![];
    assert_eq!(collect_option_results(items), Ok(vec![]));
}
