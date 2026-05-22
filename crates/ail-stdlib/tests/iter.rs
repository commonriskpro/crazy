// Tests for ail-stdlib::iter — effect-polymorphic iterator combinators.
//
// TDD cycle: tests written before implementation.
// Spec: G26 stdlib-impl, Requirements R5.1–R5.4.

use ail_stdlib::iter::{iter_filter, iter_fold, iter_map, iter_traverse};

// ── R5.1: iter_map ───────────────────────────────────────────────────────

// S5.1: doubles each element
#[test]
fn iter_map_doubles() {
    assert_eq!(iter_map(vec![1, 2, 3], |x| x * 2), vec![2, 4, 6]);
}

// Triangulate: empty vec → empty vec
#[test]
fn iter_map_empty() {
    let result: Vec<i32> = iter_map(vec![], |x: i32| x * 2);
    assert_eq!(result, vec![]);
}

// Triangulate: type transformation
#[test]
fn iter_map_to_string() {
    assert_eq!(
        iter_map(vec![1, 2, 3], |x: i32| x.to_string()),
        vec!["1".to_string(), "2".to_string(), "3".to_string()]
    );
}

// ── R5.2: iter_filter ────────────────────────────────────────────────────

// S5.2: filters even numbers
#[test]
fn iter_filter_evens() {
    assert_eq!(iter_filter(vec![1, 2, 3, 4], |x| x % 2 == 0), vec![2, 4]);
}

// Triangulate: filter that keeps all
#[test]
fn iter_filter_keeps_all() {
    assert_eq!(iter_filter(vec![2, 4, 6], |x| x % 2 == 0), vec![2, 4, 6]);
}

// Triangulate: filter that keeps none
#[test]
fn iter_filter_keeps_none() {
    let result: Vec<i32> = iter_filter(vec![1, 3, 5], |x| x % 2 == 0);
    assert_eq!(result, vec![]);
}

// Triangulate: empty input
#[test]
fn iter_filter_empty() {
    let result: Vec<i32> = iter_filter(vec![], |x: &i32| x % 2 == 0);
    assert_eq!(result, vec![]);
}

// ── R5.3: iter_fold ──────────────────────────────────────────────────────

// S5.3: sums elements starting from 0
#[test]
fn iter_fold_sum() {
    assert_eq!(iter_fold(vec![1, 2, 3], 0, |acc, x| acc + x), 6);
}

// Triangulate: product
#[test]
fn iter_fold_product() {
    assert_eq!(iter_fold(vec![1, 2, 3, 4], 1, |acc, x| acc * x), 24);
}

// Triangulate: empty vec returns init
#[test]
fn iter_fold_empty_returns_init() {
    assert_eq!(iter_fold(vec![], 42, |acc: i32, x: i32| acc + x), 42);
}

// Triangulate: string concatenation
#[test]
fn iter_fold_concat() {
    assert_eq!(
        iter_fold(
            vec!["a", "b", "c"],
            String::new(),
            |mut acc, x| {
                acc.push_str(x);
                acc
            }
        ),
        "abc"
    );
}

// ── R5.4: iter_traverse ──────────────────────────────────────────────────

// S5.4: all Ok → Ok(collected)
#[test]
fn iter_traverse_all_ok() {
    let result: Result<Vec<i32>, &str> = iter_traverse(vec![1, 2, 3], |x| Ok(x * 2));
    assert_eq!(result, Ok(vec![2, 4, 6]));
}

// S5.5: first Err short-circuits
#[test]
fn iter_traverse_short_circuits_on_err() {
    let result: Result<Vec<i32>, &str> =
        iter_traverse(vec![1, -1, 3], |x| if x > 0 { Ok(x) } else { Err("neg") });
    assert_eq!(result, Err("neg"));
}

// Triangulate: empty vec → Ok(empty)
#[test]
fn iter_traverse_empty() {
    let result: Result<Vec<i32>, &str> = iter_traverse(vec![], |x: i32| Ok(x));
    assert_eq!(result, Ok(vec![]));
}

// Triangulate: all Err returns first
#[test]
fn iter_traverse_all_err_returns_first() {
    let result: Result<Vec<i32>, &str> =
        iter_traverse(vec![1, 2, 3], |_| Err("always fails"));
    assert_eq!(result, Err("always fails"));
}
