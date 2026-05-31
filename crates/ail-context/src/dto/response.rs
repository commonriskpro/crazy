use ail_core::semantic_graph::{GraphNode, NodeRef};
use ail_storage::graph::SnapshotEnvelope;
use ail_storage::object::ObjectId;
use serde::{Deserialize, Serialize};

use super::{BundleIssue, ProvenanceBlock, RedactedDescriptor, RedactionPolicy, RedactionState};

// ── ImpactInfo ────────────────────────────────────────────────────────────

/// Structured impact classification attached to `Impact` query responses.
///
/// Provides a breakdown of the affected-node set into semantic categories
/// (tests, capabilities, public APIs) and a computed `risk_level` string so
/// consumers can make risk-aware decisions without re-classifying every node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ImpactInfo {
    /// `NodeRef`s of nodes with `NodeKind::Test` in the affected set.
    pub affected_tests: Vec<NodeRef>,
    /// `NodeRef`s of nodes with `NodeKind::Capability` in the affected set.
    pub affected_capabilities: Vec<NodeRef>,
    /// `NodeRef`s of nodes with `Visibility::Public` in the affected set.
    pub affected_public_apis: Vec<NodeRef>,
    /// Count of Contract/Invariant nodes that need re-verification.
    pub required_reverification: usize,
    /// Overall risk classification: `"none"`, `"low"`, `"medium"`, or `"high"`.
    ///
    /// Derived from `affected_public_apis.len() + required_reverification`:
    /// 0 → `"none"`, 1–3 → `"low"`, 4–10 → `"medium"`, >10 → `"high"`.
    pub risk_level: String,
}

// ── RefactorInfo ──────────────────────────────────────────────────────────

/// Structured refactoring support information for `RefactorContext`,
/// `ExtractCandidates`, and `MoveSafety` query responses.
///
/// Provides a pre-classified breakdown of nodes that must be considered when
/// performing a safe refactoring — what's locked, what must be preserved, and
/// what must be updated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RefactorInfo {
    /// `NodeRef`s of nodes with `WorkflowState::Locked` in the context set.
    ///
    /// These represent behavior locks that constrain the refactoring.
    pub behavior_locks_needed: Vec<NodeRef>,
    /// `NodeRef`s of Contract/Invariant nodes that must be preserved.
    pub contracts_to_preserve: Vec<NodeRef>,
    /// `NodeRef`s of Effect/EffectAlias nodes that must be preserved.
    pub effects_to_preserve: Vec<NodeRef>,
    /// `NodeRef`s of nodes that call the refactoring target (must be updated).
    pub callers_to_update: Vec<NodeRef>,
    /// `NodeRef`s of proof nodes (via `Proves` edges) that need re-running.
    pub proofs_to_rerun: Vec<NodeRef>,
    /// `NodeRef`s of nodes that may conflict with this refactoring.
    ///
    /// Nodes with `BreaksIfChanged` edges in the context set.
    pub possible_conflicts: Vec<NodeRef>,
    /// Human-readable suggestions for performing the refactoring safely.
    pub suggested_refactor_ops: Vec<String>,
}

// ── RepairOption ──────────────────────────────────────────────────────────

/// A structured suggestion for recovering from a context error.
///
/// Mirrors the `repair_options` block in the context-server protocol doc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairOption {
    /// Short identifier for the repair option (e.g., `"query_latest"`, `"narrow_scope"`).
    pub option_id: String,
    /// Human-readable description of the repair action.
    pub description: String,
    /// Suggested follow-up query in text form (e.g., `"context fn.checkout snapshot=latest"`).
    pub suggested_query: Option<String>,
}

// ── FreshnessStatus ───────────────────────────────────────────────────────

/// The freshness state of a context response relative to the current graph.
///
/// Mirrors the `freshness fresh | stale | unknown` field in the protocol doc.
/// `Unknown` is returned when the server cannot determine whether the snapshot
/// is current (e.g., the index version is unavailable).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreshnessStatus {
    #[default]
    /// The snapshot is the current graph root — no invalidation has occurred.
    Fresh,
    /// A newer snapshot exists; this response may describe stale state.
    Stale,
    /// Freshness cannot be determined (e.g., index not available).
    Unknown,
}
// ── ResponseLimits ────────────────────────────────────────────────────────

/// Budget accounting block attached to every `ContextResponse`.
///
/// Mirrors the `limits` block described in the context-server protocol doc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseLimits {
    /// The byte budget that was in effect for this query.
    pub budget_bytes: usize,
    /// The total CBOR bytes consumed by the `structured` slice.
    pub bytes_used: usize,
    /// `true` when `bytes_used` reached `budget_bytes` before all candidate
    /// nodes were included.
    pub truncated: bool,
    /// Names of sections omitted due to budget exhaustion.
    ///
    /// Empty when `truncated` is `false`.  Example entries:
    /// `"transitive_callers"`, `"history_chain"`.
    pub omitted_sections: Vec<String>,
}

// ── ContextResponse ───────────────────────────────────────────────────────

/// The response envelope produced by resolving a `ContextQuery`.
///
/// `context_hash` is `blake3(CBOR(structured))` — byte-stable for identical
/// `structured` inputs regardless of other field values.
///
/// # Serialization
///
/// `ContextResponse` satisfies the determinism contract: `Vec`/`BTreeMap`
/// only, no `HashMap`, no floats.  `SnapshotEnvelope` has `PartialEq` only
/// (no `Eq`), so this struct follows suit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextResponse {
    /// Schema version tag (always `"context/1.0"` for this generation).
    pub schema: String,
    /// The snapshot from which this slice was built.
    pub snapshot: SnapshotEnvelope,
    /// Content-addressed graph root; equals `snapshot.graph_root_hash`.
    pub graph_root_hash: ObjectId,
    /// `blake3(CBOR(query_bytes))` where `query_bytes = CBOR(ContextQuery)`.
    ///
    /// Stable identifier for the query that produced this response.
    /// Can be used by ChangeSets to assert which query they are based on.
    pub query_hash: [u8; 32],
    /// `blake3(CBOR(structured))` — stable for identical structured layers.
    pub context_hash: [u8; 32],
    /// Nodes matching the query, ordered by `NodeRef`.
    pub structured: Vec<GraphNode>,
    /// Text rendered from `structured` only (post-redaction/truncation).
    pub summary: String,
    /// Unix milliseconds: equals `snapshot.created_at`.
    pub freshness: u64,
    /// Unix milliseconds when this response was generated.
    ///
    /// Mirrors the `generated_at` field in the protocol doc.
    /// Set to the current wall-clock time at response build time.
    pub generated_at: u64,
    /// `true` when at least one node was withheld by the redaction policy.
    ///
    /// Legacy boolean kept for backward compatibility.  Prefer `redaction_state`.
    pub redacted: bool,
    /// Safe structural descriptors for nodes withheld by redaction.
    ///
    /// Names, stable IDs, bodies, and provenance are intentionally omitted so
    /// diagnostics can explain omissions without leaking protected content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redacted_descriptors: Vec<RedactedDescriptor>,
    /// Deterministic bundle/slice diagnostics with stable issue codes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<BundleIssue>,
    /// Explicit redaction state for this response.
    ///
    /// Mirrors `redaction none | partial | restricted` in the protocol doc.
    #[serde(default, skip_serializing_if = "is_redaction_none")]
    pub redaction_state: RedactionState,
    /// The redaction policy applied to this response, if any.
    ///
    /// Populated when `redaction_state` is `Partial` or `Restricted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_policy: Option<RedactionPolicy>,
    /// `true` when the byte budget was exhausted before all matching nodes
    /// were included.
    ///
    /// Mirrors `limits.truncated`; kept for backward compatibility.
    pub truncated: bool,
    /// Budget accounting for this response.
    pub limits: ResponseLimits,
    /// Snapshot provenance chain for `History` and `Why` queries.
    ///
    /// Empty for all other query kinds.  Ordered oldest-first.
    pub history_entries: Vec<SnapshotEnvelope>,
    /// Freshness status relative to the current graph.
    ///
    /// `Fresh` when this response was built from the current snapshot.
    /// `Stale` when a newer snapshot exists.  `Unknown` when the server
    /// cannot determine currency (e.g., index unavailable).
    ///
    /// Serialized only when not `Fresh` (additive; pre-G27 decoders see
    /// `None` and may treat absence as fresh).
    #[serde(default, skip_serializing_if = "is_fresh")]
    pub freshness_status: FreshnessStatus,
    /// Provenance block listing sources, index versions, and report hashes.
    ///
    /// Mirrors the `provenance` block in the protocol doc.
    #[serde(default, skip_serializing_if = "is_provenance_empty")]
    pub provenance: ProvenanceBlock,
    /// Structured repair options when errors or staleness occurred.
    ///
    /// Empty for successful fresh responses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repair_options: Vec<RepairOption>,
    /// Impact classification for `Impact` queries.
    ///
    /// `None` for all other query kinds.  Populated by `ResponseBuilder`
    /// when building responses for `ContextQuery::Impact`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_info: Option<ImpactInfo>,
    /// Refactoring support information for `RefactorContext`, `ExtractCandidates`,
    /// and `MoveSafety` queries.
    ///
    /// `None` for all other query kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refactor_info: Option<RefactorInfo>,
}

fn is_fresh(s: &FreshnessStatus) -> bool {
    *s == FreshnessStatus::Fresh
}

fn is_redaction_none(s: &RedactionState) -> bool {
    *s == RedactionState::None
}

fn is_provenance_empty(p: &ProvenanceBlock) -> bool {
    p.sources.is_empty() && p.indexes.is_empty() && p.reports.is_empty()
}
