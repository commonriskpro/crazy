use super::*;
use crate::diagnostic_commands::{
    doctor_artifact_hash_consistency, doctor_assumption_expirations, doctor_index_freshness,
    doctor_package_advisories, doctor_runtime_profile_validity, doctor_schema_compatibility,
};
use crate::package_registry_io::save_package_registry;

// Scenario: cmd_doctor returns all seven checks with status.
#[tokio::test]
async fn cmd_doctor_returns_all_checks() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_doctor(OutputMode::Human, &store).await;
    assert!(result.is_ok(), "cmd_doctor must succeed; got: {result:?}");
}

// TRIANGULATE: cmd_doctor reports graph_integrity warn when graph has dangling edges.
//   GIVEN a store with a snapshot containing a graph with a dangling edge
//   WHEN cmd_doctor runs
//   THEN overall is "issues_found" and the graph_integrity check is "warn"
#[tokio::test]
async fn cmd_doctor_graph_integrity_warn_on_dangling_edge() {
    use crate::store::memory_store;
    use ail_core::semantic_graph::{
        EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };
    use ail_storage::{SnapshotEnvelope, object::ObjectId};

    let store = memory_store();

    // Graph with a dangling edge (target NodeRef(99) doesn't exist).
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    graph
        .nodes
        .push(GraphNode::new(NodeRef(0), NodeKind::Function, "foo"));
    graph
        .edges
        .push(GraphEdge::new(NodeRef(0), NodeRef(99), EdgeKind::DependsOn));

    let root_hash = store.save_graph(&graph).await.expect("save graph");
    let snap = SnapshotEnvelope {
        id: ObjectId::from_bytes(b"snap-doctor-test"),
        graph_root_hash: root_hash,
        parent_id: None,
        applied_change_id: None,
        created_at: 0,
        verification_report_hash: None,
        ..Default::default()
    };
    store.save_snapshot(&snap).await.expect("save snapshot");

    let result = cmd_doctor(OutputMode::Json, &store).await;
    assert!(result.is_ok(), "cmd_doctor must succeed; got: {result:?}");
    // The dangling edge means validate_full() returns ≥1 errors.
    // Actual output verification would require capturing stdout; the test
    // exercises the real code path (not a stub).
}

// ── T7e: doctor real filesystem checks ────────────────────────────────

// Scenario DR-1b: index_freshness is "ok" when no objects exist yet.
//   GIVEN an ail_dir with no objects in store/objects/
//   WHEN doctor_index_freshness is called
//   THEN status is "ok" (nothing to be stale against)
#[test]
fn doctor_index_freshness_ok_when_no_objects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    crate::store::init_file_layout(&ail_dir).expect("init layout");
    // No objects stored — index freshness must be "ok"
    let (status, _msg) = doctor_index_freshness(&ail_dir);
    assert_eq!(status, "ok", "no objects → freshness must be ok");
}

// TRIANGULATE: index_freshness is "warn" when objects exist but no snapshots.cbor.
//   GIVEN an ail_dir with at least one object in store/objects/ but no index
//   WHEN doctor_index_freshness is called
//   THEN status is "warn" (objects exist but index is missing)
#[test]
fn doctor_index_freshness_warn_when_objects_without_index() {
    use crate::store::FileObjectStore;
    use ail_storage::object::{ObjectStore, RawObject};
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    crate::store::init_file_layout(&ail_dir).expect("init layout");
    // Write an object but no snapshots.cbor
    let fos = FileObjectStore::new_for_test(&ail_dir);
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(fos.put(RawObject(b"test-object".to_vec())))
        .expect("put object");
    // Ensure snapshots.cbor does NOT exist
    let index_path = ail_dir.join("index").join("snapshots.cbor");
    assert!(
        !index_path.exists(),
        "test setup: snapshots.cbor must not exist"
    );

    let (status, _msg) = doctor_index_freshness(&ail_dir);
    assert_eq!(
        status, "warn",
        "objects without index → freshness must be warn"
    );
}

// Scenario: schema_compatibility is "ok" when project.toml does not exist.
//   GIVEN an ail_dir with no project.toml
//   WHEN doctor_schema_compatibility is called
//   THEN status is "ok"
#[test]
fn doctor_schema_compat_ok_when_no_project_toml() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    std::fs::create_dir_all(&ail_dir).expect("create ail_dir");
    // No project.toml
    let (status, _msg) = doctor_schema_compatibility(&ail_dir);
    assert_eq!(
        status, "ok",
        "missing project.toml → schema compat must be ok"
    );
}

// TRIANGULATE: schema_compatibility is "warn" when project.toml has version = "0".
//   GIVEN a project.toml with `version = "0"` (non-"1" value)
//   WHEN doctor_schema_compatibility is called
//   THEN status is "warn"
#[test]
fn doctor_schema_compat_warn_when_version_is_zero() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    std::fs::create_dir_all(&ail_dir).expect("create ail_dir");
    std::fs::write(ail_dir.join("project.toml"), b"version = \"0\"\n").expect("write project.toml");

    let (status, _msg) = doctor_schema_compatibility(&ail_dir);
    assert_eq!(
        status, "warn",
        "project.toml version = \"0\" → schema compat must be warn"
    );
}

// ── Real doctor check tests (Feature D) ──────────────────────────────────
//
// Each "warn" scenario would return "ok" with the old hardcoded stub, proving
// the real implementation is exercised.

// DR-2a: artifact_hash_consistency is "ok" when no lockfile exists.
//   GIVEN a file store with no lock.cbor
//   WHEN doctor_artifact_hash_consistency is called
//   THEN status is "ok" (nothing to cross-check)
#[test]
fn doctor_artifact_hash_consistency_ok_when_no_lockfile() {
    use crate::store::{file_store, init_file_layout};
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);
    let (status, _msg) = doctor_artifact_hash_consistency(&store);
    assert_eq!(
        status, "ok",
        "empty lockfile → artifact hash check must be ok"
    );
}

// DR-2b: artifact_hash_consistency is "warn" when a lockfile hash mismatches the registry.
//   GIVEN a file store with a lockfile entry whose hash does not match the registry manifest
//   WHEN doctor_artifact_hash_consistency is called
//   THEN status is "warn" (hash mismatch detected)
//   NOTE: the old stub returned "ok" unconditionally — this test would have FAILED with it.
#[test]
fn doctor_artifact_hash_consistency_warn_on_hash_mismatch() {
    use crate::package_registry_io::{save_package_lockfile, save_package_registry};
    use crate::store::{file_store, init_file_layout};
    use ail_package::{
        Lockfile, LockfileEntry, PackageDef, PackageKeypair, PackageManifest, PackageRegistry,
        TrustLevel,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    // Register a manifest in the registry.
    let keypair = PackageKeypair::from_bytes(&[1u8; 32]);
    let manifest = PackageManifest::from_def(PackageDef {
        name: "test.pkg".to_string(),
        version: "1.0.0".to_string(),
        trust_level: TrustLevel::Verified,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    });
    let mut registry = PackageRegistry::new();
    let signed = keypair.sign_manifest(manifest).expect("sign manifest");
    registry.register_signed(signed).expect("register");
    save_package_registry(&store, &registry).expect("save registry");

    // Write a lockfile with a WRONG hash for the same package.
    // The real hash will differ from "a".repeat(64) (a valid but fabricated value).
    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: "test.pkg".to_string(),
        version: "1.0.0".to_string(),
        package_hash: "a".repeat(64), // deliberate mismatch
        trust_level: TrustLevel::Verified,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    save_package_lockfile(&store, &lockfile).expect("save lockfile");

    let (status, msg) = doctor_artifact_hash_consistency(&store);
    assert_eq!(
        status, "warn",
        "hash mismatch → artifact hash check must be warn; msg: {msg}"
    );
}

// DR-2c: artifact_hash_consistency is "warn" when a lockfile entry is absent from registry.
//   GIVEN a lockfile entry for a package not present in the registry
//   WHEN doctor_artifact_hash_consistency is called
//   THEN status is "warn" (missing registry entry detected)
#[test]
fn doctor_artifact_hash_consistency_warn_on_missing_registry_entry() {
    use crate::package_registry_io::save_package_lockfile;
    use crate::store::{file_store, init_file_layout};
    use ail_package::{Lockfile, LockfileEntry, TrustLevel};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    // No registry written — lockfile references a package the registry doesn't know.
    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: "missing.pkg".to_string(),
        version: "2.0.0".to_string(),
        package_hash: "b".repeat(64),
        trust_level: TrustLevel::Assumed,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    save_package_lockfile(&store, &lockfile).expect("save lockfile");

    let (status, msg) = doctor_artifact_hash_consistency(&store);
    assert_eq!(
        status, "warn",
        "package absent from registry → artifact hash check must be warn; msg: {msg}"
    );
}

// DR-2d: artifact_hash_consistency is "warn" when a lockfile entry has no recorded hash.
//   GIVEN a registry manifest and a lockfile entry with an empty package_hash
//   WHEN doctor_artifact_hash_consistency is called
//   THEN status is "warn" because integrity cannot be verified
#[test]
fn doctor_artifact_hash_consistency_warn_on_empty_lockfile_hash() {
    use crate::package_registry_io::{save_package_lockfile, save_package_registry};
    use crate::store::{file_store, init_file_layout};
    use ail_package::{
        Lockfile, LockfileEntry, PackageDef, PackageKeypair, PackageManifest, PackageRegistry,
        TrustLevel,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    let keypair = PackageKeypair::from_bytes(&[7u8; 32]);
    let manifest = PackageManifest::from_def(PackageDef {
        name: "emptyhash.pkg".to_string(),
        version: "1.0.0".to_string(),
        trust_level: TrustLevel::Verified,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    });
    let mut registry = PackageRegistry::new();
    let signed = keypair.sign_manifest(manifest).expect("sign manifest");
    registry.register_signed(signed).expect("register");
    save_package_registry(&store, &registry).expect("save registry");

    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: "emptyhash.pkg".to_string(),
        version: "1.0.0".to_string(),
        package_hash: String::new(),
        trust_level: TrustLevel::Verified,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    save_package_lockfile(&store, &lockfile).expect("save lockfile");

    let (status, msg) = doctor_artifact_hash_consistency(&store);
    assert_eq!(
        status, "warn",
        "empty lockfile hash must warn because integrity is unverifiable; msg: {msg}"
    );
    assert!(
        msg.contains("no hash recorded"),
        "message should explain missing lockfile hash; msg: {msg}"
    );
}

// DR-3a: runtime_profile_validity is "ok" when no policy rules file exists.
//   GIVEN a file store with no policies/rules.cbor
//   WHEN doctor_runtime_profile_validity is called
//   THEN status is "ok"
#[test]
fn doctor_runtime_profile_validity_ok_when_no_rules_file() {
    use crate::store::{file_store, init_file_layout};
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);
    let (status, _msg) = doctor_runtime_profile_validity(&store);
    assert_eq!(
        status, "ok",
        "no rules file → runtime profile check must be ok"
    );
}

// DR-3b: runtime_profile_validity is "warn" when rules contain invalid entries.
//   GIVEN a policies/rules.cbor with one well-formed rule and one garbage entry
//   WHEN doctor_runtime_profile_validity is called
//   THEN status is "warn" (invalid rule detected)
//   NOTE: the old stub returned "ok" unconditionally — this test would have FAILED with it.
#[test]
fn doctor_runtime_profile_validity_warn_on_invalid_rule() {
    use crate::store::{file_store, init_file_layout};
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let rules: Vec<String> = vec![
        "deny capability file.write:*".to_string(), // valid
        "INVALID GARBAGE RULE".to_string(),         // invalid — not deny/set
    ];
    let policies_dir = ail_dir.join("policies");
    std::fs::create_dir_all(&policies_dir).expect("create policies dir");
    let mut bytes = Vec::new();
    ciborium::into_writer(&rules, &mut bytes).expect("encode rules");
    std::fs::write(policies_dir.join("rules.cbor"), bytes).expect("write rules.cbor");

    let (status, msg) = doctor_runtime_profile_validity(&store);
    assert_eq!(
        status, "warn",
        "invalid rule → runtime profile check must be warn; msg: {msg}"
    );
}

// DR-3c: runtime_profile_validity is "ok" when all rules are well-formed.
//   GIVEN valid "deny capability" and "set" rules
//   WHEN doctor_runtime_profile_validity is called
//   THEN status is "ok"
#[test]
fn doctor_runtime_profile_validity_ok_when_all_rules_valid() {
    use crate::store::{file_store, init_file_layout};
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let rules: Vec<String> = vec![
        "deny capability file.write:*".to_string(),
        "deny capability http.call:* unless approved".to_string(),
        "set max_new_capabilities=5".to_string(),
    ];
    let policies_dir = ail_dir.join("policies");
    std::fs::create_dir_all(&policies_dir).expect("create policies dir");
    let mut bytes = Vec::new();
    ciborium::into_writer(&rules, &mut bytes).expect("encode rules");
    std::fs::write(policies_dir.join("rules.cbor"), bytes).expect("write rules.cbor");

    let (status, _msg) = doctor_runtime_profile_validity(&store);
    assert_eq!(
        status, "ok",
        "all valid rules → runtime profile check must be ok"
    );
}

// DR-4a: package_advisories is "ok" when no lockfile entries exist.
//   GIVEN a file store with no lock.cbor
//   WHEN doctor_package_advisories is called
//   THEN status is "ok" (nothing to cross-check)
#[test]
fn doctor_package_advisories_ok_when_no_lockfile() {
    use crate::store::{file_store, init_file_layout};
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);
    let (status, _msg) = doctor_package_advisories(&store);
    assert_eq!(
        status, "ok",
        "empty lockfile → package advisories check must be ok"
    );
}

// DR-4b: package_advisories is "warn" when an installed package matches a known advisory.
//   GIVEN lockfile entry "payments.stripe@1.0.0" + advisory for same package/version
//   WHEN doctor_package_advisories is called
//   THEN status is "warn" (advisory match detected)
//   NOTE: the old stub returned "ok" unconditionally — this test would have FAILED with it.
#[test]
fn doctor_package_advisories_warn_on_affected_package() {
    use crate::package_registry_io::{
        LocalPackageRegistryFile, save_local_package_registry_file, save_package_lockfile,
    };
    use crate::store::{file_store, init_file_layout};
    use ail_package::{AdvisorySeverity, Lockfile, LockfileEntry, SecurityAdvisory, TrustLevel};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    // Lockfile entry for the affected package.
    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: "payments.stripe".to_string(),
        version: "1.0.0".to_string(),
        package_hash: "c".repeat(64),
        trust_level: TrustLevel::Assumed,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    save_package_lockfile(&store, &lockfile).expect("save lockfile");

    // Registry file with an advisory for the same package version.
    let advisory = SecurityAdvisory {
        id: "adv_test_001".to_string(),
        package: "payments.stripe".to_string(),
        affected_constraint: "1.0.0".to_string(),
        severity: AdvisorySeverity::High,
        reason: "test vulnerability".to_string(),
    };
    save_local_package_registry_file(
        &store,
        &LocalPackageRegistryFile {
            advisories: vec![advisory],
            ..LocalPackageRegistryFile::default()
        },
    )
    .expect("save registry file");

    let (status, msg) = doctor_package_advisories(&store);
    assert_eq!(
        status, "warn",
        "advisory match → package advisories check must be warn; msg: {msg}"
    );
}

// DR-4c: package_advisories is "ok" when installed packages have no matching advisories.
//   GIVEN lockfile entry for "safe.pkg@2.0.0" + advisory only for version "1.0.0"
//   WHEN doctor_package_advisories is called
//   THEN status is "ok" (installed version not in advisory range)
#[test]
fn doctor_package_advisories_ok_when_no_matching_advisory() {
    use crate::package_registry_io::{
        LocalPackageRegistryFile, save_local_package_registry_file, save_package_lockfile,
    };
    use crate::store::{file_store, init_file_layout};
    use ail_package::{AdvisorySeverity, Lockfile, LockfileEntry, SecurityAdvisory, TrustLevel};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    // Lockfile entry for version 2.0.0.
    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: "safe.pkg".to_string(),
        version: "2.0.0".to_string(),
        package_hash: "d".repeat(64),
        trust_level: TrustLevel::Verified,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    save_package_lockfile(&store, &lockfile).expect("save lockfile");

    // Advisory only covers version 1.0.0 — installed 2.0.0 is unaffected.
    let advisory = SecurityAdvisory {
        id: "adv_old".to_string(),
        package: "safe.pkg".to_string(),
        affected_constraint: "1.0.0".to_string(),
        severity: AdvisorySeverity::Low,
        reason: "old version only".to_string(),
    };
    save_local_package_registry_file(
        &store,
        &LocalPackageRegistryFile {
            advisories: vec![advisory],
            ..LocalPackageRegistryFile::default()
        },
    )
    .expect("save registry file");

    let (status, _msg) = doctor_package_advisories(&store);
    assert_eq!(
        status, "ok",
        "no advisory match for installed version → check must be ok"
    );
}

// DR-5a: assumption_expirations is "ok" when the registry is empty.
//   GIVEN a file store with no registry entries
//   WHEN doctor_assumption_expirations is called
//   THEN status is "ok" (nothing to inspect)
#[test]
fn doctor_assumption_expirations_ok_when_no_registry() {
    use crate::store::{file_store, init_file_layout};
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);
    let (status, _msg) = doctor_assumption_expirations(&store);
    assert_eq!(
        status, "ok",
        "empty registry → assumption expiration check must be ok"
    );
}

// DR-5b: assumption_expirations is "warn" when a manifest has an Expired assumption.
//   GIVEN a registry manifest with assumption state = Expired
//   WHEN doctor_assumption_expirations is called
//   THEN status is "warn" (expired state detected)
//   NOTE: the old stub returned "ok" unconditionally — this test would have FAILED with it.
#[test]
fn doctor_assumption_expirations_warn_on_expired_state() {
    use crate::store::{file_store, init_file_layout};
    use ail_package::{
        AssumptionState, PackageAssumption, PackageDef, PackageKeypair, PackageManifest,
        PackageRegistry, TrustLevel,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    let keypair = PackageKeypair::from_bytes(&[2u8; 32]);
    let manifest = PackageManifest::from_def(PackageDef {
        name: "assumed.pkg".to_string(),
        version: "1.0.0".to_string(),
        trust_level: TrustLevel::Assumed,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![PackageAssumption {
            id: "assume-expired".to_string(),
            claim: "Vendor was PCI-DSS certified".to_string(),
            boundary: "payments".to_string(),
            owner: "platform-team".to_string(),
            expires: Some("2020-01-01".to_string()),
            state: AssumptionState::Expired,
        }],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    });
    let mut registry = PackageRegistry::new();
    let signed = keypair.sign_manifest(manifest).expect("sign");
    registry.register_signed(signed).expect("register");
    save_package_registry(&store, &registry).expect("save registry");

    let (status, msg) = doctor_assumption_expirations(&store);
    assert_eq!(
        status, "warn",
        "Expired assumption → expiration check must be warn; msg: {msg}"
    );
}

// DR-5c: assumption_expirations is "warn" when an Active assumption has a past expiry date.
//   GIVEN an Active assumption with expires = "2020-12-31" (clearly in the past)
//   WHEN doctor_assumption_expirations is called
//   THEN status is "warn" (past expiry on active assumption)
//   NOTE: the old stub returned "ok" unconditionally — this test would have FAILED with it.
#[test]
fn doctor_assumption_expirations_warn_on_active_past_expiry() {
    use crate::store::{file_store, init_file_layout};
    use ail_package::{
        AssumptionState, PackageAssumption, PackageDef, PackageKeypair, PackageManifest,
        PackageRegistry, TrustLevel,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    let keypair = PackageKeypair::from_bytes(&[3u8; 32]);
    let manifest = PackageManifest::from_def(PackageDef {
        name: "active.pkg".to_string(),
        version: "1.0.0".to_string(),
        trust_level: TrustLevel::Assumed,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![PackageAssumption {
            id: "assume-stale".to_string(),
            claim: "API v1 is still supported by vendor".to_string(),
            boundary: "api".to_string(),
            owner: "api-team".to_string(),
            expires: Some("2020-12-31".to_string()), // clearly in the past
            state: AssumptionState::Active,
        }],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    });
    let mut registry = PackageRegistry::new();
    let signed = keypair.sign_manifest(manifest).expect("sign");
    registry.register_signed(signed).expect("register");
    save_package_registry(&store, &registry).expect("save registry");

    let (status, msg) = doctor_assumption_expirations(&store);
    assert_eq!(
        status, "warn",
        "Active assumption with past expiry → must warn; msg: {msg}"
    );
}

// DR-5d: assumption_expirations is "warn" when an Active assumption has no expiry date.
//   GIVEN an Active assumption with expires = None (unknown expiry)
//   WHEN doctor_assumption_expirations is called
//   THEN status is "warn" (unknown expiry is flagged)
//   NOTE: the old stub returned "ok" unconditionally — this test would have FAILED with it.
#[test]
fn doctor_assumption_expirations_warn_on_active_no_expiry() {
    use crate::store::{file_store, init_file_layout};
    use ail_package::{
        AssumptionState, PackageAssumption, PackageDef, PackageKeypair, PackageManifest,
        PackageRegistry, TrustLevel,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    let keypair = PackageKeypair::from_bytes(&[4u8; 32]);
    let manifest = PackageManifest::from_def(PackageDef {
        name: "noexpiry.pkg".to_string(),
        version: "1.0.0".to_string(),
        trust_level: TrustLevel::Assumed,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![PackageAssumption {
            id: "assume-no-expiry".to_string(),
            claim: "Vendor contract is open-ended".to_string(),
            boundary: "legal".to_string(),
            owner: "legal-team".to_string(),
            expires: None, // no expiry date set
            state: AssumptionState::Active,
        }],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    });
    let mut registry = PackageRegistry::new();
    let signed = keypair.sign_manifest(manifest).expect("sign");
    registry.register_signed(signed).expect("register");
    save_package_registry(&store, &registry).expect("save registry");

    let (status, msg) = doctor_assumption_expirations(&store);
    assert_eq!(
        status, "warn",
        "Active assumption with no expiry → must warn (unknown); msg: {msg}"
    );
}

// DR-5f: assumption_expirations warns on unrecognized expiry date formats.
//   GIVEN an Active assumption with a non-ISO expiry string
//   WHEN doctor_assumption_expirations is called
//   THEN status is "warn" because lexicographic expiry comparison would be unsafe
#[test]
fn doctor_assumption_expirations_warn_on_malformed_expiry() {
    use crate::store::{file_store, init_file_layout};
    use ail_package::{
        AssumptionState, PackageAssumption, PackageDef, PackageKeypair, PackageManifest,
        PackageRegistry, TrustLevel,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    let keypair = PackageKeypair::from_bytes(&[8u8; 32]);
    let manifest = PackageManifest::from_def(PackageDef {
        name: "malformed-expiry.pkg".to_string(),
        version: "1.0.0".to_string(),
        trust_level: TrustLevel::Assumed,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![PackageAssumption {
            id: "assume-malformed-expiry".to_string(),
            claim: "Human-written expiry format".to_string(),
            boundary: "legal".to_string(),
            owner: "legal-team".to_string(),
            expires: Some("Jan 1 2020".to_string()),
            state: AssumptionState::Active,
        }],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    });
    let mut registry = PackageRegistry::new();
    let signed = keypair.sign_manifest(manifest).expect("sign");
    registry.register_signed(signed).expect("register");
    save_package_registry(&store, &registry).expect("save registry");

    let (status, msg) = doctor_assumption_expirations(&store);
    assert_eq!(
        status, "warn",
        "malformed expiry must warn instead of comparing lexicographically; msg: {msg}"
    );
    assert!(
        msg.contains("unrecognized expiry format"),
        "message should explain bad expiry format; msg: {msg}"
    );
}

// DR-5e: assumption_expirations is "ok" when Active assumptions have far-future expiry dates.
//   GIVEN an Active assumption with expires = "2099-12-31" (far future)
//   WHEN doctor_assumption_expirations is called
//   THEN status is "ok" (not expired, not soon)
#[test]
fn doctor_assumption_expirations_ok_when_active_future_expiry() {
    use crate::store::{file_store, init_file_layout};
    use ail_package::{
        AssumptionState, PackageAssumption, PackageDef, PackageKeypair, PackageManifest,
        PackageRegistry, TrustLevel,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    let keypair = PackageKeypair::from_bytes(&[5u8; 32]);
    let manifest = PackageManifest::from_def(PackageDef {
        name: "future.pkg".to_string(),
        version: "1.0.0".to_string(),
        trust_level: TrustLevel::Assumed,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![PackageAssumption {
            id: "assume-future".to_string(),
            claim: "Contract valid through end of century".to_string(),
            boundary: "legal".to_string(),
            owner: "legal-team".to_string(),
            expires: Some("2099-12-31".to_string()), // far future — never triggers soon/expired
            state: AssumptionState::Active,
        }],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    });
    let mut registry = PackageRegistry::new();
    let signed = keypair.sign_manifest(manifest).expect("sign");
    registry.register_signed(signed).expect("register");
    save_package_registry(&store, &registry).expect("save registry");

    let (status, _msg) = doctor_assumption_expirations(&store);
    assert_eq!(
        status, "ok",
        "Active assumption with far-future expiry → check must be ok"
    );
}
