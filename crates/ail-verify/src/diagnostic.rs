// ── ail-verify::diagnostic ────────────────────────────────────────────────
//
// Structured diagnostic type with error code, severity, target, evidence,
// expected/actual values, repair options, and a blocking flag.
//
// # Design
//
// `Diagnostic` replaces the role of plain error enums/strings as the
// primary output of verification failures.  Every diagnostic carries:
//   - a stable error code constant (E_xxx),
//   - a severity classification,
//   - a `NodeRef` target identifying the graph node involved,
//   - optional evidence, expected, and actual value strings,
//   - zero or more `RepairOption` items describing actionable fixes,
//   - a `blocking` flag indicating whether the diagnostic prevents acceptance.
//
// # Repair options
//
// `RepairOption::DirectOp`   — a single `ChangeSetOp` the toolchain can apply.
// `RepairOption::Choice`     — several `ChangeSetOp`s; the user selects one.
// `RepairOption::Migration`  — a free-text description of a migration path.
// `RepairOption::Approval`   — an explanation of what approval is required.
// `RepairOption::Explanation` — a plain-text clarification (no automated fix).

use ail_change::model::ChangeSetOp;
use ail_core::semantic_graph::NodeRef;
use serde::{Deserialize, Serialize};

// ── Error code constants ───────────────────────────────────────────────────

/// Type mismatch: declared type does not match inferred or expected type.
pub const E_TYPE_MISMATCH: &str = "E_TYPE_MISMATCH";

/// An effect is used in a function body but not declared in its effect row.
pub const E_EFFECT_UNDECLARED: &str = "E_EFFECT_UNDECLARED";

/// An effect is declared in a function's effect row but never used or propagated.
pub const E_EFFECT_UNUSED: &str = "E_EFFECT_UNUSED";

/// A refinement predicate could not be proven by the solver or runtime check.
pub const E_REFINEMENT_NOT_PROVEN: &str = "E_REFINEMENT_NOT_PROVEN";

/// A contract clause (requires/ensures/invariant) is violated.
pub const E_CONTRACT_VIOLATED: &str = "E_CONTRACT_VIOLATED";

/// A capability is required by the node but not granted by the active profile.
pub const E_CAPABILITY_DENIED: &str = "E_CAPABILITY_DENIED";

/// The changeset targets a snapshot that is no longer the current base.
pub const E_STALE_BASE: &str = "E_STALE_BASE";

// ── DiagnosticSeverity ────────────────────────────────────────────────────

/// Severity classification of a `Diagnostic`.
///
/// `Error` diagnostics always set `blocking = true`.
/// `Warning` and `Info` diagnostics may or may not block depending on profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    /// A condition that must be resolved; blocks acceptance when `blocking = true`.
    Error,
    /// A condition that is undesirable but does not necessarily block.
    Warning,
    /// An informational note with no blocking implication.
    Info,
}

// ── RepairOption ──────────────────────────────────────────────────────────

/// An actionable or explanatory repair suggestion attached to a `Diagnostic`.
///
/// Variants are intentionally non-exhaustive in spirit — new variants must be
/// added here and handled by all downstream consumers (exhaustive match will
/// cause compile errors, which is intentional).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RepairOption {
    /// A single graph operation the toolchain can apply directly.
    DirectOp(ChangeSetOp),
    /// A set of graph operations; the user or toolchain selects one.
    Choice(Vec<ChangeSetOp>),
    /// A free-text description of a migration path that requires manual steps.
    Migration(String),
    /// An explanation of what approval is required before the change can proceed.
    Approval(String),
    /// A plain-text clarification; no automated fix is available.
    Explanation(String),
}

// ── Diagnostic ────────────────────────────────────────────────────────────

/// A structured verification diagnostic emitted by `Checker` or
/// `ContractChecker` when a verification condition is violated or degraded.
///
/// Every diagnostic carries a stable `code` string (one of the `E_xxx`
/// constants in this module) so that tooling can identify and route it
/// without guessing semantics from the message text.
///
/// # Serde note
///
/// `code` is stored as `String` so that `Diagnostic` can round-trip through
/// CBOR/JSON without lifetime constraints.  Use the `E_xxx` constants when
/// constructing diagnostics to ensure code stability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Stable error code identifying the kind of violation (e.g. `"E_TYPE_MISMATCH"`).
    pub code: String,
    /// How severe the violation is.
    pub severity: DiagnosticSeverity,
    /// The graph node involved in this diagnostic.
    pub target: NodeRef,
    /// Supporting evidence describing what was found (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// What the verifier expected to find (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// What the verifier actually found (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    /// Zero or more suggestions for resolving the diagnostic.
    pub repair_options: Vec<RepairOption>,
    /// Whether this diagnostic prevents the changeset from being accepted.
    pub blocking: bool,
}

impl Diagnostic {
    /// Construct a minimal blocking `Error`-severity diagnostic with no
    /// evidence, expected, actual, or repair options.
    ///
    /// Pass one of the `E_xxx` constants for `code`.  Use builder helpers
    /// (`with_evidence`, `with_expected`, `with_actual`, `with_repair`) to
    /// attach optional context after construction.
    pub fn error(code: &'static str, target: NodeRef) -> Self {
        Self {
            code: code.to_string(),
            severity: DiagnosticSeverity::Error,
            target,
            evidence: None,
            expected: None,
            actual: None,
            repair_options: vec![],
            blocking: true,
        }
    }

    /// Construct a non-blocking `Warning`-severity diagnostic.
    ///
    /// Pass one of the `E_xxx` constants for `code`.
    pub fn warning(code: &'static str, target: NodeRef) -> Self {
        Self {
            code: code.to_string(),
            severity: DiagnosticSeverity::Warning,
            target,
            evidence: None,
            expected: None,
            actual: None,
            repair_options: vec![],
            blocking: false,
        }
    }

    /// Attach evidence text to this diagnostic (builder helper).
    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
    }

    /// Attach an expected value description (builder helper).
    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    /// Attach an actual value description (builder helper).
    pub fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    /// Append a repair option (builder helper).
    pub fn with_repair(mut self, option: RepairOption) -> Self {
        self.repair_options.push(option);
        self
    }
}
