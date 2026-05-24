# Release Policy

<!-- Status: Implemented subset. Release policy reflects current lockstep workspace versioning and tag script expectations. -->

This document describes the versioning contract, tagging procedure, signing policy, and
lockstep versioning rationale for the AIL workspace.

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
3. **Runs** `scripts/release-preflight.sh` to verify release metadata:
   `VERSION` must match `workspace.package.version`, workspace crates must use
   `version.workspace = true`, `CHANGELOG.md` must contain a release heading for
   `VERSION`, and migration compatibility metadata must match the implemented
   storage migration target.
4. **Runs** `cargo test --workspace` — all tests must pass.
5. **Runs** `cargo deny check` — no license violations, no known advisories.
6. **Creates** an annotated tag `v$VERSION` with a standard message.

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
remain under `[Unreleased]` until the actual release branch is prepared.

## Release Preflight Metadata

`scripts/release-preflight.sh` is the compatibility gate for tag readiness. It is
read-only and checks release metadata only; it does not run tests, create tags,
or push anything.

The preflight checks:

- `VERSION` matches `workspace.package.version`; with `--allow-unreleased`, an
  omitted `VERSION` defaults to the workspace version for local/CI validation.
  The preflight currently accepts stable `MAJOR.MINOR.PATCH` versions only; RC
  versions require relaxing this validator before tagging.
- All releasable crates under `crates/*` use `version.workspace = true`.
- `CHANGELOG.md` has either `## [VERSION] - YYYY-MM-DD` or, only with
  `--allow-unreleased`, an active `## [Unreleased]` section.
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

Use `[compatibility-breaking]` only for release notes that require downstream
users to change code, data, package metadata, or migration procedures.
