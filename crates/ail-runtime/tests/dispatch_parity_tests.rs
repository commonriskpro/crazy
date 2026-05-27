// ── ail-runtime::dispatch_parity_tests ───────────────────────────────────
//
// TDD RED phase — verifying dispatch_host_call grant check parity with
// call_capability (R-4).
//
// Spec scenarios covered (R-4a, R-4b, R-4c):
//  - WASM call to granted capability → handler dispatched, returns result.
//  - WASM call to ungranted capability → returns -1, no handler called.
//  - WASM call to granted capability with no bound handler → returns -1.

#[path = "dispatch_parity/helpers.rs"]
mod helpers;
#[path = "dispatch_parity/host_call.rs"]
mod host_call;
#[path = "dispatch_parity/host_call_write.rs"]
mod host_call_write;
#[path = "dispatch_parity/limits.rs"]
mod limits;
