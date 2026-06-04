// ── ail-cli integration tests: G31 R2 + T7d ───────────────────────────────
//
// Covers:
//   G31 R2 — extended coverage of all commands (no package/remote)
//   T7d    — LLM agent loop E2E
//
// Shared helpers live in common/mod.rs.

mod common;

#[path = "cli_g31r2/change.rs"]
mod change;
#[path = "cli_g31r2/change_workflows.rs"]
mod change_workflows;
#[path = "cli_g31r2/compile_run.rs"]
mod compile_run;
#[path = "cli_g31r2/context_impact.rs"]
mod context_impact;
#[path = "cli_g31r2/doctor_agent.rs"]
mod doctor_agent;
#[path = "cli_g31r2/inspect_status.rs"]
mod inspect_status;
#[path = "cli_g31r2/verify_apply_policy.rs"]
mod verify_apply_policy;
