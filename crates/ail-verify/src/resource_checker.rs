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
// | `"use-after-release"`    | Use-after-release violation detected      |
// | `"double-release"`       | Double-release violation detected         |
// | `"never-consumed"`       | Linear resource never consumed (violation)|
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
// | linear + `never-consumed` tag          | Failed   |
// | shared + no `safe-capability` tag      | Unsafe   |
// | properly released/committed            | Proven   |
// | shared + `safe-capability`             | Proven   |

use ail_core::semantic_graph::SemanticGraph;

use crate::diagnostic::{Diagnostic, DiagnosticSeverity};
use crate::report::{SummaryCounts, VerificationEntry, VerificationReport, VerificationState};

// ── Error codes ───────────────────────────────────────────────────────────

pub const E_USE_AFTER_RELEASE: &str = "E_USE_AFTER_RELEASE";
pub const E_DOUBLE_RELEASE: &str = "E_DOUBLE_RELEASE";
pub const E_LINEAR_NOT_CONSUMED: &str = "E_LINEAR_NOT_CONSUMED";
pub const E_SHARED_WITHOUT_SAFE_CAPABILITY: &str = "E_SHARED_WITHOUT_SAFE_CAPABILITY";

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
            let claim = format!("resource-lifecycle[{}]", tm.level);

            let (state, evidence) = Self::classify_resource(resource_kind, tags, &scope);

            // Emit diagnostics for blocking violations
            match state {
                VerificationState::Failed => {
                    let code = if has_tag(tags, "use-after-release") {
                        E_USE_AFTER_RELEASE
                    } else if has_tag(tags, "double-release") {
                        E_DOUBLE_RELEASE
                    } else {
                        E_LINEAR_NOT_CONSUMED
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
                        code: E_SHARED_WITHOUT_SAFE_CAPABILITY.to_string(),
                        severity: DiagnosticSeverity::Error,
                        target: node.id,
                        evidence: evidence.clone(),
                        expected: None,
                        actual: None,
                        repair_options: vec![],
                        blocking: true,
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

    fn classify_resource(
        kind: ResourceKind,
        tags: &[String],
        scope: &str,
    ) -> (VerificationState, Option<String>) {
        // Violations take priority regardless of resource kind
        if has_tag(tags, "use-after-release") {
            return (
                VerificationState::Failed,
                Some(format!(
                    "resource '{}' used after release; use-after-release is always failed",
                    scope
                )),
            );
        }
        if has_tag(tags, "double-release") {
            return (
                VerificationState::Failed,
                Some(format!(
                    "resource '{}' released twice; double-release is always failed",
                    scope
                )),
            );
        }

        match kind {
            ResourceKind::Linear => {
                // Linear resources must be consumed exactly once.
                if has_tag(tags, "never-consumed") {
                    return (
                        VerificationState::Failed,
                        Some(format!(
                            "linear resource '{}' was never consumed; linear resources must be used exactly once",
                            scope
                        )),
                    );
                }
                if has_tag(tags, "released") {
                    (VerificationState::Proven, None)
                } else {
                    // No explicit release/consume tag — treat as unverified
                    (
                        VerificationState::Unverified,
                        Some(format!(
                            "linear resource '{}' has no 'released' or 'never-consumed' tag; lifecycle is unverified",
                            scope
                        )),
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
                    (VerificationState::Proven, None)
                } else {
                    // Not released — unverified (affine allows non-consumption)
                    (
                        VerificationState::Unverified,
                        Some(format!(
                            "affine resource '{}' has no release tag; lifecycle unverified",
                            scope
                        )),
                    )
                }
            }
            ResourceKind::Shared => {
                // Shared resources require concurrency-safe capability.
                if has_tag(tags, "safe-capability") {
                    (VerificationState::Proven, None)
                } else {
                    (
                        VerificationState::Unsafe,
                        Some(format!(
                            "shared resource '{}' lacks 'safe-capability' tag; concurrent access is unsafe without it",
                            scope
                        )),
                    )
                }
            }
        }
    }
}

// ── ResourceKind ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourceKind {
    Linear,
    Affine,
    Shared,
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
