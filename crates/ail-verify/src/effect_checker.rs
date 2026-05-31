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
//      → `Failed` with evidence `E_EFFECT_UNDECLARED`
//   2. Node has declared effects not present in inferred effects:
//      → `Assumed` with evidence `E_EFFECT_UNUSED`
//   3. Node has BOTH declared AND inferred effects with exact set equality:
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

use std::collections::{BTreeSet, HashMap};

use ail_core::semantic_graph::{EdgeKind, NodeRef, SemanticGraph};

use crate::diagnostic::{E_EFFECT_UNDECLARED, E_EFFECT_UNUSED};
use crate::report::{VerificationEntry, VerificationReport, VerificationState};

/// Stable category for an effect inferred by `Emits` but missing from `effect_row`.
pub const EFFECT_DIAGNOSTIC_CATEGORY_MISSING_EFFECT: &str = "effect.missing";

/// Stable category for an effect declared in `effect_row` but absent from inferred effects.
pub const EFFECT_DIAGNOSTIC_CATEGORY_EXTRA_EFFECT: &str = "effect.extra";

// ── EffectChecker ─────────────────────────────────────────────────────────

/// Pure, stateless effect checker.
///
/// Compares declared effects (`EffectRow`) against inferred effects (`Emits`
/// edges) for every node in the graph, emitting one `VerificationEntry` per
/// stable issue.
pub struct EffectChecker;

impl EffectChecker {
    /// Walk `graph` and compare declared vs inferred effects per node.
    ///
    /// # Determinism
    ///
    /// Output entries are sorted by stable machine fields and exact duplicate
    /// entries are collapsed. Two calls with identical input produce identical
    /// output.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        // Build inferred-effects map: NodeRef → sorted, deduped target names.
        // We use the *target node's name* as the effect name when following Emits edges.
        let name_of: HashMap<NodeRef, &str> = graph
            .nodes
            .iter()
            .map(|n| (n.id, n.name.as_str()))
            .collect();

        let mut inferred: HashMap<NodeRef, BTreeSet<String>> = HashMap::new();
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

        let mut entries: Vec<VerificationEntry> = graph
            .nodes
            .iter()
            .map(|node| {
                let scope = node.name.clone();
                let declared: BTreeSet<String> = node
                    .effect_row
                    .as_ref()
                    .map(|er| er.effects.iter().cloned().collect())
                    .unwrap_or_default();
                let inf = inferred.get(&node.id).cloned().unwrap_or_default();

                // Rule 1: undeclared inferred effects → Failed
                let undeclared: Vec<String> = inf.difference(&declared).cloned().collect();
                if !undeclared.is_empty() {
                    return VerificationEntry {
                        claim: "effect-check".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some(effect_issue_evidence(
                            E_EFFECT_UNDECLARED,
                            EFFECT_DIAGNOSTIC_CATEGORY_MISSING_EFFECT,
                            "inferred effect(s) missing from effect_row",
                            "effect",
                            &undeclared,
                        )),
                        blocking: true,
                        repair_options: vec![
                            "add the missing effect to the node's effect_row declaration".into(),
                            "remove the Emits edge if the effect emission is unintended".into(),
                        ],
                    };
                }

                // Rule 2: declared but not inferred → Assumed (extra/unused)
                let extra: Vec<String> = declared.difference(&inf).cloned().collect();
                if !extra.is_empty() {
                    return VerificationEntry {
                        claim: "effect-check".into(),
                        state: VerificationState::Assumed,
                        scope,
                        evidence: Some(effect_issue_evidence(
                            E_EFFECT_UNUSED,
                            EFFECT_DIAGNOSTIC_CATEGORY_EXTRA_EFFECT,
                            "declared effect(s) absent from inferred Emits edges",
                            "effect",
                            &extra,
                        )),
                        blocking: false,
                        repair_options: vec![
                            "remove the unused effect declaration from effect_row".into(),
                            "add an Emits edge to connect the declared effect to its target node"
                                .into(),
                        ],
                    };
                }

                // Rule 3: declared and inferred effect sets match → Proven
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

        normalize_effect_entries(&mut entries);

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

fn effect_issue_evidence(
    code: &str,
    category: &str,
    reason: &str,
    descriptor_kind: &str,
    raw_descriptors: &[String],
) -> String {
    let descriptors = redacted_descriptors(descriptor_kind, raw_descriptors.len());
    format!(
        "{code}: category={category}; reason={reason}; count={}; descriptors=[{}]",
        raw_descriptors.len(),
        descriptors.join(", ")
    )
}

fn redacted_descriptors(kind: &str, count: usize) -> Vec<String> {
    (0..count).map(|index| format!("{kind}#{index}")).collect()
}

fn normalize_effect_entries(entries: &mut Vec<VerificationEntry>) {
    entries.sort_by(|a, b| {
        a.claim
            .cmp(&b.claim)
            .then_with(|| a.scope.cmp(&b.scope))
            .then_with(|| verification_state_rank(a.state).cmp(&verification_state_rank(b.state)))
            .then_with(|| a.evidence.cmp(&b.evidence))
            .then_with(|| a.repair_options.cmp(&b.repair_options))
    });
    entries.dedup();
}

fn verification_state_rank(state: VerificationState) -> u8 {
    match state {
        VerificationState::Proven => 0,
        VerificationState::RuntimeChecked => 1,
        VerificationState::Assumed => 2,
        VerificationState::Unverified => 3,
        VerificationState::Unsafe => 4,
        VerificationState::Failed => 5,
    }
}
