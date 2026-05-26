# AIL: AI-native language toolchain

AIL is a validation-stage programming language and toolchain where the program lives as a versioned Semantic Graph. Humans direct intent, LLMs propose structured ChangeSets, and the toolchain verifies, applies, compiles, and runs accepted changes.

Current release: [`v0.1.0`](https://github.com/commonriskpro/crazy/releases/tag/v0.1.0). This is a public foundation release for architecture validation, not production-ready language infrastructure.

## Quick Path

1. Read the [mental model](docs/codebase/mental-model.md) to understand what AIL is and is not.
2. Build and test the workspace:

   ```sh
   cargo build --workspace
   cargo test --workspace
   ```

3. Inspect the CLI surface:

   ```sh
   cargo run -p ail-cli -- --help
   cargo run -p ail-cli -- init
   cargo run -p ail-cli -- status
   ```

4. Use the [codebase guide](docs/CODEBASE-GUIDE.md) as the documentation entry point.
5. Check the [roadmap](docs/implementation-blueprint.md), [risks](docs/risks.md), and [changelog](CHANGELOG.md) before relying on any subsystem.

Requires Rust 1.95.0, pinned by `rust-toolchain.toml`.

## What AIL Is

| Topic | Current answer |
|---|---|
| Source of truth | A versioned Semantic Graph, not text files as the authoritative program model. |
| AI write path | LLMs propose AI Change Language ChangeSets; the toolchain canonicalizes, verifies, and applies them transactionally. |
| Verification model | Type, effect, contract, policy, capability, provenance, and profile checks are first-class design goals. |
| Execution path | Current crates cover graph, ChangeSets, verification reports, compiler lowering, WASM runtime hosting, storage, packages, context, remote primitives, coordinator, CLI, and dogfooding examples. |
| Release status | `v0.1.0` is an implemented subset and validation milestone. It does not claim production-ready safety, compatibility, or operational hardening. |

## Test And Release Commands

```sh
# Formatting
cargo fmt --check

# Lints
cargo clippy --all-targets -- -D warnings

# Tests
cargo test --workspace

# Public CLI dogfood conformance
./scripts/dogfood-conformance.sh

# Release metadata preflight for local/CI validation
./scripts/release-preflight.sh --allow-unreleased
```

Release process details live in [release policy](docs/release-policy.md). Published release notes live on GitHub at [`v0.1.0`](https://github.com/commonriskpro/crazy/releases/tag/v0.1.0) and in [CHANGELOG.md](CHANGELOG.md).

## Known Limitations

- Not production-ready: implemented milestones prove architecture slices, not operational safety for production programs.
- Language coverage is incomplete: the docs describe a broader target design than the executable surface currently supports.
- Verification is not final: production/critical profile rigor, translation validation, and policy UX still need hardening.
- Runtime and ABI are still maturing: rich typed WASM ABI, hardened isolation, full async/channel runtime, and external secret providers remain gaps.
- Ecosystem infrastructure is early: no deployed package registry federation, Sigstore/keyless signing integration, or production Context Server deployment.
- Performance evidence is limited: deterministic compiler regression evidence exists, but large-project benchmark coverage and thresholds are still roadmap work.

See the [implementation blueprint](docs/implementation-blueprint.md) for the current milestone map and next recommended milestones.

## Documentation Map

Start with [docs/CODEBASE-GUIDE.md](docs/CODEBASE-GUIDE.md). It gives the reading order, status legend, implementation reality, and links into the codebase maps.

| Need | Start here |
|---|---|
| What AIL is and is not | [Mental model](docs/codebase/mental-model.md) |
| Repository layout | [Repository map](docs/codebase/repository-map.md) |
| Architecture | [Architecture](docs/architecture.md) |
| Semantic Graph and Core IR | [Core IR](docs/core-ir.md) |
| AI Change Language | [Change language](docs/change-language.md) |
| Verification | [Verification](docs/verification.md) |
| Runtime/capabilities | [Runtime](docs/runtime.md) |
| Storage/versioning | [Storage](docs/storage.md) |
| Context Server | [Context Server](docs/context-server.md) |
| Packages/trust | [Packages](docs/packages.md) |
| Compiler | [Compiler](docs/compiler.md) |
| Roadmap/status | [Implementation blueprint](docs/implementation-blueprint.md) |
| Risks and decisions | [Risks](docs/risks.md), [Decision log](docs/decision-log.md), [Decisions register](docs/open-questions.md) |

`docs/history/ai-native-language-draft.md` is historical context, not the source of truth.

## Release Checklist

- [x] Public repository exists: [`commonriskpro/crazy`](https://github.com/commonriskpro/crazy).
- [x] `v0.1.0` tag and GitHub release are published.
- [x] Workspace version is lockstep at `0.1.0`.
- [x] Release policy, migration guide, changelog, and preflight script exist.
- [ ] Production-readiness claims are intentionally out of scope for `v0.1.0`.
