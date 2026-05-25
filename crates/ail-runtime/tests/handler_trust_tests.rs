// ── ail-runtime::handler_trust_tests ─────────────────────────────────────
//
// Handler trust-level enforcement preflight tests.
//
// Spec scenarios:
//   T1 — No trust gate: unverified handler passes (backward compat)
//   T2 — No trust gate: unsafe handler passes (backward compat)
//   T3 — Trust gate Assumed: assumed handler passes
//   T4 — Trust gate Assumed: verified handler passes
//   T5 — Trust gate Assumed: unverified handler BLOCKED (prod/critical rule)
//   T6 — Trust gate Assumed: unsafe handler BLOCKED (prod/critical rule)
//   T7 — Trust gate with binding required: both checks apply
//   T8 — Trust gate without binding: skips unbound grants, checks bound ones
//   T9 — Default trust_level() is Assumed (Handler trait default impl)
//  T10 — HandlerTrustViolation carries handler name, required, actual

use std::sync::Arc;

use ail_runtime::{
    CapabilityGrant, CapabilityId, CapabilityManifest, Handler, HostResult, InMemoryHandler,
    PreflightFailure, ResourceLimits, RuntimeError, RuntimeHost, RuntimeProfile, TrustLevel,
    blake3_hex_of,
};

// ── helpers ──────────────────────────────────────────────────────────────

fn minimal_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

fn matching_profile(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    grants: Vec<CapabilityGrant>,
) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    RuntimeProfile::new(
        "test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        grants,
        ResourceLimits::default(),
    )
}

/// A handler that declares a specific trust level.
struct TrustedHandler {
    name: &'static str,
    caps: Vec<CapabilityId>,
    trust: TrustLevel,
}

impl TrustedHandler {
    fn new(name: &'static str, cap: CapabilityId, trust: TrustLevel) -> Self {
        Self {
            name,
            caps: vec![cap],
            trust,
        }
    }
}

impl Handler for TrustedHandler {
    fn name(&self) -> &str {
        self.name
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn trust_level(&self) -> TrustLevel {
        self.trust
    }

    fn handle(
        &self,
        _capability: &CapabilityId,
        _operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        Ok(b"ok".to_vec())
    }
}

// ── T1 — No trust gate: unverified handler passes (backward compat) ───────

#[test]
fn t1_no_trust_gate_unverified_handler_passes() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("data.read");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap.clone(),
    };
    // No with_min_handler_trust() — gate disabled.
    let profile = matching_profile(&wasm, &manifest, vec![grant]).with_handler_binding_required();

    let handler = Arc::new(TrustedHandler::new(
        "unverified-h",
        cap,
        TrustLevel::Unverified,
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "no trust gate must not block unverified handler: {result:?}"
    );
}

// ── T2 — No trust gate: unsafe handler passes (backward compat) ───────────

#[test]
fn t2_no_trust_gate_unsafe_handler_passes() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("ffi.call");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant]).with_handler_binding_required();

    let handler = Arc::new(TrustedHandler::new("unsafe-h", cap, TrustLevel::Unsafe));
    let mut host = RuntimeHost::new().with_handler(handler);
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "no trust gate must not block unsafe handler: {result:?}"
    );
}

// ── T3 — Trust gate Assumed: assumed handler passes ───────────────────────

#[test]
fn t3_trust_gate_assumed_allows_assumed_handler() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("data.read");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant])
        .with_handler_binding_required()
        .with_min_handler_trust(TrustLevel::Assumed);

    let handler = Arc::new(TrustedHandler::new("assumed-h", cap, TrustLevel::Assumed));
    let mut host = RuntimeHost::new().with_handler(handler);
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "trust gate Assumed must allow Assumed handler: {result:?}"
    );
}

// ── T4 — Trust gate Assumed: verified handler passes ─────────────────────

#[test]
fn t4_trust_gate_assumed_allows_verified_handler() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("data.read");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant])
        .with_handler_binding_required()
        .with_min_handler_trust(TrustLevel::Assumed);

    let handler = Arc::new(TrustedHandler::new("verified-h", cap, TrustLevel::Verified));
    let mut host = RuntimeHost::new().with_handler(handler);
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "trust gate Assumed must allow Verified handler: {result:?}"
    );
}

// ── T5 — Trust gate Assumed: unverified handler BLOCKED ──────────────────

#[test]
fn t5_trust_gate_assumed_blocks_unverified_handler() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("data.read");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant])
        .with_handler_binding_required()
        .with_min_handler_trust(TrustLevel::Assumed);

    let handler = Arc::new(TrustedHandler::new(
        "unverified-h",
        cap,
        TrustLevel::Unverified,
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::HandlerTrustViolation {
            handler,
            required,
            actual,
        })) => {
            assert_eq!(handler, "unverified-h");
            assert_eq!(required, TrustLevel::Assumed);
            assert_eq!(actual, TrustLevel::Unverified);
        }
        other => panic!("expected HandlerTrustViolation, got {other:?}"),
    }

    // Audit must record a failure.
    let log = host.audit_log();
    assert_eq!(log.len(), 1);
    assert!(!log.events()[0].is_passed());
}

// ── T6 — Trust gate Assumed: unsafe handler BLOCKED ──────────────────────

#[test]
fn t6_trust_gate_assumed_blocks_unsafe_handler() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("ffi.call");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap.clone(),
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant])
        .with_handler_binding_required()
        .with_min_handler_trust(TrustLevel::Assumed);

    let handler = Arc::new(TrustedHandler::new("unsafe-h", cap, TrustLevel::Unsafe));
    let mut host = RuntimeHost::new().with_handler(handler);
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    match result {
        Err(RuntimeError::PreflightFailed(PreflightFailure::HandlerTrustViolation {
            required,
            actual,
            ..
        })) => {
            assert_eq!(required, TrustLevel::Assumed);
            assert_eq!(actual, TrustLevel::Unsafe);
        }
        other => panic!("expected HandlerTrustViolation for Unsafe handler, got {other:?}"),
    }
}

// ── T7 — Trust gate with binding required: both checks apply ─────────────

// When require_handler_binding AND min_handler_trust are both set, a missing
// handler still fails with HandlerNotBound (not HandlerTrustViolation).
#[test]
fn t7_trust_gate_and_binding_required_missing_handler_fails_not_bound() {
    let wasm = minimal_wasm();
    let cap = CapabilityId::new("data.read");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap.clone()],
    };
    let grant = CapabilityGrant {
        module: "mod".to_string(),
        capability: cap,
    };
    let profile = matching_profile(&wasm, &manifest, vec![grant])
        .with_handler_binding_required()
        .with_min_handler_trust(TrustLevel::Assumed);

    // No handler registered.
    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        matches!(
            result,
            Err(RuntimeError::PreflightFailed(
                PreflightFailure::HandlerNotBound { .. }
            ))
        ),
        "missing handler with binding required must fail HandlerNotBound, got {result:?}"
    );
}

// ── T8 — Trust gate without binding: skips unbound grants ────────────────

// When only min_handler_trust is set (binding not required), grants with no
// bound handler are skipped — only bound handlers are checked for trust.
#[test]
fn t8_trust_gate_only_checks_bound_handlers() {
    let wasm = minimal_wasm();
    let cap_bound = CapabilityId::new("data.read");
    let cap_unbound = CapabilityId::new("data.write");
    let manifest = CapabilityManifest {
        module: "mod".to_string(),
        requires: vec![cap_bound.clone(), cap_unbound.clone()],
    };
    let profile = matching_profile(
        &wasm,
        &manifest,
        vec![
            CapabilityGrant {
                module: "mod".to_string(),
                capability: cap_bound.clone(),
            },
            CapabilityGrant {
                module: "mod".to_string(),
                capability: cap_unbound.clone(),
            },
        ],
    )
    // Trust gate enabled but binding NOT required.
    .with_min_handler_trust(TrustLevel::Assumed);

    // Only cap_bound has a handler (trusted); cap_unbound is unbound.
    let handler = Arc::new(TrustedHandler::new(
        "read-h",
        cap_bound,
        TrustLevel::Verified,
    ));
    let mut host = RuntimeHost::new().with_handler(handler);
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "unbound grants must be skipped by trust check (binding not required): {result:?}"
    );
}

// ── T9 — Default trust_level() is Assumed ────────────────────────────────

// InMemoryHandler (which does not override trust_level) must return Assumed.
#[test]
fn t9_default_trust_level_is_assumed() {
    let cap = CapabilityId::new("noop");
    let handler = InMemoryHandler::new("noop-h", vec![cap], b"".to_vec());
    assert_eq!(
        handler.trust_level(),
        TrustLevel::Assumed,
        "Handler default trust_level() must be TrustLevel::Assumed"
    );
}

// ── T10 — HandlerTrustViolation carries all fields ────────────────────────

#[test]
fn t10_handler_trust_violation_error_display() {
    let failure = PreflightFailure::HandlerTrustViolation {
        handler: "risky-handler".to_string(),
        required: TrustLevel::Assumed,
        actual: TrustLevel::Unverified,
    };
    let msg = failure.to_string();
    assert!(
        msg.contains("risky-handler"),
        "display must include handler name: {msg}"
    );
    assert!(
        msg.contains("assumed"),
        "display must include required level: {msg}"
    );
    assert!(
        msg.contains("unverified"),
        "display must include actual level: {msg}"
    );
}
