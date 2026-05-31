// ── ail-stdlib::iter ──────────────────────────────────────────────────────
//
// Effect-polymorphic iterator combinators for the AIL `std.iter` module.
// Implementations follow G26 stdlib-impl spec R5.1–R5.4.
//
// # Effect polymorphism
//
// The spec requires `map<T,U,e>`, `filter<T>`, `fold<T,U,e>`, and
// `traverse<T,U,E,e>` to be effect-polymorphic (preserving the caller's
// declared effects).  At the Rust host-side representation level, effect
// parameters are not yet tracked in the type system (that belongs to the
// language's type checker — G24).  These functions implement the pure-Rust
// semantics; the `EffectPoly` marker is recorded in the `StdlibEntry`
// metadata (see `v1.rs`).

/// Apply `f` to every element of `items`, collecting the results.
pub fn iter_map<T, U>(items: Vec<T>, f: impl Fn(T) -> U) -> Vec<U> {
    items.into_iter().map(f).collect()
}

/// Retain only the elements of `items` for which `pred` returns `true`.
pub fn iter_filter<T>(items: Vec<T>, pred: impl Fn(&T) -> bool) -> Vec<T> {
    items.into_iter().filter(|x| pred(x)).collect()
}

/// Reduce `items` to a single value by applying `f` left-to-right,
/// starting from `init`.
pub fn iter_fold<T, U>(items: Vec<T>, init: U, f: impl Fn(U, T) -> U) -> U {
    items.into_iter().fold(init, f)
}

/// Apply `f` to every element of `items`, collecting `Ok` values into a `Vec`.
/// Short-circuits on the first `Err`.
///
/// This is the effect-polymorphic `traverse` for the `Result` applicative:
/// every `f` call must succeed for the overall traversal to succeed.
pub fn iter_traverse<T, U, E>(items: Vec<T>, f: impl Fn(T) -> Result<U, E>) -> Result<Vec<U>, E> {
    items.into_iter().map(f).collect()
}

/// Return `true` if `pred` returns `true` for at least one element.
///
/// Empty inputs deterministically return `false`.
pub fn iter_any<T>(items: &[T], pred: impl Fn(&T) -> bool) -> bool {
    items.iter().any(pred)
}

/// Return `true` if `pred` returns `true` for every element.
///
/// Empty inputs deterministically return `true`, matching universal
/// quantification and Rust iterator semantics.
pub fn iter_all<T>(items: &[T], pred: impl Fn(&T) -> bool) -> bool {
    items.iter().all(pred)
}

/// Return the first element for which `pred` returns `true`.
///
/// Empty inputs and misses deterministically return `None`.
pub fn iter_find<T>(items: &[T], pred: impl Fn(&T) -> bool) -> Option<&T> {
    items.iter().find(|item| pred(*item))
}

/// Return the zero-based index of the first element for which `pred` returns
/// `true`.
///
/// Empty inputs and misses deterministically return `None`.
pub fn iter_position<T>(items: &[T], pred: impl Fn(&T) -> bool) -> Option<usize> {
    items.iter().position(pred)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_helpers_handle_empty_inputs() {
        let empty: Vec<i32> = Vec::new();

        assert!(!iter_any(&empty, |_| true));
        assert!(iter_all(&empty, |_| false));
        assert_eq!(iter_find(&empty, |_| true), None);
        assert_eq!(iter_position(&empty, |_| true), None);
    }

    #[test]
    fn search_helpers_return_first_matching_element_or_index() {
        let items = vec![1, 2, 3, 2];

        assert!(iter_any(&items, |item| *item == 3));
        assert!(!iter_all(&items, |item| *item < 3));
        assert_eq!(iter_find(&items, |item| *item % 2 == 0), Some(&2));
        assert_eq!(iter_position(&items, |item| *item % 2 == 0), Some(1));
    }

    #[test]
    fn search_helpers_return_none_for_misses() {
        let items = vec![1, 3, 5];

        assert_eq!(iter_find(&items, |item| *item % 2 == 0), None);
        assert_eq!(iter_position(&items, |item| *item % 2 == 0), None);
    }
}
