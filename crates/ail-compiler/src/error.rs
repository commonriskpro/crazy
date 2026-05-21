// ── ail-compiler::error ───────────────────────────────────────────────────
//
// `CompileError` — exhaustive error enum for the three-stage pipeline.

use ail_core::semantic_graph::NodeRef;

/// Errors produced by the `ail-compiler` pipeline.
///
/// Each variant corresponds to a distinct failure mode; the `lower_to_core_ir`,
/// `lower_to_anf`, and `emit_wasm` functions all return `CompileError` on failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    /// The `VerificationReport` did not have an accepted summary
    /// (`Proven` or `RuntimeChecked`).  Lowering is refused.
    RejectedReport,

    /// The `SemanticGraph` failed structural validation.
    InvalidGraph(String),

    /// A `NodeRef` present in the graph could not be resolved during lowering.
    MissingNode(NodeRef),

    /// CBOR serialization failed while computing a stage hash.
    EncodingError(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::RejectedReport => {
                write!(f, "compilation refused: verification report not accepted")
            }
            CompileError::InvalidGraph(msg) => write!(f, "invalid graph: {msg}"),
            CompileError::MissingNode(r) => write!(f, "missing node: NodeRef({})", r.0),
            CompileError::EncodingError(msg) => write!(f, "encoding error: {msg}"),
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
}
