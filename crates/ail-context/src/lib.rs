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
pub mod source;
pub mod summary;

pub use builder::ResponseBuilder;
pub use dto::{ContextQuery, ContextResponse, QueryScope, SnapshotSelector};
pub use error::{ContextError, ContextResult};
pub use source::{ContextSource, InMemoryContextSource, StoreContextSource};
pub use summary::render_summary;
