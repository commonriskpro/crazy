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

use crate::dto::{RedactionPolicy, RedactionState};

// ── filter_redacted ───────────────────────────────────────────────────────

/// Remove redacted nodes from `candidates`.
///
/// Returns `(unredacted_nodes, was_any_redacted)`.  The boolean is `true` if
/// at least one node was removed.
pub(crate) fn filter_redacted(
    candidates: Vec<GraphNode>,
    redacted_refs: &BTreeSet<NodeRef>,
) -> (Vec<GraphNode>, bool) {
    let mut redacted = false;
    let unredacted = candidates
        .into_iter()
        .filter(|n| {
            if redacted_refs.contains(&n.id) {
                redacted = true;
                false
            } else {
                true
            }
        })
        .collect();
    (unredacted, redacted)
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
