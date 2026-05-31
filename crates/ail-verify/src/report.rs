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
// `VerificationReport` gained additive optional fields:
// - `proof_obligations` — first-class obligation ledger entries
// - `solver_diagnostics` — structured timeout/resource/unsupported solver outcomes
// - `degradation_events` — every state downgrade with reason and repair options
// - `artifact_hashes`   — artifact hash entries for codegen consistency
//
// # Wave 8A extension
//
// `verified_profile` was added so hash-addressed reports carry the profile used
// at `ail verify` time without requiring the sidecar index.
//
// All new fields use `serde(default)` so older CBOR/JSON without them still
// deserializes cleanly.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::diagnostic::{Diagnostic, DiagnosticSeverity};
use crate::policy::{ApprovalRecord, PolicyAudit, PolicyDecision, StructuralDiff};
use crate::proof::ObligationLedgerEntry;

/// Stable verifier diagnostic code for a mismatch between the requested
/// verification profile and the policy profile gate that was evaluated.
///
/// This is blocking because a weaker `ProfileGate` can otherwise silently
/// downgrade the policy portion of a stricter verification run.
pub const VERIFY_PROFILE_RULE_MISMATCH: &str = "VERIFY_PROFILE_RULE_MISMATCH";

/// Stable verifier diagnostic code for a solver timeout or external solver budget expiry.
pub const VERIFY_SOLVER_TIMEOUT: &str = "VERIFY_SOLVER_TIMEOUT";

/// Stable verifier diagnostic code for solver memory/search/resource exhaustion.
pub const VERIFY_SOLVER_RESOURCE_LIMITED: &str = "VERIFY_SOLVER_RESOURCE_LIMITED";

/// Stable verifier diagnostic code for predicates outside the supported solver fragment.
pub const VERIFY_SOLVER_UNSUPPORTED: &str = "VERIFY_SOLVER_UNSUPPORTED";

// Re-export PolicyAudit sub-types so callers can import from `report` module.
pub use crate::policy::PolicyAuditEntry;

// ── AssumptionState ───────────────────────────────────────────────────────

/// Lifecycle state of a documented assumption.
///
/// Every `Assumed` verification claim must be backed by an explicit
/// `assumption` declaration that tracks its lifecycle.  The six states
/// correspond to the `assumption lifecycle` section of `verification.md`:
///
/// ```text
/// proposed → approved → active
///                    ↘ expired
///                    ↘ revoked
///                    ↘ failed_review
/// ```
///
/// # Policy implications (verification.md §Assumption lifecycle)
///
/// If an assumption reaches `Expired`, `Revoked`, or `FailedReview`, any
/// `prod`/`critical` build that depends on it is blocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssumptionState {
    /// The assumption has been proposed but not yet reviewed.
    Proposed,
    /// The assumption has been reviewed and approved.
    Approved,
    /// The assumption is currently active and in use.
    Active,
    /// The assumption has passed its expiry date or review window.
    Expired,
    /// The assumption has been explicitly revoked by an owner or reviewer.
    Revoked,
    /// The assumption failed a scheduled review cycle.
    FailedReview,
}

impl AssumptionState {
    /// Return `true` if this state is still considered valid for production use.
    ///
    /// `Proposed` and `Approved` are valid (not yet active but not expired).
    /// `Active` is valid.  `Expired`, `Revoked`, `FailedReview` are invalid.
    pub fn is_valid_for_prod(self) -> bool {
        matches!(
            self,
            AssumptionState::Proposed | AssumptionState::Approved | AssumptionState::Active
        )
    }
}

impl std::fmt::Display for AssumptionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssumptionState::Proposed => write!(f, "proposed"),
            AssumptionState::Approved => write!(f, "approved"),
            AssumptionState::Active => write!(f, "active"),
            AssumptionState::Expired => write!(f, "expired"),
            AssumptionState::Revoked => write!(f, "revoked"),
            AssumptionState::FailedReview => write!(f, "failed_review"),
        }
    }
}

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
    /// Whether this entry blocks pipeline progression.
    ///
    /// `true` for `Failed` and `Unsafe` entries; `false` for all others.
    /// Uses `serde(default)` so older reports without this field still
    /// deserialize cleanly (absent → `false`).
    #[serde(default)]
    pub blocking: bool,
    /// Ordered list of actionable repair suggestions for this entry.
    ///
    /// Mirrors the `repair_options` field documented for each verification
    /// entry in `verification.md §Forma de cada entry`.
    /// Empty if no automated repairs are available.
    /// Uses `serde(default)` so older reports without this field still
    /// deserialize cleanly (absent → `[]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repair_options: Vec<String>,
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

// ── SolverDiagnostic ─────────────────────────────────────────────────────

/// Stable solver diagnostic status exposed in verification reports.
///
/// These names are part of the serialized report contract.  Keep the serde
/// representation snake_case so report JSON does not expose Rust enum variant
/// names such as `ResourceLimited`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolverDiagnosticStatus {
    /// Solver attempt exceeded its configured or external time budget.
    Timeout,
    /// Solver attempt exhausted a configured or external resource budget.
    ResourceLimited,
    /// Solver cannot handle the obligation's predicate language or shape.
    Unsupported,
}

impl SolverDiagnosticStatus {
    /// Stable status string used by docs and repair tooling.
    pub fn as_str(self) -> &'static str {
        match self {
            SolverDiagnosticStatus::Timeout => "timeout",
            SolverDiagnosticStatus::ResourceLimited => "resource_limited",
            SolverDiagnosticStatus::Unsupported => "unsupported",
        }
    }

    /// Stable issue code used by policy gates, report consumers, and CI tooling.
    pub fn issue_code(self) -> &'static str {
        match self {
            SolverDiagnosticStatus::Timeout => VERIFY_SOLVER_TIMEOUT,
            SolverDiagnosticStatus::ResourceLimited => VERIFY_SOLVER_RESOURCE_LIMITED,
            SolverDiagnosticStatus::Unsupported => VERIFY_SOLVER_UNSUPPORTED,
        }
    }
}

/// Structured diagnostic derived from proof-obligation solver attempts.
///
/// The current ledger stores solver details as stable attempt outcomes plus
/// optional free-form evidence.  Classification is intentionally conservative:
/// it recognizes explicit solver-attempt statuses and stable solver-scoped
/// prefixes (`solver_timeout:`, `solver_resource_limited:`,
/// `solver_unsupported:`).  Other prose remains unclassified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolverDiagnostic {
    /// Stable machine-readable verifier issue code for this solver outcome.
    ///
    /// Older serialized reports did not include this field; defaulting keeps
    /// those reports readable while new pipeline output always populates it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    /// Stable proof obligation id this diagnostic describes.
    pub obligation_id: String,
    /// Verification stage that generated the obligation.
    pub source_stage: String,
    /// Stable machine-readable status.
    pub status: SolverDiagnosticStatus,
    /// Human-readable solver reason or evidence.
    pub reason: String,
    /// Ordered list of actionable repair suggestions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repair_options: Vec<String>,
}

impl SolverDiagnostic {
    /// Build a report diagnostic from one proof-obligation ledger entry.
    pub fn from_ledger_entry(entry: &ObligationLedgerEntry) -> Option<Self> {
        let status = solver_diagnostic_status_from_ledger(entry)?;
        let reason = solver_diagnostic_reason(entry, status)
            .unwrap_or_else(|| format!("solver outcome classified as {}", status.as_str()));
        let mut repair_options = solver_diagnostic_repair_options(status);
        for option in &entry.repair_options {
            if !repair_options.contains(option) {
                repair_options.push(option.clone());
            }
        }

        Some(Self {
            code: status.issue_code().to_string(),
            obligation_id: entry.id.clone(),
            source_stage: entry.source_stage.clone(),
            status,
            reason,
            repair_options,
        })
    }
}

/// Classify stable solver-scoped reason prefixes into report statuses.
pub fn solver_diagnostic_status_from_reason(reason: &str) -> Option<SolverDiagnosticStatus> {
    let normalized = reason.trim().to_ascii_lowercase();
    if normalized.starts_with("solver_timeout:") {
        return Some(SolverDiagnosticStatus::Timeout);
    }
    if normalized.starts_with("solver_resource_limited:") {
        return Some(SolverDiagnosticStatus::ResourceLimited);
    }
    if normalized.starts_with("solver_unsupported:") {
        return Some(SolverDiagnosticStatus::Unsupported);
    }
    None
}

fn solver_diagnostic_status_from_attempt_outcome(outcome: &str) -> Option<SolverDiagnosticStatus> {
    let normalized = outcome.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "timeout" => Some(SolverDiagnosticStatus::Timeout),
        "resource_limited" => Some(SolverDiagnosticStatus::ResourceLimited),
        "unsupported" => Some(SolverDiagnosticStatus::Unsupported),
        _ => solver_diagnostic_status_from_reason(outcome),
    }
}

fn solver_diagnostic_status_from_ledger(
    entry: &ObligationLedgerEntry,
) -> Option<SolverDiagnosticStatus> {
    for attempt in entry
        .attempts
        .iter()
        .filter(|attempt| attempt.stage == "solver")
    {
        if let Some(status) = solver_diagnostic_status_from_attempt_outcome(&attempt.outcome) {
            return Some(status);
        }
        if let Some(status) = attempt
            .evidence
            .as_deref()
            .and_then(solver_diagnostic_status_from_reason)
        {
            return Some(status);
        }
    }

    None
}

fn solver_diagnostic_reason(
    entry: &ObligationLedgerEntry,
    status: SolverDiagnosticStatus,
) -> Option<String> {
    for attempt in entry
        .attempts
        .iter()
        .filter(|attempt| attempt.stage == "solver")
    {
        if solver_diagnostic_status_from_attempt_outcome(&attempt.outcome) == Some(status) {
            return attempt
                .evidence
                .clone()
                .or_else(|| Some(attempt.outcome.clone()));
        }
        if attempt
            .evidence
            .as_deref()
            .and_then(solver_diagnostic_status_from_reason)
            == Some(status)
        {
            return attempt.evidence.clone();
        }
    }

    None
}

fn solver_diagnostic_repair_options(status: SolverDiagnosticStatus) -> Vec<String> {
    match status {
        SolverDiagnosticStatus::Timeout => vec![
            "simplify the predicate or split it into smaller obligations".into(),
            "provide a narrower precondition or invariant for the solver".into(),
            "add a runtime check when static proof is not practical".into(),
        ],
        SolverDiagnosticStatus::ResourceLimited => vec![
            "reduce solver search space with stronger local facts".into(),
            "split the proof obligation into lower-cost predicates".into(),
            "add a runtime check or explicit assumption with policy approval".into(),
        ],
        SolverDiagnosticStatus::Unsupported => vec![
            "rewrite the predicate into the supported solver fragment".into(),
            "add a runtime check for the unsupported condition".into(),
            "record an explicit assumption when the boundary is policy-approved".into(),
        ],
    }
}

// ── ProfileDiagnostic ────────────────────────────────────────────────────

/// Stable machine-readable diagnostic for verifier profile invariants.
///
/// Profile diagnostics cover cross-layer mismatches that are not owned by a
/// single verification entry, for example when the pipeline is requested with
/// `prod` but the policy rules evaluate `ProfileGate("dev")`.  These records
/// are serialized into reports so apply gates and audit tooling do not have to
/// infer profile downgrades from prose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileDiagnostic {
    /// Stable diagnostic code, e.g. [`VERIFY_PROFILE_RULE_MISMATCH`].
    pub code: String,
    /// Profile requested by the verifier pipeline.
    pub requested_profile: String,
    /// Profile used by the policy gate.
    pub policy_profile: String,
    /// Human-readable explanation for humans and logs.
    pub message: String,
    /// Whether this diagnostic must block acceptance.
    pub blocking: bool,
}

impl ProfileDiagnostic {
    /// Construct a blocking profile-rule mismatch diagnostic.
    pub fn rule_mismatch(requested_profile: &str, policy_profile: &str) -> Self {
        Self {
            code: VERIFY_PROFILE_RULE_MISMATCH.to_string(),
            requested_profile: requested_profile.to_string(),
            policy_profile: policy_profile.to_string(),
            message: format!(
                "verification profile '{requested_profile}' does not match policy gate profile \
                 '{policy_profile}'"
            ),
            blocking: true,
        }
    }
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
/// Additive fields were added for the verification-pipeline change:
/// - `proof_obligations` — first-class obligation ledger entries from the proof pipeline.
/// - `degradation_events` — every recorded state downgrade.
/// - `artifact_hashes` — artifact hash entries for codegen consistency checking.
/// - `solver_diagnostics` — structured timeout/resource/unsupported solver outcomes.
///
/// These use `serde(default)` so pre-extension reports still deserialize cleanly.
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
    /// Structured diagnostics derived from proof-obligation solver attempts.
    ///
    /// Empty for reports produced before solver diagnostic tracking or when no
    /// solver attempt classifies as timeout, resource-limited, or unsupported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solver_diagnostics: Vec<SolverDiagnostic>,
    /// Stable profile-level diagnostics such as requested-policy profile mismatches.
    ///
    /// Empty for reports without profile invariant violations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile_diagnostics: Vec<ProfileDiagnostic>,

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

    /// Identifier of the base graph snapshot this report was computed against.
    ///
    /// `None` when no base snapshot was provided (e.g. initial creation).
    /// Set from `PipelineContext::base_graph` when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_snapshot: Option<String>,

    /// Identifier of the target graph snapshot this report describes.
    ///
    /// `None` when the target snapshot id was not provided to the pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_snapshot: Option<String>,

    /// Structural diff consumed by the policy engine.
    ///
    /// Stored in the report so auditors can trace policy decisions back to
    /// the structural changes that triggered them.
    /// `None` when no structural diff was provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural_diff: Option<StructuralDiff>,

    /// Approval records that were active during this pipeline run.
    ///
    /// Stored verbatim so the report is self-contained for audit.
    /// Empty when no approvals were provided.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<ApprovalRecord>,

    /// Verification profile used when this report was produced.
    ///
    /// Embedded at `ail verify` time so that hash-addressed report lookup
    /// (`inspect report <hash>`) can surface the profile without requiring
    /// the sidecar index.  The sidecar remains the authoritative source for
    /// the apply gate; this field is a first-class complement.
    ///
    /// `None` for reports persisted before Wave 8A (backward compatible via
    /// `serde(default)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_profile: Option<String>,
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
    /// Canonicalize report collections that commonly appear in CI output.
    ///
    /// `entries` keep their first-seen pipeline order because that order explains
    /// stage chronology, but exact duplicates are removed. Diagnostic-like
    /// collections are sorted by stable machine fields and then deduplicated so
    /// equivalent verifier output serializes consistently across runs.
    pub fn canonicalize_for_ci(&mut self) {
        dedupe_preserve_order(&mut self.entries);
        self.diagnostics.sort_by(cmp_diagnostic);
        self.diagnostics.dedup();
        self.solver_diagnostics.sort_by(cmp_solver_diagnostic);
        self.solver_diagnostics.dedup();
        self.profile_diagnostics.sort_by(cmp_profile_diagnostic);
        self.profile_diagnostics.dedup();
        self.degradation_events.sort_by(cmp_degradation_event);
        self.degradation_events.dedup();
        self.artifact_hashes.sort_by(cmp_artifact_hash);
        self.artifact_hashes.dedup();
        self.summary_counts = SummaryCounts::from_entries(&self.entries);
    }

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

impl SummaryCounts {
    fn from_entries(entries: &[VerificationEntry]) -> Self {
        Self {
            verified_count: entries
                .iter()
                .filter(|e| {
                    e.state == VerificationState::Proven
                        || e.state == VerificationState::RuntimeChecked
                })
                .count(),
            runtime_checked_count: entries
                .iter()
                .filter(|e| e.state == VerificationState::RuntimeChecked)
                .count(),
            assumed_count: entries
                .iter()
                .filter(|e| e.state == VerificationState::Assumed)
                .count(),
            unverified_count: entries
                .iter()
                .filter(|e| e.state == VerificationState::Unverified)
                .count(),
            unsafe_count: entries
                .iter()
                .filter(|e| e.state == VerificationState::Unsafe)
                .count(),
            failed_count: entries
                .iter()
                .filter(|e| e.state == VerificationState::Failed)
                .count(),
        }
    }
}

fn dedupe_preserve_order<T: PartialEq>(items: &mut Vec<T>) {
    let mut deduped = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        if !deduped.contains(&item) {
            deduped.push(item);
        }
    }
    *items = deduped;
}

fn cmp_diagnostic(a: &Diagnostic, b: &Diagnostic) -> Ordering {
    severity_rank(a.severity)
        .cmp(&severity_rank(b.severity))
        .then_with(|| a.code.cmp(&b.code))
        .then_with(|| a.target.cmp(&b.target))
        .then_with(|| a.blocking.cmp(&b.blocking).reverse())
        .then_with(|| a.evidence.cmp(&b.evidence))
        .then_with(|| a.expected.cmp(&b.expected))
        .then_with(|| a.actual.cmp(&b.actual))
        .then_with(|| format!("{:?}", a.repair_options).cmp(&format!("{:?}", b.repair_options)))
}

fn severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
    }
}

fn cmp_solver_diagnostic(a: &SolverDiagnostic, b: &SolverDiagnostic) -> Ordering {
    a.code
        .cmp(&b.code)
        .then_with(|| a.obligation_id.cmp(&b.obligation_id))
        .then_with(|| a.source_stage.cmp(&b.source_stage))
        .then_with(|| solver_status_rank(a.status).cmp(&solver_status_rank(b.status)))
        .then_with(|| a.reason.cmp(&b.reason))
        .then_with(|| a.repair_options.cmp(&b.repair_options))
}

fn solver_status_rank(status: SolverDiagnosticStatus) -> u8 {
    match status {
        SolverDiagnosticStatus::Timeout => 0,
        SolverDiagnosticStatus::ResourceLimited => 1,
        SolverDiagnosticStatus::Unsupported => 2,
    }
}

fn cmp_profile_diagnostic(a: &ProfileDiagnostic, b: &ProfileDiagnostic) -> Ordering {
    a.code
        .cmp(&b.code)
        .then_with(|| a.requested_profile.cmp(&b.requested_profile))
        .then_with(|| a.policy_profile.cmp(&b.policy_profile))
        .then_with(|| a.blocking.cmp(&b.blocking).reverse())
        .then_with(|| a.message.cmp(&b.message))
}

fn cmp_degradation_event(a: &DegradationEvent, b: &DegradationEvent) -> Ordering {
    a.obligation_id
        .cmp(&b.obligation_id)
        .then_with(|| a.source_stage.cmp(&b.source_stage))
        .then_with(|| a.from_state.priority().cmp(&b.from_state.priority()))
        .then_with(|| a.to_state.priority().cmp(&b.to_state.priority()))
        .then_with(|| a.reason.cmp(&b.reason))
        .then_with(|| a.repair_options.cmp(&b.repair_options))
}

fn cmp_artifact_hash(a: &ArtifactHash, b: &ArtifactHash) -> Ordering {
    a.artifact
        .cmp(&b.artifact)
        .then_with(|| a.hash.cmp(&b.hash))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // TASK-01: RED — blocking field on VerificationEntry

    #[test]
    fn verification_entry_blocking_defaults_to_false() {
        let entry = VerificationEntry {
            claim: "test-claim".into(),
            state: VerificationState::Proven,
            scope: "scope".into(),
            evidence: None,
            blocking: false,
            repair_options: vec![],
        };
        assert!(!entry.blocking, "blocking must default to false for Proven");
    }

    #[test]
    fn verification_entry_blocking_true_for_failed() {
        let entry = VerificationEntry {
            claim: "test-claim".into(),
            state: VerificationState::Failed,
            scope: "scope".into(),
            evidence: Some("E_TEST".into()),
            blocking: true,
            repair_options: vec![],
        };
        assert!(entry.blocking, "Failed entries must have blocking=true");
    }

    #[test]
    fn verification_entry_blocking_true_for_unsafe() {
        let entry = VerificationEntry {
            claim: "test-claim".into(),
            state: VerificationState::Unsafe,
            scope: "scope".into(),
            evidence: None,
            blocking: true,
            repair_options: vec![],
        };
        assert!(entry.blocking, "Unsafe entries must have blocking=true");
    }

    #[test]
    fn verification_entry_blocking_serialization_roundtrip() {
        let entry = VerificationEntry {
            claim: "roundtrip".into(),
            state: VerificationState::Failed,
            scope: "s".into(),
            evidence: None,
            blocking: true,
            repair_options: vec![],
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(
            json.contains("blocking"),
            "blocking field must appear in JSON"
        );
        let decoded: VerificationEntry = serde_json::from_str(&json).expect("deserialize");
        assert!(decoded.blocking);
    }

    #[test]
    fn verification_entry_blocking_false_roundtrip_with_serde_default() {
        // Simulate old JSON without blocking field — serde default must supply false
        let json = r#"{"claim":"c","state":"Proven","scope":"s"}"#;
        let decoded: VerificationEntry =
            serde_json::from_str(json).expect("deserialize without blocking field");
        assert!(
            !decoded.blocking,
            "absent blocking field must default to false"
        );
    }

    // ── AssumptionState tests ─────────────────────────────────────────────

    #[test]
    fn assumption_state_six_variants_constructible() {
        let states = [
            AssumptionState::Proposed,
            AssumptionState::Approved,
            AssumptionState::Active,
            AssumptionState::Expired,
            AssumptionState::Revoked,
            AssumptionState::FailedReview,
        ];
        assert_eq!(states.len(), 6);
    }

    #[test]
    fn assumption_state_is_valid_for_prod_lifecycle() {
        assert!(AssumptionState::Proposed.is_valid_for_prod());
        assert!(AssumptionState::Approved.is_valid_for_prod());
        assert!(AssumptionState::Active.is_valid_for_prod());
        assert!(!AssumptionState::Expired.is_valid_for_prod());
        assert!(!AssumptionState::Revoked.is_valid_for_prod());
        assert!(!AssumptionState::FailedReview.is_valid_for_prod());
    }

    #[test]
    fn assumption_state_display_matches_doc_names() {
        assert_eq!(AssumptionState::Proposed.to_string(), "proposed");
        assert_eq!(AssumptionState::Approved.to_string(), "approved");
        assert_eq!(AssumptionState::Active.to_string(), "active");
        assert_eq!(AssumptionState::Expired.to_string(), "expired");
        assert_eq!(AssumptionState::Revoked.to_string(), "revoked");
        assert_eq!(AssumptionState::FailedReview.to_string(), "failed_review");
    }

    #[test]
    fn assumption_state_roundtrips_json() {
        let state = AssumptionState::FailedReview;
        let json = serde_json::to_string(&state).expect("serialize");
        let decoded: AssumptionState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, state);
    }

    // ── repair_options tests ──────────────────────────────────────────────

    #[test]
    fn verification_entry_repair_options_defaults_to_empty() {
        let entry = VerificationEntry {
            claim: "test".into(),
            state: VerificationState::Failed,
            scope: "s".into(),
            evidence: None,
            blocking: true,
            repair_options: vec![],
        };
        assert!(entry.repair_options.is_empty());
    }

    #[test]
    fn verification_entry_repair_options_serialized_when_non_empty() {
        let entry = VerificationEntry {
            claim: "test".into(),
            state: VerificationState::Failed,
            scope: "s".into(),
            evidence: None,
            blocking: true,
            repair_options: vec!["add_guard".into(), "add_runtime_check".into()],
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(
            json.contains("repair_options"),
            "non-empty repair_options must appear in JSON"
        );
        assert!(json.contains("add_guard"));
    }

    #[test]
    fn verification_entry_repair_options_absent_when_empty() {
        let entry = VerificationEntry {
            claim: "test".into(),
            state: VerificationState::Proven,
            scope: "s".into(),
            evidence: None,
            blocking: false,
            repair_options: vec![],
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(
            !json.contains("repair_options"),
            "empty repair_options must be skipped in JSON output"
        );
    }

    #[test]
    fn verification_entry_repair_options_deserializes_without_field() {
        let json = r#"{"claim":"c","state":"Proven","scope":"s"}"#;
        let decoded: VerificationEntry = serde_json::from_str(json).expect("deserialize");
        assert!(
            decoded.repair_options.is_empty(),
            "absent field must default to empty vec"
        );
    }
}
