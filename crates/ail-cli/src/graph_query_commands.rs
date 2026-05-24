// ── ail-cli::graph_query_commands ────────────────────────────────────────
//
// Read-only semantic graph query handlers: impact, callers, effects, proofs.
//
// All four commands share the same pattern:
//   1. Resolve the target name to node refs.
//   2. Traverse the matching edge kinds.
//   3. Emit hash-bound output tied to the current snapshot.
//
// None of these commands mutate the graph.

use ail_core::semantic_graph::EdgeKind;
use serde_json::{Value, json};

use crate::cli::{load_current_graph_for_cli, node_refs_for_name, target_node_name};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;

// ── Shared snapshot identity ──────────────────────────────────────────────

/// Fetch snapshot identity strings from the store for output binding.
async fn snapshot_identity(store: &StoreHandle) -> (String, String) {
    let snapshots = store.list_snapshots().await.unwrap_or_default();
    let snapshot_id = snapshots
        .last()
        .map(|s| s.id.to_hex())
        .unwrap_or_else(|| "(no snapshot)".to_string());
    let snapshot_hash = snapshots
        .last()
        .map(|s| s.graph_root_hash.to_hex())
        .unwrap_or_else(|| "(no hash)".to_string());
    (snapshot_id, snapshot_hash)
}

// ── Command handlers ──────────────────────────────────────────────────────

/// `ail impact <target>` — list nodes that would be affected if `target` changes.
///
/// Traverses `DependsOn` and `BreaksIfChanged` edges FROM target nodes.
/// Output is hash-bound to the current snapshot.
pub(crate) async fn cmd_impact(
    mode: OutputMode,
    target: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    let (snapshot_id, snapshot_hash) = snapshot_identity(store).await;
    let graph = load_current_graph_for_cli(store).await?;
    let name = target_node_name(target);
    let source_refs = node_refs_for_name(&graph, name);

    // Collect nodes reachable via DependsOn or BreaksIfChanged from target.
    let affected: Vec<Value> = graph
        .edges
        .iter()
        .filter(|e| {
            source_refs.contains(&e.source)
                && matches!(e.kind, EdgeKind::DependsOn | EdgeKind::BreaksIfChanged)
        })
        .filter_map(|e| {
            graph.nodes.iter().find(|n| n.id == e.target).map(|n| {
                json!({
                    "node": n.name,
                    "kind": format!("{:?}", n.kind),
                    "edge": format!("{:?}", e.kind),
                })
            })
        })
        .collect();

    let human_msg = format!(
        "target: {target}\nsnapshot: {snapshot_id}\nhash: {snapshot_hash}\naffected: {}",
        affected.len()
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "target": target,
            "snapshot_id": snapshot_id,
            "snapshot_hash": snapshot_hash,
            "affected_nodes": affected,
        }),
    );
    Ok(())
}

/// `ail callers <target>` — list all callers of a function/node target.
///
/// Traverses `Calls` edges whose target is the named node.
/// Output is hash-bound to the current snapshot.
pub(crate) async fn cmd_callers(
    mode: OutputMode,
    target: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    let (snapshot_id, snapshot_hash) = snapshot_identity(store).await;
    let graph = load_current_graph_for_cli(store).await?;
    let name = target_node_name(target);
    let target_refs = node_refs_for_name(&graph, name);

    // Collect nodes with Calls edges pointing INTO the target.
    let callers: Vec<Value> = graph
        .edges
        .iter()
        .filter(|e| target_refs.contains(&e.target) && e.kind == EdgeKind::Calls)
        .filter_map(|e| {
            graph.nodes.iter().find(|n| n.id == e.source).map(|n| {
                json!({
                    "node": n.name,
                    "kind": format!("{:?}", n.kind),
                })
            })
        })
        .collect();

    let human_msg = format!(
        "target: {target}\nsnapshot: {snapshot_id}\nhash: {snapshot_hash}\ncallers: {}",
        callers.len()
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "target": target,
            "snapshot_id": snapshot_id,
            "snapshot_hash": snapshot_hash,
            "callers": callers,
        }),
    );
    Ok(())
}

/// `ail effects <target>` — show effects emitted by a module target.
///
/// Traverses `Emits` edges FROM the named node.
/// Output is hash-bound to the current snapshot.
pub(crate) async fn cmd_effects(
    mode: OutputMode,
    target: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    let (snapshot_id, snapshot_hash) = snapshot_identity(store).await;
    let graph = load_current_graph_for_cli(store).await?;
    let name = target_node_name(target);
    let source_refs = node_refs_for_name(&graph, name);

    // Collect effect nodes reachable via Emits edges FROM the target.
    let effects: Vec<Value> = graph
        .edges
        .iter()
        .filter(|e| source_refs.contains(&e.source) && e.kind == EdgeKind::Emits)
        .filter_map(|e| {
            graph.nodes.iter().find(|n| n.id == e.target).map(|n| {
                json!({
                    "effect": n.name,
                    "kind": format!("{:?}", n.kind),
                })
            })
        })
        .collect();

    let human_msg = format!(
        "target: {target}\nsnapshot: {snapshot_id}\nhash: {snapshot_hash}\neffects: {}",
        effects.len()
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "target": target,
            "snapshot_id": snapshot_id,
            "snapshot_hash": snapshot_hash,
            "effects": effects,
        }),
    );
    Ok(())
}

/// `ail proofs <target>` — show proof obligations for an invariant target.
///
/// Traverses `Proves` edges associated with the named node.
/// Output is hash-bound to the current snapshot.
pub(crate) async fn cmd_proofs(
    mode: OutputMode,
    target: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    let (snapshot_id, snapshot_hash) = snapshot_identity(store).await;
    let graph = load_current_graph_for_cli(store).await?;
    let name = target_node_name(target);
    let target_refs = node_refs_for_name(&graph, name);

    // Collect proof obligations: Proves edges FROM any node TO the target,
    // plus Proves edges FROM the target TO any node.
    let obligations: Vec<Value> = graph
        .edges
        .iter()
        .filter(|e| {
            e.kind == EdgeKind::Proves
                && (target_refs.contains(&e.source) || target_refs.contains(&e.target))
        })
        .map(|e| {
            let prover = graph
                .nodes
                .iter()
                .find(|n| n.id == e.source)
                .map(|n| n.name.as_str())
                .unwrap_or("?");
            let claim = graph
                .nodes
                .iter()
                .find(|n| n.id == e.target)
                .map(|n| n.name.as_str())
                .unwrap_or("?");
            json!({
                "prover": prover,
                "claim": claim,
            })
        })
        .collect();

    let human_msg = format!(
        "target: {target}\nsnapshot: {snapshot_id}\nhash: {snapshot_hash}\nproof_obligations: {}",
        obligations.len()
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "target": target,
            "snapshot_id": snapshot_id,
            "snapshot_hash": snapshot_hash,
            "proof_obligations": obligations,
        }),
    );
    Ok(())
}
