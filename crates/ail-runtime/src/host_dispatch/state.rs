// ── ail-runtime::host_dispatch::state ─────────────────────────────────────

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wasmtime::StoreLimits;

use crate::audit::AuditLog;
use crate::handler::Handler;
use crate::host_dispatch::limits::ClockFn;
use crate::host_dispatch::result_diagnostics::{
    HostDispatchResultDiagnostic, sort_host_dispatch_result_diagnostics,
};
use crate::host_dispatch::trace::TraceContext;
use crate::profile::{CapabilityRevocationRegistry, RuntimeProfile};

// ── HostState ─────────────────────────────────────────────────────────────

/// Data carried in the Wasmtime `Store`.
///
/// Holds a reference to the handler registry so that host-function closures
/// registered in the `Linker` can dispatch capability calls without needing
/// a mutable borrow of `RuntimeHost`.
///
/// `Arc<Vec<Arc<dyn Handler + Send + Sync>>>` keeps the handlers alive and
/// allows cheap cloning into `'static` Wasmtime closures.
///
/// `limiter` implements [`wasmtime::ResourceLimiter`] and enforces
/// `max_memory_bytes` from the profile's [`ResourceLimits`].
pub(crate) struct HostState {
    /// Handler registry used by WASM host-call dispatch functions.
    ///
    /// Both `dispatch_host_call` and `dispatch_host_call_write` read this
    /// field via `caller.data()` to find the registered handler for each
    /// capability.  Stored in `HostState` (the Wasmtime `Store` data type)
    /// so that `'static` Linker closures can access it without holding a
    /// reference to `RuntimeHost`.
    pub(crate) handlers: Arc<Vec<Arc<dyn Handler + Send + Sync>>>,
    pub(crate) profile: Arc<RuntimeProfile>,
    /// WASM module name from the capability manifest.
    ///
    /// Used in `dispatch_host_call` to enforce per-module grant checks
    /// (`grants_capability(module, capability)`) and to annotate audit events.
    pub(crate) module_name: String,
    /// Shared audit log — same `Arc<Mutex<_>>` as `RuntimeHost::audit_log`.
    /// Events appended by `dispatch_host_call` (WASM-side) are visible in
    /// `RuntimeHost::audit_log()` after `invoke` returns.
    pub(crate) audit_log: Arc<Mutex<AuditLog>>,
    /// Resource limiter enforcing `max_memory_bytes`.
    pub(crate) limiter: StoreLimits,
    /// Stable redacted diagnostics recorded by WASM-side host dispatch.
    pub(crate) dispatch_result_diagnostics: Vec<HostDispatchResultDiagnostic>,
    /// Active distributed trace context for WASM-side capability calls.
    ///
    /// Set via [`RuntimeInstance::set_trace_context`].  When `Some`, every
    /// call through `dispatch_host_call` creates a child span and attaches
    /// it to the [`AuditEvent::CapabilityCallExecuted`] event.
    pub(crate) trace_context: Option<TraceContext>,
    /// Capability calls consumed by the active invocation.
    pub(crate) capability_calls_used: u64,
    /// Runtime revocations enforced after grants and before handler dispatch.
    pub(crate) revocations: CapabilityRevocationRegistry,
    /// Injectable clock for rate limit window tracking (nanoseconds since Unix epoch).
    pub(crate) clock_fn: ClockFn,
    /// Fixed-window call counters for `rate_limits` enforcement.
    ///
    /// Key: `None` for a global limit, `Some(cap_name)` for a per-capability limit.
    /// Value: `(window_start_nanos, call_count_in_window)`.
    pub(crate) rate_limit_windows: HashMap<Option<String>, (u64, u64)>,
    /// Number of currently in-flight concurrent capability calls from this store.
    ///
    /// Incremented when a capability call enters dispatch (after all grant/limit
    /// checks pass), decremented when it exits.  Enforces `concurrency_limit`.
    pub(crate) concurrent_calls: u64,
    /// Current host-call recursion depth from this store.
    ///
    /// Incremented on entry to any capability dispatch, decremented on exit.
    /// Enforces `recursion_stack_limit` for re-entrant call chains (e.g. a
    /// handler that calls back into the WASM runtime).
    pub(crate) call_depth: u64,
}

impl HostState {
    pub(crate) fn record_dispatch_result_diagnostic(
        &mut self,
        diagnostic: HostDispatchResultDiagnostic,
    ) {
        self.dispatch_result_diagnostics.push(diagnostic);
    }

    pub(crate) fn dispatch_result_diagnostics(&self) -> Vec<HostDispatchResultDiagnostic> {
        sort_host_dispatch_result_diagnostics(self.dispatch_result_diagnostics.clone())
    }

    pub(crate) fn clear_dispatch_result_diagnostics(&mut self) {
        self.dispatch_result_diagnostics.clear();
    }
}
