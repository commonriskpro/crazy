// ── ail-verify::concurrency_checker ──────────────────────────────────────
//
// Concurrency safety checker — verification layer 9 per verification.md.
//
// # Responsibility
//
// `ConcurrencyChecker` inspects `GraphNode` items carrying concurrency
// lifecycle metadata and emits `VerificationEntry` items classifying each
// concurrency primitive's safety state.
//
// # Concurrency node encoding (trust_metadata)
//
// | `level`                  | Meaning                                     |
// |--------------------------|---------------------------------------------|
// | `"task"`                 | An async/concurrent task                    |
// | `"task-group"`           | A structured task group                     |
// | `"channel"`              | A typed communication channel               |
// | `"shared-state"`         | Shared mutable state                        |
// | `"cell-crossing"`        | Cell<T> value crossing a task boundary      |
// | `"unbounded-concurrency"`| Concurrency without explicit bound          |
//
// Lifecycle/safety tags (in `trust_metadata.tags`):
//
// | Tag                      | Meaning                                   |
// |--------------------------|-------------------------------------------|
// | `"awaited"`              | Task was properly awaited                 |
// | `"cancelled"`            | Task was properly cancelled               |
// | `"transferred"`          | Task/channel was transferred              |
// | `"orphan"`               | Task is orphaned (not awaited/cancelled)  |
// | `"closed"`               | Task group / channel was closed           |
// | `"safe-capability"`      | Shared state has concurrency-safe cap     |
// | `"policy-approved"`      | Unbounded concurrency is policy-approved  |
//
// # Verification states
//
// | Condition                                    | State       |
// |----------------------------------------------|-------------|
// | task with `"orphan"` tag                     | Failed      |
// | task-group not closed                        | Failed      |
// | `cell-crossing` node (always failed)         | Failed      |
// | shared-state without `safe-capability`       | Unsafe      |
// | task awaited/cancelled/transferred           | Proven      |
// | task-group closed                            | Proven      |
// | channel closed/transferred                   | Proven      |
// | shared-state with `safe-capability`          | Proven      |
// | unbounded-concurrency + policy-approved      | Assumed     |
// | unbounded-concurrency without approval       | Unverified  |
// | channel without close/transfer tag           | Unverified  |

use ail_core::semantic_graph::{EdgeKind, NodeKind, NodeRef, SemanticGraph};

use crate::diagnostic::{Diagnostic, DiagnosticSeverity};
use crate::report::{SummaryCounts, VerificationEntry, VerificationReport, VerificationState};

// ── Error codes ───────────────────────────────────────────────────────────

pub const E_ORPHAN_TASK: &str = "E_ORPHAN_TASK";
pub const E_UNCLOSED_TASK_GROUP: &str = "E_UNCLOSED_TASK_GROUP";
pub const E_CELL_CROSSING_BOUNDARY: &str = "E_CELL_CROSSING_BOUNDARY";
pub const E_SHARED_STATE_UNSAFE: &str = "E_SHARED_STATE_UNSAFE";

// ── ConcurrencyChecker ────────────────────────────────────────────────────

/// Pure, stateless concurrency safety checker.
///
/// Call [`ConcurrencyChecker::check`] with a `SemanticGraph` to receive a
/// `VerificationReport` with one entry per concurrency primitive node.
pub struct ConcurrencyChecker;

impl ConcurrencyChecker {
    /// Walk `graph` and classify each concurrency primitive's safety state.
    ///
    /// Non-concurrency nodes are silently skipped.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();

        for node in &graph.nodes {
            let Some(tm) = &node.trust_metadata else {
                continue;
            };

            let kind = match tm.level.as_str() {
                "task" => ConcurrencyKind::Task,
                "task-group" => ConcurrencyKind::TaskGroup,
                "channel" => ConcurrencyKind::Channel,
                "shared-state" => ConcurrencyKind::SharedState,
                "cell-crossing" => ConcurrencyKind::CellCrossing,
                "unbounded-concurrency" => ConcurrencyKind::UnboundedConcurrency,
                _ => continue,
            };

            let tags = &tm.tags;
            let scope = node.name.clone();
            let claim = format!("concurrency-safety[{}]", tm.level.as_str());

            let (state, evidence) = Self::classify(kind, tags, &scope, node.id, graph);

            // Emit diagnostics for blocking conditions
            match state {
                VerificationState::Failed => {
                    let code = match kind {
                        ConcurrencyKind::Task => E_ORPHAN_TASK,
                        ConcurrencyKind::TaskGroup => E_UNCLOSED_TASK_GROUP,
                        ConcurrencyKind::CellCrossing => E_CELL_CROSSING_BOUNDARY,
                        _ => E_ORPHAN_TASK, // fallback
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
                        code: E_SHARED_STATE_UNSAFE.to_string(),
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

            let blocking =
                matches!(state, VerificationState::Failed | VerificationState::Unsafe);
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

    fn classify(
        kind: ConcurrencyKind,
        tags: &[String],
        scope: &str,
        node_id: NodeRef,
        graph: &SemanticGraph,
    ) -> (VerificationState, Option<String>) {
        match kind {
            ConcurrencyKind::Task => {
                if has_tag(tags, "orphan") {
                    return (
                        VerificationState::Failed,
                        Some(format!(
                            "task '{}' is orphaned (not awaited, cancelled, or transferred); orphan tasks are always failed",
                            scope
                        )),
                    );
                }
                if has_tag(tags, "awaited")
                    || has_tag(tags, "cancelled")
                    || has_tag(tags, "transferred")
                {
                    return (VerificationState::Proven, None);
                }
                // TASK-20: no lifecycle tag — check for SpawnedBy/ChildOf edge to TaskGroup
                let has_parent_scope = graph.edges.iter().any(|e| {
                    e.source == node_id
                        && matches!(e.kind, EdgeKind::SpawnedBy | EdgeKind::ChildOf)
                        && graph.nodes.iter().any(|n| {
                            n.id == e.target
                                && n.trust_metadata
                                    .as_ref()
                                    .map(|tm| tm.level.as_str() == "task-group")
                                    .unwrap_or(false)
                        })
                });
                let evidence = if has_parent_scope {
                    format!(
                        "task '{}' has no lifecycle tag (awaited/cancelled/transferred); lifecycle unverified",
                        scope
                    )
                } else {
                    format!(
                        "task '{}' has no lifecycle tag and no parent TaskGroup scope edge; potential orphan scope",
                        scope
                    )
                };
                (VerificationState::Unverified, Some(evidence))
            }

            ConcurrencyKind::TaskGroup => {
                if has_tag(tags, "closed") {
                    (VerificationState::Proven, None)
                } else {
                    (
                        VerificationState::Failed,
                        Some(format!(
                            "task group '{}' scope is not closed; all task groups must close cleanly",
                            scope
                        )),
                    )
                }
            }

            ConcurrencyKind::Channel => {
                if has_tag(tags, "closed") || has_tag(tags, "transferred") {
                    (VerificationState::Proven, None)
                } else {
                    (
                        VerificationState::Unverified,
                        Some(format!(
                            "channel '{}' has no 'closed' or 'transferred' tag; lifecycle unverified",
                            scope
                        )),
                    )
                }
            }

            ConcurrencyKind::SharedState => {
                if has_tag(tags, "safe-capability") {
                    (VerificationState::Proven, None)
                } else {
                    (
                        VerificationState::Unsafe,
                        Some(format!(
                            "shared state '{}' lacks 'safe-capability' tag; concurrent mutable access is unsafe",
                            scope
                        )),
                    )
                }
            }

            ConcurrencyKind::CellCrossing => {
                // Cell<T> crossing a task boundary is always Failed per spec
                (
                    VerificationState::Failed,
                    Some(format!(
                        "Cell<T> '{}' crosses a task boundary; this is always a concurrency violation",
                        scope
                    )),
                )
            }

            ConcurrencyKind::UnboundedConcurrency => {
                if has_tag(tags, "policy-approved") {
                    (
                        VerificationState::Assumed,
                        Some(format!(
                            "unbounded concurrency '{}' is policy-approved; treated as assumed",
                            scope
                        )),
                    )
                } else {
                    (
                        VerificationState::Unverified,
                        Some(format!(
                            "unbounded concurrency '{}' without policy approval; unverified by default",
                            scope
                        )),
                    )
                }
            }
        }
    }
}

// ── ConcurrencyKind ───────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConcurrencyKind {
    Task,
    TaskGroup,
    Channel,
    SharedState,
    CellCrossing,
    UnboundedConcurrency,
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
