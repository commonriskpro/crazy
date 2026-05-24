# Mental model

AIL is a general-purpose programming language and toolchain designed around semantic changes, not text editing. Humans direct intent, LLMs propose structured changes, and the toolchain verifies and applies those changes against a Semantic Graph.

## Status lens

| Lens | Reality |
|------|---------|
| Target design | A full AI-native language where the Semantic Graph is source of truth, ChangeSets are the write path, verification gates changes, and executable artifacts are profile-bound. |
| Implemented subset | Rust crates implement validation slices for graph, ChangeSet parsing/apply, verification reports, compiler lowering, WASM runtime hosting, storage, packages, context, remote primitives, coordinator, CLI, and dogfooding examples. |
| Historical context | The original raw draft is preserved at [docs/history/ai-native-language-draft.md](../history/ai-native-language-draft.md). It explains origins but is not the canonical navigation surface. |

## What AIL is

- A language/toolchain where the program lives as a versioned Semantic Graph.
- A ChangeSet protocol for LLM-authored semantic edits.
- A verifier/compiler/runtime pipeline that tracks types, effects, contracts, capabilities, provenance, and profile-bound artifacts.
- A context model where AI tools ask for semantic slices instead of scraping source files.
- A language targeting Rust-level mature reliability — memory/resource safety and zero-cost abstractions — achieved via Semantic Graph-visible ownership (Handle modes), resource lifecycle, effects, and capabilities that lower through Core IR → ANF → SSA to efficient executables.

## What AIL is not

- Not a DSL limited to one application domain.
- Not a prompt wrapper around existing source files.
- Not RAG over a repository.
- Not a claim that LLM output is trusted because it looks plausible.
- Not a Rust clone. AIL targets Rust-level reliability without adopting Rust's borrow-checker model or lifetime syntax.
- Not production-ready language infrastructure yet.

## Core model

```txt
Human intent
  -> AI Change Language ChangeSet
  -> parser + canonicalizer
  -> verifier / policy gates
  -> transactional Semantic Graph snapshot
  -> compiler pipeline
  -> WASM/native artifact + manifests
  -> deny-by-default runtime host
```

The important boundary is authority: the AI proposes; the toolchain decides what is accepted.

## AI-native loop

1. Query semantic context for the target node, module, contract, effect, or risk.
2. Generate a small ChangeSet against a known base snapshot.
3. Canonicalize and verify the ChangeSet.
4. Apply only if verification and policy gates accept it.
5. Compile and run artifacts that are bound to the accepted profile and hashes.
6. Feed structured diagnostics back into the next repair ChangeSet.

## Production-readiness warning

Current code proves architecture slices. It does not yet prove the full product is safe for production programs. Treat implemented milestones as evidence that the direction is viable, not as an operational compatibility or security guarantee.
