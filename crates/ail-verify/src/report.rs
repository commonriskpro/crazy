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
//
// # G25 extensions (verification-pipeline)
//
// `VerificationReport` gained three additive optional fields:
// - `proof_obligations` — first-class obligation ledger entries
// - `degradation_events` — every state downgrade with reason and repair options
// - `artifact_hashes`   — artifact hash entries for codegen consistency
//
// All new fields use `serde(default)` so older CBOR/JSON without them still
// deserializes cleanly.

use serde::{Deserialize, Serialize};

use crate::diagnostic::Diagnostic;
use crate::policy::{PolicyAudit, PolicyDecision};
use crate::proof::ObligationLedgerEntry;

// Re-export PolicyAudit sub-types so callers can import from `report` module.
pub use crate::policy::PolicyAuditEntry;

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

// ── ArtifactHash ─────────────────────────────────────────────────────────

/// One artifact hash entry for codegen consistency verification.
///
/// Pairs an artifact name (e.g. `"canonical_change"`, `"core_ir"`, `"wasm"`)
/// with its expected or actual content-addressed hash.  The verifier compares
/// these against computed hashes to confirm generated artifacts match the
/// verified IR.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ArtifactHash {
    /// Artifact name identifier (e.g. `"canonical_change"`, `"core_ir"`, `"anf_ir"`,
    /// `"wasm"`, `"capabilities_manifest"`, `"verification_report"`).
    pub artifact: String,
    /// Content-addressed hash (hex-encoded), or empty string if not yet computed.
    pub hash: String,
}

// ── DegradationEvent ─────────────────────────────────────────────────────

/// A recorded state downgrade during proof obligation resolution.
///
/// Every time an obligation is permitted to degrade from a higher-confidence
/// state to a lower-confidence one (e.g. `Proven → Assumed`), a
/// `DegradationEvent` is recorded in the report.  The event captures:
/// - which obligation was downgraded (`obligation_id`)
/// - which pipeline stage downgraded it (`source_stage`)
/// - what state it came from and went to
/// - the policy or boundary that allowed the downgrade (`reason`)
/// - actionable repair options (e.g. `"add_runtime_check"`)
///
/// `DegradationEvent` is additive — reports produced before this type was
/// added deserialize with `degradation_events: []` (via `serde(default)`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DegradationEvent {
    /// Stable identifier of the proof obligation that was downgraded.
    pub obligation_id: String,
    /// Pipeline stage that performed the downgrade
    /// (e.g. `"resource"`, `"boundary"`, `"policy"`, `"contract"`).
    pub source_stage: String,
    /// The higher-confidence state before downgrade.
    pub from_state: VerificationState,
    /// The lower-confidence state after downgrade.
    pub to_state: VerificationState,
    /// Human-readable explanation of why the downgrade was permitted.
    pub reason: String,
    /// Ordered list of actionable repair suggestions.
    ///
    /// Empty if no automated repairs are available.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repair_options: Vec<String>,
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
///
/// `policy_decision` is set by the caller after running `PolicyEngine::evaluate`.
/// It is absent (`None`) in reports that have not yet gone through the policy layer.
///
/// # G25 extensions
///
/// Three additive fields were added for the verification-pipeline change:
/// - `proof_obligations` — first-class obligation ledger entries from the proof pipeline.
/// - `degradation_events` — every recorded state downgrade.
/// - `artifact_hashes` — artifact hash entries for codegen consistency checking.
///
/// All three use `serde(default)` so pre-G25 reports still deserialize cleanly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    /// Verification entries in graph traversal order.
    pub entries: Vec<VerificationEntry>,
    /// Structured diagnostics emitted for violated or degraded conditions.
    pub diagnostics: Vec<Diagnostic>,
    /// Schema version for this report format.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub schema_version: String,
    /// Aggregated counts by verification state.
    #[serde(default)]
    pub summary_counts: SummaryCounts,
    /// Policy engine decision for this report.
    ///
    /// `None` means the policy layer has not yet been applied.
    /// `Some(decision)` contains the result of `PolicyEngine::evaluate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision: Option<PolicyDecision>,
    /// Per-entry policy audit trail produced by `PolicyEngine::evaluate_with_audit`.
    ///
    /// Records the profile used, per-entry gate decisions, and approval scopes
    /// consulted.  `None` means no audit was requested or stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_audit: Option<PolicyAudit>,
    /// First-class proof obligation ledger entries from the proof pipeline.
    ///
    /// Each entry tracks identity, source stage, resolution attempts, and
    /// the degradation path for one proof obligation.  Empty for reports
    /// produced before the obligation ledger was introduced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proof_obligations: Vec<ObligationLedgerEntry>,
    /// Recorded degradation events: every state downgrade with reason and repair options.
    ///
    /// Empty for reports produced before degradation tracking was introduced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradation_events: Vec<DegradationEvent>,
    /// Artifact hash entries for codegen consistency checking.
    ///
    /// Pairs artifact names with their expected or actual content hashes.
    /// Empty for reports that have not gone through the codegen checker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_hashes: Vec<ArtifactHash>,
}

/// Aggregated counts of entries by verification state.
#[allow(clippy::struct_field_names)]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryCounts {
    pub verified_count: usize,
    pub runtime_checked_count: usize,
    pub assumed_count: usize,
    pub unverified_count: usize,
    pub unsafe_count: usize,
    pub failed_count: usize,
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
