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
// - `"adapter-mismatch"`  — handler/adapter shape does not match contract
// - `"ffi-host-type-mismatch"` or `"host-type-mismatch"` — FFI host type drift
//
// # Verification states
//
// | Condition                                       | State       |
// |-------------------------------------------------|-------------|
// | `"expired-assumption"` tag                      | Failed      |
// | `"no-contract"` tag                             | Failed      |
// | adapter/FFI host mismatch tag                   | Failed      |
// | `TrustLevel::Unsafe` without `"approved"` tag   | Unsafe      |
// | `"unsafe-ffi"` without `"approved"` tag        | Unsafe      |
// | all required tags present                       | Assumed     |
// | some required tags missing                      | Unverified  |
// | no `trust_metadata` at all                      | Unverified  |

use std::cmp::Ordering;

use ail_core::semantic_graph::{NodeKind, SemanticGraph, TrustLevel, TrustMetadata};

use crate::diagnostic::{Diagnostic, DiagnosticSeverity, RepairOption};
use crate::report::{SummaryCounts, VerificationEntry, VerificationReport, VerificationState};

// ── Error codes ───────────────────────────────────────────────────────────

pub const E_BOUNDARY_EXPIRED_ASSUMPTION: &str = "E_BOUNDARY_EXPIRED_ASSUMPTION";
pub const E_BOUNDARY_NO_CONTRACT: &str = "E_BOUNDARY_NO_CONTRACT";
pub const E_BOUNDARY_UNCHECKED_FFI: &str = "E_BOUNDARY_UNCHECKED_FFI";
pub const E_BOUNDARY_UNSAFE_TRUST: &str = "E_BOUNDARY_UNSAFE_TRUST";
pub const E_BOUNDARY_ADAPTER_MISMATCH: &str = "E_BOUNDARY_ADAPTER_MISMATCH";
pub const E_BOUNDARY_FFI_HOST_TYPE_MISMATCH: &str = "E_BOUNDARY_FFI_HOST_TYPE_MISMATCH";
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
                Some(tm) => Self::classify_boundary(tm, &scope, config),
            };

            // Emit stable, redacted production diagnostics for non-accepted states.
            if matches!(
                state,
                VerificationState::Failed
                    | VerificationState::Unsafe
                    | VerificationState::Unverified
            ) {
                let issue = boundary_issue(
                    node.trust_metadata.as_ref(),
                    state,
                    evidence.as_deref().unwrap_or(""),
                );
                diagnostics.push(boundary_diagnostic(node.id, issue));
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

        canonicalize_boundary_diagnostics(&mut diagnostics);

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
        trust_metadata: &TrustMetadata,
        scope: &str,
        config: &BoundaryCheckerConfig,
    ) -> (VerificationState, Option<String>) {
        let tags = trust_metadata.tags.as_slice();
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

        // Adapter shape mismatch → Failed
        if has_tag(tags, "adapter-mismatch") {
            return (
                VerificationState::Failed,
                Some(format!(
                    "{E_BOUNDARY_ADAPTER_MISMATCH}: boundary '{}' adapter shape does not match the declared contract",
                    scope
                )),
            );
        }

        // FFI/host type drift → Failed
        if has_tag(tags, "ffi-host-type-mismatch") || has_tag(tags, "host-type-mismatch") {
            return (
                VerificationState::Failed,
                Some(format!(
                    "{E_BOUNDARY_FFI_HOST_TYPE_MISMATCH}: boundary '{}' FFI host type does not match the declared boundary schema",
                    scope
                )),
            );
        }

        // Unsafe trust level without approval → Unsafe
        if trust_metadata.level == TrustLevel::Unsafe && !has_tag(tags, "approved") {
            return (
                VerificationState::Unsafe,
                Some(format!(
                    "{E_BOUNDARY_UNSAFE_TRUST}: boundary '{}' has TrustLevel::Unsafe without explicit approval",
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

    use super::{
        BoundaryChecker, BoundaryCheckerConfig, E_BOUNDARY_ADAPTER_MISMATCH,
        E_BOUNDARY_EXPIRED_ASSUMPTION, E_BOUNDARY_FFI_HOST_TYPE_MISMATCH, E_BOUNDARY_NO_CONTRACT,
        E_BOUNDARY_UNSAFE_TRUST,
    };
    use crate::report::{VerificationReport, VerificationState};

    fn boundary_graph(tags: Vec<&str>) -> SemanticGraph {
        let node = boundary_node(0, "test_boundary", TrustLevel::Verified, tags);
        SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        }
    }

    fn boundary_node(id: u32, name: &str, level: TrustLevel, tags: Vec<&str>) -> GraphNode {
        let mut node = GraphNode::new(NodeRef(id), NodeKind::Boundary, name);
        node.trust_metadata = Some(TrustMetadata {
            level,
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
        });
        node
    }

    fn complete_tags<'a>(extra: Vec<&'a str>) -> Vec<&'a str> {
        let mut tags = vec![
            "has-trust-level",
            "has-contract",
            "has-handler",
            "has-owner",
            "has-review-policy",
        ];
        tags.extend(extra);
        tags
    }

    fn diagnostic_text(report: &VerificationReport) -> String {
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "{} {:?} {:?} {:?} {:?}",
                    diagnostic.code,
                    diagnostic.evidence,
                    diagnostic.expected,
                    diagnostic.actual,
                    diagnostic.repair_options
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
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

    #[test]
    fn unsafe_trust_boundary_emits_redacted_production_diagnostic() {
        let graph = SemanticGraph {
            nodes: vec![boundary_node(
                7,
                "secret_payments_ffi_boundary",
                TrustLevel::Unsafe,
                complete_tags(vec![]),
            )],
            edges: vec![],
        };

        let report = BoundaryChecker::check(&graph);

        assert_eq!(report.entries[0].state, VerificationState::Unsafe);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code.as_str(), E_BOUNDARY_UNSAFE_TRUST);

        let text = diagnostic_text(&report);
        assert!(
            !text.contains("secret_payments_ffi_boundary"),
            "production diagnostics must redact raw boundary names, got:\n{text}"
        );
        assert!(
            text.contains("unsafe-trust-boundary"),
            "diagnostic should expose a stable redacted detail, got:\n{text}"
        );
    }

    #[test]
    fn boundary_diagnostics_are_sorted_deduped_and_redacted() {
        let graph = SemanticGraph {
            // Deliberately reverse of expected diagnostic order.
            nodes: vec![
                boundary_node(
                    40,
                    "private_unsafe_trust_boundary",
                    TrustLevel::Unsafe,
                    complete_tags(vec![]),
                ),
                boundary_node(
                    30,
                    "private_ffi_host_type_boundary",
                    TrustLevel::Verified,
                    complete_tags(vec!["ffi-host-type-mismatch"]),
                ),
                boundary_node(
                    20,
                    "private_adapter_boundary",
                    TrustLevel::Verified,
                    complete_tags(vec!["adapter-mismatch", "adapter-mismatch"]),
                ),
                // Exact duplicate diagnostic shape: production diagnostics must dedup it.
                boundary_node(
                    20,
                    "private_adapter_boundary",
                    TrustLevel::Verified,
                    complete_tags(vec!["adapter-mismatch", "adapter-mismatch"]),
                ),
                boundary_node(
                    10,
                    "private_missing_contract_boundary",
                    TrustLevel::Verified,
                    vec![
                        "has-trust-level",
                        "has-handler",
                        "has-owner",
                        "has-review-policy",
                        "no-contract",
                    ],
                ),
            ],
            edges: vec![],
        };

        let report = BoundaryChecker::check(&graph);
        let codes: Vec<_> = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect();

        assert_eq!(
            codes,
            vec![
                E_BOUNDARY_NO_CONTRACT,
                E_BOUNDARY_ADAPTER_MISMATCH,
                E_BOUNDARY_FFI_HOST_TYPE_MISMATCH,
                E_BOUNDARY_UNSAFE_TRUST,
            ],
            "diagnostics must use deterministic production order"
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code.as_str() == E_BOUNDARY_ADAPTER_MISMATCH)
                .count(),
            1,
            "duplicate adapter-mismatch tags must not duplicate diagnostics"
        );

        let text = diagnostic_text(&report);
        for secret in [
            "private_unsafe_trust_boundary",
            "private_ffi_host_type_boundary",
            "private_adapter_boundary",
            "private_missing_contract_boundary",
            "adapter-mismatch, adapter-mismatch",
        ] {
            assert!(
                !text.contains(secret),
                "production diagnostics must redact descriptor {secret:?}; got:\n{text}"
            );
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

fn has_tag(tags: &[String], tag: &str) -> bool {
    tags.iter().any(|t| t == tag)
}

#[derive(Clone, Copy)]
struct BoundaryIssue {
    code: &'static str,
    detail: &'static str,
    severity: DiagnosticSeverity,
    blocking: bool,
}

fn boundary_issue(
    trust_metadata: Option<&TrustMetadata>,
    state: VerificationState,
    evidence: &str,
) -> BoundaryIssue {
    let tags = trust_metadata.map(|tm| tm.tags.as_slice()).unwrap_or(&[]);

    let (code, detail) = if has_tag(tags, "has-assumption-expired")
        || has_tag(tags, "has-assumption-revoked")
    {
        (E_BOUNDARY_ASSUMPTION_REVOKED, "assumption-revoked")
    } else if evidence.contains(E_BOUNDARY_EXPIRED_ASSUMPTION)
        || has_tag(tags, "expired-assumption")
    {
        (E_BOUNDARY_EXPIRED_ASSUMPTION, "assumption-expired")
    } else if evidence.contains(E_BOUNDARY_ADAPTER_MISMATCH) || has_tag(tags, "adapter-mismatch") {
        (E_BOUNDARY_ADAPTER_MISMATCH, "adapter-mismatch")
    } else if evidence.contains(E_BOUNDARY_FFI_HOST_TYPE_MISMATCH)
        || has_tag(tags, "ffi-host-type-mismatch")
        || has_tag(tags, "host-type-mismatch")
    {
        (E_BOUNDARY_FFI_HOST_TYPE_MISMATCH, "ffi-host-type-mismatch")
    } else if evidence.contains(E_BOUNDARY_UNSAFE_TRUST)
        || matches!(trust_metadata, Some(tm) if tm.level == TrustLevel::Unsafe)
    {
        (E_BOUNDARY_UNSAFE_TRUST, "unsafe-trust-boundary")
    } else if has_tag(tags, "unsafe-ffi") {
        (E_BOUNDARY_UNCHECKED_FFI, "unchecked-ffi")
    } else if trust_metadata.is_none() {
        (E_BOUNDARY_INCOMPLETE, "incomplete-boundary-declaration")
    } else if has_tag(tags, "no-contract") || !has_tag(tags, "has-contract") {
        (E_BOUNDARY_NO_CONTRACT, "missing-contract")
    } else {
        (E_BOUNDARY_INCOMPLETE, "incomplete-boundary-declaration")
    };

    BoundaryIssue {
        code,
        detail,
        severity: severity_for_state(state),
        blocking: matches!(state, VerificationState::Failed | VerificationState::Unsafe),
    }
}

fn severity_for_state(state: VerificationState) -> DiagnosticSeverity {
    match state {
        VerificationState::Failed | VerificationState::Unsafe => DiagnosticSeverity::Error,
        VerificationState::Unverified => DiagnosticSeverity::Warning,
        _ => DiagnosticSeverity::Info,
    }
}

fn boundary_diagnostic(
    target: ail_core::semantic_graph::NodeRef,
    issue: BoundaryIssue,
) -> Diagnostic {
    Diagnostic {
        code: issue.code.to_string(),
        severity: issue.severity,
        target,
        evidence: Some(format!(
            "category=boundary; code={}; target=node#{}; detail={}",
            issue.code, target.0, issue.detail
        )),
        expected: Some(expected_boundary_descriptor(issue.detail).into()),
        actual: Some(format!(
            "boundary issue redacted; boundary#{}; detail={}",
            target.0, issue.detail
        )),
        repair_options: vec![RepairOption::Explanation(
            repair_boundary_descriptor(issue.detail).into(),
        )],
        blocking: issue.blocking,
    }
}

fn expected_boundary_descriptor(detail: &str) -> &'static str {
    match detail {
        "missing-contract" => "declared boundary contract",
        "adapter-mismatch" => "adapter signature matching declared boundary contract",
        "ffi-host-type-mismatch" => "FFI host type matching declared boundary schema",
        "unsafe-trust-boundary" => "approved safe boundary trust posture",
        "unchecked-ffi" => "approved FFI boundary with checked contract",
        "assumption-expired" | "assumption-revoked" => "active boundary assumption",
        _ => "complete boundary declaration set",
    }
}

fn repair_boundary_descriptor(detail: &str) -> &'static str {
    match detail {
        "missing-contract" => "add a formal boundary contract before production verification",
        "adapter-mismatch" => "align the adapter shape with the boundary contract",
        "ffi-host-type-mismatch" => "align the FFI host type with the boundary schema",
        "unsafe-trust-boundary" => {
            "downgrade the unsafe trust posture or attach explicit production approval"
        }
        "unchecked-ffi" => "add explicit approval and a checked FFI boundary contract",
        "assumption-expired" | "assumption-revoked" => {
            "refresh, replace, or remove the expired boundary assumption"
        }
        _ => "complete trust level, contract, handler, owner, and review policy declarations",
    }
}

fn canonicalize_boundary_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(cmp_boundary_diagnostic);
    diagnostics.dedup();
}

fn cmp_boundary_diagnostic(a: &Diagnostic, b: &Diagnostic) -> Ordering {
    boundary_diagnostic_rank(a)
        .cmp(&boundary_diagnostic_rank(b))
        .then_with(|| a.code.cmp(&b.code))
        .then_with(|| a.target.cmp(&b.target))
        .then_with(|| a.blocking.cmp(&b.blocking).reverse())
        .then_with(|| a.evidence.cmp(&b.evidence))
        .then_with(|| a.expected.cmp(&b.expected))
        .then_with(|| a.actual.cmp(&b.actual))
        .then_with(|| format!("{:?}", a.repair_options).cmp(&format!("{:?}", b.repair_options)))
}

fn boundary_diagnostic_rank(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.code.as_str() {
        E_BOUNDARY_NO_CONTRACT => 0,
        E_BOUNDARY_ADAPTER_MISMATCH => 1,
        E_BOUNDARY_FFI_HOST_TYPE_MISMATCH => 2,
        E_BOUNDARY_UNSAFE_TRUST => 3,
        E_BOUNDARY_UNCHECKED_FFI => 4,
        E_BOUNDARY_ASSUMPTION_REVOKED => 5,
        E_BOUNDARY_EXPIRED_ASSUMPTION => 6,
        E_BOUNDARY_INCOMPLETE => 7,
        _ => 8,
    }
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
