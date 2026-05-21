// ContentCodec trait and CborCodec implementation.
//
// # Determinism contract
//
// `CborCodec` treats its encoded output as deterministic for fixed-layout
// Serde types. To maintain that guarantee, types serialized through this
// codec MUST NOT contain:
//
// - `HashMap` fields (key iteration order is undefined)
// - floating-point values in hash-covered types (`f32` / `f64`)
// - any collection whose serialization order is not guaranteed by its type
//
// Use ordered collections (`Vec`, `BTreeMap`) and integer timestamps instead.

use crate::error::{StorageError, StorageResult};

/// Codec that can deterministically encode and decode values.
///
/// Implementors must guarantee that encoding the same value twice always
/// produces byte-identical output. See the module-level doc for the
/// invariants required of the serialized types.
pub trait ContentCodec {
    /// Serialize `value` into bytes.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Codec`] if serialization fails.
    fn encode<T: serde::Serialize>(&self, value: &T) -> StorageResult<Vec<u8>>;

    /// Deserialize a value of type `T` from `bytes`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Codec`] if deserialization fails.
    fn decode<T: for<'de> serde::Deserialize<'de>>(&self, bytes: &[u8]) -> StorageResult<T>;
}

/// CBOR codec backed by [`ciborium`].
///
/// Deterministic for fixed-layout Serde structs. See the module-level
/// determinism contract before using this codec in hash-covered contexts.
pub struct CborCodec;

impl ContentCodec for CborCodec {
    fn encode<T: serde::Serialize>(&self, value: &T) -> StorageResult<Vec<u8>> {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(value, &mut buf)
            .map_err(|e| StorageError::Codec(e.to_string()))?;
        Ok(buf)
    }

    fn decode<T: for<'de> serde::Deserialize<'de>>(&self, bytes: &[u8]) -> StorageResult<T> {
        ciborium::de::from_reader(bytes).map_err(|e| StorageError::Codec(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::{CborCodec, ContentCodec};

    /// A simple value type used across codec tests.
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Sample {
        value: u64,
        label: String,
    }

    // ── encode_determinism ────────────────────────────────────────────────────
    // Spec scenario: "Encode determinism"
    //   GIVEN a value of type T: ContentCodec
    //   WHEN encode is called twice with the same value
    //   THEN both calls return identical byte sequences
    #[test]
    fn encode_determinism() {
        let codec = CborCodec;
        let v = Sample {
            value: 42,
            label: "hello".to_string(),
        };
        let b1 = codec.encode(&v).expect("first encode must succeed");
        let b2 = codec.encode(&v).expect("second encode must succeed");
        assert_eq!(b1, b2, "identical inputs must produce identical bytes");
    }

    // ── encode_decode_roundtrip ───────────────────────────────────────────────
    // Spec scenario: "Full round-trip" (codec layer)
    //   GIVEN a value of type T: ContentCodec
    //   WHEN encode produces bytes and decode consumes them
    //   THEN the decoded value equals the original
    #[test]
    fn encode_decode_roundtrip() {
        let codec = CborCodec;
        let original = Sample {
            value: 99,
            label: "world".to_string(),
        };
        let bytes = codec.encode(&original).expect("encode must succeed");
        let decoded: Sample = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(decoded, original, "decoded value must equal the original");
    }

    // ── TRIANGULATE: decode_error_on_garbage_bytes ────────────────────────────
    // Forces real decode logic: hardcoded success would fail this case.
    // Spec: ContentCodec.decode must return Err on invalid CBOR input.
    #[test]
    fn decode_error_on_garbage_bytes() {
        let codec = CborCodec;
        let result: Result<Sample, _> = codec.decode(b"not valid cbor at all");
        assert!(
            result.is_err(),
            "decoding garbage bytes must return an error"
        );
    }

    // ── TRIANGULATE: determinism_across_different_field_values ────────────────
    // Two distinct values must NOT produce the same bytes (non-trivial output).
    // Spec: encode is a function of the input value, not a constant.
    #[test]
    fn determinism_across_different_field_values() {
        let codec = CborCodec;
        let a = Sample {
            value: 1,
            label: "a".to_string(),
        };
        let b = Sample {
            value: 2,
            label: "b".to_string(),
        };
        let bytes_a = codec.encode(&a).expect("encode a");
        let bytes_b = codec.encode(&b).expect("encode b");
        assert_ne!(
            bytes_a, bytes_b,
            "different values must produce different byte sequences"
        );
    }
}
