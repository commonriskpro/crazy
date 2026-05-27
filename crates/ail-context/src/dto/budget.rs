use serde::{Deserialize, Serialize};

// ── QueryBudget ───────────────────────────────────────────────────────────

/// Fine-grained budget and scoping dimensions for a context query.
///
/// Replaces the single `budget: usize` byte limit with the full set of
/// dimensions specified in the context-server protocol doc:
///
/// ```txt
/// Budget fields:
///   max_depth            — limit BFS traversal depth
///   max_nodes            — limit total nodes returned
///   max_tokens           — byte limit for the structured layer (primary budget)
///   include_private      — whether to include private nodes
///   include_transitive   — whether to include transitive relationships
///   include_runtime_logs — whether to include runtime log data
///   profile              — runtime profile for capability/handler queries
/// ```
///
/// # Constructors
///
/// - `QueryBudget::bytes(n)` — set `max_tokens = n`, leave other fields as defaults.
/// - `QueryBudget::default()` — `max_tokens = usize::MAX` (unlimited byte budget).
///
/// # Effective byte limit
///
/// `QueryBudget::effective_bytes()` returns `max_tokens` — the value used
/// by `ResponseBuilder` as the byte limit for the structured layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryBudget {
    /// Maximum BFS traversal depth from the query target.
    ///
    /// `None` means unlimited depth. When set, BFS stops at this hop count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<usize>,
    /// Maximum number of nodes to include in the structured layer.
    ///
    /// `None` means unlimited nodes (bounded only by `max_tokens`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_nodes: Option<usize>,
    /// Maximum total bytes for the structured layer.
    ///
    /// `0` is invalid and will be rejected with `E_INVALID_BUDGET`.
    /// This is the primary budget dimension and replaces the former
    /// `budget: usize` field.
    pub max_tokens: usize,
    /// When `true`, private nodes are included in the response.
    ///
    /// Private nodes are those with `Visibility::Private` or no visibility
    /// annotation.  Default is `false` (private nodes are omitted).
    #[serde(default)]
    pub include_private: bool,
    /// When `true`, transitive relationships are followed during BFS.
    ///
    /// For `Callers`/`Callees` queries this overrides the per-variant
    /// `transitive` flag.  Default is `true`.
    #[serde(default = "default_true")]
    pub include_transitive: bool,
    /// When `true`, runtime log data is included in the response.
    ///
    /// Runtime logs can be large; default is `false` (logs omitted).
    #[serde(default)]
    pub include_runtime_logs: bool,
    /// Runtime profile identifier for capability/handler/runtime queries.
    ///
    /// `None` means all profiles or the default profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for QueryBudget {
    fn default() -> Self {
        Self {
            max_depth: None,
            max_nodes: None,
            max_tokens: usize::MAX,
            include_private: false,
            include_transitive: true,
            include_runtime_logs: false,
            profile: None,
        }
    }
}

impl QueryBudget {
    /// Create a `QueryBudget` with `max_tokens = bytes` and all other fields
    /// at their defaults.
    ///
    /// This is the recommended constructor for callers that only care about
    /// the byte limit (equivalent to the former `budget: usize` usage).
    pub fn bytes(bytes: usize) -> Self {
        Self {
            max_tokens: bytes,
            ..Self::default()
        }
    }

    /// The effective byte limit used by `ResponseBuilder`.
    ///
    /// Returns `max_tokens` — the primary budget dimension.
    pub fn effective_bytes(&self) -> usize {
        self.max_tokens
    }
}
