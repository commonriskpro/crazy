# Consistency review

## Reviewed decisions

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
- Use full product design and full product implementation language.
- Internal validation milestones may exist, but should not be presented as scope-reducing MVP.

## Remaining consistency work

- The raw `ai-native-language-draft.md` still contains earlier exploratory/proposal sections and repeated material.
- Canonical organized docs are now under `docs/`.
- Future edits should update split docs first, then optionally sync/archive raw draft.
