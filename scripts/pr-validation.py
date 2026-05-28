#!/usr/bin/env python3
"""Validate AIL pull request process metadata.

Checks are intentionally about review discipline, not code execution:
- PR body must link an issue with Closes/Fixes/Resolves #N.
- Linked issue must have status:approved.
- PR must have exactly one allowed type:* label.
- PR body must name a valid maturity gate and evidence.
- PR body must classify compatibility surface, impact, and evidence.
- PR body must include verification commands or an explicit not-run explanation.
- PR commits must use conventional commit subjects.
- PR commits must not include AI attribution or Co-Authored-By trailers.
"""

from __future__ import annotations

import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

ALLOWED_TYPE_LABELS = {
    "type:bug",
    "type:feature",
    "type:docs",
    "type:refactor",
    "type:chore",
    "type:breaking-change",
}
ALLOWED_MATURITY_GATES = {
    "Language surface",
    "Verification",
    "Runtime safety",
    "Tooling UX",
    "Package ecosystem",
    "Standard library",
    "Compatibility",
    "Performance",
    "Documentation",
    "AI-native workflow",
}
ALLOWED_COMPATIBILITY_SURFACES = {
    "Rust crate APIs",
    "Storage schema",
    "ACL / ChangeSet syntax",
    "Semantic Graph schema",
    "CLI human output",
    "CLI JSON output",
    "Runtime capability names",
    "WASM runtime ABI",
    "Native object artifacts",
    "Stdlib APIs",
    "Package metadata/lockfiles",
    "Documentation/process only",
}
ALLOWED_COMPATIBILITY_CLASSIFICATIONS = {
    "Compatible",
    "Compatibility-risky",
    "Breaking",
    "Not applicable",
}

ISSUE_REFERENCE_RE = re.compile(
    r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s+#(?P<number>\d+)\b",
    re.IGNORECASE,
)
CONVENTIONAL_COMMIT_RE = re.compile(
    r"^(?:build|chore|ci|docs|feat|fix|perf|refactor|revert|style|test)"
    r"(?:\([a-z0-9._/-]+\))?!?: .+"
)
AI_ATTRIBUTION_PATTERNS = [
    re.compile(r"^co-authored-by:", re.IGNORECASE | re.MULTILINE),
    re.compile(
        r"^\s*(?:generated|created|written|authored)\s+(?:with|by)\s+"
        r"(?:ai|chatgpt|claude|codex|copilot)\b",
        re.IGNORECASE | re.MULTILINE,
    ),
    re.compile(
        r"^\s*(?:ai|chatgpt|claude|codex|copilot)[ -]?(?:assisted|generated|authored)[ -]?by:",
        re.IGNORECASE | re.MULTILINE,
    ),
]


def fail(message: str) -> None:
    print(f"::error::{message}")


def load_event() -> dict:
    event_path = os.environ.get("GITHUB_EVENT_PATH")
    if not event_path:
        raise RuntimeError("GITHUB_EVENT_PATH is not set")
    return json.loads(Path(event_path).read_text())


def pr_labels(pull_request: dict) -> set[str]:
    return {label.get("name", "") for label in pull_request.get("labels", [])}


def linked_issue_number(body: str) -> int | None:
    match = ISSUE_REFERENCE_RE.search(body or "")
    if not match:
        return None
    return int(match.group("number"))


def field_value(body: str, field_name: str) -> str:
    lines = body.splitlines()
    field_prefix = f"{field_name}:"

    for index, line in enumerate(lines):
        if line.strip().lower().startswith(field_prefix.lower()):
            value = line.split(":", 1)[1].strip()
            if value:
                return value

            continuation: list[str] = []
            for following in lines[index + 1 :]:
                stripped = following.strip()
                if not stripped:
                    if continuation:
                        break
                    continue
                if stripped.startswith("## ") or re.match(r"^[A-Za-z][A-Za-z /-]*:", stripped):
                    break
                continuation.append(stripped)
            return " ".join(continuation).strip()

    return ""


def section_body(body: str, heading: str) -> str:
    lines = body.splitlines()
    heading_text = f"## {heading}".lower()
    start = None

    for index, line in enumerate(lines):
        if line.strip().lower() == heading_text:
            start = index + 1
            break

    if start is None:
        return ""

    section_lines: list[str] = []
    for line in lines[start:]:
        if line.startswith("## "):
            break
        section_lines.append(line)
    return "\n".join(section_lines).strip()


def validate_maturity_gate(body: str) -> list[str]:
    gate = field_value(body, "Gate")
    evidence = field_value(body, "Evidence added")
    failures: list[str] = []

    if gate not in ALLOWED_MATURITY_GATES:
        failures.append(
            "PR body must set Gate to one maturity-model gate: "
            f"{', '.join(sorted(ALLOWED_MATURITY_GATES))}"
        )

    if not evidence or evidence in {"-", "n/a", "N/A"}:
        failures.append("PR body must include non-empty Evidence added for the maturity gate")

    return failures



def validate_compatibility(body: str, labels: set[str]) -> list[str]:
    surface = field_value(body, "Compatibility surface")
    classification = field_value(body, "Compatibility classification")
    evidence = field_value(body, "Compatibility evidence")
    failures: list[str] = []

    if surface not in ALLOWED_COMPATIBILITY_SURFACES:
        failures.append(
            "PR body must set Compatibility surface to one compatibility-policy surface: "
            f"{', '.join(sorted(ALLOWED_COMPATIBILITY_SURFACES))}"
        )

    if classification not in ALLOWED_COMPATIBILITY_CLASSIFICATIONS:
        failures.append(
            "PR body must set Compatibility classification to one of: "
            f"{', '.join(sorted(ALLOWED_COMPATIBILITY_CLASSIFICATIONS))}"
        )

    if not evidence or evidence in {"-", "n/a", "N/A"}:
        failures.append("PR body must include non-empty Compatibility evidence")

    if classification == "Not applicable" and surface != "Documentation/process only":
        failures.append("Compatibility classification 'Not applicable' is only valid for Documentation/process only")

    has_breaking_label = "type:breaking-change" in labels
    if classification == "Breaking" and not has_breaking_label:
        failures.append("Breaking compatibility classification requires the type:breaking-change label")
    if has_breaking_label and classification != "Breaking":
        failures.append("type:breaking-change PRs must use Compatibility classification: Breaking")
    if classification == "Breaking" and "[compatibility-breaking]" not in body:
        failures.append("Breaking compatibility classification must mention [compatibility-breaking] in the PR body")

    return failures


def validate_verification_section(body: str) -> list[str]:
    verification = section_body(body, "Verification")
    if not verification:
        return ["PR body must include a Verification section with commands or an explicit not-run explanation"]

    fenced_blocks = re.findall(r"```[^\n]*\n(?P<content>.*?)```", verification, flags=re.S)
    for block in fenced_blocks:
        meaningful_lines = [
            line.strip()
            for line in block.splitlines()
            if line.strip() and not line.strip().startswith("#")
        ]
        if meaningful_lines:
            return []

    if re.search(r"\b(?:not run|not applicable|n/a)\b\s*[:—-]\s*\S+", verification, re.IGNORECASE):
        return []

    return ["PR body Verification section must list commands/checks or explain why relevant checks were not run"]


def issue_labels_from_env() -> set[str] | None:
    raw = os.environ.get("PR_VALIDATION_ISSUE_LABELS")
    if raw is None:
        return None
    return {part.strip() for part in raw.split(",") if part.strip()}


def commit_messages_from_env() -> list[str] | None:
    raw = os.environ.get("PR_VALIDATION_COMMITS")
    if raw is None:
        return None
    payload = json.loads(raw)
    if not isinstance(payload, list) or not all(isinstance(item, str) for item in payload):
        raise RuntimeError("PR_VALIDATION_COMMITS must be a JSON array of commit message strings")
    return payload


def fetch_issue_labels(repo: str, issue_number: int, token: str) -> set[str]:
    url = f"https://api.github.com/repos/{repo}/issues/{issue_number}/labels?per_page=100"
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "ail-pr-validation",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f"failed to read labels for linked issue #{issue_number}: HTTP {exc.code}") from exc
    return {label.get("name", "") for label in payload}


def fetch_pr_commit_messages(commits_url: str, token: str) -> list[str]:
    messages: list[str] = []
    page = 1

    while True:
        separator = "&" if "?" in commits_url else "?"
        url = f"{commits_url}{separator}per_page=100&page={page}"
        request = urllib.request.Request(
            url,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {token}",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": "ail-pr-validation",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                payload = json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            raise RuntimeError(f"failed to read PR commits: HTTP {exc.code}") from exc

        if not isinstance(payload, list):
            raise RuntimeError("GitHub commits response was not a list")
        messages.extend((item.get("commit") or {}).get("message", "") for item in payload)
        if len(payload) < 100:
            return messages
        page += 1


def validate_commits(messages: list[str]) -> list[str]:
    failures: list[str] = []
    if not messages:
        failures.append("PR must contain at least one commit to validate")
        return failures

    for index, message in enumerate(messages, start=1):
        subject = (message.splitlines() or [""])[0]
        if not CONVENTIONAL_COMMIT_RE.match(subject):
            failures.append(f"Commit {index} subject must be conventional: {subject!r}")
        if any(pattern.search(message) for pattern in AI_ATTRIBUTION_PATTERNS):
            failures.append(f"Commit {index} must not contain AI attribution or Co-Authored-By trailers")

    return failures


def validate() -> list[str]:
    event = load_event()
    pull_request = event.get("pull_request") or {}
    repo = (event.get("repository") or {}).get("full_name") or os.environ.get("GITHUB_REPOSITORY", "")
    commits_url = pull_request.get("commits_url") or ""
    body = pull_request.get("body") or ""
    labels = pr_labels(pull_request)

    failures: list[str] = []

    selected_type_labels = sorted(labels & ALLOWED_TYPE_LABELS)
    if len(selected_type_labels) != 1:
        failures.append(
            "PR must have exactly one type:* label from "
            f"{', '.join(sorted(ALLOWED_TYPE_LABELS))}; found {selected_type_labels or 'none'}"
        )

    issue_number = linked_issue_number(body)
    if issue_number is None:
        failures.append("PR body must include Closes #N, Fixes #N, or Resolves #N")
        return failures

    env_issue_labels = issue_labels_from_env()
    if env_issue_labels is not None:
        linked_issue_labels = env_issue_labels
    else:
        token = os.environ.get("GITHUB_TOKEN")
        if not token:
            failures.append("GITHUB_TOKEN is required to verify linked issue labels")
            return failures
        if not repo:
            failures.append("repository full_name is missing from event payload")
            return failures
        linked_issue_labels = fetch_issue_labels(repo, issue_number, token)

    if "status:approved" not in linked_issue_labels:
        failures.append(f"Linked issue #{issue_number} must have status:approved label")

    failures.extend(validate_maturity_gate(body))
    failures.extend(validate_compatibility(body, labels))
    failures.extend(validate_verification_section(body))

    env_commit_messages = commit_messages_from_env()
    if env_commit_messages is not None:
        commit_messages = env_commit_messages
    else:
        token = os.environ.get("GITHUB_TOKEN")
        if not token:
            failures.append("GITHUB_TOKEN is required to verify PR commits")
            return failures
        if not commits_url:
            failures.append("pull_request.commits_url is missing from event payload")
            return failures
        commit_messages = fetch_pr_commit_messages(commits_url, token)

    failures.extend(validate_commits(commit_messages))

    return failures


def main() -> int:
    try:
        failures = validate()
    except Exception as exc:  # noqa: BLE001 - keep workflow failure human-readable.
        fail(str(exc))
        return 1

    if failures:
        for message in failures:
            fail(message)
        return 1

    print("PR validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
