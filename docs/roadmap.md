# Public Roadmap

AIL is currently a `v0.1` validation-stage language toolchain, not a production-ready general-purpose language. The verified core proves that the Semantic Graph, AI Change Language (ACL), verification reports, compiler lowering, and WASM runtime host can work together for a narrow executable subset.

The roadmap below describes how AIL can grow from that foundation into a language with practical usability in the direction of Rust, Python, and JavaScript: clear errors, useful standard libraries, package workflows, strong tooling, and predictable execution. It is a plan, not a guarantee of release dates.

Related: [Implementation blueprint](implementation-blueprint.md), [Codebase guide](CODEBASE-GUIDE.md), [Risks](risks.md), [Release policy](release-policy.md).

## Status Snapshot

| Area | Current public status |
|------|-----------------------|
| Release | `v0.1.0` foundation release. Useful for architecture validation and contributors; not production-ready. |
| Language surface | ACL and Core IR cover a real but narrow executable subset. General text-authoring ergonomics are still early. |
| Runtime | WASM host execution and deny-by-default capability boundaries exist for validated slices. Runtime hardening is incomplete. |
| Verification | Reports, policy gates, and contract structures exist. Production and critical profiles still need stronger evidence. |
| Tooling | CLI and dogfood tests exist. Formatting, LSP, package workflow, and everyday project ergonomics are not complete. |

## Milestone Map

| Milestone | User-facing outcome | Status |
|-----------|---------------------|--------|
| `v0.1` | Verified ACL/WASM foundation for contributors. | Released foundation. |
| `v0.2` | Write and run hello-world programs with strings and print/log output. | In progress: Text-return hello world runs; `print(...)` lowers to `log.write` and is denied without `--grant log.write`. |
| `v0.3` | Split code into modules, import local code, and use basic packages. | Planned. |
| `v0.4` | Get useful type errors and safe match behavior before runtime. | Planned. |
| `v0.5` | Build small programs that use stdlib I/O, JSON, time, filesystem, and HTTP capabilities. | Planned. |
| `v1.0` | Use AIL as a documented, tool-supported language with LSP, tests, formatting, and package workflow. | Target. |

## v0.1: Verified ACL/WASM Core

`v0.1` proves the architecture can execute a constrained language path end to end.

User-facing capabilities:

- Inspect and run the CLI foundation.
- Parse and apply ACL ChangeSets for supported forms.
- Lower supported Core IR expressions through compiler stages.
- Execute validated WASM slices behind explicit runtime capabilities.
- Use the repository as a contributor foundation for language, compiler, runtime, and verification work.

Acceptance criteria:

- `cargo test --workspace` passes for the released tag.
- ACL parser, canonicalization, apply, compiler, runtime, storage, and verification tests cover the released subset.
- Release notes clearly state that `v0.1` is a validation milestone, not a production language release.

## v0.2: Strings, Print/Log, Hello World

`v0.2` should make AIL feel runnable to a new user for the first time.

User-facing capabilities:

- Create a minimal AIL program with string literals.
- Return Text from a public function and see human-readable CLI output.
- Print or log text through an explicit output capability.
- Run a hello-world program through the CLI.
- See stable, readable diagnostics when string or output capability use is invalid.

Acceptance criteria:

- A documented `hello.ail` example runs from a clean checkout.
- `ail run` decodes Text returns from WASM ABI descriptors instead of showing packed integer pointers.
- String literals round-trip through parse, lower, compile, and WASM execution tests.
- `print(...)` output is deterministic in CLI tests and denied without the required `log.write` capability grant.
- The README points new users to the hello-world path without implying production readiness.

## v0.3: Modules, Imports, Package Basics

`v0.3` should let users organize small projects instead of writing single-file demos.

User-facing capabilities:

- Define local modules with explicit exports.
- Import code from nearby files or package entries.
- Create a basic package manifest.
- Resolve dependencies deterministically with a lockfile or equivalent recorded resolution.
- Receive clear errors for missing modules, cycles, duplicate exports, and incompatible package metadata.

Acceptance criteria:

- A two-module example compiles and runs through the CLI.
- Import resolution is deterministic and covered by tests for success, missing import, cycle, and duplicate-name cases.
- Package metadata validation rejects malformed or ambiguous packages.
- Documentation explains the difference between local modules, packages, and the Semantic Graph source of truth.

## v0.4: Type System, Errors, Match Exhaustiveness

`v0.4` should make incorrect programs fail before runtime in ways users can understand.

User-facing capabilities:

- Get actionable type errors with source or graph locations.
- Use structured errors instead of ad hoc failure strings.
- Pattern-match on supported variants, records, `Option`, or `Result` forms.
- Know when a `match` is incomplete before running the program.

Acceptance criteria:

- Type checking rejects representative mismatches before code generation.
- Match exhaustiveness tests cover complete, incomplete, wildcard, and unreachable-arm cases.
- Unsupported pattern forms produce compile-time diagnostics rather than runtime `unreachable` behavior.
- Error diagnostics have stable machine-readable fields and human-readable messages.

## v0.5: Standard Library And Capability I/O

`v0.5` should support useful small programs while keeping effects explicit.

User-facing capabilities:

- Use standard modules for text, collections, JSON, time, filesystem access, and HTTP requests.
- Grant filesystem, network, clock, and output access explicitly.
- Parse and serialize JSON values through documented APIs.
- Read and write files in permitted directories.
- Make simple HTTP requests with denied-by-default network behavior.

Acceptance criteria:

- Stdlib entries include versioned contracts for the supported APIs.
- Capability tests prove allowed and denied behavior for time, filesystem, HTTP, and output operations.
- JSON parse/serialize has success, malformed-input, and type-shape tests.
- File and HTTP examples run in a local test environment without requiring external services.
- Documentation states which stdlib APIs are stable, experimental, or intentionally unavailable.

## v1.0: Tooling, Docs, LSP, Tests, Format, Package Story

`v1.0` should be the first release where AIL can be evaluated as a real language experience, not only a research toolchain.

User-facing capabilities:

- Install or build the CLI with a documented path.
- Create a project, run it, test it, format it, and package it.
- Use editor support through an LSP for diagnostics, go-to-definition, hover, and basic completions.
- Rely on a documented package story for manifests, lockfiles, verification, signing, and compatibility.
- Read concise language, stdlib, tooling, and migration documentation.

Acceptance criteria:

- `ail new`, `ail run`, `ail test`, `ail fmt`, and package commands are documented and covered by integration tests or equivalent conformance tests.
- Formatter output is deterministic and idempotent on representative fixtures.
- LSP diagnostics match CLI diagnostics for the same project fixtures.
- Package install/build behavior is reproducible from lock data.
- Compatibility policy, migration guide, standard library reference, and getting-started tutorial are current for the release.
- A public example project demonstrates modules, tests, stdlib I/O, package metadata, and verification output.

## Non-Goals For This Roadmap

- Production-critical or safety-critical deployment before the verification, runtime, and compatibility story is proven.
- Full Rust, Python, or JavaScript compatibility. AIL may learn from their usability, but it is not trying to clone their syntax or ecosystems.
- Unrestricted ambient effects. File, network, clock, secret, and process capabilities should remain explicit.
- A large package ecosystem before package trust, signing, compatibility, and reproducibility are credible.
- Optimizing for benchmark wins before correctness, diagnostics, and toolchain reliability are solid.

## Known Risks

- The Semantic Graph source-of-truth model may be harder to teach than text-first languages unless tooling makes it feel natural.
- The gap between target design and implemented subset can mislead users if docs do not keep status labels visible.
- WASM ABI and runtime capability design must stay stable enough for packages without freezing too early.
- Type checking, match exhaustiveness, and verification profiles can become too complex unless diagnostics remain practical.
- Package trust and registry work can become a security liability if signing, advisories, and reproducible builds are treated as optional polish.
- Performance evidence is still limited; large-project graph, compiler, runtime, and context workloads need real benchmarks before `v1.0` claims.

## Release Discipline

Each milestone should ship only when its examples, tests, docs, and known limitations agree. If a capability is implemented only as a narrow slice, the release notes and docs should say that directly.
