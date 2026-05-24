use super::*;

// Scenario: valid 64-char hex change-id is accepted.
#[test]
fn valid_change_id_accepted() {
    let id = "a".repeat(64);
    assert!(is_valid_change_id(&id), "64 hex chars must be accepted");
}

// TRIANGULATE: too-short change-id is rejected.
#[test]
fn short_change_id_rejected() {
    let id = "a".repeat(63);
    assert!(!is_valid_change_id(&id), "63 hex chars must be rejected");
}

// TRIANGULATE: non-hex change-id is rejected.
#[test]
fn non_hex_change_id_rejected() {
    let id = "g".repeat(64);
    assert!(!is_valid_change_id(&id), "non-hex chars must be rejected");
}

// Scenario: SimpleSnapshotBridge returns its initialised id.
#[test]
fn simple_snapshot_bridge_returns_initial_id() {
    let bridge = SimpleSnapshotBridge(SnapshotId(7));
    assert_eq!(bridge.current_snapshot_id(), SnapshotId(7));
}

// TRIANGULATE: encode_cbor succeeds for a JSON-compatible value.
#[test]
fn encode_cbor_returns_bytes_for_serializable_value() {
    #[derive(serde::Serialize)]
    struct Dummy {
        x: u32,
    }
    let bytes = encode_cbor(&Dummy { x: 42 }).expect("encode_cbor must succeed");
    assert!(!bytes.is_empty(), "encoded bytes must not be empty");
}

// Scenario: hex_to_object_id roundtrip.
#[test]
fn hex_to_object_id_roundtrip() {
    let hex = "a0b1".repeat(16); // 64 chars
    assert_eq!(hex.len(), 64, "test input must be 64 chars");
    let oid = hex_to_object_id(&hex).expect("valid hex must parse");
    assert_eq!(oid.to_hex(), hex, "roundtrip must preserve hex");
}

// TRIANGULATE: hex_to_object_id rejects non-hex.
#[test]
fn hex_to_object_id_rejects_invalid() {
    let bad = "g".repeat(64);
    assert!(hex_to_object_id(&bad).is_err(), "non-hex must return Err");
}
