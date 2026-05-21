// ── ail-runtime::manifest ────────────────────────────────────────────────
//
// `CapabilityManifest` — declares which capabilities a WASM module requires.
//
// The manifest hash is computed as:
//   `blake3(canonical_cbor(manifest))`
//
// where `canonical_cbor` produces a deterministic byte sequence via
// `ciborium`.  Preflight compares this hash against the
// `capability_manifest_hash` recorded in `RuntimeProfile`.

use blake3::Hasher;
use ciborium::ser::into_writer;
use serde::Serialize;

use crate::profile::CapabilityId;

// ── CapabilityManifest ───────────────────────────────────────────────────

/// Declares the capability requirements of one WASM module.
///
/// The manifest is provided by the module author (or build toolchain) and
/// is validated against the profile's grants at preflight time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CapabilityManifest {
    /// Name of the WASM module this manifest describes.
    pub module: String,

    /// Capabilities required by the module, in declaration order.
    ///
    /// Preflight checks that every entry here has a matching
    /// [`CapabilityGrant`](crate::profile::CapabilityGrant) in the profile.
    pub requires: Vec<CapabilityId>,
}

impl CapabilityManifest {
    /// Compute the BLAKE3 manifest hash as a hex-encoded string.
    ///
    /// The hash covers the canonical CBOR serialization of the manifest.
    /// Returns an error string if CBOR serialization fails.
    pub fn blake3_hex(&self) -> Result<String, String> {
        let mut buf = Vec::new();
        into_writer(self, &mut buf).map_err(|e| format!("CBOR serialization failed: {e}"))?;
        let hash = blake3_hex_of(&buf);
        Ok(hash)
    }
}

// We need `CapabilityId` to be CBOR-serializable for the manifest hash.
impl Serialize for CapabilityId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────

/// Compute a BLAKE3 hex digest of a byte slice.
///
/// Exposed publicly so callers can pre-compute the `module_hash` to store in
/// a [`RuntimeProfile`](crate::profile::RuntimeProfile) before preflight.
pub fn blake3_hex_of(bytes: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    hasher.finalize().to_hex().to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Structural: manifest is constructible and fields are accessible.
    #[test]
    fn manifest_fields_are_accessible() {
        let id = CapabilityId::new("FileRead");
        let m = CapabilityManifest {
            module: "test-module".to_string(),
            requires: vec![id.clone()],
        };
        assert_eq!(m.module, "test-module");
        assert_eq!(m.requires.len(), 1);
        assert_eq!(m.requires[0], id);
    }

    // blake3_hex produces a 64-character hex string (256-bit hash).
    #[test]
    fn blake3_hex_returns_64_char_string() {
        let m = CapabilityManifest {
            module: "m".to_string(),
            requires: vec![],
        };
        let hex = m.blake3_hex().expect("serialization must succeed");
        assert_eq!(hex.len(), 64, "BLAKE3 hex must be 64 characters");
    }

    // TRIANGULATE: same manifest → same hash (determinism).
    #[test]
    fn blake3_hex_is_deterministic() {
        let m = CapabilityManifest {
            module: "m".to_string(),
            requires: vec![CapabilityId::new("X")],
        };
        let h1 = m.blake3_hex().unwrap();
        let h2 = m.blake3_hex().unwrap();
        assert_eq!(h1, h2, "hash must be deterministic");
    }

    // TRIANGULATE: different manifests → different hashes.
    #[test]
    fn blake3_hex_differs_for_different_manifests() {
        let m1 = CapabilityManifest {
            module: "m1".to_string(),
            requires: vec![],
        };
        let m2 = CapabilityManifest {
            module: "m2".to_string(),
            requires: vec![CapabilityId::new("FileRead")],
        };
        assert_ne!(
            m1.blake3_hex().unwrap(),
            m2.blake3_hex().unwrap(),
            "different manifests must have different hashes"
        );
    }
}
