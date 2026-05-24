# Wave operating model

This document captures the agreed implementation operating model for AIL hardening waves.
Use it as a checklist when planning and executing work in a worktree-per-wave workflow.

Related: [Maintainer playbook](codebase/maintainer-playbook.md), [Implementation blueprint](implementation-blueprint.md), [Codebase guide](CODEBASE-GUIDE.md), [Contract discipline](codebase/contract-discipline.md).

---

## Core principles

- **Parallel where isolated, sequential where shared.** Run 2–4 waves in parallel when they touch isolated areas (separate crates, separate scripts, docs-only). Keep package/CLI sequential unless the wave is docs-only or test-only in a clearly isolated path.
- **Smaller waves.** Target ~300–600 LOC per wave. Larger changes increase review surface and conflict risk without proportional value.
- **Contracts before code.** Predefine the deliverables: JSON output shape, exit codes, docs scope (target vs implemented), and required negative tests before writing any code.
- **Mandatory adversarial review.** Every wave gets a fresh review pass focused on claims, not effort. See review checklist below.
- **Merge only after checks pass.** No exceptions for "small" or "docs-only" changes.

---

## Lane matrix

Each lane names the crates and scripts it owns. Work in one lane is typically safe to parallelize with other lanes.

| Lane | Primary surface | Typical files |
|------|----------------|---------------|
| **Package** | Resolver, trust, signing, lockfile, advisory | `crates/ail-package/` |
| **Verify** | Solver diagnostics, report states, policy gates | `crates/ail-verify/` |
| **Compiler / native** | Lowering, codegen, artifact hash, source maps | `crates/ail-compiler/` |
| **Storage** | Snapshots, GC, retention, migration | `crates/ail-storage/` |
| **Context** | Slices, redaction, freshness, budget | `crates/ail-context/` |
| **Remote / coordinator** | Bundle integrity, signer identity, rebase | `crates/ail-remote/`, `crates/ail-coordinator/` |
| **Release / ops** | Preflight scripts, tag logic, changelog, VERSION | `scripts/`, `docs/release-policy.md` |
| **CLI** | Command surface, exit codes, `--json` output | `crates/ail-cli/` — **keep sequential with Package and Release/ops lanes** |
| **Docs** | Architecture, guides, playbooks | `docs/` — safe to parallelize unless editing shared anchor files |

> **CLI is a shared surface.** Multiple waves writing to `cli.rs` simultaneously is the most common cause of merge conflicts and logic regression. Serialize CLI-touching changes or explicitly partition sub-commands with no overlapping flag/handler ownership.

---

## Pre-wave contract checklist

Before starting implementation, write down (in the wave plan or PR description):

- [ ] **JSON shape** — If the wave produces or consumes JSON, define the exact keys, types, and error envelope. Do not infer it from existing output.
- [ ] **Exit codes** — List every exit code the wave introduces or changes. Verify alignment with `docs/tooling.md`.
- [ ] **Docs scope** — State whether docs changes describe _target design_, _implemented subset_, or both. Do not document target behavior as current.
- [ ] **Negative tests required** — List at least one negative or error-path test per new behavior unit (missing grant, invalid input, schema failure, marker conflict, etc.).
- [ ] **Lane isolation** — Confirm which lane(s) this wave touches and whether any parallel wave owns an overlapping file.

---

## Wave execution checklist

During implementation:

- [ ] Edit only the assigned worktree. Never write to the main branch worktree during a wave.
- [ ] Keep changes within the declared lane. If scope creeps into another lane, stop and create a separate wave.
- [ ] One crate owns each behavior unit. Do not duplicate logic across crates to avoid a dependency edge.
- [ ] Preserve metadata through all transforms: hashes, source maps, profile bindings, snapshot IDs.
- [ ] Classify all verification states explicitly (accepted, failed, assumed, runtime-checked, unsafe, unverified). Do not suppress uncertainty behind a success path.
- [ ] Keep human output and `--json` output in sync. Run both code paths in tests.

---

## Adversarial review checklist

Every wave must pass a fresh review pass that asks only:

- [ ] **Does the doc or code lie?** Does it claim something is implemented that is not? Does it promote a target-design statement into implemented behavior?
- [ ] **Is metadata preserved?** Hashes, source maps, artifact manifests, snapshot IDs — are they intact through the new code path?
- [ ] **Does it create a bypass?** Does the new code path skip verification, capability checks, or policy gates that the normal path enforces?
- [ ] **Does it produce contradictory JSON?** Can two runs of the same command with the same input produce JSON with different shapes or conflicting field values?
- [ ] **Does it overclaim docs?** Does the updated doc describe behavior that has not been implemented or tested?
- [ ] **Does it leak Debug representations?** Is any `{:?}` format string reachable in user-visible or machine-readable output?
- [ ] **Is shell quoting correct?** Any variable interpolated into a JSON string or `awk -v` context must handle tabs, newlines, and control characters. Test with adversarial inputs.

A wave is blocked if any of the above is YES. Fix and re-review before merge.

---

## Merge gate

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

For docs-only waves: `git diff --check` is the floor — run the relevant or full suite whenever practical; CI will.

---

## Anti-patterns to avoid

These are failure modes observed in previous waves. Do not repeat them.

| Anti-pattern | Why it fails | Fix |
|---|---|---|
| Multiple waves writing `cli.rs` in parallel | Merge conflicts; silent behavioral clobbering | Serialize CLI-lane changes or partition ownership by sub-command |
| Doc overclaiming implemented behavior | Misleads agents and reviewers; triggers adversarial review block | Add `Implementation Status` note; state "target design" or "implemented subset" explicitly |
| `Debug` enum format in user-visible output | Leaks internal type names; breaks JSON consumers | Implement `Display` or serialize explicitly; test with `--json` |
| Shell JSON escaping via string interpolation | Tabs/newlines produce invalid JSON; `awk -v` receives pre-expanded corrupt string | Escape all control chars before interpolation; validate with `jq .` in smoke tests |
| Metadata not preserved through transforms | Hash chains break; source maps become stale; snapshots lose identity | Thread metadata explicitly; add roundtrip tests that assert hash/map equality |
| Writing to the main worktree during a wave | Contaminates main with unreviewed changes; makes recovery expensive | Strict rule: edit only the assigned worktree until merge approval |
| Version/changelog prefix lookup before input validation | Invalid VERSION crashes `awk` before JSON escaping can run | Validate VERSION format first; reject with structured error before reaching awk |
| Broad ad hoc verbs in ChangeSet operations | Increases conflict surface; harder to audit and replay | Prefer small, repairable, targeted operations |

---

## Wave size guidance

| Wave size | LOC range | Use when |
|-----------|-----------|---------|
| Nano | < 100 | Single behavior fix, docs correction, single test |
| Small | 100–300 | Single subsystem, well-understood scope |
| Standard | 300–600 | Cross-crate feature with defined contract |
| Large | 600–1000 | Treat as two waves if scope permits |
| Oversized | > 1000 | Split unconditionally; review becomes ineffective |

Prefer standard or smaller. The cost of an oversized wave is not just merge risk — it is adversarial review becoming perfunctory.
