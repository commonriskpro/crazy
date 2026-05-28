#!/usr/bin/env bash
# release-preflight.sh - Validate release metadata before tagging.

set -euo pipefail

ALLOW_UNRELEASED="${ALLOW_UNRELEASED:-0}"
OUTPUT_FORMAT="text"
version_defaulted_for_unreleased=0
failures=()
warnings=()

usage() {
    cat >&2 <<'USAGE'
Usage:
  VERSION=0.2.0 ./scripts/release-preflight.sh
  VERSION=0.2.0 ./scripts/release-preflight.sh --allow-unreleased
  ./scripts/release-preflight.sh --allow-unreleased --json

Checks release metadata only. It does not run tests, create tags, or push anything.
The metadata gate includes version/changelog consistency, migration compatibility,
maturity-claim policy documentation, and contribution governance. Lockstep version
checks apply to releasable crates under crates/*; non-release workspace members
such as fuzz tooling are intentionally excluded.

When --allow-unreleased is used without VERSION, the script checks the current
workspace.package.version against the [Unreleased] changelog section.
USAGE
}

add_failure() {
    failures+=("$1")
}

add_warning() {
    warnings+=("$1")
}

json_escape() {
    local value="$1"
    local code control escaped octal

    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    value="${value//$'\b'/\\b}"
    value="${value//$'\f'/\\f}"
    value="${value//$'\n'/\\n}"

    value="${value//$'\r'/\\r}"
    value="${value//$'\t'/\\t}"
    for code in {1..7} 11 {14..31}; do
        printf -v octal '%03o' "$code"
        printf -v control "\\$octal"
        printf -v escaped '\\u%04x' "$code"
        value="${value//$control/$escaped}"
    done

    printf '%s' "$value"
}

json_array() {
    local first=1
    local item
    printf '['
    for item in "$@"; do
        if [[ "$first" == "0" ]]; then
            printf ','
        fi
        first=0
        printf '"%s"' "$(json_escape "$item")"
    done
    printf ']'
}

emit_result() {
    local status="$1"
    if [[ "$OUTPUT_FORMAT" == "json" ]]; then
        printf '{"status":"%s","version":"%s","allow_unreleased":%s,"warnings":' \
            "$status" "$(json_escape "${VERSION:-}")" "$([[ "$ALLOW_UNRELEASED" == "1" ]] && printf true || printf false)"
        if [[ "${#warnings[@]}" -gt 0 ]]; then
            json_array "${warnings[@]}"
        else
            printf '[]'
        fi
        printf ',"failures":'
        if [[ "${#failures[@]}" -gt 0 ]]; then
            json_array "${failures[@]}"
        else
            printf '[]'
        fi
        printf '}\n'
        return
    fi

    if [[ "${#warnings[@]}" -gt 0 ]]; then
        local warning
        for warning in "${warnings[@]}"; do
            echo "warning: $warning" >&2
        done
    fi

    if [[ "$status" == "failed" ]]; then
        echo "error: release preflight failed" >&2
        if [[ "${#failures[@]}" -gt 0 ]]; then
            local failure
            for failure in "${failures[@]}"; do
                echo "error: $failure" >&2
            done
        fi
    else
        echo "release preflight passed for v$VERSION"
    fi
}

section_contains() {
    local file="$1"
    local section_heading="$2"
    local literal="$3"

    awk -v section="$section_heading" -v literal="$literal" '
        $0 == section { in_section = 1; next }
        in_section && index($0, "## [") == 1 { exit }
        in_section && index($0, literal) > 0 { found = 1; exit }
        END { exit found ? 0 : 1 }
    ' "$file"
}

section_exact_line_count() {
    local file="$1"
    local section_heading="$2"
    local literal="$3"

    awk -v section="$section_heading" -v literal="$literal" '
        $0 == section { in_section = 1; next }
        in_section && index($0, "## [") == 1 { exit }
        in_section && $0 == literal { count += 1 }
        END { print count + 0 }
    ' "$file"
}

section_prefix_line_count() {
    local file="$1"
    local section_heading="$2"
    local prefix="$3"

    awk -v section="$section_heading" -v prefix="$prefix" '
        $0 == section { in_section = 1; next }
        in_section && index($0, "## [") == 1 { exit }
        in_section && index($0, prefix) == 1 { count += 1 }
        END { print count + 0 }
    ' "$file"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --allow-unreleased)
            ALLOW_UNRELEASED=1
            shift
            ;;
        --json)
            OUTPUT_FORMAT="json"
            shift
            ;;
        --format)
            if [[ "${2:-}" != "json" && "${2:-}" != "text" ]]; then
                echo "error: --format must be 'json' or 'text'" >&2
                exit 1
            fi
            OUTPUT_FORMAT="$2"
            shift 2
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

if [[ ! -f Cargo.toml || ! -f CHANGELOG.md || ! -d crates ]]; then
    add_failure "run release preflight from the workspace root"
    emit_result failed
    exit 1
fi

for required_file in \
    docs/release-policy.md \
    docs/maturity-model.md \
    docs/migration-guide.md \
    CONTRIBUTING.md \
    .github/PULL_REQUEST_TEMPLATE.md \
    .github/workflows/pr-validation.yml \
    docs/getting-started.md \
    docs/troubleshooting.md \
    docs/language-reference.md \
    docs/compatibility.md \
    docs/stdlib-reference.md \
    docs/package-reference.md \
    docs/performance.md \
    docs/security.md \
    docs/tooling-reference.md \
    scripts/docs-onboarding-smoke.sh \
    scripts/docs-troubleshooting-smoke.sh \
    scripts/docs-language-reference-smoke.sh \
    scripts/docs-compatibility-smoke.sh \
    scripts/docs-stdlib-reference-smoke.sh \
    scripts/docs-package-reference-smoke.sh \
    scripts/docs-performance-smoke.sh \
    scripts/docs-security-smoke.sh \
    scripts/docs-tooling-reference-smoke.sh \
    scripts/pr-validation.py \
    scripts/pr-validation-smoke.sh \
    scripts/tag-release.sh \
    scripts/tag-release-gate-smoke.sh \
    crates/ail-storage/src/migration.rs; do
    if [[ ! -f "$required_file" ]]; then
        add_failure "missing required release metadata file: $required_file"
    fi
done

workspace_version=$(awk '
    /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^[[:space:]]*version[[:space:]]*=/ {
        gsub(/"/, "", $3)
        print $3
        exit
    }
' Cargo.toml)

version_valid=0
if [[ -z "$workspace_version" ]]; then
    add_failure "Cargo.toml must declare workspace.package.version"
elif [[ -z "${VERSION:-}" && "$ALLOW_UNRELEASED" == "1" ]]; then
    VERSION="$workspace_version"
    version_defaulted_for_unreleased=1
    add_warning "VERSION was not set; using workspace.package.version ($VERSION) for unreleased preflight"
elif [[ -z "${VERSION:-}" ]]; then
    add_failure "VERSION is not set. Usage: VERSION=x.y.z ./scripts/release-preflight.sh"
fi

if [[ -n "${VERSION:-}" ]]; then
    if [[ "$VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
        version_valid=1
    else
        add_failure "VERSION must be MAJOR.MINOR.PATCH (e.g. 1.2.3). Got: $VERSION"
    fi
fi

if [[ "$version_valid" == "1" && -n "$workspace_version" && "$workspace_version" != "$VERSION" ]]; then
    add_failure "VERSION ($VERSION) does not match workspace.package.version ($workspace_version)"
fi

missing_workspace_version=0
for crate_manifest in crates/*/Cargo.toml; do
    if ! grep -qE '^[[:space:]]*version\.workspace[[:space:]]*=[[:space:]]*true[[:space:]]*$' "$crate_manifest"; then
        add_failure "releasable crate $crate_manifest must use version.workspace = true for lockstep releases"
        missing_workspace_version=1
    fi
done

changelog_section_heading=""
if [[ "$version_valid" == "1" ]]; then
    release_heading_prefix="## [$VERSION] - "
    release_heading=$(awk -v prefix="$release_heading_prefix" '
        index($0, prefix) == 1 { print; exit }
    ' CHANGELOG.md)
    release_date="${release_heading:${#release_heading_prefix}}"
    if [[ "$ALLOW_UNRELEASED" == "1" && "$version_defaulted_for_unreleased" == "1" && $(grep -c -x -F '## [Unreleased]' CHANGELOG.md) -gt 0 ]]; then
        changelog_section_heading="## [Unreleased]"
        add_warning "checking CHANGELOG.md [Unreleased] section for unreleased preflight"
    elif [[ -n "$release_heading" && "$release_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
        changelog_section_heading="$release_heading"
    elif [[ "$ALLOW_UNRELEASED" == "1" ]] && grep -qxF '## [Unreleased]' CHANGELOG.md; then
        changelog_section_heading="## [Unreleased]"
        add_warning "CHANGELOG.md has [Unreleased] but no [$VERSION] release heading"
    else
        add_failure "CHANGELOG.md must contain '## [$VERSION] - YYYY-MM-DD' before tagging"
    fi
fi

if [[ -n "$changelog_section_heading" ]]; then
    maturity_stage_count=0
    for maturity_stage in \
        "Maturity: Validation milestone" \
        "Maturity: Usable preview" \
        "Maturity: Real language experience" \
        "Maturity: Production-ready"; do
        maturity_stage_count=$((maturity_stage_count + $(section_exact_line_count CHANGELOG.md "$changelog_section_heading" "$maturity_stage")))
    done

    maturity_line_count=$(section_prefix_line_count CHANGELOG.md "$changelog_section_heading" "Maturity:")
    if [[ "$maturity_stage_count" != "1" || "$maturity_line_count" != "1" ]]; then
        add_failure "active CHANGELOG.md release notes must declare one maturity stage: Maturity: Validation milestone|Usable preview|Real language experience|Production-ready"
    fi
fi

if [[ -f docs/release-policy.md ]]; then
    if ! grep -qF 'scripts/release-preflight.sh' docs/release-policy.md; then
        add_failure "docs/release-policy.md must document scripts/release-preflight.sh"
    fi
    if ! grep -qF '[compatibility-breaking]' docs/release-policy.md; then
        add_failure "docs/release-policy.md must document the compatibility-breaking release marker"
    fi
    if ! grep -qF 'Maturity model' docs/release-policy.md; then
        add_failure "docs/release-policy.md must document maturity claim gates from docs/maturity-model.md"
    fi
    if ! grep -qF 'scripts/pr-validation.py' docs/release-policy.md; then
        add_failure "docs/release-policy.md must document the PR validation governance gate"
    fi
fi

if [[ -f CONTRIBUTING.md ]]; then
    if ! grep -qF 'status:approved' CONTRIBUTING.md || ! grep -qF 'type:*' CONTRIBUTING.md; then
        add_failure "CONTRIBUTING.md must document approved issue and type label PR requirements"
    fi
    if ! grep -qF 'conventional commit' CONTRIBUTING.md || ! grep -qF 'Co-Authored-By' CONTRIBUTING.md; then
        add_failure "CONTRIBUTING.md must document commit discipline and attribution policy"
    fi
fi

if [[ -f .github/PULL_REQUEST_TEMPLATE.md ]]; then
    if ! grep -qF 'status:approved' .github/PULL_REQUEST_TEMPLATE.md || ! grep -qF 'type:*' .github/PULL_REQUEST_TEMPLATE.md; then
        add_failure ".github/PULL_REQUEST_TEMPLATE.md must prompt for approved issue and type label requirements"
    fi
    if ! grep -qF 'Maturity model' .github/PULL_REQUEST_TEMPLATE.md && ! grep -qF 'maturity model' .github/PULL_REQUEST_TEMPLATE.md; then
        add_failure ".github/PULL_REQUEST_TEMPLATE.md must prompt reviewers to classify maturity-gate evidence"
    fi
fi

if [[ -f .github/workflows/pr-validation.yml ]]; then
    if ! grep -qF 'pull_request_target' .github/workflows/pr-validation.yml || ! grep -qF 'scripts/pr-validation.py' .github/workflows/pr-validation.yml; then
        add_failure ".github/workflows/pr-validation.yml must run scripts/pr-validation.py on PR metadata"
    fi
fi

if [[ -f scripts/pr-validation.py ]]; then
    if ! grep -qF 'status:approved' scripts/pr-validation.py || ! grep -qF 'ALLOWED_TYPE_LABELS' scripts/pr-validation.py; then
        add_failure "scripts/pr-validation.py must enforce approved issue and exactly one type label"
    fi
    if ! grep -qF 'ALLOWED_MATURITY_GATES' scripts/pr-validation.py || ! grep -qF 'validate_maturity_gate' scripts/pr-validation.py; then
        add_failure "scripts/pr-validation.py must enforce maturity gate and evidence fields"
    fi
    if ! grep -qF 'ALLOWED_COMPATIBILITY_SURFACES' scripts/pr-validation.py || ! grep -qF 'validate_compatibility' scripts/pr-validation.py; then
        add_failure "scripts/pr-validation.py must enforce compatibility surface/classification/evidence fields"
    fi
    if ! grep -qF 'validate_verification_section' scripts/pr-validation.py; then
        add_failure "scripts/pr-validation.py must enforce PR verification evidence"
    fi
    if ! grep -qF 'CONVENTIONAL_COMMIT_RE' scripts/pr-validation.py || ! grep -qF 'AI_ATTRIBUTION_PATTERNS' scripts/pr-validation.py; then
        add_failure "scripts/pr-validation.py must enforce conventional commits and attribution policy"
    fi
fi

if [[ -f scripts/pr-validation-smoke.sh ]]; then
    if ! grep -qF 'bad_subject_commits' scripts/pr-validation-smoke.sh || ! grep -qF 'ai_attribution_commits' scripts/pr-validation-smoke.sh; then
        add_failure "scripts/pr-validation-smoke.sh must cover commit discipline failure cases"
    fi
    if ! grep -qF 'assert_template_labels_match_validator' scripts/pr-validation-smoke.sh; then
        add_failure "scripts/pr-validation-smoke.sh must guard PR template type-label drift"
    fi
    if ! grep -qF 'assert_maturity_gates_match_model' scripts/pr-validation-smoke.sh; then
        add_failure "scripts/pr-validation-smoke.sh must guard maturity gate drift"
    fi
    if ! grep -qF 'missing_verification' scripts/pr-validation-smoke.sh || ! grep -qF 'empty_verification' scripts/pr-validation-smoke.sh; then
        add_failure "scripts/pr-validation-smoke.sh must cover missing PR verification evidence"
    fi
    if ! grep -qF 'missing_compatibility_surface' scripts/pr-validation-smoke.sh || ! grep -qF 'breaking_without_marker' scripts/pr-validation-smoke.sh; then
        add_failure "scripts/pr-validation-smoke.sh must cover compatibility classification failure cases"
    fi
fi

if [[ -f scripts/docs-onboarding-smoke.sh ]]; then
    if ! grep -qF 'docs/getting-started.md' scripts/docs-onboarding-smoke.sh || ! grep -qF 'cli_subcommands.rs' scripts/docs-onboarding-smoke.sh; then
        add_failure "scripts/docs-onboarding-smoke.sh must guard getting-started docs against CLI test drift"
    fi
fi

if [[ -f scripts/docs-troubleshooting-smoke.sh ]]; then
    if ! grep -qF 'docs/troubleshooting.md' scripts/docs-troubleshooting-smoke.sh || ! grep -qF 'workflow_commands.rs' scripts/docs-troubleshooting-smoke.sh; then
        add_failure "scripts/docs-troubleshooting-smoke.sh must guard troubleshooting docs against CLI diagnostic drift"
    fi
    if ! grep -qF 'capability denied: log.write' scripts/docs-troubleshooting-smoke.sh || ! grep -qF 'native linked execution not supported yet' scripts/docs-troubleshooting-smoke.sh; then
        add_failure "scripts/docs-troubleshooting-smoke.sh must cover capability and native execution diagnostics"
    fi
fi

if [[ -f scripts/docs-language-reference-smoke.sh ]]; then
    if ! grep -qF 'docs/language-reference.md' scripts/docs-language-reference-smoke.sh || ! grep -qF 'parser_tests.rs' scripts/docs-language-reference-smoke.sh; then
        add_failure "scripts/docs-language-reference-smoke.sh must guard language reference docs against parser drift"
    fi
    if ! grep -qF 'expr_parser_tests.rs' scripts/docs-language-reference-smoke.sh || ! grep -qF 'op_schema.rs' scripts/docs-language-reference-smoke.sh; then
        add_failure "scripts/docs-language-reference-smoke.sh must cover expression and op-schema evidence"
    fi
fi

if [[ -f scripts/docs-compatibility-smoke.sh ]]; then
    if ! grep -qF 'docs/compatibility.md' scripts/docs-compatibility-smoke.sh || ! grep -qF 'acl_migrator.rs' scripts/docs-compatibility-smoke.sh; then
        add_failure "scripts/docs-compatibility-smoke.sh must guard compatibility docs against policy drift"
    fi
    if ! grep -qF '[compatibility-breaking]' scripts/docs-compatibility-smoke.sh || ! grep -qF 'latest-storage-schema=3; compatibility-breaking=false' scripts/docs-compatibility-smoke.sh; then
        add_failure "scripts/docs-compatibility-smoke.sh must cover breaking-marker and migration metadata evidence"
    fi
fi

if [[ -f scripts/docs-stdlib-reference-smoke.sh ]]; then
    if ! grep -qF 'docs/stdlib-reference.md' scripts/docs-stdlib-reference-smoke.sh || ! grep -qF 'v1/function_entries.rs' scripts/docs-stdlib-reference-smoke.sh; then
        add_failure "scripts/docs-stdlib-reference-smoke.sh must guard stdlib reference docs against registry drift"
    fi
    if ! grep -qF 'exec/registry.rs' scripts/docs-stdlib-reference-smoke.sh || ! grep -qF 'capability.rs' scripts/docs-stdlib-reference-smoke.sh; then
        add_failure "scripts/docs-stdlib-reference-smoke.sh must cover stdlib exec descriptors and capability evidence"
    fi
fi

if [[ -f scripts/docs-package-reference-smoke.sh ]]; then
    if ! grep -qF 'docs/package-reference.md' scripts/docs-package-reference-smoke.sh || ! grep -qF 'manifest.rs' scripts/docs-package-reference-smoke.sh; then
        add_failure "scripts/docs-package-reference-smoke.sh must guard package reference docs against package manifest drift"
    fi
    if ! grep -qF 'remote_registry/types.rs' scripts/docs-package-reference-smoke.sh || ! grep -qF 'versioning.rs' scripts/docs-package-reference-smoke.sh; then
        add_failure "scripts/docs-package-reference-smoke.sh must cover package registry and compatibility evidence"
    fi
fi

if [[ -f scripts/docs-performance-smoke.sh ]]; then
    if ! grep -qF 'docs/performance.md' scripts/docs-performance-smoke.sh || ! grep -qF 'incremental_tests.rs' scripts/docs-performance-smoke.sh; then
        add_failure "scripts/docs-performance-smoke.sh must guard performance docs against compiler evidence drift"
    fi
    if ! grep -qF 'storage_perf.rs' scripts/docs-performance-smoke.sh || ! grep -qF 'perf-preflight.sh' scripts/docs-performance-smoke.sh; then
        add_failure "scripts/docs-performance-smoke.sh must cover storage benchmark and perf preflight evidence"
    fi
fi

if [[ -f scripts/docs-security-smoke.sh ]]; then
    if ! grep -qF 'docs/security.md' scripts/docs-security-smoke.sh || ! grep -qF 'preflight_tests.rs' scripts/docs-security-smoke.sh; then
        add_failure "scripts/docs-security-smoke.sh must guard security docs against runtime capability evidence drift"
    fi
    if ! grep -qF 'secret_provider_audit_tests.rs' scripts/docs-security-smoke.sh || ! grep -qF 'signing.rs' scripts/docs-security-smoke.sh || ! grep -qF 'r2_attributes.rs' scripts/docs-security-smoke.sh; then
        add_failure "scripts/docs-security-smoke.sh must cover secret audit, package signing, and context redaction evidence"
    fi
fi

if [[ -f scripts/docs-tooling-reference-smoke.sh ]]; then
    if ! grep -qF 'docs/tooling-reference.md' scripts/docs-tooling-reference-smoke.sh || ! grep -qF 'cli_g31r2.rs' scripts/docs-tooling-reference-smoke.sh; then
        add_failure "scripts/docs-tooling-reference-smoke.sh must guard tooling reference docs against CLI integration-test drift"
    fi
    if ! grep -qF 'output.rs' scripts/docs-tooling-reference-smoke.sh || ! grep -qF 'JSON_OUTPUT_VERSION' scripts/docs-tooling-reference-smoke.sh || ! grep -qF 'package_cli_compat.rs' scripts/docs-tooling-reference-smoke.sh; then
        add_failure "scripts/docs-tooling-reference-smoke.sh must cover JSON output and package tooling evidence"
    fi
fi

if [[ -f scripts/tag-release.sh ]]; then
    if ! grep -qF './scripts/docs-onboarding-smoke.sh' scripts/tag-release.sh || ! grep -qF './scripts/docs-troubleshooting-smoke.sh' scripts/tag-release.sh || ! grep -qF './scripts/docs-language-reference-smoke.sh' scripts/tag-release.sh || ! grep -qF './scripts/docs-compatibility-smoke.sh' scripts/tag-release.sh || ! grep -qF './scripts/docs-stdlib-reference-smoke.sh' scripts/tag-release.sh || ! grep -qF './scripts/docs-package-reference-smoke.sh' scripts/tag-release.sh || ! grep -qF './scripts/docs-performance-smoke.sh' scripts/tag-release.sh || ! grep -qF './scripts/docs-security-smoke.sh' scripts/tag-release.sh || ! grep -qF './scripts/docs-tooling-reference-smoke.sh' scripts/tag-release.sh; then
        add_failure "scripts/tag-release.sh must run docs smoke before release tagging"
    fi
    if ! grep -qF './scripts/pr-validation-smoke.sh' scripts/tag-release.sh; then
        add_failure "scripts/tag-release.sh must run PR governance smoke before release tagging"
    fi
fi

if [[ -f scripts/tag-release-gate-smoke.sh ]]; then
    if ! grep -qF 'docs-tooling-reference-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'docs-security-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'docs-performance-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'docs-package-reference-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'docs-stdlib-reference-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'docs-compatibility-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'docs-language-reference-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'docs-troubleshooting-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'pr-validation-smoke.sh' scripts/tag-release-gate-smoke.sh || ! grep -qF 'expected_order' scripts/tag-release-gate-smoke.sh; then
        add_failure "scripts/tag-release-gate-smoke.sh must verify tag-release gate ordering"
    fi
fi

latest_schema=""
compatibility_breaking=""
if [[ -f docs/migration-guide.md ]]; then
    migration_metadata=$(grep -E '^<!-- Release metadata:' docs/migration-guide.md || true)
    if [[ -z "$migration_metadata" ]]; then
        add_failure "docs/migration-guide.md must declare '<!-- Release metadata: latest-storage-schema=N; compatibility-breaking=true|false -->'"
    else
        if [[ "$migration_metadata" =~ latest-storage-schema=([0-9]+) ]]; then
            latest_schema="${BASH_REMATCH[1]}"
        else
            add_failure "docs/migration-guide.md release metadata must include latest-storage-schema=N"
        fi

        if [[ "$migration_metadata" =~ compatibility-breaking=(true|false) ]]; then
            compatibility_breaking="${BASH_REMATCH[1]}"
        else
            add_failure "docs/migration-guide.md release metadata must include compatibility-breaking=true|false"
        fi
    fi

    if [[ -n "$latest_schema" ]] && ! grep -qE "^[|][[:space:]]*$latest_schema[[:space:]]*[|]" docs/migration-guide.md; then
        add_failure "docs/migration-guide.md must include schema version $latest_schema in its version overview table"
    fi
fi

implemented_latest_schema=""
if [[ -f crates/ail-storage/src/migration.rs ]]; then
    implemented_latest_schema=$(awk '
        /fn target_version\(&self\) -> u32/ {
            in_target = 1
            line = $0
            if (line ~ /\{/) {
                sub(/^[^{]*\{/, "", line)
                if (line ~ /^[[:space:]]*$/) next
            } else {
                line = ""
                next
            }
        }
        in_target {
            if (line == "") line = $0
            sub(/\/\/.*/, "", line)
            if (match(line, /[0-9][0-9_]*(u32)?/)) {
                version = substr(line, RSTART, RLENGTH)
                gsub(/u32/, "", version)
                gsub(/_/, "", version)
                version += 0
                if (version > max) max = version
                in_target = 0
                line = ""
                next
            }
            if (line ~ /}/) in_target = 0
            line = ""
        }
        END { if (max != "") print max }
    ' crates/ail-storage/src/migration.rs)

    if [[ -z "$implemented_latest_schema" ]]; then
        add_failure "could not determine latest storage schema from crates/ail-storage/src/migration.rs"
    elif [[ -n "$latest_schema" && "$implemented_latest_schema" != "$latest_schema" ]]; then
        add_failure "docs/migration-guide.md latest-storage-schema ($latest_schema) does not match implemented migration target ($implemented_latest_schema)"
    fi

    if awk '
        {
            line = $0
            sub(/\/\/.*/, "", line)
        }
        pending_structural_equivalence {
            if (line ~ /false([[:space:],}]|$)/) found = 1
            pending_structural_equivalence = 0
        }
        line ~ /structural_equivalence[[:space:]]*:/ {
            sub(/^.*structural_equivalence[[:space:]]*:[[:space:]]*/, "", line)
            if (line ~ /^false([[:space:],}]|$)/) {
                found = 1
            } else if (line == "") {
                pending_structural_equivalence = 1
            }
        }
        END { exit found ? 0 : 1 }
    ' crates/ail-storage/src/migration.rs; then
        if [[ "$compatibility_breaking" != "true" ]]; then
            add_failure "storage migrations with structural_equivalence: false require compatibility-breaking=true in docs/migration-guide.md"
        fi
    fi
fi

release_notes_mark_breaking=0
if [[ -n "$changelog_section_heading" ]]; then
    if section_contains CHANGELOG.md "$changelog_section_heading" '[compatibility-breaking]'; then
        release_notes_mark_breaking=1
    fi
fi

if [[ "$compatibility_breaking" == "true" && "$release_notes_mark_breaking" != "1" ]]; then
    add_failure "compatibility-breaking migration metadata requires the active CHANGELOG.md release notes to include [compatibility-breaking]"
elif [[ "$compatibility_breaking" == "false" && "$release_notes_mark_breaking" == "1" ]]; then
    add_failure "CHANGELOG.md marks compatibility-breaking changes, but docs/migration-guide.md metadata says compatibility-breaking=false"
fi

if [[ "${#failures[@]}" -gt 0 ]]; then
    emit_result failed
    exit 1
else
    emit_result passed
fi
