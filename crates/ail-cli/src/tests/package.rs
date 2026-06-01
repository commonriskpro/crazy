use super::*;
use crate::package_commands::package_manifest_for_current_graph;
use crate::package_registry_io::{package_manifest_path, save_package_registry};
use ail_package::PackageManifest;
use ail_package::PackageRegistry;

// Scenario: cmd_package add shows trust/capabilities/advisories.
#[tokio::test]
async fn cmd_package_add_shows_full_metadata() {
    use crate::store::memory_store;
    let store = memory_store();
    let manifest = package_manifest_for_current_graph(&store, "payments.stripe", "1.2")
        .await
        .expect("manifest");
    let mut registry = PackageRegistry::new();
    registry.register(manifest);
    save_package_registry(&store, &registry).expect("registry");
    let result = cmd_package(
        OutputMode::Human,
        PackageCmd::Add {
            package: "payments.stripe@1.2".to_string(),
        },
        &store,
    )
    .await;
    assert!(result.is_ok(), "package add must succeed; got: {result:?}");
}

#[test]
fn save_package_registry_propagates_corrupt_registry_decode_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    crate::store::init_file_layout(&ail_dir).expect("init layout");
    let packages_dir = ail_dir.join("packages");
    std::fs::create_dir_all(&packages_dir).expect("create package dir");
    std::fs::write(packages_dir.join("registry.cbor"), b"not cbor").expect("write registry");

    let store = crate::store::file_store(ail_dir);
    let registry = PackageRegistry::new();
    let err = save_package_registry(&store, &registry)
        .expect_err("corrupt registry must not be silently overwritten");

    assert!(
        err.to_string().contains("package registry decoding failed"),
        "unexpected error: {err}"
    );
}

// Scenario: cmd_package explain shows trust/capabilities/assumptions/unsafe/advisories.
#[tokio::test]
async fn cmd_package_explain_shows_full_metadata() {
    use crate::store::memory_store;
    let store = memory_store();
    let manifest = package_manifest_for_current_graph(&store, "payments.stripe", "1.2")
        .await
        .expect("manifest");
    let mut registry = PackageRegistry::new();
    registry.register(manifest);
    save_package_registry(&store, &registry).expect("registry");
    let result = cmd_package(
        OutputMode::Human,
        PackageCmd::Explain {
            package: "payments.stripe".to_string(),
        },
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "package explain must succeed; got: {result:?}"
    );
}

#[tokio::test]
async fn cmd_package_lint_blocks_manifest_without_production_metadata() {
    use crate::store::memory_store;
    let store = memory_store();

    let result = cmd_package(OutputMode::Json, PackageCmd::Lint, &store).await;

    let err = result.expect_err("default manifest must not pass production package lint");
    assert!(
        err.to_string().contains("package lint failed"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("production manifest issue"),
        "lint failure should describe production manifest issues; got: {err}"
    );
}

#[tokio::test]
async fn cmd_package_init_persists_license_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    crate::store::init_file_layout(&ail_dir).expect("init layout");
    let store = crate::store::file_store(ail_dir);

    let result = cmd_package(
        OutputMode::Json,
        PackageCmd::Init {
            name: Some("local.package".to_string()),
            version: "1.2.3".to_string(),
            license: Some("MIT".to_string()),
            source_digest: None,
            toolchain_id: None,
            recipe_hash: None,
        },
        &store,
    )
    .await;

    assert!(
        result.is_ok(),
        "package init with production metadata must succeed; got: {result:?}"
    );
    let path = package_manifest_path(&store).expect("manifest path");
    let bytes = std::fs::read(path).expect("manifest bytes");
    let manifest: PackageManifest = ciborium::from_reader(bytes.as_slice()).expect("manifest cbor");
    assert_eq!(manifest.license.as_deref(), Some("MIT"));
    assert!(
        manifest.production_validation_issues().is_empty(),
        "license, semver, and package name should produce a production-clean minimal manifest"
    );
}

#[tokio::test]
async fn cmd_package_init_persists_reproducible_evidence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    crate::store::init_file_layout(&ail_dir).expect("init layout");
    let store = crate::store::file_store(ail_dir);

    let result = cmd_package(
        OutputMode::Json,
        PackageCmd::Init {
            name: Some("local.package".to_string()),
            version: "1.2.3".to_string(),
            license: Some("MIT".to_string()),
            source_digest: Some("a".repeat(64)),
            toolchain_id: Some("ail-toolchain-1".to_string()),
            recipe_hash: Some("b".repeat(64)),
        },
        &store,
    )
    .await;

    assert!(
        result.is_ok(),
        "package init with reproducible evidence must succeed; got: {result:?}"
    );
    let path = package_manifest_path(&store).expect("manifest path");
    let bytes = std::fs::read(path).expect("manifest bytes");
    let manifest: PackageManifest = ciborium::from_reader(bytes.as_slice()).expect("manifest cbor");
    let evidence = manifest
        .reproducible_evidence
        .expect("evidence must persist");
    assert_eq!(evidence.source_digest, "a".repeat(64));
    assert_eq!(evidence.toolchain_id, "ail-toolchain-1");
    assert_eq!(evidence.recipe_hash, "b".repeat(64));
}
