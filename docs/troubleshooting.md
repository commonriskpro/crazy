# Troubleshooting AIL CLI workflows

<!-- Status: Implemented subset. This guide describes diagnostics that are covered by current CLI code or tests; it is not a production-readiness claim. -->

Use this when the validation-stage CLI stops before `apply`, `compile`, or `run`. The right move is usually to preserve the failed output, identify the gate that rejected the workflow, and fix that gate instead of bypassing it.

## Quick path

1. Re-run the command with the same project state and copy the exact stderr or JSON field.
2. Match the message in the table below.
3. Run the next action, then re-run the original command.

## Common diagnostics

| Symptom | What it means | Next action | Evidence in repo |
|---|---|---|---|
| `capability denied: log.write` | The Semantic Graph or runtime grant does not authorize `log.write`. AIL is deny-by-default for effects. | Add the graph grant with `op grant target=fn.print_hello capability=log.write`, apply it, then run with `--grant log.write`. | `crates/ail-cli/tests/cli_subcommands.rs` |
| `no verification report` | `ail apply` was run before an accepted `ail verify` report was persisted for that ChangeSet. | Run `ail verify <change-id>` and only apply after the report is accepted. | `crates/ail-cli/tests/repair_loop.rs` |
| Profile mismatch mentioning both profiles, such as `dev` and `prod` | The ChangeSet was verified under one profile and applied under another. | Re-run `ail verify <change-id> --profile <profile>` using the same profile you will apply with. | `crates/ail-cli/src/tests/apply.rs` |
| `rebase required: current snapshot is <id>` or JSON `"code": "rebase_required"` | The ChangeSet base snapshot is stale. The live graph moved after the ChangeSet was created or verified. | Rebase or recreate the ChangeSet against the current snapshot, then verify again before apply. | `crates/ail-cli/src/workflow_commands.rs` |
| `preflight failed: ...` | Runtime preflight rejected the compiled artifact or metadata before execution. | Recompile the current graph/profile and check artifact/profile/hash inputs before running again. | `crates/ail-cli/src/error.rs`, `crates/ail-cli/src/tests/apply.rs` |
| `solver 'z3' requires the z3-solver cargo feature` | The CLI was compiled without the optional Z3 solver backend. | Use `--solver simple`, or rebuild the CLI with `--features z3-solver` when you intentionally need Z3. | `crates/ail-cli/src/workflow_commands.rs` |
| `unknown solver '<name>'; supported values: simple, z3` | The solver name is not recognized. | Use `--solver simple` or `--solver z3`. | `crates/ail-cli/src/workflow_commands.rs` |
| `native linked execution not supported yet` | `ail run --target native` is intentionally blocked. Native compile emits an object artifact, not a linked executable. | Use `ail run --target wasm`, or use native inspection/link commands where supported. | `crates/ail-cli/src/run_commands.rs`, `docs/tooling.md` |
| `not found: change-id not found: ...` | The requested ChangeSet does not exist in the local store. | Recreate or fetch the ChangeSet, then repeat verify/apply. | `crates/ail-cli/src/error.rs`, `crates/ail-cli/src/remote_commands.rs` |
| `not found: snapshot not found: ...` | The requested snapshot is not in the local store. | Inspect project status/history and use an existing snapshot id. | `crates/ail-cli/src/branch_commands.rs`, `crates/ail-cli/src/inspect_commands.rs` |
| `not found: capability not found: ...` | The capability is not declared or registered where the CLI is looking. | Declare the capability in the graph or inspect available package/runtime capabilities. | `crates/ail-cli/src/inspect_commands.rs` |

## Capability failures

Capability errors are not noise; they are the safety model doing its job. If a function prints, writes, calls time, or reaches any host effect, the graph must declare the capability and the runtime invocation must grant it explicitly.

For the current hello-world capability path, the implemented flow is:

```sh
ail-cli -- change --file print-hello.acl --json
ail-cli -- verify "$PRINT_CHANGE_ID"
ail-cli -- apply "$PRINT_CHANGE_ID" --yes
ail-cli -- run --profile dev --target wasm --grant log.write fn.print_hello
```

If you omit the runtime grant, the expected failure is `capability denied: log.write`.

## Verify/apply failures

`apply` is intentionally stricter than "a ChangeSet exists." It needs an accepted verification report for the same ChangeSet and compatible profile. This protects the Semantic Graph from stale or unverified AI-authored changes.

Use this repair sequence:

```sh
ail-cli -- verify "$CHANGE_ID" --profile dev
ail-cli -- apply "$CHANGE_ID" --policy dev --yes
```

If the graph moved after the ChangeSet was created, repair the `rebase_required` condition first, then verify again. Do not treat an old verification report as reusable across graph state changes.

## Solver selection

`simple` is always available. `z3` is optional and requires the CLI to be compiled with the `z3-solver` cargo feature. If you are not specifically validating solver behavior, prefer:

```sh
ail-cli -- verify "$CHANGE_ID" --solver simple
```

## Native target status

`ail compile --target native` can emit a native object artifact. `ail run --target native` is different: linked native execution is not supported yet, so the CLI returns `native linked execution not supported yet` instead of silently falling back to WASM.

For executable validation today, use:

```sh
ail-cli -- compile --profile dev --target wasm
ail-cli -- run --profile dev --target wasm fn.hello
```

## When to update this guide

Update this document whenever a CLI diagnostic changes, a repair option changes shape, or a new user-facing failure becomes part of the supported workflow. Keep the wording tied to tests or source files, not aspiration.
