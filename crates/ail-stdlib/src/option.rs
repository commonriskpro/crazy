// ── ail-stdlib::option ────────────────────────────────────────────────────
//
// `Option<T>` combinators for the AIL `std.option` module.
// Implementations follow G26 stdlib-impl spec R2.1–R2.5.

/// Apply `f` to the value inside `Some`, returning `None` if the option is `None`.
pub fn option_map<T, U>(opt: Option<T>, f: impl Fn(T) -> U) -> Option<U> {
    opt.map(f)
}

/// Chain an `Option`-returning function onto an `Option` value.
/// Returns `None` if the input is `None` or if `f` returns `None`.
pub fn option_and_then<T, U>(opt: Option<T>, f: impl Fn(T) -> Option<U>) -> Option<U> {
    opt.and_then(f)
}

/// Return the value inside `Some`, or `default` if `None`.
pub fn option_unwrap_or<T>(opt: Option<T>, default: T) -> T {
    opt.unwrap_or(default)
}

/// Transpose an `Option<Result<T, E>>` into a `Result<Option<T>, E>`.
///
/// - `Some(Ok(v))` → `Ok(Some(v))`
/// - `Some(Err(e))` → `Err(e)`
/// - `None`         → `Ok(None)`
pub fn option_transpose<T, E>(opt: Option<Result<T, E>>) -> Result<Option<T>, E> {
    match opt {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

/// Collect a `Vec<Result<T, E>>` into a `Result<Vec<T>, E>`.
///
/// Returns `Ok(values)` if all items are `Ok`; returns the first `Err` found.
pub fn collect_option_results<T, E>(items: Vec<Result<T, E>>) -> Result<Vec<T>, E> {
    items.into_iter().collect()
}
