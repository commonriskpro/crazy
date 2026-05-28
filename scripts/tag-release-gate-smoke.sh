#!/usr/bin/env bash
# Smoke-check that the release tagging path keeps governance gates before tests/tagging.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAG_SCRIPT="$ROOT_DIR/scripts/tag-release.sh"

python3 - "$TAG_SCRIPT" <<'PY'
import sys
from pathlib import Path

script = Path(sys.argv[1]).read_text(encoding="utf-8")

required = {
    "onboarding docs smoke": "./scripts/docs-onboarding-smoke.sh",
    "troubleshooting docs smoke": "./scripts/docs-troubleshooting-smoke.sh",
    "language reference docs smoke": "./scripts/docs-language-reference-smoke.sh",
    "compatibility docs smoke": "./scripts/docs-compatibility-smoke.sh",
    "stdlib reference docs smoke": "./scripts/docs-stdlib-reference-smoke.sh",
    "package reference docs smoke": "./scripts/docs-package-reference-smoke.sh",
    "performance docs smoke": "./scripts/docs-performance-smoke.sh",
    "security docs smoke": "./scripts/docs-security-smoke.sh",
    "tooling reference docs smoke": "./scripts/docs-tooling-reference-smoke.sh",
    "release metadata smoke": "./scripts/release-metadata-gate-smoke.sh",
    "PR governance smoke": "./scripts/pr-validation-smoke.sh",
    "release preflight": "./scripts/release-preflight.sh",
    "workspace tests": "cargo test --workspace",
    "supply-chain audit": "cargo deny check",
    "tag creation": "git tag",
}

positions = {}
for name, needle in required.items():
    index = script.find(needle)
    if index == -1:
        raise SystemExit(f"scripts/tag-release.sh must run {name}: {needle}")
    positions[name] = index

expected_order = [
    "onboarding docs smoke",
    "troubleshooting docs smoke",
    "language reference docs smoke",
    "compatibility docs smoke",
    "stdlib reference docs smoke",
    "package reference docs smoke",
    "performance docs smoke",
    "security docs smoke",
    "tooling reference docs smoke",
    "release metadata smoke",
    "PR governance smoke",
    "release preflight",
    "workspace tests",
    "supply-chain audit",
    "tag creation",
]

for earlier, later in zip(expected_order, expected_order[1:]):
    if positions[earlier] >= positions[later]:
        raise SystemExit(f"scripts/tag-release.sh must run {earlier} before {later}")

print("tag release gate smoke passed")
PY
