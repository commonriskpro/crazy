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
//   5. Wasmtime instantiation   — Module::new + Instance::new
//   6. Handler binding check    — only when profile.require_handler_binding() is true;
//                                 every granted capability must have a registered handler
//
// Exactly one AuditEvent is appended per `validate_and_instantiate` call.
// One AuditEvent::CapabilityCallExecuted is appended per `call_capability` call.

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use wasmtime::{Engine, Instance, Module, Store};

use ail_package::manifest::PackageManifest;
use ail_package::trust::TrustLevel;

use crate::abi::{HostError, HostResult};
use crate::audit::{AuditEvent, AuditLog};
use crate::error::{PreflightFailure, RuntimeError, RuntimeResult};
use crate::handler::Handler;
use crate::manifest::{CapabilityManifest, blake3_hex_of};
use crate::profile::{CapabilityId, RuntimeProfile};

// ── RuntimeInstance ───────────────────────────────────────────────────────

/// A validated and instantiated WASM module ready for future execution.
///
/// Carries the compiled Wasmtime `Module`, live `Store`, and instantiated
/// `Instance` as proof that the binary passed preflight and was instantiated
/// with the capability-gated host boundary.
pub struct RuntimeInstance {
    // Keep the module alive for future metadata/call APIs.
    _module: Module,
    store: Store<()>,
    instance: Instance,
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
/// Owns a single `Engine` (and its compilation configuration), an
/// in-memory `AuditLog`, and a pluggable list of [`Handler`]s.
///
/// # Handler dispatch
///
/// Handlers are checked in registration order.  The first handler whose
/// [`capabilities()`](Handler::capabilities) list contains the requested
/// [`CapabilityId`] is used.  If no handler matches, `call_capability`
/// returns a [`HostError`] with a `"HandlerNotBound"` message.
///
/// # Preflight
///
/// All preflight checks are evaluated inside [`validate_and_instantiate`]
/// before any Wasmtime work is attempted.  After a successful preflight the
/// profile is stored internally so that `call_capability` can enforce
/// grant checks without requiring callers to pass the profile again.
///
/// [`validate_and_instantiate`]: RuntimeHost::validate_and_instantiate
pub struct RuntimeHost {
    engine: Engine,
    audit_log: AuditLog,
    handlers: Vec<Arc<dyn Handler + Send + Sync>>,
    /// Stored after a successful `validate_and_instantiate`; used by
    /// `call_capability` to enforce grant checks.
    current_profile: Option<Arc<RuntimeProfile>>,
}

impl RuntimeHost {
    /// Create a new host with default Wasmtime configuration and no handlers.
    pub fn new() -> Self {
        RuntimeHost {
            engine: Engine::default(),
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
    ///
    /// `package_manifests` is the list of package manifests declared by the
    /// module.  Pass `&[]` (or call the two-argument form) when no packages
    /// are declared; the package trust gate is skipped for existing callers.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::PreflightFailed`] with the relevant
    /// [`PreflightFailure`] variant if any stage fails, or
    /// [`RuntimeError::EncodingError`] if the manifest cannot be CBOR-
    /// serialised (should not occur in practice).
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
    /// checks run.  Existing callers should continue using
    /// [`validate_and_instantiate`] (which passes `&[]` implicitly).
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
    ///
    /// # Errors
    ///
    /// - `HostError { message: "CapabilityDenied: <cap>" }` — capability not
    ///   granted in the active profile (or no profile set).
    /// - `HostError { message: "HandlerNotBound: <cap>" }` — no registered
    ///   handler serves this capability.
    /// - Any error returned by the dispatched handler.
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

    /// Build the audit event to append for a completed preflight run.
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

    /// Execute all preflight stages and instantiate the module.
    ///
    /// Does not update the audit log; that is the caller's responsibility.
    fn preflight_inner(
        &self,
        wasm: &[u8],
        manifest: &CapabilityManifest,
        profile: &RuntimeProfile,
        package_manifests: &[PackageManifest],
    ) -> RuntimeResult<RuntimeInstance> {
        // Stage 0 — Package trust gate.
        //
        // Runs before any WASM work.  If `profile.min_package_trust()` is
        // `None`, the gate is skipped entirely (backward-compatible default).
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

        // Stages 4+5 — Wasmtime validate + instantiate.
        let instance = self.instantiate_inner(wasm)?;

        // Stage 6 — Handler binding check (opt-in).
        //
        // Runs after instantiation succeeds.  Only active when
        // `profile.require_handler_binding()` is true.  Checks that every
        // *granted* capability has at least one registered handler.
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

    /// Enforce package trust gates from Stage 0.
    fn check_package_trust(
        manifests: &[PackageManifest],
        profile: &RuntimeProfile,
    ) -> RuntimeResult<()> {
        for m in manifests {
            // Unsafe packages are unconditionally blocked.
            if m.trust_level == TrustLevel::Unsafe {
                return Err(RuntimeError::PreflightFailed(
                    PreflightFailure::UnsafePackageNotApproved {
                        package: m.name.clone(),
                    },
                ));
            }

            // Check minimum tier gate.
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

    /// Validate, compile, and instantiate `wasm` with Wasmtime (stages 4 + 5).
    fn instantiate_inner(&self, wasm: &[u8]) -> RuntimeResult<RuntimeInstance> {
        Module::validate(&self.engine, wasm).map_err(|e| {
            RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
        })?;

        let module = Module::new(&self.engine, wasm).map_err(|e| {
            RuntimeError::PreflightFailed(PreflightFailure::WasmValidationError(e.to_string()))
        })?;

        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &module, &[]).map_err(|e| {
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

/// Extract the (denied capabilities, PreflightFailure) pair from an error.
///
/// Used by `build_audit_event` to populate `AuditEvent::PreflightFailed`.
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
