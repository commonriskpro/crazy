# AIL compatibility policy

<!-- Status: Implemented subset. This policy defines the current compatibility review contract for v0.x validation releases; it does not claim production-ready backwards compatibility. -->

AIL cannot become a serious language if users have to guess what breaks. This policy turns compatibility into a reviewable artifact: every change must name the surface it touches, whether it is breaking, and what evidence proves the claim.

## Quick path

1. Find the touched surface in the matrix below.
2. Classify the change as compatible, compatibility-risky, or breaking.
3. Update `CHANGELOG.md`, [Migration guide](migration-guide.md), and [Release policy](release-policy.md) when the matrix says so.
4. Run the docs/release smokes before claiming compatibility evidence.

## Current compatibility status

| Area | Current promise | Required evidence before stronger claims |
|---|---|---|
| Rust crate APIs | Semver is enforced by release policy, but v0.x APIs may still change. | Public API diffing or compatibility tests before claiming stable API. |
| Storage schema | Current built-in catalog migrates stores to schema v3; v1-v3 are structural no-ops. | Migration fixtures for old stores and rollback drills for structural migrations. |
| ACL / ChangeSet syntax | Implemented subset documented in [Language reference](language-reference.md); not stable yet. | Parser/canonicalizer compatibility fixtures and deprecation windows. |
| Semantic Graph schema | Source of truth is stable as an architectural invariant, not as a finalized wire schema. | Graph schema versioning, old graph load fixtures, and explicit migrations. |
| CLI human output | User-facing, but not stable enough for machine consumers. | Snapshot fixtures for documented messages before promising stability. |
| CLI JSON output | Compatibility-sensitive when used by automation. | JSON schema fixtures and versioned diagnostics contracts. |
| Runtime capability names | Compatibility-sensitive because grants and policies depend on exact IDs. | Capability registry/version tests and migration notes for renamed capabilities. |
| WASM runtime ABI | Validation-stage only. | ABI fixtures across releases and translation-validation evidence. |
| Native object artifacts | Experimental object output; `ail run --target native` is not supported yet. | Linked execution support plus object/ABI compatibility fixtures. |
| Stdlib APIs | Implemented subset documented in [Stdlib reference](stdlib-reference.md), with registry stability metadata but no v0.x production stability promise. | Versioned contracts, examples, and compatibility tests per stdlib item. |
| Package metadata/lockfiles | Implemented subset documented in [Package reference](package-reference.md), including manifest, lockfile, signing, advisory, yanking, registry DTO, and compatibility metadata primitives. | Lockfile round-trip fixtures, resolver compatibility tests, deployed registry policy, and package migration fixtures. |
| Documentation/process only | No runtime or user data compatibility surface is touched. | Docs/process smoke evidence proving the change stays synchronized with implementation or policy. |

## Classification rules

| Classification | Meaning | Required action |
|---|---|---|
| Compatible | Existing documented users should not need to change code, data, policy, grants, or automation. | Add normal changelog entry and relevant tests/docs. |
| Compatibility-risky | The change should be compatible, but touches persisted data, CLI JSON, ACL syntax, capability IDs, ABI, or package metadata. | Add explicit compatibility evidence in the PR and changelog. |
| Breaking | Existing users must change code, data, policy, grants, package metadata, migration procedure, or automation. | Add `[compatibility-breaking]` to active changelog notes, update migration/deprecation docs, use `type:breaking-change`, and use the release policy gates. |
| Not applicable | The PR only changes documentation/process and does not alter a compatibility surface. | Use `Documentation/process only` as the surface and explain why no user compatibility surface is touched. |

## Breaking-change examples

Treat these as breaking unless evidence proves otherwise:

- removing or renaming public Rust APIs in releasable crates;
- changing persisted CBOR shape without a migration path;
- removing or changing ACL syntax accepted by the current parser;
- changing canonicalization in a way that makes old ChangeSets apply differently;
- renaming capability IDs such as `log.write`;
- changing CLI JSON field names, status strings, or diagnostic codes used by automation;
- changing package manifest or lockfile semantics;
- changing runtime ABI/value layout for already documented executable paths.

## Deprecation process

Use deprecation when a behavior should go away but does not need to break users immediately.

1. Mark the old behavior as deprecated in docs and changelog.
2. Keep the old behavior working for at least one release stage unless there is a safety issue.
3. Add a repair or migration path before removal.
4. Add compatibility evidence: parser fixture, migration fixture, JSON fixture, or docs smoke depending on the surface.
5. Only remove the behavior in a release whose notes explain the break and include `[compatibility-breaking]` when users must act.

For ACL operations, prefer an explicit migrator path before removal. The current code already has an ACL migrator for deprecated op verbs such as `create_fn` -> `create_function`; future removals should follow that shape instead of silently rejecting old ChangeSets.

## Review checklist

Before approving a compatibility-sensitive PR, confirm:

- [ ] The PR names the touched compatibility surface and classification.
- [ ] The maturity gate evidence says whether compatibility moved forward or stayed unchanged.
- [ ] `CHANGELOG.md` names the user-visible compatibility effect.
- [ ] `docs/migration-guide.md` changes when persisted data or schema changes.
- [ ] `docs/language-reference.md` changes when ACL or expression surface changes.
- [ ] `docs/troubleshooting.md` changes when user-facing diagnostics or repair actions change.
- [ ] `[compatibility-breaking]` appears in release notes if users must act.

## Release gate relationship

`release-preflight.sh` currently enforces the pieces that are cheap and deterministic: required docs exist, migration metadata agrees with implemented storage migration targets, active changelog maturity metadata is present, and compatibility-breaking metadata matches release notes.

That is not the same as production compatibility. The long-term bar is a compatibility test matrix with old-project fixtures, old-ChangeSet fixtures, old-store fixtures, JSON schema fixtures, package lockfile fixtures, and runtime ABI fixtures.
