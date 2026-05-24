// ── ail-verify::pipeline::core_stages ────────────────────────────────────
//
// Stages 10 and 12 helpers: refinement type checking and invariant impact
// analysis via BFS reachability.
//
// All functions are pure, behavior-preserving extractions from the original
// `pipeline.rs` godfile.  Called by `VerificationPipeline::run_with_changeset`
// in the parent module.

use std::collections::{BTreeSet, VecDeque};

use ail_core::semantic_graph::SemanticGraph;

use crate::proof::{ClauseRole, ProofObligation};
use crate::report::{VerificationEntry, VerificationState};
use crate::solver::{Solver, SolverOutcome};

use super::stage_entry;

// ── Stage 10: Check refinements ───────────────────────────────────────────

pub(super) fn check_refinements(
    graph: &SemanticGraph,
    solver: &dyn Solver,
) -> Vec<VerificationEntry> {
    let mut entries = Vec::new();
    for node in &graph.nodes {
        let Some(refinement) = &node.refinement_ref else {
            continue;
        };
        let state = if refinement.predicate.trim().is_empty()
            || refinement.predicate.trim() == "false"
        {
            VerificationState::Failed
        } else if refinement.status == ail_core::semantic_graph::RefinementStatus::RuntimeChecked
            && node
                .runtime_checks
                .as_ref()
                .is_some_and(|checks| !checks.is_empty())
        {
            VerificationState::RuntimeChecked
        } else {
            match refinement.status {
                ail_core::semantic_graph::RefinementStatus::Proven => VerificationState::Proven,
                ail_core::semantic_graph::RefinementStatus::RuntimeChecked => {
                    VerificationState::Failed
                }
                ail_core::semantic_graph::RefinementStatus::Assumed => VerificationState::Assumed,
                ail_core::semantic_graph::RefinementStatus::Unverified => {
                    // TASK-10: try solver for Unverified refinements
                    let obligation = ProofObligation {
                        predicate: refinement.predicate.clone(),
                        role: ClauseRole::Requires,
                        scope: node.name.clone(),
                    };
                    match solver.solve(&obligation) {
                        SolverOutcome::Proven => VerificationState::Proven,
                        SolverOutcome::Assumed(_) | SolverOutcome::Unsupported => {
                            VerificationState::Assumed
                        }
                    }
                }
                ail_core::semantic_graph::RefinementStatus::Failed => VerificationState::Failed,
            }
        };
        entries.push(stage_entry(
            "10-check-refinements",
            state,
            node.name.clone(),
            Some(format!(
                "{} -> {}",
                refinement.base_type, refinement.predicate
            )),
        ));
    }
    if entries.is_empty() {
        entries.push(stage_entry(
            "10-check-refinements",
            VerificationState::Proven,
            "refinements",
            Some("no refinement refs present".into()),
        ));
    }
    entries
}

// ── Stage 12: Check invariants via impact analysis ────────────────────────

pub(super) fn check_invariants(
    base_graph: Option<&SemanticGraph>,
    target_graph: &SemanticGraph,
) -> Vec<VerificationEntry> {
    use ail_core::semantic_graph::{EdgeKind, NodeKind, NodeRef};

    let invariant_nodes: Vec<(NodeRef, String)> = target_graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Invariant)
        .map(|node| (node.id, node.name.clone()))
        .collect();

    if invariant_nodes.is_empty() {
        return vec![stage_entry(
            "12-check-invariants-via-impact-analysis",
            VerificationState::Proven,
            "invariants",
            Some("no invariant nodes present".into()),
        )];
    }

    // No base graph → all invariants unverified (can't determine what changed)
    let Some(base) = base_graph else {
        return invariant_nodes
            .into_iter()
            .map(|(_, name)| {
                stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Unverified,
                    name,
                    Some("no base graph snapshot provided; cannot assess impact".into()),
                )
            })
            .collect();
    };

    // Compute changed node IDs: new nodes OR nodes with changed type_facts/effect_row
    let base_by_name: std::collections::HashMap<&str, &ail_core::semantic_graph::GraphNode> =
        base.nodes.iter().map(|n| (n.name.as_str(), n)).collect();
    let changed_ids: BTreeSet<NodeRef> = target_graph
        .nodes
        .iter()
        .filter(|tn| {
            match base_by_name.get(tn.name.as_str()) {
                None => true, // new node
                Some(bn) => bn.type_facts != tn.type_facts || bn.effect_row != tn.effect_row,
            }
        })
        .map(|n| n.id)
        .collect();

    // For each invariant, BFS across all edges (bidirectional) to find reachable nodes
    invariant_nodes
        .into_iter()
        .map(|(inv_id, name)| {
            if changed_ids.is_empty() {
                return stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Proven,
                    name,
                    None,
                );
            }

            // BFS from invariant node
            let mut reachable: BTreeSet<NodeRef> = BTreeSet::new();
            let mut queue: VecDeque<NodeRef> = VecDeque::new();
            reachable.insert(inv_id);
            queue.push_back(inv_id);
            while let Some(cur) = queue.pop_front() {
                for edge in &target_graph.edges {
                    if edge.source == cur && !reachable.contains(&edge.target) {
                        reachable.insert(edge.target);
                        queue.push_back(edge.target);
                    }
                    if edge.target == cur && !reachable.contains(&edge.source) {
                        reachable.insert(edge.source);
                        queue.push_back(edge.source);
                    }
                }
            }

            // Find reachable changed nodes (excluding the invariant itself)
            let reachable_changed: Vec<NodeRef> = changed_ids
                .iter()
                .filter(|&&id| id != inv_id && reachable.contains(&id))
                .copied()
                .collect();

            if reachable_changed.is_empty() {
                return stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Proven,
                    name,
                    None,
                );
            }

            // Transitive BreaksIfChanged coverage: BFS following BreaksIfChanged edges
            // backward toward inv_id to find all transitively covered nodes (ITC-1).
            let mut covered: BTreeSet<NodeRef> = BTreeSet::new();
            let mut bfc_queue: VecDeque<NodeRef> = VecDeque::from([inv_id]);
            let mut bfc_visited: BTreeSet<NodeRef> = BTreeSet::from([inv_id]);
            while let Some(cur) = bfc_queue.pop_front() {
                for edge in &target_graph.edges {
                    if edge.kind == EdgeKind::BreaksIfChanged
                        && edge.target == cur
                        && !bfc_visited.contains(&edge.source)
                    {
                        bfc_visited.insert(edge.source);
                        covered.insert(edge.source);
                        bfc_queue.push_back(edge.source);
                    }
                }
            }

            let uncovered: Vec<&str> = reachable_changed
                .iter()
                .filter(|id| !covered.contains(id))
                .filter_map(|id| target_graph.nodes.iter().find(|n| n.id == *id))
                .map(|n| n.name.as_str())
                .collect();

            if uncovered.is_empty() {
                stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Proven,
                    name,
                    None,
                )
            } else {
                stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Unverified,
                    name,
                    Some(format!(
                        "invariant impacted by changes in: {}",
                        uncovered.join(", ")
                    )),
                )
            }
        })
        .collect()
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{
        EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };

    use crate::report::VerificationState;

    use super::check_invariants;

    fn empty_graph() -> SemanticGraph {
        SemanticGraph {
            nodes: vec![],
            edges: vec![],
        }
    }

    fn make_invariant_graph_two_hop() -> (SemanticGraph, SemanticGraph) {
        // base_graph: empty (no nodes → all target nodes are "new" / changed)
        let base = empty_graph();
        // target_graph: inv A (id=0), B (id=1), C (id=2)
        // Edges: C --BIC--> B --BIC--> A
        let inv_a = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.A");
        let node_b = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.B");
        let node_c = GraphNode::new(NodeRef(2), NodeKind::Function, "fn.C");
        let target = SemanticGraph {
            nodes: vec![inv_a, node_b, node_c],
            edges: vec![
                GraphEdge::new(NodeRef(2), NodeRef(1), EdgeKind::BreaksIfChanged), // C → B
                GraphEdge::new(NodeRef(1), NodeRef(0), EdgeKind::BreaksIfChanged), // B → A
            ],
        };
        (base, target)
    }

    // ── T-13 / T-14: Invariant BFS transitive coverage ───────────────────

    #[test]
    fn invariant_two_hop_breaks_if_changed_is_covered() {
        // C --BIC--> B --BIC--> inv A; C changed → Proven (transitive)
        let (base, target) = make_invariant_graph_two_hop();
        let entries = check_invariants(Some(&base), &target);
        let inv_entry = entries.iter().find(|e| e.scope == "inv.A").unwrap();
        assert_eq!(
            inv_entry.state,
            VerificationState::Proven,
            "two-hop BIC chain: C must be transitively covered"
        );
    }

    #[test]
    fn invariant_direct_breaks_if_changed_still_covered() {
        // Only direct edge: D --BIC--> inv A; D changed → Proven
        let base = empty_graph();
        let inv_a = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.A");
        let node_d = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.D");
        let target = SemanticGraph {
            nodes: vec![inv_a, node_d],
            edges: vec![GraphEdge::new(
                NodeRef(1),
                NodeRef(0),
                EdgeKind::BreaksIfChanged,
            )],
        };
        let entries = check_invariants(Some(&base), &target);
        let inv_entry = entries.iter().find(|e| e.scope == "inv.A").unwrap();
        assert_eq!(inv_entry.state, VerificationState::Proven);
    }

    #[test]
    fn invariant_uncovered_changed_node_is_unverified() {
        // E is reachable from inv A (via DependsOn) but has NO BIC edge → Unverified
        let base = empty_graph();
        let inv_a = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.A");
        let node_e = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.E");
        let target = SemanticGraph {
            nodes: vec![inv_a, node_e],
            // E is reachable via DependsOn but NOT covered by BreaksIfChanged
            edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn)],
        };
        let entries = check_invariants(Some(&base), &target);
        let inv_entry = entries.iter().find(|e| e.scope == "inv.A").unwrap();
        assert_eq!(
            inv_entry.state,
            VerificationState::Unverified,
            "reachable but uncovered changed node must be Unverified"
        );
    }

    #[test]
    fn invariant_three_hop_chain_covered() {
        // D --BIC--> C --BIC--> B --BIC--> inv A; D changed → Proven
        let base = empty_graph();
        let inv_a = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.A");
        let node_b = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.B");
        let node_c = GraphNode::new(NodeRef(2), NodeKind::Function, "fn.C");
        let node_d = GraphNode::new(NodeRef(3), NodeKind::Function, "fn.D");
        let target = SemanticGraph {
            nodes: vec![inv_a, node_b, node_c, node_d],
            edges: vec![
                GraphEdge::new(NodeRef(3), NodeRef(2), EdgeKind::BreaksIfChanged), // D → C
                GraphEdge::new(NodeRef(2), NodeRef(1), EdgeKind::BreaksIfChanged), // C → B
                GraphEdge::new(NodeRef(1), NodeRef(0), EdgeKind::BreaksIfChanged), // B → A
            ],
        };
        let entries = check_invariants(Some(&base), &target);
        let inv_entry = entries.iter().find(|e| e.scope == "inv.A").unwrap();
        assert_eq!(
            inv_entry.state,
            VerificationState::Proven,
            "three-hop BIC chain: D must be transitively covered"
        );
    }
}
