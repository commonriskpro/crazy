// ── ail-verify::checker ───────────────────────────────────────────────────
//
// Pure graph walker that classifies each node's type/effect/capability claim
// into `VerificationEntry` items.
//
// # Scope (Phase 5 shallow semantics)
//
// - `TypeFacts.nominal` non-empty  → `Proven`  (directly declared)
// - `TypeFacts` absent or empty    → `Unverified`
// - `EffectRow` present            → `Assumed`  (declared, not mechanically proven)
// - `EffectRow` absent             → `Unverified`
// - `CapabilityReqs` present       → `Assumed`  (declared, not mechanically proven)
// - `CapabilityReqs` absent        → `Unverified`
//
// # Exclusions (NOT in Phase 5)
//
// - No Z3/SMT solver calls
// - No runtime checks
// - No contract/refinement evaluation
// - No interface-coherence checking
// - No I/O or state mutation of any kind

use ail_core::semantic_graph::{GraphNode, SemanticGraph};

use crate::diagnostic::{Diagnostic, E_TYPE_MISMATCH};
use crate::report::{VerificationEntry, VerificationReport, VerificationState};

/// Pure, stateless graph checker.
///
/// `Checker::check` traverses a `&SemanticGraph` in node-insertion order
/// and emits three `VerificationEntry` items per node (type, effect,
/// capability), producing a deterministic `VerificationReport`.
pub struct Checker;

impl Checker {
    /// Walk `graph` and classify every node's type/effect/capability facts.
    ///
    /// # Determinism guarantee
    ///
    /// The returned `VerificationReport` has exactly `graph.nodes.len() * 3`
    /// entries (one per fact dimension per node), in `(node order, fact order)`
    /// sequence.  Two calls with identical input graphs produce byte-identical
    /// reports.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        let mut entries = Vec::with_capacity(graph.nodes.len() * 3);
        let mut diagnostics = Vec::new();
        for node in &graph.nodes {
            Self::classify_node(node, &mut entries, &mut diagnostics);
        }
        VerificationReport {
            entries,
            diagnostics,
            ..Default::default()
        }
    }

    /// Produce the three canonical fact entries for one `GraphNode` and
    /// append any structured `Diagnostic` items for violated conditions.
    ///
    /// Entries are appended in fixed order: type → effect → capability.
    ///
    /// Diagnostics emitted:
    /// - `E_TYPE_MISMATCH` (blocking Error) when the type entry is `Unverified`
    ///   because no type facts were declared.
    fn classify_node(
        node: &GraphNode,
        entries: &mut Vec<VerificationEntry>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let scope = node.name.clone();

        // ── Type fact ─────────────────────────────────────────────────────
        let type_state = match &node.type_facts {
            Some(tf) if !tf.nominal.is_empty() => VerificationState::Proven,
            _ => VerificationState::Unverified,
        };
        entries.push(VerificationEntry {
            claim: "type".into(),
            state: type_state,
            scope: scope.clone(),
            evidence: None,
            blocking: false,
            repair_options: vec![],
        });

        // Emit E_TYPE_MISMATCH when the type is unverified (no facts declared).
        if type_state == VerificationState::Unverified {
            diagnostics.push(
                Diagnostic::error(E_TYPE_MISMATCH, node.id)
                    .with_evidence(format!("node '{scope}' has no declared type facts")),
            );
        }

        // ── Effect fact ───────────────────────────────────────────────────
        let effect_state = if node.effect_row.is_some() {
            VerificationState::Assumed
        } else {
            VerificationState::Unverified
        };
        entries.push(VerificationEntry {
            claim: "effect".into(),
            state: effect_state,
            scope: scope.clone(),
            evidence: None,
            blocking: false,
            repair_options: vec![],
        });

        // ── Capability fact ───────────────────────────────────────────────
        let cap_state = if node.capability_reqs.is_some() {
            VerificationState::Assumed
        } else {
            VerificationState::Unverified
        };
        entries.push(VerificationEntry {
            claim: "capability".into(),
            state: cap_state,
            scope,
            evidence: None,
            blocking: false,
            repair_options: vec![],
        });
    }
}
