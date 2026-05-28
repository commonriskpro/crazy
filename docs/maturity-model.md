# AIL maturity model

AIL's long-term product bar is not "a clever prototype." The bar is a general-purpose AI-native language that can eventually be evaluated with the same seriousness as Rust: reliable, teachable, well-tooled, compatible, secure, and usable for real projects.

This model turns that goal into reviewable gates. It does not claim the current repo has reached them.

## Quick path

1. Use [Public roadmap](roadmap.md) for release sequencing.
2. Use this document to decide whether a milestone moves AIL toward production-grade maturity.
3. Do not call any area mature until the evidence column is satisfied by tests, docs, fixtures, benchmarks, or shipped tooling.

## North star

AIL should become a language experience where users can:

- create, run, test, format, package, and publish real projects;
- trust verification and capability boundaries before execution;
- understand diagnostics without knowing internal graph mechanics;
- rely on compatibility, migration, security, and performance discipline;
- direct AI-authored changes without giving the AI unchecked authority.

## Maturity gates

| Gate | Production-grade expectation | Required evidence before claiming maturity |
|------|------------------------------|--------------------------------------------|
| Language surface | Users can express ordinary application code with modules, types, errors, data structures, effects, and interop boundaries. | Conformance fixtures, parser/lowering/runtime tests, unsupported-feature diagnostics, [language reference](language-reference.md), and language-reference drift checks. |
| Verification | Type, effect, contract, policy, capability, provenance, and profile checks reject unsafe programs before execution. | Profile-specific policy tests, negative fixtures, solver limit behavior, verification report compatibility tests. |
| Runtime safety | Runtime execution is deny-by-default, isolated, auditable, bounded, and predictable under failure. | Capability denial tests, resource/limit tests, audit log fixtures, [security and runtime hardening](security.md), fuzz or adversarial runtime coverage. |
| Tooling UX | A new user can install the CLI, create a project, run it, test it, format it, inspect diagnostics, and use editor feedback. | `ail new/run/test/fmt` integration tests, LSP/CLI diagnostic parity fixtures, getting-started tutorial, troubleshooting guide, [tooling reference](tooling-reference.md). |
| Package ecosystem | Packages are reproducible, signed or otherwise trustable, compatible, documented, and safe to consume. | Lockfile/reproducibility tests, registry workflow fixtures, signing/advisory checks, compatibility policy, [package reference](package-reference.md), [security and runtime hardening](security.md). |
| Standard library | Core stdlib APIs cover everyday work without hiding effects or weakening verification. | Versioned stdlib contracts, examples, stability labels, [stdlib reference](stdlib-reference.md), capability-backed I/O tests. |
| Compatibility | Users can upgrade AIL and packages without guessing what breaks. | Semver policy, migration guide, [compatibility policy](compatibility.md), compatibility test matrix, deprecation process. |
| Performance | Graph, compiler, verifier, context, package, and runtime paths have realistic benchmark coverage and regression thresholds. | Large-project fixtures, benchmark baselines, [performance validation](performance.md), thresholded CI or release preflight evidence. |
| Documentation | Users and contributors can learn the model progressively without reading the whole repo. | Tutorial, language reference, architecture guide, troubleshooting guide, status labels and docs drift checks kept current. |
| AI-native workflow | LLMs propose ChangeSets, the toolchain verifies them, repairs are structured, and humans remain the authority. | End-to-end context -> ChangeSet -> verify -> apply -> repair tests with persisted project state. |

## Current maturity snapshot

This snapshot is intentionally conservative. It translates the current implementation evidence from [Implementation blueprint](implementation-blueprint.md) into the maturity gates above. A gate can have strong validation evidence and still be below production-grade maturity.

| Gate | Current maturity reading | Next proof that would move the gate |
|------|--------------------------|-------------------------------------|
| Language surface | Implemented subset: ACL/Core IR execute real slices and [language reference](language-reference.md) documents the current evidence-backed surface, but ordinary application surface remains incomplete. | Broader parser/lowering/runtime fixtures for modules, records, variants, `Option`/`Result`, pattern matching, and explicit unsupported-feature diagnostics. |
| Verification | Implemented subset: reports, policy gates, profiles, and contract structures exist, but production/critical rigor is not complete. | Prod/critical profile tests that reject unsafe assumptions, missing translation evidence, policy bypasses, and solver-limit ambiguity. |
| Runtime safety | Implemented subset: WASM host, deny-by-default capabilities, handler trust, secrets, resource limits, context redaction, package trust hooks, and [security and runtime hardening](security.md) exist for validated slices. | Enforced in-flight revocation, external vault integration, process/remote handler isolation, deeper fuzz/adversarial coverage, and richer typed ABI/resource-handle tests. |
| Tooling UX | Early implemented subset: CLI, [getting started](getting-started.md), [troubleshooting](troubleshooting.md), [tooling reference](tooling-reference.md), JSON `schema_version`, validation-stage `ail new`, ACL-only `ail fmt`, validation-stage `ail test`, validation-stage `ail lsp` diagnostics/completion/hover/same-file definition/references, and dogfood paths exist, but everyday project lifecycle is not complete. | Full `ail run`, richer `ail new` templates, richer `ail test` project workflows, general-source `ail fmt`, CLI/LSP diagnostic parity, long-term JSON schemas, and LSP cross-file navigation/rename/workspace indexing. |
| Package ecosystem | Implemented subset: package metadata, resolver/trust pieces, HTTP/remote registry DTOs, signing primitives, advisories, yanking, compatibility metadata, and [package reference](package-reference.md) exist. | Reproducible package install/build fixtures, deployed registry workflow, keyless/signing hardening, package compatibility fixtures, and ownership/federation policy. |
| Standard library | Early implemented subset: semantic stdlib registry and [stdlib reference](stdlib-reference.md) exist, including pure and capability-backed descriptors, but everyday APIs and official adapters remain limited. | Broader examples, compatibility fixtures for old stdlib IDs/contracts, capability-backed I/O modules, and package integration. |
| Compatibility | Partial release discipline: release policy, migration guide, and [compatibility policy](compatibility.md) exist with a surface matrix and deprecation process. | Compatibility fixtures for old stores, old ChangeSets, CLI JSON, package lockfiles, and runtime ABI proving old projects still behave predictably. |
| Performance | Partial evidence: deterministic compiler regression evidence, large-graph compiler benchmark, storage benchmark fixture, and [performance validation](performance.md) exist, but broad graph/storage/context/runtime/package thresholds remain a roadmap gap. | Broader large-project fixtures, benchmark baselines, regression thresholds, and controlled timing-gate integration. |
| Documentation | Solid validation docs: architecture, roadmap, guide, risks, blueprint, tutorial, troubleshooting guide, language reference, tooling reference, compatibility policy, stdlib reference, package reference, security reference, and performance validation docs exist with status labels and static drift checks. | Broader docs drift checks tied to release gates. |
| AI-native workflow | Architecture slices exist, but the durable AI authoring loop is not yet a production workflow. | Persisted context -> ChangeSet -> verify -> apply -> repair integration tests with human approval boundaries. |

## Rust-comparable does not mean Rust-clone

The comparison to Rust is about maturity discipline, not syntax or implementation strategy.

| Rust-like bar | AIL-specific interpretation |
|---------------|-----------------------------|
| Reliability | Semantic Graph-visible ownership, explicit capabilities, profile-gated verification, and deterministic lowering. |
| Tooling | CLI, formatter, LSP, diagnostics, package workflows, docs, and migration tooling that make the graph model usable. |
| Safety | No ambient effects, no unchecked AI authority, no production claim without runtime and verification evidence. |
| Ecosystem | Packages, signing, advisories, compatibility, examples, and stdlib contracts mature enough for real users. |
| Learning curve | The model may be new, but docs and tools must make the happy path obvious before exposing internals. |

## Release maturity ladder

| Stage | Meaning | Allowed claim |
|-------|---------|---------------|
| Validation milestone | A narrow architecture slice works end to end. | "This proves the direction for a slice." |
| Usable preview | Users can build small programs with documented limitations. | "This is useful for experimentation and feedback." |
| Real language experience | Project lifecycle, tooling, docs, stdlib, and package basics work together. | "This can be evaluated as a serious language experience." |
| Production-ready | Operational hardening, security, compatibility, performance, and ecosystem evidence are in place. | "This is suitable for production use within documented limits." |

Current `v0.x` releases are validation milestones unless release notes explicitly prove a higher stage.

## Review checklist

Before accepting a roadmap item as maturity progress, verify:

- [ ] It improves at least one maturity gate above.
- [ ] It preserves Semantic Graph as source of truth.
- [ ] It keeps LLM output as proposals, not authority.
- [ ] It strengthens or preserves verification-first acceptance.
- [ ] It keeps external effects explicit through capabilities.
- [ ] It includes evidence matching the breadth of the claim.
- [ ] It updates docs/status labels when implementation scope changes.

## Next step

Use this model together with [Implementation blueprint](implementation-blueprint.md) when selecting the next milestone. If a task does not make one of these gates more true, it is probably polish, research, or distraction—not production-grade maturity work.
