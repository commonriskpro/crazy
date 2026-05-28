# Getting started with AIL

<!-- Status: Implemented subset. This tutorial follows CLI paths covered by `crates/ail-cli/tests/cli_subcommands.rs`; it is not a production-readiness claim. -->

This guide shows two current validation-stage paths:

1. the new `.ail` source-file slice for direct run/test;
2. the lower-level ACL ChangeSet workflow for verification/apply.

## Quick path

Run from a checkout of this repository:

```sh
AIL_REPO="$(pwd)"
PROJECT_DIR="$(mktemp -d)"
cd "$PROJECT_DIR"

cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- init
```

## Smallest `.ail` source run

Write source code directly, without authoring ACL:

```sh
cat > main.ail <<'EOF'
fn main() -> Int = add(20, 22)
fn add_pair(x: Int, y: Int) -> Int = add(x, y)
fn with_local() -> Int {
  let base = add(20, 20)
  return if gt(base, 40) { add(base, 2) } else { 0 }
}
test main_addition = eq(add_pair(20, 22), 42)
EOF

cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- run --file main.ail
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- run --file main.ail fn.add_pair 20 22
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- run --file main.ail fn.with_local
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- test --file main.ail
```

Expected output includes:

```txt
result: 42
PASS test.main_addition
```

This is the first source-language dogfooding slice. It supports zero-argument
functions, typed scalar parameters, runtime arguments, block bodies with
`let` statements, source `if/else` expressions, and simple tests, then lowers
through the real graph → Core IR → ANF → WASM → runtime path.

## ACL ChangeSet workflow

Create a tiny text-returning function:

```sh
cat > hello-text.acl <<'EOF'
change hello_text
author getting-started
description text return hello world
base 0
op create_function id=fn.hello return=Text body=let(s, "Hello, world!", s)
end
EOF
```

Submit the ChangeSet and capture its canonical id:

```sh
CHANGE_ID="$(
  cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- \
    change --file hello-text.acl --json |
  python3 -c 'import json,sys; data=json.load(sys.stdin)["data"]; print(data.get("change_id") or data["canonical_change"]["change_id"])'
)"
```

Verify, apply, compile, and run:

```sh
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- verify "$CHANGE_ID"
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- apply "$CHANGE_ID" --yes
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- compile --profile dev --target wasm
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- run --profile dev --target wasm fn.hello
```

Expected human output includes:

```txt
result: Hello, world!
```

## Capability-backed output

Printing is intentionally capability-gated. This ChangeSet declares `log.write`,
grants it to the function in the graph, and still requires an explicit runtime
grant at execution:

```sh
cat > print-hello.acl <<'EOF'
change print_hello
author getting-started
description print hello world
base 0
op create_capability id=log.write
op create_function id=fn.print_hello return=Int body=print("Hello, world!")
op grant target=fn.print_hello capability=log.write
end
EOF

PRINT_CHANGE_ID="$(
  cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- \
    change --file print-hello.acl --json |
  python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["canonical_change"]["change_id"])'
)"

cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- verify "$PRINT_CHANGE_ID"
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- apply "$PRINT_CHANGE_ID" --yes
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- compile --profile dev --target wasm
cargo run --manifest-path "$AIL_REPO/Cargo.toml" -p ail-cli -- run --profile dev --target wasm --grant log.write fn.print_hello
```

Expected output includes:

```txt
output:
Hello, world!
result: 0
```

Without `--grant log.write`, `ail run` should fail with a capability-denied
diagnostic. That is intentional: external effects are explicit.

## What this proves

| Step | Current proof |
|------|---------------|
| Project lifecycle | `ail new` creates a starter project with `main.ail`, starter ACL, and `.ail/` metadata; `ail init` creates a local `.ail` store inside an existing directory. |
| Source-language slice | `run --file main.ail` and `test --file main.ail` execute minimal source declarations without requiring users to write ACL. |
| AI-native write path | ACL text becomes a canonical ChangeSet id before apply. |
| Verification-first flow | `verify` runs before `apply`; `apply --yes` is explicit automation. |
| Runtime execution | `compile --target wasm` plus `run` executes the accepted graph snapshot. |
| Capability boundary | `print(...)` requires both graph permission and runtime grant. |

## Current limits

- This is a validation milestone, not a production-ready language workflow.
- `ail new` is validation-stage scaffolding: it creates `.ail/` metadata, `main.ail`, and a starter ACL, but not a full package/editor project template yet.
- `.ail` source support is narrow: functions, typed scalar parameters, runtime arguments, block-local `let` statements, single-line `if/else` expressions, and simple tests only. Broader parser syntax, modules, imports, source formatter, and source LSP are still roadmap work.
- First `cargo run` may build the Rust workspace before invoking the CLI.

## Next step

Use [Maturity model](maturity-model.md) to understand what evidence is still
required before AIL can claim a broader language experience.
