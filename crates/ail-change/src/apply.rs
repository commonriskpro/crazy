// ── ail-change::apply ─────────────────────────────────────────────────────
//
// Atomic apply of a `CanonicalChangeSet` to a `SemanticGraph`.
//
// # Protocol
//
// 1. **Snapshot guard**: compare `cs.base_snapshot_id` against
//    `bridge.current_snapshot_id()`.  A mismatch returns
//    `ChangeSetOutcome::RebaseRequired` immediately, with no graph mutation.
//
// 2. **Clone-before-apply**: the graph is cloned before any mutation.
//    Any failure at any point restores the clone (rollback).
//
// 3. **Precondition evaluation**: `AssertExists` and `AssertHash` preconditions
//    are checked before the first op is applied.  Failure → `Failed` + rollback.
//
// 4. **Op application**: each `CanonicalOp` mutates the graph via its payload.
//    After each op `graph.validate()` is called; the first error → `Failed` +
//    rollback.
//
// 5. **Success**: all preconditions pass and all ops apply cleanly →
//    `ChangeSetOutcome::Applied`.

use ail_core::semantic_graph::{
    CapabilityReqs, ContractClauses, EffectRow, GraphEdge, GraphNode, NodeRef, ParamDecl,
    SemanticGraph, TrustMetadata,
};

use crate::{
    canonical::{CanonicalChangeSet, OpPayload, Precondition},
    model::{BlockHash, ChangeSetOutcome, SnapshotId},
};

// ── SnapshotBridge ────────────────────────────────────────────────────────

/// Abstraction over the storage layer for snapshot-identity checks.
///
/// The implementation is expected to be cheap — typically a single integer
/// read from an in-memory field or an atomic counter.
pub trait SnapshotBridge {
    /// Return the current (live) snapshot id of the graph being modified.
    fn current_snapshot_id(&self) -> SnapshotId;
}

// ── apply ─────────────────────────────────────────────────────────────────

/// Atomically apply a `CanonicalChangeSet` to a `SemanticGraph`.
///
/// Returns:
/// - `Applied` — all preconditions passed and all ops were applied cleanly.
/// - `RebaseRequired { current_snapshot_id }` — `cs.base_snapshot_id` does
///   not match the live snapshot id.  Graph is unmodified.
/// - `Failed { reason }` — a precondition failed or an op violated a graph
///   invariant.  Graph is restored to its pre-apply state (rollback).
pub fn apply(
    cs: CanonicalChangeSet,
    graph: &mut SemanticGraph,
    bridge: &dyn SnapshotBridge,
) -> ChangeSetOutcome {
    // ── Step 1: Snapshot guard ────────────────────────────────────────────
    let live_id = bridge.current_snapshot_id();
    if cs.base_snapshot_id != live_id {
        return ChangeSetOutcome::RebaseRequired {
            current_snapshot_id: live_id,
        };
    }

    // ── Step 2: Clone before any mutation ────────────────────────────────
    let rollback = graph.clone();

    // ── Step 3: Evaluate preconditions ───────────────────────────────────
    for precondition in &cs.preconditions {
        if let Some(reason) = evaluate_precondition(precondition, graph) {
            *graph = rollback;
            return ChangeSetOutcome::Failed { reason };
        }
    }

    // ── Step 4: Apply ops ─────────────────────────────────────────────────
    for op in &cs.ops {
        apply_payload(&op.payload, graph);

        if let Err(err) = graph.validate() {
            *graph = rollback;
            return ChangeSetOutcome::Failed {
                reason: format!("graph invariant violated: {err:?}"),
            };
        }
    }

    // ── Step 5: Success ───────────────────────────────────────────────────
    ChangeSetOutcome::Applied
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Evaluate a single precondition against the current graph state.
///
/// Returns `None` if the precondition passes, or `Some(reason)` if it fails.
fn evaluate_precondition(precondition: &Precondition, graph: &SemanticGraph) -> Option<String> {
    match precondition {
        Precondition::AssertExists(assert) => {
            let exists = graph.nodes.iter().any(|n| n.id == assert.node_id);
            if exists {
                None
            } else {
                Some(format!(
                    "AssertExists failed: node {:?} not found in graph",
                    assert.node_id
                ))
            }
        }
        Precondition::AssertHash(assert) => {
            // Minimal implementation: compute blake3 of the node's CBOR encoding
            // and compare against the expected hash.
            let node = graph.nodes.iter().find(|n| n.id == assert.node_id);
            match node {
                None => Some(format!(
                    "AssertHash failed: node {:?} not found in graph",
                    assert.node_id
                )),
                Some(n) => {
                    let computed = compute_node_hash(n);
                    if computed == assert.expected_hash {
                        None
                    } else {
                        Some(format!(
                            "AssertHash failed: node {:?} hash mismatch",
                            assert.node_id
                        ))
                    }
                }
            }
        }
    }
}

/// Apply an `OpPayload` to the graph (no validation — caller validates after).
fn apply_payload(payload: &OpPayload, graph: &mut SemanticGraph) {
    match payload {
        OpPayload::CreateNode(node) => {
            graph.nodes.push((**node).clone());
        }
        OpPayload::AddEdge(edge) => {
            graph.edges.push(edge.clone());
        }
        OpPayload::RemoveNode(node_id) => {
            graph.nodes.retain(|n| &n.id != node_id);
            // Also remove dangling edges whose source or target was this node.
            graph
                .edges
                .retain(|e| &e.source != node_id && &e.target != node_id);
        }
        OpPayload::SetNodeName { node_id, name } => {
            if let Some(n) = graph.nodes.iter_mut().find(|n| &n.id == node_id) {
                n.name.clone_from(name);
            }
        }
        OpPayload::RemoveNodeByName(name) => {
            if let Some(node_id) = node_id_by_name(graph, name) {
                graph.nodes.retain(|n| n.id != node_id);
                graph
                    .edges
                    .retain(|e| e.source != node_id && e.target != node_id);
            }
        }
        OpPayload::RenameNodeByName { target, name } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                n.name.clone_from(name);
            }
        }
        OpPayload::AddEdgeByName {
            source,
            target,
            kind,
        } => {
            if let (Some(source), Some(target)) = (
                node_id_by_name(graph, source),
                node_id_by_name(graph, target),
            ) {
                graph.edges.push(GraphEdge::new(source, target, *kind));
            }
        }
        OpPayload::RemoveEdgeByName {
            source,
            target,
            kind,
        } => {
            if let (Some(source), Some(target)) = (
                node_id_by_name(graph, source),
                node_id_by_name(graph, target),
            ) {
                graph
                    .edges
                    .retain(|e| e.source != source || e.target != target || e.kind != *kind);
            }
        }
        OpPayload::SetReturnByName { target, ty } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                n.return_type = Some(ty.clone());
            }
        }
        OpPayload::SetBodyByName { target, body } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                n.body_expr = Some(body.clone());
            }
        }
        OpPayload::SetMetadataByName { target, key, value } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                set_node_metadata(n, key, value);
            }
        }
        OpPayload::AddParamByName { target, name, ty } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                let params = n.params.get_or_insert_with(Vec::new);
                if !params.iter().any(|param| param.name == *name) {
                    params.push(ParamDecl {
                        name: name.clone(),
                        ty: ty.clone(),
                    });
                }
            }
        }
        OpPayload::AddEffectByName { target, effect } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                let row = n
                    .effect_row
                    .get_or_insert_with(|| EffectRow { effects: vec![] });
                if !row.effects.contains(effect) {
                    row.effects.push(effect.clone());
                }
            }
        }
        OpPayload::RemoveEffectByName { target, effect } => {
            if let Some(n) = node_by_name_mut(graph, target)
                && let Some(row) = &mut n.effect_row
            {
                row.effects.retain(|existing| existing != effect);
            }
        }
        OpPayload::AddContractByName { target, kind, rule } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                let clauses = n.contract_clauses.get_or_insert_with(|| ContractClauses {
                    requires: vec![],
                    ensures: vec![],
                });
                if kind == "requires" {
                    if !clauses.requires.contains(rule) {
                        clauses.requires.push(rule.clone());
                    }
                } else if !clauses.ensures.contains(rule) {
                    clauses.ensures.push(rule.clone());
                }
            }
        }
        OpPayload::RemoveContractByName { target, rule } => {
            if let Some(n) = node_by_name_mut(graph, target)
                && let Some(clauses) = &mut n.contract_clauses
            {
                clauses.requires.retain(|existing| existing != rule);
                clauses.ensures.retain(|existing| existing != rule);
            }
        }
        OpPayload::AddCapabilityReqByName { target, capability } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                let reqs = n
                    .capability_reqs
                    .get_or_insert_with(|| CapabilityReqs { caps: vec![] });
                if !reqs.caps.contains(capability) {
                    reqs.caps.push(capability.clone());
                }
            }
        }
        OpPayload::RemoveCapabilityReqByName { target, capability } => {
            if let Some(n) = node_by_name_mut(graph, target)
                && let Some(reqs) = &mut n.capability_reqs
            {
                reqs.caps.retain(|existing| existing != capability);
            }
        }
        OpPayload::SetVisibilityByName { target, visibility } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                n.visibility = Some(*visibility);
            }
        }
        OpPayload::AddBindingByName { target, binding } => {
            if let Some(n) = node_by_name_mut(graph, target)
                && !n.bindings.iter().any(|existing| existing == binding)
            {
                n.bindings.push(binding.clone());
            }
        }
        OpPayload::AddInferredFactByName { target, fact } => {
            if let Some(n) = node_by_name_mut(graph, target)
                && !n.inferred.iter().any(|existing| existing == fact)
            {
                n.inferred.push(fact.clone());
            }
        }
        OpPayload::AddDerivedImplByName { target, impl_name } => {
            if let Some(n) = node_by_name_mut(graph, target)
                && !n.derived_impls.contains(impl_name)
            {
                n.derived_impls.push(impl_name.clone());
            }
        }
        OpPayload::AddGeneratedArtifactByName { target, artifact } => {
            if let Some(n) = node_by_name_mut(graph, target)
                && !n
                    .generated_artifacts
                    .iter()
                    .any(|existing| existing == artifact)
            {
                n.generated_artifacts.push(artifact.clone());
            }
        }
        OpPayload::AddAssertionByName { target, assertion } => {
            if let Some(n) = node_by_name_mut(graph, target)
                && !n.assertions.iter().any(|existing| existing == assertion)
            {
                n.assertions.push(assertion.clone());
            }
        }
        OpPayload::SetWorkflowStateByName { target, state } => {
            if let Some(n) = node_by_name_mut(graph, target) {
                n.workflow_state = Some(*state);
            }
        }
        OpPayload::Noop => {
            // Intentional no-op: raw ChangeSet ops or malformed parsed ops.
        }
    }
}

fn node_id_by_name(graph: &SemanticGraph, name: &str) -> Option<NodeRef> {
    graph
        .nodes
        .iter()
        .find(|node| node.name == name)
        .map(|node| node.id)
}

fn node_by_name_mut<'a>(graph: &'a mut SemanticGraph, name: &str) -> Option<&'a mut GraphNode> {
    graph.nodes.iter_mut().find(|node| node.name == name)
}

fn set_node_metadata(node: &mut GraphNode, key: &str, value: &str) {
    match key {
        "return" | "type" => node.return_type = Some(value.to_string()),
        "body" => node.body_expr = Some(value.to_string()),
        _ => {
            let trust = node.trust_metadata.get_or_insert_with(|| TrustMetadata {
                level: ail_core::semantic_graph::TrustLevel::Custom("metadata".to_string()),
                tags: vec![],
            });
            let tag = format!("{key}={value}");
            if !trust.tags.contains(&tag) {
                trust.tags.push(tag);
            }
        }
    }
}

/// Compute blake3 hash of a node's CBOR encoding for `AssertHash` checks.
fn compute_node_hash(node: &ail_core::semantic_graph::GraphNode) -> BlockHash {
    let mut bytes: Vec<u8> = Vec::new();
    ciborium::into_writer(node, &mut bytes).expect("GraphNode serialization must not fail");
    BlockHash(*blake3::hash(&bytes).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical::canonicalize_parsed, parser::parse_changeset};
    use ail_core::semantic_graph::{NodeKind, SemanticGraph, Visibility, WorkflowState};

    struct TestBridge;

    impl SnapshotBridge for TestBridge {
        fn current_snapshot_id(&self) -> SnapshotId {
            SnapshotId(0)
        }
    }

    #[test]
    fn apply_parsed_function_create_payload_inserts_graph_node() {
        let parsed = parse_changeset(
            "change e2e base=0\nauthor tester\ndescription e2e\nop create_function id=fn.answer return=Int value=42\nend\n",
        )
        .expect("fixture must parse");
        let canonical = canonicalize_parsed(parsed);
        let mut graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };

        let outcome = apply(canonical, &mut graph, &TestBridge);

        assert_eq!(outcome, ChangeSetOutcome::Applied);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].kind, NodeKind::Function);
        assert_eq!(graph.nodes[0].name, "fn.answer");
    }

    #[test]
    fn apply_parsed_representable_ops_mutates_graph() {
        let parsed = parse_changeset(
            "\
change e2e base=0
author tester
description e2e
op create_module id=module.checkout
op create_capability id=cap.payment.charge
op create_function id=fn.checkout
op add_param target=fn.checkout name=cart_id type=CartId
op set_return target=fn.checkout type=OrderId
op set_body target=fn.checkout body=add(x, y)
op add_effect target=fn.checkout effect=payment.charge
op add_contract target=fn.checkout kind=ensures rule=order_created
op connect source=fn.checkout_v2 relation=uses target=cap.payment.charge
op grant target=module.checkout capability=payment.charge
op rename target=fn.checkout name=fn.checkout_v2
op move target=fn.checkout_v2 to=module.checkout
op deprecate target=fn.checkout_v2 replacement=fn.checkout_v3
op annotate target=fn.checkout_v2 key=rationale value=idempotent
end
",
        )
        .expect("fixture must parse");
        let canonical = canonicalize_parsed(parsed);
        let mut graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };

        let outcome = apply(canonical, &mut graph, &TestBridge);

        assert_eq!(outcome, ChangeSetOutcome::Applied);
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.edges.len(), 1);
        let module = graph
            .nodes
            .iter()
            .find(|node| node.name == "module.checkout")
            .expect("module must exist");
        assert_eq!(module.kind, NodeKind::Module);
        assert_eq!(
            module
                .capability_reqs
                .as_ref()
                .map(|reqs| reqs.caps.as_slice()),
            Some(["payment.charge".to_string()].as_slice())
        );
        let function = graph
            .nodes
            .iter()
            .find(|node| node.name == "fn.checkout_v2")
            .expect("renamed function must exist");
        assert_eq!(function.return_type.as_deref(), Some("OrderId"));
        assert_eq!(function.body_expr.as_deref(), Some("add(x, y)"));
        assert_eq!(
            function
                .params
                .as_ref()
                .and_then(|params| params.first())
                .map(|param| (param.name.as_str(), param.ty.as_str())),
            Some(("cart_id", "CartId"))
        );
        assert_eq!(
            function
                .effect_row
                .as_ref()
                .map(|row| row.effects.as_slice()),
            Some(["payment.charge".to_string()].as_slice())
        );
        assert_eq!(
            function
                .contract_clauses
                .as_ref()
                .map(|clauses| clauses.ensures.as_slice()),
            Some(["order_created".to_string()].as_slice())
        );
        let tags = &function
            .trust_metadata
            .as_ref()
            .expect("metadata tags must exist")
            .tags;
        assert!(tags.contains(&"module=module.checkout".to_string()));
        assert!(tags.contains(&"deprecated_replacement=fn.checkout_v3".to_string()));
        assert!(tags.contains(&"rationale=idempotent".to_string()));
    }

    #[test]
    fn apply_parsed_remove_ops_mutate_graph() {
        let parsed = parse_changeset(
            "\
change e2e base=0
author tester
description e2e
op create_capability id=cap.payment.charge
op create_function id=fn.checkout
op add_effect target=fn.checkout effect=payment.charge
op add_contract target=fn.checkout kind=ensures rule=order_created
op connect source=fn.checkout relation=uses target=cap.payment.charge
op remove_effect target=fn.checkout effect=payment.charge
op remove_contract target=fn.checkout rule=order_created
op disconnect source=fn.checkout relation=uses target=cap.payment.charge
op delete target=cap.payment.charge
end
",
        )
        .expect("fixture must parse");
        let canonical = canonicalize_parsed(parsed);
        let mut graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };

        let outcome = apply(canonical, &mut graph, &TestBridge);

        assert_eq!(outcome, ChangeSetOutcome::Applied);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].name, "fn.checkout");
        assert!(graph.edges.is_empty());
        assert!(
            graph.nodes[0]
                .effect_row
                .as_ref()
                .is_none_or(|row| row.effects.is_empty())
        );
        assert!(
            graph.nodes[0]
                .contract_clauses
                .as_ref()
                .is_none_or(|clauses| clauses.requires.is_empty() && clauses.ensures.is_empty())
        );
    }

    #[test]
    fn apply_parsed_semantic_graph_ops_mutate_node_metadata() {
        let parsed = parse_changeset(
            "\
change e2e base=0
author tester
description e2e
op create_capability id=payment.charge
op create_function id=fn.checkout
op infer_boundary target=fn.checkout
op bind_handler capability=payment.charge handler=handler.Stripe profile=prod
op expose target=fn.checkout as=api.checkout
op derive_eq target=fn.checkout mode=structural
op generate_tests target=fn.checkout from=contracts
op assert_hash target=fn.checkout hash=sig_123
op lock_behavior target=fn.checkout
op refactor_inline target=fn.checkout
op migrate_api target=fn.checkout from=sig.v1 to=sig.v2
op approve_inferred_boundary target=fn.checkout version=sig_123
op reject_inferred_boundary target=fn.checkout version=sig_124
op verify target=fn.checkout
op hide target=fn.checkout
end
",
        )
        .expect("fixture must parse");
        let canonical = canonicalize_parsed(parsed);
        let mut graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };

        let outcome = apply(canonical, &mut graph, &TestBridge);

        assert_eq!(outcome, ChangeSetOutcome::Applied);
        let capability = graph
            .nodes
            .iter()
            .find(|node| node.name == "payment.charge")
            .expect("capability must exist");
        assert_eq!(capability.bindings.len(), 1);
        assert_eq!(capability.bindings[0].implementation, "handler.Stripe");
        assert_eq!(capability.bindings[0].profile.as_deref(), Some("prod"));

        let function = graph
            .nodes
            .iter()
            .find(|node| node.name == "fn.checkout")
            .expect("function must exist");
        assert_eq!(function.visibility, Some(Visibility::Private));
        assert_eq!(function.inferred[0].kind, "boundary");
        assert_eq!(function.derived_impls, vec!["eq".to_string()]);
        assert_eq!(function.generated_artifacts[0].kind, "tests");
        assert_eq!(
            function.generated_artifacts[0].source.as_deref(),
            Some("contracts")
        );
        assert_eq!(function.assertions[0].kind, "hash");
        assert_eq!(function.assertions[0].value, "sig_123");
        assert_eq!(function.workflow_state, Some(WorkflowState::Verified));
    }
}
