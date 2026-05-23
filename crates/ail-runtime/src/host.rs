// ── ail-runtime::host ────────────────────────────────────────────────────
//
// `RuntimeHost` — capability-gated Wasmtime host.
//
// Preflight pipeline (strict order):
//   0. Package trust gate       — optional; skipped when no packages declared
//   1. WASM bytes hash check    — blake3(wasm) vs profile.module_hash()
//   2. Manifest hash check      — blake3(cbor(manifest)) vs profile.capability_manifest_hash()
//   3. Capability grant check   — manifest.requires ⊆ profile.grants
//   4. Wasmtime validation      — Module::validate (structural / binary format)
//   5. Wasmtime instantiation   — linker.instantiate (replaces bare Instance::new)
//   6. Handler binding check    — only when profile.require_handler_binding() is true;
//                                 every granted capability must have a registered handler
//
// Exactly one AuditEvent is appended per `validate_and_instantiate` call.
// One AuditEvent::CapabilityCallExecuted is appended per `call_capability` call.
//
// Schema enforcement (G29 R2):
//   When a CapabilityDefinition is registered via `with_capability_definition`,
//   `call_capability` validates the input payload against the declared
//   CapabilityInputSchema before dispatching to the handler, then validates
//   successful handler responses against the declared CapabilityOutputSchema.
//   Calls for capabilities without a registered schema pass through unchanged.
//
// Transaction rollback integration (G29 R2):
//   `execute_with_rollback(tx, closure)` runs the closure with a `&mut RuntimeHost`.
//   On closure success, it commits the TransactionGroup.
//   On closure failure, it rolls back the TransactionGroup.
//   `execute_with_rollback_detail` additionally returns the non-rollbackable
//   capability IDs on failure.
//
// Wasmtime Linker:
//   A `Linker<HostState>` is constructed once at `RuntimeHost::new` and
//   registers the stub import `ail/host_call`.  `Store<HostState>` carries
//   an `Arc<HandlerRegistry>` so host-function closures can dispatch to
//   registered handlers.  The `Store` data type changed from `()` to
//   `HostState` — all existing tests are unaffected because the Linker
//   satisfies WASM modules that have no host imports (existing test WASMs).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ── TraceContext ──────────────────────────────────────────────────────────

/// Distributed trace correlation context.
///
/// Carries the W3C-compatible identifiers for a single logical trace span.
/// When a [`TraceContext`] is active on a [`RuntimeHost`] or [`RuntimeInstance`],
/// every capability call creates a **child span**: the child inherits
/// `trace_id`, gets a fresh `span_id`, and records the parent's `span_id` in
/// `parent_span_id`.  The child context is attached to the
/// [`AuditEvent::CapabilityCallExecuted`] event for correlation.
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

// ── CapabilityCallMode ────────────────────────────────────────────────────

/// Dispatch mode for capability calls.
///
/// `Sync` — the default path.  The host dispatches the call on the calling
/// thread and blocks until the handler returns.
///
/// `Async` — enables host-side async scheduling via
/// [`invoke_async`](RuntimeInstance::invoke_async).  WASM execution itself
/// is always synchronous; the variant signals that the Rust wrapper may
/// offload the blocking call to a tokio thread via
/// [`tokio::task::block_in_place`], keeping the async executor responsive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityCallMode {
    /// Synchronous host dispatch (default).
    Sync,
    /// Async host dispatch — use with `invoke_async`.
    Async,
}

use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Val};

use ail_package::manifest::PackageManifest;
use ail_package::trust::TrustLevel;

use crate::abi::{HostError, HostResult};
use crate::audit::{AuditEvent, AuditLog};
use crate::codec::{StructuredValue, ValueDecoder, ValueLayout};
use crate::error::{PreflightFailure, RuntimeError, RuntimeResult};
use crate::handler::Handler;
use crate::manifest::{CapabilityManifest, blake3_hex_of};
use crate::profile::{CapabilityId, RuntimeProfile};
use crate::report::{CapabilityCallSummary, RuntimeReport, RuntimeReportStatus};
use crate::schema::CapabilityDefinition;
use crate::transaction::TransactionGroup;

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

impl std::fmt::Display for RuntimeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeValue::I64(v) => write!(f, "{v}"),
            RuntimeValue::I32(v) => write!(f, "{v}"),
            RuntimeValue::F64(v) => write!(f, "{v}"),
            RuntimeValue::Unit => write!(f, "()"),
        }
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
    /// Handler registry shared with Wasmtime host-function closures.
    ///
    /// Currently not read by the stub `ail/host_call` closure — dispatch
    /// happens via `RuntimeHost::call_capability` on the host side.  The
    /// field is retained so full in-WASM dispatch can be wired in a future
    /// phase without changing the `Store` type.
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

fn runtime_arg_to_val(arg: &RuntimeArg) -> Val {
    match arg {
        RuntimeArg::I64(value) => Val::I64(*value),
        RuntimeArg::I32(value) => Val::I32(*value),
        RuntimeArg::F64(value) => Val::F64((*value).to_bits()),
    }
}

impl fmt::Debug for RuntimeInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeInstance").finish_non_exhaustive()
    }
}

// ── RuntimeHost ───────────────────────────────────────────────────────────

/// Capability-gated Wasmtime host.
///
/// Owns a single `Engine`, a `Linker<HostState>` (with the `ail/host_call`
/// stub registered), an in-memory `AuditLog`, a pluggable list of
/// [`Handler`]s, and a schema registry of [`CapabilityDefinition`]s for
/// boundary validation.
///
/// # Handler dispatch
///
/// Handlers are checked in registration order.  The first handler whose
/// [`capabilities()`](Handler::capabilities) list contains the requested
/// [`CapabilityId`] is used.
///
/// # Schema enforcement
///
/// When a [`CapabilityDefinition`] is registered via [`with_capability_definition`],
/// [`call_capability`] validates the input payload against the declared
/// `CapabilityInputSchema` before dispatching, then validates successful
/// handler responses against the declared `CapabilityOutputSchema`.
/// Capabilities without a registered schema are not validated (pass-through).
///
/// # Preflight
///
/// All preflight checks are evaluated inside [`validate_and_instantiate`]
/// before Wasmtime instantiation.  After a successful preflight the
/// profile is stored so `call_capability` can enforce grant checks.
///
/// [`with_capability_definition`]: RuntimeHost::with_capability_definition
/// [`call_capability`]: RuntimeHost::call_capability
/// [`validate_and_instantiate`]: RuntimeHost::validate_and_instantiate
pub struct RuntimeHost {
    engine: Engine,
    linker: Linker<HostState>,
    audit_log: Arc<Mutex<AuditLog>>,
    handlers: Vec<Arc<dyn Handler + Send + Sync>>,
    /// Schema registry: capability ID string → CapabilityDefinition.
    schema_registry: HashMap<String, CapabilityDefinition>,
    /// Stored after a successful `validate_and_instantiate`; used by
    /// `call_capability` to enforce grant checks.
    current_profile: Option<Arc<RuntimeProfile>>,
    /// Module name from the most recent capability manifest (used in reports).
    current_module_name: Option<String>,
    /// Active distributed trace context for host-side capability calls.
    ///
    /// When `Some`, `call_capability` creates a child span derived from this
    /// context and attaches it to the `CapabilityCallExecuted` audit event.
    current_trace_context: Option<TraceContext>,
}

impl RuntimeHost {
    /// Create a new host with default Wasmtime configuration and no handlers.
    ///
    /// A `Linker<HostState>` is constructed and the stub import
    /// `ail/host_call` is registered so that WASM modules declaring this
    /// import can instantiate without error.
    ///
    /// `consume_fuel` is enabled globally on the `Engine` so that per-store
    /// fuel budgets set via [`Store::set_fuel`] are honoured.  When no
    /// `max_fuel` is configured in a profile the store receives no initial
    /// fuel call, meaning fuel tracking is active but the module runs without
    /// a cap (the store starts with 0 fuel by default when consume_fuel is
    /// enabled, so we must only call `set_fuel` when a limit is present).
    pub fn new() -> Self {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).expect("wasmtime Engine::new must succeed");
        let mut linker: Linker<HostState> = Linker::new(&engine);

        // Register host import: module="ail", name="host_call".
        // Signature: (cap_ptr: i32, cap_len: i32, op_ptr: i32, op_len: i32,
        //             args_ptr: i32, args_len: i32) -> i64
        linker
            .func_wrap(
                "ail",
                "host_call",
                |mut caller: wasmtime::Caller<'_, HostState>,
                 cap_ptr: i32,
                 cap_len: i32,
                 op_ptr: i32,
                 op_len: i32,
                 args_ptr: i32,
                 args_len: i32|
                 -> i64 {
                    dispatch_host_call(
                        &mut caller,
                        cap_ptr,
                        cap_len,
                        op_ptr,
                        op_len,
                        args_ptr,
                        args_len,
                    )
                    .unwrap_or(-1)
                },
            )
            .expect("ail/host_call registration must succeed");

        // Register host import: module="ail", name="host_call_write".
        // Signature: (cap_ptr: i32, cap_len: i32, op_ptr: i32, op_len: i32,
        //             args_ptr: i32, args_len: i32, out_ptr: i32, out_max: i32) -> i32
        // Stub returns -1 until dispatch_host_call_write is implemented (TASK-F4).
        linker
            .func_wrap(
                "ail",
                "host_call_write",
                |mut caller: wasmtime::Caller<'_, HostState>,
                 cap_ptr: i32,
                 cap_len: i32,
                 op_ptr: i32,
                 op_len: i32,
                 args_ptr: i32,
                 args_len: i32,
                 out_ptr: i32,
                 out_max: i32|
                 -> i32 {
                    dispatch_host_call_write(
                        &mut caller,
                        cap_ptr,
                        cap_len,
                        op_ptr,
                        op_len,
                        args_ptr,
                        args_len,
                        out_ptr,
                        out_max,
                    )
                    .unwrap_or(-1)
                },
            )
            .expect("ail/host_call_write registration must succeed");

        RuntimeHost {
            engine,
            linker,
            audit_log: Arc::new(Mutex::new(AuditLog::new())),
            handlers: Vec::new(),
            schema_registry: HashMap::new(),
            current_profile: None,
            current_module_name: None,
            current_trace_context: None,
        }
    }

    /// Register a handler (builder pattern).
    ///
    /// Handlers are checked in registration order.  Multiple handlers for
    /// overlapping capability sets are permitted; only the first match is used.
    pub fn with_handler(mut self, handler: Arc<dyn Handler + Send + Sync>) -> Self {
        self.handlers.push(handler);
        self
    }

    /// Register a [`CapabilityDefinition`] for runtime boundary validation.
    ///
    /// When `call_capability` is invoked for a capability with a registered
    /// definition, the input payload is validated against the definition's
    /// `CapabilityInputSchema` before dispatch.
    ///
    /// Capabilities without a registered definition pass through without
    /// schema validation.
    pub fn with_capability_definition(mut self, def: CapabilityDefinition) -> Self {
        self.schema_registry
            .insert(def.capability().as_str().to_string(), def);
        self
    }

    /// Set the active distributed trace context for host-side capability calls.
    ///
    /// When set, `call_capability` creates a **child span** derived from `ctx`
    /// (same `trace_id`, new `span_id`, `parent_span_id` = `ctx.span_id`) and
    /// attaches it to every `CapabilityCallExecuted` audit event.
    ///
    /// Call this before a logical execution unit (e.g. one WASM invocation)
    /// to correlate all capability calls within that unit under the same trace.
    pub fn set_trace_context(&mut self, ctx: TraceContext) {
        self.current_trace_context = Some(ctx);
    }

    /// Return a snapshot of the accumulated audit log.
    ///
    /// Includes events from both host-side (`call_capability`) and WASM-side
    /// (`dispatch_host_call`) dispatches, since both share the same
    /// `Arc<Mutex<AuditLog>>`.
    pub fn audit_log(&self) -> AuditLog {
        self.audit_log
            .lock()
            .expect("audit_log lock must not be poisoned")
            .clone()
    }

    /// Preflight-check and instantiate a WASM module.
    ///
    /// Runs the preflight pipeline (see module doc).  Appends exactly one
    /// [`AuditEvent`] to the internal log regardless of outcome.
    ///
    /// On success, stores the profile internally so `call_capability` can
    /// enforce grant checks.
    #[cfg_attr(
        feature = "otel",
        tracing::instrument(skip_all, name = "runtime.validate_and_instantiate")
    )]
    pub fn validate_and_instantiate(
        &mut self,
        wasm: &[u8],
        manifest: &CapabilityManifest,
        profile: &RuntimeProfile,
    ) -> RuntimeResult<RuntimeInstance> {
        self.validate_and_instantiate_with_packages(wasm, manifest, profile, &[])
    }

    /// Like [`validate_and_instantiate`] but with explicit package manifests.
    ///
    /// Package manifests are checked in Stage 0 before any WASM or capability
    /// checks run.
    pub fn validate_and_instantiate_with_packages(
        &mut self,
        wasm: &[u8],
        manifest: &CapabilityManifest,
        profile: &RuntimeProfile,
        package_manifests: &[PackageManifest],
    ) -> RuntimeResult<RuntimeInstance> {
        let result = self.preflight_inner(wasm, manifest, profile, package_manifests);
        let event = Self::build_audit_event(&result, profile, wasm);
        self.audit_log
            .lock()
            .expect("audit_log lock")
            .push(event);
        if result.is_ok() {
            self.current_profile = Some(Arc::new(profile.clone()));
            self.current_module_name = Some(manifest.module.clone());
        }
        result
    }

    /// Dispatch a capability call to a registered handler.
    ///
    /// Steps:
    /// 1. Check the active profile grants `capability` for the current module; deny if not.
    /// 2. Validate the input payload against the registered schema (if any).
    /// 3. Find the first handler whose `capabilities()` includes `capability`.
    /// 4. Dispatch to `handler.handle(capability, operation, payload)`.
    /// 5. Validate successful handler responses against the output schema.
    /// 6. Append an [`AuditEvent::CapabilityCallExecuted`] event.
    pub fn call_capability(
        &mut self,
        capability: &CapabilityId,
        operation: &str,
        payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        let start = Instant::now();
        let timestamp = unix_timestamp_micros();

        // Derive a child trace span if a context is active.
        let child_trace = self.current_trace_context.as_ref().map(|ctx| ctx.child());

        // Snapshot context fields used in audit events.
        let module_name = self.current_module_name.clone();
        let profile_name = self.current_profile.as_ref().map(|p| p.name().to_string());
        let vr_hash = self
            .current_profile
            .as_ref()
            .map(|p| p.verification_report_hash().to_string());
        let trace_id = child_trace.as_ref().map(|tc| tc.trace_id.clone());
        let input_hash = Some(blake3_hex_of(payload));

        // Step 1: grant check (module-scoped per docs/runtime.md §Grants per profile).
        let module_str = module_name.as_deref().unwrap_or("");
        let granted = self
            .current_profile
            .as_ref()
            .map(|p| p.grants_capability(module_str, capability))
            .unwrap_or(false);

        if !granted {
            let err = HostError::CapabilityDenied(capability.as_str().to_string());
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log
                .lock()
                .expect("audit_log lock")
                .push(AuditEvent::CapabilityCallExecuted {
                    capability: capability.clone(),
                    operation: operation.to_string(),
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us,
                    timestamp,
                    profile: profile_name,
                    module: module_name,
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                });
            return Err(err);
        }

        // Step 2: schema/boundary validation (if a definition is registered).
        if let Some(def) = self.schema_registry.get(capability.as_str()) {
            if let Err(schema_err) = def.schema().input().validate(payload) {
                let err = HostError::PayloadDecodeError(format!(
                    "schema validation failed for `{}`: {}",
                    capability.as_str(),
                    schema_err.message
                ));
                let duration_us = start.elapsed().as_micros() as u64;
                self.audit_log
                    .lock()
                    .expect("audit_log lock")
                    .push(AuditEvent::CapabilityCallExecuted {
                        capability: capability.clone(),
                        operation: operation.to_string(),
                        handler_name: "none".to_string(),
                        succeeded: false,
                        duration_us,
                        timestamp,
                        profile: profile_name,
                        module: module_name,
                        function: None,
                        input_hash,
                        output_hash: None,
                        trace_id,
                        verification_report_hash: vr_hash,
                        trace_context: child_trace,
                    });
                return Err(err);
            }
        }

        // Step 3: find matching handler.
        let handler = self
            .handlers
            .iter()
            .find(|h| h.capabilities().contains(capability));

        let Some(handler) = handler else {
            let err = HostError::HandlerNotBound(capability.as_str().to_string());
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log
                .lock()
                .expect("audit_log lock")
                .push(AuditEvent::CapabilityCallExecuted {
                    capability: capability.clone(),
                    operation: operation.to_string(),
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us,
                    timestamp,
                    profile: profile_name,
                    module: module_name,
                    function: None,
                    input_hash,
                    output_hash: None,
                    trace_id,
                    verification_report_hash: vr_hash,
                    trace_context: child_trace,
                });
            return Err(err);
        };

        let handler_name = handler.name().to_string();

        // Step 4: dispatch.
        let result = match handler.handle(capability, operation, payload) {
            Ok(response) => {
                // Step 5: output schema/boundary validation (if registered).
                if let Some(def) = self.schema_registry.get(capability.as_str())
                    && let Err(schema_err) = def.schema().output().validate(&response)
                {
                    Err(HostError::ContractViolation(format!(
                        "schema validation failed for `{}` response: {}",
                        capability.as_str(),
                        schema_err.message
                    )))
                } else {
                    Ok(response)
                }
            }
            Err(err) => Err(err),
        };
        let duration_us = start.elapsed().as_micros() as u64;
        let succeeded = result.is_ok();
        let output_hash = result
            .as_ref()
            .ok()
            .map(|bytes| blake3_hex_of(bytes.as_slice()));

        // Step 5: audit.
        self.audit_log
            .lock()
            .expect("audit_log lock")
            .push(AuditEvent::CapabilityCallExecuted {
                capability: capability.clone(),
                operation: operation.to_string(),
                handler_name,
                succeeded,
                duration_us,
                timestamp,
                profile: profile_name,
                module: module_name,
                function: None,
                input_hash,
                output_hash,
                trace_id,
                verification_report_hash: vr_hash,
                trace_context: child_trace,
            });

        result
    }

    /// Execute a closure within a transaction group, committing on success and
    /// rolling back on failure.
    ///
    /// The closure receives `&mut RuntimeHost` for capability dispatch.
    /// - On `Ok(_)`: `tx.commit()` is called and the result is returned.
    /// - On `Err(_)`: `tx.rollback()` is called and the error is returned.
    ///
    /// Use [`execute_with_rollback_detail`] to also receive the list of
    /// non-rollbackable capability IDs on failure.
    ///
    /// [`execute_with_rollback_detail`]: RuntimeHost::execute_with_rollback_detail
    pub fn execute_with_rollback<F, T>(&mut self, tx: &mut TransactionGroup, f: F) -> HostResult<T>
    where
        F: FnOnce(&mut RuntimeHost) -> HostResult<T>,
    {
        let result = f(self);
        match result {
            Ok(val) => {
                tx.commit();
                Ok(val)
            }
            Err(err) => {
                tx.rollback();
                Err(err)
            }
        }
    }

    /// Execute a closure within a transaction group, committing on success and
    /// rolling back on failure.
    ///
    /// Returns `(result, non_rollbackable)` where `non_rollbackable` is the
    /// list of [`CapabilityId`]s that could not be rolled back automatically
    /// (i.e. those with [`TransactionPolicy::NonRollbackable`]).
    ///
    /// On success, `non_rollbackable` is always empty.
    ///
    /// [`TransactionPolicy::NonRollbackable`]: crate::transaction::TransactionPolicy::NonRollbackable
    pub fn execute_with_rollback_detail<F, T>(
        &mut self,
        tx: &mut TransactionGroup,
        f: F,
    ) -> (HostResult<T>, Vec<CapabilityId>)
    where
        F: FnOnce(&mut RuntimeHost) -> HostResult<T>,
    {
        let result = f(self);
        match result {
            Ok(val) => {
                tx.commit();
                (Ok(val), vec![])
            }
            Err(err) => {
                let non_rollbackable = tx.rollback();
                (Err(err), non_rollbackable)
            }
        }
    }

    /// Emit a [`RuntimeReport`] summarising the current execution.
    ///
    /// Aggregates capability call statistics from the audit log and populates
    /// all report fields required by docs/runtime.md §"Runtime report":
    /// - module_name from the most recently instantiated manifest
    /// - verification_report_hash from the active profile
    /// - capability_summaries from CapabilityCallExecuted audit events
    /// - audit_log_hash from BLAKE3(serialized audit events)
    ///
    /// Must be called after [`validate_and_instantiate`] has stored the
    /// active profile; if no profile is stored yet, profile fields are empty.
    ///
    /// `status` — the caller-supplied execution outcome.
    /// `id` — caller-supplied report identifier (e.g. a trace ID).
    ///
    /// [`validate_and_instantiate`]: RuntimeHost::validate_and_instantiate
    pub fn emit_report(&self, status: RuntimeReportStatus, id: impl Into<String>) -> RuntimeReport {
        let (profile_name, module_hash, verification_report_hash) = self
            .current_profile
            .as_ref()
            .map(|p| {
                (
                    p.name().to_string(),
                    p.module_hash().to_string(),
                    p.verification_report_hash().to_string(),
                )
            })
            .unwrap_or_default();

        let module_name = self.current_module_name.clone().unwrap_or_default();

        // Build per-capability summaries from CapabilityCallExecuted events.
        let mut totals: HashMap<String, (u32, u32, u32)> = HashMap::new(); // cap → (total, ok, err)

        let log_snapshot = self
            .audit_log
            .lock()
            .expect("audit_log lock")
            .clone();

        for event in log_snapshot.events() {
            if let AuditEvent::CapabilityCallExecuted {
                capability,
                succeeded,
                ..
            } = event
            {
                let entry = totals.entry(capability.as_str().to_string()).or_default();
                entry.0 += 1;
                if *succeeded {
                    entry.1 += 1;
                } else {
                    entry.2 += 1;
                }
            }
        }

        let summaries: Vec<CapabilityCallSummary> = totals
            .into_iter()
            .map(|(cap_str, (total, ok, err))| CapabilityCallSummary {
                capability: CapabilityId::new(cap_str),
                total_calls: total,
                succeeded: ok,
                failed: err,
            })
            .collect();

        // Compute audit log hash: BLAKE3 over the concatenation of all event
        // debug representations (stable, deterministic for the same event set).
        let audit_log_hash = if !log_snapshot.is_empty() {
            let serialized: String = log_snapshot
                .events()
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join("|");
            Some(blake3_hex_of(serialized.as_bytes()))
        } else {
            None
        };

        let report = RuntimeReport::new(id.into(), profile_name, module_name, module_hash, status)
            .with_verification_report_hash(verification_report_hash)
            .with_summaries(summaries);

        let report = if let Some(hash) = audit_log_hash {
            report.with_audit_log_hash(hash)
        } else {
            report
        };

        report
    }

    // ── private ──────────────────────────────────────────────────────────

    fn build_audit_event(
        result: &RuntimeResult<RuntimeInstance>,
        profile: &RuntimeProfile,
        wasm: &[u8],
    ) -> AuditEvent {
        match result {
            Ok(_) => AuditEvent::PreflightPassed {
                profile_name: profile.name().to_string(),
                module_hash: blake3_hex_of(wasm),
            },
            Err(err) => {
                let (denied, reason) = failure_parts(err);
                AuditEvent::PreflightFailed {
                    profile_name: profile.name().to_string(),
                    denied,
                    reason,
                }
            }
        }
    }

    fn preflight_inner(
        &self,
        wasm: &[u8],
        manifest: &CapabilityManifest,
        profile: &RuntimeProfile,
        package_manifests: &[PackageManifest],
    ) -> RuntimeResult<RuntimeInstance> {
        // Stage 0 — Package trust gate.
        if !package_manifests.is_empty() {
            Self::check_package_trust(package_manifests, profile)?;
        }

        // Stage 1 — WASM bytes hash check.
        let actual_module_hash = blake3_hex_of(wasm);
        if actual_module_hash != profile.module_hash() {
            return Err(RuntimeError::PreflightFailed(
                PreflightFailure::HashMismatch {
                    expected: profile.module_hash().to_string(),
                    actual: actual_module_hash,
                },
            ));
        }

        // Stage 2 — Manifest CBOR hash check.
        let actual_manifest_hash = manifest.blake3_hex().map_err(RuntimeError::EncodingError)?;
        if actual_manifest_hash != profile.capability_manifest_hash() {
            return Err(RuntimeError::PreflightFailed(
                PreflightFailure::HashMismatch {
                    expected: profile.capability_manifest_hash().to_string(),
                    actual: actual_manifest_hash,
                },
            ));
        }

        // Stage 3 — Capability grant check (module-scoped).
        let denied: Vec<CapabilityId> = manifest
            .requires
            .iter()
            .filter(|cap| !profile.grants_capability(&manifest.module, cap))
            .cloned()
            .collect();
        if !denied.is_empty() {
            return Err(RuntimeError::PreflightFailed(
                PreflightFailure::CapabilityDenied { denied },
            ));
        }

        // Stages 4+5 — Wasmtime validate + instantiate via linker.
        let instance = self.instantiate_inner(wasm, profile, &manifest.module)?;

        // Stage 6 — Handler binding check (opt-in).
        if profile.require_handler_binding() {
            for grant in profile.grants() {
                let bound = self
                    .handlers
                    .iter()
                    .any(|h| h.capabilities().contains(&grant.capability));
                if !bound {
                    return Err(RuntimeError::PreflightFailed(
                        PreflightFailure::HandlerNotBound {
                            capability: grant.capability.clone(),
                        },
                    ));
                }
            }
        }

        Ok(instance)
    }

    fn check_package_trust(
        manifests: &[PackageManifest],
        profile: &RuntimeProfile,
    ) -> RuntimeResult<()> {
        for m in manifests {
            if m.trust_level == TrustLevel::Unsafe {
                return Err(RuntimeError::PreflightFailed(
                    PreflightFailure::UnsafePackageNotApproved {
                        package: m.name.clone(),
                    },
                ));
            }

            if let Some(required) = profile.min_package_trust()
                && !m.trust_level.satisfies(required)
            {
                return Err(RuntimeError::PreflightFailed(
                    PreflightFailure::PackageTrustViolation {
                        package: m.name.clone(),
                        required,
                        actual: m.trust_level,
                    },
                ));
            }
        }
        Ok(())
    }

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
    fn instantiate_inner(
        &self,
        wasm: &[u8],
        profile: &RuntimeProfile,
        module_name: &str,
    ) -> RuntimeResult<RuntimeInstance> {
        Module::validate(&self.engine, wasm).map_err(|e| {
            RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
        })?;

        let module = Module::new(&self.engine, wasm).map_err(|e| {
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

        let handlers_arc: Arc<Vec<Arc<dyn Handler + Send + Sync>>> =
            Arc::new(self.handlers.clone());
        let mut store = Store::new(
            &self.engine,
            HostState {
                handlers: handlers_arc,
                profile: Arc::new(profile.clone()),
                module_name: module_name.to_string(),
                audit_log: self.audit_log.clone(),
                limiter: store_limits,
                trace_context: None,
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

        let instance = self
            .linker
            .instantiate(&mut store, &module)
            .map_err(Self::classify_instantiate_error)?;

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
}

impl Default for RuntimeHost {
    fn default() -> Self {
        Self::new()
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Return the current Unix timestamp in microseconds.
///
/// Used to stamp [`AuditEvent::CapabilityCallExecuted`] events.
/// Falls back to 0 if the system clock is before the Unix epoch (pathological).
fn unix_timestamp_micros() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

fn failure_parts(err: &RuntimeError) -> (Vec<CapabilityId>, PreflightFailure) {
    match err {
        RuntimeError::PreflightFailed(PreflightFailure::CapabilityDenied { denied }) => (
            denied.clone(),
            PreflightFailure::CapabilityDenied {
                denied: denied.clone(),
            },
        ),
        RuntimeError::PreflightFailed(
            PreflightFailure::PackageTrustViolation { .. }
            | PreflightFailure::UnsafePackageNotApproved { .. }
            | PreflightFailure::HashMismatch { .. }
            | PreflightFailure::WasmValidationError(_)
            | PreflightFailure::HandlerNotBound { .. }
            | PreflightFailure::ResourceLimitExceeded { .. },
        ) => {
            let failure = match err {
                RuntimeError::PreflightFailed(f) => f.clone(),
                _ => unreachable!(),
            };
            (vec![], failure)
        }
        RuntimeError::EncodingError(msg) => (
            vec![],
            PreflightFailure::WasmValidationError(format!("encoding: {msg}")),
        ),
        RuntimeError::CapabilityCallFailed(_) => (
            vec![],
            PreflightFailure::WasmValidationError(
                "unexpected capability call failure in preflight".to_string(),
            ),
        ),
    }
}

fn read_memory(
    caller: &mut wasmtime::Caller<'_, HostState>,
    ptr: i32,
    len: i32,
) -> Option<Vec<u8>> {
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
fn dispatch_host_call_write(
    caller: &mut wasmtime::Caller<'_, HostState>,
    cap_ptr: i32,
    cap_len: i32,
    op_ptr: i32,
    op_len: i32,
    args_ptr: i32,
    args_len: i32,
    out_ptr: i32,
    out_max: i32,
) -> Option<i32> {
    // Validate output buffer params.
    if out_ptr < 0 || out_max < 0 {
        return None;
    }

    // Read capability name, operation name, and args bytes from WASM memory.
    let capability = String::from_utf8(read_memory(caller, cap_ptr, cap_len)?).ok()?;
    let operation = String::from_utf8(read_memory(caller, op_ptr, op_len)?).ok()?;
    let args_bytes = read_memory(caller, args_ptr, args_len.checked_mul(8)?)?;
    let cap = CapabilityId::new(capability);

    // Grant check (module-scoped).
    {
        let state = caller.data();
        if !state.profile.grants_capability(&state.module_name, &cap) {
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
    let handler = handler?;

    // Dispatch.
    let result = handler.handle(&cap, &operation, &args_bytes);
    let response = result.ok()?;

    // Bounds-check: response must fit in the out buffer.
    if response.len() > out_max as usize {
        return None;
    }

    // Write response bytes to WASM memory at out_ptr.
    let memory = caller.get_export("memory")?.into_memory()?;
    memory
        .write(caller, out_ptr as usize, &response)
        .ok()?;

    Some(response.len() as i32)
}

fn dispatch_host_call(
    caller: &mut wasmtime::Caller<'_, HostState>,
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
                });
            return Some(-1);
        }
        state
            .handlers
            .iter()
            .find(|h| h.capabilities().contains(&cap))
            .cloned()
    };

    let Some(handler) = handler else {
        caller
            .data_mut()
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
            });
        return Some(-1);
    };

    let handler_name = handler.name().to_string();
    let result = handler.handle(&cap, &operation, &args_bytes);
    let succeeded = result.is_ok();
    let output_hash = result
        .as_ref()
        .ok()
        .map(|bytes| blake3_hex_of(bytes.as_slice()));
    caller
        .data_mut()
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
        });

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
