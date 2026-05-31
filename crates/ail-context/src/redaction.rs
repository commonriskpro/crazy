// ── ail-context::redaction ────────────────────────────────────────────────
//
// Redaction filtering and access-shaping helpers.
//
// Responsible for:
// - Removing redacted `NodeRef`s from candidate sets.
// - Computing the `RedactionState` for a response given a policy and whether
//   any nodes were withheld.

use std::collections::BTreeSet;

use ail_core::semantic_graph::{GraphNode, NodeRef};

use crate::dto::{RedactedDescriptor, RedactionPolicy, RedactionState};

// ── RedactionFilter ──────────────────────────────────────────────────────

/// Output of redaction filtering.
pub(crate) struct RedactionFilter {
    /// Candidate nodes that remain visible.
    pub(crate) unredacted: Vec<GraphNode>,
    /// `true` when at least one candidate was withheld.
    pub(crate) redacted: bool,
    /// Safe structural descriptors for withheld nodes.
    pub(crate) descriptors: Vec<RedactedDescriptor>,
}

// ── filter_redacted ───────────────────────────────────────────────────────

/// Remove redacted nodes from `candidates`.
///
/// Descriptors intentionally expose only `NodeRef` + candidate ordinal, never
/// names or bodies, so callers can diagnose omissions without leaking content.
pub(crate) fn filter_redacted(
    candidates: Vec<GraphNode>,
    redacted_refs: &BTreeSet<NodeRef>,
) -> RedactionFilter {
    let mut redacted = false;
    let mut unredacted = Vec::with_capacity(candidates.len());
    let mut descriptors = Vec::new();

    for (ordinal, node) in candidates.into_iter().enumerate() {
        if redacted_refs.contains(&node.id) {
            redacted = true;
            descriptors.push(RedactedDescriptor {
                node_ref: node.id,
                ordinal: Some(ordinal),
            });
        } else {
            unredacted.push(node);
        }
    }

    RedactionFilter {
        unredacted,
        redacted,
        descriptors,
    }
}

// ── compute_redaction_state ───────────────────────────────────────────────

/// Derive the `RedactionState` for a response.
///
/// Rules:
/// - `policy.requires_approval` → `Restricted` (regardless of whether nodes were withheld).
/// - `redacted == true`         → `Partial`.
/// - Otherwise                  → `None`.
pub(crate) fn compute_redaction_state(
    policy: Option<&RedactionPolicy>,
    redacted: bool,
) -> RedactionState {
    if let Some(p) = policy {
        if p.requires_approval {
            RedactionState::Restricted
        } else if redacted {
            RedactionState::Partial
        } else {
            RedactionState::None
        }
    } else if redacted {
        RedactionState::Partial
    } else {
        RedactionState::None
    }
}
