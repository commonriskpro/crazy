#!/usr/bin/env bash
# Static smoke checks for validation-stage security and runtime hardening docs.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/security.md"
RUNTIME_HOST="$ROOT_DIR/crates/ail-runtime/src/host.rs"
RUNTIME_PROFILE="$ROOT_DIR/crates/ail-runtime/src/profile.rs"
RUNTIME_SECRET="$ROOT_DIR/crates/ail-runtime/src/secret.rs"
PREFLIGHT_TEST="$ROOT_DIR/crates/ail-runtime/tests/preflight_tests.rs"
RESOURCE_TEST="$ROOT_DIR/crates/ail-runtime/tests/resource_limits_tests.rs"
TRUST_TEST="$ROOT_DIR/crates/ail-runtime/tests/handler_trust_tests.rs"
SECRET_AUDIT_TEST="$ROOT_DIR/crates/ail-runtime/tests/secret_provider_audit_tests.rs"
CONTEXT_REDACTION="$ROOT_DIR/crates/ail-context/src/redaction.rs"
CONTEXT_R2_TEST="$ROOT_DIR/crates/ail-context/src/builder_tests/r2_attributes.rs"
PACKAGE_SIGNING="$ROOT_DIR/crates/ail-package/src/signing.rs"
PACKAGE_ADVISORY="$ROOT_DIR/crates/ail-package/src/advisory.rs"
PACKAGE_RESOLVER="$ROOT_DIR/crates/ail-package/src/resolver.rs"
PACKAGE_LIFECYCLE="$ROOT_DIR/crates/ail-package/tests/lifecycle.rs"
MATURITY="$ROOT_DIR/docs/maturity-model.md"

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
require_literal "$DOC" "not a production security guarantee" "production caveat"
require_literal "$DOC" "deny-by-default runtime gate" "deny-by-default framing"
require_literal "$DOC" "import != grant" "package grant invariant"
require_literal "$DOC" "In-flight revocation policy is stored but not currently enforced" "revocation gap"
require_literal "$DOC" "denial_category as audit-only data" "audit-only denial category"
require_literal "$DOC" "./scripts/docs-security-smoke.sh" "verification command"

for symbol in "Package trust gate" "Capability grant check" "schema_registry" "denial_category" "CapabilityRevocationRegistry"; do
  require_literal "$RUNTIME_HOST" "$symbol" "runtime host evidence $symbol"
  require_literal "$DOC" "$symbol" "documented runtime host evidence $symbol"
done

for symbol in ResourceLimits CapabilityRevocationRegistry InFlightPolicy with_min_handler_trust RuntimeProfile; do
  require_literal "$RUNTIME_PROFILE" "$symbol" "runtime profile evidence $symbol"
  require_literal "$DOC" "$symbol" "documented runtime profile evidence $symbol"
done

for symbol in SecretReadHandler SecretProviderError SecretVault; do
  require_literal "$RUNTIME_SECRET" "$symbol" "secret evidence $symbol"
  require_literal "$DOC" "$symbol" "documented secret evidence $symbol"
done

require_literal "$PREFLIGHT_TEST" "ungranted_capability_denied" "capability denial test"
require_literal "$RESOURCE_TEST" "ResourceLimits enforcement" "resource limits test scope"
require_literal "$TRUST_TEST" "with_min_handler_trust" "handler trust test"
require_literal "$SECRET_AUDIT_TEST" "denial_category must not contain secret" "secret audit non-oracle test"
require_literal "$CONTEXT_REDACTION" "filter_redacted" "context redaction implementation"
require_literal "$CONTEXT_R2_TEST" "E_ACCESS_DENIED" "context access denial test"

for symbol in PackageKeypair tampered_manifest_rejects_signature TransparencyLog; do
  require_literal "$PACKAGE_SIGNING" "$symbol" "package signing evidence $symbol"
  require_literal "$DOC" "$symbol" "documented package signing evidence $symbol"
done

for symbol in SecurityAdvisory AdvisoryChecker; do
  require_literal "$PACKAGE_ADVISORY" "$symbol" "advisory evidence $symbol"
  require_literal "$DOC" "$symbol" "documented advisory evidence $symbol"
done

for symbol in ResolverError CapabilityConflict HandlerConflict; do
  require_literal "$PACKAGE_RESOLVER" "$symbol" "resolver security evidence $symbol"
  require_literal "$DOC" "$symbol" "documented resolver evidence $symbol"
done

require_literal "$PACKAGE_LIFECYCLE" "tampered package must be rejected" "package lifecycle tamper test"
require_literal "$MATURITY" "Runtime safety" "maturity runtime gate"
require_literal "$DOC" "Runtime safety" "security doc maturity reference"

printf 'docs security smoke passed\n'
