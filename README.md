# AIL: AI-native language toolchain

AIL is a validation-stage programming language and toolchain where the program lives as a versioned Semantic Graph. Humans direct intent, LLMs propose structured ChangeSets, and the toolchain verifies, applies, compiles, and runs accepted changes.

Current release: [`v0.1.0`](https://github.com/commonriskpro/crazy/releases/tag/v0.1.0). This is a public foundation release for architecture validation, not production-ready language infrastructure.

## Quick Path

1. Read the [mental model](docs/codebase/mental-model.md) to understand what AIL is and is not.
2. Try the validation-stage [getting started tutorial](docs/getting-started.md).
3. Build and test the workspace:

   ```sh
   cargo build --workspace
   cargo test --workspace
   ```

4. Inspect the CLI surface:

   ```sh
   cargo run -p ail-cli -- --help
   cargo run -p ail-cli -- init
   cargo run -p ail-cli -- status
   cargo run -p ail-cli -- fmt --file crates/ail-cli/tests/fixtures/sample.acl --check
   ```

   Narrow v0.2 hello-world output is available after creating a function and
   granting its `log.write` capability in the Semantic Graph. Once that setup is
   applied and compiled, run it with an explicit runtime grant:

   ```sh
   cargo run -p ail-cli -- run --grant log.write fn.print_hello
   ```

   Clean-checkout examples for the current public `Text` and `print`/`log.write`
   slices live in [examples/](examples/). They can be checked with:

   ```sh
   ./scripts/examples-smoke.sh
   ```

5. Use the [codebase guide](docs/CODEBASE-GUIDE.md) as the documentation entry point.
6. Check the [public roadmap](docs/roadmap.md), [implementation blueprint](docs/implementation-blueprint.md), [risks](docs/risks.md), and [changelog](CHANGELOG.md) before relying on any subsystem.

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

# PR governance and release gate smokes, including maturity-claim policy checks
./scripts/docs-onboarding-smoke.sh
./scripts/docs-troubleshooting-smoke.sh
./scripts/docs-language-reference-smoke.sh
./scripts/docs-compatibility-smoke.sh
./scripts/docs-stdlib-reference-smoke.sh
./scripts/docs-package-reference-smoke.sh
./scripts/docs-performance-smoke.sh
./scripts/docs-security-smoke.sh
./scripts/docs-tooling-reference-smoke.sh
./scripts/pr-validation-smoke.sh
./scripts/tag-release-gate-smoke.sh
./scripts/release-metadata-gate-smoke.sh

# Release metadata preflight for local/CI validation
./scripts/release-preflight.sh --allow-unreleased
```

Release process details live in [release policy](docs/release-policy.md). Published release notes live on GitHub at [`v0.1.0`](https://github.com/commonriskpro/crazy/releases/tag/v0.1.0) and in [CHANGELOG.md](CHANGELOG.md).

## Known Limitations

- Not production-ready: implemented milestones prove architecture slices, not operational safety for production programs.
- Language coverage is incomplete: the docs describe a broader target design than the executable surface currently supports.
- Verification is not final: production/critical profile rigor, translation validation, and policy UX still need hardening.
- Runtime and ABI are still maturing: rich typed WASM ABI, hardened isolation, full async/channel runtime, external secret providers, and production security operations remain gaps.
- Ecosystem infrastructure is early: no deployed package registry federation, Sigstore/keyless signing integration, or production Context Server deployment.
- Performance evidence is limited: deterministic compiler regression evidence exists, but large-project benchmark coverage and thresholds are still roadmap work.

See the [public roadmap](docs/roadmap.md) for release-facing milestones, the [maturity model](docs/maturity-model.md) for production-grade gates, and the [implementation blueprint](docs/implementation-blueprint.md) for technical validation status.

## Documentation Map

Start with [docs/CODEBASE-GUIDE.md](docs/CODEBASE-GUIDE.md). It gives the reading order, status legend, implementation reality, and links into the codebase maps.

| Need | Start here |
|---|---|
| What AIL is and is not | [Mental model](docs/codebase/mental-model.md) |
| First CLI walkthrough | [Getting started](docs/getting-started.md), [Troubleshooting](docs/troubleshooting.md), [Tooling reference](docs/tooling-reference.md) |
| Repository layout | [Repository map](docs/codebase/repository-map.md) |
| Architecture | [Architecture](docs/architecture.md) |
| Semantic Graph and Core IR | [Core IR](docs/core-ir.md) |
| Language surface and AI Change Language | [Language reference](docs/language-reference.md), [Change language](docs/change-language.md) |
| Verification | [Verification](docs/verification.md) |
| Runtime/capabilities | [Runtime](docs/runtime.md), [Security and runtime hardening](docs/security.md) |
| Storage/versioning | [Storage](docs/storage.md) |
| Context Server | [Context Server](docs/context-server.md) |
| Packages/trust | [Package reference](docs/package-reference.md), [Package/trust model](docs/packages.md) |
| Compiler | [Compiler](docs/compiler.md) |
| Roadmap/status | [Public roadmap](docs/roadmap.md), [Maturity model](docs/maturity-model.md), [Implementation blueprint](docs/implementation-blueprint.md) |
| Security and runtime hardening | [Security and runtime hardening](docs/security.md) |
| Tooling UX | [Tooling reference](docs/tooling-reference.md), [Tooling design](docs/tooling.md) |
| Performance validation | [Performance validation](docs/performance.md) |
| Contributing | [Contributing guide](CONTRIBUTING.md), [Maintainer playbook](docs/codebase/maintainer-playbook.md) |
| Compatibility and releases | [Compatibility policy](docs/compatibility.md), [Release policy](docs/release-policy.md), [Migration guide](docs/migration-guide.md) |
| Risks and decisions | [Risks](docs/risks.md), [Decision log](docs/decision-log.md), [Decisions register](docs/open-questions.md) |

`docs/history/ai-native-language-draft.md` is historical context, not the source of truth.

## Release Checklist

- [x] Public repository exists: [`commonriskpro/crazy`](https://github.com/commonriskpro/crazy).
- [x] `v0.1.0` tag and GitHub release are published.
- [x] Workspace version is lockstep at `0.1.0`.
- [x] Release policy, migration guide, changelog, and preflight script exist.
- [ ] Production-readiness claims are intentionally out of scope for `v0.1.0`.
