#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/ail-pr-validation.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

write_event() {
  local path="$1"
  local body="$2"
  shift 2

  python3 - "$path" "$body" "$@" <<'PY'
import json
import sys

path = sys.argv[1]
body = sys.argv[2]
labels = [{"name": label} for label in sys.argv[3:]]

event = {
    "repository": {"full_name": "ail-lang/ail"},
    "pull_request": {
        "body": body,
        "labels": labels,
    },
}

with open(path, "w", encoding="utf-8") as handle:
    json.dump(event, handle)
PY
}

run_expect_success() {
  local event_path="$1"
  local issue_labels="$2"
  local commits="$3"

  GITHUB_EVENT_PATH="$event_path" \
    PR_VALIDATION_ISSUE_LABELS="$issue_labels" \
    PR_VALIDATION_COMMITS="$commits" \
    python3 "$ROOT_DIR/scripts/pr-validation.py" >/dev/null
}

run_expect_failure() {
  local event_path="$1"
  local issue_labels="$2"
  local commits="$3"

  if GITHUB_EVENT_PATH="$event_path" \
    PR_VALIDATION_ISSUE_LABELS="$issue_labels" \
    PR_VALIDATION_COMMITS="$commits" \
    python3 "$ROOT_DIR/scripts/pr-validation.py" >/dev/null 2>&1; then
    echo "expected PR validation to fail for $event_path" >&2
    return 1
  fi
}

assert_template_labels_match_validator() {
  python3 - "$ROOT_DIR/scripts/pr-validation.py" "$ROOT_DIR/.github/PULL_REQUEST_TEMPLATE.md" <<'PY'
import ast
import re
import sys
from pathlib import Path

script_path = Path(sys.argv[1])
template_path = Path(sys.argv[2])

script = ast.parse(script_path.read_text(encoding="utf-8"))
validator_labels = None
for node in script.body:
    if isinstance(node, ast.Assign):
        for target in node.targets:
            if isinstance(target, ast.Name) and target.id == "ALLOWED_TYPE_LABELS":
                validator_labels = set(ast.literal_eval(node.value))

if validator_labels is None:
    raise SystemExit("ALLOWED_TYPE_LABELS not found in scripts/pr-validation.py")

template_text = template_path.read_text(encoding="utf-8")
template_labels = set(re.findall(r"`(type:[a-z0-9-]+)`", template_text))

if template_labels != validator_labels:
    missing_in_template = sorted(validator_labels - template_labels)
    missing_in_validator = sorted(template_labels - validator_labels)
    raise SystemExit(
        "PR template type labels must match scripts/pr-validation.py "
        f"(missing in template: {missing_in_template}; missing in validator: {missing_in_validator})"
    )
PY
}

assert_maturity_gates_match_model() {
  python3 - "$ROOT_DIR/scripts/pr-validation.py" "$ROOT_DIR/docs/maturity-model.md" <<'PY'
import ast
import re
import sys
from pathlib import Path

script_path = Path(sys.argv[1])
model_path = Path(sys.argv[2])

script = ast.parse(script_path.read_text(encoding="utf-8"))
validator_gates = None
for node in script.body:
    if isinstance(node, ast.Assign):
        for target in node.targets:
            if isinstance(target, ast.Name) and target.id == "ALLOWED_MATURITY_GATES":
                validator_gates = set(ast.literal_eval(node.value))

if validator_gates is None:
    raise SystemExit("ALLOWED_MATURITY_GATES not found in scripts/pr-validation.py")

model_text = model_path.read_text(encoding="utf-8")
match = re.search(r"## Maturity gates\n\n(?P<table>.*?)(?:\n## |\Z)", model_text, re.S)
if not match:
    raise SystemExit("Maturity gates table not found in docs/maturity-model.md")

model_gates = set()
for line in match.group("table").splitlines():
    if not line.startswith("|") or line.startswith("|------"):
        continue
    cells = [cell.strip() for cell in line.strip("|").split("|")]
    if cells and cells[0] != "Gate":
        model_gates.add(cells[0])

if model_gates != validator_gates:
    missing_in_model = sorted(validator_gates - model_gates)
    missing_in_validator = sorted(model_gates - validator_gates)
    raise SystemExit(
        "Maturity gates in docs/maturity-model.md must match scripts/pr-validation.py "
        f"(missing in model: {missing_in_model}; missing in validator: {missing_in_validator})"
    )
PY
}

assert_compatibility_surfaces_match_policy() {
  python3 - "$ROOT_DIR/scripts/pr-validation.py" "$ROOT_DIR/docs/compatibility.md" <<'PY'
import ast
import re
import sys
from pathlib import Path

script_path = Path(sys.argv[1])
policy_path = Path(sys.argv[2])

script = ast.parse(script_path.read_text(encoding="utf-8"))
validator_surfaces = None
for node in script.body:
    if isinstance(node, ast.Assign):
        for target in node.targets:
            if isinstance(target, ast.Name) and target.id == "ALLOWED_COMPATIBILITY_SURFACES":
                validator_surfaces = set(ast.literal_eval(node.value))

if validator_surfaces is None:
    raise SystemExit("ALLOWED_COMPATIBILITY_SURFACES not found in scripts/pr-validation.py")

policy_text = policy_path.read_text(encoding="utf-8")
match = re.search(r"## Current compatibility status\n\n(?P<table>.*?)(?:\n## |\Z)", policy_text, re.S)
if not match:
    raise SystemExit("Current compatibility status table not found in docs/compatibility.md")

policy_surfaces = set()
for line in match.group("table").splitlines():
    if not line.startswith("|") or line.startswith("|---"):
        continue
    cells = [cell.strip() for cell in line.strip("|").split("|")]
    if cells and cells[0] != "Area":
        policy_surfaces.add(cells[0])

if policy_surfaces != validator_surfaces:
    missing_in_policy = sorted(validator_surfaces - policy_surfaces)
    missing_in_validator = sorted(policy_surfaces - validator_surfaces)
    raise SystemExit(
        "Compatibility surfaces in docs/compatibility.md must match scripts/pr-validation.py "
        f"(missing in policy: {missing_in_policy}; missing in validator: {missing_in_validator})"
    )
PY
}


happy_path="$TMP_DIR/happy.json"
missing_type="$TMP_DIR/missing-type.json"
missing_approval="$TMP_DIR/missing-approval.json"
multiple_types="$TMP_DIR/multiple-types.json"
bad_commit_subject="$TMP_DIR/bad-commit-subject.json"
ai_attribution="$TMP_DIR/ai-attribution.json"
missing_maturity_gate="$TMP_DIR/missing-maturity-gate.json"
invalid_maturity_gate="$TMP_DIR/invalid-maturity-gate.json"
missing_maturity_evidence="$TMP_DIR/missing-maturity-evidence.json"
missing_verification="$TMP_DIR/missing-verification.json"
empty_verification="$TMP_DIR/empty-verification.json"
not_run_verification="$TMP_DIR/not-run-verification.json"
missing_compatibility_surface="$TMP_DIR/missing-compatibility-surface.json"
invalid_compatibility_classification="$TMP_DIR/invalid-compatibility-classification.json"
breaking_without_label="$TMP_DIR/breaking-without-label.json"
breaking_without_marker="$TMP_DIR/breaking-without-marker.json"
breaking_success="$TMP_DIR/breaking-success.json"

good_commits='["feat: add package resolver smoke"]'
bad_subject_commits='["add package resolver smoke"]'
ai_attribution_commits='["fix: tighten verifier\n\nGenerated with Codex"]'
compat_fields=$'Compatibility surface: Runtime capability names
Compatibility classification: Compatible
Compatibility evidence: no capability IDs changed'
docs_compat_fields=$'Compatibility surface: Documentation/process only
Compatibility classification: Not applicable
Compatibility evidence: docs/process-only change'
breaking_compat_fields=$'Compatibility surface: ACL / ChangeSet syntax
Compatibility classification: Breaking
Compatibility evidence: [compatibility-breaking] removes accepted ACL syntax'
breaking_no_marker_fields=$'Compatibility surface: ACL / ChangeSet syntax
Compatibility classification: Breaking
Compatibility evidence: removes accepted ACL syntax'
valid_body=$'Closes #42

Gate: Verification
Evidence added: negative verifier fixture
'"$compat_fields"$'

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'
missing_gate_body=$'Closes #42

Evidence added: negative verifier fixture
'"$compat_fields"$'

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'
invalid_gate_body=$'Closes #42

Gate: Nice idea
Evidence added: negative verifier fixture
'"$compat_fields"$'

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'
missing_evidence_body=$'Closes #42

Gate: Verification
Evidence added:
'"$compat_fields"$'

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'
missing_verification_body=$'Closes #42

Gate: Verification
Evidence added: negative verifier fixture
'"$compat_fields"
empty_verification_body=$'Closes #42

Gate: Verification
Evidence added: negative verifier fixture
'"$compat_fields"$'

## Verification

```sh

```'
not_run_verification_body=$'Closes #42

Gate: Documentation
Evidence added: docs-only wording update
'"$docs_compat_fields"$'

## Verification

Not run: docs-only copy edit'
missing_compatibility_surface_body=$'Closes #42

Gate: Verification
Evidence added: negative verifier fixture
Compatibility classification: Compatible
Compatibility evidence: no compatibility surface changed

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'
invalid_compatibility_classification_body=$'Closes #42

Gate: Compatibility
Evidence added: compatibility policy fixture
Compatibility surface: Storage schema
Compatibility classification: Maybe safe
Compatibility evidence: migration smoke

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'
breaking_without_label_body=$'Closes #42

Gate: Compatibility
Evidence added: breaking ACL fixture
'"$breaking_compat_fields"$'

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'
breaking_without_marker_body=$'Closes #42

Gate: Compatibility
Evidence added: breaking ACL fixture
'"$breaking_no_marker_fields"$'

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'
breaking_success_body=$'Closes #42

Gate: Compatibility
Evidence added: breaking ACL fixture
'"$breaking_compat_fields"$'

## Verification

```sh
./scripts/pr-validation-smoke.sh
```'

write_event "$happy_path" "$valid_body" "type:feature"
write_event "$missing_type" "$valid_body"
write_event "$missing_approval" "$valid_body" "type:feature"
write_event "$multiple_types" "$valid_body" "type:feature" "type:docs"
write_event "$bad_commit_subject" "$valid_body" "type:feature"
write_event "$ai_attribution" "$valid_body" "type:feature"
write_event "$missing_maturity_gate" "$missing_gate_body" "type:feature"
write_event "$invalid_maturity_gate" "$invalid_gate_body" "type:feature"
write_event "$missing_maturity_evidence" "$missing_evidence_body" "type:feature"
write_event "$missing_verification" "$missing_verification_body" "type:feature"
write_event "$empty_verification" "$empty_verification_body" "type:feature"
write_event "$not_run_verification" "$not_run_verification_body" "type:docs"
write_event "$missing_compatibility_surface" "$missing_compatibility_surface_body" "type:feature"
write_event "$invalid_compatibility_classification" "$invalid_compatibility_classification_body" "type:feature"
write_event "$breaking_without_label" "$breaking_without_label_body" "type:feature"
write_event "$breaking_without_marker" "$breaking_without_marker_body" "type:breaking-change"
write_event "$breaking_success" "$breaking_success_body" "type:breaking-change"

run_expect_success "$happy_path" "status:approved" "$good_commits"
run_expect_failure "$missing_type" "status:approved" "$good_commits"
run_expect_failure "$missing_approval" "status:triage" "$good_commits"
run_expect_failure "$multiple_types" "status:approved" "$good_commits"
run_expect_failure "$bad_commit_subject" "status:approved" "$bad_subject_commits"
run_expect_failure "$ai_attribution" "status:approved" "$ai_attribution_commits"
run_expect_failure "$missing_maturity_gate" "status:approved" "$good_commits"
run_expect_failure "$invalid_maturity_gate" "status:approved" "$good_commits"
run_expect_failure "$missing_maturity_evidence" "status:approved" "$good_commits"
run_expect_failure "$missing_verification" "status:approved" "$good_commits"
run_expect_failure "$empty_verification" "status:approved" "$good_commits"
run_expect_success "$not_run_verification" "status:approved" "$good_commits"
run_expect_failure "$missing_compatibility_surface" "status:approved" "$good_commits"
run_expect_failure "$invalid_compatibility_classification" "status:approved" "$good_commits"
run_expect_failure "$breaking_without_label" "status:approved" "$good_commits"
run_expect_failure "$breaking_without_marker" "status:approved" "$good_commits"
run_expect_success "$breaking_success" "status:approved" "$good_commits"
assert_template_labels_match_validator
assert_maturity_gates_match_model
assert_compatibility_surfaces_match_policy

echo "PR validation smoke passed"
