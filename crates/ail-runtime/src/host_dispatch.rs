// ── ail-runtime::host_dispatch ────────────────────────────────────────────
//
// WASM host call dispatch layer and shared runtime types.
//
// Provides:
//   - Public types: `TraceContext`, `RuntimeArg`, `RuntimeValue`, `RuntimeInstance`
//   - Internal type: `HostState` (Wasmtime Store data carrier)
//   - WASM instantiation: `instantiate_inner`
//   - WASM host imports: `dispatch_host_call`, `dispatch_host_call_write`
//   - Helpers: `unix_timestamp_micros`, `CapabilityAuditContext`

mod audit;
mod dispatch;
mod instance;
mod instantiate;
mod limits;
mod memory;
mod state;
mod trace;
mod values;
mod write_dispatch;

pub(crate) use dispatch::dispatch_host_call;
pub(crate) use instantiate::instantiate_inner;
pub(crate) use limits::{ClockFn, check_rate_limits, default_clock_fn};
pub(crate) use state::HostState;
pub(crate) use write_dispatch::dispatch_host_call_write;

pub use instance::RuntimeInstance;
pub use trace::TraceContext;
pub use values::{RuntimeArg, RuntimeValue};
