// ── ail-verify::policy ────────────────────────────────────────────────────
//
// Policy engine — verification layer 11 per verification.md.
//
// # Responsibility
//
// `PolicyEngine` takes a `PolicyInput` (VerificationReport + rules +
// approval records + context) and returns a `PolicyDecision`:
//   - `Passed`                      — all rules satisfied, no warnings.
//   - `PassedWithWarnings(warnings)` — passed but some entries need attention.
//   - `Failed(violations)`          — one or more blocking violations found.
//   - `ApprovalRequired(scopes)`    — no outright failure, but explicit
//                                     approval is needed for listed scopes.
//
// # Rules
//
// `NoUnsafe`                         — any `Unsafe` entry without approval → Failed.
// `NoUnverifiedPublicApi`            — any `Unverified` entry in `pub::` → Failed.
// `RequireApproval`                  — any `Unsafe` entry without approval → ApprovalRequired
//                                      (weaker than NoUnsafe; does not hard-block).
// `ProfileGate(name)`                — applies the named profile's blocking matrix.
// `NoPublicApiChangesWithoutApproval` — any public API change without approval → Failed.
//
// # Decision priority
//
// When multiple rules fire, the most severe wins:
//   Failed > ApprovalRequired > PassedWithWarnings > Passed
//
// # Profile matrix (per verification.md)
//
// | Profile  | Proven | RuntimeChecked | Assumed        | Unverified | Unsafe | Failed |
// |----------|--------|----------------|----------------|------------|--------|--------|
// | draft    | pass   | pass           | warn if no ev  | pass+warn  | block  | block  |
// | dev      | pass   | pass           | need boundary  | pub=block  | block  | block  |
// | test     | pass   | pass           | pass (test)    | pass       | block  | block  |
// | staging  | pass   | pass           | need approval  | block      | block  | block  |
// | prod     | pass   | pass           | strong approval| block      | strong | block  |
// | critical | pass   | pass           | strong only    | block      | block  | block  |
// | unknown  | pass   | pass           | strong only    | block      | block  | block  |
//
// # Strict-by-default
//
// Unknown profiles are treated as "critical" (most restrictive) — conservative
// fallback per spec: "Strict by default. Relaxed only by explicit policy/profile."
//
// # Public scope convention
//
// In this phase, "public" means `scope.starts_with("pub::")`.

use serde::{Deserialize, Serialize};

use crate::report::{VerificationReport, VerificationState};

// ── Error code constants ──────────────────────────────────────────────────

/// Policy violation: an `Unsafe` entry exists without a required approval.
pub const POLICY_UNSAFE_BLOCKED: &str = "POLICY_UNSAFE_BLOCKED";

/// Policy violation: a public-scope entry has `Unverified` state.
pub const POLICY_UNVERIFIED_PUBLIC_API: &str = "POLICY_UNVERIFIED_PUBLIC_API";

/// Policy violation: the active profile's gate rejects an entry's state.
pub const POLICY_PROFILE_GATE: &str = "POLICY_PROFILE_GATE";

/// Policy violation code used when approval is required (informational).
pub const POLICY_APPROVAL_REQUIRED: &str = "POLICY_APPROVAL_REQUIRED";

/// Policy violation: an `Assumed` entry has no approval record in a strict profile.
pub const POLICY_ASSUMED_UNAPPROVED: &str = "POLICY_ASSUMED_UNAPPROVED";

/// Policy violation: a critical profile requires Strong approval but only Weak was provided.
pub const POLICY_WEAK_ASSUMPTION: &str = "POLICY_WEAK_ASSUMPTION";

/// Policy violation: a public API was changed without explicit approval.
pub const POLICY_PUBLIC_API_CHANGED: &str = "POLICY_PUBLIC_API_CHANGED";

// ── ApprovalStrength ──────────────────────────────────────────────────────

/// The strength of an approval record.
///
/// Critical profiles only accept `Strong` approvals for `Assumed` entries.
/// `Weak` approvals are sufficient for `dev`/`staging` but not `critical`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStrength {
    /// A formal, reviewed approval (e.g. from a security team or senior reviewer).
    Strong,
    /// A informal or boundary-level approval (e.g. developer self-approved assumption).
    Weak,
}

// ── ApprovalRecord ────────────────────────────────────────────────────────

/// An explicit approval record for a verification entry scope.
///
/// An approval record allows an otherwise-blocking entry to pass a policy
/// rule (e.g. `NoUnsafe` with an approval for "fn.transfer").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    /// The scope string of the `VerificationEntry` this approval covers.
    /// Must exactly match `VerificationEntry::scope`.
    pub scope: String,
    /// Identity of the approver (e.g. team name, person, or policy tool).
    pub approver: String,
    /// Human-readable reason for the approval.
    pub reason: String,
    /// Strength of this approval (Strong or Weak).
    ///
    /// Critical profiles require `Strong` approvals for `Assumed` entries.
    #[serde(default = "default_approval_strength")]
    pub strength: ApprovalStrength,
}

fn default_approval_strength() -> ApprovalStrength {
    ApprovalStrength::Strong
}

// ── PolicyViolation ───────────────────────────────────────────────────────

/// One blocking policy violation discovered during evaluation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyViolation {
    /// Stable error code; one of the `POLICY_xxx` constants.
    pub code: String,
    /// The scope of the `VerificationEntry` that triggered this violation.
    pub scope: String,
    /// Human-readable description of the violation.
    pub message: String,
}

// ── PolicyWarning ─────────────────────────────────────────────────────────

/// A non-blocking policy warning (advisory; does not prevent acceptance).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyWarning {
    /// Stable warning code (e.g. "POLICY_UNVERIFIED_WARN").
    pub code: String,
    /// The scope triggering the warning.
    pub scope: String,
    /// Human-readable description.
    pub message: String,
}

// ── PolicyDecision ────────────────────────────────────────────────────────

/// The outcome of a `PolicyEngine::evaluate` call.
///
/// Priority order (most severe first):
///   `Failed` > `ApprovalRequired` > `PassedWithWarnings` > `Passed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// All rules satisfied; the changeset may proceed with no warnings.
    Passed,
    /// All rules satisfied but some non-blocking warnings were emitted.
    /// The changeset may proceed; warnings are advisory.
    PassedWithWarnings(Vec<PolicyWarning>),
    /// One or more blocking violations; the changeset is rejected.
    Failed(Vec<PolicyViolation>),
    /// No blocking violations, but explicit approval is required for the
    /// listed scopes before the changeset can proceed.
    ApprovalRequired(Vec<String>),
}

// ── PolicyRule ────────────────────────────────────────────────────────────

/// A single policy rule to evaluate against a `VerificationReport`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyRule {
    /// Block any `Unsafe` entry that lacks an explicit `ApprovalRecord`.
    NoUnsafe,
    /// Block any `Unverified` entry whose scope starts with `"pub::"`.
    NoUnverifiedPublicApi,
    /// Return `ApprovalRequired` for any `Unsafe` entry lacking approval.
    ///
    /// Weaker than `NoUnsafe` — surfaces the requirement without hard-blocking.
    /// If `NoUnsafe` is also active, `Failed` takes priority over
    /// `ApprovalRequired`.
    RequireApproval,
    /// Apply the named profile's blocking matrix.
    ///
    /// Profiles: `draft`, `dev`, `test`, `staging`, `prod`, `critical`.
    /// Unknown profile names use conservative/restrictive fallback (like `critical`).
    ProfileGate(String),
    /// Block any public API change that lacks an explicit approval record.
    NoPublicApiChangesWithoutApproval,
}

// ── Context types (PolicyInput extensions) ────────────────────────────────

/// A structural diff entry representing a change to the graph structure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralDiff {
    /// Human-readable summary of the structural change.
    pub description: String,
}

/// A public API change that may require approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicApiChange {
    /// The scope (node/ref) of the API that changed.
    pub scope: String,
    /// Description of what changed.
    pub description: String,
}

/// A capability grant issued to a module or profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    /// The scope being granted the capability.
    pub scope: String,
    /// The capability identifier (e.g. "database.write:Order").
    pub capability: String,
}

/// Package trust metadata for dependency verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageTrustEntry {
    /// Package name.
    pub package: String,
    /// Trust level (e.g. "verified", "audited", "unknown").
    pub trust_level: String,
}

// ── PolicyInput ───────────────────────────────────────────────────────────

/// Input bundle for `PolicyEngine::evaluate`.
pub struct PolicyInput<'a> {
    /// The report whose entries are checked against the rules.
    pub report: &'a VerificationReport,
    /// Ordered list of rules to apply (all are evaluated; results merged).
    pub rules: &'a [PolicyRule],
    /// Explicit approval records that can satisfy policy rules.
    pub approvals: &'a [ApprovalRecord],
    /// Optional structural diff from the canonical change.
    ///
    /// Used by the policy engine to assess risk of structural changes.
    pub structural_diff: Option<&'a StructuralDiff>,
    /// Capability grants active for this changeset evaluation.
    pub capability_grants: &'a [CapabilityGrant],
    /// Public API changes in this changeset.
    pub public_api_changes: &'a [PublicApiChange],
    /// Package trust metadata for dependency checks.
    pub package_trust_metadata: &'a [PackageTrustEntry],
}

// ── PolicyAudit ───────────────────────────────────────────────────────────

/// Audit trail produced by `PolicyEngine::evaluate_with_audit`.
///
/// Records which profile was used, what decisions were made per entry,
/// and which approval scopes were consulted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyAudit {
    /// The profile name used during evaluation.
    pub profile: String,
    /// Per-entry audit records.
    pub entries: Vec<PolicyAuditEntry>,
    /// Approval scopes consulted during evaluation.
    pub approval_scopes_consulted: Vec<String>,
}

/// One entry in the `PolicyAudit` trail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyAuditEntry {
    /// Entry scope identifier.
    pub scope: String,
    /// Entry verification state.
    pub state: String,
    /// The gate decision for this entry ("passed", "failed", "warning", "approval_required").
    pub gate_decision: String,
    /// Approver identity used, if any.
    pub approval_used: Option<String>,
}

// ── PolicyEngine ──────────────────────────────────────────────────────────

/// Stateless policy evaluation engine.
///
/// Call [`PolicyEngine::evaluate`] with a `PolicyInput` to receive a
/// `PolicyDecision`.  All logic is pure — no I/O, no mutations.
pub struct PolicyEngine;

impl PolicyEngine {
    /// Evaluate all rules in `input.rules` against `input.report` and
    /// return the merged `PolicyDecision`.
    ///
    /// # Decision merging
    ///
    /// Each rule is evaluated independently.  Results are merged by priority:
    /// `Failed` (any) > `ApprovalRequired` (any) > `PassedWithWarnings` (any) > `Passed`.
    ///
    /// Violations from all `Failed` rules are collected into a single
    /// `Failed(Vec<PolicyViolation>)`.  Scopes from all `ApprovalRequired`
    /// rules are collected into a single `ApprovalRequired(Vec<String>)`.
    pub fn evaluate(input: &PolicyInput<'_>) -> PolicyDecision {
        let mut all_violations: Vec<PolicyViolation> = Vec::new();
        let mut approval_scopes: Vec<String> = Vec::new();
        let mut all_warnings: Vec<PolicyWarning> = Vec::new();

        for rule in input.rules {
            match Self::evaluate_rule(rule, input) {
                PolicyDecision::Failed(mut violations) => {
                    all_violations.append(&mut violations);
                }
                PolicyDecision::ApprovalRequired(mut scopes) => {
                    approval_scopes.append(&mut scopes);
                }
                PolicyDecision::PassedWithWarnings(mut warnings) => {
                    all_warnings.append(&mut warnings);
                }
                PolicyDecision::Passed => {}
            }
        }

        // Priority: Failed > ApprovalRequired > PassedWithWarnings > Passed
        if !all_violations.is_empty() {
            PolicyDecision::Failed(all_violations)
        } else if !approval_scopes.is_empty() {
            PolicyDecision::ApprovalRequired(approval_scopes)
        } else if !all_warnings.is_empty() {
            PolicyDecision::PassedWithWarnings(all_warnings)
        } else {
            PolicyDecision::Passed
        }
    }

    /// Evaluate all rules and return both the decision and a full audit trail.
    ///
    /// The audit contains per-entry gate decisions and the profile used.
    /// Use this variant when the caller needs to persist the policy/approvals
    /// sections of a `VerificationReport`.
    pub fn evaluate_with_audit(input: &PolicyInput<'_>) -> (PolicyDecision, PolicyAudit) {
        let decision = Self::evaluate(input);

        // Extract profile name from ProfileGate rule if present.
        let profile = input
            .rules
            .iter()
            .find_map(|r| {
                if let PolicyRule::ProfileGate(p) = r {
                    Some(p.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        let approval_scopes_consulted: Vec<String> =
            input.approvals.iter().map(|a| a.scope.clone()).collect();

        let entries: Vec<PolicyAuditEntry> = input
            .report
            .entries
            .iter()
            .map(|e| {
                let approval = input.approvals.iter().find(|a| a.scope == e.scope);
                let gate_decision = Self::gate_decision_for(&profile, e.state, &e.scope, approval);
                PolicyAuditEntry {
                    scope: e.scope.clone(),
                    state: format!("{:?}", e.state).to_lowercase(),
                    gate_decision,
                    approval_used: approval.map(|a| a.approver.clone()),
                }
            })
            .collect();

        let audit = PolicyAudit {
            profile,
            entries,
            approval_scopes_consulted,
        };

        (decision, audit)
    }

    // ── Rule evaluators ───────────────────────────────────────────────────

    fn evaluate_rule(rule: &PolicyRule, input: &PolicyInput<'_>) -> PolicyDecision {
        match rule {
            PolicyRule::NoUnsafe => Self::eval_no_unsafe(input.report, input.approvals),
            PolicyRule::NoUnverifiedPublicApi => Self::eval_no_unverified_public_api(input.report),
            PolicyRule::RequireApproval => {
                Self::eval_require_approval(input.report, input.approvals)
            }
            PolicyRule::ProfileGate(profile) => {
                Self::eval_profile_gate(input.report, profile, input.approvals)
            }
            PolicyRule::NoPublicApiChangesWithoutApproval => {
                Self::eval_no_public_api_changes(input.public_api_changes, input.approvals)
            }
        }
    }

    // ── NoUnsafe ──────────────────────────────────────────────────────────

    fn eval_no_unsafe(report: &VerificationReport, approvals: &[ApprovalRecord]) -> PolicyDecision {
        let violations: Vec<PolicyViolation> = report
            .entries
            .iter()
            .filter(|e| e.state == VerificationState::Unsafe)
            .filter(|e| !Self::has_any_approval(&e.scope, approvals))
            .map(|e| PolicyViolation {
                code: POLICY_UNSAFE_BLOCKED.to_string(),
                scope: e.scope.clone(),
                message: format!(
                    "entry '{}' has Unsafe state and no explicit approval record",
                    e.scope
                ),
            })
            .collect();

        if violations.is_empty() {
            PolicyDecision::Passed
        } else {
            PolicyDecision::Failed(violations)
        }
    }

    // ── NoUnverifiedPublicApi ─────────────────────────────────────────────

    fn eval_no_unverified_public_api(report: &VerificationReport) -> PolicyDecision {
        let violations: Vec<PolicyViolation> = report
            .entries
            .iter()
            .filter(|e| e.state == VerificationState::Unverified && e.scope.starts_with("pub::"))
            .map(|e| PolicyViolation {
                code: POLICY_UNVERIFIED_PUBLIC_API.to_string(),
                scope: e.scope.clone(),
                message: format!(
                    "public-scope entry '{}' has Unverified state; public API must be verified",
                    e.scope
                ),
            })
            .collect();

        if violations.is_empty() {
            PolicyDecision::Passed
        } else {
            PolicyDecision::Failed(violations)
        }
    }

    // ── RequireApproval ───────────────────────────────────────────────────

    fn eval_require_approval(
        report: &VerificationReport,
        approvals: &[ApprovalRecord],
    ) -> PolicyDecision {
        let scopes: Vec<String> = report
            .entries
            .iter()
            .filter(|e| e.state == VerificationState::Unsafe)
            .filter(|e| !Self::has_any_approval(&e.scope, approvals))
            .map(|e| e.scope.clone())
            .collect();

        if scopes.is_empty() {
            PolicyDecision::Passed
        } else {
            PolicyDecision::ApprovalRequired(scopes)
        }
    }

    // ── NoPublicApiChangesWithoutApproval ─────────────────────────────────

    fn eval_no_public_api_changes(
        public_api_changes: &[PublicApiChange],
        approvals: &[ApprovalRecord],
    ) -> PolicyDecision {
        let violations: Vec<PolicyViolation> = public_api_changes
            .iter()
            .filter(|c| !Self::has_any_approval(&c.scope, approvals))
            .map(|c| PolicyViolation {
                code: POLICY_PUBLIC_API_CHANGED.to_string(),
                scope: c.scope.clone(),
                message: format!(
                    "public API '{}' changed without approval: {}",
                    c.scope, c.description
                ),
            })
            .collect();

        if violations.is_empty() {
            PolicyDecision::Passed
        } else {
            PolicyDecision::Failed(violations)
        }
    }

    // ── ProfileGate ───────────────────────────────────────────────────────

    fn eval_profile_gate(
        report: &VerificationReport,
        profile: &str,
        approvals: &[ApprovalRecord],
    ) -> PolicyDecision {
        let mut violations: Vec<PolicyViolation> = Vec::new();
        let mut warnings: Vec<PolicyWarning> = Vec::new();

        for entry in &report.entries {
            let is_public = entry.scope.starts_with("pub::");
            let approval = approvals.iter().find(|a| a.scope == entry.scope);

            match Self::profile_gate_result(profile, entry.state, is_public, approval) {
                GateResult::Block(code, msg) => {
                    violations.push(PolicyViolation {
                        code,
                        scope: entry.scope.clone(),
                        message: msg,
                    });
                }
                GateResult::Warn(code, msg) => {
                    warnings.push(PolicyWarning {
                        code,
                        scope: entry.scope.clone(),
                        message: msg,
                    });
                }
                GateResult::RequireApproval => {
                    // Emit as violation with POLICY_ASSUMED_UNAPPROVED — blocks
                    violations.push(PolicyViolation {
                        code: POLICY_ASSUMED_UNAPPROVED.to_string(),
                        scope: entry.scope.clone(),
                        message: format!(
                            "profile '{}' requires approval for entry '{}' with state {:?}",
                            profile, entry.scope, entry.state
                        ),
                    });
                }
                GateResult::Pass => {}
            }
        }

        if !violations.is_empty() {
            PolicyDecision::Failed(violations)
        } else if !warnings.is_empty() {
            PolicyDecision::PassedWithWarnings(warnings)
        } else {
            PolicyDecision::Passed
        }
    }

    /// Compute a human-readable gate decision label for audit purposes.
    fn gate_decision_for(
        profile: &str,
        state: VerificationState,
        scope: &str,
        approval: Option<&ApprovalRecord>,
    ) -> String {
        let is_public = scope.starts_with("pub::");
        match Self::profile_gate_result(profile, state, is_public, approval) {
            GateResult::Block(_, _) => "failed".to_string(),
            GateResult::Warn(_, _) => "warning".to_string(),
            GateResult::RequireApproval => "approval_required".to_string(),
            GateResult::Pass => "passed".to_string(),
        }
    }

    /// Return the gate result for a single entry under the given profile.
    ///
    /// # Profile matrix (strict-by-default)
    ///
    /// Unknown profiles map to the `critical` gate (most restrictive).
    #[allow(clippy::too_many_lines)]
    fn profile_gate_result(
        profile: &str,
        state: VerificationState,
        is_public: bool,
        approval: Option<&ApprovalRecord>,
    ) -> GateResult {
        match state {
            // Failed always blocks regardless of profile.
            VerificationState::Failed => GateResult::Block(
                POLICY_PROFILE_GATE.to_string(),
                format!("profile '{profile}' always blocks Failed entries"),
            ),

            // Unsafe — profile-specific handling.
            VerificationState::Unsafe => match profile {
                // critical: Unsafe always blocks, even with Strong approval
                "critical" => GateResult::Block(
                    POLICY_PROFILE_GATE.to_string(),
                    format!("profile '{profile}' always blocks Unsafe entries"),
                ),
                // prod: blocks without Strong approval (security exception = Strong)
                "prod" => {
                    if approval
                        .map(|a| a.strength == ApprovalStrength::Strong)
                        .unwrap_or(false)
                    {
                        GateResult::Pass
                    } else {
                        GateResult::Block(
                            POLICY_UNSAFE_BLOCKED.to_string(),
                            format!(
                                "profile '{profile}' blocks Unsafe without strong security exception"
                            ),
                        )
                    }
                }
                // staging: always blocks Unsafe
                "staging" => GateResult::Block(
                    POLICY_PROFILE_GATE.to_string(),
                    format!("profile '{profile}' blocks Unsafe entries"),
                ),
                // draft/dev/test/unknown: block Unsafe without any approval
                _ => {
                    if approval.is_some() {
                        GateResult::Pass
                    } else {
                        GateResult::Block(
                            POLICY_UNSAFE_BLOCKED.to_string(),
                            format!("profile '{profile}' blocks Unsafe without approval"),
                        )
                    }
                }
            },

            // Unverified — depends on profile strictness.
            VerificationState::Unverified => match profile {
                // Strict profiles: Unverified always blocks.
                "prod" | "staging" | "critical" => GateResult::Block(
                    POLICY_PROFILE_GATE.to_string(),
                    format!("profile '{profile}' blocks Unverified entries"),
                ),
                // dev: blocks public Unverified, allows private (with optional warning).
                "dev" => {
                    if is_public {
                        GateResult::Block(
                            POLICY_PROFILE_GATE.to_string(),
                            "dev profile blocks Unverified in public scope".to_string(),
                        )
                    } else {
                        GateResult::Pass
                    }
                }
                // draft: Unverified allowed but emits a warning.
                "draft" => GateResult::Warn(
                    "POLICY_UNVERIFIED_WARN".to_string(),
                    "entry has Unverified state in draft profile; annotation recommended"
                        .to_string(),
                ),
                // test: Unverified allowed (no warning required).
                "test" => GateResult::Pass,
                // STRICT-BY-DEFAULT: unknown profiles block Unverified like critical.
                _ => GateResult::Block(
                    POLICY_PROFILE_GATE.to_string(),
                    format!(
                        "unknown profile '{profile}' treated as conservative — blocks Unverified"
                    ),
                ),
            },

            // Assumed — requires boundary/approval in strict profiles.
            VerificationState::Assumed => match profile {
                // critical: requires Strong approval — Weak is rejected.
                "critical" => match approval {
                    Some(a) if a.strength == ApprovalStrength::Strong => GateResult::Pass,
                    Some(_) => GateResult::Block(
                        POLICY_WEAK_ASSUMPTION.to_string(),
                        "critical profile rejects Assumed with only Weak approval".to_string(),
                    ),
                    None => GateResult::RequireApproval,
                },
                // prod: requires any Strong approval.
                "prod" => match approval {
                    Some(a) if a.strength == ApprovalStrength::Strong => GateResult::Pass,
                    Some(_) => GateResult::Block(
                        POLICY_WEAK_ASSUMPTION.to_string(),
                        "prod profile requires Strong approval for Assumed entries".to_string(),
                    ),
                    None => GateResult::RequireApproval,
                },
                // staging: requires any approval (Strong or Weak).
                "staging" => {
                    if approval.is_some() {
                        GateResult::Pass
                    } else {
                        GateResult::RequireApproval
                    }
                }
                // dev: requires any approval (boundary assumption).
                "dev" => {
                    if approval.is_some() {
                        GateResult::Pass
                    } else {
                        GateResult::RequireApproval
                    }
                }
                // draft: Assumed allowed; warn if no evidence annotation.
                "draft" => GateResult::Warn(
                    "POLICY_ASSUMED_WARN".to_string(),
                    "Assumed entry in draft profile; boundary annotation recommended".to_string(),
                ),
                // test: Assumed allowed (test-only assumptions).
                "test" => GateResult::Pass,
                // STRICT-BY-DEFAULT: unknown profiles treat Assumed like critical.
                _ => match approval {
                    Some(a) if a.strength == ApprovalStrength::Strong => GateResult::Pass,
                    Some(_) => GateResult::Block(
                        POLICY_WEAK_ASSUMPTION.to_string(),
                        format!(
                            "unknown profile '{profile}' treated as conservative — rejects weak Assumed"
                        ),
                    ),
                    None => GateResult::RequireApproval,
                },
            },

            // RuntimeChecked — passes in all profiles (materialised check present).
            VerificationState::RuntimeChecked => GateResult::Pass,

            // Proven — always passes in every profile.
            VerificationState::Proven => GateResult::Pass,
        }
    }

    // ── Approval lookup ───────────────────────────────────────────────────

    /// Return `true` if `approvals` contains a record whose `scope` exactly
    /// matches `scope` (any strength).
    fn has_any_approval(scope: &str, approvals: &[ApprovalRecord]) -> bool {
        approvals.iter().any(|a| a.scope == scope)
    }
}

// ── GateResult ────────────────────────────────────────────────────────────

/// Internal result of evaluating a single entry against a profile gate.
enum GateResult {
    /// Entry passes without action.
    Pass,
    /// Entry triggers a non-blocking warning.
    Warn(String, String),
    /// Entry must have an approval but has none → surface as ApprovalRequired.
    RequireApproval,
    /// Entry is blocked; carries error code and message.
    Block(String, String),
}
