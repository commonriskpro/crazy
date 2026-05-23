# AI-native programming language design

Diseño de un lenguaje general-purpose optimizado para LLMs: el humano dirige intención, la IA emite cambios semánticos y el toolchain verifica/aplica transacciones sobre un Semantic Graph.

## Canonical docs

Start with `docs/CODEBASE-GUIDE.md`. It gives the reading order, status legend, implementation reality, and links into the codebase maps.

- `docs/codebase/mental-model.md` — what AIL is and is not.
- `docs/codebase/repository-map.md` — crates/directories by responsibility.
- `docs/codebase/reference-map.md` — "if you need X, read Y".
- `docs/codebase/maintainer-playbook.md` — maintainer checklists by subsystem.
- `docs/architecture.md` — system overview and architecture.
- `docs/core-ir.md` — Semantic Graph, Core IR, ANF/SSA, and primitives.
- `docs/type-system.md` — type-system design.
- `docs/change-language.md` — AI Change Language.
- `docs/verification.md` — verification model.
- `docs/runtime.md` — runtime/capability protocol.
- `docs/storage.md` — storage/versioning model.
- `docs/context-server.md` — Context Server protocol.
- `docs/packages.md` — package/trust model.
- `docs/compiler.md` — compiler pipeline.
- `docs/stdlib.md` — standard library shape.
- `docs/tooling.md` — developer workflow/tooling.
- `docs/risks.md` — risks and validation register.
- `docs/decision-log.md` — accepted decisions.
- `docs/open-questions.md` — closed decisions and validation-required items.
- `docs/implementation-blueprint.md` — living roadmap and milestone status.
- `docs/consistency-review.md` — consistency review notes.

`docs/history/ai-native-language-draft.md` is historical/raw context, not the source of truth.

## Building & Testing

Requires Rust 1.95.0 (pinned via `rust-toolchain.toml`).

```sh
# Build all crates
cargo build --workspace

# Run all tests
cargo test --workspace

# Check for lint violations
cargo clippy --all-targets -- -D warnings

# Verify formatting
cargo fmt --check
```
