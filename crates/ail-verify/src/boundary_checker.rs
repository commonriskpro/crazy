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
pub const E_BOUNDARY_ASSUMPTION_REVOKED: &str = "E_BOUNDARY_ASSUMPTION_REVOKED";

/// Required tags for a fully-declared boundary.
const REQUIRED_TAGS: &[&str] = &[
    "has-trust-level",
    "has-contract",
    "has-handler",
    "has-owner",
    "has-review-policy",
];

// ── BoundaryCheckerConfig ─────────────────────────────────────────────────

/// Configuration for [`BoundaryChecker::check_with_config`].
///
/// # Expiry checking
///
/// When `reference_date` is `Some("YYYY-MM-DD")`, boundary tags of the form
/// `"expires:YYYY-MM-DD"` are compared lexicographically against the reference
/// date.  If `expires_date <= reference_date`, the entry is `Failed` with
/// `E_BOUNDARY_EXPIRED_ASSUMPTION`.
///
/// When `reference_date` is `None` (the default), timestamp-based expiry is
/// skipped entirely — preserving existing behavior.
#[derive(Clone, Debug, Default)]
pub struct BoundaryCheckerConfig {
    /// Reference date in `"YYYY-MM-DD"` format for `expires:` tag comparison.
    ///
    /// When `None`, timestamp-based expiry checking is skipped.
    pub reference_date: Option<String>,
}

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
    ///
    /// Calls `check_with_config` with a default `BoundaryCheckerConfig`
    /// (no timestamp-based expiry checking).
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        Self::check_with_config(graph, &BoundaryCheckerConfig::default())
    }

    /// Walk `graph` and classify each boundary node's trust state,
    /// applying timestamp-based expiry checking from `config`.
    ///
    /// Non-boundary nodes are silently skipped.
    pub fn check_with_config(
        graph: &SemanticGraph,
        config: &BoundaryCheckerConfig,
    ) -> VerificationReport {
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
                Some(tm) => Self::classify_boundary(&tm.tags, &scope, config),
            };

            // Emit diagnostics for blocking conditions
            match state {
                VerificationState::Failed => {
                    let tags = node
                        .trust_metadata
                        .as_ref()
                        .map(|tm| tm.tags.as_slice())
                        .unwrap_or(&[]);
                    let code = if has_tag(tags, "has-assumption-expired")
                        || has_tag(tags, "has-assumption-revoked")
                    {
                        E_BOUNDARY_ASSUMPTION_REVOKED
                    } else if has_tag(tags, "expired-assumption") {
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

            let blocking = matches!(state, VerificationState::Failed | VerificationState::Unsafe);
            entries.push(VerificationEntry {
                claim,
                state,
                scope,
                evidence,
                blocking,
                repair_options: vec![],
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

    fn classify_boundary(
        tags: &[String],
        scope: &str,
        config: &BoundaryCheckerConfig,
    ) -> (VerificationState, Option<String>) {
        // AET-4 / AET-5 / AET-6: expires tag check BEFORE all other checks.
        // Only runs when config.reference_date is Some (AET-3 / backward compat).
        if let Some(ref ref_date) = config.reference_date {
            for tag in tags {
                if let Some(expires_str) = tag.strip_prefix("expires:") {
                    // Validate format: YYYY-MM-DD (10 chars, digits + dashes at [4] and [7])
                    let valid_format = expires_str.len() == 10
                        && expires_str.chars().enumerate().all(|(i, c)| {
                            if i == 4 || i == 7 {
                                c == '-'
                            } else {
                                c.is_ascii_digit()
                            }
                        });
                    if !valid_format {
                        return (
                            VerificationState::Unverified,
                            Some(format!(
                                "expires tag has invalid date format: {expires_str}"
                            )),
                        );
                    }
                    // ISO 8601 lexicographic comparison: same-day or past → Failed
                    if expires_str <= ref_date.as_str() {
                        return (
                            VerificationState::Failed,
                            Some(format!(
                                "{E_BOUNDARY_EXPIRED_ASSUMPTION}: boundary '{scope}' \
                                 assumption expired {expires_str} (reference: {ref_date})"
                            )),
                        );
                    }
                    // Future date → continues to normal classification
                }
            }
        }

        // TASK-22: assumption lifecycle checks (before legacy checks)
        // Revoked or expired assumption → Failed
        if has_tag(tags, "has-assumption-expired") || has_tag(tags, "has-assumption-revoked") {
            return (
                VerificationState::Failed,
                Some(format!(
                    "{E_BOUNDARY_ASSUMPTION_REVOKED}: boundary '{}' assumption has been expired or revoked",
                    scope
                )),
            );
        }

        // Proposed but not yet approved/active → Unverified
        if has_tag(tags, "has-assumption-proposed")
            && !has_tag(tags, "has-assumption-approved")
            && !has_tag(tags, "has-assumption-active")
        {
            return (
                VerificationState::Unverified,
                Some(format!(
                    "boundary '{}' assumption is proposed but not yet approved or active",
                    scope
                )),
            );
        }

        // Expired assumption → Failed (legacy tag)
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

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{
        GraphNode, NodeKind, NodeRef, SemanticGraph, TrustLevel, TrustMetadata,
    };

    use super::{BoundaryChecker, BoundaryCheckerConfig, E_BOUNDARY_EXPIRED_ASSUMPTION};
    use crate::report::VerificationState;

    fn boundary_graph(tags: Vec<&str>) -> SemanticGraph {
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Boundary, "test_boundary");
        node.trust_metadata = Some(TrustMetadata {
            level: TrustLevel::Verified,
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
        });
        SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        }
    }

    // ── T-15 / T-16: expires tag ─────────────────────────────────────────

    #[test]
    fn boundary_expires_tag_past_date_fails() {
        // expires:2025-01-01, reference 2026-01-01 → Failed
        let graph = boundary_graph(vec![
            "has-trust-level",
            "has-contract",
            "has-handler",
            "has-owner",
            "has-review-policy",
            "expires:2025-01-01",
        ]);
        let config = BoundaryCheckerConfig {
            reference_date: Some("2026-01-01".to_string()),
        };
        let report = BoundaryChecker::check_with_config(&graph, &config);
        let entry = &report.entries[0];
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_BOUNDARY_EXPIRED_ASSUMPTION),
            "evidence must contain E_BOUNDARY_EXPIRED_ASSUMPTION"
        );
    }

    #[test]
    fn boundary_expires_tag_future_date_passes_through() {
        // expires:2030-12-31, reference 2026-01-01 → continues to normal classification
        let graph = boundary_graph(vec![
            "has-trust-level",
            "has-contract",
            "has-handler",
            "has-owner",
            "has-review-policy",
            "expires:2030-12-31",
        ]);
        let config = BoundaryCheckerConfig {
            reference_date: Some("2026-01-01".to_string()),
        };
        let report = BoundaryChecker::check_with_config(&graph, &config);
        let entry = &report.entries[0];
        // All required tags present → Assumed (not Failed by expiry)
        assert_eq!(entry.state, VerificationState::Assumed);
    }

    #[test]
    fn boundary_expires_tag_same_day_fails() {
        // expires:2026-05-22, reference 2026-05-22 (same day = expired) → Failed
        let graph = boundary_graph(vec![
            "has-trust-level",
            "has-contract",
            "has-handler",
            "has-owner",
            "has-review-policy",
            "expires:2026-05-22",
        ]);
        let config = BoundaryCheckerConfig {
            reference_date: Some("2026-05-22".to_string()),
        };
        let report = BoundaryChecker::check_with_config(&graph, &config);
        assert_eq!(report.entries[0].state, VerificationState::Failed);
    }

    #[test]
    fn boundary_expires_tag_malformed_unverified() {
        // expires:not-a-date → Unverified
        let graph = boundary_graph(vec!["has-trust-level", "expires:not-a-date"]);
        let config = BoundaryCheckerConfig {
            reference_date: Some("2026-01-01".to_string()),
        };
        let report = BoundaryChecker::check_with_config(&graph, &config);
        assert_eq!(report.entries[0].state, VerificationState::Unverified);
    }

    #[test]
    fn boundary_no_reference_date_skips_timestamp_check() {
        // reference_date: None, tag "expires:2020-01-01" → behavior unchanged (no auto-expiry)
        let graph = boundary_graph(vec![
            "has-trust-level",
            "has-contract",
            "has-handler",
            "has-owner",
            "has-review-policy",
            "expires:2020-01-01",
        ]);
        let config = BoundaryCheckerConfig {
            reference_date: None,
        };
        let report = BoundaryChecker::check_with_config(&graph, &config);
        // Without reference_date, expires tag is ignored → all required tags → Assumed
        assert_eq!(report.entries[0].state, VerificationState::Assumed);
    }

    #[test]
    fn boundary_check_no_config_backward_compatible() {
        // BoundaryChecker::check(graph) still works without config
        let graph = boundary_graph(vec![
            "has-trust-level",
            "has-contract",
            "has-handler",
            "has-owner",
            "has-review-policy",
            "expires:2020-01-01", // would expire if config had reference_date
        ]);
        let report = BoundaryChecker::check(&graph);
        // Without config, expires tag ignored → Assumed
        assert_eq!(report.entries[0].state, VerificationState::Assumed);
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
                e.state == VerificationState::Proven || e.state == VerificationState::RuntimeChecked
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
