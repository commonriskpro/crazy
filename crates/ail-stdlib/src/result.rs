// ── ail-stdlib::result ────────────────────────────────────────────────────
//
// `Result<T, E>` combinators for the AIL `std.result` module.
// Implementations follow G26 stdlib-impl spec R3.1–R3.4.

/// Apply `f` to the `Ok` value, leaving `Err` unchanged.
pub fn result_map<T, U, E>(r: Result<T, E>, f: impl Fn(T) -> U) -> Result<U, E> {
    r.map(f)
}

/// Chain a `Result`-returning function onto a `Result` value.
/// Short-circuits on the first `Err`.
pub fn result_and_then<T, U, E>(r: Result<T, E>, f: impl Fn(T) -> Result<U, E>) -> Result<U, E> {
    r.and_then(f)
}

/// Return the `Ok` value, or `default` if the result is `Err`.
pub fn result_unwrap_or<T, E>(r: Result<T, E>, default: T) -> T {
    r.unwrap_or(default)
}

/// Transpose a `Result<Option<T>, E>` into an `Option<Result<T, E>>`.
///
/// - `Ok(Some(v))` → `Some(Ok(v))`
/// - `Ok(None)`    → `None`
/// - `Err(e)`      → `Some(Err(e))`
pub fn result_transpose<T, E>(r: Result<Option<T>, E>) -> Option<Result<T, E>> {
    match r {
        Ok(Some(v)) => Some(Ok(v)),
        Ok(None) => None,
        Err(e) => Some(Err(e)),
    }
}
