// ── ail-verify::effect_checker ────────────────────────────────────────────
//
// Effect checking pass for the verification pipeline (step 8).
//
// # Model
//
// The effect checker compares declared effects (from `node.effect_row`) against
// effects inferred from the semantic graph (from `GraphEdge { kind: Emits, .. }`).
//
// For each node in the graph, the checker gathers:
//   - `declared`: `node.effect_row.effects` (empty vec if None)
//   - `inferred`: names of nodes that `node` emits to via `EdgeKind::Emits`
//
// Rules (in priority order):
//   1. Node has inferred effects that are NOT covered by declared:
//      → `Failed` with evidence `E_EFFECT_UNDECLARED: <effect_name>`
//   2. Node has declared effects but NO inferred effects (unused declaration):
//      → `Assumed` with evidence `E_EFFECT_UNUSED: declared but not used`
//   3. Node has BOTH declared AND inferred effects, declared covers all inferred:
//      → `Proven`
//   4. Node has NEITHER declared NOR inferred effects:
//      → `Proven` (pure node; nothing to check)
//
// One `VerificationEntry` is emitted per node.
//
// # Exclusions
//
// - No runtime execution.
// - No SMT/Z3 calls.
// - No I/O or mutation.

use std::collections::{HashMap, HashSet};

use ail_core::semantic_graph::{EdgeKind, NodeRef, SemanticGraph};

use crate::report::{VerificationEntry, VerificationReport, VerificationState};

// ── EffectChecker ─────────────────────────────────────────────────────────

/// Pure, stateless effect checker.
///
/// Compares declared effects (`EffectRow`) against inferred effects (`Emits`
/// edges) for every node in the graph, emitting one `VerificationEntry` per
/// node.
pub struct EffectChecker;

impl EffectChecker {
    /// Walk `graph` and compare declared vs inferred effects per node.
    ///
    /// # Determinism
    ///
    /// Output entries are in graph-node-insertion order.  Two calls with
    /// identical input produce identical output.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        // Build inferred-effects map: NodeRef → sorted Vec of target names.
        // We use the *target node's name* as the effect name when following Emits edges.
        let name_of: HashMap<NodeRef, &str> = graph
            .nodes
            .iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();

        let mut inferred: HashMap<NodeRef, HashSet<String>> = HashMap::new();
        for edge in &graph.edges {
            if edge.kind == EdgeKind::Emits
                && let Some(&target_name) = name_of.get(&edge.target)
            {
                inferred
                    .entry(edge.source)
                    .or_default()
                    .insert(target_name.to_string());
            }
        }

        let entries: Vec<VerificationEntry> = graph
            .nodes
            .iter()
            .map(|node| {
                let scope = node.name.clone();
                let declared: HashSet<String> = node
                    .effect_row
                    .as_ref()
                    .map(|er| er.effects.iter().cloned().collect())
                    .unwrap_or_default();
                let inf = inferred.get(&node.id).cloned().unwrap_or_default();

                // Rule 1: undeclared inferred effects → Failed
                let undeclared: Vec<&String> =
                    inf.iter().filter(|e| !declared.contains(*e)).collect();
                if !undeclared.is_empty() {
                    let mut names: Vec<String> =
                        undeclared.into_iter().map(|s| s.to_string()).collect();
                    names.sort();
                    return VerificationEntry {
                        claim: "effect-check".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some(format!("E_EFFECT_UNDECLARED: {}", names.join(", "))),
                        blocking: true,
                        repair_options: vec![
                            "add the missing effect to the node's effect_row declaration".into(),
                            "remove the Emits edge if the effect emission is unintended".into(),
                        ],
                    };
                }

                // Rule 2: declared but no inferred → Assumed (unused)
                if !declared.is_empty() && inf.is_empty() {
                    let mut names: Vec<String> = declared.into_iter().collect();
                    names.sort();
                    return VerificationEntry {
                        claim: "effect-check".into(),
                        state: VerificationState::Assumed,
                        scope,
                        evidence: Some(format!("E_EFFECT_UNUSED: {}", names.join(", "))),
                        blocking: false,
                        repair_options: vec![
                            "remove the unused effect declaration from effect_row".into(),
                            "add an Emits edge to connect the declared effect to its target node"
                                .into(),
                        ],
                    };
                }

                // Rule 3: declared covers all inferred → Proven
                // Rule 4: neither declared nor inferred → Proven
                VerificationEntry {
                    claim: "effect-check".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                }
            })
            .collect();

        let summary_counts = crate::report::SummaryCounts {
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
        };
        VerificationReport {
            entries,
            schema_version: "verification/1.0".into(),
            summary_counts,
            ..Default::default()
        }
    }
}
