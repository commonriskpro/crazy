// ── ail-package::import ───────────────────────────────────────────────────
//
// `ImportDeclaration` — a declared dependency import for a package.
//
// An import records which package is being depended on, which items are
// brought into scope, and the version constraint applied.  Importing a
// package does NOT automatically grant any capabilities — capability grants
// are declared separately in runtime profiles (`import != grant`).
//
// # Determinism contract
//
// All collection fields use `Vec`, never `HashMap`.  CBOR serialization
// via `ciborium` is byte-deterministic for this layout.

use serde::{Deserialize, Serialize};

// ── ImportDeclaration ─────────────────────────────────────────────────────

/// A declared dependency import in a package manifest.
///
/// Records the source package, the items imported from it, and the version
/// constraint applied.  The runtime resolver uses this to enforce trust
/// gates and capability grants independently.
///
/// See `docs/packages.md` §Imports for the full design.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportDeclaration {
    /// Name of the source package (e.g., `"payments.stripe"`).
    pub source_package: String,
    /// Names of symbols imported from the source package
    /// (e.g., `["PaymentRequest", "charge"]`).
    ///
    /// An empty `Vec` means the entire public namespace is imported (wildcard).
    pub items: Vec<String>,
    /// SemVer version constraint string (e.g., `"^1.2"`, `">=2.0.0 <3.0.0"`).
    ///
    /// `None` means no constraint is declared (use the latest available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_constraint: Option<String>,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_import() -> ImportDeclaration {
        ImportDeclaration {
            source_package: "payments.stripe".to_string(),
            items: vec!["PaymentRequest".to_string(), "charge".to_string()],
            version_constraint: Some("^1.2".to_string()),
        }
    }

    // ── import_declaration_cbor_round_trip ────────────────────────────────
    // Spec scenario: "ImportDeclaration round-trips through CBOR"
    //   GIVEN an ImportDeclaration with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn import_declaration_cbor_round_trip() {
        let original = sample_import();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR serialization must succeed");

        let decoded: ImportDeclaration =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

        assert_eq!(decoded, original);
    }

    // ── import_declaration_cbor_is_deterministic ──────────────────────────
    // TRIANGULATE: encoding the same value twice yields identical bytes.
    #[test]
    fn import_declaration_cbor_is_deterministic() {
        let import = sample_import();

        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&import, &mut buf1).expect("first encode");

        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&import, &mut buf2).expect("second encode");

        assert_eq!(
            buf1, buf2,
            "identical inputs must produce identical CBOR bytes"
        );
    }

    // ── wildcard_import_has_empty_items ───────────────────────────────────
    // TRIANGULATE: an import with empty items list (wildcard) round-trips cleanly.
    #[test]
    fn wildcard_import_has_empty_items() {
        let import = ImportDeclaration {
            source_package: "utils.core".to_string(),
            items: vec![],
            version_constraint: None,
        };

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&import, &mut buf).expect("encode");
        let decoded: ImportDeclaration = ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert!(decoded.items.is_empty());
        assert!(decoded.version_constraint.is_none());
    }
}
