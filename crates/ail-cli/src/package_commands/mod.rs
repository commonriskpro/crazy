// ail-cli::package_commands ------------------------------------------------
//
// Handlers and business logic for the `ail package` command surface.
//
// Dispatch entry point: `cmd_package`. Sub-command helpers are split by
// responsibility so package install, verify, audit, compatibility, manifest,
// and parsing logic each have one clear reason to change.
// Pure I/O lives in `package_registry_io`. Output formatting lives in
// `package_output`.

use std::collections::{BTreeMap, BTreeSet};

use ail_compiler::ArtifactManifest;
use ail_core::semantic_graph::NodeKind;
use ail_package::{
    AdvisoryChecker, ArtifactHashEntry, CompatibilityEngine, CompatibilityError,
    LocalCompatibilityIssue, LocalCompatibilityIssueKind, LockfileArtifactEvidence, LockfileEntry,
    PackageCompatibilityMetadata, PackageDef, PackageKeypair, PackageManifest, PackageRegistry,
    PublishRequest, RegistryClient, SecurityAdvisory, TrustLevel,
};
use serde_json::json;

use crate::cli::{AdvisoryCmd, PackageCmd, bytes_to_hex, load_current_graph_for_cli};
use crate::error::CliError;
use crate::output::{OutputMode, print_error_response, print_response};
use crate::package_output::{
    InstalledPackage, LockfileReproducibilityCliIssue, PackageAuditIssue,
    PackageCompatibilityCliIssue, PackageInstallFailure, PackageInstallResult,
    RegistryPackageIntegrity, VerificationReportHashMismatch, advisory_to_json,
    emit_package_compatibility_blocked, emit_package_install_failure, format_warnings_for_human,
    lockfile_entry_to_json, package_compatibility_issue_to_json, package_manifest_to_json,
    reproducible_evidence_status, verification_report_hash_mismatch_to_json,
    verification_report_status, yank_to_json,
};
use crate::package_registry_io::{
    LocalRegistryClient, load_local_package_registry_file_for_read,
    load_local_package_registry_file_for_update, load_package_lockfile, load_package_registry,
    load_package_registry_with_advisories, load_package_registry_with_compatibility,
    package_manifest_path, save_local_package_registry_file, save_package_lockfile,
    save_package_manifest, save_package_registry, trusted_package_lookup,
};
use crate::store::StoreHandle;

mod audit;
mod compat;
mod dispatch;
mod install;
mod manifest;
mod parse;
mod verify;

use audit::audit_package_lockfile;
use compat::{package_compatibility_issues_for_install, package_compatibility_issues_for_verify};
pub(crate) use dispatch::cmd_package;
use install::install_package_from_registry;
use manifest::load_or_create_package_manifest;
pub(crate) use manifest::package_manifest_for_current_graph;
use parse::{
    parse_advisory_severity, parse_package_spec, validate_required_package_metadata_field,
};
use verify::{verification_report_hash_for_manifest, verification_report_hash_mismatches};
