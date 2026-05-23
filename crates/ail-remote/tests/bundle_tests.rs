// ── ail-remote / bundle_tests ─────────────────────────────────────────────
//
// Integration tests for `ObjectBundle` CBOR serialization and integrity.
//
// Spec scenarios covered:
//   - Bundle CBOR roundtrip (deterministic CBOR via `CborCodec`)
//   - Multi-object bundle integrity verification
//   - Empty bundle edge case (no objects, root absent → RootNotFound)

use std::collections::BTreeMap;

use ail_remote::bundle::{BundleError, ObjectBundle};
use ail_storage::SnapshotEnvelope;
use ail_storage::backends::memory::MemoryObjectStore;
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::object::{ObjectId, ObjectStore, RawObject};
use futures::executor::block_on;

// ── bundle_cbor_roundtrip ─────────────────────────────────────────────────
// Spec scenario: Bundle CBOR roundtrip.
//   GIVEN a valid `ObjectBundle` with at least one object
//   WHEN encoded to CBOR and decoded back
//   THEN the decoded bundle equals the original
#[test]
fn bundle_cbor_roundtrip() {
    let codec = CborCodec;
    let bytes = b"integration test object".to_vec();
    let id = ObjectId::from_bytes(&bytes);
    let mut objects = BTreeMap::new();
    objects.insert(id, bytes);
    let bundle = ObjectBundle::new(id, objects);

    let encoded = codec.encode(&bundle).expect("encode must succeed");
    let decoded: ObjectBundle = codec.decode(&encoded).expect("decode must succeed");
    assert_eq!(decoded, bundle, "CBOR roundtrip must preserve the bundle");
}

// ── multi_object_bundle_verify ────────────────────────────────────────────
// Verify that a bundle with multiple objects passes integrity check.
#[test]
fn multi_object_bundle_verify() {
    let entries: &[&[u8]] = &[b"first object", b"second object", b"root object"];
    let mut objects = BTreeMap::new();
    for entry in entries {
        let id = ObjectId::from_bytes(entry);
        objects.insert(id, entry.to_vec());
    }
    let root = ObjectId::from_bytes(b"root object");
    let bundle = ObjectBundle::new(root, objects);
    bundle
        .verify_integrity()
        .expect("multi-object bundle must pass integrity check");
}

// ── multi_object_bundle_cbor_roundtrip ────────────────────────────────────
// CBOR roundtrip with multiple objects; re-encode produces identical bytes.
#[test]
fn multi_object_bundle_cbor_roundtrip() {
    let codec = CborCodec;
    let entries: &[&[u8]] = &[b"alpha", b"beta", b"gamma"];
    let mut objects = BTreeMap::new();
    for entry in entries {
        let id = ObjectId::from_bytes(entry);
        objects.insert(id, entry.to_vec());
    }
    let root = ObjectId::from_bytes(b"alpha");
    let bundle = ObjectBundle::new(root, objects);

    let encoded_a = codec.encode(&bundle).expect("first encode");
    let encoded_b = codec.encode(&bundle).expect("second encode");
    assert_eq!(
        encoded_a, encoded_b,
        "identical bundle must produce identical CBOR bytes"
    );

    let decoded: ObjectBundle = codec.decode(&encoded_a).expect("decode");
    assert_eq!(decoded, bundle, "roundtrip must preserve bundle");
}

// ── empty_bundle_root_not_found ───────────────────────────────────────────
// Edge case: empty objects map with a declared root → RootNotFound.
#[test]
fn empty_bundle_root_not_found() {
    let phantom_root = ObjectId::from_bytes(b"does-not-exist");
    let bundle = ObjectBundle::new(phantom_root, BTreeMap::new());
    let result = bundle.verify_integrity();
    assert_eq!(
        result,
        Err(BundleError::RootNotFound),
        "empty bundle with phantom root must return RootNotFound"
    );
}

// ── empty_bundle_cbor_roundtrip ───────────────────────────────────────────
// Edge case: empty bundle (no objects) survives CBOR roundtrip.
#[test]
fn empty_bundle_cbor_roundtrip() {
    let codec = CborCodec;
    let phantom_root = ObjectId::from_bytes(b"empty-root");
    let bundle = ObjectBundle::new(phantom_root, BTreeMap::new());
    let encoded = codec.encode(&bundle).expect("encode");
    let decoded: ObjectBundle = codec.decode(&encoded).expect("decode");
    assert_eq!(decoded, bundle, "empty bundle must survive CBOR roundtrip");
}

#[test]
fn snapshot_envelope_bundle_includes_available_direct_dependencies() {
    block_on(async {
        let codec = CborCodec;
        let store = MemoryObjectStore::new();
        let graph_id = store
            .put(RawObject(b"graph root".to_vec()))
            .await
            .expect("store graph object");
        let audit_id = store
            .put(RawObject(b"audit record".to_vec()))
            .await
            .expect("store audit object");
        let declared_change_id = ObjectId::from_bytes(b"declared change payload");
        store
            .put(RawObject(b"declared change payload".to_vec()))
            .await
            .expect("store declared change object");
        let snapshot = SnapshotEnvelope {
            id: ObjectId::from_bytes(b"snapshot identity"),
            graph_root_hash: graph_id,
            applied_change_id: Some(declared_change_id),
            audit_record_ids: vec![audit_id],
            ..Default::default()
        };
        let snapshot_bytes = codec.encode(&snapshot).expect("encode snapshot");
        let snapshot_root = store
            .put(RawObject(snapshot_bytes))
            .await
            .expect("store snapshot envelope");

        let bundle = ObjectBundle::from_store_with_snapshot_dependencies(snapshot_root, &store)
            .await
            .expect("build bundle");

        bundle.verify_integrity().expect("bundle must verify");
        assert!(bundle.includes_snapshot_envelope_dependencies());
        assert!(bundle.objects.contains_key(&snapshot_root));
        assert!(bundle.objects.contains_key(&graph_id));
        assert!(bundle.objects.contains_key(&audit_id));
        assert!(bundle.objects.contains_key(&declared_change_id));
        assert_eq!(bundle.objects.len(), 4);
    });
}

#[test]
fn non_genesis_snapshot_parent_identity_is_not_required_as_bundle_object() {
    block_on(async {
        let codec = CborCodec;
        let store = MemoryObjectStore::new();
        let graph_id = store
            .put(RawObject(b"graph root for child snapshot".to_vec()))
            .await
            .expect("store graph object");
        let parent_identity = ObjectId::from_bytes(b"parent snapshot envelope identity");
        let snapshot = SnapshotEnvelope {
            id: ObjectId::from_bytes(b"child snapshot identity"),
            graph_root_hash: graph_id,
            parent_id: Some(parent_identity),
            ..Default::default()
        };
        let snapshot_bytes = codec.encode(&snapshot).expect("encode snapshot");
        let snapshot_root = store
            .put(RawObject(snapshot_bytes))
            .await
            .expect("store child snapshot envelope");

        let bundle = ObjectBundle::from_store_with_snapshot_dependencies(snapshot_root, &store)
            .await
            .expect("build bundle without parent object");

        bundle.verify_integrity().expect("bundle must verify");
        assert!(bundle.objects.contains_key(&snapshot_root));
        assert!(bundle.objects.contains_key(&graph_id));
        assert!(!bundle.objects.contains_key(&parent_identity));
        assert_eq!(bundle.objects.len(), 2);
    });
}

#[test]
fn snapshot_envelope_bundle_rejects_missing_declared_dependency() {
    let codec = CborCodec;
    let graph_id = ObjectId::from_bytes(b"declared graph root");
    let snapshot = SnapshotEnvelope {
        id: ObjectId::from_bytes(b"snapshot identity"),
        graph_root_hash: graph_id,
        ..Default::default()
    };
    let snapshot_bytes = codec.encode(&snapshot).expect("encode snapshot");
    let snapshot_root = ObjectId::from_bytes(&snapshot_bytes);
    let mut objects = BTreeMap::new();
    objects.insert(snapshot_root, snapshot_bytes);
    let bundle = ObjectBundle::new(snapshot_root, objects);

    assert_eq!(
        bundle.verify_integrity(),
        Err(BundleError::MissingSnapshotDependency {
            dependency: graph_id,
        }),
        "snapshot bundle must not be trusted when declared dependencies are absent"
    );
}

#[test]
fn snapshot_envelope_bundle_build_fails_when_declared_dependency_missing_from_store() {
    block_on(async {
        let codec = CborCodec;
        let store = MemoryObjectStore::new();
        let missing_graph_id = ObjectId::from_bytes(b"missing declared graph root");
        let snapshot = SnapshotEnvelope {
            id: ObjectId::from_bytes(b"snapshot identity"),
            graph_root_hash: missing_graph_id,
            ..Default::default()
        };
        let snapshot_bytes = codec.encode(&snapshot).expect("encode snapshot");
        let snapshot_root = store
            .put(RawObject(snapshot_bytes))
            .await
            .expect("store snapshot envelope");

        let result =
            ObjectBundle::from_store_with_snapshot_dependencies(snapshot_root, &store).await;

        assert!(
            matches!(result, Err(ail_storage::error::StorageError::NotFound)),
            "bundle builder must fail rather than silently omit declared dependencies; got {result:?}"
        );
    });
}

#[test]
fn raw_root_bundle_reports_no_snapshot_envelope_dependencies() {
    block_on(async {
        let store = MemoryObjectStore::new();
        let root = store
            .put(RawObject(b"raw root object".to_vec()))
            .await
            .expect("store raw root");

        let bundle = ObjectBundle::from_store_with_snapshot_dependencies(root, &store)
            .await
            .expect("build bundle");

        bundle.verify_integrity().expect("bundle must verify");
        assert!(!bundle.includes_snapshot_envelope_dependencies());
        assert_eq!(bundle.objects.len(), 1);
    });
}
