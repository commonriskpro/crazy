# AIL language reference

<!-- Status: Implemented subset. This reference documents the current ACL and expression surface that is backed by parser, canonicalizer, compiler, CLI, or smoke-test evidence. It is not a production-readiness claim. -->

AIL programs are authored through Semantic Graph changes. Today, the user-facing language surface is the AI Change Language (ACL): a line-oriented ChangeSet format that creates graph nodes, attaches metadata, verifies the change, and lets the CLI compile/run the accepted graph.

## Quick path

1. Use ACL to describe graph changes, not as the permanent source of truth.
2. Include `author`, `base`, and one or more `op` lines.
3. Run `change -> verify -> apply` before compile/run.
4. Check [Getting started](getting-started.md) for a runnable walkthrough and [Troubleshooting](troubleshooting.md) when a gate rejects the workflow.

## What is stable enough to document now

| Surface | Current status | Evidence |
|---|---|---|
| ChangeSet document shape | Implemented subset. | `crates/ail-change/src/parser.rs` |
| ACL op verb families | Implemented subset with prefix mapping. | `crates/ail-change/src/parser_tests.rs` |
| Required op arguments | Partial schema validation. Unknown/unconstrained accepted verbs can still canonicalize to no-op if they lack materialized payload support. | `crates/ail-change/src/op_schema.rs`, `crates/ail-change/src/canonical_ops.rs` |
| Expression bodies | Implemented subset parsed by the compiler expression parser when graph nodes carry inline `body=` expressions. | `crates/ail-compiler/src/expr_parser_tests.rs` |
| Capability-backed printing | Implemented subset through `print("...")` -> `log.write`. | `crates/ail-cli/tests/cli_subcommands.rs` |

## ChangeSet document

Minimal shape:

```txt
change <name>
author <name>
base <snapshot-id>
description <text>
op <verb> key=value key=value
end
```

Supported top-level metadata directives include:

| Directive | Meaning |
|---|---|
| `author <name>` | Required author metadata. |
| `base <u64>` or `base snapshot_<u64>` | Required base snapshot id. |
| `description <text>` | Human-readable change description. |
| `intent "<text>"` | Description-style intent text. |
| `language acl/<version>` or `acl_version <version>` | ACL version metadata. |
| `op_schema`, `graph_schema`, `core_ir_schema`, `diagnostics_schema`, `verification_schema` | Optional schema-version metadata. |
| `depends_on`, `supersedes`, `conflicts_with`, `part_of`, `blocks` | Change composition metadata. |

Section forms also exist for larger changes:

```txt
change checkout_update
author agent
base 0

requires
  assert_exists fn.checkout
end

ops
  op create_function id=fn.checkout_v2 return=Int body=42
end

verify target=fn.checkout_v2
end
```

Supported sections today: `metadata`, `requires`, `ops`, `expect`, `approval`, `verify`, and free-form `block <kind> @ref` blocks.

## Preconditions

Inside `requires`, the parser accepts:

| Form | Use |
|---|---|
| `assert_exists <node>` | Require a node to exist before apply. |
| `assert_hash <node> sig=<hex>` | Require a node hash before apply. |
| `assert_context <target> [hash=<hash>]` | Require a context slice relationship. |

These are stale-context protection, not runtime checks.

## Operation syntax

Every operation starts with `op`:

```txt
op <verb> key=value key=value
```

Values can be bare words or quoted strings. Parenthesized expression bodies can contain spaces, for example `body=add(x, y)`.

## Verb families

The parser maps exact verbs and `<family>_*` verbs to ChangeSet operation families:

| Family | Examples |
|---|---|
| `create` | `create_module`, `create_type`, `create_function`, `create_capability` |
| `set` | `set_return`, `set_body` |
| `add` / `remove` | `add_param`, `add_effect`, `add_contract`, `remove_effect`, `remove_contract` |
| `connect` / `disconnect` | `connect source=... target=...`, `disconnect source=... target=...` |
| `delete` | `delete target=...` |
| `rename` / `move` / `replace` | `rename target=... name=...`, `move target=... to=...`, `replace target=... with=...` |
| `bind` / `expose` / `hide` | `bind_handler`, `expose`, `hide` |
| `grant` / `revoke` | `grant target=... capability=...`, `revoke target=... capability=...` |
| `infer` / `derive` / `generate` | `infer_boundary`, `derive_eq`, `generate_tests` |
| `assert` / `lock` / `refactor` / `migrate` | Workflow and safety metadata operations. |
| `approve` / `reject` / `deprecate` / `annotate` / `verify` | Review, lifecycle, metadata, and verification operations. |

## Required arguments for schema-checked ops

The current op-schema layer enforces required arguments for these verbs:

| Verb | Required args |
|---|---|
| `create_function`, `create_type`, `create_module`, `create_capability` | `id` |
| `add_param` | `target`, `name`, `type` |
| `set_return` | `target`, `type` |
| `add_effect`, `remove_effect` | `target`, `effect` |
| `add_contract` | `target`, `kind`, `rule` |
| `connect`, `disconnect` | `source`, `target` |
| `expose`, `hide`, `deprecate`, `infer_boundary` | `target` |
| `rename` | `target`, `name` |
| `grant`, `revoke` | `target`, `capability` |
| `annotate` | `target`, `key`, `value` |
| `bind_handler` | `capability`, `handler` |

Important: parser recognition is broader than payload materialization. A verb can parse and still become a no-op if the canonicalizer does not have enough graph identity or payload support for that exact form. That's not good enough for a production language yet, but documenting the boundary keeps us honest.

## Implemented graph-building examples

Create and run a pure text-returning function:

```txt
change hello_text
author user
base 0
op create_function id=fn.hello return=Text body=let(s, "Hello, world!", s)
end
```

Create a function that prints through an explicit capability:

```txt
change print_hello
author user
base 0
op create_capability id=log.write
op create_function id=fn.print_hello return=Int body=print("Hello, world!")
op grant target=fn.print_hello capability=log.write
end
```

At runtime, the graph grant is necessary but not sufficient. The CLI run also needs `--grant log.write`.

## Expression body subset

Inline `body=` expressions are parsed by the compiler expression parser. The implemented subset includes:

| Category | Forms |
|---|---|
| Literals and variables | integers, floats, text strings, identifiers |
| Arithmetic/comparison | `add`, `mul`, `gt`, `eq`, `ne`, `le`, `ge` |
| Boolean/control | `and`, `or`, `not`, `if`, `match`, `return`, `abort` |
| Bindings | `let(name, value, body)` |
| Records/variants/lists | `record`, `field`, `variant`, `list`, `none`, `some`, `ok`, `err` |
| Collections | `map`, `set`, `index` |
| Effects | `print("text")`, `effect_call(capability, operation, args...)` |
| Functional/iteration | `lambda`, `foreach`, `fold` |
| Cells/loops | `cell_new`, `cell_get`, `cell_set`, `loop`, `while`, `break`, `continue` |

Examples:

```txt
body=add(x, y)
body=let(total, add(x, y), if(gt(total, 10), total, 0))
body=match(result, Ok(value), value, _, 0)
body=print("Hello, world!")
```

Additional expression forms with parser-test coverage:

```txt
body=effect_call(database.read, Cart, cartId)
body=lambda(x, add(x, 1))
body=foreach(item, items, add(acc, item))
body=fold(0, items, add_item)
body=cell_new(0)
body=map(x, add(x, 1))
body=set(add(x, 1), mul(y, 2))
body=index(lst, add(i, 1))
body=match(result, Ok(val), val, Err(e), -1)
body=abort("unreachable branch")
```

## Not production language surface yet

Do not infer more than the current implementation proves. These are still maturity gaps:

- no complete human-friendly source language separate from ACL;
- no final language reference for modules, packages, imports, traits/interfaces, errors, or async workflows;
- no production stability guarantee for every parsed ACL verb;
- no Rust-like compatibility promise for expression or ChangeSet syntax yet;
- no claim that native linked execution is available through `ail run --target native`.

## Next step

Use [Getting started](getting-started.md) to exercise the documented happy path. Use [Maturity model](maturity-model.md) before claiming any part of this reference is stable, preview-ready, or production-ready.
