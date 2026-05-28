#!/usr/bin/env bash
# Static smoke checks for the validation-stage package reference.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/package-reference.md"
LIB="$ROOT_DIR/crates/ail-package/src/lib.rs"
MANIFEST="$ROOT_DIR/crates/ail-package/src/manifest.rs"
TRUST="$ROOT_DIR/crates/ail-package/src/trust.rs"
POLICY="$ROOT_DIR/crates/ail-package/src/policy.rs"
VERIFICATION="$ROOT_DIR/crates/ail-package/src/verification.rs"
RESOLVER="$ROOT_DIR/crates/ail-package/src/resolver.rs"
LOCKFILE="$ROOT_DIR/crates/ail-package/src/lockfile.rs"
SIGNING="$ROOT_DIR/crates/ail-package/src/signing.rs"
REGISTRY="$ROOT_DIR/crates/ail-package/src/registry.rs"
REMOTE_TYPES="$ROOT_DIR/crates/ail-package/src/remote_registry/types.rs"
SIGNED_PUBLISH="$ROOT_DIR/crates/ail-package/src/remote_registry/signed_publish.rs"
ADVISORY="$ROOT_DIR/crates/ail-package/src/advisory.rs"
YANK="$ROOT_DIR/crates/ail-package/src/yank.rs"
VERSIONING="$ROOT_DIR/crates/ail-package/src/versioning.rs"
COMPAT="$ROOT_DIR/docs/compatibility.md"

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
require_literal "$DOC" "not a production registry or ecosystem stability claim" "production caveat"
require_literal "$DOC" "importing a package never grants runtime capabilities automatically" "import-not-grant invariant"
require_literal "$DOC" "Unsafe < Unverified < Assumed < Verified" "trust ordering"
require_literal "$DOC" "Yanked versions remain available" "yank reproducibility invariant"

for module in advisory assumption coherence export generated_artifact handler http_registry import lockfile manifest namespace policy registry remote_registry resolver signing surface trust verification versioning yank; do
  require_literal "$LIB" "pub mod $module;" "lib module $module"
done

for symbol in PackageManifest PackageDef ReproducibleBuildEvidence Provenance ArtifactHashEntry blake3_hex validate; do
  require_literal "$MANIFEST" "$symbol" "manifest evidence $symbol"
  require_literal "$DOC" "$symbol" "documented manifest $symbol"
done

for trust in Unsafe Unverified Assumed Verified; do
  require_literal "$TRUST" "$trust" "trust source $trust"
  require_literal "$DOC" "$trust" "documented trust $trust"
done

for symbol in TrustGate CapabilityPolicyEnforcer DeploymentProfile UnsafeSurfacePolicyEnforcer; do
  require_literal "$POLICY" "$symbol" "policy evidence $symbol"
done

for symbol in PackageVerificationReport validate_verified_package_evidence MissingReproducibleEvidence ArtifactHashesMismatch; do
  require_literal "$VERIFICATION" "$symbol" "verification evidence $symbol"
  require_literal "$DOC" "$symbol" "documented verification $symbol"
done

for symbol in DependencySpec DependencyResolver ResolverError; do
  require_literal "$RESOLVER" "$symbol" "resolver evidence $symbol"
  require_literal "$DOC" "$symbol" "documented resolver $symbol"
done

for symbol in LockfileEntry Lockfile verify_integrity; do
  require_literal "$LOCKFILE" "$symbol" "lockfile evidence $symbol"
  require_literal "$DOC" "$symbol" "documented lockfile $symbol"
done

for symbol in PackageKeypair PackageSignature SignedPackage TransparencyLogEntry TransparencyLog; do
  require_literal "$SIGNING" "$symbol" "signing evidence $symbol"
  require_literal "$DOC" "$symbol" "documented signing $symbol"
done

for symbol in PackageRegistry register_signed lookup_by_name_version is_yanked; do
  require_literal "$REGISTRY" "$symbol" "registry evidence $symbol"
done

for symbol in PublishRequest FetchRequest SearchRequest VerifyRequest VerifyOutcome; do
  require_literal "$REMOTE_TYPES" "$symbol" "remote DTO evidence $symbol"
  require_literal "$DOC" "$symbol" "documented remote DTO $symbol"
done

require_literal "$SIGNED_PUBLISH" "publish_signed" "signed publish evidence"
require_literal "$SIGNED_PUBLISH" "verifying its signature" "signed publish verification comment"
require_literal "$DOC" "publish_signed" "documented signed publish"

for symbol in SecurityAdvisory AdvisorySeverity AdvisoryChecker; do
  require_literal "$ADVISORY" "$symbol" "advisory evidence $symbol"
  require_literal "$DOC" "$symbol" "documented advisory $symbol"
done

require_literal "$YANK" "YankRecord" "yank evidence"
require_literal "$YANK" "Yanked packages are NOT removed" "yank reproducibility source"
require_literal "$DOC" "YankRecord" "documented yank"

for symbol in CompatibilityClass PackageVersioning MigrationRecord MigrationStep PackageCompatibilityMetadata CompatibilityEngine; do
  require_literal "$VERSIONING" "$symbol" "versioning evidence $symbol"
  require_literal "$DOC" "$symbol" "documented versioning $symbol"
done

require_literal "$COMPAT" "Package metadata/lockfiles" "compatibility package surface"
require_literal "$DOC" "import != grant" "package invariant text"

printf 'docs package reference smoke passed\n'
