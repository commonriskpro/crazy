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
// Wasmtime Linker:
//   A `Linker<HostState>` is constructed once at `RuntimeHost::new` and
//   registers the stub import `ail/host_call`.  `Store<HostState>` carries
//   an `Arc<HandlerRegistry>` so host-function closures can dispatch to
//   registered handlers.  The `Store` data type changed from `()` to
//   `HostState` — all existing tests are unaffected because the Linker
//   satisfies WASM modules that have no host imports (existing test WASMs).

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use wasmtime::{Engine, Linker, Module, Store};

use ail_package::manifest::PackageManifest;
use ail_package::trust::TrustLevel;

use crate::abi::{HostError, HostResult};
use crate::audit::{AuditEvent, AuditLog};
use crate::error::{PreflightFailure, RuntimeError, RuntimeResult};
use crate::handler::Handler;
use crate::manifest::{CapabilityManifest, blake3_hex_of};
use crate::profile::{CapabilityId, RuntimeProfile};

// ── HostState ─────────────────────────────────────────────────────────────

/// Data carried in the Wasmtime `Store`.
///
/// Holds a reference to the handler registry so that host-function closures
/// registered in the `Linker` can dispatch capability calls without needing
/// a mutable borrow of `RuntimeHost`.
///
/// `Arc<Vec<Arc<dyn Handler + Send + Sync>>>` keeps the handlers alive and
/// allows cheap cloning into `'static` Wasmtime closures.
pub(crate) struct HostState {
    /// Handler registry shared with Wasmtime host-function closures.
    ///
    /// Currently not read by the stub `ail/host_call` closure — dispatch
    /// happens via `RuntimeHost::call_capability` on the host side.  The
    /// field is retained so full in-WASM dispatch can be wired in a future
    /// phase without changing the `Store` type.
    #[allow(dead_code)]
    pub(crate) handlers: Arc<Vec<Arc<dyn Handler + Send + Sync>>>,
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
/// stub registered), an in-memory `AuditLog`, and a pluggable list of
/// [`Handler`]s.
///
/// # Handler dispatch
///
/// Handlers are checked in registration order.  The first handler whose
/// [`capabilities()`](Handler::capabilities) list contains the requested
/// [`CapabilityId`] is used.
///
/// # Preflight
///
/// All preflight checks are evaluated inside [`validate_and_instantiate`]
/// before Wasmtime instantiation.  After a successful preflight the
/// profile is stored so `call_capability` can enforce grant checks.
///
/// [`validate_and_instantiate`]: RuntimeHost::validate_and_instantiate
pub struct RuntimeHost {
    engine: Engine,
    linker: Linker<HostState>,
    audit_log: AuditLog,
    handlers: Vec<Arc<dyn Handler + Send + Sync>>,
    /// Stored after a successful `validate_and_instantiate`; used by
    /// `call_capability` to enforce grant checks.
    current_profile: Option<Arc<RuntimeProfile>>,
}

impl RuntimeHost {
    /// Create a new host with default Wasmtime configuration and no handlers.
    ///
    /// A `Linker<HostState>` is constructed and the stub import
    /// `ail/host_call` is registered so that WASM modules declaring this
    /// import can instantiate without error.
    pub fn new() -> Self {
        let engine = Engine::default();
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
            current_profile: None,
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
        }
        result
    }

    /// Dispatch a capability call to a registered handler.
    ///
    /// Steps:
    /// 1. Check the active profile grants `capability`; deny if not.
    /// 2. Find the first handler whose `capabilities()` includes `capability`.
    /// 3. Dispatch to `handler.handle(capability, operation, payload)`.
    /// 4. Append an [`AuditEvent::CapabilityCallExecuted`] event.
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

        // Step 2: find matching handler.
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

        // Step 3: dispatch.
        let result = handler.handle(capability, operation, payload);
        let duration_us = start.elapsed().as_micros() as u64;
        let succeeded = result.is_ok();

        // Step 4: audit.
        self.audit_log.push(AuditEvent::CapabilityCallExecuted {
            capability: capability.clone(),
            operation: operation.to_string(),
            handler_name,
            succeeded,
            duration_us,
        });

        result
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
        let instance = self.instantiate_inner(wasm)?;

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
    fn instantiate_inner(&self, wasm: &[u8]) -> RuntimeResult<RuntimeInstance> {
        Module::validate(&self.engine, wasm).map_err(|e| {
            RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
        })?;

        let module = Module::new(&self.engine, wasm).map_err(|e| {
            RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
        })?;

        let handlers_arc: Arc<Vec<Arc<dyn Handler + Send + Sync>>> =
            Arc::new(self.handlers.clone());
        let mut store = Store::new(
            &self.engine,
            HostState {
                handlers: handlers_arc,
            },
        );

        let instance = self.linker.instantiate(&mut store, &module).map_err(|e| {
            RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
        })?;

        Ok(RuntimeInstance {
            _module: module,
            store,
            instance,
        })
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
            | PreflightFailure::HandlerNotBound { .. },
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
