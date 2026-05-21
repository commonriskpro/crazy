// ── ail-compiler::hash ────────────────────────────────────────────────────
//
// Stable CBOR serialization and BLAKE3 hash utilities for the compiler
// pipeline's deterministic hash chain.
//
// # Hash chain contract
//
// Each stage seals its output with:
//   `stage_hash = blake3(parent_hash_bytes || stage_content_bytes)`
//
// where `stage_content_bytes` is the CBOR encoding of the stage's data model.
// Using `ciborium` with `Vec`/`BTreeMap`-only models guarantees that the byte
// sequence is identical across runs and platforms.

use serde::Serialize;

use crate::error::CompileError;

// ── stable_cbor_bytes ─────────────────────────────────────────────────────

/// Serialize `value` to deterministic CBOR bytes using `ciborium`.
///
/// # Determinism guarantee
///
/// Callers MUST ensure `T` uses only `Vec` / `BTreeMap` collections —
/// never `HashMap` — so that the byte sequence is stable across runs.
/// All workspace data models (`CoreNode`, `AnfBinding`, etc.) already
/// satisfy this requirement.
///
/// # Errors
///
/// Returns `CompileError::EncodingError` if `ciborium` cannot serialise
/// the value (e.g. unsupported type, I/O error on the backing buffer).
pub fn stable_cbor_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CompileError> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(value, &mut buf)
        .map_err(|e| CompileError::EncodingError(e.to_string()))?;
    Ok(buf)
}

// ── hash_with_parent ──────────────────────────────────────────────────────

/// Compute `blake3(parent_bytes || content_bytes)` and return the 32-byte digest.
///
/// This is the primitive used by every pipeline stage to chain its hash to
/// the previous stage's hash.  The `parent` is the previous stage's `[u8; 32]`
/// digest cast to `&[u8]`; `bytes` is the CBOR encoding of the current stage.
///
/// # Pure function
///
/// No state, no I/O.  Same inputs always produce the same output.
pub fn hash_with_parent(parent: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(parent);
    hasher.update(bytes);
    *hasher.finalize().as_bytes()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Unit tests complementary to tests/hash_tests.rs (integration).
    // These inline tests keep the safety net close to the implementation.

    // Scenario: zero-length inputs are accepted without panic.
    #[test]
    fn hash_with_empty_inputs_does_not_panic() {
        let h = hash_with_parent(b"", b"");
        assert_eq!(
            h.len(),
            32,
            "empty inputs must still produce 32-byte digest"
        );
    }

    // Scenario: stable_cbor_bytes returns Ok for a trivially serialisable value.
    #[test]
    fn stable_cbor_bytes_encodes_simple_string() {
        let bytes = stable_cbor_bytes(&"hello").expect("must encode string");
        assert!(!bytes.is_empty(), "encoded bytes must be non-empty");
    }

    // TRIANGULATE: repeated calls on the same value are byte-identical.
    #[test]
    fn stable_cbor_bytes_repeated_call_is_identical() {
        let v: Vec<u32> = vec![1, 2, 3, 42];
        let b1 = stable_cbor_bytes(&v).expect("first encode");
        let b2 = stable_cbor_bytes(&v).expect("second encode");
        assert_eq!(b1, b2, "same value must encode to identical bytes");
    }
}
