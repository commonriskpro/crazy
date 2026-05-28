# AIL package reference

<!-- Status: Implemented subset. This reference documents the current `ail-package` manifest, trust, resolver, registry, signing, advisory, yanking, lockfile, remote-registry, and compatibility primitives backed by source evidence. It is not a production registry or ecosystem stability claim. -->

AIL packages are semantic artifacts, not just code archives. A package can carry exports, imports, contracts, capabilities, handlers, trust metadata, verification evidence, unsafe-surface declarations, reproducible-build metadata, signatures, advisories, yanks, lockfiles, and compatibility records.

## Quick path

1. Use this document to see what `ail-package` currently implements.
2. Use [Compatibility policy](compatibility.md) before changing manifest fields, lockfile fields, trust tiers, registry protocol shapes, or package compatibility rules.
3. Keep the core invariant: importing a package never grants runtime capabilities automatically.

## Current implementation model

| Area | Implemented evidence | What it means |
|---|---|---|
| Manifest | `crates/ail-package/src/manifest.rs` | `PackageManifest`, `PackageDef`, provenance, artifact hashes, reproducible-build metadata, `blake3_hex`, `validate`, deterministic BLAKE3 hash, validation. |
| Trust | `crates/ail-package/src/trust.rs`, `policy.rs`, `assumption.rs`, `surface.rs` | Ordered trust levels, trust gates, unsafe-surface and assumption enforcement. |
| Verification evidence | `crates/ail-package/src/verification.rs` | Hash-bound `PackageVerificationReport` and local evidence validation for `TrustLevel::Verified`. |
| Imports/exports/handlers | `import.rs`, `export.rs`, `handler.rs` | Typed package dependencies, exported symbols, handler exports, visibility/stability metadata. |
| Resolution and lockfiles | `resolver.rs`, `lockfile.rs` | Dependency resolution from a registry plus hash-bound lockfile entries and integrity checks. |
| Signing and transparency | `signing.rs`, `remote_registry/signed_publish.rs` | Ed25519 signed packages, signature verification, transparency-log records, signed publish path. |
| Registry APIs | `registry.rs`, `http_registry.rs`, `remote_registry/types.rs` | In-memory/local registry, HTTP registry primitives, remote publish/fetch/search/verify DTOs. |
| Advisories and yanking | `advisory.rs`, `yank.rs` | Advisory matching, severity ordering, yanked versions remain present for reproducibility. |
| Versioning/compatibility | `versioning.rs` | SemVer classification, package schema metadata, migration records, local upgrade compatibility checks. |

## Manifest and trust fields

`PackageManifest` is the canonical typed package record; it exposes `blake3_hex` and `validate` for deterministic identity and structural checks. It includes package identity and compatibility metadata plus optional evidence and policy fields.

Key implemented concepts:

| Concept | Current type |
|---|---|
| Package identity/input | `PackageDef`, `PackageManifest` |
| Trust tier | `TrustLevel::{Unsafe, Unverified, Assumed, Verified}` |
| Exports/imports | `ExportDeclaration`, `ImportDeclaration` |
| Capabilities/handlers | required/exported capabilities in manifest data, `HandlerExport` |
| Unsafe surface | `UnsafeSurfaceEntry` |
| Verification evidence | `PackageVerificationReport` |
| Reproducible build metadata | `ReproducibleBuildEvidence` |
| Provenance/artifacts | `Provenance`, `ArtifactHashEntry`, `GeneratedArtifact` |

Validation currently enforces structural rules such as: unsafe packages must declare unsafe surface, verified packages need local verification evidence, and deterministic hashes use canonical CBOR plus BLAKE3.

## Trust levels

Trust tiers are ordered from least to most trusted:

```txt
Unsafe < Unverified < Assumed < Verified
```

| Trust level | Meaning |
|---|---|
| `Unsafe` | Requires unsafe-surface declaration and explicit approval to pass gates. |
| `Unverified` | No accepted trust evidence yet. |
| `Assumed` | Accepted through explicit assumptions/boundaries. |
| `Verified` | Highest local tier; must carry verification and reproducible-build evidence. |

Do not reorder trust variants. The source explicitly treats ordering as serialized compatibility-sensitive state.

## Verification evidence

`PackageVerificationReport` is content-addressed by deterministic CBOR + BLAKE3. `validate_verified_package_evidence` performs the current local preflight. The verified-package preflight checks:

- manifest has a report;
- report package/version match manifest package/version;
- artifact hashes are declared and match (`ArtifactHashesMismatch` when they do not);
- reproducible-build evidence exists (`MissingReproducibleEvidence` when absent);
- evidence hashes are valid 64-char lowercase hex;
- `build_inputs_hash` matches the documented derivation from source digest and toolchain id.

This is local evidence validation. It is not a global transparency-log or Sigstore-equivalent production guarantee yet.

## Resolver and lockfile

Implemented resolver/lockfile pieces include:

| Type | Role |
|---|---|
| `DependencySpec` | Requested package name/version constraint. |
| `DependencyResolver` | Resolves dependency specs against a package registry; failures use `ResolverError`. |
| `LockfileEntry` | Pins a resolved package version and content hash. |
| `Lockfile` | Ordered lockfile with deterministic BLAKE3 hash and `verify_integrity` checks. |

A lockfile records what was resolved. It does not grant runtime capabilities, and it does not replace package verification evidence.

## Signing, registry, advisories, and yanking

| Capability | Current implementation |
|---|---|
| Signing | `PackageKeypair`, `PackageSignature`, `SignedPackage`, `SigningError`. |
| Transparency | `TransparencyLogEntry`, `TransparencyLog`. |
| Local registry | `PackageRegistry::register`, `register_signed`, `lookup_by_name_version`, signed lookup, yanking. |
| Remote DTOs | `PublishRequest`, `FetchRequest`, `SearchRequest`, `VerifyRequest`, `VerifyOutcome`, and matching responses. |
| Signed publish | `publish_signed` verifies signature before registering the package. |
| Advisories | `SecurityAdvisory`, `AdvisorySeverity`, `AdvisoryChecker`. |
| Yanking | `YankRecord`; yanked packages remain available so old builds stay reproducible. |

## Compatibility and migrations

Package versioning uses SemVer plus schema metadata:

| Type | Role |
|---|---|
| `CompatibilityClass` | Patch, minor, or major compatibility class. |
| `PackageVersioning` | Version string plus graph/core-ir/ACL schema versions and compatibility class. |
| `MigrationRecord` / `MigrationStep` | Records required migration metadata for breaking package upgrades. |
| `PackageCompatibilityMetadata` | Local compatibility metadata for a package version. |
| `CompatibilityEngine` | Local compatibility/migration evaluator. |

Breaking package upgrades require migration metadata. That metadata records what changed and what replaces it; it does not execute a migration automatically.

## Non-negotiable package invariants

- Importing a package does not grant capabilities.
- Handler export is not handler binding.
- Capability definition is not runtime permission.
- `TrustLevel::Verified` requires evidence; the label alone is not enough.
- Yanked versions remain available for reproducibility of old builds.
- Lockfiles pin resolved content, but they do not prove trust.
- Compatibility metadata records upgrade risk; it does not magically make breaking upgrades safe.

## What is not product-grade yet

Do not overclaim the package ecosystem. Current gaps include:

- no deployed federated production registry;
- no production transparency-log or Sigstore/keyless signing integration;
- no broad package compatibility fixture matrix across historical releases;
- no full package authoring/publish CLI workflow comparable to Cargo;
- no ecosystem-scale advisory operations;
- no production policy story for package review, ownership, and federation.

## Review checklist

Before changing package surface, verify:

- [ ] Manifest/lockfile/registry DTO changes are classified with [Compatibility policy](compatibility.md).
- [ ] Trust-level changes preserve ordering and serialization compatibility.
- [ ] Verified package behavior still requires report, artifact, and reproducible-build evidence.
- [ ] Capability, handler, and import changes preserve `import != grant`.
- [ ] Breaking package compatibility changes include migration metadata and changelog notes.
