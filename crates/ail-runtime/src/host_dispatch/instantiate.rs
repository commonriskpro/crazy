// ── ail-runtime::host_dispatch::instantiate ───────────────────────────────

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wasmtime::{Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

use crate::audit::AuditLog;
use crate::error::{PreflightFailure, RuntimeError, RuntimeResult};
use crate::handler::Handler;
use crate::host_dispatch::instance::RuntimeInstance;
use crate::host_dispatch::limits::ClockFn;
use crate::host_dispatch::state::HostState;
use crate::profile::{CapabilityRevocationRegistry, RuntimeProfile};

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
