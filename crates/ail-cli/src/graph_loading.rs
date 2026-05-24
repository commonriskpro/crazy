// ── ail-cli::graph_loading ───────────────────────────────────────────────
//
// Graph loading and context-graph helper functions extracted from cli.rs.
//
// Re-exported by `crate::cli` via `pub(crate) use crate::graph_loading::*`
// so that existing `use crate::cli::load_current_graph_for_cli` (and similar)
// imports in other modules, and `use super::*` in cli_tests.rs, continue to
// compile without any change.

use ail_change::{canonical::canonicalize_parsed, parser::parse_changeset};
use ail_context::{
    ContextServer, ContextServerConfig, DerivedIndexCache, FieldRedactionRule,
    InMemoryContextSource, QueryBudget, QueryScope, TrustLevel as ContextTrustLevel,
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, Provenance, SemanticGraph};
use ail_storage::{
    SnapshotEnvelope,
    codec::{CborCodec, ContentCodec},
    object::ObjectId,
};

use crate::cli_helpers::{
    SimpleSnapshotBridge, changeset_outcome_message, latest_snapshot, unix_ms_now,
};
use crate::error::CliError;
use crate::store::StoreHandle;

// Re-export types that cli_tests.rs accesses via `use super::*` and that
// are no longer directly imported in cli.rs after this extraction.
pub(crate) use ail_change::model::SnapshotId;
pub(crate) use ail_context::ContextQuery;

// ── Context-server helpers ────────────────────────────────────────────────

pub(crate) async fn context_server_for_cli(
    store: &StoreHandle,
    snapshots: &[SnapshotEnvelope],
) -> Result<(ContextServer<InMemoryContextSource>, SnapshotEnvelope), CliError> {
    let graph = load_current_graph_for_cli(store).await?;
    let snapshot = match store
        .head_snapshot()
        .await?
        .or_else(|| latest_snapshot(snapshots).cloned())
    {
        Some(snapshot) => snapshot,
        None => synthetic_context_snapshot(&graph)?,
    };
    let source = InMemoryContextSource::new();
    source.insert_snapshot(snapshot.clone());
    source.insert_graph(snapshot.graph_root_hash, graph);

    let config = ContextServerConfig {
        redaction_rules: vec![
            FieldRedactionRule {
                field: "body_expr".to_string(),
                min_trust: ContextTrustLevel::Privileged,
                category: "restricted business logic".to_string(),
            },
            FieldRedactionRule {
                field: "runtime_checks".to_string(),
                min_trust: ContextTrustLevel::Internal,
                category: "runtime payloads".to_string(),
            },
        ],
        ..Default::default()
    };
    let mut server = ContextServer::new(source).with_config(config);
    if let Some(path) = store.context_index_path() {
        server = server.with_index_cache(DerivedIndexCache::new(path));
    }
    Ok((server, snapshot))
}

fn synthetic_context_snapshot(graph: &SemanticGraph) -> Result<SnapshotEnvelope, CliError> {
    let bytes = CborCodec
        .encode(graph)
        .map_err(|e| CliError::Domain(format!("graph encoding failed: {e}")))?;
    let root = ObjectId::from_bytes(&bytes);
    Ok(SnapshotEnvelope {
        id: ObjectId::from_bytes(b"synthetic-context-snapshot"),
        graph_root_hash: root,
        parent_id: None,
        applied_change_id: None,
        created_at: unix_ms_now(),
        verification_report_hash: None,
        ..Default::default()
    })
}

pub(crate) async fn parse_context_query_for_cli(
    kind: &str,
    args: &[String],
    store: &StoreHandle,
) -> Result<ContextQuery, CliError> {
    let graph = load_current_graph_for_cli(store).await?;
    let budget = QueryBudget::default();
    let target = || -> Result<NodeRef, CliError> {
        let raw = args.first().map(String::as_str).unwrap_or("0");
        node_ref_for_cli_target(raw, &graph)
    };
    match kind {
        "context" => Ok(ContextQuery::Node {
            target: target()?,
            scope: QueryScope::Full,
            budget,
        }),
        "graph" => Ok(ContextQuery::Graph {
            scope: QueryScope::Full,
            budget,
        }),
        "impact" => Ok(ContextQuery::Impact {
            target: target()?,
            budget,
        }),
        "callers" => Ok(ContextQuery::Callers {
            target: target()?,
            transitive: true,
            budget,
        }),
        "callees" => Ok(ContextQuery::Callees {
            target: target()?,
            transitive: true,
            budget,
        }),
        "effects" => Ok(ContextQuery::Effects {
            target: target()?,
            budget,
        }),
        "contracts" => Ok(ContextQuery::Contracts {
            target: target()?,
            budget,
        }),
        "history" => Ok(ContextQuery::History {
            target: target()?,
            budget,
        }),
        "why" => Ok(ContextQuery::Why {
            target: target()?,
            budget,
        }),
        "proofs" | "obligations" => Ok(ContextQuery::Proofs {
            target: target()?,
            budget,
        }),
        "resources" => Ok(ContextQuery::Resources {
            target: target()?,
            budget,
        }),
        "boundaries" => Ok(ContextQuery::Boundaries {
            target: target()?,
            budget,
        }),
        "refactor_context" => Ok(ContextQuery::RefactorContext {
            target: target()?,
            budget,
        }),
        "runtime" => Ok(ContextQuery::Runtime {
            target: target()?,
            profile: "dev".to_string(),
            budget,
        }),
        "concurrency" => Ok(ContextQuery::Concurrency {
            target: target()?,
            budget,
        }),
        "tasks" => Ok(ContextQuery::Tasks {
            target: target()?,
            budget,
        }),
        "diff" => Ok(ContextQuery::Diff {
            snapshot_a: None,
            snapshot_b: None,
            budget,
        }),
        "risks" => Ok(ContextQuery::Risks {
            target: target()?,
            budget,
        }),
        "todo" => Ok(ContextQuery::Todo {
            target: target()?,
            budget,
        }),
        "extract_candidates" => Ok(ContextQuery::ExtractCandidates {
            target: target()?,
            budget,
        }),
        "move_safety" => {
            let destination_raw = args.get(1).map(String::as_str).unwrap_or("0");
            let destination = node_ref_for_cli_target(destination_raw, &graph)?;
            Ok(ContextQuery::MoveSafety {
                target: target()?,
                destination,
                budget,
            })
        }
        "capabilities" => Ok(ContextQuery::Capabilities {
            target: target()?,
            profile: "dev".to_string(),
            budget,
        }),
        "handlers" => Ok(ContextQuery::Handlers {
            target: target()?,
            profile: "dev".to_string(),
            budget,
        }),
        "assumptions" => Ok(ContextQuery::Assumptions {
            target: target()?,
            budget,
        }),
        other => Err(CliError::ParseError(format!(
            "unsupported context query type: {other}"
        ))),
    }
}

fn node_ref_for_cli_target(target: &str, graph: &SemanticGraph) -> Result<NodeRef, CliError> {
    if let Ok(id) = target.parse::<u32>() {
        return Ok(NodeRef(id));
    }
    if let Some(node) = graph.nodes.iter().find(|node| node.name == target) {
        return Ok(node.id);
    }
    graph
        .nodes
        .first()
        .map(|node| node.id)
        .ok_or_else(|| CliError::NotFound(format!("node not found: {target}")))
}

// ── Graph loading helpers ─────────────────────────────────────────────────

/// Build the fallback CLI graph when no snapshot is available.
///
/// Applies a minimal hard-coded ChangeSet to produce a graph with
/// `fn.answer` and `fn.checkout` — used by tests and by commands that
/// run without a persisted project.
pub(crate) fn current_graph_for_cli() -> Result<SemanticGraph, CliError> {
    let source = "change e2e base=0\nauthor cli\ndescription e2e\nop create_function id=fn.answer return=Int value=42\nend\n";
    let parsed = parse_changeset(source).map_err(|e| CliError::Domain(format!("parse: {e}")))?;
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let bridge = SimpleSnapshotBridge(SnapshotId(0));
    match ail_change::apply::apply(canonical, &mut graph, &bridge) {
        ail_change::model::ChangeSetOutcome::Applied => {
            for node in &mut graph.nodes {
                node.provenance = Some(Provenance {
                    change_id: "change.e2e".to_string(),
                });
            }
            if !graph.nodes.iter().any(|node| node.name == "fn.checkout") {
                let mut checkout = GraphNode::new(
                    NodeRef(graph.nodes.len() as u32),
                    NodeKind::Function,
                    "fn.checkout",
                );
                checkout.provenance = Some(Provenance {
                    change_id: "change.e2e".to_string(),
                });
                graph.nodes.push(checkout);
            }
            Ok(graph)
        }
        other => Err(CliError::Domain(format!(
            "Failed to build fallback graph: {}",
            changeset_outcome_message(&other)
        ))),
    }
}

pub(crate) async fn load_current_graph_for_cli(
    store: &StoreHandle,
) -> Result<SemanticGraph, CliError> {
    let snapshots = store.list_snapshots().await?;
    let head_snapshot = store.head_snapshot().await?;
    let Some(snapshot) = head_snapshot
        .as_ref()
        .or_else(|| latest_snapshot(&snapshots))
    else {
        return if store.has_persistent_project() {
            Ok(SemanticGraph {
                nodes: vec![],
                edges: vec![],
            })
        } else {
            current_graph_for_cli()
        };
    };

    match store.load_graph(&snapshot.graph_root_hash).await? {
        Some(graph) => Ok(graph),
        None if store.has_persistent_project() => current_graph_for_cli(),
        None => current_graph_for_cli(),
    }
}

pub(crate) async fn load_current_graph_with_snapshot_id_for_cli(
    store: &StoreHandle,
) -> Result<(SemanticGraph, SnapshotId), CliError> {
    let snapshots = store.list_snapshots().await?;
    let head_snapshot = store.head_snapshot().await?;
    let Some(snapshot) = head_snapshot
        .as_ref()
        .or_else(|| latest_snapshot(&snapshots))
    else {
        return if store.has_persistent_project() {
            Ok((
                SemanticGraph {
                    nodes: vec![],
                    edges: vec![],
                },
                SnapshotId(0),
            ))
        } else {
            current_graph_for_cli().map(|graph| (graph, SnapshotId(0)))
        };
    };

    let graph = match store.load_graph(&snapshot.graph_root_hash).await? {
        Some(graph) => Ok(graph),
        None if store.has_persistent_project() => current_graph_for_cli(),
        None => current_graph_for_cli(),
    }?;
    let snapshot_id = snapshot_id_from_parent_chain(store, snapshot).await?;
    Ok((graph, snapshot_id))
}

async fn snapshot_id_from_parent_chain(
    store: &StoreHandle,
    snapshot: &SnapshotEnvelope,
) -> Result<SnapshotId, CliError> {
    let mut depth = 0;
    let mut current = snapshot.clone();
    let mut seen = vec![current.id];

    while let Some(parent_id) = current.parent_id {
        if seen.contains(&parent_id) {
            return Err(CliError::Domain(format!(
                "snapshot parent cycle detected at {}",
                parent_id.to_hex()
            )));
        }
        let Some(parent) = store.load_snapshot(&parent_id).await? else {
            return Err(CliError::Domain(format!(
                "snapshot parent not found: {}",
                parent_id.to_hex()
            )));
        };
        depth += 1;
        seen.push(parent_id);
        current = parent;
    }

    Ok(SnapshotId(depth))
}
