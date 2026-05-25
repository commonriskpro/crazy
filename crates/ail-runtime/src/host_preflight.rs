// ── ail-runtime::host_preflight ──────────────────────────────────────────
//
// Preflight validation pipeline and audit helpers.
//
// All functions in this module are called exclusively from
// `RuntimeHost::validate_and_instantiate_with_packages`.
//
// Preflight stages (in order):
//   0. Package trust gate
//   1. WASM bytes hash check
//   2. Manifest CBOR hash check
//   3. Capability grant check
//   4+5. Wasmtime validate + instantiate (via `instantiate_inner`)
//   6. Handler binding check (opt-in)

use std::sync::{Arc, Mutex};

use ail_package::manifest::PackageManifest;
use ail_package::trust::TrustLevel;
use ail_package::validate_verified_package_evidence;

use crate::audit::{AuditEvent, AuditLog};
use crate::error::{PreflightFailure, RuntimeError, RuntimeResult};
use crate::handler::Handler;
use crate::host_dispatch::{ClockFn, HostState, RuntimeInstance, instantiate_inner};
use crate::manifest::{CapabilityManifest, blake3_hex_of};
use crate::profile::{CapabilityId, CapabilityRevocationRegistry, RuntimeProfile};
use wasmtime::{Engine, Linker};

// ── failure_parts ─────────────────────────────────────────────────────────

pub(crate) fn failure_parts(err: &RuntimeError) -> (Vec<CapabilityId>, PreflightFailure) {
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
            | PreflightFailure::PackageVerificationEvidenceInvalid { .. }
            | PreflightFailure::HashMismatch { .. }
            | PreflightFailure::WasmValidationError(_)
            | PreflightFailure::HandlerNotBound { .. }
            | PreflightFailure::ResourceLimitExceeded { .. }
            | PreflightFailure::HandlerTrustViolation { .. },
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

// ── build_audit_event ─────────────────────────────────────────────────────

pub(crate) fn build_audit_event(
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

// ── check_package_trust ───────────────────────────────────────────────────

pub(crate) fn check_package_trust(
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

        validate_verified_package_evidence(m).map_err(|err| {
            RuntimeError::PreflightFailed(PreflightFailure::PackageVerificationEvidenceInvalid {
                package: m.name.clone(),
                reason: err.to_string(),
            })
        })?;

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

// ── preflight_inner ───────────────────────────────────────────────────────

/// Run the full preflight pipeline and return a ready [`RuntimeInstance`].
///
/// Stages:
///   0. Package trust gate (skipped when `package_manifests` is empty)
///   1. WASM bytes hash check
///   2. Manifest CBOR hash check
///   3. Capability grant check (module-scoped)
///   4. Wasmtime validate + instantiate
///   5. Handler binding check (opt-in via `profile.require_handler_binding()`)
#[allow(clippy::too_many_arguments)]
pub(crate) fn preflight_inner(
    engine: &Engine,
    linker: &Linker<HostState>,
    handlers: &[Arc<dyn Handler + Send + Sync>],
    revocations: &CapabilityRevocationRegistry,
    audit_log: &Arc<Mutex<AuditLog>>,
    wasm: &[u8],
    manifest: &CapabilityManifest,
    profile: &RuntimeProfile,
    package_manifests: &[PackageManifest],
    clock_fn: ClockFn,
) -> RuntimeResult<RuntimeInstance> {
    // Stage 0 — Package trust gate.
    if !package_manifests.is_empty() {
        check_package_trust(package_manifests, profile)?;
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
    let instance = instantiate_inner(
        engine,
        linker,
        handlers,
        revocations,
        audit_log,
        wasm,
        profile,
        &manifest.module,
        clock_fn,
    )?;

    // Stage 6 — Handler binding check (opt-in) and handler trust gate.
    //
    // This stage runs when either:
    //   a) `profile.require_handler_binding()` is true  — every granted capability
    //      must have a registered handler, or preflight fails with HandlerNotBound.
    //   b) `profile.min_handler_trust()` is Some(level) — every bound handler that
    //      serves a granted capability must declare at least `level` trust, or
    //      preflight fails with HandlerTrustViolation.
    let min_handler_trust = profile.min_handler_trust();
    if profile.require_handler_binding() || min_handler_trust.is_some() {
        for grant in profile.grants() {
            let handler = handlers
                .iter()
                .find(|h| h.capabilities().contains(&grant.capability));

            match handler {
                None if profile.require_handler_binding() => {
                    return Err(RuntimeError::PreflightFailed(
                        PreflightFailure::HandlerNotBound {
                            capability: grant.capability.clone(),
                        },
                    ));
                }
                Some(h) if let Some(required) = min_handler_trust => {
                    let actual = h.trust_level();
                    if !actual.satisfies(required) {
                        return Err(RuntimeError::PreflightFailed(
                            PreflightFailure::HandlerTrustViolation {
                                handler: h.name().to_string(),
                                required,
                                actual,
                            },
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    Ok(instance)
}
