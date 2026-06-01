use super::*;
use crate::package_commands::package_manifest_for_current_graph;
use crate::package_registry_io::save_package_registry;
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
