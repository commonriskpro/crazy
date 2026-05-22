// ── ail-verify::policy ────────────────────────────────────────────────────
//
// Policy engine — verification layer 11 per verification.md.
//
// # Responsibility
//
// `PolicyEngine` takes a `PolicyInput` (VerificationReport + rules +
// approval records) and returns a `PolicyDecision`:
//   - `Passed`             — all rules satisfied, changeset may proceed.
//   - `Failed(violations)` — one or more blocking violations found.
//   - `ApprovalRequired(scopes)` — no outright failure, but explicit
//                                   approval is needed for listed scopes.
//
// # Rules
//
// `NoUnsafe`              — any `Unsafe` entry without an approval → Failed.
// `NoUnverifiedPublicApi` — any `Unverified` entry in a `pub::` scope → Failed.
// `RequireApproval`       — any `Unsafe` entry without approval → ApprovalRequired
//                           (weaker than NoUnsafe; does not hard-block).
// `ProfileGate(name)`     — applies the named profile's blocking matrix:
//   - all profiles: Failed always blocks
//   - prod/staging/critical: Unsafe (no approval) blocks; Unverified blocks
//   - dev: Unsafe (no approval) blocks; Unverified in pub:: scope blocks
//   - draft/test: Unsafe (no approval) blocks; Unverified allowed with warning
//
// # Decision priority
//
// When multiple rules fire, the most severe wins:
//   Failed > ApprovalRequired > Passed
//
// # Public scope convention
//
// In this phase, "public" means `scope.starts_with("pub::")`.
// Future phases will use the graph's visibility model directly.

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

// ── PolicyDecision ────────────────────────────────────────────────────────

/// The outcome of a `PolicyEngine::evaluate` call.
///
/// Priority order (most severe first): `Failed` > `ApprovalRequired` > `Passed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// All rules satisfied; the changeset may proceed.
    Passed,
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
    /// Unknown profile names are treated as `prod` (conservative fallback).
    ProfileGate(String),
}

// ── PolicyInput ───────────────────────────────────────────────────────────

/// Input bundle for `PolicyEngine::evaluate`.
pub struct PolicyInput<'a> {
    /// The report whose entries are checked against the rules.
    pub report: &'a VerificationReport,
    /// Ordered list of rules to apply (all are evaluated; results merged).
    pub rules: &'a [PolicyRule],
    /// Explicit approval records that can satisfy `NoUnsafe` / `RequireApproval`.
    pub approvals: &'a [ApprovalRecord],
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
    /// `Failed` (any) > `ApprovalRequired` (any) > `Passed`.
    ///
    /// Violations from all `Failed` rules are collected into a single
    /// `Failed(Vec<PolicyViolation>)`.  Scopes from all `ApprovalRequired`
    /// rules are collected into a single `ApprovalRequired(Vec<String>)`.
    pub fn evaluate(input: &PolicyInput<'_>) -> PolicyDecision {
        let mut all_violations: Vec<PolicyViolation> = Vec::new();
        let mut approval_scopes: Vec<String> = Vec::new();

        for rule in input.rules {
            match Self::evaluate_rule(rule, input.report, input.approvals) {
                PolicyDecision::Failed(mut violations) => {
                    all_violations.append(&mut violations);
                }
                PolicyDecision::ApprovalRequired(mut scopes) => {
                    approval_scopes.append(&mut scopes);
                }
                PolicyDecision::Passed => {}
            }
        }

        // Priority: Failed > ApprovalRequired > Passed
        if !all_violations.is_empty() {
            PolicyDecision::Failed(all_violations)
        } else if !approval_scopes.is_empty() {
            PolicyDecision::ApprovalRequired(approval_scopes)
        } else {
            PolicyDecision::Passed
        }
    }

    // ── Rule evaluators ───────────────────────────────────────────────────

    fn evaluate_rule(
        rule: &PolicyRule,
        report: &VerificationReport,
        approvals: &[ApprovalRecord],
    ) -> PolicyDecision {
        match rule {
            PolicyRule::NoUnsafe => Self::eval_no_unsafe(report, approvals),
            PolicyRule::NoUnverifiedPublicApi => Self::eval_no_unverified_public_api(report),
            PolicyRule::RequireApproval => Self::eval_require_approval(report, approvals),
            PolicyRule::ProfileGate(profile) => Self::eval_profile_gate(report, profile, approvals),
        }
    }

    // ── NoUnsafe ──────────────────────────────────────────────────────────

    fn eval_no_unsafe(report: &VerificationReport, approvals: &[ApprovalRecord]) -> PolicyDecision {
        let violations: Vec<PolicyViolation> = report
            .entries
            .iter()
            .filter(|e| e.state == VerificationState::Unsafe)
            .filter(|e| !Self::has_approval(&e.scope, approvals))
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
            .filter(|e| {
                e.state == VerificationState::Unverified && e.scope.starts_with("pub::")
            })
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
            .filter(|e| !Self::has_approval(&e.scope, approvals))
            .map(|e| e.scope.clone())
            .collect();

        if scopes.is_empty() {
            PolicyDecision::Passed
        } else {
            PolicyDecision::ApprovalRequired(scopes)
        }
    }

    // ── ProfileGate ───────────────────────────────────────────────────────

    fn eval_profile_gate(
        report: &VerificationReport,
        profile: &str,
        approvals: &[ApprovalRecord],
    ) -> PolicyDecision {
        let mut violations: Vec<PolicyViolation> = Vec::new();

        for entry in &report.entries {
            let is_public = entry.scope.starts_with("pub::");
            let has_approval = Self::has_approval(&entry.scope, approvals);

            let blocked = Self::profile_blocks(profile, entry.state, is_public, has_approval);
            if blocked {
                violations.push(PolicyViolation {
                    code: POLICY_PROFILE_GATE.to_string(),
                    scope: entry.scope.clone(),
                    message: format!(
                        "profile '{}' blocks entry '{}' with state {:?}",
                        profile, entry.scope, entry.state
                    ),
                });
            }
        }

        if violations.is_empty() {
            PolicyDecision::Passed
        } else {
            PolicyDecision::Failed(violations)
        }
    }

    /// Return `true` when `profile` blocks the given `state` in the given context.
    ///
    /// Matrix per verification.md:
    ///   - `Failed` always blocks in every profile.
    ///   - `Unsafe` without approval always blocks in every profile.
    ///   - `Unverified` in a public scope blocks in dev/staging/prod/critical.
    ///   - `Unverified` in any scope blocks in staging/prod/critical.
    ///   - draft/test: only `Failed` and `Unsafe` (without approval) block.
    fn profile_blocks(
        profile: &str,
        state: VerificationState,
        is_public: bool,
        has_approval: bool,
    ) -> bool {
        match state {
            // Failed always blocks regardless of profile.
            VerificationState::Failed => true,

            // Unsafe without explicit approval always blocks.
            VerificationState::Unsafe => !has_approval,

            // Unverified — depends on profile strictness.
            VerificationState::Unverified => match profile {
                // Strict profiles: Unverified always blocks.
                "prod" | "staging" | "critical" => true,
                // dev: blocks only public-scope Unverified.
                "dev" => is_public,
                // draft / test / unknown: Unverified is allowed (with warnings).
                _ => false,
            },

            // Proven, RuntimeChecked, Assumed — always pass in every profile.
            VerificationState::Proven
            | VerificationState::RuntimeChecked
            | VerificationState::Assumed => false,
        }
    }

    // ── Approval lookup ───────────────────────────────────────────────────

    /// Return `true` if `approvals` contains a record whose `scope` exactly
    /// matches `scope`.
    fn has_approval(scope: &str, approvals: &[ApprovalRecord]) -> bool {
        approvals.iter().any(|a| a.scope == scope)
    }
}
