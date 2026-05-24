# Contract discipline

> One owner, one implementation. Duplicate policy is a bug.

This guide names the owner for each behavioral contract and states the rules that prevent duplication and divergence. Read this before adding behavior that touches JSON output, status strings, policy gates, or cross-crate semantics.

Related: [Maintainer playbook](maintainer-playbook.md), [Reference map](reference-map.md), [Wave operating model](../wave-operating-model.md).

---

## Contract ownership

Each behavioral contract has exactly one owner. Other crates call the owner; they do not re-implement the logic.

| Contract | Owner crate | Owner module(s) | Must NOT duplicate in |
|---|---|---|---|
| Package trust level (Untrusted / Audited / Verified) | `ail-package` | `trust.rs` | verifier, CLI, scripts |
| Package policy gates (capability, advisory, yank) | `ail-package` | `policy.rs` | CLI, verifier, remote |
| Package verification (hash, sig, report, repro evidence) | `ail-package` | `verification.rs` | CLI command handlers |
| Verify profile policy gates | `ail-verify` | `policy.rs` | `ail-package`, CLI |
| Verify report state classification | `ail-verify` | `report.rs` | CLI output handlers |
| CLI output shaping (JSON envelope, human tables, exit codes) | `ail-cli` | `output.rs` | Individual command arms in `cli.rs` |
| Release JSON metadata | `scripts/release-preflight.sh` | (shell script) | any Rust crate |
| Storage CAS identity (hash, object ID) | `ail-storage` | `object.rs` | `ail-cli`, `ail-context` |

---

## JSON and status string rules

**Rule 1: Never use `{:?}` (Debug) in user-visible or machine-readable output.**

`Debug` format leaks internal Rust type names, changes across compiler versions, and breaks JSON consumers silently on refactors.

Fix a violation:
1. Implement `Display` on the enum or struct.
2. Or annotate with `#[serde(rename = "...")]` / `#[serde(rename_all = "snake_case")]`.
3. Delete every `format!("{:?}", ...)` or `write!(f, "{:?}", ...)` that can reach user output or `--json` output paths.

This check appears explicitly in the adversarial review checklist. Any `{:?}` reachable through a CLI path is a review blocker.

**Rule 2: Stable string literals live in the owner module. Callers must not hardcode copies.**

If `ail-package::trust` produces `"verified"` in JSON, `ail-cli` must call `trust.to_string()` — not write `"verified"` directly. When the owner renames the variant, callers update automatically; hardcoded copies silently diverge.

---

## Dependency direction

Crate dependencies must flow in one direction. Cycles are build errors.

```
ail-core
  ↑
ail-change  ail-package  ail-storage  ail-context
ail-remote  ail-coordinator  ail-stdlib
  ↑
ail-verify
  ↑
ail-compiler  ail-runtime
  ↑
ail-cli
```

Hard rules:
- `ail-package` must NOT import `ail-verify`, `ail-runtime`, or `ail-compiler`.
- `ail-verify` may import `ail-package` but must NOT import `ail-runtime` or `ail-compiler`.
- `ail-cli` is the broad orchestrator; it imports across all layers and that is its role.

When a new cross-crate dependency is needed: confirm it does not create a cycle and does not push policy logic upstream.

---

## Docs must not duplicate complex semantics

If a doc section needs to explain how package trust verification works, link to `docs/packages.md` — do not re-explain the algorithm.

Duplication means two sources can drift. When they drift, agents and reviewers read the stale copy.

Rules:
- A doc explaining a subsystem's behavior must link to the canonical doc for that subsystem.
- A doc describing a multi-subsystem flow (e.g. install → verify → compile) may summarize the flow, but must link each step to its owner doc.
- Do not copy verification report state tables, trust level definitions, or policy gate rules into docs that are not the owner. Link instead.

---

## Negative tests for every blocker-prone contract

For every behavior unit that can block a verification pass, release gate, or preflight check, write at least one negative test.

The negative test must assert on:
- The error type or exit code.
- The human output (where applicable).
- The `--json` output (where the command supports `--json`).

**Examples of blocker-prone contracts requiring negative tests:**

| Contract | Negative scenario |
|---|---|
| Package trust gate | `TrustLevel::Untrusted` → install blocked |
| Advisory gate | flagged advisory → install fails with advisory ID in output |
| Yank gate | yanked version → resolve fails, error includes version |
| Verify profile gate | `critical` profile + solver diagnostic → report blocked |
| CLI `--json` output | command fails → JSON envelope has `"status": "error"` with message |
| Release preflight | bad `VERSION` format → structured error before any shell expansion |
| Reproducible evidence gate | Verified package missing evidence → `preflight` hard-fails |

A wave that introduces a new policy gate without a negative test is incomplete. Do not merge it.
