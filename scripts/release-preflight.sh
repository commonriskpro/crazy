#!/usr/bin/env bash
# release-preflight.sh - Validate release metadata before tagging.

set -euo pipefail

ALLOW_UNRELEASED="${ALLOW_UNRELEASED:-0}"
OUTPUT_FORMAT="text"
failures=()
warnings=()

usage() {
    cat >&2 <<'USAGE'
Usage:
  VERSION=0.2.0 ./scripts/release-preflight.sh
  VERSION=0.2.0 ./scripts/release-preflight.sh --allow-unreleased
  ./scripts/release-preflight.sh --allow-unreleased --json

Checks release metadata only. It does not run tests, create tags, or push anything.
Lockstep version checks apply to releasable crates under crates/*; non-release
workspace members such as fuzz tooling are intentionally excluded.

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

for required_file in docs/release-policy.md docs/migration-guide.md crates/ail-storage/src/migration.rs; do
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
    if [[ -n "$release_heading" && "$release_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
        changelog_section_heading="$release_heading"
    elif [[ "$ALLOW_UNRELEASED" == "1" ]] && grep -qxF '## [Unreleased]' CHANGELOG.md; then
        changelog_section_heading="## [Unreleased]"
        add_warning "CHANGELOG.md has [Unreleased] but no [$VERSION] release heading"
    else
        add_failure "CHANGELOG.md must contain '## [$VERSION] - YYYY-MM-DD' before tagging"
    fi
fi

if [[ -f docs/release-policy.md ]]; then
    if ! grep -qF 'scripts/release-preflight.sh' docs/release-policy.md; then
        add_failure "docs/release-policy.md must document scripts/release-preflight.sh"
    fi
    if ! grep -qF '[compatibility-breaking]' docs/release-policy.md; then
        add_failure "docs/release-policy.md must document the compatibility-breaking release marker"
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
