# Maintainer playbook

Use these checklists before changing an AIL subsystem. The goal is to keep target design, implemented subset, tests, and docs aligned.

## Status lens

| Lens | Reality |
|------|---------|
| Target design | Maintainer work should preserve the full AI-native architecture: graph source of truth, verified ChangeSets, explicit capabilities, profile-bound artifacts. |
| Implemented subset | Most changes touch milestone implementations that are intentionally narrower than the full docs. Update status notes when scope changes. |
| Historical context | Historical draft material is not the place to document new behavior; update canonical docs first. |

## General checklist

- Confirm whether the change affects target design, implemented behavior, or both.
- Keep crate boundaries directional; avoid dependency cycles by checking existing `src/lib.rs` contracts.
- Add or update tests in the crate that owns the behavior.
- Update `Implementation Status` notes when implementation scope changes.
- Update [Implementation blueprint](../implementation-blueprint.md) if the change advances a validation milestone.
- Update [Risks](../risks.md) or [Decisions register](../open-questions.md) if the change closes or creates validation work.

## Core IR / graph

- Check `docs/core-ir.md`, `docs/type-system.md`, and `crates/ail-core/src/semantic_graph.rs`.
- Preserve stable graph identity semantics; do not make text files the source of truth.
- Add roundtrip or validation tests for new node, edge, effect, contract, or reference types.
- Check compiler, verifier, context, and storage consumers for new graph fields.

## ChangeSet / ACL

- Check `docs/change-language.md` and `crates/ail-change/src/`.
- Keep parser, canonicalizer, op schema, and apply behavior aligned.
- Prefer small, repairable operations over broad ad hoc verbs.
- Add parser, canonicalization, conflict, and apply tests for new operations.
- If persisted ACL shape changes, add migration or compatibility notes.

## Verifier / contracts

- Check `docs/verification.md`, `docs/type-system.md`, and `crates/ail-verify/src/`.
- Classify every claim into explicit report states; do not hide uncertainty behind success.
- Keep profile and policy gates visible in reports.
- Add tests for accepted, failed, assumed, runtime-checked, unsafe, and unverified paths where relevant.
- If solver behavior changes, update risk/validation notes.

## Compiler / backend

- Check `docs/compiler.md` and `crates/ail-compiler/src/`.
- Preserve deterministic hash chains, source maps, artifact manifests, and profile binding.
- Keep verifier decisions authoritative; compiler should not silently promote weaker verification.
- Add lowering/codegen tests and artifact hash tests for new executable surface.
- Mark any native or WASM subset explicitly if it is not full parity.

## Runtime / capabilities

- Check `docs/runtime.md` and `crates/ail-runtime/src/`.
- Preserve deny-by-default capability behavior.
- Keep grants, handlers, schema checks, audit, replay, and rollback semantics explicit.
- Add tests for denied capability, missing grant, schema failure, audit output, and profile-bound execution.
- Treat ABI or memory-layout changes as high-risk and document migration impact.

## Storage

- Check `docs/storage.md`, `docs/migration-guide.md`, and `crates/ail-storage/src/`.
- Preserve content addressing, immutable snapshots, append-only semantic history, and policy-driven GC.
- Add migration tests for persisted schema changes.
- Check memory, file, and Postgres behavior when touching store contracts.
- Update release/migration docs for schema or compatibility changes.

## Context Server

- Check `docs/context-server.md` and `crates/ail-context/src/`.
- Keep structured data authoritative; summaries are helpers only.
- Bind responses to snapshot/hash and preserve freshness/redaction semantics.
- Add tests for budget limits, stale context, missing nodes, redaction, and deterministic summaries.
- Do not imply network transport unless it exists.

## Packages

- Check `docs/packages.md` and `crates/ail-package/src/`.
- Preserve the rule that import does not grant capabilities.
- Keep trust level, signing, lockfile, advisory, yanking, and compatibility behavior explicit.
- Add resolver, policy, trust gate, signing, or registry tests for changed behavior.
- Avoid dependencies from `ail-package` upward into verifier, runtime, or compiler.

## Remote / coordinator

- Check `docs/remote.md`, `docs/coordinator.md`, `crates/ail-remote/src/`, and `crates/ail-coordinator/src/`.
- Preserve signer identity as cryptographic authority; labels are metadata only.
- Keep remote primitives transport-agnostic unless a real transport is implemented.
- Add tests for signature failure, allowlist rejection, bundle integrity, stale base, rebase, and conflict classification.

## CLI / tooling

- Check `docs/tooling.md` and `crates/ail-cli/src/`.
- Keep human output and `--json` machine output aligned.
- Make local persistence behavior explicit: memory fallback, file `.ail/`, or Postgres.
- Add CLI tests for command behavior, exit codes, and JSON shape.
- Update docs if a command moves from target workflow to implemented durable behavior.

## Docs

- New guides must distinguish target design, implemented subset, and historical context where relevant.
- Prefer short entry points and maps over another giant document.
- Keep links relative and update references when moving files.
- Do not document historical draft content as current behavior.
- Suggested checks: `git diff --check -- '*.md'` and a link check if available.
