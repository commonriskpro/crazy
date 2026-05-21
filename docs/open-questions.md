# Open questions register

This register consolidates serious unknowns from the full design. Questions here should be tracked as research items, not left hidden in prose.

## Core IR / type system

1. Exact formal semantics of Core IR.
2. How powerful refinement predicates can be before solver performance collapses.
3. Whether effect handlers need algebraic effect semantics or the simpler handler model is enough.
4. How far resource ownership should go toward linear/affine type theory.
5. Exact model for `Dyn<Interface>` + contracts + effects.

## Compiler

1. Exact ANF representation and serialization.
2. Custom SSA vs Cranelift/LLVM IR.
3. WASM ABI layout for records, variants, `Result`, `Option`, handles.
4. Memory management strategy for WASM.
5. Translation validation requirements for `prod`/`critical`.

## Runtime

1. Binary encoding for `host.call` payloads: CBOR, MessagePack, canonical JSON, or custom.
2. Whether to use WASI underneath or hide it fully behind host ABI.
3. Handler execution isolation model.
4. Distributed tracing standard across capability calls.
5. Sync vs async capability call typing.

## Storage

1. Concrete backend: embedded DB, CAS filesystem, object DB, or hybrid.
2. Hash algorithm and canonical serialization.
3. Distributed collaboration protocol for graph branches.
4. Default local retention policy.
5. Protected audit archive strategy.

## Context Server

1. Exact query syntax: line-oriented DSL, RPC JSON, or both.
2. How summaries are generated and checked against structured data.
3. Whether context slices should be signed for distributed agents.
4. Default budgets by model/context size.
5. Safe exposure policy for runtime/audit context.

## Packages

1. Registry protocol and package signing.
2. Whether verified packages require reproducible builds.
3. Federated trust across organizations.
4. Local proof checking vs trusted remote verification.
5. Package yanking while preserving old builds.

## Standard library

1. How large v1 stdlib should be.
2. Whether database capability is stdlib core or an official package.
3. Exact crypto safe defaults.
4. Async runtime placement: `std.concurrent` vs `std.runtime`.
5. Stdlib versioning independent from language/Core IR.

## Tooling

1. Final CLI name.
2. Whether interactive shell is required for first full product release.
3. How editor edits convert into ChangeSets.
4. Default human approval UX.
5. Whether local experiments can disable persistent graph storage.

## Product / implementation framing

1. How to sequence implementation while preserving full-product scope.
2. How to validate high-risk subsystems without presenting them as scope-reducing MVPs.
3. What parts must be built before the language is usable enough for dogfooding.
