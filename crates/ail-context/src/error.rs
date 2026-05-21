// ── ail-context::error ────────────────────────────────────────────────────
//
// Stable error catalogue for context operations.
//
// # Error codes
//
// Each `ContextError` variant maps to a stable string code that appears in
// `Display` output.  Consumers that need to match on codes should match on
// the enum variant, not the string.

use std::fmt;

// ── Stable error code constants ───────────────────────────────────────────

/// Stable code for `ContextError::Stale`: snapshot or graph root absent.
pub const E_CONTEXT_STALE: &str = "E_CONTEXT_STALE";
/// Stable code for `ContextError::NodeNotFound`: queried node absent.
pub const E_NODE_NOT_FOUND: &str = "E_NODE_NOT_FOUND";
/// Stable code for `ContextError::InvalidBudget`: zero-byte budget.
pub const E_INVALID_BUDGET: &str = "E_INVALID_BUDGET";
/// Stable code for `ContextError::Codec`: encode/decode failure.
pub const E_CODEC: &str = "E_CODEC";

// ── ContextError ──────────────────────────────────────────────────────────

/// Errors that can occur during context operations.
///
/// Each variant maps to a stable error code (see the `E_*` constants) that
/// appears in `Display` output for diagnostics.
#[derive(Debug, PartialEq, Eq)]
pub enum ContextError {
    /// `graph_root_hash` not found in `ObjectStore` (`E_CONTEXT_STALE`).
    Stale,
    /// Queried `NodeRef` absent from the materialized graph (`E_NODE_NOT_FOUND`).
    NodeNotFound,
    /// `budget = 0` in any `ContextQuery` variant (`E_INVALID_BUDGET`).
    InvalidBudget,
    /// Encode/decode failure with a descriptive message (`E_CODEC`).
    Codec(String),
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextError::Stale => {
                write!(
                    f,
                    "{E_CONTEXT_STALE}: snapshot or graph root not found in store"
                )
            }
            ContextError::NodeNotFound => {
                write!(f, "{E_NODE_NOT_FOUND}: queried node ref absent from graph")
            }
            ContextError::InvalidBudget => {
                write!(f, "{E_INVALID_BUDGET}: budget must be greater than zero")
            }
            ContextError::Codec(msg) => {
                write!(f, "{E_CODEC}: {msg}")
            }
        }
    }
}

impl std::error::Error for ContextError {}

// ── ContextResult ─────────────────────────────────────────────────────────

/// Convenience alias for `Result<T, ContextError>`.
pub type ContextResult<T> = Result<T, ContextError>;

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── stale_displays_with_error_code ────────────────────────────────────
    // Spec: E_CONTEXT_STALE maps to ContextError::Stale.
    //
    // RED: `ContextError::Stale` did not exist when this test was authored.
    // GREEN: enum + Display impl makes it compile and pass.
    #[test]
    fn stale_displays_with_error_code() {
        let err = ContextError::Stale;
        let s = err.to_string();
        assert!(
            s.contains(E_CONTEXT_STALE),
            "Display for Stale must contain {E_CONTEXT_STALE}, got: {s}"
        );
    }

    // ── node_not_found_displays_with_error_code ───────────────────────────
    // Spec: E_NODE_NOT_FOUND maps to ContextError::NodeNotFound.
    #[test]
    fn node_not_found_displays_with_error_code() {
        let err = ContextError::NodeNotFound;
        let s = err.to_string();
        assert!(
            s.contains(E_NODE_NOT_FOUND),
            "Display for NodeNotFound must contain {E_NODE_NOT_FOUND}, got: {s}"
        );
    }

    // ── invalid_budget_displays_with_error_code ───────────────────────────
    // Spec: E_INVALID_BUDGET maps to ContextError::InvalidBudget.
    #[test]
    fn invalid_budget_displays_with_error_code() {
        let err = ContextError::InvalidBudget;
        let s = err.to_string();
        assert!(
            s.contains(E_INVALID_BUDGET),
            "Display for InvalidBudget must contain {E_INVALID_BUDGET}, got: {s}"
        );
    }

    // ── codec_displays_with_error_code_and_message ────────────────────────
    // Spec: E_CODEC maps to ContextError::Codec with an inner message.
    #[test]
    fn codec_displays_with_error_code_and_message() {
        let err = ContextError::Codec("serialization failed".to_string());
        let s = err.to_string();
        assert!(
            s.contains(E_CODEC),
            "Display for Codec must contain {E_CODEC}, got: {s}"
        );
        assert!(
            s.contains("serialization failed"),
            "Display for Codec must preserve inner message, got: {s}"
        );
    }

    // ── TRIANGULATE: variants_are_distinct ───────────────────────────────
    // Different variants must not compare as equal.
    #[test]
    fn variants_are_distinct() {
        assert_ne!(ContextError::Stale, ContextError::NodeNotFound);
        assert_ne!(ContextError::NodeNotFound, ContextError::InvalidBudget);
        assert_ne!(
            ContextError::Codec("a".to_string()),
            ContextError::Codec("b".to_string())
        );
    }
}
