// ── ail-coordinator ───────────────────────────────────────────────────────
//
// Authoritative coordinator crate for multi-agent ChangeSet serialization.
//
// # Architecture
//
// The coordinator owns the authoritative live snapshot pointer and serializes
// concurrent ChangeSet submissions via a `tokio::sync::Mutex`.  Semantic rebase
// is a pure function over `CanonicalChangeSet` ops and a `StructuralDiff` value.
//
// # Modules
//
// - `conflict`    — Re-exports `ConflictReason` from `ail-change`.
// - `rebase`      — Pure `rebase()` function, `StructuralDiff`, `RebaseResult`.
// - `coordinator` — `Coordinator`, `CoordinatorOutcome`, `submit()` async impl.
//
// See `sdd/multi-agent-coordination/design` for the full architecture.

pub mod conflict;
pub mod coordinator;
pub mod rebase;

// Top-level re-exports for ergonomic use by consumers.
pub use conflict::ConflictReason;
pub use coordinator::{Coordinator, CoordinatorOutcome};
pub use rebase::{RebaseResult, StructuralDiff};
