use super::*;

// ── Manifest helpers ──────────────────────────────────────────────────────

/// Build a `PackageManifest` from the current semantic graph.
pub(crate) async fn package_manifest_for_current_graph(
    store: &StoreHandle,
    name: &str,
    version: &str,
) -> Result<PackageManifest, CliError> {
    let graph = load_current_graph_for_cli(store).await?;
    let graph_hash = store.save_graph(&graph).await?.to_hex();
    let required_capabilities = graph
        .nodes
        .iter()
        .flat_map(|node| {
            node.capability_reqs
                .as_ref()
                .map(|reqs| reqs.caps.clone())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let manifest = PackageManifest::from_def(PackageDef {
        name: name.to_string(),
        version: version.to_string(),
        trust_level: TrustLevel::Verified,
        required_capabilities,
        exported_capabilities: graph
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Capability)
            .map(|node| node.name.clone())
            .collect(),
        assumptions: vec![],
        unsafe_surface: vec![],
        artifact_hashes: vec![ArtifactHashEntry {
            role: "semantic-graph".to_string(),
            hash: graph_hash,
        }],
        build_env_hash: None,
        handlers: vec![],
        contracts: graph
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Contract)
            .map(|node| node.name.clone())
            .collect(),
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: Some(ail_package::Provenance::from_url("local graph package")),
        verification_report: None,
        graph_schema: Some(1),
        core_ir_schema: Some(1),
        // 4G fields
        reproducible_evidence: None,
    });
    manifest
        .validate()
        .map_err(|e| CliError::Domain(format!("package manifest invalid: {e}")))?;
    Ok(manifest)
}

pub(super) async fn load_or_create_package_manifest(
    store: &StoreHandle,
) -> Result<PackageManifest, CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        return package_manifest_for_current_graph(store, "local.package", "0.1.0").await;
    }
    let path = package_manifest_path(store)?;
    if path.exists() {
        let bytes = std::fs::read(path)?;
        return ciborium::from_reader(bytes.as_slice())
            .map_err(|e| CliError::Domain(format!("package manifest decoding failed: {e}")));
    }
    package_manifest_for_current_graph(store, "local.package", "0.1.0").await
}
