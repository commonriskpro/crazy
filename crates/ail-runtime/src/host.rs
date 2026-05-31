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
//   Both paths append one redacted AuditEvent::TransactionLifecycle event.
//   `execute_with_rollback_detail` additionally returns the non-rollbackable
//   capability IDs on failure.
//
// Wasmtime Linker:
//   A `Linker<HostState>` is constructed once at `RuntimeHost::new` and
//   registers two host imports: `ail/host_call` (returns i64 result value)
//   and `ail/host_call_write` (writes response bytes into a WASM out-buffer,
//   returns bytes-written as i32).  Both are implemented in `host_dispatch`.
//   `Store<HostState>` carries an `Arc<HandlerRegistry>` so host-function
//   closures can dispatch to registered handlers.  The `Store` data type
//   changed from `()` to `HostState` — all existing tests are unaffected
//   because the Linker satisfies WASM modules that have no host imports.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use wasmtime::{Config, Engine, Linker};

use ail_package::manifest::PackageManifest;

use crate::abi::{HostError, HostResult};
use crate::audit::{
    AuditEvent, AuditLog, DENIAL_CATEGORY_CAPABILITY_AMBIENT_ACCESS,
    DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED, DENIAL_CATEGORY_CAPABILITY_PROFILE_MISMATCH,
    DENIAL_CATEGORY_CAPABILITY_REVOKED, DENIAL_CATEGORY_HANDLER_NOT_BOUND,
    DENIAL_CATEGORY_LIMIT_CONCURRENCY, DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS,
    DENIAL_CATEGORY_LIMIT_OUTPUT_SIZE, DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE,
    DENIAL_CATEGORY_LIMIT_RATE, DENIAL_CATEGORY_LIMIT_RECURSION_DEPTH,
    DENIAL_CATEGORY_SCHEMA_INPUT, DENIAL_CATEGORY_SCHEMA_OUTPUT, denial_category,
};
use crate::codec::ValueLayout;
use crate::error::RuntimeResult;
use crate::handler::Handler;
use crate::host_dispatch::{
    ClockFn, HostState, WasmBridgeDiagnostic, default_clock_fn, diagnose_wasm_bridge_module,
    dispatch_host_call, dispatch_host_call_write, unix_timestamp_micros,
};
use crate::manifest::{CapabilityManifest, blake3_hex_of};
use crate::profile::{CapabilityId, CapabilityRevocationRegistry, InFlightPolicy, RuntimeProfile};
use crate::report::{CapabilityCallSummary, RuntimeReport, RuntimeReportStatus};
use crate::schema::CapabilityDefinition;
use crate::transaction::{TransactionAuditRecord, TransactionGroup};

// Re-export public types that originate in sub-modules.
pub use crate::host_dispatch::{RuntimeArg, RuntimeInstance, RuntimeValue, TraceContext};

fn transaction_lifecycle_event(record: TransactionAuditRecord) -> AuditEvent {
    AuditEvent::TransactionLifecycle {
        group_name_shape: record.group_name_shape,
        action: record.action.to_string(),
        category: record.category.to_string(),
        status_before: record.status_before.to_string(),
        status_after: record.status_after.to_string(),
        entry_count: record.entry_count,
        non_rollbackable_count: record.non_rollbackable_count,
        compensation_required_count: record.compensation_required_count,
    }
}

fn capability_denial_category_for_access(
    profile: Option<&RuntimeProfile>,
    module: Option<&str>,
    capability: &CapabilityId,
) -> &'static str {
    let Some(profile) = profile else {
        return DENIAL_CATEGORY_CAPABILITY_AMBIENT_ACCESS;
    };
    let Some(module) = module.filter(|module| !module.is_empty()) else {
        return DENIAL_CATEGORY_CAPABILITY_AMBIENT_ACCESS;
    };
    if profile.grants_capability_to_any_module(capability) {
        DENIAL_CATEGORY_CAPABILITY_PROFILE_MISMATCH
    } else {
        DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED
    }
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
/// `CapabilityInputSchema` before dispatch, then validates successful
/// handler responses against the declared `CapabilityOutputSchema`.
/// Capabilities without a registered definition are not validated (pass-through).
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
    /// Capability calls consumed since the last successful preflight.
    capability_calls_used: u64,
    /// Runtime-mutable revocations for active profile grants.
    revocations: CapabilityRevocationRegistry,
    /// Injectable clock for rate limit window tracking (nanoseconds since epoch).
    ///
    /// Replaced via [`with_clock_fn`] for deterministic testing.
    ///
    /// [`with_clock_fn`]: RuntimeHost::with_clock_fn
    clock_fn: ClockFn,
    /// Fixed-window call counters for `rate_limits` enforcement on the
    /// host-side `call_capability` path (same semantics as `HostState`).
    rate_limit_windows: HashMap<Option<String>, (u64, u64)>,
    /// In-flight concurrent calls on the host-side `call_capability` path.
    concurrent_calls: u64,
    /// Host-call recursion depth on the host-side `call_capability` path.
    call_depth: u64,
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
        // Writes the handler response into WASM memory at out_ptr; returns bytes
        // written on success or -1 on denial, revocation, limit exceeded, or error.
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
            capability_calls_used: 0,
            revocations: CapabilityRevocationRegistry::new(),
            clock_fn: default_clock_fn(),
            rate_limit_windows: HashMap::new(),
            concurrent_calls: 0,
            call_depth: 0,
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

    /// Install an existing revocation registry for this host.
    pub fn with_revocation_registry(mut self, registry: CapabilityRevocationRegistry) -> Self {
        self.revocations = registry;
        self
    }

    /// Replace the clock used for rate limit window tracking.
    ///
    /// The clock must return nanoseconds since Unix epoch.  The default is the
    /// system wall clock.  Inject a controllable counter in tests to make rate
    /// limit assertions fully deterministic without any `sleep` calls.
    ///
    /// # Example
    /// ```rust,ignore
    /// use std::sync::Arc;
    /// use std::sync::atomic::{AtomicU64, Ordering};
    ///
    /// let clock = Arc::new(AtomicU64::new(0));
    /// let clock_fn = { let c = clock.clone(); Arc::new(move || c.load(Ordering::SeqCst)) };
    /// let host = RuntimeHost::new().with_clock_fn(clock_fn);
    /// ```
    pub fn with_clock_fn(mut self, clock: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.clock_fn = clock;
        self
    }

    /// Revoke a capability grant for subsequent calls on this host.
    pub fn revoke_capability(
        &mut self,
        module: impl Into<String>,
        capability: impl Into<String>,
        profile: impl Into<String>,
        in_flight_policy: InFlightPolicy,
    ) {
        self.revocations
            .revoke(module, capability, profile, in_flight_policy);
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

    /// Return stable redacted diagnostics for WASM bridge/module issues.
    ///
    /// This check is additive and does not mutate runtime state.  It validates
    /// the module enough to classify malformed bytes, missing runtime imports,
    /// and known `ail/*` host import ABI mismatches with deterministic ordering.
    pub fn wasm_bridge_diagnostics(&self, wasm: &[u8]) -> Vec<WasmBridgeDiagnostic> {
        diagnose_wasm_bridge_module(&self.engine, wasm)
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
        let result = crate::host_preflight::preflight_inner(
            &self.engine,
            &self.linker,
            &self.handlers,
            &self.revocations,
            &self.audit_log,
            wasm,
            manifest,
            profile,
            package_manifests,
            self.clock_fn.clone(),
        );
        let event = crate::host_preflight::build_audit_event(&result, profile, wasm);
        self.audit_log.lock().expect("audit_log lock").push(event);
        if result.is_ok() {
            self.current_profile = Some(Arc::new(profile.clone()));
            self.current_module_name = Some(manifest.module.clone());
            self.capability_calls_used = 0;
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
            let category = capability_denial_category_for_access(
                self.current_profile.as_deref(),
                module_name.as_deref(),
                capability,
            );
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
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
                    denial_category: denial_category(category),
                },
            );
            return Err(err);
        }

        let revoked = self
            .current_profile
            .as_ref()
            .map(|p| {
                self.revocations
                    .is_revoked(module_str, capability.as_str(), p.name())
            })
            .unwrap_or(false);

        if revoked {
            let err = HostError::CapabilityDenied(capability.as_str().to_string());
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
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
                    denial_category: denial_category(DENIAL_CATEGORY_CAPABILITY_REVOKED),
                },
            );
            return Err(err);
        }

        if let Some(max_payload_bytes) = self
            .current_profile
            .as_ref()
            .and_then(|p| p.limits().payload_size_limit)
            && payload.len() as u64 > max_payload_bytes
        {
            let err = HostError::LimitExceeded(format!(
                "payload_size_limit exceeded: limit={max_payload_bytes}, actual={}",
                payload.len()
            ));
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
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
                    denial_category: denial_category(DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE),
                },
            );
            return Err(err);
        }

        if let Some(max_calls) = self
            .current_profile
            .as_ref()
            .and_then(|p| p.limits().max_capability_calls)
            && self.capability_calls_used >= max_calls
        {
            let err = HostError::LimitExceeded(format!(
                "max_capability_calls exceeded: limit={max_calls}, used={}",
                self.capability_calls_used
            ));
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
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
                    denial_category: denial_category(DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS),
                },
            );
            return Err(err);
        }

        // Rate limit enforcement.
        {
            let rate_limits_vec = self
                .current_profile
                .as_ref()
                .and_then(|p| p.limits().rate_limits.clone())
                .unwrap_or_default();
            if !crate::host_dispatch::check_rate_limits(
                &rate_limits_vec,
                &self.clock_fn,
                &mut self.rate_limit_windows,
                capability,
            ) {
                let err = HostError::LimitExceeded(format!(
                    "rate_limit exceeded for capability `{}`",
                    capability.as_str()
                ));
                let duration_us = start.elapsed().as_micros() as u64;
                self.audit_log.lock().expect("audit_log lock").push(
                    AuditEvent::CapabilityCallExecuted {
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
                        denial_category: denial_category(DENIAL_CATEGORY_LIMIT_RATE),
                    },
                );
                return Err(err);
            }
        }

        // Concurrency limit enforcement.
        if let Some(max_concurrent) = self
            .current_profile
            .as_ref()
            .and_then(|p| p.limits().concurrency_limit)
            && self.concurrent_calls >= max_concurrent
        {
            let err = HostError::LimitExceeded(format!(
                "concurrency_limit exceeded: limit={max_concurrent}, in_flight={}",
                self.concurrent_calls
            ));
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
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
                    denial_category: denial_category(DENIAL_CATEGORY_LIMIT_CONCURRENCY),
                },
            );
            return Err(err);
        }

        // Recursion stack (call depth) limit enforcement.
        if let Some(max_depth) = self
            .current_profile
            .as_ref()
            .and_then(|p| p.limits().recursion_stack_limit)
            && self.call_depth >= max_depth
        {
            let err = HostError::LimitExceeded(format!(
                "recursion_stack_limit exceeded: limit={max_depth}, depth={}",
                self.call_depth
            ));
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
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
                    denial_category: denial_category(DENIAL_CATEGORY_LIMIT_RECURSION_DEPTH),
                },
            );
            return Err(err);
        }

        self.capability_calls_used += 1;
        self.concurrent_calls += 1;
        self.call_depth += 1;

        // Step 2: schema/boundary validation (if a definition is registered).
        if let Some(def) = self.schema_registry.get(capability.as_str())
            && let Err(schema_err) = def.schema().input().validate(payload)
        {
            self.concurrent_calls -= 1;
            self.call_depth -= 1;
            let err = HostError::PayloadDecodeError(format!(
                "schema validation failed for `{}`: {}",
                capability.as_str(),
                schema_err.message
            ));
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
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
                    denial_category: denial_category(DENIAL_CATEGORY_SCHEMA_INPUT),
                },
            );
            return Err(err);
        }

        // Step 3: find matching handler.
        let handler = self
            .handlers
            .iter()
            .find(|h| h.capabilities().contains(capability));

        let Some(handler) = handler else {
            self.concurrent_calls -= 1;
            self.call_depth -= 1;
            let err = HostError::HandlerNotBound(capability.as_str().to_string());
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.lock().expect("audit_log lock").push(
                AuditEvent::CapabilityCallExecuted {
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
                    denial_category: denial_category(DENIAL_CATEGORY_HANDLER_NOT_BOUND),
                },
            );
            return Err(err);
        };

        let handler_name = handler.name().to_string();

        // Step 4: dispatch.
        let (result, runtime_denial_category) = match handler.handle(capability, operation, payload)
        {
            Ok(response) => {
                if let Some(max_output_bytes) = self
                    .current_profile
                    .as_ref()
                    .and_then(|p| p.limits().output_size_limit)
                    && response.len() as u64 > max_output_bytes
                {
                    (
                        Err(HostError::LimitExceeded(format!(
                            "output_size_limit exceeded: limit={max_output_bytes}"
                        ))),
                        denial_category(DENIAL_CATEGORY_LIMIT_OUTPUT_SIZE),
                    )
                // Step 5: output schema/boundary validation (if registered).
                //
                // Branch on declared_value_layout() to pick the correct validator:
                //   - Bytes layout → validate_bytes_response(): shape-only check;
                //     byte content is never read or logged here.
                //   - All other layouts (Text, Scalar, Handle, None/multi-field) →
                //     validate(): text key=value boundary protocol.
                } else if let Some(def) = self.schema_registry.get(capability.as_str()) {
                    let output_schema = def.schema().output();
                    if output_schema.declared_value_layout() == Some(ValueLayout::Bytes) {
                        match output_schema.validate_bytes_response(&response) {
                            Ok(_) => (Ok(response), None),
                            Err(schema_err) => (
                                Err(HostError::ContractViolation(format!(
                                    "schema validation failed for `{}` response: {}",
                                    capability.as_str(),
                                    schema_err.message
                                ))),
                                denial_category(DENIAL_CATEGORY_SCHEMA_OUTPUT),
                            ),
                        }
                    } else {
                        match output_schema.validate(&response) {
                            Ok(()) => (Ok(response), None),
                            Err(schema_err) => (
                                Err(HostError::ContractViolation(format!(
                                    "schema validation failed for `{}` response: {}",
                                    capability.as_str(),
                                    schema_err.message
                                ))),
                                denial_category(DENIAL_CATEGORY_SCHEMA_OUTPUT),
                            ),
                        }
                    }
                } else {
                    (Ok(response), None)
                }
            }
            Err(err) => (Err(err), None),
        };

        // Extract the generic audit category BEFORE converting the error to
        // opaque form.  The category is written only to the audit log; callers
        // always receive a plain `CapabilityDenied` (no category exposed).
        let denial_category = result
            .as_ref()
            .err()
            .and_then(|e| e.audit_category())
            .map(|s| s.to_string())
            .or(runtime_denial_category);
        // Convert any `CapabilityDeniedCategorized` to `CapabilityDenied` so
        // the audit category is never returned to the caller.
        let result = result.map_err(|e| e.into_opaque_denial());

        let duration_us = start.elapsed().as_micros() as u64;
        let succeeded = result.is_ok();
        let output_hash = result
            .as_ref()
            .ok()
            .map(|bytes| blake3_hex_of(bytes.as_slice()));

        // Decrement in-flight counters before audit to keep state consistent.
        self.concurrent_calls -= 1;
        self.call_depth -= 1;

        // Step 6: audit (denial_category carries opaque handler failure reason).
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
                denial_category,
            });

        result
    }

    /// Execute a closure within a transaction group, committing on success and
    /// rolling back on failure.
    ///
    /// The closure receives `&mut RuntimeHost` for capability dispatch.
    /// - On `Ok(_)`: `tx.commit_with_audit()` is called and the result is returned.
    /// - On `Err(_)`: `tx.rollback_with_audit()` is called and the error is returned.
    ///
    /// Both paths append one redacted [`AuditEvent::TransactionLifecycle`] event.
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
                let audit = tx.commit_with_audit();
                self.audit_log
                    .lock()
                    .expect("audit_log lock")
                    .push(transaction_lifecycle_event(audit));
                Ok(val)
            }
            Err(err) => {
                let (_, audit) = tx.rollback_with_audit();
                self.audit_log
                    .lock()
                    .expect("audit_log lock")
                    .push(transaction_lifecycle_event(audit));
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
    /// Both success and failure append one redacted
    /// [`AuditEvent::TransactionLifecycle`] event.
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
                let audit = tx.commit_with_audit();
                self.audit_log
                    .lock()
                    .expect("audit_log lock")
                    .push(transaction_lifecycle_event(audit));
                (Ok(val), vec![])
            }
            Err(err) => {
                let (non_rollbackable, audit) = tx.rollback_with_audit();
                self.audit_log
                    .lock()
                    .expect("audit_log lock")
                    .push(transaction_lifecycle_event(audit));
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

        let log_snapshot = self.audit_log.lock().expect("audit_log lock").clone();

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

        let mut summaries: Vec<CapabilityCallSummary> = totals
            .into_iter()
            .map(|(cap_str, (total, ok, err))| CapabilityCallSummary {
                capability: CapabilityId::new(cap_str),
                total_calls: total,
                succeeded: ok,
                failed: err,
            })
            .collect();
        // Sort by capability name for deterministic output order.
        summaries.sort_by(|a, b| a.capability.as_str().cmp(b.capability.as_str()));

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

        if let Some(hash) = audit_log_hash {
            report.with_audit_log_hash(hash)
        } else {
            report
        }
    }
}

impl Default for RuntimeHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::audit::{
        AuditEvent, DENIAL_CATEGORY_CAPABILITY_AMBIENT_ACCESS,
        DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED, DENIAL_CATEGORY_CAPABILITY_PROFILE_MISMATCH,
        DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS, DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE,
    };
    use crate::handler::InMemoryHandler;
    use crate::manifest::CapabilityManifest;
    use crate::profile::{CapabilityGrant, ResourceLimits, RuntimeProfile};

    use super::*;

    fn minimal_wasm() -> Vec<u8> {
        vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
    }

    fn matching_profile_with_limits(
        wasm: &[u8],
        manifest: &CapabilityManifest,
        grant: CapabilityId,
        limits: ResourceLimits,
    ) -> RuntimeProfile {
        RuntimeProfile::new(
            "audit-category-profile".to_string(),
            blake3_hex_of(wasm),
            "a".repeat(64),
            manifest.blake3_hex().expect("manifest hash must succeed"),
            vec![CapabilityGrant {
                module: manifest.module.clone(),
                capability: grant,
            }],
            limits,
        )
    }

    fn instantiate_for_capability(cap: CapabilityId, limits: ResourceLimits) -> RuntimeHost {
        let wasm = minimal_wasm();
        let manifest = CapabilityManifest {
            module: "audit-category-test".to_string(),
            requires: vec![cap.clone()],
        };
        let profile = matching_profile_with_limits(&wasm, &manifest, cap.clone(), limits);
        let handler = Arc::new(InMemoryHandler::new(
            "audit-category-handler",
            vec![cap],
            b"ok".to_vec(),
        ));
        let mut host = RuntimeHost::new().with_handler(handler);
        host.validate_and_instantiate(&wasm, &manifest, &profile)
            .expect("preflight must pass");
        host
    }

    fn last_denial_category(host: &RuntimeHost) -> Option<String> {
        match host.audit_log().events().last() {
            Some(AuditEvent::CapabilityCallExecuted {
                denial_category, ..
            }) => denial_category.clone(),
            other => panic!("expected capability audit event, got {other:?}"),
        }
    }

    #[test]
    fn ungranted_direct_capability_call_records_stable_denial_category() {
        let granted = CapabilityId::new("audit.granted");
        let denied = CapabilityId::new("audit.denied");
        let mut host = instantiate_for_capability(granted, ResourceLimits::default());

        let result = host.call_capability(&denied, "op", b"");

        assert!(result.is_err(), "ungranted capability must be denied");
        assert_eq!(
            last_denial_category(&host).as_deref(),
            Some(DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED)
        );
    }

    #[test]
    fn ambient_direct_capability_call_records_stable_denial_category() {
        let mut host = RuntimeHost::new();
        let result = host.call_capability(&CapabilityId::new("audit.ambient"), "op", b"");

        assert!(result.is_err(), "ambient capability call must be denied");
        assert_eq!(
            last_denial_category(&host).as_deref(),
            Some(DENIAL_CATEGORY_CAPABILITY_AMBIENT_ACCESS)
        );
    }

    #[test]
    fn profile_mismatch_direct_capability_call_records_stable_denial_category() {
        let wasm = minimal_wasm();
        let granted = CapabilityId::new("audit.granted");
        let mismatched = CapabilityId::new("audit.mismatched");
        let manifest = CapabilityManifest {
            module: "active-module".to_string(),
            requires: vec![granted.clone()],
        };
        let profile = RuntimeProfile::new(
            "audit-category-profile".to_string(),
            blake3_hex_of(&wasm),
            "a".repeat(64),
            manifest.blake3_hex().expect("manifest hash must succeed"),
            vec![
                CapabilityGrant {
                    module: manifest.module.clone(),
                    capability: granted.clone(),
                },
                CapabilityGrant {
                    module: "other-module".to_string(),
                    capability: mismatched.clone(),
                },
            ],
            ResourceLimits::default(),
        );
        let handler = Arc::new(InMemoryHandler::new(
            "audit-category-handler",
            vec![granted, mismatched.clone()],
            b"ok".to_vec(),
        ));
        let mut host = RuntimeHost::new().with_handler(handler);
        host.validate_and_instantiate(&wasm, &manifest, &profile)
            .expect("preflight must pass");

        let result = host.call_capability(&mismatched, "op", b"");

        assert!(
            result.is_err(),
            "profile-mismatched capability must be denied"
        );
        assert_eq!(
            last_denial_category(&host).as_deref(),
            Some(DENIAL_CATEGORY_CAPABILITY_PROFILE_MISMATCH)
        );
    }

    #[test]
    fn payload_limit_direct_capability_call_records_stable_denial_category() {
        let cap = CapabilityId::new("audit.payload-limit");
        let mut host = instantiate_for_capability(
            cap.clone(),
            ResourceLimits {
                payload_size_limit: Some(2),
                ..Default::default()
            },
        );

        let result = host.call_capability(&cap, "op", b"too-large");

        assert!(result.is_err(), "oversized payload must be denied");
        assert_eq!(
            last_denial_category(&host).as_deref(),
            Some(DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE)
        );
    }

    #[test]
    fn max_calls_direct_capability_call_records_stable_denial_category() {
        let cap = CapabilityId::new("audit.max-calls");
        let mut host = instantiate_for_capability(
            cap.clone(),
            ResourceLimits {
                max_capability_calls: Some(0),
                ..Default::default()
            },
        );

        let result = host.call_capability(&cap, "op", b"");

        assert!(result.is_err(), "exhausted call budget must be denied");
        assert_eq!(
            last_denial_category(&host).as_deref(),
            Some(DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS)
        );
    }
}
