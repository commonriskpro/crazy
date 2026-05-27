use serde::{Deserialize, Serialize};

// ── RedactionState ────────────────────────────────────────────────────────

/// Explicit redaction state for a context response.
///
/// Mirrors the `redaction none | partial | restricted` field in the
/// context-server protocol doc.  This replaces the old `redacted: bool` flag
/// and carries richer semantic intent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedactionState {
    #[default]
    /// No nodes were withheld; full structured content is present.
    None,
    /// One or more nodes were withheld; redacted fields are marked.
    Partial,
    /// The entire response is restricted; access requires approval.
    Restricted,
}

// ── RedactionPolicy ───────────────────────────────────────────────────────

/// Caller-supplied redaction policy that describes which node categories
/// are withheld and why.
///
/// The `RedactionPolicy` accompanies the `redacted_refs` set passed to
/// `ResponseBuilder`: it documents intent rather than driving the filter
/// (the `BTreeSet<NodeRef>` is still the operative list).  Responses include
/// this policy in their audit trail so consumers understand what was omitted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionPolicy {
    /// Human-readable label for this policy (e.g., `"PII"`, `"restricted"`).
    pub label: String,
    /// Categories withheld (e.g., `["secrets", "PII", "audit_logs"]`).
    pub categories: Vec<String>,
    /// Whether a session capability is required to lift this policy.
    pub requires_approval: bool,
}
