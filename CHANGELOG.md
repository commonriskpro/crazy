# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **`clock.now` epoch-ms capability boundary** (Wave 23B): replaced silent-truncating
  `.as_millis() as i64` cast in `ClockHandler::handle` with a checked `try_into()`
  conversion. A pathological system clock exceeding `i64::MAX` now surfaces a
  `HostError::Custom` instead of silently wrapping.
- **`ClockHandler` returns epoch-milliseconds** (Wave 23B): handler previously called
  `.as_secs()` (epoch-seconds); corrected to `.as_millis()` to match the `clock.now`
  contract.
- **`bytes_length` lossy cast** (Wave 19): replaced `as i64` with `i64::try_from`,
  surfacing an error instead of silently truncating byte-buffer lengths above `i64::MAX`.
- **`CoreExpr::WhileLoop` single-evaluation bug** (Wave 21A): condition was compiled
  once and never re-evaluated per iteration. Desugared to `Loop + If + Break/Continue`
  so the condition is re-read on every pass.
- **Local binding atomization in `lower_core_expr_to_anf_local`** (Wave 20): bindings
  for `WhileLoop`, `ForEach`, `Fold`, and `Cell` were not atomized, allowing stale
  captures. Now each is bound to a fresh `let` before entering the loop/fold body.
- **`std.iter.fold` contract wording** (Wave 22A): `requires` clause incorrectly
  described the reducer signature as `(U, T) -> U`; corrected to state the actual
  binary-pair encoding `List([acc, item])`.
- **`std.time.duration_since` contract** (Wave 21A): corrected ensures clause and
  strengthened type-error coverage.
- **Fold DCE optimizer retains atom bindings** (Wave 22C): dead-let elimination
  incorrectly removed `Fold` atom bindings visible downstream. Proven fixed via
  `OPT-FOLD-DCE-1` regression test.
- **Package HTTP 500 mock request-drain flake** (Wave 19): registry mock did not drain
  the request body on 500 responses, causing intermittent test hangs; drain added.

### Added

- **`std.bytes` exec handlers** (Wave 19): `bytes_length`, `bytes_concat`, and
  `bytes_slice` promoted from metadata-only stubs to live WASM host-ABI handlers;
  v1 registry entries added.
- **ACL map/set constructor forms** (Wave 20C): `map(k, v, ...)` and `set(x, ...)`
  parser forms added to the ACL expression language; lowered to `CoreExpr::MapNew`
  and `CoreExpr::SetNew`.
- **ACL `index(collection, index)` parser form** (Wave 22B): surface-level index
  access lowered to `CoreExpr::IndexGet`; missing `IndexGet` arm in
  `lower_core_expr_to_anf_local` fixed simultaneously.
- **`std.iter` contract clauses** (Wave 22A): `map`, `filter`, `fold`, and `traverse`
  v1 entries now carry `ContractClauses` (previously `None`).
- **Collection and time stdlib contract clauses** (Wave 21A): `list.*`, `map.*`,
  `set.*`, and `time.*` v1 function entries now carry `ContractClauses`.
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
  lockstep versioning, the changelog has a release heading before tagging, and
  migration compatibility metadata agrees with the implemented storage schema
  target.

### Tests / Conformance

- **Wave 19A** — ANF control-flow execution conformance suite (`RUNTIME-CTRL-*`):
  `If`, `Seq`, `Abort`, early-exit via `Seq+Abort`, `Return` proofs.
- **Wave 19B** — Data-structure execution conformance suite: `ListNew`, `TupleNew`,
  `RecordNew`, `CellNew/Get/Set`, `IndexGet`, `ForEach` round-trip proofs.
- **Wave 19C** — ACL source-level E2E expression conformance: arithmetic, comparison,
  string, boolean, and let-binding forms compiled through ACL → Core → ANF → WASM.
- **Wave 20B/C** — `MapNew`/`SetNew` WASM memory-layout proof via `read_memory_i64`;
  empty and multi-pair map/set ACL E2E tests.
- **Wave 20D** — Basic collection exec coverage for `list`, `map`, `set` including
  type-error paths for `map.insert` / `set.insert`.
- **Wave 21B** — ACL E2E pipeline tests for `list`, `tuple`, and `record` constructor
  and field-access forms.
- **Wave 21C** — WASM memory-layout conformance for `ListNew`, `TupleNew`, `RecordNew`
  (pointer, length, tag offsets verified via `read_memory_i64`).
- **Wave 22C** — ACL source-level iteration E2E: `foreach` (RUNTIME-ACL-FOREACH-1),
  `while` desugared form (RUNTIME-ACL-WHILE-7), `fold` with named function reference
  (RUNTIME-ACL-FOLD-1).
- **Wave 23A** — ACL inline lambda as `fold` reducer E2E (RUNTIME-ACL-FOLD-2,
  RUNTIME-ACL-FOLD-3): proves `lambda(...)` as the `func` arg of `fold(...)` through
  ACL parser → Core → ANF → WASM without a named binding.
- **Wave 23B** — `clock.now` epoch-ms E2E (RUNTIME-CLOCK-NOW-1): host clock result
  asserted to be a plausible epoch-ms value (> 1 000 000 000 000).
- **Wave 23C** — ACL record field update E2E (RUNTIME-ACL-RECORD-UPDATE-1/2): proves
  `update(record, field, value)` mutates the target field and leaves other fields
  intact through the full pipeline.
