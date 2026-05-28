## Linked issue

Closes #

The linked issue must have `status:approved`.

## PR type

Check exactly one and add the matching `type:*` label:

- [ ] Bug fix — `type:bug`
- [ ] New feature — `type:feature`
- [ ] Documentation only — `type:docs`
- [ ] Code refactoring — `type:refactor`
- [ ] Maintenance/tooling — `type:chore`
- [ ] Breaking change — `type:breaking-change`

## Summary

-
-

## Maturity gate

Which gate from [AIL maturity model](../docs/maturity-model.md) does this advance?

Allowed gates: Language surface, Verification, Runtime safety, Tooling UX, Package ecosystem, Standard library, Compatibility, Performance, Documentation, AI-native workflow.

Gate:
Evidence added:

## Compatibility

Classify this with [Compatibility policy](../docs/compatibility.md).

Allowed surfaces: Rust crate APIs, Storage schema, ACL / ChangeSet syntax, Semantic Graph schema, CLI human output, CLI JSON output, Runtime capability names, WASM runtime ABI, Native object artifacts, Stdlib APIs, Package metadata/lockfiles, Documentation/process only.

Allowed classifications: Compatible, Compatibility-risky, Breaking, Not applicable.

Compatibility surface:
Compatibility classification:
Compatibility evidence:

If this is breaking, use `type:breaking-change` and mention `[compatibility-breaking]` here.

## Release / claim discipline

- [ ] I did not claim production-readiness, Rust-comparable maturity, or general-purpose completeness beyond the evidence in [Maturity model](../docs/maturity-model.md).
- [ ] I updated `CHANGELOG.md` for user-visible behavior, maintainer workflow, release process, or compatibility changes.

## Verification

List the exact commands or checks run:

```sh

```

If a relevant check was not run, explain why.

## Reviewer notes

What should reviewers inspect first?

-

## Checklist

- [ ] I linked an approved issue.
- [ ] I selected exactly one PR type and added exactly one matching `type:*` label.
- [ ] Every commit uses a conventional commit subject and has no AI attribution or `Co-Authored-By` trailer.
- [ ] I checked [Maintainer playbook](../docs/codebase/maintainer-playbook.md).
- [ ] I kept Semantic Graph source-of-truth, verification-first acceptance, and explicit capability boundaries intact.
- [ ] I updated docs/status labels when implementation scope changed.
