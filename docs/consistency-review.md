# Consistency review

<!-- Status: Consistency review. This pass tracks docs against current implementation state. -->

## Reviewed decisions

### Parser choice

Status: resolved.

- Earlier docs mentioned `chumsky`/`lalrpop` as the parser spike outcome.
- Implementation uses hand-written parsers for ACL and the current expression subset.
- Updated `decision-log.md`, `open-questions.md`, and `compiler.md` to document the reversal.

### Runtime ABI and effect dispatch

Status: resolved with remaining gap documented.

- Runtime docs now reflect implemented `RuntimeHost`, `ail/host_call`, handler dispatch, schema checks, audit events, and rollback helpers.
- Full typed ABI layout is not claimed complete; it is tracked as risk/validation work.

### Context Server transport

Status: resolved with deliberate deviation documented.

- The design keeps the Context Server protocol shape.
- Implementation is currently an in-process `ail-context` crate with DTOs, source adapters, builder, and deterministic summary rendering.
- Network transport is not implemented and is no longer implied by the docs.

### Phase status

Status: resolved.

- `implementation-blueprint.md` now maps subsystem status against code evidence and remaining production gaps.
- Milestone completion does not imply every full-design feature is complete.

### Money

Status: consistent.

- `Money` is not a Core IR primitive.
- Core numeric primitives are `Int`, `UInt`, `Float`, `Decimal<Scale, Precision>`.
- `Money<C>` belongs to stdlib/domain packages.

### Unverified compilation

Status: consistent after correction.

- Compiler accepts verification reports accepted for target profile.
- `draft/dev/test` may compile unverified if profile policy accepts it.
- Artifacts are profile-bound and cannot be promoted to prod by relabeling.

### Change Language format

Status: consistent.

- LLM-facing format is line-oriented DSL.
- JSON may exist as internal/API representation.
- YAML is not primary format.

### ANF/SSA roles

Status: consistent.

- ANF is main compiler IR because effect ordering is structural.
- SSA is backend artifact for optimization/codegen.

### Product-scope wording

Status: corrected.

- Avoid scope-reducing MVP framing.
- Use target design language for the full product, and implemented-subset language for current code.
- Internal validation milestones may exist, but should not be presented as scope-reducing MVP.

## Remaining consistency work

- The raw `docs/history/ai-native-language-draft.md` still contains earlier exploratory/proposal sections and repeated material.
- Canonical organized docs are now under `docs/`.
- Full WASM value layout, native execution parity, remote Context Server transport, and large-project performance benchmarks remain known implementation gaps.
- Future edits should update split docs first, then optionally sync/archive raw draft.
