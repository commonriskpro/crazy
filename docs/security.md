# Security and runtime hardening

<!-- Status: Implemented subset. AIL has validation-stage security primitives across runtime capability gates, handler trust, package trust, advisories, signing, context redaction, and audit records. This is not a production security guarantee. -->

AIL's security model is simple: programs do not get ambient authority. Every external effect must flow through explicit capabilities, runtime profiles, package trust gates, and evidence that reviewers can inspect.

## Quick path

1. Read [Runtime](runtime.md) for the capability host protocol.
2. Read [Package reference](package-reference.md) before trusting package metadata, handlers, or advisories.
3. Use this file as the security review checklist before claiming Runtime safety, runtime, or ecosystem hardening.
4. Run `./scripts/docs-security-smoke.sh` after changing runtime, package, context, or security docs.

## Current security invariants

| Area | Current invariant | Evidence |
|------|-------------------|----------|
| Runtime startup | `RuntimeHost` preflight checks package trust, WASM hash, manifest hash, capability grants, Wasmtime validation, handler binding, handler trust, and profile assumptions before execution. | `crates/ail-runtime/src/host.rs`, `crates/ail-runtime/tests/preflight_tests.rs`, `crates/ail-runtime/tests/package_trust_tests.rs`, `crates/ail-runtime/tests/handler_trust_tests.rs`, `crates/ail-runtime/tests/assumption_expiry_tests.rs` |
| Capability calls | Runtime calls are deny-by-default runtime gate checks: ungranted capabilities are denied before handlers receive the request. | `Capability grant check`, `ungranted_capability_denied`, `crates/ail-runtime/tests/handler_tests.rs` |
| Boundary schemas | Registered capability definitions validate input and output payloads before/after handler dispatch. | `schema_registry`, `crates/ail-runtime/tests/schema_enforcement_tests.rs`, `crates/ail-runtime/tests/schema_typed_output_tests.rs` |
| Limits | `RuntimeProfile` entries carry `ResourceLimits` for memory, fuel, timeout, capability-call counts, rate limits, payload size, concurrency, recursion, and output size. | `crates/ail-runtime/src/profile.rs`, `crates/ail-runtime/tests/resource_limits_tests.rs`, `crates/ail-runtime/tests/rate_limits_enforcement_tests.rs` |
| Audit | Capability calls emit audit records with hashes and `denial_category` as audit-only data, not caller-visible secret detail. Plain invariant: denial_category as audit-only data. | `crates/ail-runtime/src/host.rs`, `crates/ail-runtime/tests/audit_tests.rs`, `crates/ail-runtime/tests/secret_provider_audit_tests.rs` |
| Secrets | `SecretReadHandler` maps logical secret IDs through profile-controlled mappings; callers receive opaque denials, while audit categories stay non-oracle. | `crates/ail-runtime/src/secret.rs`, `crates/ail-runtime/tests/secret_wasm_e2e_tests.rs` |
| Context redaction | Context responses remove redacted nodes and return `E_ACCESS_DENIED` for unauthorized redacted targets. | `crates/ail-context/src/redaction.rs`, `crates/ail-context/src/builder_tests/r2_attributes.rs` |
| Package supply chain | Package signing uses `PackageKeypair`, signed manifests, tamper rejection, `SecurityAdvisory`, `AdvisoryChecker`, advisories, yanking, trust gates, and `ResolverError` conflicts such as `CapabilityConflict` and `HandlerConflict`. | `crates/ail-package/src/signing.rs`, `crates/ail-package/src/advisory.rs`, `crates/ail-package/src/resolver.rs`, `crates/ail-package/tests/lifecycle.rs` |

## Evidence anchors

These exact source symbols are intentionally named so static docs drift checks can catch accidental overclaims:

- Runtime host: `Package trust gate`, `Capability grant check`, `schema_registry`, `denial_category`, `CapabilityRevocationRegistry`.
- Runtime profile: `RuntimeProfile`, `ResourceLimits`, `CapabilityRevocationRegistry`, `InFlightPolicy`, `with_min_handler_trust`.
- Secrets: `SecretReadHandler`, `SecretProviderError`, `SecretVault`.
- Package supply chain: `PackageKeypair`, `tampered_manifest_rejects_signature`, `TransparencyLog`, `SecurityAdvisory`, `AdvisoryChecker`, `ResolverError`, `CapabilityConflict`, `HandlerConflict`.

## Review checklist

Before a PR claims security or runtime-safety progress, verify:

- [ ] The claim names the exact security surface: runtime, package, context, storage, CLI, stdlib capability, or release process.
- [ ] The change preserves import != grant: installing or importing package code must not grant runtime capabilities automatically.
- [ ] Capability denial has negative coverage, not just happy-path execution.
- [ ] Handler trust (`with_min_handler_trust`) and package trust behavior are explicit for production-like profiles.
- [ ] Secret, token, path, and vault details are not exposed in caller errors or raw audit payloads.
- [ ] The release note does not imply production security unless the [maturity model](maturity-model.md) security/runtime evidence is satisfied.

## Current gaps

These are NOT production-ready yet:

- In-flight revocation policy is stored but not currently enforced: the current data path uses `CapabilityRevocationRegistry` / `InFlightPolicy`, while `allow_complete`, `cancel`, and `timeout_then_cancel` remain target semantics.
- External vault integration is not implemented; current secret handling is validation-stage and in-memory/test-focused.
- Process/remote handler isolation, adversarial fuzzing depth, and operational key lifecycle are still hardening work.
- Package registry federation and Sigstore-style/keyless signing remain future supply-chain hardening.
- Security docs are evidence-backed but do not replace a threat model, audit, or production incident process.

## Verification

```sh
./scripts/docs-security-smoke.sh
```

This smoke is static on purpose. It prevents this reference from drifting ahead of the implemented runtime/package/context evidence without running build-heavy checks.
