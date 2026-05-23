# Codebase guide

Use this as the entry point for the AIL docs. The canonical material lives in `docs/`; the historical draft is preserved only for context.

## Quick path

1. Read [Mental model](codebase/mental-model.md) to understand what AIL is and is not.
2. Read [Repository map](codebase/repository-map.md) to locate the crate or directory you need.
3. Use [Reference map](codebase/reference-map.md) when you know the question but not the document.
4. Use [Maintainer playbook](codebase/maintainer-playbook.md) before changing a subsystem.
5. Use [Implementation blueprint](implementation-blueprint.md) to separate completed validation milestones from remaining product work.

## Status legend

| Status | Meaning |
|--------|---------|
| Target design | The intended full product behavior described by the architecture docs. Do not assume it is implemented. |
| Implemented subset | Code exists for the current milestone, often narrower than the target design. Check crate docs and tests before relying on it. |
| Completed validation milestone | A phase proved the selected architecture can work for a slice. It does not mean the subsystem is production-ready. |
| Production-ready | Suitable for production use with hardened operations, compatibility policy, security review, and performance evidence. This repo does not currently claim that status. |
| Historical context | Raw or preserved design material that explains origin and tradeoffs, but is not the source of truth. |

## Current implementation reality

AIL has working Rust crates for the major architecture slices: Semantic Graph, ChangeSets, verification reports, compiler pipeline, Wasmtime runtime host, storage, context slices, stdlib registry, packages, remote bundles, coordinator, CLI, and dogfooding examples.

That is still a validation-stage implementation. The docs describe a broader target design than the executable surface currently supports, especially around full language syntax, rich WASM ABI/value layout, production verification policy, runtime hardening, networked Context Server transport, large-project benchmarks, and ecosystem workflows.

## Canonical docs

| Need | Start here |
|------|------------|
| System overview | [Architecture](architecture.md) |
| Current roadmap and milestone status | [Implementation blueprint](implementation-blueprint.md) |
| Source of truth and IR | [Core IR and Semantic Graph](core-ir.md) |
| LLM-authored changes | [AI Change Language](change-language.md) |
| Verification model | [Verification](verification.md) |
| Runtime capability model | [Runtime](runtime.md) |
| Storage and snapshots | [Storage](storage.md) |
| Context slices | [Context Server](context-server.md) |
| Packages and trust | [Packages](packages.md) |
| Compiler pipeline | [Compiler](compiler.md) |
| Standard library | [Standard library](stdlib.md) |
| Tooling and CLI workflow | [Tooling](tooling.md) |
| Risks and validation gaps | [Risks](risks.md) and [Decisions register](open-questions.md) |

## Historical context

- [Historical AI-native language draft](history/ai-native-language-draft.md) is raw source material. Use it for audit and background only.
- When historical material conflicts with split docs, prefer the canonical docs under `docs/`.
