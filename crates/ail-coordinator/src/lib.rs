// ── ail-coordinator ───────────────────────────────────────────────────────
//
// Authoritative coordinator crate for multi-agent ChangeSet serialization.
//
// # Status
//
// PR1 workspace scaffold — module declarations reserved for PR2 implementation.
// The crate compiles clean as a workspace member so PR1 verification passes.
//
// # Architecture
//
// The coordinator owns the authoritative live snapshot pointer and serializes
// concurrent ChangeSet submissions via a `tokio::sync::Mutex`.  Semantic rebase
// is a pure function over `CanonicalChangeSet` ops and a `StructuralDiff` value.
//
// See `sdd/multi-agent-coordination/design` for the full architecture.
