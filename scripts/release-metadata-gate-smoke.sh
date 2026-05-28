#!/usr/bin/env bash
# Smoke checks for release preflight markers and maturity-gate metadata.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/release-metadata-gate-smoke.XXXXXX")
trap 'rm -rf "$tmp_root"' EXIT

write_fixture() {
    local name="$1"
    local changelog_body="$2"
    local fixture="$tmp_root/$name"

    mkdir -p "$fixture/crates/ail-storage/src" "$fixture/docs" "$fixture/.github/workflows" "$fixture/scripts"

    cat >"$fixture/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/ail-storage"]
resolver = "3"

[workspace.package]
version = "1.0.0"
EOF

    cat >"$fixture/crates/ail-storage/Cargo.toml" <<'EOF'
[package]
name = "ail-storage"
version.workspace = true
edition = "2024"
EOF

    cat >"$fixture/crates/ail-storage/src/migration.rs" <<'EOF'
struct V0ToV1;

impl V0ToV1 {
    fn target_version(&self) -> u32 { 1 }
}
EOF

    cat >"$fixture/docs/release-policy.md" <<'EOF'
# Release Policy

Run scripts/release-preflight.sh before tagging.
Use the exact [compatibility-breaking] marker for breaking release notes.
Use the Maturity model before making production-readiness claims.
Keep scripts/pr-validation.py as the PR validation governance gate.
EOF

    cat >"$fixture/docs/maturity-model.md" <<'EOF'
# Maturity model

Release claim gates.
EOF

    cat >"$fixture/docs/migration-guide.md" <<'EOF'
# Migration Guide

<!-- Release metadata: latest-storage-schema=1; compatibility-breaking=true -->

| Version | Description |
|---------|-------------|
| 1 | Baseline. |
EOF

    cat >"$fixture/CONTRIBUTING.md" <<'EOF'
# Contributing

PRs require status:approved, exactly one type:* label, conventional commit
subjects, and no Co-Authored-By trailers.
EOF

    cat >"$fixture/.github/PULL_REQUEST_TEMPLATE.md" <<'EOF'
## Maturity gate

Use the Maturity model.

## Checklist

- [ ] status:approved linked issue.
- [ ] exactly one type:* label.
EOF

    cat >"$fixture/.github/workflows/pr-validation.yml" <<'EOF'
name: PR Validation
on:
  pull_request_target:
jobs:
  validate:
    steps:
      - run: python3 scripts/pr-validation.py
EOF

    cat >"$fixture/scripts/pr-validation.py" <<'EOF'
status = "status:approved"
ALLOWED_TYPE_LABELS = {"type:feature"}
ALLOWED_MATURITY_GATES = {"Verification"}
ALLOWED_COMPATIBILITY_SURFACES = {"Runtime capability names"}
def validate_maturity_gate(): pass
def validate_compatibility(): pass
def validate_verification_section(): pass
CONVENTIONAL_COMMIT_RE = None
AI_ATTRIBUTION_PATTERNS = []
EOF

    cat >"$fixture/scripts/pr-validation-smoke.sh" <<'EOF'
bad_subject_commits='["bad"]'
ai_attribution_commits='["fix: x\n\nGenerated with Codex"]'
assert_template_labels_match_validator() { :; }
assert_maturity_gates_match_model() { :; }
missing_verification=x
empty_verification=x
missing_compatibility_surface=x
breaking_without_marker=x
EOF

    cat >"$fixture/docs/getting-started.md" <<'EOF'
# Getting started
EOF

    cat >"$fixture/docs/troubleshooting.md" <<'EOF'
# Troubleshooting
EOF

    cat >"$fixture/docs/language-reference.md" <<'EOF'
# Language reference
EOF

    cat >"$fixture/docs/compatibility.md" <<'EOF'
# Compatibility policy
EOF

    cat >"$fixture/docs/stdlib-reference.md" <<'EOF'
# Stdlib reference
EOF

    cat >"$fixture/docs/package-reference.md" <<'EOF'
# Package reference
EOF

    cat >"$fixture/docs/performance.md" <<'EOF'
# Performance validation
EOF

    cat >"$fixture/docs/security.md" <<'EOF'
# Security and runtime hardening
EOF

    cat >"$fixture/docs/tooling-reference.md" <<'EOF'
# Tooling reference
EOF

    cat >"$fixture/scripts/docs-onboarding-smoke.sh" <<'EOF'
docs/getting-started.md
cli_subcommands.rs
EOF

    cat >"$fixture/scripts/docs-troubleshooting-smoke.sh" <<'EOF'
docs/troubleshooting.md
workflow_commands.rs
capability denied: log.write
native linked execution not supported yet
EOF

    cat >"$fixture/scripts/docs-language-reference-smoke.sh" <<'EOF'
docs/language-reference.md
parser_tests.rs
expr_parser_tests.rs
op_schema.rs
EOF

    cat >"$fixture/scripts/docs-compatibility-smoke.sh" <<'EOF'
docs/compatibility.md
acl_migrator.rs
[compatibility-breaking]
latest-storage-schema=3; compatibility-breaking=false
EOF

    cat >"$fixture/scripts/docs-stdlib-reference-smoke.sh" <<'EOF'
docs/stdlib-reference.md
v1/function_entries.rs
exec/registry.rs
capability.rs
EOF

    cat >"$fixture/scripts/docs-package-reference-smoke.sh" <<'EOF'
docs/package-reference.md
manifest.rs
remote_registry/types.rs
versioning.rs
EOF

    cat >"$fixture/scripts/docs-performance-smoke.sh" <<'EOF'
docs/performance.md
incremental_tests.rs
storage_perf.rs
perf-preflight.sh
EOF

    cat >"$fixture/scripts/docs-security-smoke.sh" <<'EOF'
docs/security.md
preflight_tests.rs
secret_provider_audit_tests.rs
signing.rs
r2_attributes.rs
EOF

    cat >"$fixture/scripts/docs-tooling-reference-smoke.sh" <<'EOF'
docs/tooling-reference.md
cli_g31r2.rs
output.rs
JSON_OUTPUT_VERSION
package_cli_compat.rs
EOF

    cat >"$fixture/scripts/tag-release.sh" <<'EOF'
#!/usr/bin/env bash
./scripts/docs-onboarding-smoke.sh
./scripts/docs-troubleshooting-smoke.sh
./scripts/docs-language-reference-smoke.sh
./scripts/docs-compatibility-smoke.sh
./scripts/docs-stdlib-reference-smoke.sh
./scripts/docs-package-reference-smoke.sh
./scripts/docs-performance-smoke.sh
./scripts/docs-security-smoke.sh
./scripts/docs-tooling-reference-smoke.sh
./scripts/release-metadata-gate-smoke.sh
./scripts/pr-validation-smoke.sh
./scripts/release-preflight.sh
cargo test --workspace
cargo deny check
git tag -a v1.0.0 -m release
EOF

    cat >"$fixture/scripts/tag-release-gate-smoke.sh" <<'EOF'
expected_order=1
docs-troubleshooting-smoke.sh
docs-language-reference-smoke.sh
docs-compatibility-smoke.sh
docs-stdlib-reference-smoke.sh
docs-package-reference-smoke.sh
docs-performance-smoke.sh
docs-security-smoke.sh
docs-tooling-reference-smoke.sh
pr-validation-smoke.sh
EOF

    printf '%s\n' "$changelog_body" >"$fixture/CHANGELOG.md"
}

run_preflight() {
    local fixture="$1"
    (
        cd "$fixture"
        bash "$repo_root/scripts/release-preflight.sh" --allow-unreleased
    ) >"$fixture/stdout" 2>"$fixture/stderr"
}

assert_fails_for_missing_marker() {
    local fixture="$1"
    if run_preflight "$fixture"; then
        printf 'expected failure for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'requires the active CHANGELOG.md release notes to include [compatibility-breaking]' "$fixture/stderr"; then
        printf 'missing expected marker failure for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_fails_for_missing_maturity_policy() {
    local fixture="$1"
    python3 - "$fixture/docs/release-policy.md" <<'PY'
from pathlib import Path
path = Path(__import__('sys').argv[1])
text = path.read_text()
text = text.replace('Use the Maturity model before making production-readiness claims.\n', '')
path.write_text(text)
PY
    if run_preflight "$fixture"; then
        printf 'expected maturity policy failure for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'docs/release-policy.md must document maturity claim gates from docs/maturity-model.md' "$fixture/stderr"; then
        printf 'missing expected maturity policy failure for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_fails_for_missing_maturity_model_file() {
    local fixture="$1"
    rm "$fixture/docs/maturity-model.md"
    if run_preflight "$fixture"; then
        printf 'expected missing maturity model failure for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'missing required release metadata file: docs/maturity-model.md' "$fixture/stderr"; then
        printf 'missing expected maturity model file failure for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_fails_for_missing_changelog_maturity_stage() {
    local fixture="$1"
    if run_preflight "$fixture"; then
        printf 'expected missing changelog maturity stage failure for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'active CHANGELOG.md release notes must declare one maturity stage' "$fixture/stderr"; then
        printf 'missing expected changelog maturity stage failure for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_fails_for_multiple_changelog_maturity_stages() {
    local fixture="$1"
    if run_preflight "$fixture"; then
        printf 'expected multiple changelog maturity stages failure for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'active CHANGELOG.md release notes must declare one maturity stage' "$fixture/stderr"; then
        printf 'missing expected multiple changelog maturity stages failure for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_fails_for_invalid_changelog_maturity_stage() {
    local fixture="$1"
    if run_preflight "$fixture"; then
        printf 'expected invalid changelog maturity stage failure for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'active CHANGELOG.md release notes must declare one maturity stage' "$fixture/stderr"; then
        printf 'missing expected invalid changelog maturity stage failure for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_fails_for_missing_pr_validation_workflow() {
    local fixture="$1"
    rm "$fixture/.github/workflows/pr-validation.yml"
    if run_preflight "$fixture"; then
        printf 'expected missing PR validation workflow failure for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'missing required release metadata file: .github/workflows/pr-validation.yml' "$fixture/stderr"; then
        printf 'missing expected PR validation workflow failure for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_fails_for_missing_pr_validation_policy() {
    local fixture="$1"
    python3 - "$fixture/docs/release-policy.md" <<'PY'
from pathlib import Path
path = Path(__import__('sys').argv[1])
text = path.read_text()
text = text.replace('Keep scripts/pr-validation.py as the PR validation governance gate.\n', '')
path.write_text(text)
PY
    if run_preflight "$fixture"; then
        printf 'expected PR validation policy failure for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'docs/release-policy.md must document the PR validation governance gate' "$fixture/stderr"; then
        printf 'missing expected PR validation policy failure for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_json_rejects_invalid_version() {
    local fixture="$1"
    local name="$2"
    local invalid_version="$3"
    local status=0

    if ! command -v jq >/dev/null 2>&1; then
        printf 'jq is required for JSON smoke checks\n' >&2
        return 1
    fi

    (
        cd "$fixture"
        VERSION="$invalid_version" bash "$repo_root/scripts/release-preflight.sh" --allow-unreleased --json
    ) >"$fixture/json-stdout" 2>"$fixture/json-stderr" || status=$?

    if [[ "$status" -eq 0 ]]; then
        printf 'expected failure for %s VERSION in %s\n' "$name" "$fixture" >&2
        return 1
    fi

    if ! jq . "$fixture/json-stdout" >/dev/null; then
        printf '%s VERSION did not emit valid JSON for %s\n' "$name" "$fixture" >&2
        return 1
    fi

    if ! jq -e '.status == "failed" and any(.failures[]; contains("VERSION must be MAJOR.MINOR.PATCH"))' "$fixture/json-stdout" >/dev/null; then
        printf '%s VERSION did not emit the expected JSON validation failure for %s\n' "$name" "$fixture" >&2
        return 1
    fi
}

assert_text_passes_with_empty_warnings() {
    local fixture="$1"

    (
        cd "$fixture"
        VERSION=1.0.0 bash "$repo_root/scripts/release-preflight.sh"
    ) >"$fixture/text-stdout" 2>"$fixture/text-stderr"

    if ! grep -qF 'release preflight passed for v1.0.0' "$fixture/text-stdout"; then
        printf 'text preflight did not pass cleanly for %s\n' "$fixture" >&2
        return 1
    fi
}

assert_allow_unreleased_prefers_unreleased_section() {
    local fixture="$1"
    if run_preflight "$fixture"; then
        printf 'expected allow-unreleased to validate [Unreleased] for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'checking CHANGELOG.md [Unreleased] section for unreleased preflight' "$fixture/stderr"; then
        printf 'missing expected [Unreleased] selection warning for %s\n' "$fixture" >&2
        return 1
    fi
    if ! grep -qF 'active CHANGELOG.md release notes must declare one maturity stage' "$fixture/stderr"; then
        printf 'missing expected [Unreleased] maturity failure for %s\n' "$fixture" >&2
        return 1
    fi
}

plain_prose_changelog='# Changelog

## [Unreleased]

Maturity: Validation milestone

- This release is compatibility-breaking for old clients.
'

exact_marker_changelog='# Changelog

## [Unreleased]

Maturity: Validation milestone

- [compatibility-breaking] Existing stores require migration review.
'

exact_release_changelog='# Changelog

## [Unreleased]

## [1.0.0] - 2026-05-26

Maturity: Validation milestone

- [compatibility-breaking] Existing stores require migration review.
'

outside_marker_changelog='# Changelog

## [Unreleased]

Maturity: Validation milestone

- Normal unreleased note.

## [0.9.0] - 2025-01-01

- [compatibility-breaking] Historical breaking change.
'

missing_maturity_stage_changelog='# Changelog

## [Unreleased]

- [compatibility-breaking] Existing stores require migration review.
'

multiple_maturity_stages_changelog='# Changelog

## [Unreleased]

Maturity: Validation milestone
Maturity: Usable preview

- [compatibility-breaking] Existing stores require migration review.
'

invalid_maturity_stage_changelog='# Changelog

## [Unreleased]

Maturity: Validation milestone
Maturity: Aspirational production

- [compatibility-breaking] Existing stores require migration review.
'

unreleased_without_stage_but_versioned_has_stage_changelog='# Changelog

## [Unreleased]

- [compatibility-breaking] Existing stores require migration review.

## [1.0.0] - 2026-05-26

Maturity: Validation milestone

- [compatibility-breaking] Existing stores require migration review.
'

write_fixture plain-prose "$plain_prose_changelog"
assert_fails_for_missing_marker "$tmp_root/plain-prose"

write_fixture exact-marker "$exact_marker_changelog"
run_preflight "$tmp_root/exact-marker"

write_fixture missing-maturity-policy "$exact_marker_changelog"
assert_fails_for_missing_maturity_policy "$tmp_root/missing-maturity-policy"

write_fixture missing-maturity-model-file "$exact_marker_changelog"
assert_fails_for_missing_maturity_model_file "$tmp_root/missing-maturity-model-file"

write_fixture missing-changelog-maturity-stage "$missing_maturity_stage_changelog"
assert_fails_for_missing_changelog_maturity_stage "$tmp_root/missing-changelog-maturity-stage"

write_fixture multiple-changelog-maturity-stages "$multiple_maturity_stages_changelog"
assert_fails_for_multiple_changelog_maturity_stages "$tmp_root/multiple-changelog-maturity-stages"

write_fixture invalid-changelog-maturity-stage "$invalid_maturity_stage_changelog"
assert_fails_for_invalid_changelog_maturity_stage "$tmp_root/invalid-changelog-maturity-stage"

write_fixture allow-unreleased-prefers-unreleased "$unreleased_without_stage_but_versioned_has_stage_changelog"
assert_allow_unreleased_prefers_unreleased_section "$tmp_root/allow-unreleased-prefers-unreleased"

write_fixture missing-pr-validation-workflow "$exact_marker_changelog"
assert_fails_for_missing_pr_validation_workflow "$tmp_root/missing-pr-validation-workflow"

write_fixture missing-pr-validation-policy "$exact_marker_changelog"
assert_fails_for_missing_pr_validation_policy "$tmp_root/missing-pr-validation-policy"

write_fixture exact-release "$exact_release_changelog"
assert_text_passes_with_empty_warnings "$tmp_root/exact-release"

write_fixture outside-marker "$outside_marker_changelog"
assert_fails_for_missing_marker "$tmp_root/outside-marker"

write_fixture json-tabbed-version "$exact_marker_changelog"
assert_json_rejects_invalid_version "$tmp_root/json-tabbed-version" tabbed $'0.1.0\tbad'

write_fixture json-newline-version "$exact_marker_changelog"
assert_json_rejects_invalid_version "$tmp_root/json-newline-version" newline $'0.1.0\nbad'

printf 'release preflight marker and maturity-gate smoke checks passed\n'
