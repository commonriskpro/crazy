use super::*;

// ── Public entry point ────────────────────────────────────────────────────

fn reproducible_evidence_preflight_issue(
    evidence: &ReproducibleBuildEvidence,
) -> Option<(&'static str, String)> {
    if !is_package_blake3_hex(&evidence.build_inputs_hash) {
        return Some((
            "build_inputs_hash",
            "expected 64-character lowercase BLAKE3 hex digest".to_string(),
        ));
    }
    if evidence.toolchain_id.trim().is_empty() {
        return Some(("toolchain_id", "must be non-empty".to_string()));
    }
    if !is_package_blake3_hex(&evidence.source_digest) {
        return Some((
            "source_digest",
            "expected 64-character lowercase BLAKE3 hex digest".to_string(),
        ));
    }
    if !is_package_blake3_hex(&evidence.recipe_hash) {
        return Some((
            "recipe_hash",
            "expected 64-character lowercase BLAKE3 hex digest".to_string(),
        ));
    }

    let derived = ReproducibleBuildEvidence::compute_build_inputs_hash(
        &evidence.source_digest,
        &evidence.toolchain_id,
    );
    if evidence.build_inputs_hash != derived {
        return Some((
            "build_inputs_hash",
            "must equal BLAKE3(source_digest || toolchain_id)".to_string(),
        ));
    }

    None
}

fn production_provenance_preflight_issue(
    provenance: Option<&Provenance>,
) -> Option<(&'static str, String)> {
    let Some(provenance) = provenance else {
        return Some(("provenance", "must be present".to_string()));
    };
    if provenance
        .source_repository
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Some((
            "source_repository",
            "must identify the package source repository".to_string(),
        ));
    }
    if provenance
        .commit_hash
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Some((
            "commit_hash",
            "must identify the source revision".to_string(),
        ));
    }
    None
}

fn is_package_blake3_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn format_package_string_list(values: &[String]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        values.join(", ")
    }
}

fn format_package_assumptions(manifest: &PackageManifest) -> String {
    if manifest.assumptions.is_empty() {
        "[]".to_string()
    } else {
        manifest
            .assumptions
            .iter()
            .map(|assumption| assumption.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_package_unsafe_surface(manifest: &PackageManifest) -> String {
    if manifest.unsafe_surface.is_empty() {
        "[]".to_string()
    } else {
        manifest
            .unsafe_surface
            .iter()
            .map(|surface| surface.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn missing_package_assumption_ids(
    manifest: &PackageManifest,
    accepted_assumptions: &[String],
) -> Vec<String> {
    manifest
        .assumptions
        .iter()
        .filter(|assumption| !accepted_assumptions.contains(&assumption.id))
        .map(|assumption| assumption.id.clone())
        .collect()
}

fn lockfile_entry_with_assumption_status_to_json(
    entry: &LockfileEntry,
    manifest: Option<&PackageManifest>,
) -> serde_json::Value {
    let mut value = lockfile_entry_to_json(entry);
    let missing_assumptions = manifest
        .map(|manifest| missing_package_assumption_ids(manifest, &entry.accepted_assumptions))
        .unwrap_or_default();
    let assumptions_valid = missing_assumptions.is_empty();
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "assumptions_count".to_string(),
            json!(
                manifest
                    .map(|manifest| manifest.assumptions.len())
                    .unwrap_or(0)
            ),
        );
        object.insert(
            "missing_assumptions".to_string(),
            json!(missing_assumptions),
        );
        object.insert("assumptions_valid".to_string(), json!(assumptions_valid));
    }
    value
}

fn format_package_advisories(advisories: &[PackageAuditIssue]) -> String {
    if advisories.is_empty() {
        "[]".to_string()
    } else {
        advisories
            .iter()
            .map(|issue| format!("{}:{}", issue.kind, issue.status))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `ail package <add|lint|verify|publish|audit|explain>` — manage packages.
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
        PackageCmd::Init {
            name,
            version,
            license,
            source_digest,
            toolchain_id,
            recipe_hash,
            provenance_url,
            source_repository,
            commit_hash,
            build_id,
        } => {
            let package_name = name.unwrap_or_else(|| "local.package".to_string());
            let license = license
                .map(|value| validate_required_package_metadata_field("license", value))
                .transpose()?;
            let reproducible_evidence =
                validate_optional_reproducible_evidence(source_digest, toolchain_id, recipe_hash)?;
            let provenance = validate_optional_package_provenance(
                provenance_url,
                source_repository,
                commit_hash,
                build_id,
            )?;
            let manifest = package_manifest_for_current_graph_with_metadata(
                store,
                &package_name,
                &version,
                license,
                reproducible_evidence,
                provenance,
            )
            .await?;
            save_package_manifest(store, &manifest)?;
            let hash = manifest
                .blake3_hex()
                .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))?;
            let production_issues = manifest.production_validation_issues();
            let production_lint = if production_issues.is_empty() {
                "passed"
            } else {
                "warning"
            };
            let human_msg = format!(
                "package initialized\nname: {package_name}\nversion: {version}\nlicense: {}\nmanifest_hash: {hash}\nprovenance: {}\nreproducible_evidence: {}\nproduction_lint: {production_lint}\nproduction_issue_count: {}",
                manifest.license.as_deref().unwrap_or("none"),
                manifest
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.url.as_deref())
                    .unwrap_or("structured"),
                reproducible_evidence_status(manifest.reproducible_evidence.is_some()),
                production_issues.len()
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "initialized": true,
                    "manifest": package_manifest_to_json(&manifest)?,
                    "manifest_hash": hash,
                    "reproducible_evidence_status": reproducible_evidence_status(
                        manifest.reproducible_evidence.is_some()
                    ),
                    "production_lint": production_lint,
                    "production_issue_count": production_issues.len(),
                    "production_issues": production_issues
                        .iter()
                        .map(package_manifest_issue_to_json)
                        .collect::<Vec<_>>(),
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
            let manifest = &installed.manifest;
            let accepted_assumptions = entry.accepted_assumptions.clone();
            let missing_assumptions =
                missing_package_assumption_ids(manifest, &accepted_assumptions);
            let assumptions_valid = missing_assumptions.is_empty();
            let verification_report_status =
                verification_report_status(installed.verification_report.is_some());
            let repro_evidence_status =
                reproducible_evidence_status(installed.reproducible_evidence.is_some());
            let human_msg = format!(
                "added: {package}\nname: {}\nversion: {}\nrequested_version: {}\nresolved_version: {}\ntrust: {:?}\nsignature: {}\nverification_report: {verification_report_status}\nreproducible_evidence: {repro_evidence_status}\nlockfile_reproducibility: {}\ninstalled_package_count: {}\ncapabilities: {}\nexported_capabilities: {}\nassumptions: {}\naccepted_assumptions: {}\nmissing_assumptions: {}\nassumptions_valid: {}\nunsafe_surface: {}\nadvisories: {}\nnote: package install does not grant capabilities{}",
                entry.name,
                entry.version,
                entry.requested_version.as_deref().unwrap_or(&entry.version),
                entry.version,
                entry.trust_level,
                installed.signature_status,
                installed.lockfile_reproducibility,
                installed.installed_package_count,
                format_package_string_list(&manifest.required_capabilities),
                format_package_string_list(&manifest.exported_capabilities),
                format_package_assumptions(manifest),
                format_package_string_list(&accepted_assumptions),
                format_package_string_list(&missing_assumptions),
                assumptions_valid,
                format_package_unsafe_surface(manifest),
                format_package_advisories(&installed.advisory_issues),
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
                    "artifact_hashes": &entry.artifact_hashes,
                    "verification_report_status": verification_report_status,
                    "reproducible_evidence_status": repro_evidence_status,
                    "lockfile_hash": installed.lockfile_hash,
                    "installed_package_count": installed.installed_package_count,
                    "lockfile_reproducibility": installed.lockfile_reproducibility,
                    "lockfile_reproducibility_issues": installed.lockfile_reproducibility_issues.iter().map(LockfileReproducibilityCliIssue::to_json).collect::<Vec<_>>(),
                    "capabilities": &manifest.required_capabilities,
                    "exported_capabilities": &manifest.exported_capabilities,
                    "assumptions": &manifest.assumptions,
                    "accepted_assumptions": accepted_assumptions,
                    "missing_assumptions": missing_assumptions,
                    "assumptions_valid": assumptions_valid,
                    "unsafe_surface": &manifest.unsafe_surface,
                    "advisories": installed.advisory_issues.iter().map(PackageAuditIssue::to_json).collect::<Vec<_>>(),
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
            let manifest = &installed.manifest;
            let accepted_assumptions = entry.accepted_assumptions.clone();
            let missing_assumptions =
                missing_package_assumption_ids(manifest, &accepted_assumptions);
            let assumptions_valid = missing_assumptions.is_empty();
            let verification_report_status =
                verification_report_status(installed.verification_report.is_some());
            let repro_evidence_status =
                reproducible_evidence_status(installed.reproducible_evidence.is_some());
            let human_msg = format!(
                "installed: {}@{}\nrequested_version: {}\nresolved_version: {}\ntrust: {:?}\npackage_hash: {}\nsignature: {}\nverification_report: {verification_report_status}\nreproducible_evidence: {repro_evidence_status}\nlockfile_reproducibility: {}\ninstalled_package_count: {}\ncapabilities: {}\nexported_capabilities: {}\nassumptions: {}\naccepted_assumptions: {}\nmissing_assumptions: {}\nassumptions_valid: {}\nunsafe_surface: {}\nadvisories: {}\nnote: package install does not grant capabilities{}",
                entry.name,
                entry.version,
                entry.requested_version.as_deref().unwrap_or(&entry.version),
                entry.version,
                entry.trust_level,
                entry.package_hash,
                installed.signature_status,
                installed.lockfile_reproducibility,
                installed.installed_package_count,
                format_package_string_list(&manifest.required_capabilities),
                format_package_string_list(&manifest.exported_capabilities),
                format_package_assumptions(manifest),
                format_package_string_list(&accepted_assumptions),
                format_package_string_list(&missing_assumptions),
                assumptions_valid,
                format_package_unsafe_surface(manifest),
                format_package_advisories(&installed.advisory_issues),
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
                    "artifact_hashes": &entry.artifact_hashes,
                    "verification_report_status": verification_report_status,
                    "reproducible_evidence_status": repro_evidence_status,
                    "lockfile_hash": installed.lockfile_hash,
                    "installed_package_count": installed.installed_package_count,
                    "lockfile_reproducibility": installed.lockfile_reproducibility,
                    "lockfile_reproducibility_issues": installed.lockfile_reproducibility_issues.iter().map(LockfileReproducibilityCliIssue::to_json).collect::<Vec<_>>(),
                    "capabilities": &manifest.required_capabilities,
                    "exported_capabilities": &manifest.exported_capabilities,
                    "assumptions": &manifest.assumptions,
                    "accepted_assumptions": accepted_assumptions,
                    "missing_assumptions": missing_assumptions,
                    "assumptions_valid": assumptions_valid,
                    "unsafe_surface": &manifest.unsafe_surface,
                    "advisories": installed.advisory_issues.iter().map(PackageAuditIssue::to_json).collect::<Vec<_>>(),
                    "capabilities_granted": false,
                    "warnings": installed.warnings,
                    "compatibility_issues": installed.compatibility_issues.iter().map(package_compatibility_issue_to_json).collect::<Vec<_>>(),
                }),
            );
        }

        PackageCmd::AcceptAssumption {
            package,
            assumption,
        } => {
            let (name, requested_version) = parse_package_spec(&package);
            let assumption = validate_required_package_metadata_field("assumption", assumption)?;
            let registry = load_package_registry(store)?;
            let mut lockfile = load_package_lockfile(store)?;
            let matching_indices = lockfile
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.name == name
                        && (requested_version == "latest" || entry.version == requested_version)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let index = match matching_indices.as_slice() {
                [index] => *index,
                [] => {
                    return Err(CliError::NotFound(format!(
                        "locked package not found: {name}@{requested_version}"
                    )));
                }
                _ => {
                    return Err(CliError::Domain(format!(
                        "multiple locked versions for {name}; use an exact package@version"
                    )));
                }
            };
            let locked_name = lockfile.entries[index].name.clone();
            let locked_version = lockfile.entries[index].version.clone();
            let manifest = registry
                .lookup_by_name_version(&locked_name, &locked_version)
                .ok_or_else(|| {
                    CliError::NotFound(format!(
                        "package manifest not found for locked package: {locked_name}@{locked_version}"
                    ))
                })?;
            if !manifest
                .assumptions
                .iter()
                .any(|declared| declared.id == assumption)
            {
                return Err(CliError::Domain(format!(
                    "package {locked_name}@{locked_version} does not declare assumption {assumption}"
                )));
            }

            let entry = &mut lockfile.entries[index];
            let already_accepted = entry.accepted_assumptions.contains(&assumption);
            if !already_accepted {
                entry.accepted_assumptions.push(assumption.clone());
                entry.accepted_assumptions.sort();
                entry.accepted_assumptions.dedup();
                save_package_lockfile(store, &lockfile)?;
            }
            let accepted_assumptions = lockfile.entries[index].accepted_assumptions.clone();
            let status = if already_accepted {
                "already_accepted"
            } else {
                "accepted"
            };
            let human_msg = format!(
                "package assumption {status}\npackage: {locked_name}\nversion: {locked_version}\nassumption: {assumption}\naccepted_assumptions: {}",
                accepted_assumptions.join(", ")
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "status": status,
                    "package": locked_name,
                    "version": locked_version,
                    "assumption": assumption,
                    "accepted": !already_accepted,
                    "accepted_assumptions": accepted_assumptions,
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
        PackageCmd::Lint => {
            let manifest = load_or_create_package_manifest(store).await?;
            let hash = manifest
                .blake3_hex()
                .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))?;
            let issues = manifest.production_validation_issues();
            let passed = issues.is_empty();
            let response_data = json!({
                "passed": passed,
                "preflight": if passed { "passed" } else { "failed" },
                "name": &manifest.name,
                "version": &manifest.version,
                "manifest_hash": &hash,
                "issue_count": issues.len(),
                "issues": issues.iter().map(package_manifest_issue_to_json).collect::<Vec<_>>(),
            });
            if passed {
                print_response(
                    mode,
                    &format!(
                        "package lint passed\nname: {}\nversion: {}\nmanifest_hash: {hash}\nissues: 0",
                        manifest.name, manifest.version
                    ),
                    response_data,
                );
            } else {
                let message = format!(
                    "package lint failed: {} production manifest issue(s)",
                    issues.len()
                );
                if mode == OutputMode::Json {
                    let mut error_data = response_data;
                    if let Some(obj) = error_data.as_object_mut() {
                        obj.insert("error".to_string(), json!("package_lint_failed"));
                        obj.insert("message".to_string(), json!(message.clone()));
                    }
                    print_error_response(error_data);
                } else {
                    let human_msg = format!(
                        "{message}\nname: {}\nversion: {}\nmanifest_hash: {hash}\nissues:\n{}",
                        manifest.name,
                        manifest.version,
                        issues
                            .iter()
                            .map(package_manifest_issue_to_human_line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    print_response(mode, &human_msg, response_data);
                }
                return Err(CliError::Domain(message));
            }
        }
        PackageCmd::Verify => {
            let lockfile = load_package_lockfile(store)?;
            let (registry, compatibility_metadata) =
                load_package_registry_with_compatibility(store)?;
            let mut seen = BTreeSet::new();
            let mut actual = Vec::new();
            let mut actual_artifact_evidence = Vec::new();
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
                        actual_artifact_evidence.push(LockfileArtifactEvidence::new(
                            lookup.manifest.name.clone(),
                            lookup.manifest.version.clone(),
                            lookup.manifest.artifact_hashes.clone(),
                        ));
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
            let mut lockfile_validation_issues = lockfile.validate_reproducibility(&actual_refs);
            lockfile_validation_issues
                .extend(lockfile.validate_artifact_reproducibility(&actual_artifact_evidence));
            let lockfile_reproducibility_issues = lockfile_validation_issues
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
            let mut packages_missing_assumptions = Vec::new();
            let mut missing_assumption_lines = Vec::new();
            for entry in &lockfile.entries {
                let Some(manifest) = registry.lookup_by_name_version(&entry.name, &entry.version)
                else {
                    continue;
                };
                let missing_assumptions =
                    missing_package_assumption_ids(manifest, &entry.accepted_assumptions);
                if missing_assumptions.is_empty() {
                    continue;
                }
                missing_assumption_lines.push(format!(
                    "{}@{}: {}",
                    entry.name,
                    entry.version,
                    missing_assumptions.join(", ")
                ));
                packages_missing_assumptions.push(json!({
                    "name": &entry.name,
                    "version": &entry.version,
                    "missing_assumptions": missing_assumptions,
                }));
            }
            let assumptions_valid = packages_missing_assumptions.is_empty();
            let assumptions_integrity = if assumptions_valid { "ok" } else { "warning" };
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
                "packages: {packages_summary}\nhash_integrity: {}\nsignature_integrity: {}\nverification_report_integrity: {}\nlockfile_reproducibility: {}\ncompatibility_integrity: {}\nassumptions_integrity: {assumptions_integrity}\nreproducible_evidence_integrity: {}\nlock_file: {}\npackages_checked: {}\nwarnings: {}",
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
            if !missing_assumption_lines.is_empty() {
                human_msg.push_str(&format!(
                    "\nWARNING: {} package(s) missing accepted assumptions — run `ail package accept-assumption`: {}",
                    missing_assumption_lines.len(),
                    missing_assumption_lines.join("; ")
                ));
            }
            if !signature_failures.is_empty() {
                human_msg.push_str(&format!(
                    "\nsignature_failures: {}\n{}",
                    signature_failures.len(),
                    signature_failures
                        .iter()
                        .map(|failure| format!("- {failure}"))
                        .collect::<Vec<_>>()
                        .join("\n")
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
                "assumptions_integrity": assumptions_integrity,
                "assumptions_valid": assumptions_valid,
                "packages_missing_assumptions": packages_missing_assumptions,
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
                    .map(|entry| {
                        lockfile_entry_with_assumption_status_to_json(
                            entry,
                            registry.lookup_by_name_version(&entry.name, &entry.version),
                        )
                    })
                    .collect::<Vec<_>>(),
            });
            if !signature_ok {
                let message = format!(
                    "package verification failed: {} signature or package metadata issue(s)",
                    signature_failures.len()
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
        PackageCmd::Publish { production } => {
            let manifest = load_or_create_package_manifest(store).await?;
            if let Err(error) = manifest.validate() {
                let message = format!("package publish preflight failed: {error}");
                let response_data = json!({
                    "published": false,
                    "preflight": "failed",
                    "production": production,
                    "name": &manifest.name,
                    "version": &manifest.version,
                    "error": "package_publish_preflight_failed",
                    "message": message.clone(),
                });
                if mode == OutputMode::Json {
                    print_error_response(response_data);
                } else {
                    print_response(mode, &message, response_data);
                }
                return Err(CliError::Domain(message));
            }
            let hash = manifest
                .blake3_hex()
                .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))?;
            let production_issues = manifest.production_validation_issues();
            if production && !production_issues.is_empty() {
                let message = format!(
                    "package publish preflight failed: {} production manifest issue(s)",
                    production_issues.len()
                );
                let response_data = json!({
                    "published": false,
                    "preflight": "failed",
                    "production": true,
                    "name": &manifest.name,
                    "version": &manifest.version,
                    "package_hash": &hash,
                    "production_lint": "failed",
                    "production_issue_count": production_issues.len(),
                    "production_issues": production_issues
                        .iter()
                        .map(package_manifest_issue_to_json)
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
                        "{message}\nname: {}\nversion: {}\npackage_hash: {hash}\nproduction_lint: failed\nproduction_issues:\n{}",
                        manifest.name,
                        manifest.version,
                        production_issues
                            .iter()
                            .map(package_manifest_issue_to_human_line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    print_response(mode, &human_msg, response_data);
                }
                return Err(CliError::Domain(message));
            }
            if production
                && manifest.trust_level == TrustLevel::Verified
                && manifest.reproducible_evidence.is_none()
            {
                let message =
                    "package publish preflight failed: verified package missing reproducible_evidence"
                        .to_string();
                let response_data = json!({
                    "published": false,
                    "preflight": "failed",
                    "production": true,
                    "name": &manifest.name,
                    "version": &manifest.version,
                    "package_hash": &hash,
                    "production_lint": "passed",
                    "production_issue_count": 0,
                    "production_issues": [],
                    "reproducible_evidence_integrity": "failed",
                    "reproducible_evidence_status": "none",
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
                        "{message}\nname: {}\nversion: {}\npackage_hash: {hash}\nproduction_lint: passed\nreproducible_evidence_integrity: failed\nreproducible_evidence_status: none",
                        manifest.name, manifest.version
                    );
                    print_response(mode, &human_msg, response_data);
                }
                return Err(CliError::Domain(message));
            }
            if production {
                if let Some(evidence) = manifest.reproducible_evidence.as_ref() {
                    if let Some((field, reason)) = reproducible_evidence_preflight_issue(evidence) {
                        let message = format!(
                            "package publish preflight failed: reproducible_evidence field {field} invalid"
                        );
                        let response_data = json!({
                            "published": false,
                            "preflight": "failed",
                            "production": true,
                            "name": &manifest.name,
                            "version": &manifest.version,
                            "package_hash": &hash,
                            "production_lint": "passed",
                            "production_issue_count": 0,
                            "production_issues": [],
                            "reproducible_evidence_integrity": "failed",
                            "reproducible_evidence_status": "present",
                            "reproducible_evidence_issue": {
                                "field": field,
                                "reason": reason,
                            },
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
                                "{message}\nname: {}\nversion: {}\npackage_hash: {hash}\nproduction_lint: passed\nreproducible_evidence_integrity: failed\nreproducible_evidence_status: present\nreproducible_evidence_issue: {field}: {reason}",
                                manifest.name, manifest.version
                            );
                            print_response(mode, &human_msg, response_data);
                        }
                        return Err(CliError::Domain(message));
                    }
                }
            }
            if production {
                if let Some((field, reason)) =
                    production_provenance_preflight_issue(manifest.provenance.as_ref())
                {
                    let message = format!(
                        "package publish preflight failed: provenance field {field} invalid"
                    );
                    let response_data = json!({
                        "published": false,
                        "preflight": "failed",
                        "production": true,
                        "name": &manifest.name,
                        "version": &manifest.version,
                        "package_hash": &hash,
                        "production_lint": "passed",
                        "production_issue_count": 0,
                        "production_issues": [],
                        "reproducible_evidence_integrity": "ok",
                        "provenance_integrity": "failed",
                        "provenance_issue": {
                            "field": field,
                            "reason": reason,
                        },
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
                            "{message}\nname: {}\nversion: {}\npackage_hash: {hash}\nproduction_lint: passed\nreproducible_evidence_integrity: ok\nprovenance_integrity: failed\nprovenance_issue: {field}: {reason}",
                            manifest.name, manifest.version
                        );
                        print_response(mode, &human_msg, response_data);
                    }
                    return Err(CliError::Domain(message));
                }
            }
            if production {
                if let Err(error) = validate_verified_package_evidence(&manifest) {
                    let message = "package publish preflight failed: verification evidence invalid"
                        .to_string();
                    let response_data = json!({
                        "published": false,
                        "preflight": "failed",
                        "production": true,
                        "name": &manifest.name,
                        "version": &manifest.version,
                        "package_hash": &hash,
                        "production_lint": "passed",
                        "production_issue_count": 0,
                        "production_issues": [],
                        "reproducible_evidence_integrity": "ok",
                        "provenance_integrity": "ok",
                        "verification_evidence_integrity": "failed",
                        "verification_evidence_issue": error.to_string(),
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
                            "{message}\nname: {}\nversion: {}\npackage_hash: {hash}\nproduction_lint: passed\nreproducible_evidence_integrity: ok\nprovenance_integrity: ok\nverification_evidence_integrity: failed\nverification_evidence_issue: {error}",
                            manifest.name, manifest.version
                        );
                        print_response(mode, &human_msg, response_data);
                    }
                    return Err(CliError::Domain(message));
                }
            }
            let lockfile = load_package_lockfile(store)?;
            let registry = load_package_registry(store)?;
            let actual_artifact_evidence = registry
                .all()
                .iter()
                .map(|manifest| {
                    LockfileArtifactEvidence::new(
                        manifest.name.clone(),
                        manifest.version.clone(),
                        manifest.artifact_hashes.clone(),
                    )
                })
                .collect::<Vec<_>>();
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
            let mut lockfile_validation_issues = lockfile.validate_reproducibility(&actual_refs);
            lockfile_validation_issues
                .extend(lockfile.validate_artifact_reproducibility(&actual_artifact_evidence));
            let lockfile_reproducibility_issues = lockfile_validation_issues
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
                    "production": production,
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
            let production_lint = if production { "passed" } else { "not_enforced" };
            let provenance_integrity = if production { "ok" } else { "not_enforced" };
            let verification_evidence_integrity = if production { "ok" } else { "not_enforced" };
            let human_msg = format!(
                "published\nname: {}\nversion: {}\npackage_hash: {hash}\ntrust: {:?}\nsignature: signed\ncapabilities_manifest: attached\nverification_report: {verification_report_status}\nreproducible_evidence: {repro_evidence_status}\npreflight: passed\nproduction_lint: {production_lint}\nprovenance_integrity: {provenance_integrity}\nverification_evidence_integrity: {verification_evidence_integrity}\nlockfile_reproducibility: {lockfile_reproducibility}\nlocked_package_count: {locked_package_count}",
                manifest.name, manifest.version, manifest.trust_level
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "published": true,
                    "production": production,
                    "name": manifest.name,
                    "version": manifest.version,
                    "package_hash": hash,
                    "trust": manifest.trust_level.to_string(),
                    "signature_status": "signed",
                    "log_id": published.log_id,
                    "sequence": published.sequence,
                    "preflight": "passed",
                    "production_lint": production_lint,
                    "provenance_integrity": provenance_integrity,
                    "verification_evidence_integrity": verification_evidence_integrity,
                    "production_issue_count": production_issues.len(),
                    "production_issues": production_issues
                        .iter()
                        .map(package_manifest_issue_to_json)
                        .collect::<Vec<_>>(),
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
            let assumption_issue_count = issues
                .iter()
                .filter(|issue| issue.kind == "assumption")
                .count();
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
            let mut packages_missing_assumptions = Vec::new();
            let mut missing_assumption_lines = Vec::new();
            for entry in &lockfile.entries {
                let Some(manifest) = registry.lookup_by_name_version(&entry.name, &entry.version)
                else {
                    continue;
                };
                let missing_assumptions =
                    missing_package_assumption_ids(manifest, &entry.accepted_assumptions);
                if missing_assumptions.is_empty() {
                    continue;
                }
                missing_assumption_lines.push(format!(
                    "{}@{}: {}",
                    entry.name,
                    entry.version,
                    missing_assumptions.join(", ")
                ));
                packages_missing_assumptions.push(json!({
                    "name": &entry.name,
                    "version": &entry.version,
                    "missing_assumptions": missing_assumptions,
                }));
            }
            let mut human_msg = if issue_lines.is_empty() {
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
            if !missing_assumption_lines.is_empty() {
                human_msg.push_str(&format!(
                    "\nremediation: run `ail package accept-assumption` for {} package(s): {}",
                    missing_assumption_lines.len(),
                    missing_assumption_lines.join("; ")
                ));
            }
            let unsafe_surface_json = lockfile
                .entries
                .iter()
                .flat_map(|entry| {
                    registry
                        .lookup_by_name_version(&entry.name, &entry.version)
                        .into_iter()
                        .flat_map(move |manifest| {
                            manifest.unsafe_surface.iter().map(move |surface| {
                                json!({
                                    "package": &entry.name,
                                    "version": &entry.version,
                                    "kind": &surface.kind,
                                    "name": &surface.name,
                                    "description": &surface.description,
                                })
                            })
                        })
                })
                .collect::<Vec<_>>();
            let audited_packages = lockfile
                .entries
                .iter()
                .map(|entry| {
                    let manifest = registry.lookup_by_name_version(&entry.name, &entry.version);
                    let accepted_assumptions = entry.accepted_assumptions.clone();
                    let missing_assumptions = manifest
                        .map(|manifest| {
                            missing_package_assumption_ids(manifest, &accepted_assumptions)
                        })
                        .unwrap_or_default();
                    let assumptions_valid = missing_assumptions.is_empty();
                    let package_issues = issues
                        .iter()
                        .filter(|issue| issue.package == entry.name && issue.version == entry.version)
                        .collect::<Vec<_>>();
                    let package_blocked = package_issues
                        .iter()
                        .filter(|issue| issue.status == "blocked")
                        .count();
                    let package_warnings = package_issues
                        .iter()
                        .filter(|issue| issue.status == "warning")
                        .count();
                    json!({
                        "name": &entry.name,
                        "version": &entry.version,
                        "trust": entry.trust_level.to_string(),
                        "manifest_present": manifest.is_some(),
                        "capabilities": manifest
                            .map(|manifest| manifest.required_capabilities.clone())
                            .unwrap_or_default(),
                        "exported_capabilities": manifest
                            .map(|manifest| manifest.exported_capabilities.clone())
                            .unwrap_or_default(),
                        "assumptions_count": manifest.map(|manifest| manifest.assumptions.len()).unwrap_or(0),
                        "accepted_assumptions": accepted_assumptions,
                        "missing_assumptions": missing_assumptions,
                        "assumptions_valid": assumptions_valid,
                        "unsafe_surface_count": manifest
                            .map(|manifest| manifest.unsafe_surface.len())
                            .unwrap_or(0),
                        "risk_status": if package_blocked > 0 {
                            "blocked"
                        } else if package_warnings > 0 {
                            "warning"
                        } else {
                            "clean"
                        },
                        "blocked_issues": package_blocked,
                        "warning_issues": package_warnings,
                    })
                })
                .collect::<Vec<_>>();
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
                    "packages": audited_packages,
                    "packages_checked": packages_checked,
                    "assumptions_valid": assumption_issue_count == 0,
                    "packages_missing_assumptions": packages_missing_assumptions,
                    "unsafe_surface": unsafe_surface_json,
                    "summary": {
                        "packages_checked": packages_checked,
                        "issues": issues.len(),
                        "advisories": advisory_count,
                        "yanked": yanked_count,
                        "assumptions": assumption_issue_count,
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
            let (registry, advisories) = load_package_registry_with_advisories(store)?;
            let lookup = trusted_package_lookup(&registry, name, version)?;
            let manifest = lookup.manifest;
            let lockfile = load_package_lockfile(store)?;
            let accepted_assumptions = lockfile
                .entries
                .iter()
                .find(|entry| entry.name == manifest.name && entry.version == manifest.version)
                .map(|entry| entry.accepted_assumptions.clone())
                .unwrap_or_default();
            let missing_assumptions = manifest
                .assumptions
                .iter()
                .filter(|assumption| !accepted_assumptions.contains(&assumption.id))
                .map(|assumption| assumption.id.clone())
                .collect::<Vec<_>>();
            let assumptions_valid = missing_assumptions.is_empty();
            let risk_issues = package_risk_issues_for_manifest(&registry, &advisories, &manifest);
            let warnings = lookup.warning.into_iter().collect::<Vec<_>>();
            let verification_report_status =
                verification_report_status(manifest.verification_report.is_some());
            let human_msg = format!(
                "package: {package}\nname: {}\nversion: {}\ntrust: {:?}\nsignature: {}\nverification_report: {verification_report_status}\ncapabilities: {}\nexported_capabilities: {}\nassumptions: {}\naccepted_assumptions: {}\nmissing_assumptions: {}\nassumptions_valid: {}\nunsafe_surface: {}\nadvisories: {}{}",
                manifest.name,
                manifest.version,
                manifest.trust_level,
                lookup.signature_status,
                format_package_string_list(&manifest.required_capabilities),
                format_package_string_list(&manifest.exported_capabilities),
                format_package_assumptions(&manifest),
                format_package_string_list(&accepted_assumptions),
                format_package_string_list(&missing_assumptions),
                assumptions_valid,
                format_package_unsafe_surface(&manifest),
                format_package_advisories(&risk_issues),
                format_warnings_for_human(&warnings)
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "package": package,
                    "name": &manifest.name,
                    "version": &manifest.version,
                    "trust": manifest.trust_level.to_string(),
                    "signature_status": lookup.signature_status,
                    "verification_report": &manifest.verification_report,
                    "verification_report_status": verification_report_status,
                    "capabilities": &manifest.required_capabilities,
                    "exported_capabilities": &manifest.exported_capabilities,
                    "assumptions": &manifest.assumptions,
                    "accepted_assumptions": accepted_assumptions,
                    "missing_assumptions": missing_assumptions,
                    "assumptions_valid": assumptions_valid,
                    "unsafe_surface": &manifest.unsafe_surface,
                    "advisories": risk_issues.iter().map(PackageAuditIssue::to_json).collect::<Vec<_>>(),
                    "warnings": warnings,
                }),
            );
        }
    }
    Ok(())
}
