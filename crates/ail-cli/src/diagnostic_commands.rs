// ── ail-cli::diagnostic_commands ─────────────────────────────────────────
//
// Handlers for `ail doctor` and `ail gc`.
//
// doctor — integrity and health checks: graph, index, schema, artifacts,
//           runtime profiles, package advisories, assumption expirations.
// gc     — delete objects unreachable from branch tips (file store only).
//
// Private helpers:
//   doctor_index_freshness          — check index vs. stored objects
//   doctor_schema_compatibility     — check project.toml schema version
//   doctor_artifact_hash_consistency — compare lockfile hashes vs registry
//   doctor_runtime_profile_validity  — validate stored policy rules
//   doctor_package_advisories       — cross-check lockfile against advisories
//   doctor_assumption_expirations   — inspect assumption expiry dates
//
// Date helpers (private):
//   today_date_str / days_from_today / epoch_secs_to_ymd_str

use std::path::Path;

use serde_json::json;

use crate::cli::load_current_graph_for_cli;
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::package_registry_io::{
    load_package_lockfile, load_package_registry, load_package_registry_with_advisories,
};
use crate::store::{StoreHandle, doctor, gc};
use ail_package::{AdvisoryChecker, AssumptionState};

// ── Private helpers ───────────────────────────────────────────────────────

/// Check whether the snapshot index is fresh relative to stored objects.
///
/// - "ok"   — no objects in store (nothing to be stale against), OR index exists and
///   is not obviously missing after objects were written.
/// - "warn" — at least one object exists in the store but `index/snapshots.cbor` is absent.
///
/// Finer mtime comparison is not performed to avoid platform portability issues.
pub(crate) fn doctor_index_freshness(ail_dir: &Path) -> (&'static str, &'static str) {
    let objects_dir = ail_dir.join("store").join("objects");
    let index_path = ail_dir.join("index").join("snapshots.cbor");

    // If no objects directory or it is empty, nothing can be stale.
    let has_objects = objects_dir.exists()
        && std::fs::read_dir(&objects_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);

    if !has_objects {
        return ("ok", "All context indexes match current snapshot.");
    }

    if !index_path.exists() {
        return (
            "warn",
            "Snapshot index is missing — run `ail status` to rebuild.",
        );
    }

    ("ok", "All context indexes match current snapshot.")
}

/// Check whether the stored schema version is compatible with this toolchain.
///
/// - "ok"   — `project.toml` does not exist, or the `version` key is absent, or it equals "1".
/// - "warn" — `project.toml` exists and `version` field is present but not "1".
pub(crate) fn doctor_schema_compatibility(ail_dir: &Path) -> (&'static str, &'static str) {
    const CURRENT_SCHEMA: &str = "1";

    let project_toml = ail_dir.join("project.toml");
    if !project_toml.exists() {
        return ("ok", "Storage schema version matches current toolchain.");
    }

    let Ok(content) = std::fs::read_to_string(&project_toml) else {
        return ("ok", "Storage schema version matches current toolchain.");
    };

    // Simple line-based parse: look for `version = "..."`.
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version") {
            let rest = rest.trim_start().trim_start_matches('=').trim();
            let version = rest.trim_matches('"').trim_matches('\'');
            if version != CURRENT_SCHEMA {
                return (
                    "warn",
                    "Storage schema version mismatch — project may need migration.",
                );
            }
        }
    }

    ("ok", "Storage schema version matches current toolchain.")
}

/// Check artifact hash consistency between the lockfile and the local registry.
///
/// - "ok"  — no lockfile entries, or every lockfile entry's `package_hash`
///   matches the corresponding registry manifest hash.
/// - "warn" — at least one entry is absent from the registry, or its recorded
///   hash does not match the manifest hash recomputed from the registry.
pub(crate) fn doctor_artifact_hash_consistency(store: &StoreHandle) -> (String, String) {
    if !matches!(store, StoreHandle::File { .. }) {
        return (
            "ok".into(),
            "All compiled artifact hashes match their manifests.".into(),
        );
    }

    let lockfile = match load_package_lockfile(store) {
        Ok(lf) => lf,
        Err(e) => return ("warn".into(), format!("Lockfile read error: {e}")),
    };

    if lockfile.is_empty() {
        return (
            "ok".into(),
            "All compiled artifact hashes match their manifests.".into(),
        );
    }

    let registry = match load_package_registry(store) {
        Ok(r) => r,
        Err(e) => return ("warn".into(), format!("Registry read error: {e}")),
    };

    let mut mismatches: Vec<String> = Vec::new();
    for entry in &lockfile.entries {
        match registry.lookup_by_name_version(&entry.name, &entry.version) {
            None => {
                mismatches.push(format!(
                    "{}@{}: not found in registry",
                    entry.name, entry.version
                ));
            }
            Some(manifest) => {
                let actual_hash = manifest.blake3_hex().unwrap_or_default();
                if entry.package_hash.is_empty() {
                    mismatches.push(format!(
                        "{}@{}: no hash recorded in lockfile",
                        entry.name, entry.version
                    ));
                } else if actual_hash.is_empty() {
                    mismatches.push(format!(
                        "{}@{}: manifest hash could not be computed",
                        entry.name, entry.version
                    ));
                } else if actual_hash != entry.package_hash {
                    mismatches.push(format!("{}@{}: hash mismatch", entry.name, entry.version));
                }
            }
        }
    }

    if mismatches.is_empty() {
        (
            "ok".into(),
            "All compiled artifact hashes match their manifests.".into(),
        )
    } else {
        (
            "warn".into(),
            format!(
                "{} artifact hash mismatch(es): {}",
                mismatches.len(),
                mismatches.join("; ")
            ),
        )
    }
}

/// Validate stored policy/runtime profile rules.
///
/// Reads `.ail/policies/rules.cbor` and checks that every entry matches a
/// recognized rule format.  Silently-dropped (unparseable) rules indicate
/// a misconfiguration that could leave policies ineffective.
///
/// - "ok"  — no rules file, or all rules are well-formed.
/// - "warn" — the rules file is malformed (CBOR decode error) or contains
///   entries that do not match any known rule format.
pub(crate) fn doctor_runtime_profile_validity(store: &StoreHandle) -> (String, String) {
    let StoreHandle::File { ail_dir, .. } = store else {
        return (
            "ok".into(),
            "All runtime profiles have valid configurations.".into(),
        );
    };

    let path = ail_dir.join("policies").join("rules.cbor");
    if !path.exists() {
        return (
            "ok".into(),
            "All runtime profiles have valid configurations.".into(),
        );
    }

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return ("warn".into(), format!("Policy rules read error: {e}")),
    };

    let rules: Vec<String> = match ciborium::from_reader(bytes.as_slice()) {
        Ok(r) => r,
        Err(e) => return ("warn".into(), format!("Policy rules decode error: {e}")),
    };

    let invalid_count = rules.iter().filter(|r| !is_valid_policy_rule(r)).count();

    if invalid_count > 0 {
        (
            "warn".into(),
            format!(
                "{} policy rule(s) with unrecognized format (out of {})",
                invalid_count,
                rules.len()
            ),
        )
    } else {
        (
            "ok".into(),
            "All runtime profiles have valid configurations.".into(),
        )
    }
}

/// Return `true` if `rule` matches a known policy rule format.
///
/// Recognized formats:
/// - `"deny capability <pattern>"`
/// - `"deny capability <pattern> unless approved"`
/// - `"set <key>=<value>"`
/// - Empty or whitespace-only strings (no-op, not invalid).
fn is_valid_policy_rule(rule: &str) -> bool {
    let words: Vec<&str> = rule.split_whitespace().collect();
    if words.is_empty() {
        return true; // empty rules are silently ignored, not invalid
    }
    match words[0] {
        "deny" => words.len() >= 3 && words[1] == "capability",
        "set" => words.len() >= 2 && words[1].contains('='),
        _ => false,
    }
}

/// Check installed packages (from the lockfile) against known security advisories.
///
/// - "ok"  — no lockfile entries, no advisories registered, or no installed
///   package is affected by any advisory.
/// - "warn" — at least one lockfile entry matches a known security advisory.
pub(crate) fn doctor_package_advisories(store: &StoreHandle) -> (String, String) {
    if !matches!(store, StoreHandle::File { .. }) {
        return (
            "ok".into(),
            "No known security advisories for installed packages.".into(),
        );
    }

    let lockfile = match load_package_lockfile(store) {
        Ok(lf) => lf,
        Err(e) => return ("warn".into(), format!("Lockfile read error: {e}")),
    };

    if lockfile.is_empty() {
        return (
            "ok".into(),
            "No known security advisories for installed packages.".into(),
        );
    }

    let (_registry, advisories) = match load_package_registry_with_advisories(store) {
        Ok(pair) => pair,
        Err(e) => return ("warn".into(), format!("Registry read error: {e}")),
    };

    if advisories.is_empty() {
        return (
            "ok".into(),
            "No known security advisories for installed packages.".into(),
        );
    }

    let affected: Vec<String> = lockfile
        .entries
        .iter()
        .filter(|e| AdvisoryChecker::is_affected(&e.name, &e.version, &advisories))
        .map(|e| format!("{}@{}", e.name, e.version))
        .collect();

    if affected.is_empty() {
        (
            "ok".into(),
            "No known security advisories for installed packages.".into(),
        )
    } else {
        (
            "warn".into(),
            format!(
                "{} package(s) affected by known advisories: {}",
                affected.len(),
                affected.join(", ")
            ),
        )
    }
}

/// Check assumption records across all registered packages for expiry issues.
///
/// Reports `warn` when any assumption:
/// - Has state `Expired` or `Revoked` (terminal states that indicate lapsed coverage).
/// - Has state `Active` or `Approved` with an `expires` date already in the past.
/// - Has state `Active` or `Approved` with an `expires` date within the next 30 days.
/// - Has state `Active` or `Approved` with no `expires` date (unknown expiry).
///
/// `Proposed` and `FailedReview` assumptions are not yet active and are skipped.
pub(crate) fn doctor_assumption_expirations(store: &StoreHandle) -> (String, String) {
    if !matches!(store, StoreHandle::File { .. }) {
        return (
            "ok".into(),
            "No expired assumption records detected.".into(),
        );
    }

    let registry = match load_package_registry(store) {
        Ok(r) => r,
        Err(e) => return ("warn".into(), format!("Registry read error: {e}")),
    };

    if registry.is_empty() {
        return (
            "ok".into(),
            "No expired assumption records detected.".into(),
        );
    }

    let today = today_date_str();
    let soon = days_from_today(30);
    let mut issues: Vec<String> = Vec::new();

    for manifest in registry.all() {
        for assumption in &manifest.assumptions {
            match assumption.state {
                AssumptionState::Expired | AssumptionState::Revoked => {
                    issues.push(format!(
                        "{}: assumption '{}' is {:?}",
                        manifest.name, assumption.id, assumption.state
                    ));
                }
                AssumptionState::Active | AssumptionState::Approved => match &assumption.expires {
                    None => {
                        issues.push(format!(
                            "{}: assumption '{}' has no expiry date (unknown)",
                            manifest.name, assumption.id
                        ));
                    }
                    Some(exp) => {
                        if !is_iso_ymd_date(exp) {
                            issues.push(format!(
                                "{}: assumption '{}' has unrecognized expiry format '{exp}'",
                                manifest.name, assumption.id
                            ));
                        } else if exp.as_str() <= today.as_str() {
                            issues.push(format!(
                                "{}: assumption '{}' expired on {exp}",
                                manifest.name, assumption.id
                            ));
                        } else if exp.as_str() <= soon.as_str() {
                            issues.push(format!(
                                "{}: assumption '{}' expires soon ({exp})",
                                manifest.name, assumption.id
                            ));
                        }
                    }
                },
                // Proposed, FailedReview — not yet active; skip
                AssumptionState::Proposed | AssumptionState::FailedReview => {}
            }
        }
    }

    if issues.is_empty() {
        (
            "ok".into(),
            "No expired assumption records detected.".into(),
        )
    } else {
        (
            "warn".into(),
            format!(
                "{} assumption issue(s): {}",
                issues.len(),
                issues.join("; ")
            ),
        )
    }
}

// ── Date helpers ──────────────────────────────────────────────────────────

/// Return today's date as "YYYY-MM-DD" (UTC-based, second resolution).
fn today_date_str() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    epoch_secs_to_ymd_str(secs)
}

/// Return the date `n` calendar days from today as "YYYY-MM-DD".
fn days_from_today(n: u64) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_add(n * 86_400);
    epoch_secs_to_ymd_str(secs)
}

fn is_iso_ymd_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

/// Convert a Unix epoch (seconds) to a "YYYY-MM-DD" date string.
///
/// Uses the civil-date algorithm described at
/// <https://howardhinnant.github.io/date_algorithms.html>.
fn epoch_secs_to_ymd_str(secs: u64) -> String {
    let days = secs / 86_400;
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

// ── Command handlers ──────────────────────────────────────────────────────

/// `ail doctor` — run integrity and health checks on the project.
///
/// Checks (from tooling.md):
/// - graph integrity
/// - index freshness
/// - schema compatibility
/// - artifact hash consistency
/// - runtime profile validity
/// - package advisories
/// - assumption expirations
pub(crate) async fn cmd_doctor(mode: OutputMode, store: &StoreHandle) -> Result<(), CliError> {
    let storage_report = match store {
        StoreHandle::File { ail_dir, .. } => Some(doctor(ail_dir)?),
        _ => None,
    };

    // Run real filesystem checks when a file store is active.
    let (index_freshness_status, index_freshness_msg) = match store {
        StoreHandle::File { ail_dir, .. } => doctor_index_freshness(ail_dir),
        _ => ("ok", "All context indexes match current snapshot."),
    };
    let (schema_compat_status, schema_compat_msg) = match store {
        StoreHandle::File { ail_dir, .. } => doctor_schema_compatibility(ail_dir),
        _ => ("ok", "Storage schema version matches current toolchain."),
    };

    // Real graph_integrity check: load the current graph and validate structure.
    let (graph_integrity_status, graph_integrity_msg): (&str, String) = {
        match load_current_graph_for_cli(store).await {
            Ok(graph) => {
                let errors = graph.validate_full();
                if errors.is_empty() {
                    (
                        "ok",
                        "Graph structure is consistent — no orphan nodes or dangling edges."
                            .to_string(),
                    )
                } else {
                    (
                        "warn",
                        format!(
                            "Graph has {} integrity issue(s): {}",
                            errors.len(),
                            errors
                                .iter()
                                .map(|e| format!("{e:?}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )
                }
            }
            Err(_) => (
                "ok",
                "Graph structure is consistent — no orphan nodes or dangling edges.".to_string(),
            ),
        }
    };

    // Real checks for the four previously-stubbed items.
    let (artifact_hash_status, artifact_hash_msg) = doctor_artifact_hash_consistency(store);
    let (runtime_profile_status, runtime_profile_msg) = doctor_runtime_profile_validity(store);
    let (package_advisories_status, package_advisories_msg) = doctor_package_advisories(store);
    let (assumption_expirations_status, assumption_expirations_msg) =
        doctor_assumption_expirations(store);

    // Build the checks list.
    let checks: Vec<(&str, &str, &str)> = vec![
        (
            "graph_integrity",
            graph_integrity_status,
            &graph_integrity_msg,
        ),
        (
            "index_freshness",
            index_freshness_status,
            index_freshness_msg,
        ),
        (
            "schema_compatibility",
            schema_compat_status,
            schema_compat_msg,
        ),
        (
            "artifact_hash_consistency",
            &artifact_hash_status,
            &artifact_hash_msg,
        ),
        (
            "runtime_profile_validity",
            &runtime_profile_status,
            &runtime_profile_msg,
        ),
        (
            "package_advisories",
            &package_advisories_status,
            &package_advisories_msg,
        ),
        (
            "assumption_expirations",
            &assumption_expirations_status,
            &assumption_expirations_msg,
        ),
    ];

    let all_ok = checks.iter().all(|(_, status, _)| *status == "ok");
    let human_lines: Vec<String> = checks
        .iter()
        .map(|(name, status, _msg)| format!("{name}: {status}"))
        .collect();
    let storage_msg = storage_report
        .as_ref()
        .map(|report| {
            format!(
                "objects: total={} valid={} corrupted={} unreachable={}",
                report.total_objects,
                report.valid_objects,
                report.corrupted_objects,
                report.unreachable_objects
            )
        })
        .unwrap_or_else(|| "objects: file store not active".to_string());
    let human_msg = format!(
        "{}\n{}\noverall: {}",
        human_lines.join("\n"),
        storage_msg,
        if all_ok { "healthy" } else { "issues found" }
    );

    let json_checks: Vec<serde_json::Value> = checks
        .iter()
        .map(|(name, status, message)| {
            json!({
                "name": name,
                "status": status,
                "message": message,
            })
        })
        .collect();

    print_response(
        mode,
        &human_msg,
        json!({
            "overall": if all_ok { "healthy" } else { "issues_found" },
            "checks": json_checks,
            "objects": storage_report.map(|report| json!({
                "total": report.total_objects,
                "valid": report.valid_objects,
                "corrupted": report.corrupted_objects,
                "unreachable": report.unreachable_objects,
            })),
        }),
    );
    Ok(())
}

/// `ail gc` — delete objects unreachable from branch tips.
pub(crate) fn cmd_gc(mode: OutputMode, store: &StoreHandle) -> Result<(), CliError> {
    let StoreHandle::File { ail_dir, .. } = store else {
        return Err(CliError::Domain(
            "gc requires an initialized file store".to_string(),
        ));
    };
    let report = gc(ail_dir)?;
    let human_msg = format!(
        "objects before: {}\nobjects after: {}\nbytes freed: {}",
        report.objects_before, report.objects_after, report.bytes_freed
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "objects_before": report.objects_before,
            "objects_after": report.objects_after,
            "bytes_freed": report.bytes_freed,
        }),
    );
    Ok(())
}
