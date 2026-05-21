/// Returns a [`std::path::PathBuf`] pointing to a file inside the **calling
/// crate's** `tests/fixtures/` directory.
///
/// Because this is a `macro_rules!` macro, `env!("CARGO_MANIFEST_DIR")` is
/// expanded at the call site, so the resolved path always belongs to the crate
/// that invokes the macro — not to `ail-testkit` itself.
///
/// # Panics
///
/// Panics with an informative message if the file does not exist at the
/// resolved path.
///
/// # Example
///
/// ```rust,no_run
/// // Inside a test in some other crate that depends on ail-testkit:
/// let path = ail_testkit::fixture!("sample.atl");
/// ```
#[macro_export]
macro_rules! fixture {
    ($name:expr) => {{
        let path = ::std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join($name);
        if !path.exists() {
            panic!(
                "fixture not found: {}\n\
                 Hint: create the file at that path to use it in tests.",
                path.display()
            );
        }
        path
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixture_resolves_existing_file() {
        let path = crate::fixture!("sample.atl");
        assert!(path.exists(), "fixture path must exist");
    }

    #[test]
    #[should_panic(expected = "fixture not found")]
    fn fixture_panics_on_missing_file() {
        crate::fixture!("does_not_exist.atl");
    }
}
