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

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use wasmtime::{Caller, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Val};

use crate::abi::HostError;
use crate::audit::{AuditEvent, AuditLog};
use crate::codec::{StructuredValue, ValueDecoder, ValueLayout};
use crate::error::{PreflightFailure, RuntimeError, RuntimeResult};
use crate::handler::Handler;
use crate::manifest::blake3_hex_of;
use crate::profile::{CapabilityId, CapabilityRevocationRegistry, RateLimit, RuntimeProfile};

// ── TraceContext ──────────────────────────────────────────────────────────

/// Distributed trace correlation context.
///
/// Carries the W3C-compatible identifiers for a single logical trace span.
/// When a [`TraceContext`] is active on a [`RuntimeHost`] or [`RuntimeInstance`],
/// every capability call creates a **child span**: the child inherits
/// `trace_id`, gets a fresh `span_id`, and records the parent's `span_id` in
/// `parent_span_id`.  The child context is attached to the
/// [`AuditEvent::CapabilityCallExecuted`] event for correlation.
///
/// [`RuntimeHost`]: crate::host::RuntimeHost
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceContext {
    /// Globally unique identifier for the logical trace (e.g. W3C `traceparent` trace-id).
    pub trace_id: String,
    /// Unique identifier for this specific span within the trace.
    pub span_id: String,
    /// The `span_id` of the direct parent span, or `None` for root spans.
    pub parent_span_id: Option<String>,
}

impl TraceContext {
    /// Derive a child span from this context.
    ///
    /// The child inherits `trace_id`, gets a fresh monotonic `span_id`, and
    /// sets `parent_span_id` to this span's `span_id`.
    pub fn child(&self) -> TraceContext {
        TraceContext {
            trace_id: self.trace_id.clone(),
            span_id: next_span_id(),
            parent_span_id: Some(self.span_id.clone()),
        }
    }
}

/// Generate a unique span ID using a monotonic counter.
///
/// The IDs are process-unique and ordered.  They are not cryptographically
/// random — use an external tracing library (e.g. `opentelemetry`) for
/// production-grade IDs.
fn next_span_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{id:016x}")
}

// ── RuntimeArg / RuntimeValue ─────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeArg {
    I64(i64),
    I32(i32),
    F64(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeValue {
    I64(i64),
    I32(i32),
    F64(f64),
    Unit,
}

impl fmt::Display for RuntimeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeValue::I64(v) => write!(f, "{v}"),
            RuntimeValue::I32(v) => write!(f, "{v}"),
            RuntimeValue::F64(v) => write!(f, "{v}"),
            RuntimeValue::Unit => write!(f, "()"),
        }
    }
}

fn runtime_arg_to_val(arg: &RuntimeArg) -> Val {
    match arg {
        RuntimeArg::I64(value) => Val::I64(*value),
        RuntimeArg::I32(value) => Val::I32(*value),
        RuntimeArg::F64(value) => Val::F64((*value).to_bits()),
    }
}

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

// ── RuntimeInstance ───────────────────────────────────────────────────────

/// A validated and instantiated WASM module ready for future execution.
///
/// Uses `Store<HostState>` so that Wasmtime host-function closures can
/// dispatch capability calls to registered handlers.
pub struct RuntimeInstance {
    // Keep the module alive for future metadata/call APIs.
    _module: Module,
    store: Store<HostState>,
    instance: wasmtime::Instance,
}

impl RuntimeInstance {
    /// Count exports visible from the instantiated module.
    ///
    /// Current compiler output has no exports, but this method proves tests are
    /// observing a real Wasmtime `Instance` rather than only a compiled module.
    pub fn export_count(&mut self) -> usize {
        self.instance.exports(&mut self.store).count()
    }

    pub fn invoke(
        &mut self,
        export_name: &str,
        args: &[RuntimeArg],
    ) -> RuntimeResult<RuntimeValue> {
        let func = self
            .instance
            .get_func(&mut self.store, export_name)
            .ok_or_else(|| {
                RuntimeError::EncodingError(format!("export `{export_name}` not found"))
            })?;
        let func_ty = func.ty(&self.store);
        let params = func_ty.params().collect::<Vec<_>>();
        if params.len() != args.len() {
            return Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` expects {} args, got {}",
                params.len(),
                args.len()
            )));
        }
        if params.iter().any(|ty| {
            !matches!(
                ty,
                wasmtime::ValType::I64 | wasmtime::ValType::I32 | wasmtime::ValType::F64
            )
        }) {
            return Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` only supports i64/i32/f64 parameters"
            )));
        }
        let results = func_ty.results().collect::<Vec<_>>();
        if results.len() > 1 {
            return Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` returned {} values; at most one is supported",
                results.len()
            )));
        }
        if results.iter().any(|ty| {
            !matches!(
                ty,
                wasmtime::ValType::I64 | wasmtime::ValType::I32 | wasmtime::ValType::F64
            )
        }) {
            return Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` only supports i64/i32/f64 results"
            )));
        }

        let wasm_args = args.iter().map(runtime_arg_to_val).collect::<Vec<_>>();
        let mut wasm_results = results
            .iter()
            .map(|ty| {
                Val::default_for_ty(ty).ok_or_else(|| {
                    RuntimeError::EncodingError(format!(
                        "export `{export_name}` has unsupported result type {ty}"
                    ))
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;

        self.store.data_mut().capability_calls_used = 0;
        func.call(&mut self.store, &wasm_args, &mut wasm_results)
            .map_err(|e| RuntimeError::EncodingError(format!("export `{export_name}`: {e}")))?;

        match wasm_results.as_slice() {
            [] => Ok(RuntimeValue::Unit),
            [Val::I64(value)] => Ok(RuntimeValue::I64(*value)),
            [Val::I32(value)] => Ok(RuntimeValue::I32(*value)),
            [Val::F64(bits)] => Ok(RuntimeValue::F64(f64::from_bits(*bits))),
            [value] => Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` returned unsupported value {value:?}"
            ))),
            _ => unreachable!("multiple results rejected before invocation"),
        }
    }

    /// Read `len` bytes from WASM linear memory starting at `ptr`.
    ///
    /// Returns `None` if:
    /// - `ptr` is negative.
    /// - The module has no exported `"memory"`.
    /// - The read range `[ptr, ptr + len)` exceeds the memory size.
    pub fn read_wasm_memory(&mut self, ptr: i32, len: usize) -> Option<Vec<u8>> {
        if ptr < 0 {
            return None;
        }
        let memory = self.instance.get_memory(&mut self.store, "memory")?;
        let mut buf = vec![0u8; len];
        memory.read(&self.store, ptr as usize, &mut buf).ok()?;
        Some(buf)
    }

    /// Read a little-endian `i64` from WASM linear memory at `ptr + byte_offset`.
    ///
    /// This is a **read-only, bounds-checked** helper intended for test-side
    /// memory introspection.  It does not modify the WASM linear memory.
    ///
    /// Returns `None` if:
    /// - Either `ptr` or `byte_offset` is negative.
    /// - `ptr + byte_offset` overflows a `u32` (WASM memory is 32-bit addressed).
    /// - `ptr + byte_offset` exceeds `i32::MAX` (would become negative when passed
    ///   to the memory accessor).
    /// - The module has no exported `"memory"`.
    /// - The 8-byte read range `[ptr + byte_offset, ptr + byte_offset + 8)` is
    ///   out of bounds for the current linear-memory size.
    pub fn read_memory_i64(&mut self, ptr: i32, byte_offset: i32) -> Option<i64> {
        if ptr < 0 || byte_offset < 0 {
            return None;
        }
        // Use u32 arithmetic to detect overflow before narrowing to i32.
        let base = (ptr as u32).checked_add(byte_offset as u32)?;
        let base_i32 = i32::try_from(base).ok()?;
        let bytes = self.read_wasm_memory(base_i32, 8)?;
        let arr: [u8; 8] = bytes.try_into().ok()?;
        Some(i64::from_le_bytes(arr))
    }

    /// Write `bytes` into WASM linear memory at `ptr`.
    ///
    /// Returns `true` on success, `false` if `ptr` is negative, if the module
    /// has no exported `"memory"`, or if the write range exceeds memory size.
    pub fn write_wasm_memory(&mut self, ptr: i32, bytes: &[u8]) -> bool {
        if ptr < 0 {
            return false;
        }
        let memory = match self.instance.get_memory(&mut self.store, "memory") {
            Some(m) => m,
            None => return false,
        };
        memory.write(&mut self.store, ptr as usize, bytes).is_ok()
    }

    /// Invoke an exported function and decode its return value as a `StructuredValue`.
    ///
    /// This is the typed ABI entry point.  It calls `invoke`, maps the raw
    /// `RuntimeValue` to a base integer, reads the full WASM linear memory for
    /// pointer-based decoding, then delegates to `ValueDecoder::decode`.
    ///
    /// `layout` must match the return type of the export as produced by the
    /// compiler (see `WasmArtifact::export_types`).
    pub fn invoke_typed(
        &mut self,
        export_name: &str,
        args: &[RuntimeArg],
        layout: &ValueLayout,
    ) -> RuntimeResult<StructuredValue> {
        let raw = self.invoke(export_name, args)?;
        let raw_i64 = match raw {
            RuntimeValue::I64(v) => v,
            RuntimeValue::I32(v) => v as i64,
            RuntimeValue::F64(f) => return Ok(StructuredValue::Float(f)),
            RuntimeValue::Unit => return Ok(StructuredValue::Unit),
        };
        let memory_size = self.wasm_memory_size();
        let memory = self.read_wasm_memory(0, memory_size).unwrap_or_default();
        Ok(ValueDecoder::decode(layout, raw_i64, &memory))
    }

    /// Return the byte size of the exported `"memory"`, or 0 if none.
    fn wasm_memory_size(&mut self) -> usize {
        self.instance
            .get_memory(&mut self.store, "memory")
            .map(|m| m.data_size(&self.store))
            .unwrap_or(0)
    }

    /// Invoke an exported WASM function asynchronously.
    ///
    /// Wraps the blocking [`invoke`](Self::invoke) call in
    /// [`tokio::task::block_in_place`], allowing callers to `.await` the
    /// result without blocking the tokio multi-thread executor.
    ///
    /// WASM execution itself is always synchronous.  The async wrapper yields
    /// cooperative control to the scheduler while the blocking call runs on
    /// the current thread, keeping other tasks responsive.
    ///
    /// # Mode note
    ///
    /// This method corresponds to [`CapabilityCallMode::Async`].  For
    /// synchronous invocations use [`invoke`](Self::invoke) directly.
    ///
    /// # Errors
    ///
    /// Propagates the same errors as [`invoke`](Self::invoke): missing
    /// exports, arity mismatches, and WASM traps.
    ///
    /// # Panics
    ///
    /// Panics if called from a tokio `current_thread` runtime that cannot
    /// support `block_in_place`.  Always use `flavor = "multi_thread"` or
    /// the default multi-thread scheduler.
    pub async fn invoke_async(
        &mut self,
        export_name: &str,
        args: &[RuntimeArg],
    ) -> RuntimeResult<RuntimeValue> {
        let export_name = export_name.to_string();
        let args = args.to_vec();
        // `Store<HostState>` is !Send — we cannot move it into spawn_blocking.
        // `block_in_place` runs the closure on the current (blocking-capable)
        // thread without requiring Send, satisfying Wasmtime's constraint.
        tokio::task::block_in_place(|| self.invoke(&export_name, &args))
    }

    /// Set the active distributed trace context for WASM-side capability calls.
    ///
    /// After calling this, every capability call dispatched through
    /// `dispatch_host_call` (WASM import `ail/host_call`) will create a child
    /// span derived from `ctx` and attach it to the `CapabilityCallExecuted`
    /// audit event for distributed trace correlation.
    ///
    /// Call with a fresh [`TraceContext`] at the start of each WASM invocation
    /// to correlate audit events with your tracing backend.
    pub fn set_trace_context(&mut self, ctx: TraceContext) {
        self.store.data_mut().trace_context = Some(ctx);
    }

    /// Return the currently active trace context, if any.
    ///
    /// Returns `None` if no context has been set via [`set_trace_context`].
    pub fn trace_context(&self) -> Option<TraceContext> {
        self.store.data().trace_context.clone()
    }

    /// Return a snapshot of the audit log from the shared log.
    pub fn audit_log(&self) -> AuditLog {
        self.store
            .data()
            .audit_log
            .lock()
            .expect("audit_log lock must not be poisoned")
            .clone()
    }
}

impl fmt::Debug for RuntimeInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeInstance").finish_non_exhaustive()
    }
}

// ── instantiate_inner ─────────────────────────────────────────────────────

/// Validate and instantiate `wasm` via the Wasmtime `Linker` (stages 4+5).
///
/// Using `linker.instantiate` (instead of bare `Instance::new`) allows
/// WASM modules that declare `(import "ail" "host_call" ...)` to be
/// satisfied at link time.  Existing test WASMs with no imports work
/// identically — the Linker simply has no imports to satisfy.
///
/// Resource limits from the profile are applied here:
/// - `max_fuel`: sets the fuel budget on the `Store`; modules that exhaust
///   it trap with [`PreflightFailure::ResourceLimitExceeded`].
/// - `max_memory_bytes`: wires a [`StoreLimits`] resource limiter that
///   denies memory growth beyond the configured byte cap.
#[allow(clippy::too_many_arguments)]
pub(crate) fn instantiate_inner(
    engine: &Engine,
    linker: &Linker<HostState>,
    handlers: &[Arc<dyn Handler + Send + Sync>],
    revocations: &CapabilityRevocationRegistry,
    audit_log: &Arc<Mutex<AuditLog>>,
    wasm: &[u8],
    profile: &RuntimeProfile,
    module_name: &str,
    clock_fn: ClockFn,
) -> RuntimeResult<RuntimeInstance> {
    Module::validate(engine, wasm).map_err(|e| {
        RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
    })?;

    let module = Module::new(engine, wasm).map_err(|e| {
        RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
    })?;

    // Build the StoreLimits for memory caps (no-op when None).
    let store_limits: StoreLimits = match profile.limits().max_memory_bytes {
        Some(max_bytes) => StoreLimitsBuilder::new()
            .memory_size(max_bytes as usize)
            .trap_on_grow_failure(true)
            .build(),
        None => StoreLimitsBuilder::new().build(),
    };

    let handlers_arc: Arc<Vec<Arc<dyn Handler + Send + Sync>>> = Arc::new(handlers.to_vec());
    let mut store = Store::new(
        engine,
        HostState {
            handlers: handlers_arc,
            profile: Arc::new(profile.clone()),
            module_name: module_name.to_string(),
            audit_log: audit_log.clone(),
            limiter: store_limits,
            trace_context: None,
            capability_calls_used: 0,
            revocations: revocations.clone(),
            clock_fn,
            rate_limit_windows: HashMap::new(),
            concurrent_calls: 0,
            call_depth: 0,
        },
    );

    // Wire the resource limiter so Wasmtime consults it on memory growth.
    store.limiter(|state| &mut state.limiter);

    // Set the fuel budget.  When consume_fuel is enabled on the Engine
    // every Store starts with 0 fuel — we must always initialise it.
    // If no limit is configured, grant effectively-unlimited fuel.
    let fuel = profile.limits().max_fuel.unwrap_or(u64::MAX);
    store.set_fuel(fuel).map_err(|e| {
        RuntimeError::PreflightFailed(PreflightFailure::ResourceLimitExceeded {
            reason: format!("failed to set fuel: {e}"),
        })
    })?;

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(classify_instantiate_error)?;

    Ok(RuntimeInstance {
        _module: module,
        store,
        instance,
    })
}

/// Classify a Wasmtime instantiation error into a [`RuntimeError`].
///
/// Wasmtime traps (including fuel exhaustion and memory limit denials)
/// surface as `wasmtime::Error` (`anyhow::Error`) potentially wrapping a
/// [`wasmtime::Trap`].  We detect:
/// - `Trap::OutOfFuel` — fuel budget exhausted
/// - `Trap::MemoryOutOfBounds` — memory growth denied by `StoreLimits`
///   (when `trap_on_grow_failure(true)` is set)
///
/// Both are mapped to [`PreflightFailure::ResourceLimitExceeded`];
/// all other errors become [`PreflightFailure::WasmValidationError`].
fn classify_instantiate_error(e: wasmtime::Error) -> RuntimeError {
    // Wasmtime wraps traps inside the anyhow::Error chain.  Walk the
    // chain looking for resource-limit indicators.
    //
    // - Fuel exhaustion: `wasmtime::Trap::OutOfFuel` appears in the chain.
    // - Memory growth denial: when `StoreLimitsBuilder::trap_on_grow_failure(true)`
    //   is set, Wasmtime wraps the denial as a plain `String`-like error whose
    //   display contains "forcing trap when growing memory to".  This string is
    //   emitted by wasmtime internals and is stable across patch versions.
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(e.as_ref());
    while let Some(err) = source {
        if err
            .downcast_ref::<wasmtime::Trap>()
            .is_some_and(|t| *t == wasmtime::Trap::OutOfFuel)
        {
            return RuntimeError::PreflightFailed(PreflightFailure::ResourceLimitExceeded {
                reason: "fuel limit exceeded".to_string(),
            });
        }
        if err.to_string().contains("forcing trap when growing memory") {
            return RuntimeError::PreflightFailed(PreflightFailure::ResourceLimitExceeded {
                reason: "memory growth denied by resource limiter".to_string(),
            });
        }
        source = err.source();
    }
    RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Return the current Unix timestamp in microseconds.
///
/// Used to stamp [`AuditEvent::CapabilityCallExecuted`] events.
/// Falls back to 0 if the system clock is before the Unix epoch (pathological).
pub(crate) fn unix_timestamp_micros() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// ── Clock abstraction for rate limit windows ──────────────────────────────

/// A function that returns the current time as nanoseconds since Unix epoch.
///
/// `Arc<dyn Fn()>` instead of a trait keeps the API simple and avoids
/// object-safety constraints.  The default implementation calls
/// `SystemTime::now()`; tests inject a controllable counter instead.
pub(crate) type ClockFn = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Return the default wall-clock `ClockFn` (nanoseconds since Unix epoch).
pub(crate) fn default_clock_fn() -> ClockFn {
    Arc::new(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    })
}

/// Check rate limits for `cap` and update the sliding-window counters.
///
/// Uses a **fixed window** strategy: each `RateLimit` entry maintains an
/// independent `(window_start_nanos, call_count)` pair.  When the clock
/// advances ≥ 1 second past `window_start`, the window resets.
///
/// Returns `true` if the call is allowed, `false` if any applicable limit
/// would be exceeded.  Uses a two-pass approach: check ALL limits before
/// mutating ANY window so that a denial leaves the state unchanged.
///
/// `rate_limits` — the ordered list of limits from the active profile.
/// `clock_fn`    — injectable clock (nanoseconds since Unix epoch).
/// `windows`     — per-limit window state stored in `HostState`.
/// `cap`         — the capability being invoked (used for per-cap matching).
pub(crate) fn check_rate_limits(
    rate_limits: &[RateLimit],
    clock_fn: &ClockFn,
    windows: &mut HashMap<Option<String>, (u64, u64)>,
    cap: &CapabilityId,
) -> bool {
    if rate_limits.is_empty() {
        return true;
    }

    const WINDOW_NANOS: u64 = 1_000_000_000; // 1 second
    let now = clock_fn();

    // Pass 1: check all applicable limits without mutating state.
    for rl in rate_limits {
        let applies = rl.capability.is_none() || rl.capability.as_deref() == Some(cap.as_str());
        if !applies {
            continue;
        }
        let key = &rl.capability;
        let (window_start, count) = windows.get(key).copied().unwrap_or((now, 0));
        let effective_count = if now.saturating_sub(window_start) >= WINDOW_NANOS {
            0 // window expired; effective count resets to 0
        } else {
            count
        };
        if effective_count >= rl.max_calls_per_second {
            return false;
        }
    }

    // Pass 2: update all applicable windows (only reached if all checks pass).
    // Track which keys have already been incremented so that duplicate RateLimit
    // entries sharing the same key (e.g. two global `capability: None` entries)
    // do not double-count a single call.
    let mut updated_keys: HashSet<Option<String>> = HashSet::new();
    for rl in rate_limits {
        let applies = rl.capability.is_none() || rl.capability.as_deref() == Some(cap.as_str());
        if !applies {
            continue;
        }
        let key = rl.capability.clone();
        if !updated_keys.insert(key.clone()) {
            // This key was already incremented by an earlier duplicate entry.
            continue;
        }
        let window = windows.entry(key).or_insert((now, 0));
        if now.saturating_sub(window.0) >= WINDOW_NANOS {
            *window = (now, 1); // start a fresh window
        } else {
            window.1 += 1;
        }
    }

    true
}

#[derive(Clone)]
pub(crate) struct CapabilityAuditContext {
    pub(crate) start: Instant,
    pub(crate) timestamp: u64,
    pub(crate) profile: Option<String>,
    pub(crate) module: Option<String>,
    pub(crate) input_hash: Option<String>,
    pub(crate) trace_id: Option<String>,
    pub(crate) verification_report_hash: Option<String>,
    pub(crate) trace_context: Option<TraceContext>,
    /// Generic failure category set by the handler on denial.
    ///
    /// Defaults to `None`.  Set this field on a clone of the context before
    /// calling `push` when the handler returned a categorized denial (e.g.
    /// `"secret.not_found"`, `"secret.provider_unavailable"`).  The category
    /// MUST NOT contain secret IDs, vault paths, or any sensitive data.
    pub(crate) denial_category: Option<String>,
}

impl CapabilityAuditContext {
    /// Append a [`AuditEvent::CapabilityCallExecuted`] event to `audit_log`.
    ///
    /// The `denial_category` field of this context is forwarded to the event;
    /// set it to `Some(category)` on a clone before calling `push` when the
    /// handler returned a `CapabilityDeniedCategorized` error.
    pub(crate) fn push(
        &self,
        audit_log: &Arc<Mutex<AuditLog>>,
        capability: CapabilityId,
        operation: String,
        handler_name: String,
        succeeded: bool,
        output_hash: Option<String>,
    ) {
        audit_log
            .lock()
            .expect("audit_log lock")
            .push(AuditEvent::CapabilityCallExecuted {
                capability,
                operation,
                handler_name,
                succeeded,
                duration_us: self.start.elapsed().as_micros() as u64,
                timestamp: self.timestamp,
                profile: self.profile.clone(),
                module: self.module.clone(),
                function: None,
                input_hash: self.input_hash.clone(),
                output_hash,
                trace_id: self.trace_id.clone(),
                verification_report_hash: self.verification_report_hash.clone(),
                trace_context: self.trace_context.clone(),
                denial_category: self.denial_category.clone(),
            });
    }
}

fn read_memory(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    if ptr < 0 || len < 0 {
        return None;
    }
    let memory = caller.get_export("memory")?.into_memory()?;
    let mut bytes = vec![0; len as usize];
    memory.read(caller, ptr as usize, &mut bytes).ok()?;
    Some(bytes)
}

/// Dispatch a structured capability call.
///
/// Reads cap/op/args from WASM memory, dispatches to the matching handler,
/// and writes the handler's response bytes into WASM memory at `out_ptr`
/// (up to `out_max` bytes).  Returns the number of bytes written as `Some(n)`
/// on success, or `None` (→ -1 at the call site) on denial, missing handler,
/// overflow, or any other error.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_host_call_write(
    caller: &mut Caller<'_, HostState>,
    cap_ptr: i32,
    cap_len: i32,
    op_ptr: i32,
    op_len: i32,
    args_ptr: i32,
    args_len: i32,
    out_ptr: i32,
    out_max: i32,
) -> Option<i32> {
    // Read capability name, operation name, and args bytes from WASM memory.
    let capability = String::from_utf8(read_memory(caller, cap_ptr, cap_len)?).ok()?;
    let operation = String::from_utf8(read_memory(caller, op_ptr, op_len)?).ok()?;
    let args_bytes = read_memory(caller, args_ptr, args_len.checked_mul(8)?)?;
    let cap = CapabilityId::new(capability);
    let start = Instant::now();
    let timestamp = unix_timestamp_micros();
    let input_hash = Some(blake3_hex_of(&args_bytes));

    let child_trace = caller.data().trace_context.as_ref().map(|ctx| ctx.child());
    let audit_log = caller.data().audit_log.clone();
    let audit = CapabilityAuditContext {
        start,
        timestamp,
        profile: Some(caller.data().profile.name().to_string()),
        module: Some(caller.data().module_name.clone()),
        input_hash,
        trace_id: child_trace.as_ref().map(|tc| tc.trace_id.clone()),
        verification_report_hash: Some(
            caller.data().profile.verification_report_hash().to_string(),
        ),
        trace_context: child_trace,
        denial_category: None,
    };

    // Validate output buffer params after decoding call metadata so failures are auditable.
    if out_ptr < 0 || out_max < 0 {
        audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
        return None;
    }

    // Grant check (module-scoped).
    {
        let state = caller.data_mut();
        if !state.profile.grants_capability(&state.module_name, &cap) {
            audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
            return None;
        }
        if state
            .revocations
            .is_revoked(&state.module_name, cap.as_str(), state.profile.name())
        {
            audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
            return None;
        }
        if let Some(max_payload_bytes) = state.profile.limits().payload_size_limit
            && args_bytes.len() as u64 > max_payload_bytes
        {
            audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
            return None;
        }
        if let Some(max_calls) = state.profile.limits().max_capability_calls
            && state.capability_calls_used >= max_calls
        {
            audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
            return None;
        }
        // Rate limit enforcement.
        let clock_fn = state.clock_fn.clone();
        let rate_limits_vec = state
            .profile
            .limits()
            .rate_limits
            .clone()
            .unwrap_or_default();
        if !check_rate_limits(
            &rate_limits_vec,
            &clock_fn,
            &mut state.rate_limit_windows,
            &cap,
        ) {
            audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
            return None;
        }
        // Concurrency limit enforcement.
        if let Some(max_concurrent) = state.profile.limits().concurrency_limit
            && state.concurrent_calls >= max_concurrent
        {
            audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
            return None;
        }
        // Recursion stack (call depth) limit enforcement.
        if let Some(max_depth) = state.profile.limits().recursion_stack_limit
            && state.call_depth >= max_depth
        {
            audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
            return None;
        }
    }

    // Find the matching handler.
    let handler = {
        let state = caller.data();
        state
            .handlers
            .iter()
            .find(|h| h.capabilities().contains(&cap))
            .cloned()
    };
    let Some(handler) = handler else {
        audit.push(&audit_log, cap, operation, "none".to_string(), false, None);
        return None;
    };
    // Increment AFTER handler is found: a granted-but-unbound call does NOT
    // consume a capability call slot.  This is intentional pre-existing
    // behavior (matches main verbatim) — contrast with `dispatch_host_call`,
    // which increments before handler lookup.  The regression tests in
    // dispatch_parity_tests.rs cover both.
    {
        let state = caller.data_mut();
        state.capability_calls_used += 1;
        // Concurrency and depth counters track in-flight calls; decremented at
        // every return point below.
        state.concurrent_calls += 1;
        state.call_depth += 1;
    }
    let handler_name = handler.name().to_string();

    // Dispatch.
    let result = handler.handle(&cap, &operation, &args_bytes);
    let response = match result {
        Ok(response) => response,
        Err(err) => {
            // Extract the generic audit category before discarding the error.
            // The category is opaque (no secret data) and recorded only in the
            // audit log.  The caller only sees the -1 return code.
            let denial_category = err.audit_category().map(|s| s.to_string());
            {
                let state = caller.data_mut();
                state.concurrent_calls -= 1;
                state.call_depth -= 1;
            }
            // Clone the context so we can attach the category without mutating
            // the shared `audit` context used by other push sites.
            let mut audit_err = audit.clone();
            audit_err.denial_category = denial_category;
            audit_err.push(&audit_log, cap, operation, handler_name, false, None);
            return None;
        }
    };

    if let Some(max_output_bytes) = caller.data().profile.limits().output_size_limit
        && response.len() as u64 > max_output_bytes
    {
        {
            let state = caller.data_mut();
            state.concurrent_calls -= 1;
            state.call_depth -= 1;
        }
        audit.push(&audit_log, cap, operation, handler_name, false, None);
        return None;
    }

    // Bounds-check: response must fit in the out buffer.
    if response.len() > out_max as usize {
        {
            let state = caller.data_mut();
            state.concurrent_calls -= 1;
            state.call_depth -= 1;
        }
        audit.push(&audit_log, cap, operation, handler_name, false, None);
        return None;
    }

    // Write response bytes to WASM memory at out_ptr.
    let memory = match caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
    {
        Some(memory) => memory,
        None => {
            {
                let state = caller.data_mut();
                state.concurrent_calls -= 1;
                state.call_depth -= 1;
            }
            audit.push(&audit_log, cap, operation, handler_name, false, None);
            return None;
        }
    };
    if memory
        .write(&mut *caller, out_ptr as usize, &response)
        .is_err()
    {
        {
            let state = caller.data_mut();
            state.concurrent_calls -= 1;
            state.call_depth -= 1;
        }
        audit.push(&audit_log, cap, operation, handler_name, false, None);
        return None;
    }

    let output_hash = Some(blake3_hex_of(response.as_slice()));
    {
        let state = caller.data_mut();
        state.concurrent_calls -= 1;
        state.call_depth -= 1;
    }
    audit.push(&audit_log, cap, operation, handler_name, true, output_hash);

    Some(response.len() as i32)
}

pub(crate) fn dispatch_host_call(
    caller: &mut Caller<'_, HostState>,
    cap_ptr: i32,
    cap_len: i32,
    op_ptr: i32,
    op_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> Option<i64> {
    let capability = String::from_utf8(read_memory(caller, cap_ptr, cap_len)?).ok()?;
    let operation = String::from_utf8(read_memory(caller, op_ptr, op_len)?).ok()?;
    let args_bytes = read_memory(caller, args_ptr, args_len.checked_mul(8)?)?;
    let cap = CapabilityId::new(capability);
    let start = Instant::now();
    let timestamp = unix_timestamp_micros();
    let input_hash = Some(blake3_hex_of(&args_bytes));

    // Derive a child span from the active trace context (if any).
    let child_trace = caller.data().trace_context.as_ref().map(|ctx| ctx.child());

    // Snapshot context needed for audit events.
    let module_name = caller.data().module_name.clone();
    let profile_name = Some(caller.data().profile.name().to_string());
    let vr_hash = Some(caller.data().profile.verification_report_hash().to_string());
    let trace_id = child_trace.as_ref().map(|tc| tc.trace_id.clone());

    let handler = {
        let state = caller.data_mut();
        // Grant check (module-scoped).
        if !state.profile.grants_capability(&state.module_name, &cap) {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        if state
            .revocations
            .is_revoked(&state.module_name, cap.as_str(), state.profile.name())
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        if let Some(max_payload_bytes) = state.profile.limits().payload_size_limit
            && args_bytes.len() as u64 > max_payload_bytes
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        if let Some(max_calls) = state.profile.limits().max_capability_calls
            && state.capability_calls_used >= max_calls
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        // Rate limit enforcement.
        let clock_fn = state.clock_fn.clone();
        let rate_limits_vec = state
            .profile
            .limits()
            .rate_limits
            .clone()
            .unwrap_or_default();
        if !check_rate_limits(
            &rate_limits_vec,
            &clock_fn,
            &mut state.rate_limit_windows,
            &cap,
        ) {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        // Concurrency limit enforcement.
        if let Some(max_concurrent) = state.profile.limits().concurrency_limit
            && state.concurrent_calls >= max_concurrent
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        // Recursion stack (call depth) limit enforcement.
        if let Some(max_depth) = state.profile.limits().recursion_stack_limit
            && state.call_depth >= max_depth
        {
            state.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
                    capability: cap,
                    operation,
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us: start.elapsed().as_micros() as u64,
                    timestamp,
                    profile: profile_name,
                    module: Some(module_name),
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                    denial_category: None,
                },
            );
            return Some(-1);
        }
        // Increment BEFORE handler lookup: a granted-but-unbound call still
        // consumes a capability call slot.  This is intentional pre-existing
        // behavior (matches main verbatim) — contrast with
        // `dispatch_host_call_write`, which increments only after a handler is
        // found.  The regression tests in dispatch_parity_tests.rs cover both.
        state.capability_calls_used += 1;
        // Concurrency and depth counters track in-flight calls; decremented at
        // every return point below.
        state.concurrent_calls += 1;
        state.call_depth += 1;
        state
            .handlers
            .iter()
            .find(|h| h.capabilities().contains(&cap))
            .cloned()
    };

    let Some(handler) = handler else {
        let state = caller.data_mut();
        state.concurrent_calls -= 1;
        state.call_depth -= 1;
        state
            .audit_log
            .lock()
            .expect("audit_log lock")
            .push(AuditEvent::CapabilityCallExecuted {
                capability: cap,
                operation,
                handler_name: "none".to_string(),
                succeeded: false,
                duration_us: start.elapsed().as_micros() as u64,
                timestamp,
                profile: profile_name,
                module: Some(module_name),
                function: None,
                input_hash,
                output_hash: None,
                trace_id,
                verification_report_hash: vr_hash,
                trace_context: child_trace,
                denial_category: None,
            });
        return Some(-1);
    };

    let handler_name = handler.name().to_string();
    let result = match handler.handle(&cap, &operation, &args_bytes) {
        Ok(bytes) => {
            if let Some(max_output_bytes) = caller.data().profile.limits().output_size_limit
                && bytes.len() as u64 > max_output_bytes
            {
                Err(HostError::LimitExceeded(format!(
                    "output_size_limit exceeded: limit={max_output_bytes}"
                )))
            } else {
                Ok(bytes)
            }
        }
        Err(err) => Err(err),
    };
    // Extract the generic audit category before the result is consumed.
    // The category is opaque (no secret data) and recorded only in the audit
    // log.  The caller only sees the -1 return code — no category is leaked.
    let denial_category = result
        .as_ref()
        .err()
        .and_then(|e| e.audit_category())
        .map(|s| s.to_string());
    let succeeded = result.is_ok();
    let output_hash = result
        .as_ref()
        .ok()
        .map(|bytes| blake3_hex_of(bytes.as_slice()));
    {
        let state = caller.data_mut();
        state.concurrent_calls -= 1;
        state.call_depth -= 1;
        state
            .audit_log
            .lock()
            .expect("audit_log lock")
            .push(AuditEvent::CapabilityCallExecuted {
                capability: cap,
                operation,
                handler_name,
                succeeded,
                duration_us: start.elapsed().as_micros() as u64,
                timestamp,
                profile: profile_name,
                module: Some(module_name),
                function: None,
                input_hash,
                output_hash,
                trace_id,
                verification_report_hash: vr_hash,
                trace_context: child_trace,
                denial_category,
            });
    }

    match result {
        Ok(bytes) if bytes.len() >= 8 => {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[..8]);
            Some(i64::from_le_bytes(buf))
        }
        Ok(_) => Some(0),
        Err(_) => Some(-1),
    }
}
