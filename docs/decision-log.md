# Decision log

## Product scope

- Design full product upfront; implementation can be sequenced but not framed as a scope-reducing MVP.
- General-purpose AI-native language, not a DSL.
- Source of truth is Semantic Graph, not files.

## Core architecture

- Semantic Core IR is ML-like with effect rows, contracts, capabilities, resource handles.
- ANF is main compiler IR; SSA is backend artifact.
- WASM is first executable target; native can come later.
- Runtime is deny-by-default capability host.

## Type system

- Nominal by default; structural only via explicit constraints.
- No general implicit subtyping.
- Inference can propose; canonical graph stores explicit signatures.
- Generics include type/effect/capability/limited const params.
- Dynamic dispatch explicit with `Dyn<Interface>`.
- No null/nil/undefined in Core IR.

## Change/verification/runtime

- AI Change Language is line-oriented DSL and versioned protocol.
- ChangeSets are atomic graph transactions.
- Requires/expect are AI claims, not authority.
- Verification states are explicit and profile-gated.
- Assumptions must be boundary-scoped, owned, expiring, approved.
- Runtime checks must be materialized and hash-covered.
- Packages import symbols but do not grant capabilities.

## Context/tooling/storage

- Context Server is semantic query layer, not RAG over files.
- Structured context is authoritative; summary is helper.
- Storage is append-only semantically, GC/compacted physically by policy.
- Tooling operates on graph snapshots and ChangeSets.
