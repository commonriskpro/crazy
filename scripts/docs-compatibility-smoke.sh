#!/usr/bin/env bash
# Static smoke checks for the validation-stage compatibility policy.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/compatibility.md"
RELEASE_POLICY="$ROOT_DIR/docs/release-policy.md"
MIGRATION_GUIDE="$ROOT_DIR/docs/migration-guide.md"
MATURITY_MODEL="$ROOT_DIR/docs/maturity-model.md"
LANGUAGE_REFERENCE="$ROOT_DIR/docs/language-reference.md"
TROUBLESHOOTING="$ROOT_DIR/docs/troubleshooting.md"
ACL_MIGRATOR="$ROOT_DIR/crates/ail-change/src/acl_migrator.rs"
RELEASE_PREFLIGHT="$ROOT_DIR/scripts/release-preflight.sh"

require_literal() {
  local file="$1"
  local literal="$2"
  local label="$3"

  if ! grep -qF "$literal" "$file"; then
    printf 'missing %s in %s: %s\n' "$label" "$file" "$literal" >&2
    return 1
  fi
}

require_literal "$DOC" "<!-- Status: Implemented subset." "implemented-subset status"
require_literal "$DOC" "does not claim production-ready backwards compatibility" "production caveat"
require_literal "$DOC" "Rust crate APIs" "rust api matrix row"
require_literal "$DOC" "Storage schema" "storage matrix row"
require_literal "$DOC" "ACL / ChangeSet syntax" "acl matrix row"
require_literal "$DOC" "Semantic Graph schema" "semantic graph matrix row"
require_literal "$DOC" "CLI JSON output" "cli json matrix row"
require_literal "$DOC" "Runtime capability names" "capability matrix row"
require_literal "$DOC" "WASM runtime ABI" "wasm abi matrix row"
require_literal "$DOC" "Stdlib APIs" "stdlib matrix row"
require_literal "$DOC" "Package metadata/lockfiles" "package matrix row"
require_literal "$DOC" "Documentation/process only" "docs/process matrix row"
require_literal "$DOC" "[compatibility-breaking]" "breaking marker policy"
require_literal "$DOC" "Not applicable" "not-applicable classification"
require_literal "$DOC" "Deprecation process" "deprecation process"
require_literal "$DOC" "create_fn" "acl migrator example"
require_literal "$DOC" "release-preflight.sh" "release preflight relationship"

require_literal "$RELEASE_POLICY" "[compatibility-breaking]" "release policy breaking marker"
require_literal "$RELEASE_POLICY" "scripts/release-preflight.sh" "release policy preflight"
require_literal "$MIGRATION_GUIDE" "latest-storage-schema=3; compatibility-breaking=false" "migration metadata"
require_literal "$MATURITY_MODEL" "Compatibility" "maturity compatibility gate"
require_literal "$LANGUAGE_REFERENCE" "A verb can parse and still become a no-op" "language compatibility caveat"
require_literal "$TROUBLESHOOTING" "not a production-readiness claim" "troubleshooting caveat"
require_literal "$ACL_MIGRATOR" "create_fn" "acl migrator source example"
require_literal "$ACL_MIGRATOR" "create_function" "acl migrator target example"
require_literal "$RELEASE_PREFLIGHT" "compatibility-breaking" "preflight compatibility gate"
require_literal "$ROOT_DIR/scripts/pr-validation.py" "ALLOWED_COMPATIBILITY_SURFACES" "validator compatibility surfaces"
require_literal "$ROOT_DIR/scripts/pr-validation.py" "validate_compatibility" "validator compatibility function"
require_literal "$ROOT_DIR/scripts/pr-validation-smoke.sh" "missing_compatibility_surface" "pr smoke missing compatibility case"

printf 'docs compatibility smoke passed\n'
