# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Supply-chain audit gate** (`deny.toml`, `.cargo/audit.toml`, CI job): `cargo deny
  check` runs on every PR to enforce license policy, advisory bans, and duplicate
  detection. `cargo audit` runs nightly.
- **Fuzz harness** (`fuzz/`): three `cargo-fuzz` targets covering the changeset parser,
  CBOR codec, and WASM runtime. Seeds committed; CI runs each target for 30 s on nightly.
- **Schema migration runner** (`crates/ail-storage/src/migration.rs`): `Migration` trait,
  `MigrationCatalog` with `apply` / `current_version`, `V0ToV1Migration` (structural no-op),
  and `default_catalog()`. Schema version stored as CBOR-encoded `u32` in the object store.
- **Compatibility matrix** (`crates/ail-dogfood/tests/compat_matrix.rs`): frozen CBOR
  snapshot (`tests/fixtures/schema_v0.cbor`) with BLAKE3 bit-stability check; migration
  applied on a pre-populated store; codec round-trip verified against the fixture.
- **Release tagging script** (`scripts/tag-release.sh`): checks clean worktree, runs full
  test suite and `cargo deny check`, creates an annotated tag; supports GPG signing via
  `SIGN=1`.
- **Release policy** (`docs/release-policy.md`): semver contract, tagging procedure,
  signing flow stub, lockstep versioning rationale.
- **Migration guide** (`docs/migration-guide.md`): user-facing v0 → v1 upgrade
  instructions and rollback procedure via snapshot restore.
- **Release metadata preflight** (`scripts/release-preflight.sh`): verifies that
  the requested release version matches `Cargo.toml`, workspace crates use
  lockstep versioning, and the changelog has a release heading before tagging.
