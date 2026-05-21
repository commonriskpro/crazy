// ── ail-verify::type_checker ──────────────────────────────────────────────
//
// Type-checking pass for the verification pipeline (step 7).
//
// # Scope
//
// `TypeChecker::check` walks a `&SemanticGraph` and validates type facts for
// every `Function` and `Type` node.  Rules:
//
// - `TypeFacts` present with non-empty `nominal`:
//   - Every element of `generics` must be a non-empty string.
//     Empty string → `Failed` with evidence `E_GENERIC_ARITY`.
//   - Otherwise → `Proven`.
// - `TypeFacts` absent or `nominal` empty → `Unverified`.
// - All other node kinds (Module, Effect, etc.) are skipped.
//
// One `VerificationEntry` is emitted per checked node with `claim: "type-check"`.
//
// # Exclusions
//
// - No SMT/Z3 calls.
// - No runtime execution.
// - No I/O or mutation.

use ail_core::semantic_graph::{NodeKind, SemanticGraph};

use crate::report::{VerificationEntry, VerificationReport, VerificationState};

// ── TypeChecker ───────────────────────────────────────────────────────────

/// Pure, stateless type checker.
///
/// Validates `TypeFacts` on `Function` and `Type` nodes, emitting one
/// `VerificationEntry` per checked node.
pub struct TypeChecker;

impl TypeChecker {
    /// Walk `graph` and validate type facts for `Function`/`Type` nodes.
    ///
    /// # Determinism
    ///
    /// Output entries are in graph-traversal order.  Two calls with identical
    /// input produce identical output.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        let mut entries = Vec::new();
        for node in &graph.nodes {
            if !matches!(node.kind, NodeKind::Function | NodeKind::Type) {
                continue;
            }
            entries.push(Self::classify_node_type(node));
        }
        VerificationReport::new(entries)
    }

    fn classify_node_type(node: &ail_core::semantic_graph::GraphNode) -> VerificationEntry {
        let scope = node.name.clone();

        match &node.type_facts {
            None => VerificationEntry {
                claim: "type-check".into(),
                state: VerificationState::Unverified,
                scope,
                evidence: None,
            },
            Some(tf) if tf.nominal.is_empty() => VerificationEntry {
                claim: "type-check".into(),
                state: VerificationState::Unverified,
                scope,
                evidence: None,
            },
            Some(tf) => {
                // Validate every generic parameter is a non-empty string.
                let bad_generic = tf.generics.iter().any(|g| g.is_empty());
                if bad_generic {
                    VerificationEntry {
                        claim: "type-check".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some("E_GENERIC_ARITY: generic parameter name is empty".into()),
                    }
                } else {
                    VerificationEntry {
                        claim: "type-check".into(),
                        state: VerificationState::Proven,
                        scope,
                        evidence: None,
                    }
                }
            }
        }
    }
}
