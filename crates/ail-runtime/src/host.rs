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
//   CapabilityInputSchema before dispatching to the handler.
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
use std::sync::Arc;
use std::time::Instant;

use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

use ail_package::manifest::PackageManifest;
use ail_package::trust::TrustLevel;

use crate::abi::{HostError, HostResult};
use crate::audit::{AuditEvent, AuditLog};
use crate::error::{PreflightFailure, RuntimeError, RuntimeResult};
use crate::handler::Handler;
use crate::manifest::{CapabilityManifest, blake3_hex_of};
use crate::profile::{CapabilityId, RuntimeProfile};
use crate::report::{CapabilityCallSummary, RuntimeReport, RuntimeReportStatus};
use crate::schema::CapabilityDefinition;
use crate::transaction::TransactionGroup;

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
    #[allow(dead_code)]
    pub(crate) handlers: Arc<Vec<Arc<dyn Handler + Send + Sync>>>,
    /// Resource limiter enforcing `max_memory_bytes`.
    pub(crate) limiter: StoreLimits,
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
/// `CapabilityInputSchema` before dispatching.  Capabilities without a
/// registered schema are not validated (pass-through).
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
    audit_log: AuditLog,
    handlers: Vec<Arc<dyn Handler + Send + Sync>>,
    /// Schema registry: capability ID string → CapabilityDefinition.
    schema_registry: HashMap<String, CapabilityDefinition>,
    /// Stored after a successful `validate_and_instantiate`; used by
    /// `call_capability` to enforce grant checks.
    current_profile: Option<Arc<RuntimeProfile>>,
    /// Module name from the most recent capability manifest (used in reports).
    current_module_name: Option<String>,
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

        // Register stub import: module="ail", name="host_call".
        //
        // Signature: (capability_ptr: i32, capability_len: i32,
        //              op_ptr: i32,         op_len: i32,
        //              payload_ptr: i32,    payload_len: i32) -> i32
        //
        // The stub returns 0 (success) without reading WASM memory.  Full
        // memory-safe dispatch via `call_capability` is the host-side API;
        // the linker function serves as the ABI boundary for WASM modules
        // that declare the import.
        linker
            .func_wrap(
                "ail",
                "host_call",
                |_caller: wasmtime::Caller<'_, HostState>,
                 _cap_ptr: i32,
                 _cap_len: i32,
                 _op_ptr: i32,
                 _op_len: i32,
                 _payload_ptr: i32,
                 _payload_len: i32|
                 -> i32 { 0 },
            )
            .expect("ail/host_call registration must succeed");

        RuntimeHost {
            engine,
            linker,
            audit_log: AuditLog::new(),
            handlers: Vec::new(),
            schema_registry: HashMap::new(),
            current_profile: None,
            current_module_name: None,
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

    /// Read-only access to the accumulated audit log.
    pub fn audit_log(&self) -> &AuditLog {
        &self.audit_log
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
        self.audit_log.push(event);
        if result.is_ok() {
            self.current_profile = Some(Arc::new(profile.clone()));
            self.current_module_name = Some(manifest.module.clone());
        }
        result
    }

    /// Dispatch a capability call to a registered handler.
    ///
    /// Steps:
    /// 1. Check the active profile grants `capability`; deny if not.
    /// 2. Validate the input payload against the registered schema (if any).
    /// 3. Find the first handler whose `capabilities()` includes `capability`.
    /// 4. Dispatch to `handler.handle(capability, operation, payload)`.
    /// 5. Append an [`AuditEvent::CapabilityCallExecuted`] event.
    pub fn call_capability(
        &mut self,
        capability: &CapabilityId,
        operation: &str,
        payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        let start = Instant::now();

        // Step 1: grant check.
        let granted = self
            .current_profile
            .as_ref()
            .map(|p| p.grants_capability(capability))
            .unwrap_or(false);

        if !granted {
            let err = HostError {
                message: format!("CapabilityDenied: {}", capability.as_str()),
            };
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.push(AuditEvent::CapabilityCallExecuted {
                capability: capability.clone(),
                operation: operation.to_string(),
                handler_name: "none".to_string(),
                succeeded: false,
                duration_us,
            });
            return Err(err);
        }

        // Step 2: schema/boundary validation (if a definition is registered).
        if let Some(def) = self.schema_registry.get(capability.as_str()) {
            if let Err(schema_err) = def.schema().input().validate(payload) {
                let err = HostError {
                    message: format!(
                        "PayloadDecodeError: schema validation failed for `{}`: {}",
                        capability.as_str(),
                        schema_err.message
                    ),
                };
                let duration_us = start.elapsed().as_micros() as u64;
                self.audit_log.push(AuditEvent::CapabilityCallExecuted {
                    capability: capability.clone(),
                    operation: operation.to_string(),
                    handler_name: "none".to_string(),
                    succeeded: false,
                    duration_us,
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
            let err = HostError {
                message: format!("HandlerNotBound: {}", capability.as_str()),
            };
            let duration_us = start.elapsed().as_micros() as u64;
            self.audit_log.push(AuditEvent::CapabilityCallExecuted {
                capability: capability.clone(),
                operation: operation.to_string(),
                handler_name: "none".to_string(),
                succeeded: false,
                duration_us,
            });
            return Err(err);
        };

        let handler_name = handler.name().to_string();

        // Step 4: dispatch.
        let result = handler.handle(capability, operation, payload);
        let duration_us = start.elapsed().as_micros() as u64;
        let succeeded = result.is_ok();

        // Step 5: audit.
        self.audit_log.push(AuditEvent::CapabilityCallExecuted {
            capability: capability.clone(),
            operation: operation.to_string(),
            handler_name,
            succeeded,
            duration_us,
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
    pub fn execute_with_rollback<F, T>(
        &mut self,
        tx: &mut TransactionGroup,
        f: F,
    ) -> HostResult<T>
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
    pub fn emit_report(
        &self,
        status: RuntimeReportStatus,
        id: impl Into<String>,
    ) -> RuntimeReport {
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

        let module_name = self
            .current_module_name
            .clone()
            .unwrap_or_default();

        // Build per-capability summaries from CapabilityCallExecuted events.
        let mut totals: HashMap<String, (u32, u32, u32)> = HashMap::new(); // cap → (total, ok, err)

        for event in self.audit_log.events() {
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
        let audit_log_hash = if !self.audit_log.is_empty() {
            let serialized: String = self
                .audit_log
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

        // Stage 3 — Capability grant check.
        let denied: Vec<CapabilityId> = manifest
            .requires
            .iter()
            .filter(|cap| !profile.grants_capability(cap))
            .cloned()
            .collect();
        if !denied.is_empty() {
            return Err(RuntimeError::PreflightFailed(
                PreflightFailure::CapabilityDenied { denied },
            ));
        }

        // Stages 4+5 — Wasmtime validate + instantiate via linker.
        let instance = self.instantiate_inner(wasm, profile.limits())?;

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
        limits: &crate::profile::ResourceLimits,
    ) -> RuntimeResult<RuntimeInstance> {
        Module::validate(&self.engine, wasm).map_err(|e| {
            RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
        })?;

        let module = Module::new(&self.engine, wasm).map_err(|e| {
            RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
        })?;

        // Build the StoreLimits for memory caps (no-op when None).
        let store_limits: StoreLimits = match limits.max_memory_bytes {
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
                limiter: store_limits,
            },
        );

        // Wire the resource limiter so Wasmtime consults it on memory growth.
        store.limiter(|state| &mut state.limiter);

        // Set the fuel budget.  When consume_fuel is enabled on the Engine
        // every Store starts with 0 fuel — we must always initialise it.
        // If no limit is configured, grant effectively-unlimited fuel.
        let fuel = limits.max_fuel.unwrap_or(u64::MAX);
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
