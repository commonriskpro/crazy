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
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::object::ObjectId;

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
