/// `ail-context` — read-only semantic context slices from the AIL graph.
///
/// Turns an immutable `SnapshotEnvelope.graph_root_hash` into bounded,
/// hash-stable `ContextResponse` envelopes.  No mutations, no CLI wiring.
///
/// # Module overview
///
/// - [`error`]   — [`ContextError`](error::ContextError) enum and [`ContextResult`](error::ContextResult) alias.
/// - [`dto`]     — Query/response/selector DTOs.
/// - [`source`]  — [`ContextSource`](source::ContextSource) trait and adapters.
/// - [`builder`] — [`ResponseBuilder`](builder::ResponseBuilder) for bounded slices.
/// - [`summary`] — Deterministic summary renderer.
pub mod builder;
pub mod dto;
pub mod error;
pub mod server;
pub mod source;
pub mod summary;

pub use builder::ResponseBuilder;
pub use dto::{
    CONTEXT_SCHEMA_V1, ContextQuery, ContextResponse, FreshnessStatus, IndexInfo, ProvenanceBlock,
    QueryBudget, QueryScope, RedactionPolicy, RedactionState, RepairOption, ResponseLimits,
    SnapshotSelector,
};
pub use error::{
    ContextError, ContextResult, E_ACCESS_DENIED, E_BUDGET_EXCEEDED, E_CODEC, E_CONTEXT_STALE,
    E_INDEX_STALE, E_INVALID_BUDGET, E_NODE_NOT_FOUND, E_QUERY_AMBIGUOUS, E_REDACTION_REQUIRED,
    E_SNAPSHOT_NOT_FOUND,
};
pub use server::{
    AuthSession, ContextRequest, ContextResponse as ServerContextResponse, ContextRpcError,
    ContextRpcRequest, ContextRpcResponse, ContextServer, ContextServerConfig, DerivedIndexCache,
    DerivedIndexes, FieldRedactionRule, TrustLevel,
};
pub use source::{ContextSource, InMemoryContextSource, StoreContextSource};
pub use summary::render_summary;
