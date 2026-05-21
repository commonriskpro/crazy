// ── ail-package::export ───────────────────────────────────────────────────
//
// `ExportDeclaration` — a public export entry for a package.
//
// Every public export in a package must declare its signature, effects,
// contracts, visibility, and stability so the verifier and dependency
// resolver can reason about it without inspecting source code.
//
// # Determinism contract
//
// All collection fields use `Vec`, never `HashMap`.  CBOR serialization
// via `ciborium` is byte-deterministic for this layout.

use serde::{Deserialize, Serialize};

// ── ExportVisibility ──────────────────────────────────────────────────────

/// Visibility tier of an exported symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportVisibility {
    /// Visible to any importer.
    Public,
    /// Visible only within the same organization namespace.
    Internal,
    /// Visible only within the package itself (not truly exported; retained
    /// for completeness so round-tripped manifests are lossless).
    Private,
}

impl std::fmt::Display for ExportVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ExportVisibility::Public => "public",
            ExportVisibility::Internal => "internal",
            ExportVisibility::Private => "private",
        };
        f.write_str(s)
    }
}

// ── ExportStability ───────────────────────────────────────────────────────

/// Stability promise attached to an exported symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportStability {
    /// The API is stable across minor versions.
    Stable,
    /// The API is maturing; breaking changes require a minor-version bump.
    Beta,
    /// The API may change in any release without notice.
    Experimental,
    /// The API is deprecated and scheduled for removal.
    Deprecated,
}

impl std::fmt::Display for ExportStability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ExportStability::Stable => "stable",
            ExportStability::Beta => "beta",
            ExportStability::Experimental => "experimental",
            ExportStability::Deprecated => "deprecated",
        };
        f.write_str(s)
    }
}

// ── ExportDeclaration ─────────────────────────────────────────────────────

/// A declared public export in a package manifest.
///
/// Each export must enumerate its type signature, the effects it may trigger,
/// the contracts it upholds, its visibility tier, and its stability promise.
/// This information is used by the verifier and dependency resolver without
/// requiring access to source code.
///
/// See `docs/packages.md` §Exports for the full design.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportDeclaration {
    /// Qualified export name (e.g., `"charge"`, `"StripePayment"`).
    pub name: String,
    /// Human-readable type signature string
    /// (e.g., `"PaymentRequest -> Result<PaymentReceipt, PaymentError>"`).
    pub signature: String,
    /// Effect tokens that this export may trigger
    /// (e.g., `["payment.charge:PaymentProvider"]`).
    ///
    /// An empty `Vec` means the export is pure / has no declared effects.
    pub effects: Vec<String>,
    /// Contract IDs upheld by this export (e.g., `["idempotent_by_key"]`).
    ///
    /// An empty `Vec` means no contracts are declared.
    pub contracts: Vec<String>,
    /// Visibility tier of this export.
    pub visibility: ExportVisibility,
    /// Stability promise for this export.
    pub stability: ExportStability,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_export() -> ExportDeclaration {
        ExportDeclaration {
            name: "charge".to_string(),
            signature: "PaymentRequest -> Result<PaymentReceipt, PaymentError>".to_string(),
            effects: vec!["payment.charge:PaymentProvider".to_string()],
            contracts: vec!["idempotent_by_key".to_string()],
            visibility: ExportVisibility::Public,
            stability: ExportStability::Stable,
        }
    }

    // ── export_declaration_cbor_round_trip ────────────────────────────────
    // Spec scenario: "ExportDeclaration round-trips through CBOR"
    //   GIVEN an ExportDeclaration with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn export_declaration_cbor_round_trip() {
        let original = sample_export();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR serialization must succeed");

        let decoded: ExportDeclaration =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

        assert_eq!(decoded, original);
    }

    // ── export_declaration_cbor_is_deterministic ──────────────────────────
    // TRIANGULATE: encoding the same value twice yields identical bytes.
    #[test]
    fn export_declaration_cbor_is_deterministic() {
        let export = sample_export();

        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&export, &mut buf1).expect("first encode");

        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&export, &mut buf2).expect("second encode");

        assert_eq!(
            buf1, buf2,
            "identical inputs must produce identical CBOR bytes"
        );
    }

    // ── pure_export_has_empty_effects ─────────────────────────────────────
    // TRIANGULATE: an export with no effects serializes and deserializes cleanly.
    #[test]
    fn pure_export_has_empty_effects() {
        let export = ExportDeclaration {
            name: "identity".to_string(),
            signature: "T -> T".to_string(),
            effects: vec![],
            contracts: vec![],
            visibility: ExportVisibility::Public,
            stability: ExportStability::Stable,
        };

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&export, &mut buf).expect("encode");
        let decoded: ExportDeclaration = ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert!(decoded.effects.is_empty());
        assert!(decoded.contracts.is_empty());
    }

    // ── visibility_display ────────────────────────────────────────────────
    #[test]
    fn visibility_display() {
        assert_eq!(ExportVisibility::Public.to_string(), "public");
        assert_eq!(ExportVisibility::Internal.to_string(), "internal");
        assert_eq!(ExportVisibility::Private.to_string(), "private");
    }

    // ── stability_display ─────────────────────────────────────────────────
    #[test]
    fn stability_display() {
        assert_eq!(ExportStability::Stable.to_string(), "stable");
        assert_eq!(ExportStability::Beta.to_string(), "beta");
        assert_eq!(ExportStability::Experimental.to_string(), "experimental");
        assert_eq!(ExportStability::Deprecated.to_string(), "deprecated");
    }
}
