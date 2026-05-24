// ── ail-compiler::error ───────────────────────────────────────────────────
//
// `CompileError` — exhaustive error enum for the compilation pipeline.

use ail_core::semantic_graph::NodeRef;

/// Errors produced by the `ail-compiler` pipeline.
///
/// Each variant corresponds to a distinct failure mode; the `lower_to_core_ir`,
/// `lower_to_anf`, `emit_wasm`, and `emit_native` functions all return
/// `CompileError` on failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    /// The `VerificationReport` did not have an accepted summary
    /// (`Proven` or `RuntimeChecked`).  Lowering is refused.
    RejectedReport,

    /// The `SemanticGraph` failed structural validation.
    InvalidGraph(String),

    /// A `NodeRef` present in the graph could not be resolved during lowering.
    MissingNode(NodeRef),

    /// A production-like backend profile required audit provenance that was
    /// missing from the source map.
    MissingProvenanceMetadata {
        profile: String,
        binding_name: String,
        node_id: NodeRef,
        field: &'static str,
    },

    /// CBOR serialization failed while computing a stage hash.
    EncodingError(String),

    /// A Cranelift native-backend codegen failure.
    ///
    /// Distinct from `EncodingError` — this variant is only produced by
    /// `emit_native`; it carries the Cranelift error message.
    NativeEncodingError(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::RejectedReport => {
                write!(f, "compilation refused: verification report not accepted")
            }
            CompileError::InvalidGraph(msg) => write!(f, "invalid graph: {msg}"),
            CompileError::MissingNode(r) => write!(f, "missing node: NodeRef({})", r.0),
            CompileError::MissingProvenanceMetadata {
                profile,
                binding_name,
                node_id,
                field,
            } => write!(
                f,
                "missing provenance metadata for profile {profile}: binding {binding_name} NodeRef({}) lacks {field}",
                node_id.0
            ),
            CompileError::EncodingError(msg) => write!(f, "encoding error: {msg}"),
            CompileError::NativeEncodingError(msg) => {
                write!(f, "native encoding error: {msg}")
            }
        }
    }
}

impl std::error::Error for CompileError {}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::NodeRef;

    use super::*;

    // Task 1.3 — RED: tests written before production code existed.
    // All tests reference CompileError variants and assert structural behaviour.

    // Scenario: RejectedReport variant is constructible and matchable.
    // (The base acceptance test — simplest variant, no payload.)
    #[test]
    fn rejected_report_variant_is_constructible() {
        let e = CompileError::RejectedReport;
        assert!(
            matches!(e, CompileError::RejectedReport),
            "RejectedReport must be unit variant"
        );
    }

    // Scenario: Display message identifies the error class.
    #[test]
    fn rejected_report_display_mentions_verification() {
        let msg = CompileError::RejectedReport.to_string();
        assert!(
            msg.contains("verification"),
            "display must mention 'verification', got: {msg}"
        );
    }

    // TRIANGULATE: EncodingError carries its message string.
    #[test]
    fn encoding_error_carries_message() {
        let e = CompileError::EncodingError("cbor overflow".to_string());
        match &e {
            CompileError::EncodingError(msg) => {
                assert_eq!(msg, "cbor overflow");
            }
            other => panic!("expected EncodingError, got {other:?}"),
        }
    }

    // TRIANGULATE: MissingNode carries the NodeRef payload.
    #[test]
    fn missing_node_carries_ref() {
        let e = CompileError::MissingNode(NodeRef(42));
        assert!(
            matches!(e, CompileError::MissingNode(NodeRef(42))),
            "MissingNode must carry NodeRef(42)"
        );
    }

    #[test]
    fn missing_provenance_metadata_display_mentions_profile_and_field() {
        let e = CompileError::MissingProvenanceMetadata {
            profile: "prod".to_string(),
            binding_name: "checkout".to_string(),
            node_id: NodeRef(7),
            field: "change_set",
        };
        let msg = e.to_string();
        assert!(msg.contains("prod"), "display must include profile: {msg}");
        assert!(
            msg.contains("change_set"),
            "display must include missing field: {msg}"
        );
        assert!(
            msg.contains("NodeRef(7)"),
            "display must include node ref: {msg}"
        );
    }

    // TRIANGULATE: InvalidGraph carries its description string.
    #[test]
    fn invalid_graph_carries_description() {
        let e = CompileError::InvalidGraph("duplicate ref".to_string());
        let msg = e.to_string();
        assert!(
            msg.contains("duplicate ref"),
            "InvalidGraph display must include the description"
        );
    }

    // TRIANGULATE: different error values are not equal.
    #[test]
    fn errors_are_distinguishable() {
        assert_ne!(
            CompileError::RejectedReport,
            CompileError::MissingNode(NodeRef(0))
        );
        assert_ne!(
            CompileError::EncodingError("a".to_string()),
            CompileError::EncodingError("b".to_string())
        );
    }

    // Scenario: NativeEncodingError variant is constructible and distinct from EncodingError.
    #[test]
    fn native_encoding_error_is_constructible_and_distinct() {
        let e = CompileError::NativeEncodingError("cranelift trap".to_string());
        match &e {
            CompileError::NativeEncodingError(msg) => {
                assert_eq!(msg, "cranelift trap");
            }
            other => panic!("expected NativeEncodingError, got {other:?}"),
        }
        assert_ne!(
            e,
            CompileError::EncodingError("cranelift trap".to_string()),
            "NativeEncodingError must be distinct from EncodingError"
        );
    }

    // Scenario: NativeEncodingError Display contains 'native'.
    #[test]
    fn native_encoding_error_display_mentions_native() {
        let msg = CompileError::NativeEncodingError("test reason".to_string()).to_string();
        assert!(
            msg.contains("native"),
            "display must mention 'native', got: {msg}"
        );
    }
}
