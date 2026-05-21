// ── ail-coordinator::conflict ─────────────────────────────────────────────
//
// Conflict reason type for the coordinator layer.
//
// `ConflictReason` is imported from `ail_change::model` and re-exported here
// so that coordinator consumers have a single import path.  The coordinator
// does not need its own copy — the variant set is owned by `ail-change`.

pub use ail_change::model::ConflictReason;
