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
pub fn iter_traverse<T, U, E>(
    items: Vec<T>,
    f: impl Fn(T) -> Result<U, E>,
) -> Result<Vec<U>, E> {
    items.into_iter().map(f).collect()
}
