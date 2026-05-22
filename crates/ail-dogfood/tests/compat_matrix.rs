// Compatibility matrix tests: frozen schema v0 CBOR fixture.
//
// Spec scenarios:
//   - "decode schema_v0.cbor with current codec succeeds"
//   - "apply V0ToV1Migration on store containing fixture object"
//   - "fixture is bit-stable (blake3 hash check against pinned value)"
//
// The fixture at `tests/fixtures/schema_v0.cbor` is a frozen binary snapshot
// of a v0 RawObject encoded with CborCodec. It must decode to the same value
// across all future codec changes.

use std::sync::Arc;

use ail_storage::{
    backends::memory::MemoryObjectStore,
    codec::{CborCodec, ContentCodec},
    migration::default_catalog,
    object::{ObjectStore, RawObject},
};
use futures::executor::block_on;

/// Path to the frozen v0 fixture, relative to the workspace root.
const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/schema_v0.cbor");

/// Pinned BLAKE3 hex hash of `schema_v0.cbor`.
///
/// Update this constant if the fixture is intentionally regenerated.
/// NEVER update it due to an accidental codec change — that would hide a
/// compatibility regression.
const FIXTURE_BLAKE3_HEX: &str = "352887189aff91d2878b52438f02b3e344d5a62ac055e5b81779ffa959bed332";

// ── decode_schema_v0_fixture_succeeds ────────────────────────────────────────
// Spec: The frozen v0 CBOR fixture must decode successfully with the current
//       CborCodec. A decode failure indicates a codec regression.
#[test]
fn decode_schema_v0_fixture_succeeds() {
    let bytes = std::fs::read(FIXTURE_PATH)
        .unwrap_or_else(|e| panic!("cannot read fixture {FIXTURE_PATH}: {e}"));

    let codec = CborCodec;
    // The fixture encodes a simple struct: { label: String, value: u64 }
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct V0Record {
        label: String,
        value: u64,
    }

    let record: V0Record = codec
        .decode(&bytes)
        .expect("current codec must decode v0 fixture without error");

    assert_eq!(
        record.label, "schema_v0_fixture",
        "decoded label must match"
    );
    assert_eq!(record.value, 0, "decoded value must match v0 sentinel");
}

// ── apply_migration_on_store_with_fixture_succeeds ───────────────────────────
// Spec: After loading the v0 fixture into a MemoryObjectStore and running the
//       V0ToV1Migration, the store must report schema version 1.
#[test]
fn apply_migration_on_store_with_fixture_succeeds() {
    block_on(async {
        let fixture_bytes = std::fs::read(FIXTURE_PATH)
            .unwrap_or_else(|e| panic!("cannot read fixture {FIXTURE_PATH}: {e}"));

        let store = Arc::new(MemoryObjectStore::new());

        // Pre-populate the store with the fixture object (simulates a v0 store
        // that already contains user data).
        store
            .put(RawObject(fixture_bytes))
            .await
            .expect("putting fixture into store must succeed");

        // Apply the default catalog: should advance to v1.
        let catalog = default_catalog();
        let new_version = catalog
            .apply(Arc::clone(&store))
            .await
            .expect("migration must succeed on store containing v0 fixture");

        assert_eq!(new_version, 1, "migration must advance store to version 1");

        // Verify current_version reflects the change.
        let current = catalog
            .current_version(Arc::clone(&store))
            .await
            .expect("current_version must succeed after migration");
        assert_eq!(current, 1, "current_version must be 1 after migration");
    });
}

// ── fixture_is_bit_stable ────────────────────────────────────────────────────
// Spec: The BLAKE3 hash of `schema_v0.cbor` must equal the pinned value.
//       A mismatch means the fixture file was accidentally modified.
//
// NOTE: The pinned hash below is generated from the fixture file itself.
//       If the fixture must be intentionally regenerated, update both the
//       file AND the constant `FIXTURE_BLAKE3_HEX` in this test.
#[test]
fn fixture_is_bit_stable() {
    let bytes = std::fs::read(FIXTURE_PATH)
        .unwrap_or_else(|e| panic!("cannot read fixture {FIXTURE_PATH}: {e}"));

    let hash = blake3::hash(&bytes);
    let hex = hash.to_hex().to_string();

    assert_eq!(
        hex, FIXTURE_BLAKE3_HEX,
        "fixture hash mismatch — file was modified unexpectedly.\n\
         If this is intentional, update FIXTURE_BLAKE3_HEX in compat_matrix.rs.\n\
         Expected: {FIXTURE_BLAKE3_HEX}\n\
         Got:      {hex}"
    );
}
