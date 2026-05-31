// ── ail-verify::resource_checker ─────────────────────────────────────────
//
// Resource lifecycle checker — verification layer 8 per verification.md.
//
// # Responsibility
//
// `ResourceChecker` inspects every `GraphNode` that carries resource lifecycle
// metadata (encoded in `trust_metadata`) and emits `VerificationEntry` items
// classifying the resource's lifecycle state.
//
// # Resource encoding (trust_metadata)
//
// Resource nodes encode their kind and lifecycle state through the
// `trust_metadata` field:
//
// | `level`              | Meaning                                     |
// |----------------------|---------------------------------------------|
// | `"resource:linear"`  | Affine-linear: must be consumed exactly once |
// | `"resource:affine"`  | Affine: consumed at most once               |
// | `"resource:shared"`  | Shared: multiple use, needs safe capability |
//
// Lifecycle tags (in `trust_metadata.tags`):
//
// | Tag                      | Meaning                                   |
// |--------------------------|-------------------------------------------|
// | `"released"`             | Resource was released/consumed            |
// | `"use-after-close"`      | Use-after-close violation detected        |
// | `"use-after-release"`    | Use-after-release violation detected      |
// | `"double-close"`         | Double-close violation detected           |
// | `"double-release"`       | Double-release violation detected         |
// | `"leaked-resource"`      | Linear resource leak detected             |
// | `"lifetime-mismatch"`    | Resource lifetime does not match owner    |
// | `"missing-capability"`   | Required resource capability is absent    |
// | `"never-consumed"`       | Linear resource never consumed (violation)|
// | `"quota-exceeded"`       | Resource quota/budget was exceeded        |
// | `"limit-exceeded"`       | Resource configured limit was exceeded    |
// | `"safe-capability"`      | Shared resource has safe concurrency cap  |
// | `"transaction-committed"`| Transaction was committed                 |
// | `"transaction-rolled-back"` | Transaction was rolled back            |
// | `"lock-released"`        | Lock was released                         |
// | `"stream-closed"`        | Stream was closed or transferred          |
//
// # Verification states
//
// | Condition                              | State    |
// |----------------------------------------|----------|
// | `use-after-release` tag present        | Failed   |
// | `double-release` tag present           | Failed   |
// | `lifetime-mismatch` tag present        | Failed   |
// | `leaked-resource` tag present          | Failed   |
// | linear + `never-consumed` tag          | Failed   |
// | shared + no `safe-capability` tag      | Unsafe   |
// | properly released/committed            | Proven   |
// | shared + `safe-capability`             | Proven   |

use std::cmp::Ordering;

use ail_core::semantic_graph::{EdgeKind, NodeRef, SemanticGraph};

use crate::diagnostic::{Diagnostic, DiagnosticSeverity, RepairOption};
use crate::report::{SummaryCounts, VerificationEntry, VerificationReport, VerificationState};

// ── Error codes ───────────────────────────────────────────────────────────

pub const RESOURCE_DIAGNOSTIC_CATEGORY_QUOTA_LIMIT: &str = "resource-quota-limit";
pub const RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE: &str = "resource-lifecycle";
pub const RESOURCE_DIAGNOSTIC_CATEGORY_CAPABILITY: &str = "resource-capability";

pub const E_USE_AFTER_CLOSE: &str = "E_USE_AFTER_CLOSE";
pub const E_USE_AFTER_RELEASE: &str = "E_USE_AFTER_RELEASE";
pub const E_DOUBLE_CLOSE: &str = "E_DOUBLE_CLOSE";
pub const E_DOUBLE_RELEASE: &str = "E_DOUBLE_RELEASE";
pub const E_RESOURCE_LEAKED: &str = "E_RESOURCE_LEAKED";
pub const E_LINEAR_NOT_CONSUMED: &str = "E_LINEAR_NOT_CONSUMED";
pub const E_SHARED_WITHOUT_SAFE_CAPABILITY: &str = "E_SHARED_WITHOUT_SAFE_CAPABILITY";
pub const E_RESOURCE_NO_LIFECYCLE_EDGE: &str = "E_RESOURCE_NO_LIFECYCLE_EDGE";
pub const E_RESOURCE_QUOTA_EXCEEDED: &str = "E_RESOURCE_QUOTA_EXCEEDED";
pub const E_RESOURCE_LIMIT_EXCEEDED: &str = "E_RESOURCE_LIMIT_EXCEEDED";
pub const E_RESOURCE_MISSING_CAPABILITY: &str = "E_RESOURCE_MISSING_CAPABILITY";
pub const E_RESOURCE_LIFETIME_MISMATCH: &str = "E_RESOURCE_LIFETIME_MISMATCH";

// ── ResourceChecker ───────────────────────────────────────────────────────

/// Pure, stateless resource lifecycle checker.
///
/// Call [`ResourceChecker::check`] with a `SemanticGraph` to receive a
/// `VerificationReport` with one entry per resource node.
pub struct ResourceChecker;

impl ResourceChecker {
    /// Walk `graph` and classify each resource node's lifecycle state.
    ///
    /// Non-resource nodes (those without `trust_metadata.level` starting with
    /// `"resource:"`) are silently skipped.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();

        for node in &graph.nodes {
            let Some(tm) = &node.trust_metadata else {
                continue;
            };

            let resource_kind = match tm.level.as_str() {
                "resource:linear" => ResourceKind::Linear,
                "resource:affine" => ResourceKind::Affine,
                "resource:shared" => ResourceKind::Shared,
                _ => continue, // not a resource node
            };

            let tags = &tm.tags;
            let scope = node.name.clone();
            let claim = format!("resource-lifecycle[{}]", tm.level.as_str());

            let (state, evidence, issue) =
                Self::classify_resource(resource_kind, tags, &scope, node.id, graph);

            if let Some(issue) = issue {
                diagnostics.push(Self::resource_diagnostic(node.id, issue));
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

        canonicalize_resource_diagnostics(&mut diagnostics);

        let summary_counts = compute_counts(&entries);
        VerificationReport {
            entries,
            diagnostics,
            schema_version: "verification/1.0".into(),
            summary_counts,
            ..Default::default()
        }
    }

    fn resource_diagnostic(target: NodeRef, issue: ResourceIssue) -> Diagnostic {
        Diagnostic {
            code: issue.code.to_string(),
            severity: issue.severity,
            target,
            evidence: Some(resource_issue_evidence(
                issue.code,
                issue.category,
                issue.detail,
                target,
            )),
            expected: Some(issue.expected.into()),
            actual: Some(issue.actual.into()),
            repair_options: vec![RepairOption::Explanation(issue.repair.into())],
            blocking: issue.blocking,
        }
    }

    fn classify_resource(
        kind: ResourceKind,
        tags: &[String],
        _scope: &str,
        node_id: NodeRef,
        graph: &SemanticGraph,
    ) -> (VerificationState, Option<String>, Option<ResourceIssue>) {
        // Violations take priority regardless of resource kind
        if has_tag(tags, "use-after-close") {
            return Self::issue_result(
                VerificationState::Failed,
                ResourceIssue::error(
                    E_USE_AFTER_CLOSE,
                    RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE,
                    "use-after-close",
                    "resource is not used after close",
                    "use-after-close",
                    "move close after the final use or remove the later use",
                ),
                node_id,
            );
        }
        if has_tag(tags, "use-after-release") {
            return Self::issue_result(
                VerificationState::Failed,
                ResourceIssue::error(
                    E_USE_AFTER_RELEASE,
                    RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE,
                    "use-after-release",
                    "resource is not used after release",
                    "use-after-release",
                    "move release after the final use or remove the later use",
                ),
                node_id,
            );
        }
        if has_tag(tags, "double-close") {
            return Self::issue_result(
                VerificationState::Failed,
                ResourceIssue::error(
                    E_DOUBLE_CLOSE,
                    RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE,
                    "double-close",
                    "resource is closed at most once",
                    "double-close",
                    "make close idempotent in one owner or remove the duplicate close path",
                ),
                node_id,
            );
        }
        if has_tag(tags, "double-release") {
            return Self::issue_result(
                VerificationState::Failed,
                ResourceIssue::error(
                    E_DOUBLE_RELEASE,
                    RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE,
                    "double-release",
                    "resource is released at most once",
                    "double-release",
                    "make release idempotent in one owner or remove the duplicate release path",
                ),
                node_id,
            );
        }
        if has_tag(tags, "lifetime-mismatch") {
            return Self::issue_result(
                VerificationState::Failed,
                ResourceIssue::error(
                    E_RESOURCE_LIFETIME_MISMATCH,
                    RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE,
                    "lifetime-mismatch",
                    "resource lifetime matches the owning scope",
                    "lifetime-mismatch",
                    "shorten the borrow, transfer ownership, or extend the owner scope explicitly",
                ),
                node_id,
            );
        }
        if has_tag(tags, "quota-exceeded") {
            return Self::issue_result(
                VerificationState::Failed,
                ResourceIssue::error(
                    E_RESOURCE_QUOTA_EXCEEDED,
                    RESOURCE_DIAGNOSTIC_CATEGORY_QUOTA_LIMIT,
                    "quota-exceeded",
                    "resource usage stays within declared quota",
                    "quota-exceeded",
                    "reduce resource usage, increase the declared quota, or split the workload",
                ),
                node_id,
            );
        }
        if has_tag(tags, "limit-exceeded") {
            return Self::issue_result(
                VerificationState::Failed,
                ResourceIssue::error(
                    E_RESOURCE_LIMIT_EXCEEDED,
                    RESOURCE_DIAGNOSTIC_CATEGORY_QUOTA_LIMIT,
                    "limit-exceeded",
                    "resource usage stays within configured limit",
                    "limit-exceeded",
                    "lower peak resource usage, raise the configured limit, or add backpressure",
                ),
                node_id,
            );
        }
        if has_tag(tags, "missing-capability") {
            return Self::issue_result(
                VerificationState::Unsafe,
                ResourceIssue::error(
                    E_RESOURCE_MISSING_CAPABILITY,
                    RESOURCE_DIAGNOSTIC_CATEGORY_CAPABILITY,
                    "missing-capability",
                    "resource has the required capability proof",
                    "missing-capability",
                    "attach a SafeCapability edge or safe-capability lifecycle tag",
                ),
                node_id,
            );
        }
        if has_tag(tags, "leaked-resource") || has_tag(tags, "leaked") {
            return Self::issue_result(
                VerificationState::Failed,
                ResourceIssue::error(
                    E_RESOURCE_LEAKED,
                    RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE,
                    "leaked-resource",
                    "linear resource is consumed exactly once or explicitly released",
                    "leaked-resource",
                    "add a release/consume path before the resource leaves scope",
                ),
                node_id,
            );
        }

        match kind {
            ResourceKind::Linear => {
                // Linear resources must be consumed exactly once.
                if has_tag(tags, "never-consumed") {
                    return Self::issue_result(
                        VerificationState::Failed,
                        ResourceIssue::error(
                            E_LINEAR_NOT_CONSUMED,
                            RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE,
                            "never consumed",
                            "linear resource is consumed exactly once or explicitly released",
                            "never-consumed",
                            "add a release/consume path before the linear resource leaves scope",
                        ),
                        node_id,
                    );
                }
                if has_tag(tags, "released") {
                    return (VerificationState::Proven, None, None);
                }
                // TASK-18: check for outgoing Consumes or Releases edges as proof of lifecycle
                let has_lifecycle_edge = graph.edges.iter().any(|e| {
                    e.source == node_id && matches!(e.kind, EdgeKind::Consumes | EdgeKind::Releases)
                });
                if has_lifecycle_edge {
                    (VerificationState::Proven, None, None)
                } else {
                    let issue = ResourceIssue::warning(
                        E_RESOURCE_NO_LIFECYCLE_EDGE,
                        RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE,
                        "missing-lifecycle-edge",
                        "linear resource has a release tag or Consumes/Releases edge",
                        "no lifecycle proof",
                        "add an explicit release tag or a Consumes/Releases edge",
                    );
                    (
                        VerificationState::Unverified,
                        Some(resource_issue_evidence(
                            issue.code,
                            issue.category,
                            issue.detail,
                            node_id,
                        )),
                        Some(issue),
                    )
                }
            }
            ResourceKind::Affine => {
                // Affine resources must be released at most once.
                // Properly released or scope-cleaned → Proven.
                if has_tag(tags, "released")
                    || has_tag(tags, "transaction-committed")
                    || has_tag(tags, "transaction-rolled-back")
                    || has_tag(tags, "lock-released")
                    || has_tag(tags, "stream-closed")
                {
                    (VerificationState::Proven, None, None)
                } else {
                    // Not released — unverified (affine allows non-consumption)
                    (
                        VerificationState::Unverified,
                        Some(format!(
                            "{E_RESOURCE_NO_LIFECYCLE_EDGE}: category={RESOURCE_DIAGNOSTIC_CATEGORY_LIFECYCLE}; target=node#{}; resource=resource#{}; detail=affine-lifecycle-unverified",
                            node_id.0, node_id.0
                        )),
                        None,
                    )
                }
            }
            ResourceKind::Shared => {
                // Shared resources require concurrency-safe capability (tag or edge).
                if has_tag(tags, "safe-capability") {
                    return (VerificationState::Proven, None, None);
                }
                // TASK-18: check for outgoing SafeCapability edge
                let has_cap_edge = graph
                    .edges
                    .iter()
                    .any(|e| e.source == node_id && e.kind == EdgeKind::SafeCapability);
                if has_cap_edge {
                    (VerificationState::Proven, None, None)
                } else {
                    Self::issue_result(
                        VerificationState::Unsafe,
                        ResourceIssue::error(
                            E_SHARED_WITHOUT_SAFE_CAPABILITY,
                            RESOURCE_DIAGNOSTIC_CATEGORY_CAPABILITY,
                            "missing-safe-capability",
                            "shared resource has a SafeCapability edge or safe-capability tag",
                            "missing-safe-capability",
                            "attach a SafeCapability edge or safe-capability lifecycle tag",
                        ),
                        node_id,
                    )
                }
            }
        }
    }

    fn issue_result(
        state: VerificationState,
        issue: ResourceIssue,
        target: NodeRef,
    ) -> (VerificationState, Option<String>, Option<ResourceIssue>) {
        (
            state,
            Some(resource_issue_evidence(
                issue.code,
                issue.category,
                issue.detail,
                target,
            )),
            Some(issue),
        )
    }
}

// ── ResourceKind ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourceKind {
    Linear,
    Affine,
    Shared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResourceIssue {
    code: &'static str,
    category: &'static str,
    detail: &'static str,
    severity: DiagnosticSeverity,
    expected: &'static str,
    actual: &'static str,
    repair: &'static str,
    blocking: bool,
}

impl ResourceIssue {
    fn error(
        code: &'static str,
        category: &'static str,
        detail: &'static str,
        expected: &'static str,
        actual: &'static str,
        repair: &'static str,
    ) -> Self {
        Self {
            code,
            category,
            detail,
            severity: DiagnosticSeverity::Error,
            expected,
            actual,
            repair,
            blocking: true,
        }
    }

    fn warning(
        code: &'static str,
        category: &'static str,
        detail: &'static str,
        expected: &'static str,
        actual: &'static str,
        repair: &'static str,
    ) -> Self {
        Self {
            code,
            category,
            detail,
            severity: DiagnosticSeverity::Warning,
            expected,
            actual,
            repair,
            blocking: false,
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

fn has_tag(tags: &[String], tag: &str) -> bool {
    tags.iter().any(|t| t == tag)
}

fn resource_issue_evidence(code: &str, category: &str, detail: &str, target: NodeRef) -> String {
    format!(
        "{code}: category={category}; target=node#{}; resource=resource#{}; detail={detail}",
        target.0, target.0
    )
}

fn canonicalize_resource_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(cmp_resource_diagnostic);
    diagnostics.dedup();
}

fn cmp_resource_diagnostic(a: &Diagnostic, b: &Diagnostic) -> Ordering {
    resource_diagnostic_rank(a.code.as_str())
        .cmp(&resource_diagnostic_rank(b.code.as_str()))
        .then_with(|| a.code.cmp(&b.code))
        .then_with(|| a.target.cmp(&b.target))
        .then_with(|| {
            diagnostic_severity_rank(a.severity).cmp(&diagnostic_severity_rank(b.severity))
        })
        .then_with(|| a.blocking.cmp(&b.blocking).reverse())
        .then_with(|| a.evidence.cmp(&b.evidence))
        .then_with(|| a.expected.cmp(&b.expected))
        .then_with(|| a.actual.cmp(&b.actual))
        .then_with(|| format!("{:?}", a.repair_options).cmp(&format!("{:?}", b.repair_options)))
}

fn resource_diagnostic_rank(code: &str) -> u8 {
    match code {
        E_USE_AFTER_CLOSE | E_USE_AFTER_RELEASE => 0,
        E_DOUBLE_CLOSE | E_DOUBLE_RELEASE => 1,
        E_RESOURCE_LIFETIME_MISMATCH => 2,
        E_RESOURCE_LEAKED | E_LINEAR_NOT_CONSUMED => 3,
        E_RESOURCE_MISSING_CAPABILITY | E_SHARED_WITHOUT_SAFE_CAPABILITY => 4,
        E_RESOURCE_QUOTA_EXCEEDED => 5,
        E_RESOURCE_LIMIT_EXCEEDED => 6,
        E_RESOURCE_NO_LIFECYCLE_EDGE => 7,
        _ => 8,
    }
}

fn diagnostic_severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
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
