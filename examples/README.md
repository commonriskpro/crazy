# AIL Public CLI Examples

These examples exercise the current public AIL CLI surface from a clean checkout. They are intentionally small: v0.1 verifies the core toolchain foundation, and the current v0.2 slices cover narrow `Text` and `print`/`log.write` execution paths. AIL is not general-purpose yet.

## Quick Check

From the repository root:

```sh
./scripts/examples-smoke.sh
```

The script builds or reuses the public CLI, creates a fresh temporary AIL project for each example, applies the ACL file, compiles to WASM, and runs the target function.

## Text Hello

`examples/text-hello.acl` creates `fn.hello`, a pure `Text` function.

Manual flow from the repository root without writing `.ail/` into the checkout:

```sh
cargo build -p ail-cli
REPO="$(pwd)"
WORKDIR="$(mktemp -d)"
cd "$WORKDIR"
"$REPO/target/debug/ail" init
"$REPO/target/debug/ail" --json change --file "$REPO/examples/text-hello.acl"
CHANGE_ID="<change-id-from-change-output>"
"$REPO/target/debug/ail" --json verify "$CHANGE_ID"
"$REPO/target/debug/ail" --json apply "$CHANGE_ID" --yes
"$REPO/target/debug/ail" --json compile --profile dev --target wasm
"$REPO/target/debug/ail" --json run --profile dev --target wasm fn.hello
```

Expected run result includes:

```json
"invoke_result":"result: Hello, world!"
```

## Print With log.write

`examples/print-log-write.acl` creates `log.write`, creates `fn.print_hello`, and grants `log.write` to that function in the Semantic Graph. Runtime execution still requires an explicit invocation grant.

Manual flow from the repository root without writing `.ail/` into the checkout:

```sh
cargo build -p ail-cli
REPO="$(pwd)"
WORKDIR="$(mktemp -d)"
cd "$WORKDIR"
"$REPO/target/debug/ail" init
"$REPO/target/debug/ail" --json change --file "$REPO/examples/print-log-write.acl"
CHANGE_ID="<change-id-from-change-output>"
"$REPO/target/debug/ail" --json verify "$CHANGE_ID"
"$REPO/target/debug/ail" --json apply "$CHANGE_ID" --yes
"$REPO/target/debug/ail" --json compile --profile dev --target wasm
"$REPO/target/debug/ail" --json run --profile dev --target wasm --grant log.write fn.print_hello
```

Expected run result includes:

```json
"output":["Hello, world!"]
```

Each ACL file uses `base 0`, so run them in separate fresh projects or use `./scripts/examples-smoke.sh`.
