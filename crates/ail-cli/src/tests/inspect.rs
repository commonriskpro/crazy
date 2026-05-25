use super::*;

// Scenario: cmd_inspect node returns edges/effects/capabilities/contracts.
#[tokio::test]
async fn cmd_inspect_node_returns_metadata() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_inspect(OutputMode::Human, "node", "fn.answer", &store).await;
    assert!(result.is_ok(), "inspect node must succeed; got: {result:?}");
}

// Scenario: cmd_inspect report returns status/entries/diagnostics.
#[tokio::test]
async fn cmd_inspect_report_returns_metadata() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_inspect(OutputMode::Human, "report", "ver_123", &store).await;
    assert!(
        result.is_ok(),
        "inspect report must succeed; got: {result:?}"
    );
}

// Scenario: cmd_inspect artifact returns name/hash/profile.
#[tokio::test]
async fn cmd_inspect_artifact_returns_metadata() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_inspect(OutputMode::Human, "artifact", "checkout.wasm", &store).await;
    assert!(
        result.is_ok(),
        "inspect artifact must succeed; got: {result:?}"
    );
}

// Scenario: cmd_inspect capability returns NotFound for unknown capability.
// NOTE: old stub returned Ok unconditionally — this test FAILS with the stub.
#[tokio::test]
async fn cmd_inspect_capability_unknown_returns_not_found() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_inspect(
        OutputMode::Human,
        "capability",
        "payment.charge:PaymentProvider",
        &store,
    )
    .await;
    assert!(
        matches!(result, Err(CliError::NotFound(_))),
        "inspect capability for unknown cap must return NotFound; got: {result:?}"
    );
}

// Scenario: cmd_inspect capability returns real data for a registered package capability.
// NOTE: old stub always returned Ok with granted:false — this tests real registry lookup.
#[tokio::test]
async fn cmd_inspect_capability_found_in_registry() {
    use crate::package_registry_io::save_package_registry;
    use crate::store::{file_store, init_file_layout};
    use ail_package::{PackageDef, PackageKeypair, PackageManifest, PackageRegistry, TrustLevel};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    // Register a package that exports "http.call".
    let keypair = PackageKeypair::from_bytes(&[9u8; 32]);
    let manifest = PackageManifest::from_def(PackageDef {
        name: "net.http".to_string(),
        version: "1.0.0".to_string(),
        trust_level: TrustLevel::Assumed,
        required_capabilities: vec![],
        exported_capabilities: vec!["http.call".to_string()],
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
    let signed = keypair.sign_manifest(manifest).expect("sign");
    registry.register_signed(signed).expect("register");
    save_package_registry(&store, &registry).expect("save registry");

    let result = cmd_inspect(OutputMode::Human, "capability", "http.call", &store).await;
    assert!(
        result.is_ok(),
        "inspect capability for registered cap must succeed; got: {result:?}"
    );
}

// Scenario: cmd_inspect report returns real Checker data (not hardcoded "accepted").
// NOTE: old stub returned status:"accepted" — this test verifies the field is derived.
#[tokio::test]
async fn cmd_inspect_report_returns_checker_derived_entries() {
    use crate::store::memory_store;
    let store = memory_store();
    // Must succeed (real Checker runs on default graph).
    let result = cmd_inspect(OutputMode::Human, "report", "ver_123", &store).await;
    assert!(
        result.is_ok(),
        "inspect report must succeed with real checker; got: {result:?}"
    );
}

// Scenario: cmd_inspect artifact returns real compilation data (not null hash).
// NOTE: old stub returned hash:null — this test verifies compilation runs on demand.
#[tokio::test]
async fn cmd_inspect_artifact_compiles_on_demand() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_inspect(OutputMode::Human, "artifact", "program.wasm", &store).await;
    assert!(
        result.is_ok(),
        "inspect artifact must succeed with on-demand compile; got: {result:?}"
    );
}

// Scenario: cmd_inspect artifact prefers persisted artifact over on-demand compile.
//   GIVEN a file store where cmd_compile has been run
//   WHEN cmd_inspect artifact is called with the profile name
//   THEN the response succeeds (source will be persisted_artifact, not computed_on_demand)
#[tokio::test]
async fn cmd_inspect_artifact_prefers_persisted_over_on_demand() {
    use crate::store::{file_store, init_file_layout};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    // First compile so there is a persisted artifact.
    cmd_compile(OutputMode::Human, "dev", "wasm", &store)
        .await
        .expect("compile must succeed");

    // Now inspect — load_wasm_artifact matches profile "dev" for name "dev.wasm".
    let result = cmd_inspect(OutputMode::Human, "artifact", "dev.wasm", &store).await;
    assert!(
        result.is_ok(),
        "inspect artifact must succeed after compile; got: {result:?}"
    );
}

// Scenario: cmd_inspect artifact falls back to on-demand when no persisted data exists.
//   GIVEN a file store with NO prior compile
//   WHEN cmd_inspect artifact is called
//   THEN it succeeds via on-demand compilation
#[tokio::test]
async fn cmd_inspect_artifact_falls_back_to_on_demand_with_file_store() {
    use crate::store::{file_store, init_file_layout};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    let result = cmd_inspect(OutputMode::Human, "artifact", "checkout.wasm", &store).await;
    assert!(
        result.is_ok(),
        "inspect artifact must succeed via on-demand fallback; got: {result:?}"
    );
}

// Scenario: cmd_inspect artifact prefers persisted native artifact over on-demand compile.
//   GIVEN a file store where cmd_compile --target native has been run
//   WHEN cmd_inspect artifact is called with the profile name (no wasm artifact present)
//   THEN the response succeeds with source = persisted_native_artifact
#[tokio::test]
async fn cmd_inspect_artifact_prefers_persisted_native_over_on_demand() {
    use crate::store::{file_store, init_file_layout};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    // Compile native so there is a persisted native artifact (no WASM artifact).
    cmd_compile(OutputMode::Human, "dev", "native", &store)
        .await
        .expect("native compile must succeed");

    // Prove the persisted native branch is taken: load_native_artifact must return Some.
    // This is the exact condition cmd_inspect uses to choose the persisted_native_artifact
    // branch over on-demand compilation.
    let persisted = store
        .load_native_artifact("dev.o")
        .expect("load_native_artifact must not error")
        .expect("load_native_artifact must return Some after compile --target native");
    assert_eq!(
        persisted.target, "native",
        "persisted artifact target must be \"native\"; got: {:?}",
        persisted.target
    );

    // Inspect — load_wasm_artifact returns None, load_native_artifact matches "dev".
    let result = cmd_inspect(OutputMode::Human, "artifact", "dev.o", &store).await;
    assert!(
        result.is_ok(),
        "inspect artifact must succeed after native compile; got: {result:?}"
    );
}

// Scenario: cmd_inspect report surfaces verified_profile from file-backed sidecar.
//   GIVEN a file store with a report saved under a non-hex change-id with profile "prod"
//   WHEN cmd_inspect report is called with that change-id
//   THEN the call succeeds (verified_profile is surfaced, not discarded with _profile)
//
// Uses a non-hex change-id to exercise the sidecar branch directly, avoiding the
// pre-existing object-store collision where the changeset's CBOR bytes occupy the
// same content-addressed slot as the expected report hash.
#[tokio::test]
async fn cmd_inspect_report_surfaces_verified_profile_from_sidecar() {
    use crate::store::{file_store, init_file_layout};
    use ail_verify::report::VerificationReport;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");
    let store = file_store(ail_dir);

    // Save a report directly with profile "prod" under a non-hex change-id.
    let change_id = "change.inspect_profile_test";
    let report = VerificationReport::default();
    store
        .save_verification_report(change_id, "prod", &report)
        .await
        .expect("save report");

    // inspect report by non-hex change-id — must succeed (verified_profile surfaced, not dropped).
    let result = cmd_inspect(OutputMode::Human, "report", change_id, &store).await;
    assert!(
        result.is_ok(),
        "inspect report by change-id must succeed when sidecar contains verified_profile; got: {result:?}"
    );
}

// Scenario: cmd_inspect report succeeds when id is a 64-char hex change-id that
// also happens to be the BLAKE3 hash of a stored CanonicalChangeSet object.
//
// Regression guard for the latent `cmd_inspect report` bug:
//   The first hash-lookup branch calls `load_verification_report_by_hash(change_id)`.
//   When the changeset CBOR is stored at that content-address, CBOR decoding as
//   VerificationReport fails with "missing field entries".  The fix captures that
//   error and falls back to the sidecar index before propagating it.
//
// Setup:
//   1. Save a CanonicalChangeSet via save_changeset_payload(change_id, cbor_bytes).
//      The object store now holds the changeset CBOR at ObjectId == change_id.
//   2. Save a VerificationReport via save_verification_report(change_id, "prod", &report).
//      A sidecar at .ail/reports/<change_id> records the report hash + profile.
//   3. cmd_inspect report <change_id> must succeed and surface verified_profile.
#[tokio::test]
async fn cmd_inspect_report_succeeds_when_change_id_collides_with_changeset_hash() {
    use ail_change::canonical::CanonicalChangeSet;
    use ail_storage::object::ObjectId;
    use ail_verify::report::VerificationReport;

    use crate::store::{file_store, init_file_layout};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");
    let store = file_store(ail_dir);

    // Build a CanonicalChangeSet and derive its change_id (64-char hex BLAKE3 hash).
    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode");
    // change_id IS the content-addressed ObjectId of the changeset bytes.
    let change_id = ObjectId::from_bytes(&cbor_bytes).to_hex();

    // 1. Store the changeset payload — this writes changeset CBOR at the same
    //    object-store key that the hash-lookup branch will try to decode as a report.
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save_changeset_payload must succeed");

    // 2. Store a VerificationReport under the same change_id — the sidecar records
    //    the report hash + profile so the fallback branch can find it.
    let report = VerificationReport::default();
    store
        .save_verification_report(&change_id, "prod", &report)
        .await
        .expect("save_verification_report must succeed");

    // 3. inspect report <change_id> — before the fix this failed with a cryptic
    //    "report decoding failed: missing field `entries`" error because the hash
    //    lookup found changeset CBOR and tried to decode it as VerificationReport.
    let result = cmd_inspect(OutputMode::Human, "report", &change_id, &store).await;
    assert!(
        result.is_ok(),
        "inspect report with hex change_id that collides with changeset hash must succeed \
         (sidecar fallback); got: {result:?}"
    );
}

// Scenario: inspect artifact dev.o resolves native when both WASM and native artifacts coexist.
//   GIVEN a file store where both cmd_compile --target wasm and --target native have been run
//   WHEN cmd_inspect artifact is called with "dev.o"
//   THEN load_wasm_artifact("dev.o") returns None (suppressed foreign-ext fallback)
//   AND  load_native_artifact("dev.o") returns Some (native artifact is selected)
//   AND  cmd_inspect returns Ok (no cross-type contamination)
//
// Regression guard for the Feature-O post-merge bug:
//   Before the fix, load_wasm_artifact("dev.o") fell back to the latest WASM entry,
//   causing cmd_inspect to return WASM data for a .o-named request.
#[tokio::test]
async fn cmd_inspect_artifact_dot_o_resolves_native_when_both_artifacts_coexist() {
    use crate::store::{file_store, init_file_layout};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    // Persist both a WASM and a native artifact so both index entries exist.
    cmd_compile(OutputMode::Human, "dev", "wasm", &store)
        .await
        .expect("wasm compile must succeed");
    cmd_compile(OutputMode::Human, "dev", "native", &store)
        .await
        .expect("native compile must succeed");

    // Regression check: load_wasm_artifact must NOT claim "dev.o" via fallback-to-latest.
    // Before the fix this returned Some(wasm) because the .wasm profile-match was empty
    // and the function fell back to the latest WASM entry unconditionally.
    let wasm_for_dot_o = store
        .load_wasm_artifact("dev.o")
        .expect("load_wasm_artifact must not error");
    assert!(
        wasm_for_dot_o.is_none(),
        "load_wasm_artifact must return None for a .o-suffixed name (foreign extension); \
         got Some(_) — WASM fallback incorrectly claimed a .o name"
    );

    // Positive check: load_native_artifact must resolve "dev.o" to the persisted artifact.
    let native_for_dot_o = store
        .load_native_artifact("dev.o")
        .expect("load_native_artifact must not error")
        .expect("load_native_artifact must return Some for 'dev.o' after native compile");
    assert_eq!(
        native_for_dot_o.target, "native",
        "persisted native artifact target must be 'native'; got: {:?}",
        native_for_dot_o.target
    );

    // End-to-end check: cmd_inspect must succeed (native branch is taken, not WASM).
    let result = cmd_inspect(OutputMode::Human, "artifact", "dev.o", &store).await;
    assert!(
        result.is_ok(),
        "inspect artifact dev.o must succeed when both WASM and native artifacts coexist; \
         got: {result:?}"
    );
}
