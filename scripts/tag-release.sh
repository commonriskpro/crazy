#!/usr/bin/env bash
# tag-release.sh - Create an annotated release tag for the AIL workspace.
#
# Usage:
#   VERSION=0.2.0 ./scripts/tag-release.sh
#   SIGN=1 VERSION=0.2.0 ./scripts/tag-release.sh   # GPG-signed tag
#
# Prerequisites:
#   - Clean working tree (no staged or unstaged changes)
#   - cargo, cargo-deny installed
#   - gpg configured (when SIGN=1)
#
# The script does NOT push the tag. After review, push with:
#   git push origin v$VERSION

set -euo pipefail

# ── Validation ────────────────────────────────────────────────────────────────

if [[ -z "${VERSION:-}" ]]; then
    echo "error: VERSION is not set. Usage: VERSION=x.y.z ./scripts/tag-release.sh" >&2
    exit 1
fi

# Validate semver format (MAJOR.MINOR.PATCH, no leading zeros, no pre-release suffix).
if ! echo "$VERSION" | grep -qE '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'; then
    echo "error: VERSION must be MAJOR.MINOR.PATCH (e.g. 1.2.3). Got: $VERSION" >&2
    exit 1
fi

TAG="v${VERSION}"

# -- Clean worktree check ------------------------------------------------------

if ! git diff --quiet || ! git diff --staged --quiet; then
    echo "error: working tree is not clean. Commit or stash changes before tagging." >&2
    git status --short >&2
    exit 1
fi

# -- Release metadata preflight ------------------------------------------------

echo "==> Running release metadata preflight ..."
./scripts/release-preflight.sh

# -- Test suite ----------------------------------------------------------------

echo "==> Running cargo test --workspace ..."
cargo test --workspace

# -- Supply-chain audit --------------------------------------------------------

echo "==> Running cargo deny check ..."
cargo deny check

# -- Create annotated tag ------------------------------------------------------

SIGN_FLAG=""
if [[ "${SIGN:-0}" == "1" ]]; then
    SIGN_FLAG="-s"
    echo "==> Creating GPG-signed annotated tag ${TAG} ..."
else
    echo "==> Creating annotated tag ${TAG} ..."
fi

git tag ${SIGN_FLAG} -a "${TAG}" -m "Release ${TAG}

See CHANGELOG.md for the list of changes included in this release.

Tagging procedure: scripts/tag-release.sh
Release policy:   docs/release-policy.md"

echo ""
echo "Tag ${TAG} created. To push:"
echo "  git push origin ${TAG}"
