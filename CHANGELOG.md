# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-26

### Fixed

- **`clock.now` epoch-ms capability boundary** (Wave 23B): replaced silent-truncating
  `.as_millis() as i64` cast in `ClockHandler::handle` with a checked `try_into()`
  conversion. A pathological system clock exceeding `i64::MAX` now surfaces a
  `HostError::Custom` instead of silently wrapping.
- **`ClockHandler` returns epoch-milliseconds** (Wave 23B): handler previously called
  `.as_secs()` (epoch-seconds); corrected to `.as_millis()` to match the `clock.now`
  contract.
- **`ClockHandler` rejects unknown operations** (Wave 24C): any operation string other
  than `"now"` previously returned epoch-ms silently. `handle` now matches on the
  operation name and returns `HostError::Custom(format!("unknown clock operation: {op}"))` for
  unrecognised ops, enforcing the operation contract.
- **`uses_var` silent miss on `ShortCircuitAnd`/`ShortCircuitOr` left atom** (Wave 24D):
  both arms only inspected the right sub-expression, so the dead-let pass eliminated
  bindings whose sole use was as the left operand of a short-circuit expression. The
  missing left-atom check is now added; `let x = true in or(x, abort("dead"))` no
  longer mis-fires.
- **`FixedClock` rejects unknown operations** (Wave 25B): `FixedClock::handle` previously
  ignored the operation parameter and always returned the pinned timestamp. Any operation
  string other than `"now"` now returns
  `HostError::Custom(format!("unknown FixedClock operation: {op}"))`, matching the
  contract enforcement added to `ClockHandler` in Wave 24C.
- **Variant well-known/user tag collision** (Wave 25C): user-defined variant tags were
  allocated starting at `0`, colliding with reserved well-known IDs `0` (None/Ok) and
  `1` (Some/Err). `WasmCodegenCtx::next_variant_tag` now initialises to `2` so user
  tags can never alias the well-known set.
- **`uses_var` silent skip of `ForEach` body when binding is empty** (Wave 25D):
  the `!binding.is_empty()` guard in the `ForEach` arm of `uses_var` suppressed body
  scanning whenever the loop variable was an empty string, causing dead-let elimination
  to incorrectly remove variables used only inside the body. Guard removed; body is now
  always scanned regardless of binding length.
- **`SeededRandom` rejects unknown operations** (Wave 26B): `SeededRandom::handle`
  previously ignored the `operation` parameter and always returned random bytes. Any
  operation string other than `"next_u64"` now returns
  `HostError::Custom(format!("unknown SeededRandom operation: {operation}"))`, matching
  the contract enforcement added to `FixedClock` in Wave 25B.
- **Fold preflight rejects capture-free wrong-arity Lambda reducer** (Wave 26C): a
  capture-free `Lambda` with `params.len() != 2` used as a `Fold` reducer previously
  fell through to the non-hoistable `else` branch and silently dispatched to `table[0]`
  at runtime, causing a type-mismatch trap instead of a deterministic compile error.
  `has_fold_with_uncaptured_wrong_arity_reducer` now runs as a pre-emit preflight and
  returns `CompileError::UnsupportedWasmConstruct("FoldWithUncapturedWrongArityReducer")`
  before any code is generated. The diagnostic was also renamed from
  `FoldWithNonHoistableReducer` to `FoldWithUncapturedWrongArityReducer` for precision.
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
- **Wave 24A** — ACL record offset / 3-field conformance (RUNTIME-ACL-RECORD-UPDATE-3,
  RUNTIME-ACL-RECORD-3FIELD-1, RUNTIME-ACL-RECORD-3FIELD-UPDATE-1): verifies 8-byte
  stride formula at offset 0, 8, and 16; proves `FieldUpdate` is field-surgical when
  the target has neighbours on both sides. Existing neighbour test split into two
  single-assertion tests for precision.
- **Wave 24B** — ACL user-defined variant tag E2E (RUNTIME-ACL-VARIANT-USER-1/2/3):
  proves user-defined discriminant assignment (first-encounter order, starting at 0),
  multi-tag dispatch, and wildcard fallthrough through ACL parser → Core → ANF → WASM.
- **Wave 24C** — `ClockHandler` operation contract (RUNTIME-CLOCK-OP-CONTRACT-1/2):
  CLOCK-OP-CONTRACT-1 asserts `clock.now` returns epoch-ms in `[1e12, 1e13)` — distinct
  from RUNTIME-CLOCK-NOW-1 (Wave 23B) in that it adds an upper-bound check and exercises
  the handler operation-dispatch contract directly rather than through the ACL pipeline;
  CLOCK-OP-CONTRACT-2 asserts `clock.elapsed` returns `HostError::Custom` identifying
  the unknown operation.
- **Wave 24D** — ACL boolean short-circuit E2E (RUNTIME-ACL-AND-1/2, RUNTIME-ACL-OR-1/2):
  exercises `and()`/`or()` through the full ACL → ANF → `ShortCircuitAnd`/`ShortCircuitOr`
  → WASM pipeline. AND-2 and OR-1 use `abort("dead")` as the right operand to prove
  non-evaluation by absence of trap. Adds `abort(msg)` form to `expr_parser`.
  - **unit tests** — `uses_var` coverage for `ShortCircuitAnd`/`Or`
    (OPT-USESVAR-AND-1/2, OPT-USESVAR-OR-1/2) and `abort()` parser coverage
    (PARSE-ABORT-1/2/3: well-formed, non-literal arg rejected, zero-arg rejected).
- **Wave 25B** — `FixedClock` operation replay contract (REPLAY-FIXEDCLOCK-OP-CONTRACT-1/2;
  `REPLAY-` prefix reflects placement in `replay_tests.rs`):
  OP-CONTRACT-1 (`fixed_clock_now_op_returns_timestamp`) asserts `"now"` returns the
  configured pinned timestamp; OP-CONTRACT-2 (`fixed_clock_unknown_op_returns_error`)
  asserts any other operation returns `HostError::Custom` naming the unknown op.
- **Wave 25C** — Variant tag collision conformance (RUNTIME-ACL-VARIANT-COLLISION-1/2/3):
  COLLISION-1 proves user tag `Active` (now assigned `2`) does not fire the `None` arm
  (discriminant `0`); COLLISION-2 proves `variant(None)` and `none()` both resolve to
  discriminant `0` via the well-known path; COLLISION-3 proves well-known IDs
  `None`=0, `Ok`=0, `Some`=1, `Err`=1 remain stable after the reservation change.
- **Wave 25D** — `uses_var` unit coverage for collection exprs
  (OPT-USESVAR-INDEXGET-1/2/3, OPT-USESVAR-MAPNEW-1/2/3, OPT-USESVAR-SETNEW-1/2,
  OPT-USESVAR-FOREACH-1/2/3, OPT-USESVAR-FOREACH-SHADOW-1): each form tested for
  true-hit, false-miss, and unrelated-variable scenarios. Fixed narrow bug in the
  `ForEach` arm: the `!binding.is_empty()` guard silently suppressed body scan when
  the loop variable was an empty string; guard removed.
- **Wave 26B** — `SeededRandom` operation contract
  (`seeded_random_unknown_op_returns_error` in `replay_tests.rs`): asserts any
  operation string other than `"next_u64"` returns `HostError::Custom` naming the
  handler and the unknown op. Companion positive-path test
  `seeded_random_next_u64_returns_8_bytes` asserts `"next_u64"` returns exactly
  8 bytes of random data.
- **Wave 26C** — Fold wrong-arity preflight guard unit tests: proves 1-param, 3-param,
  and 0-param capture-free Lambda reducers in `Fold` return
  `UnsupportedWasmConstruct("FoldWithUncapturedWrongArityReducer")` before code
  generation; negative cases confirm 2-param capture-free (hoistable) and 2-param
  captured Lambda are not rejected by the guard. Transitive-alias case
  (`fold_with_transitive_alias_of_wrong_arity_reducer_returns_uncaptured_wrong_arity_error`)
  proves the guard propagates membership through `Var` aliases so an aliased
  wrong-arity reducer is caught even when referenced indirectly.
- **Wave 26D** — ACL `not()` and `mod()` E2E conformance (RUNTIME-ACL-NOT-1/2/3,
  RUNTIME-ACL-MOD-1/2): proves `not(true)`, `not(false)`, `not(eq(1,2))`,
  `mod(10,3)`, and `mod(10,2)` through ACL parser → Core → ANF → WASM → wasmtime
  execution.
