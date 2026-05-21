// ── ail-verify::report ────────────────────────────────────────────────────
//
// Pure value types for the verification report layer.
//
// # Six verification states
//
// | State          | Meaning                                                  |
// |----------------|----------------------------------------------------------|
// | `Proven`       | Directly established by declared nominal type facts.     |
// | `RuntimeChecked` | Validated by a runtime assertion (future phase).       |
// | `Assumed`      | Declared by the programmer (effect/capability present).  |
// | `Unverified`   | No fact declared; status unknown.                        |
// | `Unsafe`       | Explicitly marked as unsafe / unsound.                   |
// | `Failed`       | A verification condition is violated.                    |
//
// # Summary priority
//
// `Failed > Unsafe > Unverified > Assumed > RuntimeChecked > Proven`
//
// An empty report has vacuous summary `Proven`.

use serde::{Deserialize, Serialize};

use crate::diagnostic::Diagnostic;

// ── VerificationState ─────────────────────────────────────────────────────

/// The result of checking one claim in a `VerificationEntry`.
///
/// Exactly six variants are permitted in Phase 5.  Exhaustive matches
/// elsewhere in the codebase will break if variants are added, which is
/// intentional — it forces callers to handle new states explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationState {
    /// Claim directly established via declared nominal type facts.
    Proven,
    /// Claim validated by a runtime assertion (reserved for future phase).
    RuntimeChecked,
    /// Claim declared by the programmer; correctness not yet mechanically proven.
    Assumed,
    /// No fact was declared; status unknown.
    Unverified,
    /// Explicitly marked as unsafe or potentially unsound.
    Unsafe,
    /// A verification condition is known to be violated.
    Failed,
}

impl VerificationState {
    /// Numeric priority used by `VerificationReport::summary()`.
    ///
    /// Higher value = higher severity.  `Proven` has priority 0 (lowest).
    fn priority(self) -> u8 {
        match self {
            VerificationState::Proven => 0,
            VerificationState::RuntimeChecked => 1,
            VerificationState::Assumed => 2,
            VerificationState::Unverified => 3,
            VerificationState::Unsafe => 4,
            VerificationState::Failed => 5,
        }
    }
}

// ── VerificationEntry ─────────────────────────────────────────────────────

/// One verification claim for a single fact in a `GraphNode`.
///
/// `evidence` is `Option<String>` so that absent evidence is omitted from
/// CBOR output rather than serialized as an empty string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationEntry {
    /// Human-readable description of the claim being checked.
    pub claim: String,
    /// Verification result for this claim.
    pub state: VerificationState,
    /// Scope identifier (typically a node name or id).
    pub scope: String,
    /// Optional supporting evidence or rationale.
    ///
    /// Serialized with `skip_serializing_if` so that `None` is absent
    /// (not `null` or `""`) in CBOR output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

// ── VerificationReport ────────────────────────────────────────────────────

/// Ordered collection of verification entries and structured diagnostics for
/// one `SemanticGraph` pass.
///
/// Iteration order is preserved from the graph traversal, guaranteeing
/// deterministic output across runs.
///
/// `diagnostics` is populated by `Checker` and `ContractChecker` alongside
/// `entries` when verification conditions are violated.  An empty
/// `diagnostics` vec means no structured violations were found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    /// Verification entries in graph traversal order.
    pub entries: Vec<VerificationEntry>,
    /// Structured diagnostics emitted for violated or degraded conditions.
    pub diagnostics: Vec<Diagnostic>,
}

impl VerificationReport {
    /// Returns the highest-severity `VerificationState` present in the report.
    ///
    /// An empty report returns `Proven` (vacuous truth — nothing has failed).
    ///
    /// Priority order (descending): `Failed > Unsafe > Unverified > Assumed >
    /// RuntimeChecked > Proven`.
    pub fn summary(&self) -> VerificationState {
        self.entries
            .iter()
            .map(|e| e.state)
            .max_by_key(|s| s.priority())
            .unwrap_or(VerificationState::Proven)
    }
}
