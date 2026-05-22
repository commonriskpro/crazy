// ── ail-package::generated_artifact ──────────────────────────────────────
//
// Generated artifact metadata linking derived outputs back to the package
// graph hash.
//
// # Design (docs/packages.md §Importing generated artifacts)
//
// Generated SDKs and docs are not the source of truth for a package.
// They are derived from the package graph and must link back to the
// package graph hash that produced them:
//
//   generated_artifacts
//     sdk.typescript hash=...
//     docs hash=...
//   end
//
// Rule: Generated artifacts link back to package graph hash.

use serde::{Deserialize, Serialize};

// ── GeneratedArtifact ─────────────────────────────────────────────────────

/// One derived artifact produced from a package build.
///
/// Derived artifacts (SDKs, documentation) are not the source of truth
/// for a package.  Each artifact records:
/// - its role/kind (e.g., `"sdk.typescript"`, `"docs"`)
/// - its BLAKE3 content hash
/// - the package graph hash that produced it — binding the artifact back to
///   the canonical package source
///
/// See `docs/packages.md` §Importing generated artifacts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedArtifact {
    /// Role or kind of this artifact (e.g., `"sdk.typescript"`, `"docs"`).
    pub role: String,
    /// BLAKE3 hex digest of the artifact content.
    pub artifact_hash: String,
    /// BLAKE3 hex digest of the package graph at the time the artifact was
    /// generated.  Links the artifact back to the exact package revision.
    pub graph_hash: String,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_artifact() -> GeneratedArtifact {
        GeneratedArtifact {
            role: "sdk.typescript".to_string(),
            artifact_hash: "a".repeat(64),
            graph_hash: "b".repeat(64),
        }
    }

    // ── generated_artifact_cbor_round_trip ───────────────────────────────
    // Spec scenario: "GeneratedArtifact round-trips through CBOR"
    //   GIVEN a GeneratedArtifact with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn generated_artifact_cbor_round_trip() {
        let original = sample_artifact();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR encode must succeed");
        let decoded: GeneratedArtifact =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR decode must succeed");

        assert_eq!(decoded, original);
    }

    // ── generated_artifact_links_back_to_graph_hash ───────────────────────
    // Spec scenario: "GeneratedArtifact links back to package graph hash"
    //   GIVEN two artifacts with the same role but different graph_hashes
    //   WHEN compared
    //   THEN they are not equal (the binding to graph hash distinguishes them)
    #[test]
    fn generated_artifact_links_back_to_graph_hash() {
        let a1 = sample_artifact();
        let mut a2 = sample_artifact();
        a2.graph_hash = "c".repeat(64);

        assert_ne!(a1, a2, "different graph_hash must not be equal");
        assert_eq!(a1.role, a2.role);
    }

    // ── generated_artifact_cbor_is_deterministic ─────────────────────────
    // TRIANGULATE: encoding the same value twice produces identical bytes.
    #[test]
    fn generated_artifact_cbor_is_deterministic() {
        let a = sample_artifact();

        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&a, &mut buf1).expect("encode 1");

        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&a, &mut buf2).expect("encode 2");

        assert_eq!(buf1, buf2);
    }

    // ── generated_artifact_docs_role ─────────────────────────────────────
    // TRIANGULATE: docs artifact survives round-trip with its graph_hash.
    #[test]
    fn generated_artifact_docs_role() {
        let doc = GeneratedArtifact {
            role: "docs".to_string(),
            artifact_hash: "d".repeat(64),
            graph_hash: "e".repeat(64),
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&doc, &mut buf).expect("encode");
        let decoded: GeneratedArtifact = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded.role, "docs");
        assert_eq!(decoded.graph_hash, "e".repeat(64));
    }
}
