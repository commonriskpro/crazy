// ── ail-runtime::wasmtime_tests ──────────────────────────────────────────
//
// Task 3.2 (RED): Tests written BEFORE host.rs / RuntimeHost exist.
//
// Spec scenarios covered:
//  - Malformed WASM bytes are rejected at Wasmtime validation (WasmValidationError).
//  - WASM produced by ail-compiler validates and instantiates successfully.
//  - Failed preflight (hash mismatch) blocks Wasmtime invocation.
//  - RuntimeHost::new() succeeds (engine initialization is infallible from caller's view).

#[path = "wasmtime/control_flow/mod.rs"]
mod control_flow;
#[path = "wasmtime/fold.rs"]
mod fold;
#[path = "wasmtime/helpers.rs"]
mod helpers;
#[path = "wasmtime/invoke.rs"]
mod invoke;
#[path = "wasmtime/resources/mod.rs"]
mod resources;
#[path = "wasmtime/validation.rs"]
mod validation;
#[path = "wasmtime/variants.rs"]
mod variants;
