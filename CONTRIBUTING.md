# Contributing to AIL

AIL is validation-stage language infrastructure. Contributions should move it toward a Rust-comparable level of maturity without weakening its core model: Semantic Graph as source of truth, LLM-proposed ChangeSets, verification-first acceptance, and explicit capabilities.

## Quick path

1. Read [Codebase guide](docs/CODEBASE-GUIDE.md) and [Maintainer playbook](docs/codebase/maintainer-playbook.md).
2. Identify the gate your change advances in [AIL maturity model](docs/maturity-model.md). If it touches a compatibility-sensitive surface, classify it with [Compatibility policy](docs/compatibility.md).
3. Keep implementation evidence with the change: tests, fixtures, docs, scripts, or release-gate checks.
4. Open a PR using the template, link an approved issue, choose exactly one `type:*` label, name a valid maturity gate plus evidence, and list verification commands or why they were not run. PR validation will enforce the issue link, `status:approved`, label count, maturity gate/evidence, compatibility surface/classification/evidence, verification evidence, conventional commit subjects, and no AI attribution trailers.

## Contribution rules

| Rule | Why it matters |
|------|----------------|
| Preserve Semantic Graph source of truth. | Text files are not the authoritative program model. |
| Keep LLM output as proposals, not authority. | The verifier/toolchain decides what is accepted. |
| Preserve verification-first acceptance. | Claims without evidence are not maturity. |
| Keep capabilities explicit. | No ambient effects or hidden runtime permissions. |
| Match claims to evidence. | Do not imply production-readiness or Rust-comparable maturity unless the maturity model supports it. |
| Update `CHANGELOG.md` for user-visible or maintainer workflow changes. | Maintainers need traceability. |

## Verification

Run the checks that match the files you touched. For release/process changes, use:

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
./scripts/pr-validation-smoke.sh
./scripts/tag-release-gate-smoke.sh
./scripts/release-metadata-gate-smoke.sh
./scripts/release-preflight.sh --allow-unreleased
```

For Rust code changes, follow the relevant crate docs and maintainer playbook. Do not claim broader coverage than the commands actually prove.

## Pull requests

The PR template asks for:

- linked approved issue;
- exactly one PR type / `type:*` label;
- valid maturity gate from the maturity model plus evidence added;
- compatibility surface/classification/evidence from the compatibility policy;
- verification commands/checks, or an explicit not-run explanation;
- conventional commit subjects with no AI attribution or `Co-Authored-By` trailers;
- summary;
- maturity gate advanced;
- evidence added;
- exact verification commands;
- reviewer focus.

If a change does not advance a maturity gate, explain why it is worth reviewing now. Small polish can be valuable, but it should not masquerade as progress toward production-grade maturity.
