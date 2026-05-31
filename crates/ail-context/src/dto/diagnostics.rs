use ail_core::semantic_graph::NodeRef;
use serde::{Deserialize, Serialize};

/// Duplicate `NodeRef` values were present in the candidate bundle.
pub const ISSUE_DUPLICATE_NODE_REF: &str = "CTX_BUNDLE_DUPLICATE_NODE_REF";
/// An edge referenced a node that is absent from the graph node table.
pub const ISSUE_MISSING_NODE_REF: &str = "CTX_BUNDLE_MISSING_NODE_REF";
/// Input order was not canonical and had to be normalized for stable output.
pub const ISSUE_UNSTABLE_INPUT_ORDER: &str = "CTX_BUNDLE_UNSTABLE_INPUT_ORDER";

/// Redacted entry descriptor that is safe to expose in diagnostics.
///
/// It intentionally carries only structural identity (`NodeRef`) and input
/// position.  Names, stable IDs, bodies, and provenance are omitted so the
/// descriptor cannot reveal the redacted content it points at.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RedactedDescriptor {
    /// Redacted node identity in the source graph.
    pub node_ref: NodeRef,
    /// Original candidate ordinal before redaction, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<usize>,
}

/// Deterministic descriptor for a bundle/slice diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BundleDescriptor {
    /// Node associated with this issue, when the issue is node-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,
    /// Edge source for missing-edge-endpoint diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_source: Option<NodeRef>,
    /// Edge target for missing-edge-endpoint diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_target: Option<NodeRef>,
    /// Stable edge kind label for missing-edge-endpoint diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_kind: Option<String>,
    /// Input ordinal, when useful for finding the unstable entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<usize>,
}

/// Diagnosable issue attached to a context bundle/slice response.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BundleIssue {
    /// Stable machine-readable issue code.
    pub code: String,
    /// Deterministic structural descriptor for locating the issue.
    pub descriptor: BundleDescriptor,
}
