// ── ail-verify ────────────────────────────────────────────────────────────
//
// Pure type/effect/capability verification layer for the AIL semantic graph.
//
// # Phase 5 scope
//
// `ail-verify` reads a `&SemanticGraph` (from `ail-core`) and emits a
// `VerificationReport` without performing any I/O, mutations, or external
// solver calls.  It does NOT wire into `ChangeSetOp::Verify` — that remains
// a no-op placeholder.
//
// # Module layout
//
// - `report`  — `VerificationState`, `VerificationEntry`, `VerificationReport`
//               (six-state enum + priority-based summary).
// - `checker` — `Checker::check(&SemanticGraph) -> VerificationReport`
//               (pure graph walker; stubbed until PR 2).
//
// These modules are scaffolded here for workspace compilation; their full
// implementations ship in PR 2.

// PR 2 will add:
// pub mod checker;
// pub mod report;
