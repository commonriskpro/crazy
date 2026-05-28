# Release Policy

<!-- Status: Implemented subset. Release policy reflects current lockstep workspace versioning and tag script expectations. -->

This document describes the versioning contract, tagging procedure, signing policy, and
lockstep versioning rationale for the AIL workspace.

## Release Readiness Quick Path

Use this path before creating a release branch or tag:

```sh
./scripts/docs-onboarding-smoke.sh
./scripts/docs-troubleshooting-smoke.sh
./scripts/docs-language-reference-smoke.sh
./scripts/docs-compatibility-smoke.sh
./scripts/docs-stdlib-reference-smoke.sh
./scripts/docs-package-reference-smoke.sh
./scripts/docs-performance-smoke.sh
./scripts/docs-security-smoke.sh
./scripts/docs-tooling-reference-smoke.sh
./scripts/tag-release-gate-smoke.sh
./scripts/release-metadata-gate-smoke.sh
./scripts/release-preflight.sh --allow-unreleased
```

Then prepare `CHANGELOG.md`, confirm maturity claims against
[Maturity model](maturity-model.md), and run the tagging procedure below.
Release readiness also assumes the PR governance gate is present:
`scripts/pr-validation.py` must continue enforcing approved linked issues, one
`type:*` label, valid maturity gate evidence, compatibility classification
evidence, PR verification evidence, conventional commit subjects, and no AI
attribution trailers.

## Semver Contract

All crates in this workspace follow [Semantic Versioning 2.0.0](https://semver.org/).
The workspace uses a single shared version (lockstep). Every published crate moves to
the same version at release time.

| Change kind | Version bump |
|-------------|-------------|
| Bug fix that does not change any public API | `PATCH` |
| New public API added in a backward-compatible way | `MINOR` |
| Any breaking public API change (type removal, renamed field, changed trait signature) | `MAJOR` |
| Schema migration (new `Migration` impl in `ail-storage`) | at least `MINOR` |

### Breaking changes

A change is breaking if it requires existing dependents to update their code.
Examples in this workspace:

- Removing or renaming a type in `ail-storage`, `ail-change`, or `ail-core`.
- Changing a trait method signature (e.g., `ObjectStore`, `ContentCodec`, `Migration`).
- Modifying the CBOR wire format of any persisted type so that old data cannot be decoded.
- Removing a `pub` item that was previously exported.

### Schema migrations are MINOR, not PATCH

A new schema migration means new object-store keys are written. Old stores remain
readable (migration is additive), but the catalog version advances. This is a minor
version bump, not a patch.

## Lockstep Versioning

All 14 workspace crates share the same `version` field declared in the root `Cargo.toml`.
The motivation:

- Inter-crate dependencies within the workspace use `workspace = true` — they always
  resolve to the same version.
- A single version number in `CHANGELOG.md` describes the entire release, reducing
  the coordination overhead of per-crate changelogs.
- Downstream consumers pin a single version for the whole toolchain.

## Tagging Procedure

Use `scripts/tag-release.sh` to create a release tag:

```sh
VERSION=0.2.0 ./scripts/tag-release.sh
```

The script performs these steps:

1. **Validates** that `VERSION` is set and matches `MAJOR.MINOR.PATCH`.
2. **Checks** that the working tree is clean (no staged or unstaged changes).
3. **Runs** `scripts/docs-onboarding-smoke.sh` and
   `scripts/docs-troubleshooting-smoke.sh`, and
   `scripts/docs-language-reference-smoke.sh`, and
   `scripts/docs-compatibility-smoke.sh`, and
   `scripts/docs-stdlib-reference-smoke.sh`, and
   `scripts/docs-package-reference-smoke.sh`, `scripts/docs-performance-smoke.sh`, and
   `scripts/docs-security-smoke.sh`, and `scripts/docs-tooling-reference-smoke.sh`
   to keep user-facing CLI, language, compatibility, stdlib, package,
   performance, security, and tooling docs tied to implemented evidence.
4. **Runs** `scripts/release-metadata-gate-smoke.sh` to prove release
   metadata gate checks still fail for missing compatibility and maturity
   evidence.
5. **Runs** `scripts/pr-validation-smoke.sh` to prove PR governance checks
   still reject missing approval, label, maturity, compatibility,
   verification, and commit evidence before release publication.
6. **Runs** `scripts/release-preflight.sh` to verify release metadata:
   `VERSION` must match `workspace.package.version`, workspace crates must use
   `version.workspace = true`, `CHANGELOG.md` must contain a release heading for
   `VERSION`, maturity-claim policy docs must be present, and migration
   compatibility metadata must match the implemented storage migration target.
7. **Runs** `cargo test --workspace` — all tests must pass.
8. **Runs** `cargo deny check` — no license violations, no known advisories.
9. **Creates** an annotated tag `v$VERSION` with a standard message.

After the script exits cleanly, push the tag:

```sh
git push origin v0.2.0
```

Do NOT push the tag if any step fails. Fix the issue and re-run the script.

## Signing Flow (Stub)

GPG-signed tags are supported via the `SIGN=1` environment variable:

```sh
SIGN=1 VERSION=0.2.0 ./scripts/tag-release.sh
```

This passes `-s` to `git tag`, which uses the committer's GPG key. To enable:

1. Configure a GPG key in git: `git config --global user.signingkey <KEY_ID>`
2. Ensure `gpg-agent` is running and the key is unlocked.
3. Verify the signature after creation: `git tag -v v0.2.0`

Signed releases are recommended for any `MAJOR` or `MINOR` bump published to crates.io.
`PATCH` releases on internal branches may skip signing at maintainer discretion.

## Pre-release and RC Versions

Current release automation accepts stable `MAJOR.MINOR.PATCH` versions only.
Pre-release identifiers such as `0.2.0-rc.1` are not published to crates.io and
are also rejected by the release scripts until the validators are deliberately
relaxed.

If maintainers need an RC tag, use the full identifier:

```sh
VERSION=0.2.0-rc.1 ./scripts/tag-release.sh
```

Before running that command, update both release gates for pre-release semver:

- `scripts/tag-release.sh` must accept the pre-release suffix.
- `scripts/release-preflight.sh` must accept the same suffix when validating
  `VERSION` against `workspace.package.version`.
- The changelog must use the matching heading, for example
  `## [0.2.0-rc.1] - YYYY-MM-DD`.

## Maturity Claims

Release notes must declare exactly one maturity stage using one exact `Maturity:`
line and must not imply production readiness, Rust-comparable maturity, or
general-purpose language completeness unless the claim is backed by the gates in
[Maturity model](maturity-model.md). The preflight verifies the line is present
and valid; maintainers still review whether the evidence is strong enough for
that stage.

Before publishing a release, maintainers must classify the release using the
maturity ladder in `docs/maturity-model.md`:

| Claim level | Required release evidence |
|-------------|---------------------------|
| Validation milestone | End-to-end tests or fixtures prove the advertised slice, and limitations are visible in release notes. |
| Usable preview | A documented user workflow works from a clean checkout, with known limitations called out. |
| Real language experience | Project lifecycle, tooling, docs, stdlib, and package basics work together with integration evidence. |
| Production-ready | Compatibility, security, operational hardening, performance, migration, and ecosystem evidence exist for the documented scope. |

If evidence is incomplete, the release must use the lower claim level. Be strict
here: maturity language is a promise to users, not motivation for contributors.
Declare the stage in the active changelog section:

```md
Maturity: Validation milestone
```

## Changelog Maintenance

Update `CHANGELOG.md` with every PR that adds a user-visible change. Use the
`[Unreleased]` section. At release time, rename it to `[VERSION] - YYYY-MM-DD`
and add a new empty `[Unreleased]` above it.

For CI or PR validation before a release has been cut, run:

```sh
VERSION=$(awk '/^\[workspace\.package\]$/ { p = 1; next } /^\[/ { p = 0 } p && /^version/ { gsub(/"/, "", $3); print $3; exit }' Cargo.toml) \
  ./scripts/release-preflight.sh --allow-unreleased
```

`--allow-unreleased` keeps metadata checks active while allowing the changelog to
remain under `[Unreleased]` until the actual release branch is prepared. When
`VERSION` is omitted with `--allow-unreleased`, preflight validates the active
`[Unreleased]` section even if a historical heading exists for the current
workspace version.

## Release Preflight Metadata

`scripts/release-preflight.sh` is the compatibility gate for tag readiness. It is
read-only and checks release metadata only; it does not run tests, create tags,
or push anything.

The preflight checks:

- `docs/maturity-model.md` exists and `docs/release-policy.md` documents the
  maturity claim gate so release-readiness discipline cannot silently disappear.
- `docs/getting-started.md`, `docs/troubleshooting.md`,
  `docs/language-reference.md`, `docs/compatibility.md`,
  `docs/stdlib-reference.md`, `scripts/docs-onboarding-smoke.sh`,
  `scripts/docs-troubleshooting-smoke.sh`,
  `scripts/docs-language-reference-smoke.sh`,
  `scripts/docs-compatibility-smoke.sh`, and
  `scripts/docs-stdlib-reference-smoke.sh`, and
  `scripts/docs-package-reference-smoke.sh`,
  `scripts/docs-performance-smoke.sh`, `scripts/docs-security-smoke.sh`, and
  `scripts/docs-tooling-reference-smoke.sh` exist so validation-stage onboarding,
  CLI repair guidance, language-surface docs, tooling reference, compatibility
  policy, stdlib reference, package reference, performance validation, and
  security/runtime hardening stay tied to evidence
  instead of drifting into aspirational docs.
- Contribution governance files exist and stay wired: `CONTRIBUTING.md`, the PR
  template, `.github/workflows/pr-validation.yml`, `scripts/pr-validation.py`,
  `scripts/pr-validation-smoke.sh`, `scripts/tag-release.sh`, and
  `scripts/tag-release-gate-smoke.sh`. These protect the approved-issue,
  one-`type:*`-label, maturity-gate/evidence, compatibility classification,
  verification-evidence, conventional-commit, and no-AI-attribution requirements from silently
  becoming checklist-only again or disappearing from the actual tag path. The
  smoke also guards drift between the PR template's type labels and
  `scripts/pr-validation.py`, plus drift between maturity gates in
  `docs/maturity-model.md` and PR validation.
- `VERSION` matches `workspace.package.version`; with `--allow-unreleased`, an
  omitted `VERSION` defaults to the workspace version for local/CI validation.
  The preflight currently accepts stable `MAJOR.MINOR.PATCH` versions only; RC
  versions require relaxing this validator before tagging.
- All releasable crates under `crates/*` use `version.workspace = true`.
- `CHANGELOG.md` has either `## [VERSION] - YYYY-MM-DD` or, only with
  `--allow-unreleased`, an active `## [Unreleased]` section.
- The active changelog section declares exactly one release maturity stage:
  `Maturity: Validation milestone`, `Maturity: Usable preview`,
  `Maturity: Real language experience`, or `Maturity: Production-ready`.
- `docs/migration-guide.md` has release metadata in this form:

  ```md
  <!-- Release metadata: latest-storage-schema=3; compatibility-breaking=false -->
  ```

- `latest-storage-schema` matches the highest implemented storage migration
  target in `crates/ail-storage/src/migration.rs` and appears in the migration
  guide version table.
- If a storage migration is structurally non-equivalent, the migration guide
  metadata must set `compatibility-breaking=true`.
- If `compatibility-breaking=true`, the active changelog release notes must
  include the exact `[compatibility-breaking]` marker; prose mentioning
  compatibility-breaking without brackets does not count. If the changelog
  includes that marker, the migration guide metadata must agree.

Parser scope:

- The migration target check extracts numeric literals returned by
  `fn target_version(&self) -> u32`; computed target versions require manual
  review or a future parser-backed check.
- The structural-equivalence check detects direct `structural_equivalence: false`
  assignments and ignores line comments; indirect values require manual review.

For machine-readable preflight output, run:

```sh
./scripts/release-preflight.sh --allow-unreleased --json
```

To smoke-test the release metadata gates themselves, including the exact
`[compatibility-breaking]` marker and maturity-claim policy checks, run:

```sh
./scripts/release-metadata-gate-smoke.sh
```

Use `[compatibility-breaking]` only for release notes that require downstream
users to change code, data, package metadata, or migration procedures.
