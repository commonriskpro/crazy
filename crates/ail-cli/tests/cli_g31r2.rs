// ── ail-cli integration tests: G31 R2 + T7d ───────────────────────────────
//
// Covers:
//   G31 R2 — extended coverage of all commands (no package/remote)
//   T7d    — LLM agent loop E2E
//
// Shared helpers live in common/mod.rs.

mod common;

mod change;
mod change_workflows;
mod compile_run;
mod context_impact;
mod doctor_agent;
mod inspect_status;
mod verify_apply_policy;
