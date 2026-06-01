use super::*;

// ── Manifest helpers ──────────────────────────────────────────────────────

/// Build a `PackageManifest` from the current semantic graph.
pub(crate) async fn package_manifest_for_current_graph(
    store: &StoreHandle,
    name: &str,
    version: &str,
) -> Result<PackageManifest, CliError> {
    package_manifest_for_current_graph_with_metadata(store, name, version, None).await
}

pub(super) async fn package_manifest_for_current_graph_with_metadata(
    store: &StoreHandle,
    name: &str,
    version: &str,
    license: Option<String>,
) -> Result<PackageManifest, CliError> {
    let graph = load_current_graph_for_cli(store).await?;
    let graph_hash = store.save_graph(&graph).await?.to_hex();
    let mut artifact_hashes = vec![ArtifactHashEntry {
        role: "semantic-graph".to_string(),
        hash: graph_hash.clone(),
    }];
    artifact_hashes.extend(persisted_wasm_package_artifact_hashes(store, &graph_hash)?);
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
        artifact_hashes,
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
        license,
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

fn persisted_wasm_package_artifact_hashes(
    store: &StoreHandle,
    graph_hash: &str,
) -> Result<Vec<ArtifactHashEntry>, CliError> {
    let Some(persisted) = store.load_wasm_artifact("dev")? else {
        return Ok(Vec::new());
    };
    if persisted.profile != "dev" || persisted.target != "wasm" {
        return Ok(Vec::new());
    }
    let manifest: ArtifactManifest = serde_json::from_slice(&persisted.artifact_manifest_json)
        .map_err(|e| {
            CliError::Domain(format!(
                "package manifest wasm artifact sidecar invalid: {e}"
            ))
        })?;
    if bytes_to_hex(&manifest.graph_snapshot_hash) != graph_hash {
        return Ok(Vec::new());
    }

    let Some(abi_descriptor_json) = persisted.abi_descriptor_json.as_ref() else {
        return Ok(Vec::new());
    };
    let abi_descriptor_hash = blake3::hash(abi_descriptor_json).to_hex().to_string();

    Ok(vec![
        ArtifactHashEntry {
            role: "wasm-artifact".to_string(),
            hash: persisted.hash,
        },
        ArtifactHashEntry {
            role: "wasm-abi-descriptor".to_string(),
            hash: abi_descriptor_hash,
        },
    ])
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
