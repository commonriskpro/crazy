// ── ail-runtime::host ────────────────────────────────────────────────────
//
// `RuntimeHost` — capability-gated Wasmtime host.
//
// Preflight pipeline (strict order):
//   1. WASM bytes hash check    — blake3(wasm) vs profile.module_hash()
//   2. Manifest hash check      — blake3(cbor(manifest)) vs profile.capability_manifest_hash()
//   3. Capability grant check   — manifest.requires ⊆ profile.grants
//   4. Wasmtime validation      — Module::validate (structural / binary format)
//   5. Wasmtime instantiation   — Module::new + Instance::new
//
// Exactly one AuditEvent is appended per `validate_and_instantiate` call.
// Linker / host-function wiring is deferred to a later phase.

use std::fmt;

use wasmtime::{Engine, Instance, Module, Store};

use crate::audit::{AuditEvent, AuditLog};
use crate::error::{PreflightFailure, RuntimeError, RuntimeResult};
use crate::manifest::{CapabilityManifest, blake3_hex_of};
use crate::profile::{CapabilityId, RuntimeProfile};

// ── RuntimeInstance ───────────────────────────────────────────────────────

/// A validated and instantiated WASM module ready for future execution.
///
/// Carries the compiled Wasmtime `Module`, live `Store`, and instantiated
/// `Instance` as proof that the binary passed preflight and was instantiated
/// with the Phase 8 empty-import host boundary.
///
/// Host-function linker registration (importing capabilities) is deferred
/// to a later phase; this type does not yet expose call APIs.
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
/// Owns a single `Engine` (and its compilation configuration) and an
/// in-memory `AuditLog`.  All preflight checks are evaluated inside
/// [`validate_and_instantiate`] before any Wasmtime work is attempted.
pub struct RuntimeHost {
    engine: Engine,
    audit_log: AuditLog,
}

impl RuntimeHost {
    /// Create a new host with default Wasmtime configuration.
    ///
    /// `Engine::default()` is used; fuel and reference types are disabled,
    /// which is the correct baseline for Phase 8.
    pub fn new() -> Self {
        RuntimeHost {
            engine: Engine::default(),
            audit_log: AuditLog::new(),
        }
    }

    /// Read-only access to the accumulated audit log.
    pub fn audit_log(&self) -> &AuditLog {
        &self.audit_log
    }

    /// Preflight-check and instantiate a WASM module.
    ///
    /// Runs the five-stage preflight pipeline (see module doc).  Appends
    /// exactly one [`AuditEvent`] to the internal log regardless of outcome.
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
        let result = self.preflight_inner(wasm, manifest, profile);
        let event = Self::build_audit_event(&result, profile, wasm);
        self.audit_log.push(event);
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
    ) -> RuntimeResult<RuntimeInstance> {
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
        self.instantiate_inner(wasm)
    }

    /// Validate, compile, and instantiate `wasm` with Wasmtime (stages 4 + 5).
    ///
    /// Called only after all preflight checks pass.  Both Wasmtime errors
    /// are wrapped in [`PreflightFailure::WasmValidationError`] because they
    /// both reflect a binary format or structural issue in the submitted bytes.
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
        RuntimeError::PreflightFailed(failure) => (vec![], failure.clone()),
        RuntimeError::EncodingError(msg) => (
            vec![],
            PreflightFailure::WasmValidationError(format!("encoding: {msg}")),
        ),
    }
}
