#!/usr/bin/env bash
# Smoke checks for exact compatibility-breaking marker handling.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/release-preflight-marker-smoke.XXXXXX")
trap 'rm -rf "$tmp_root"' EXIT

write_fixture() {
    local name="$1"
    local changelog_body="$2"
    local fixture="$tmp_root/$name"

    mkdir -p "$fixture/crates/ail-storage/src" "$fixture/docs"

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
EOF

    cat >"$fixture/docs/migration-guide.md" <<'EOF'
# Migration Guide

<!-- Release metadata: latest-storage-schema=1; compatibility-breaking=true -->

| Version | Description |
|---------|-------------|
| 1 | Baseline. |
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

plain_prose_changelog='# Changelog

## [Unreleased]

- This release is compatibility-breaking for old clients.
'

exact_marker_changelog='# Changelog

## [Unreleased]

- [compatibility-breaking] Existing stores require migration review.
'

exact_release_changelog='# Changelog

## [Unreleased]

## [1.0.0] - 2026-05-26

- [compatibility-breaking] Existing stores require migration review.
'

outside_marker_changelog='# Changelog

## [Unreleased]

- Normal unreleased note.

## [0.9.0] - 2025-01-01

- [compatibility-breaking] Historical breaking change.
'

write_fixture plain-prose "$plain_prose_changelog"
assert_fails_for_missing_marker "$tmp_root/plain-prose"

write_fixture exact-marker "$exact_marker_changelog"
run_preflight "$tmp_root/exact-marker"

write_fixture exact-release "$exact_release_changelog"
assert_text_passes_with_empty_warnings "$tmp_root/exact-release"

write_fixture outside-marker "$outside_marker_changelog"
assert_fails_for_missing_marker "$tmp_root/outside-marker"

write_fixture json-tabbed-version "$exact_marker_changelog"
assert_json_rejects_invalid_version "$tmp_root/json-tabbed-version" tabbed $'0.1.0\tbad'

write_fixture json-newline-version "$exact_marker_changelog"
assert_json_rejects_invalid_version "$tmp_root/json-newline-version" newline $'0.1.0\nbad'

printf 'release preflight marker smoke checks passed\n'
