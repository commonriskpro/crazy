#!/usr/bin/env bash
# release-preflight.sh - Validate release metadata before tagging.

set -euo pipefail

ALLOW_UNRELEASED="${ALLOW_UNRELEASED:-0}"

usage() {
    cat >&2 <<'USAGE'
Usage:
  VERSION=0.2.0 ./scripts/release-preflight.sh
  VERSION=0.2.0 ./scripts/release-preflight.sh --allow-unreleased

Checks release metadata only. It does not run tests, create tags, or push anything.
Lockstep version checks apply to releasable crates under crates/*; non-release
workspace members such as fuzz tooling are intentionally excluded.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --allow-unreleased)
            ALLOW_UNRELEASED=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage
            exit 1
            ;;
    esac
done

if [[ -z "${VERSION:-}" ]]; then
    echo "error: VERSION is not set. Usage: VERSION=x.y.z ./scripts/release-preflight.sh" >&2
    exit 1
fi

if [[ ! "$VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "error: VERSION must be MAJOR.MINOR.PATCH (e.g. 1.2.3). Got: $VERSION" >&2
    exit 1
fi

if [[ ! -f Cargo.toml || ! -f CHANGELOG.md ]]; then
    echo "error: run release preflight from the workspace root" >&2
    exit 1
fi

workspace_version=$(awk '
    /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^[[:space:]]*version[[:space:]]*=/ {
        gsub(/"/, "", $3)
        print $3
        exit
    }
' Cargo.toml)

if [[ "$workspace_version" != "$VERSION" ]]; then
    echo "error: VERSION ($VERSION) does not match workspace.package.version ($workspace_version)" >&2
    exit 1
fi

missing_workspace_version=0
for crate_manifest in crates/*/Cargo.toml; do
    if ! grep -qE '^[[:space:]]*version\.workspace[[:space:]]*=[[:space:]]*true[[:space:]]*$' "$crate_manifest"; then
        echo "error: releasable crate $crate_manifest must use version.workspace = true for lockstep releases" >&2
        missing_workspace_version=1
    fi
done

if [[ "$missing_workspace_version" == "1" ]]; then
    exit 1
fi

release_heading="^## \[$VERSION\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$"
if grep -qE "$release_heading" CHANGELOG.md; then
    :
elif [[ "$ALLOW_UNRELEASED" == "1" ]] && grep -qE '^## \[Unreleased\]$' CHANGELOG.md; then
    echo "warning: CHANGELOG.md has [Unreleased] but no [$VERSION] release heading" >&2
else
    echo "error: CHANGELOG.md must contain '## [$VERSION] - YYYY-MM-DD' before tagging" >&2
    exit 1
fi

echo "release preflight passed for v$VERSION"
