// ── ail-verify::boundary_checker ─────────────────────────────────────────
//
// Boundary/FFI trust checker — verification layer 10 per verification.md.
//
// # Responsibility
//
// `BoundaryChecker` inspects `NodeKind::Boundary` nodes and validates their
// trust declarations.  Every boundary must declare trust level, capabilities,
// contracts, handlers/adapters, failure modes, assumptions, owner, and
// review/expiration policy.
//
// # Boundary node encoding
//
// `BoundaryChecker` operates on nodes with `kind == NodeKind::Boundary`.
// Boundary metadata is encoded in `trust_metadata`:
//
// Required tags (all must be present for `Assumed`):
// - `"has-trust-level"`   — boundary declares a trust level
// - `"has-contract"`      — boundary has a formal contract
// - `"has-handler"`       — boundary has an adapter/handler
// - `"has-owner"`         — boundary has a declared owner/team
// - `"has-review-policy"` — boundary has an expiration/review policy
//
// Special condition tags:
// - `"unsafe-ffi"`        — boundary is raw/unchecked FFI
// - `"approved"`          — explicit approval record exists
// - `"expired-assumption"`— the boundary's assumption has expired
// - `"no-contract"`       — boundary explicitly has no contract
//
// # Verification states
//
// | Condition                                       | State       |
// |-------------------------------------------------|-------------|
// | `"expired-assumption"` tag                      | Failed      |
// | `"no-contract"` tag                             | Failed      |
// | `"unsafe-ffi"` without `"approved"` tag        | Unsafe      |
// | all required tags present                       | Assumed     |
// | some required tags missing                      | Unverified  |
// | no `trust_metadata` at all                      | Unverified  |

use ail_core::semantic_graph::{NodeKind, SemanticGraph};

use crate::diagnostic::{Diagnostic, DiagnosticSeverity};
use crate::report::{SummaryCounts, VerificationEntry, VerificationReport, VerificationState};

// ── Error codes ───────────────────────────────────────────────────────────

pub const E_BOUNDARY_EXPIRED_ASSUMPTION: &str = "E_BOUNDARY_EXPIRED_ASSUMPTION";
pub const E_BOUNDARY_NO_CONTRACT: &str = "E_BOUNDARY_NO_CONTRACT";
pub const E_BOUNDARY_UNCHECKED_FFI: &str = "E_BOUNDARY_UNCHECKED_FFI";
pub const E_BOUNDARY_INCOMPLETE: &str = "E_BOUNDARY_INCOMPLETE";

/// Required tags for a fully-declared boundary.
const REQUIRED_TAGS: &[&str] = &[
    "has-trust-level",
    "has-contract",
    "has-handler",
    "has-owner",
    "has-review-policy",
];

// ── BoundaryChecker ───────────────────────────────────────────────────────

/// Pure, stateless boundary/FFI trust checker.
///
/// Call [`BoundaryChecker::check`] with a `SemanticGraph` to receive a
/// `VerificationReport` with one entry per `NodeKind::Boundary` node.
pub struct BoundaryChecker;

impl BoundaryChecker {
    /// Walk `graph` and classify each boundary node's trust state.
    ///
    /// Non-boundary nodes are silently skipped.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();

        for node in &graph.nodes {
            if node.kind != NodeKind::Boundary {
                continue;
            }

            let scope = node.name.clone();
            let claim = "boundary-trust".to_string();

            let (state, evidence) = match &node.trust_metadata {
                None => (
                    VerificationState::Unverified,
                    Some(format!(
                        "boundary '{}' has no trust_metadata; boundary declarations are required",
                        scope
                    )),
                ),
                Some(tm) => Self::classify_boundary(&tm.tags, &scope),
            };

            // Emit diagnostics for blocking conditions
            match state {
                VerificationState::Failed => {
                    let tags = node
                        .trust_metadata
                        .as_ref()
                        .map(|tm| tm.tags.as_slice())
                        .unwrap_or(&[]);
                    let code = if has_tag(tags, "expired-assumption") {
                        E_BOUNDARY_EXPIRED_ASSUMPTION
                    } else {
                        E_BOUNDARY_NO_CONTRACT
                    };
                    diagnostics.push(Diagnostic {
                        code: code.to_string(),
                        severity: DiagnosticSeverity::Error,
                        target: node.id,
                        evidence: evidence.clone(),
                        expected: None,
                        actual: None,
                        repair_options: vec![],
                        blocking: true,
                    });
                }
                VerificationState::Unsafe => {
                    diagnostics.push(Diagnostic {
                        code: E_BOUNDARY_UNCHECKED_FFI.to_string(),
                        severity: DiagnosticSeverity::Error,
                        target: node.id,
                        evidence: evidence.clone(),
                        expected: None,
                        actual: None,
                        repair_options: vec![],
                        blocking: true,
                    });
                }
                VerificationState::Unverified => {
                    // Emit warning for incomplete declarations
                    diagnostics.push(Diagnostic {
                        code: E_BOUNDARY_INCOMPLETE.to_string(),
                        severity: DiagnosticSeverity::Warning,
                        target: node.id,
                        evidence: evidence.clone(),
                        expected: None,
                        actual: None,
                        repair_options: vec![],
                        blocking: false,
                    });
                }
                _ => {}
            }

            entries.push(VerificationEntry {
                claim,
                state,
                scope,
                evidence,
            });
        }

        let summary_counts = compute_counts(&entries);
        VerificationReport {
            entries,
            diagnostics,
            schema_version: "verification/1.0".into(),
            summary_counts,
            ..Default::default()
        }
    }

    fn classify_boundary(tags: &[String], scope: &str) -> (VerificationState, Option<String>) {
        // Expired assumption → Failed
        if has_tag(tags, "expired-assumption") {
            return (
                VerificationState::Failed,
                Some(format!(
                    "boundary '{}' assumption has expired; expired assumptions are rejected in prod/critical",
                    scope
                )),
            );
        }

        // No contract → Failed
        if has_tag(tags, "no-contract") {
            return (
                VerificationState::Failed,
                Some(format!(
                    "boundary '{}' has no contract; boundaries without contracts are always failed",
                    scope
                )),
            );
        }

        // Unsafe FFI without approval → Unsafe
        if has_tag(tags, "unsafe-ffi") && !has_tag(tags, "approved") {
            return (
                VerificationState::Unsafe,
                Some(format!(
                    "boundary '{}' uses unchecked FFI without explicit approval; unsafe FFI requires approval",
                    scope
                )),
            );
        }

        // Check required tags
        let missing: Vec<&str> = REQUIRED_TAGS
            .iter()
            .filter(|&&req| !has_tag(tags, req))
            .copied()
            .collect();

        if missing.is_empty() {
            // All required tags present → Assumed (boundaries are assumed by nature)
            (VerificationState::Assumed, None)
        } else {
            // Missing required declarations → Unverified
            (
                VerificationState::Unverified,
                Some(format!(
                    "boundary '{}' is missing required declarations: {}",
                    scope,
                    missing.join(", ")
                )),
            )
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

fn has_tag(tags: &[String], tag: &str) -> bool {
    tags.iter().any(|t| t == tag)
}

fn compute_counts(entries: &[VerificationEntry]) -> SummaryCounts {
    SummaryCounts {
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
