// ── ail-core::semantic_graph::validation ─────────────────────────────────
//
// `GraphValidationError`, `DanglingRole`, and the `SemanticGraph` validation
// methods (`validate` and `validate_full`).
//
// This module is private to `semantic_graph`; all public items are
// re-exported from `semantic_graph/mod.rs`.

use super::types::{EdgeKind, NodeKind, SemanticGraph};

// ── GraphValidationError ──────────────────────────────────────────────────

/// Errors produced by `SemanticGraph::validate` and `SemanticGraph::validate_full`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphValidationError {
    /// Two nodes share the same `NodeRef`.
    DuplicateRef(super::types::NodeRef),
    /// An edge endpoint references a `NodeRef` not present in the graph.
    DanglingEdge {
        /// The missing `NodeRef`.
        r#ref: super::types::NodeRef,
        /// Whether the missing ref was the edge source or target.
        role: DanglingRole,
    },
    /// A node declares a non-empty `effect_row` but has no outgoing `Emits` edges.
    ///
    /// Emitting effects requires graph edges — a declared effect row that is
    /// never wired to an `Emits` edge is an incoherent graph state.
    EffectRowNoEmitsEdge(super::types::NodeRef),
    /// A node's `capability_reqs` names a capability that has no matching
    /// `Capability`-kind node in this graph.
    CapabilityReqsMissingNode {
        /// The node that declared the unsatisfied requirement.
        owner_ref: super::types::NodeRef,
        /// The capability name that could not be matched to any `Capability` node.
        cap_name: String,
    },
}

/// Whether a dangling edge endpoint was the source or the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DanglingRole {
    Source,
    Target,
}

// ── SemanticGraph::validate ───────────────────────────────────────────────

impl SemanticGraph {
    /// Validate structural invariants:
    ///
    /// 1. All `NodeRef`s in `nodes` are unique.
    /// 2. Every edge endpoint corresponds to an existing node in this graph.
    ///
    /// Returns `Ok(())` when all invariants hold; otherwise returns the first
    /// `GraphValidationError` found.
    pub fn validate(&self) -> Result<(), GraphValidationError> {
        use super::types::NodeRef;
        use std::collections::BTreeSet;

        // Pass 1 — build the set of known refs, detecting duplicates.
        let mut seen: BTreeSet<NodeRef> = BTreeSet::new();
        for node in &self.nodes {
            if !seen.insert(node.id) {
                return Err(GraphValidationError::DuplicateRef(node.id));
            }
        }

        // Pass 2 — verify all edge endpoints are in the known set.
        for edge in &self.edges {
            if !seen.contains(&edge.source) {
                return Err(GraphValidationError::DanglingEdge {
                    r#ref: edge.source,
                    role: DanglingRole::Source,
                });
            }
            if !seen.contains(&edge.target) {
                return Err(GraphValidationError::DanglingEdge {
                    r#ref: edge.target,
                    role: DanglingRole::Target,
                });
            }
        }

        Ok(())
    }

    /// Full semantic validation — returns ALL errors found (not just the first).
    ///
    /// Performs the same structural checks as [`validate`] (duplicate refs,
    /// dangling edges) plus two additional semantic coherence checks:
    ///
    /// 3. **Effect-row coherence** — every node whose `effect_row` is `Some` and
    ///    non-empty must have at least one outgoing `Emits` edge.  A declared
    ///    effect row that is never connected to an `Emits` edge is incoherent.
    ///
    /// 4. **Capability-reqs consistency** — every capability name listed in a
    ///    node's `capability_reqs` must correspond to a `Capability`-kind node
    ///    present in this graph.  Requirements that reference non-existent
    ///    capability nodes indicate a malformed graph.
    ///
    /// Returns an empty `Vec` when all invariants hold; the caller can call
    /// `validate_full().is_empty()` to test overall validity.
    pub fn validate_full(&self) -> Vec<GraphValidationError> {
        use super::types::NodeRef;
        use std::collections::BTreeSet;

        let mut errors: Vec<GraphValidationError> = Vec::new();

        // Pass 1 — duplicate NodeRef detection.
        let mut seen: BTreeSet<NodeRef> = BTreeSet::new();
        for node in &self.nodes {
            if !seen.insert(node.id) {
                errors.push(GraphValidationError::DuplicateRef(node.id));
            }
        }

        // Pass 2 — dangling edge endpoints.
        for edge in &self.edges {
            if !seen.contains(&edge.source) {
                errors.push(GraphValidationError::DanglingEdge {
                    r#ref: edge.source,
                    role: DanglingRole::Source,
                });
            }
            if !seen.contains(&edge.target) {
                errors.push(GraphValidationError::DanglingEdge {
                    r#ref: edge.target,
                    role: DanglingRole::Target,
                });
            }
        }

        // Pass 3 — effect-row coherence.
        // A node with a non-empty effect_row must have at least one Emits edge.
        for node in &self.nodes {
            if node
                .effect_row
                .as_ref()
                .is_some_and(|r| !r.effects.is_empty())
            {
                let has_emits = self
                    .edges
                    .iter()
                    .any(|e| e.source == node.id && e.kind == EdgeKind::Emits);
                if !has_emits {
                    errors.push(GraphValidationError::EffectRowNoEmitsEdge(node.id));
                }
            }
        }

        // Pass 4 — capability-reqs consistency.
        // Build the set of Capability-kind node names available in this graph.
        let capability_names: BTreeSet<&str> = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Capability)
            .map(|n| n.name.as_str())
            .collect();

        for node in &self.nodes {
            if let Some(cap_reqs) = &node.capability_reqs {
                for cap_name in &cap_reqs.caps {
                    if !capability_names.contains(cap_name.as_str()) {
                        errors.push(GraphValidationError::CapabilityReqsMissingNode {
                            owner_ref: node.id,
                            cap_name: cap_name.clone(),
                        });
                    }
                }
            }
        }

        errors
    }
}
