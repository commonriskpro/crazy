// Integration test: hash stability.
//
// Spec scenario: "Encode determinism (object layer)"
//   GIVEN a fixed struct value encoded to CBOR via CborCodec
//   WHEN the bytes are hashed twice via ObjectId::from_bytes
//   THEN both `ObjectId` values are byte-identical
//
//   Also verifies that different structs produce different ObjectIds
//   (non-trivial — forces real hashing logic).

use ail_storage::{
    codec::{CborCodec, ContentCodec},
    object::ObjectId,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Fixture {
    id: u64,
    name: String,
}

// ── same_bytes_same_id ────────────────────────────────────────────────────────
// Encoding the same value twice must yield identical ObjectIds.
#[test]
fn same_bytes_same_id() {
    let codec = CborCodec;
    let val = Fixture {
        id: 1,
        name: "alpha".to_string(),
    };

    let bytes1 = codec.encode(&val).expect("first encode");
    let bytes2 = codec.encode(&val).expect("second encode");

    let id1 = ObjectId::from_bytes(&bytes1);
    let id2 = ObjectId::from_bytes(&bytes2);

    assert_eq!(id1, id2, "same encoded bytes must yield the same ObjectId");
}

// ── TRIANGULATE: different_values_different_ids ───────────────────────────────
// Two distinct values must produce different ObjectIds.
// Forces real hashing logic; a trivially hardcoded ObjectId would fail this.
#[test]
fn different_values_different_ids() {
    let codec = CborCodec;
    let a = Fixture {
        id: 1,
        name: "alpha".to_string(),
    };
    let b = Fixture {
        id: 2,
        name: "beta".to_string(),
    };

    let id_a = ObjectId::from_bytes(&codec.encode(&a).expect("encode a"));
    let id_b = ObjectId::from_bytes(&codec.encode(&b).expect("encode b"));

    assert_ne!(
        id_a, id_b,
        "different values must produce different ObjectIds"
    );
}
