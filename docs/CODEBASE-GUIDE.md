# Codebase guide

Use this as the entry point for the AIL docs. The canonical material lives in `docs/`; the historical draft is preserved only for context.

## Quick path

1. Read [Mental model](codebase/mental-model.md) to understand what AIL is and is not.
2. Read [Repository map](codebase/repository-map.md) to locate the crate or directory you need.
3. Use [Reference map](codebase/reference-map.md) when you know the question but not the document.
4. Use [Maintainer playbook](codebase/maintainer-playbook.md) before changing a subsystem.
5. Use [Public roadmap](roadmap.md) for release-facing milestones and [Implementation blueprint](implementation-blueprint.md) to separate completed validation milestones from remaining product work.
6. Use [Wave operating model](wave-operating-model.md) when planning or executing a multi-wave implementation run.
7. Use [Contract discipline](codebase/contract-discipline.md) before adding behavior that touches JSON output, status strings, or policy gates.

## Change map

Do not start by reading everything. Use this table to find the files that matter for your task.

| Task / domain | Read first (docs) | Read first (code) |
|---|---|---|
| Package install | [Packages](packages.md) | `ail-package/src/{manifest.rs, import.rs, resolver.rs, lockfile.rs}` |
| Package verify / audit | [Packages](packages.md) | `ail-package/src/{verification.rs, trust.rs, advisory.rs, policy.rs}` |
| Verify policy / diagnostics | [Verification](verification.md) | `ail-verify/src/{policy.rs, report.rs, diagnostic.rs, pipeline.rs}` |
| Compiler WASM lowering | [Compiler](compiler.md) | `ail-compiler/src/{lower.rs, anf.rs, wasm.rs, artifact_manifest.rs}` |
| Compiler native / object | [Compiler](compiler.md) | `ail-compiler/src/{native.rs, hash.rs, artifact_manifest.rs}` |
| Context redaction / freshness | [Context Server](context-server.md) | `ail-context/src/{builder.rs, server.rs, summary.rs}` |
| Remote bundle integrity | [Remote](remote.md) | `ail-remote/src/{bundle.rs, identity.rs, signing.rs}` |
| Release preflight | [Release policy](release-policy.md) | `scripts/{release-preflight.sh, tag-release.sh}` |
| Storage CAS / snapshots | [Storage](storage.md) | `ail-storage/src/{object.rs, graph.rs, migration.rs}` |
| CLI output / exit codes | [Tooling](tooling.md) | `ail-cli/src/{output.rs, cli.rs}` |
| ChangeSet / ACL | [AI Change Language](change-language.md) | `ail-change/src/{parser.rs, canonical.rs, apply.rs, op_schema.rs}` |

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
| Current roadmap and milestone status | [Public roadmap](roadmap.md) and [Implementation blueprint](implementation-blueprint.md) |
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
