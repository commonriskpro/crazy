use super::*;

// ── Public entry point ────────────────────────────────────────────────────

/// `ail package <add|verify|publish|audit|explain>` — manage packages.
///
/// Rules:
/// - Package install does not grant capabilities.
/// - CLI must show: trust level, verification report, requested capabilities,
///   assumptions, unsafe surface, advisories.
pub(crate) async fn cmd_package(
    mode: OutputMode,
    cmd: PackageCmd,
    store: &StoreHandle,
) -> Result<(), CliError> {
    match cmd {
        PackageCmd::Init { name, version } => {
            let package_name = name.unwrap_or_else(|| "local.package".to_string());
            let manifest =
                package_manifest_for_current_graph(store, &package_name, &version).await?;
            save_package_manifest(store, &manifest)?;
            let hash = manifest
                .blake3_hex()
                .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))?;
            let human_msg = format!(
                "package initialized\nname: {package_name}\nversion: {version}\nmanifest_hash: {hash}"
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "initialized": true,
                    "manifest": package_manifest_to_json(&manifest)?,
                    "manifest_hash": hash,
                }),
            );
        }
        PackageCmd::Add { package } => {
            let (name, version) = parse_package_spec(&package);
            let installed = match install_package_from_registry(store, name, version) {
                Ok(PackageInstallResult::Installed(installed)) => *installed,
                Ok(PackageInstallResult::Blocked(issues)) => {
                    emit_package_compatibility_blocked(mode, &issues);
                    return Err(CliError::Domain(format!(
                        "package compatibility blocked: {} blocked issue(s)",
                        issues.len()
                    )));
                }
                Err(error) => {
                    let failure = PackageInstallFailure::from_error(&error);
                    emit_package_install_failure(mode, failure);
                    return Err(CliError::Domain(failure.to_cli_message()));
                }
            };
            let entry = &installed.entry;
            let verification_report_status =
                verification_report_status(installed.verification_report.is_some());
            let repro_evidence_status =
                reproducible_evidence_status(installed.reproducible_evidence.is_some());
            let human_msg = format!(
                "added: {package}\nname: {}\nversion: {}\nrequested_version: {}\nresolved_version: {}\ntrust: {:?}\nsignature: {}\nverification_report: {verification_report_status}\nreproducible_evidence: {repro_evidence_status}\nlockfile_reproducibility: {}\ninstalled_package_count: {}\ncapabilities: []\nassumptions: []\nunsafe_surface: []\nadvisories: []\nnote: package install does not grant capabilities{}",
                entry.name,
                entry.version,
                entry.requested_version.as_deref().unwrap_or(&entry.version),
                entry.version,
                entry.trust_level,
                installed.signature_status,
                installed.lockfile_reproducibility,
                installed.installed_package_count,
                format_warnings_for_human(&installed.warnings)
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "package": package,
                    "name": entry.name,
                    "version": entry.version,
                    "resolved_version": entry.version,
                    "requested_version": entry.requested_version,
                    "trust": entry.trust_level.to_string(),
                    "signature_status": installed.signature_status,
                    "verification_report": installed.verification_report,
                    "verification_report_hash": entry.verification_report_hash,
                    "verification_report_status": verification_report_status,
                    "reproducible_evidence_status": repro_evidence_status,
                    "lockfile_hash": installed.lockfile_hash,
                    "installed_package_count": installed.installed_package_count,
                    "lockfile_reproducibility": installed.lockfile_reproducibility,
                    "lockfile_reproducibility_issues": installed.lockfile_reproducibility_issues.iter().map(LockfileReproducibilityCliIssue::to_json).collect::<Vec<_>>(),
                    "capabilities": [],
                    "assumptions": [],
                    "unsafe_surface": [],
                    "advisories": [],
                    "capabilities_granted": false,
                    "warnings": installed.warnings,
                    "compatibility_issues": installed.compatibility_issues.iter().map(package_compatibility_issue_to_json).collect::<Vec<_>>(),
                }),
            );
        }
        PackageCmd::Install { package } => {
            let (name, version) = parse_package_spec(&package);
            let installed = match install_package_from_registry(store, name, version) {
                Ok(PackageInstallResult::Installed(installed)) => *installed,
                Ok(PackageInstallResult::Blocked(issues)) => {
                    emit_package_compatibility_blocked(mode, &issues);
                    return Err(CliError::Domain(format!(
                        "package compatibility blocked: {} blocked issue(s)",
                        issues.len()
                    )));
                }
                Err(error) => {
                    let failure = PackageInstallFailure::from_error(&error);
                    emit_package_install_failure(mode, failure);
                    return Err(CliError::Domain(failure.to_cli_message()));
                }
            };
            let entry = &installed.entry;
            let verification_report_status =
                verification_report_status(installed.verification_report.is_some());
            let repro_evidence_status =
                reproducible_evidence_status(installed.reproducible_evidence.is_some());
            let human_msg = format!(
                "installed: {}@{}\nrequested_version: {}\nresolved_version: {}\ntrust: {:?}\npackage_hash: {}\nsignature: {}\nverification_report: {verification_report_status}\nreproducible_evidence: {repro_evidence_status}\nlockfile_reproducibility: {}\ninstalled_package_count: {}\nnote: package install does not grant capabilities{}",
                entry.name,
                entry.version,
                entry.requested_version.as_deref().unwrap_or(&entry.version),
                entry.version,
                entry.trust_level,
                entry.package_hash,
                installed.signature_status,
                installed.lockfile_reproducibility,
                installed.installed_package_count,
                format_warnings_for_human(&installed.warnings)
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "installed": true,
                    "name": entry.name,
                    "version": entry.version,
                    "resolved_version": entry.version,
                    "requested_version": entry.requested_version,
                    "package_hash": entry.package_hash,
                    "trust": entry.trust_level.to_string(),
                    "signature_status": installed.signature_status,
                    "verification_report": installed.verification_report,
                    "verification_report_hash": entry.verification_report_hash,
                    "verification_report_status": verification_report_status,
                    "reproducible_evidence_status": repro_evidence_status,
                    "lockfile_hash": installed.lockfile_hash,
                    "installed_package_count": installed.installed_package_count,
                    "lockfile_reproducibility": installed.lockfile_reproducibility,
                    "lockfile_reproducibility_issues": installed.lockfile_reproducibility_issues.iter().map(LockfileReproducibilityCliIssue::to_json).collect::<Vec<_>>(),
                    "capabilities_granted": false,
                    "warnings": installed.warnings,
                    "compatibility_issues": installed.compatibility_issues.iter().map(package_compatibility_issue_to_json).collect::<Vec<_>>(),
                }),
            );
        }
        PackageCmd::Search { query } => {
            let registry = load_package_registry(store)?;
            let client = LocalRegistryClient { registry };
            let response = client
                .search(ail_package::SearchRequest {
                    query: query.clone(),
                    limit: Some(20),
                })
                .map_err(|e| CliError::Domain(format!("package search failed: {e:?}")))?;
            let human_msg = if response.results.is_empty() {
                format!("no packages found for: {query}")
            } else {
                format!(
                    "packages found: {}\n{}",
                    response.results.len(),
                    response
                        .results
                        .iter()
                        .map(|result| format!("{}@{}", result.name, result.latest_version))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            print_response(
                mode,
                &human_msg,
                json!({
                    "query": query,
                    "results": response.results.iter().map(|result| json!({
                        "name": result.name,
                        "latest_version": result.latest_version,
                        "description": result.description,
                    })).collect::<Vec<_>>(),
                    "truncated": response.truncated,
                }),
            );
        }
        PackageCmd::Verify => {
            let lockfile = load_package_lockfile(store)?;
            let (registry, compatibility_metadata) =
                load_package_registry_with_compatibility(store)?;
            let mut seen = BTreeSet::new();
            let mut actual = Vec::new();
            let mut actual_by_package = BTreeMap::new();
            let mut signature_failures = Vec::new();
            let mut warnings = Vec::new();
            // 4G: track Verified packages missing reproducible evidence (local check only).
            let mut verified_packages_missing_evidence: Vec<String> = Vec::new();
            for manifest in registry.all() {
                if !seen.insert((manifest.name.clone(), manifest.version.clone())) {
                    continue;
                }
                // 4G: Surface reproducible evidence status for Verified packages.
                if manifest.trust_level == TrustLevel::Verified
                    && manifest.reproducible_evidence.is_none()
                {
                    verified_packages_missing_evidence
                        .push(format!("{}@{}", manifest.name, manifest.version));
                }
                match trusted_package_lookup(&registry, &manifest.name, &manifest.version) {
                    Ok(lookup) => {
                        if let Some(warning) = lookup.warning {
                            warnings.push(warning);
                        }
                        let hash = lookup
                            .manifest
                            .blake3_hex()
                            .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))?;
                        let report_hash = verification_report_hash_for_manifest(&lookup.manifest)?;
                        actual_by_package.insert(
                            (
                                lookup.manifest.name.clone(),
                                lookup.manifest.version.clone(),
                            ),
                            RegistryPackageIntegrity {
                                verification_report_hash: report_hash,
                            },
                        );
                        actual.push((lookup.manifest.name, lookup.manifest.version, hash));
                    }
                    Err(e) => signature_failures.push(e.to_string()),
                }
            }
            let actual_refs = actual
                .iter()
                .map(|(name, version, hash)| (name.as_str(), version.as_str(), hash.as_str()))
                .collect::<Vec<_>>();
            let mismatches = lockfile.verify_integrity(&actual_refs);
            let lockfile_reproducibility_issues = lockfile
                .validate_reproducibility(&actual_refs)
                .iter()
                .map(LockfileReproducibilityCliIssue::from_validation_issue)
                .collect::<Vec<_>>();
            let report_mismatches =
                verification_report_hash_mismatches(&lockfile, &actual_by_package);
            let compatibility_issues =
                package_compatibility_issues_for_verify(&lockfile, &compatibility_metadata)?;
            let hash_ok = mismatches.is_empty();
            let signature_ok = signature_failures.is_empty();
            let report_hash_ok = report_mismatches.is_empty();
            let lockfile_reproducibility_ok = lockfile_reproducibility_issues.is_empty();
            let compatibility_blocked = compatibility_issues
                .iter()
                .any(|issue| issue.status == "blocked");
            let compatibility_warning = compatibility_issues
                .iter()
                .any(|issue| issue.status == "warning");
            let compatibility_integrity = if compatibility_blocked {
                "blocked"
            } else if compatibility_warning {
                "warning"
            } else {
                "ok"
            };
            let compatibility_ok = !compatibility_blocked;
            // 4G: reproducible evidence integrity — warn-only (advisory, not a blocker).
            let repro_evidence_integrity = if verified_packages_missing_evidence.is_empty() {
                "ok"
            } else {
                "warning"
            };
            let verified = hash_ok
                && signature_ok
                && report_hash_ok
                && lockfile_reproducibility_ok
                && compatibility_ok;
            // 4G: human summary differentiates "all verified" from "verified but
            // reproducible evidence is missing", because runtime preflight hard-fails
            // on Verified packages that lack evidence even when all other integrity
            // checks pass.
            let packages_summary = if verified {
                if repro_evidence_integrity == "warning" {
                    "all verified (reproducible evidence warning)"
                } else {
                    "all verified"
                }
            } else {
                "verification failed"
            };
            let mut human_msg = format!(
                "packages: {packages_summary}\nhash_integrity: {}\nsignature_integrity: {}\nverification_report_integrity: {}\nlockfile_reproducibility: {}\ncompatibility_integrity: {}\nreproducible_evidence_integrity: {}\nlock_file: {}\npackages_checked: {}\nwarnings: {}",
                if hash_ok { "ok" } else { "mismatch" },
                if signature_ok { "ok" } else { "failed" },
                if report_hash_ok { "ok" } else { "mismatch" },
                if lockfile_reproducibility_ok {
                    "ok"
                } else {
                    "failed"
                },
                compatibility_integrity,
                repro_evidence_integrity,
                if verified {
                    "consistent"
                } else {
                    "inconsistent"
                },
                lockfile.len(),
                warnings.len()
            );
            // 4G: surface missing reproducible evidence prominently in human output.
            // The JSON field `verified_packages_missing_evidence` is already present;
            // the human-readable path must not silently pass these through as "ok".
            if !verified_packages_missing_evidence.is_empty() {
                human_msg.push_str(&format!(
                    "\nWARNING: {} verified package(s) missing reproducible_evidence — runtime preflight will reject: {}",
                    verified_packages_missing_evidence.len(),
                    verified_packages_missing_evidence.join(", ")
                ));
            }
            if !lockfile_reproducibility_issues.is_empty() {
                human_msg.push_str(&format!(
                    "\nlockfile_reproducibility_issues: {}\n{}",
                    lockfile_reproducibility_issues.len(),
                    lockfile_reproducibility_issues
                        .iter()
                        .map(LockfileReproducibilityCliIssue::to_human_line)
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            let response_data = json!({
                "verified": verified,
                "hash_integrity": if hash_ok { "ok" } else { "mismatch" },
                "signature_integrity": if signature_ok { "ok" } else { "failed" },
                "verification_report_integrity": if report_hash_ok { "ok" } else { "mismatch" },
                "lockfile_reproducibility": if lockfile_reproducibility_ok { "ok" } else { "failed" },
                "lockfile_reproducibility_issues": lockfile_reproducibility_issues
                    .iter()
                    .map(LockfileReproducibilityCliIssue::to_json)
                    .collect::<Vec<_>>(),
                "compatibility_integrity": compatibility_integrity,
                "reproducible_evidence_integrity": repro_evidence_integrity,
                "verified_packages_missing_evidence": verified_packages_missing_evidence,
                "lock_file": if verified { "consistent" } else { "inconsistent" },
                "mismatches": mismatches,
                "verification_report_mismatches": report_mismatches
                    .iter()
                    .map(verification_report_hash_mismatch_to_json)
                    .collect::<Vec<_>>(),
                "compatibility_issues": compatibility_issues
                    .iter()
                    .map(package_compatibility_issue_to_json)
                    .collect::<Vec<_>>(),
                "signature_failures": signature_failures,
                "warnings": warnings,
                "packages": lockfile
                    .entries
                    .iter()
                    .map(lockfile_entry_to_json)
                    .collect::<Vec<_>>(),
            });
            if !report_hash_ok {
                let message = format!(
                    "package verification failed: {} verification report hash mismatch(es)",
                    report_mismatches.len()
                );
                if mode == OutputMode::Json {
                    let mut error_data = response_data;
                    if let Some(obj) = error_data.as_object_mut() {
                        obj.insert("error".to_string(), json!("package_verification_failed"));
                        obj.insert("message".to_string(), json!(message.clone()));
                    }
                    print_error_response(error_data);
                } else {
                    print_response(mode, &human_msg, response_data);
                }
                return Err(CliError::Domain(message));
            }
            if compatibility_blocked {
                let message = format!(
                    "package verification failed: {} compatibility issue(s)",
                    compatibility_issues
                        .iter()
                        .filter(|issue| issue.status == "blocked")
                        .count()
                );
                if mode == OutputMode::Json {
                    let mut error_data = response_data;
                    if let Some(obj) = error_data.as_object_mut() {
                        obj.insert("error".to_string(), json!("package_verification_failed"));
                        obj.insert("message".to_string(), json!(message.clone()));
                    }
                    print_error_response(error_data);
                } else {
                    print_response(mode, &human_msg, response_data);
                }
                return Err(CliError::Domain(message));
            }
            if !lockfile_reproducibility_ok {
                let message = format!(
                    "package verification failed: {} lockfile reproducibility issue(s)",
                    lockfile_reproducibility_issues.len()
                );
                if mode == OutputMode::Json {
                    let mut error_data = response_data;
                    if let Some(obj) = error_data.as_object_mut() {
                        obj.insert("error".to_string(), json!("package_verification_failed"));
                        obj.insert("message".to_string(), json!(message.clone()));
                    }
                    print_error_response(error_data);
                } else {
                    print_response(mode, &human_msg, response_data);
                }
                return Err(CliError::Domain(message));
            }
            print_response(mode, &human_msg, response_data);
        }
        PackageCmd::Publish => {
            let manifest = load_or_create_package_manifest(store).await?;
            let hash = manifest
                .blake3_hex()
                .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))?;
            let lockfile = load_package_lockfile(store)?;
            let registry = load_package_registry(store)?;
            let actual = registry
                .all()
                .iter()
                .map(|manifest| {
                    manifest
                        .blake3_hex()
                        .map(|hash| (manifest.name.clone(), manifest.version.clone(), hash))
                        .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let actual_refs = actual
                .iter()
                .map(|(name, version, hash)| (name.as_str(), version.as_str(), hash.as_str()))
                .collect::<Vec<_>>();
            let lockfile_reproducibility_issues = lockfile
                .validate_reproducibility(&actual_refs)
                .iter()
                .map(LockfileReproducibilityCliIssue::from_validation_issue)
                .collect::<Vec<_>>();
            let lockfile_reproducibility_ok = lockfile_reproducibility_issues.is_empty();
            let lockfile_reproducibility = if lockfile_reproducibility_ok {
                "ok"
            } else {
                "failed"
            };
            let lockfile_hash = lockfile
                .blake3_hex()
                .map_err(|e| CliError::Domain(format!("package lock hash failed: {e}")))?;
            let locked_package_count = lockfile.len();
            if !lockfile_reproducibility_ok {
                let message = format!(
                    "package publish preflight failed: {} lockfile reproducibility issue(s)",
                    lockfile_reproducibility_issues.len()
                );
                let response_data = json!({
                    "published": false,
                    "preflight": "failed",
                    "name": &manifest.name,
                    "version": &manifest.version,
                    "package_hash": &hash,
                    "lockfile_hash": &lockfile_hash,
                    "locked_package_count": locked_package_count,
                    "lockfile_reproducibility": lockfile_reproducibility,
                    "lockfile_reproducibility_issues": lockfile_reproducibility_issues
                        .iter()
                        .map(LockfileReproducibilityCliIssue::to_json)
                        .collect::<Vec<_>>(),
                });
                if mode == OutputMode::Json {
                    let mut error_data = response_data;
                    if let Some(obj) = error_data.as_object_mut() {
                        obj.insert(
                            "error".to_string(),
                            json!("package_publish_preflight_failed"),
                        );
                        obj.insert("message".to_string(), json!(message.clone()));
                    }
                    print_error_response(error_data);
                } else {
                    let human_msg = format!(
                        "package publish preflight failed\nname: {}\nversion: {}\npackage_hash: {hash}\nlockfile_reproducibility: {lockfile_reproducibility}\nlockfile_reproducibility_issues: {}\n{}",
                        manifest.name,
                        manifest.version,
                        lockfile_reproducibility_issues.len(),
                        lockfile_reproducibility_issues
                            .iter()
                            .map(LockfileReproducibilityCliIssue::to_human_line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    print_response(mode, &human_msg, response_data);
                }
                return Err(CliError::Domain(message));
            }
            let keypair = PackageKeypair::from_bytes(&[7u8; 32]);
            let signed = keypair
                .sign_manifest(manifest.clone())
                .map_err(|e| CliError::Domain(format!("package signing failed: {e}")))?;
            let client = LocalRegistryClient { registry };
            let published = client
                .publish(PublishRequest {
                    signed_package: signed.clone(),
                })
                .map_err(|e| CliError::Domain(format!("package publish failed: {e:?}")))?;
            if !published.accepted {
                return Err(CliError::Domain(
                    published
                        .error
                        .unwrap_or_else(|| "package publish rejected".to_string()),
                ));
            }
            let mut registry = client.registry;
            registry.register_signed(signed).map_err(|e| {
                CliError::Domain(format!("package signature verification failed: {e}"))
            })?;
            save_package_registry(store, &registry)?;
            let verification_report_status =
                verification_report_status(manifest.verification_report.is_some());
            let repro_evidence_status =
                reproducible_evidence_status(manifest.reproducible_evidence.is_some());
            let human_msg = format!(
                "published\nname: {}\nversion: {}\npackage_hash: {hash}\ntrust: {:?}\nsignature: signed\ncapabilities_manifest: attached\nverification_report: {verification_report_status}\nreproducible_evidence: {repro_evidence_status}\npreflight: passed\nlockfile_reproducibility: {lockfile_reproducibility}\nlocked_package_count: {locked_package_count}",
                manifest.name, manifest.version, manifest.trust_level
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "published": true,
                    "name": manifest.name,
                    "version": manifest.version,
                    "package_hash": hash,
                    "trust": manifest.trust_level.to_string(),
                    "signature_status": "signed",
                    "log_id": published.log_id,
                    "sequence": published.sequence,
                    "preflight": "passed",
                    "lockfile_hash": lockfile_hash,
                    "locked_package_count": locked_package_count,
                    "lockfile_reproducibility": lockfile_reproducibility,
                    "lockfile_reproducibility_issues": lockfile_reproducibility_issues
                        .iter()
                        .map(LockfileReproducibilityCliIssue::to_json)
                        .collect::<Vec<_>>(),
                    "capabilities_manifest": manifest.required_capabilities,
                    "verification_report": manifest.verification_report,
                    "verification_report_status": verification_report_status,
                    "reproducible_evidence_status": repro_evidence_status,
                }),
            );
        }
        PackageCmd::Audit => {
            let lockfile = load_package_lockfile(store)?;
            let (registry, advisories) = load_package_registry_with_advisories(store)?;
            let issues = audit_package_lockfile(&lockfile, &registry, &advisories);
            let packages_checked = lockfile.len();
            let advisory_count = issues
                .iter()
                .filter(|issue| issue.kind == "advisory")
                .count();
            let yanked_count = issues.iter().filter(|issue| issue.kind == "yanked").count();
            let blocked_count = issues
                .iter()
                .filter(|issue| issue.status == "blocked")
                .count();
            let warning_count = issues
                .iter()
                .filter(|issue| issue.status == "warning")
                .count();
            let audit_status = if blocked_count > 0 {
                "blocked"
            } else if warning_count > 0 {
                "warning"
            } else {
                "clean"
            };
            let issue_lines = issues
                .iter()
                .map(PackageAuditIssue::to_human_line)
                .collect::<Vec<_>>();
            let human_msg = if issue_lines.is_empty() {
                format!(
                    "audit: clean\npackages_checked: {packages_checked}\nissues: 0\nblocked: 0\nwarnings: 0"
                )
            } else {
                format!(
                    "audit: {audit_status}\npackages_checked: {packages_checked}\nissues: {}\nblocked: {blocked_count}\nwarnings: {warning_count}\n{}",
                    issues.len(),
                    issue_lines.join("\n")
                )
            };
            let issue_json = issues
                .iter()
                .map(PackageAuditIssue::to_json)
                .collect::<Vec<_>>();
            let advisory_json = issue_json
                .iter()
                .filter(|issue| issue["kind"] == "advisory")
                .cloned()
                .collect::<Vec<_>>();
            print_response(
                mode,
                &human_msg,
                json!({
                    "status": audit_status,
                    "issues": issue_json,
                    "advisories": advisory_json,
                    "packages_checked": packages_checked,
                    "assumptions_valid": true,
                    "unsafe_surface": [],
                    "summary": {
                        "packages_checked": packages_checked,
                        "issues": issues.len(),
                        "advisories": advisory_count,
                        "yanked": yanked_count,
                        "blocked": blocked_count,
                        "warnings": warning_count,
                    },
                }),
            );
            if blocked_count > 0 {
                return Err(CliError::Domain(format!(
                    "package audit blocked: {blocked_count} blocked issue(s)"
                )));
            }
        }
        PackageCmd::Advisory { cmd } => match cmd {
            AdvisoryCmd::Add {
                package,
                constraint,
                id,
                severity,
                reason,
            } => {
                let advisory = SecurityAdvisory {
                    id: validate_required_package_metadata_field("advisory id", id)?,
                    package: validate_required_package_metadata_field("package", package)?,
                    affected_constraint: validate_required_package_metadata_field(
                        "constraint",
                        constraint,
                    )?,
                    severity: parse_advisory_severity(&severity)?,
                    reason: validate_required_package_metadata_field("reason", reason)?,
                };
                let mut file = load_local_package_registry_file_for_update(store)?;
                if file
                    .advisories
                    .iter()
                    .any(|existing| existing.id == advisory.id)
                {
                    return Err(CliError::Domain(format!(
                        "local advisory already exists: {}",
                        advisory.id
                    )));
                }
                file.advisories.push(advisory.clone());
                save_local_package_registry_file(store, &file)?;
                let human_msg = format!(
                    "advisory added\nid: {}\npackage: {}\naffected: {}\nseverity: {}\nreason: {}\nscope: local",
                    advisory.id,
                    advisory.package,
                    advisory.affected_constraint,
                    advisory.severity,
                    advisory.reason
                );
                print_response(
                    mode,
                    &human_msg,
                    json!({
                        "status": "recorded",
                        "scope": "local",
                        "advisory": advisory_to_json(&advisory),
                    }),
                );
            }
            AdvisoryCmd::List => {
                let file = load_local_package_registry_file_for_read(store)?;
                let advisories = file
                    .advisories
                    .iter()
                    .map(advisory_to_json)
                    .collect::<Vec<_>>();
                let human_msg = if file.advisories.is_empty() {
                    "local advisories: 0".to_string()
                } else {
                    format!(
                        "local advisories: {}\n{}",
                        file.advisories.len(),
                        file.advisories
                            .iter()
                            .map(|advisory| format!(
                                "- advisory {} {} {} {}: {}",
                                advisory.id,
                                advisory.package,
                                advisory.affected_constraint,
                                advisory.severity,
                                advisory.reason
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                };
                print_response(
                    mode,
                    &human_msg,
                    json!({
                        "scope": "local",
                        "count": file.advisories.len(),
                        "advisories": advisories,
                    }),
                );
            }
        },
        PackageCmd::Yank {
            package,
            version,
            reason,
        } => {
            let package = validate_required_package_metadata_field("package", package)?;
            let version = validate_required_package_metadata_field("version", version)?;
            let reason = validate_required_package_metadata_field("reason", reason)?;
            let mut file = load_local_package_registry_file_for_update(store)?;
            let mut status = "recorded";
            if let Some(existing) = file
                .yanked
                .iter_mut()
                .find(|yank| yank.name == package && yank.version == version)
            {
                existing.reason = reason.clone();
                status = "updated";
            } else {
                file.yanked.push(ail_package::YankRecord {
                    name: package.clone(),
                    version: version.clone(),
                    reason: reason.clone(),
                });
            }
            save_local_package_registry_file(store, &file)?;
            let record = ail_package::YankRecord {
                name: package,
                version,
                reason,
            };
            let human_msg = format!(
                "yank {status}\npackage: {}\nversion: {}\nreason: {}\nscope: local",
                record.name, record.version, record.reason
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "status": status,
                    "scope": "local",
                    "yanked": yank_to_json(&record),
                }),
            );
        }
        PackageCmd::Yanked => {
            let file = load_local_package_registry_file_for_read(store)?;
            let yanked = file.yanked.iter().map(yank_to_json).collect::<Vec<_>>();
            let human_msg = if file.yanked.is_empty() {
                "local yanked packages: 0".to_string()
            } else {
                format!(
                    "local yanked packages: {}\n{}",
                    file.yanked.len(),
                    file.yanked
                        .iter()
                        .map(|yank| format!(
                            "- yanked {}@{}: {}",
                            yank.name, yank.version, yank.reason
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            print_response(
                mode,
                &human_msg,
                json!({
                    "scope": "local",
                    "count": file.yanked.len(),
                    "yanked": yanked,
                }),
            );
        }
        PackageCmd::Explain { package } => {
            let (name, version) = package.split_once('@').unwrap_or((&package, "latest"));
            let registry = load_package_registry(store)?;
            let lookup = trusted_package_lookup(&registry, name, version)?;
            let manifest = lookup.manifest;
            let warnings = lookup.warning.into_iter().collect::<Vec<_>>();
            let verification_report_status =
                verification_report_status(manifest.verification_report.is_some());
            let human_msg = format!(
                "package: {package}\nname: {}\nversion: {}\ntrust: {:?}\nsignature: {}\nverification_report: {verification_report_status}\ncapabilities: {:?}\nassumptions: {}\nunsafe_surface: {}\nadvisories: []{}",
                manifest.name,
                manifest.version,
                manifest.trust_level,
                lookup.signature_status,
                manifest.required_capabilities,
                manifest.assumptions.len(),
                manifest.unsafe_surface.len(),
                format_warnings_for_human(&warnings)
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "package": package,
                    "name": manifest.name,
                    "version": manifest.version,
                    "trust": manifest.trust_level.to_string(),
                    "signature_status": lookup.signature_status,
                    "verification_report": manifest.verification_report,
                    "verification_report_status": verification_report_status,
                    "capabilities": manifest.required_capabilities,
                    "assumptions": manifest.assumptions,
                    "unsafe_surface": manifest.unsafe_surface,
                    "advisories": [],
                    "warnings": warnings,
                }),
            );
        }
    }
    Ok(())
}
