// ── Storage fixture helpers ───────────────────────────────────────────────

/// Re-export of [`ail_storage::backends::memory::MemoryObjectStore`] for use
/// in tests across the workspace without an explicit `ail-storage` dependency.
pub use ail_storage::backends::memory::MemoryObjectStore;

/// Re-export of [`ail_storage::graph::ObjectBackedGraphStore`] for use in
/// workspace tests that need a `GraphStore` backed by an in-memory store.
pub use ail_storage::graph::ObjectBackedGraphStore;

/// Build a minimal [`ail_storage::graph::SnapshotEnvelope`] fixture.
///
/// `label` is hashed with BLAKE3 to produce both `id` and `graph_root_hash`,
/// giving a deterministic but unique `ObjectId` per call site.  `parent_id`
/// and `applied_change_id` are `None` (genesis snapshot) and `created_at`
/// is set to `0`.
///
/// # Example
///
/// ```rust
/// let snap = ail_testkit::make_snapshot_envelope("my-root");
/// assert!(snap.parent_id.is_none());
/// assert!(snap.applied_change_id.is_none());
/// ```
pub fn make_snapshot_envelope(label: &str) -> ail_storage::graph::SnapshotEnvelope {
    let id = ail_storage::object::ObjectId::from_bytes(label.as_bytes());
    ail_storage::graph::SnapshotEnvelope {
        id,
        graph_root_hash: id,
        parent_id: None,
        applied_change_id: None,
        created_at: 0,
    }
}

// ── Fixture path macro ────────────────────────────────────────────────────

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
