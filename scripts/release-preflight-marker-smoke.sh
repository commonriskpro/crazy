#!/usr/bin/env bash
# Compatibility wrapper. Prefer scripts/release-metadata-gate-smoke.sh.

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec "$script_dir/release-metadata-gate-smoke.sh" "$@"
