// ── ail-cli::inspect_commands ─────────────────────────────────────────────
//
// `ail inspect <kind> <id>` — extracted from cli.rs (Phase 5 refactor).

use ail_compiler::{emit_wasm_with_profile, lower_to_anf_with_graph, lower_to_core_ir};
use ail_verify::checker::Checker;
use ail_verify::report::VerificationReport;
use serde_json::{Value, json};

use crate::cli_helpers::{
    bytes_to_hex, edge_to_json, hex_to_object_id, is_valid_change_id, node_to_json,
};
use crate::error::CliError;
use crate::graph_loading::{current_graph_for_cli, load_current_graph_for_cli};
use crate::output::{OutputMode, print_response};
use crate::package_registry_io::load_package_registry;
use crate::store::StoreHandle;

/// `ail inspect <kind> <id>` — inspect a node, snapshot, report, artifact, or capability.
///
/// Kinds:
/// - `node`       — semantic graph node by name
/// - `snapshot`   — snapshot envelope by ObjectId hex
/// - `report`     — verification report by id
/// - `artifact`   — compiled artifact by name or path
/// - `capability` — capability by name:Provider
pub(crate) async fn cmd_inspect(
    mode: OutputMode,
    kind: &str,
    id: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    match kind {
        "node" => {
            let graph = load_current_graph_for_cli(store).await?;
            let fallback_graph;
            let graph = if graph
                .nodes
                .iter()
                .any(|node| node.name == id || node.id.0.to_string() == id)
            {
                graph
            } else {
                fallback_graph = current_graph_for_cli()?;
                fallback_graph
            };
            let node = graph
                .nodes
                .iter()
                .find(|node| node.name == id || node.id.0.to_string() == id)
                .ok_or_else(|| CliError::NotFound(format!("node not found: {id}")))?;
            let incoming = graph
                .edges
                .iter()
                .filter(|edge| edge.target == node.id)
                .map(edge_to_json)
                .collect::<Vec<_>>();
            let outgoing = graph
                .edges
                .iter()
                .filter(|edge| edge.source == node.id)
                .map(edge_to_json)
                .collect::<Vec<_>>();
            let edges = incoming
                .iter()
                .chain(outgoing.iter())
                .cloned()
                .collect::<Vec<_>>();
            let effects = node
                .effect_row
                .as_ref()
                .map(|row| row.effects.clone())
                .unwrap_or_default();
            let capabilities = node
                .capability_reqs
                .as_ref()
                .map(|reqs| reqs.caps.clone())
                .unwrap_or_default();
            let contracts = node
                .contract_clauses
                .as_ref()
                .map(|clauses| {
                    clauses
                        .requires
                        .iter()
                        .chain(clauses.ensures.iter())
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let human_msg = format!(
                "type: node\nname: {}\nkind: {:?}\nref: {}\neffects: {}\ncapabilities: {}\ncontracts: {}\nbody: {}\nincoming_edges: {}\noutgoing_edges: {}",
                node.name,
                node.kind,
                node.id.0,
                effects.len(),
                capabilities.len(),
                node.contract_clauses
                    .as_ref()
                    .map(|clauses| clauses.requires.len() + clauses.ensures.len())
                    .unwrap_or(0),
                node.body_expr.as_deref().unwrap_or("(none)"),
                incoming.len(),
                outgoing.len(),
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "type": "node",
                    "node": node_to_json(node),
                    "incoming_edges": incoming,
                    "outgoing_edges": outgoing,
                    "edges": edges,
                    "effects": effects,
                    "capabilities": capabilities,
                    "contracts": contracts,
                    "body": node.body_expr,
                    "metadata": {
                        "content_hash": node.content_hash.as_ref().map(|h| h.hex.clone()),
                        "provenance": node.provenance.as_ref().map(|p| p.change_id.clone()),
                        "schema": node.schema.as_ref().map(|s| s.version.clone()),
                        "trust": node.trust_metadata,
                    }
                }),
            );
        }
        "snapshot" => {
            // Inspect snapshot by ObjectId hex.
            if !is_valid_change_id(id) {
                return Err(CliError::NotFound(format!("snapshot not found: {id}")));
            }
            let oid = hex_to_object_id(id)?;
            if let Some(snap) = store.load_snapshot(&oid).await? {
                let parent_hex = snap.parent_id.map(|p| p.to_hex());
                let change_hex = snap.applied_change_id.map(|c| c.to_hex());
                let ver_hash = snap.verification_report_hash.map(|h| bytes_to_hex(&h));
                let human_msg = format!(
                    "type: snapshot\nid: {}\ngraph_root: {}\nparent: {}\napplied_change: {}\nverification_report: {}\ncreated_at: {}",
                    snap.id,
                    snap.graph_root_hash,
                    parent_hex.as_deref().unwrap_or("(none)"),
                    change_hex.as_deref().unwrap_or("(none)"),
                    ver_hash.as_deref().unwrap_or("(none)"),
                    snap.created_at,
                );
                print_response(
                    mode,
                    &human_msg,
                    json!({
                        "type": "snapshot",
                        "id": snap.id.to_hex(),
                        "graph_root_hash": snap.graph_root_hash.to_hex(),
                        "parent_id": parent_hex,
                        "applied_change_id": change_hex,
                        "verification_report_hash": ver_hash,
                        "created_at": snap.created_at,
                    }),
                );
            } else {
                return Err(CliError::NotFound(format!("snapshot not found: {id}")));
            }
        }
        "report" => {
            // Try to load a persisted VerificationReport:
            //   1. If `id` is a 64-char hex hash → load by hash from the object store.
            //   2. Otherwise treat `id` as a change-id and load via the sidecar index.
            //   3. If neither is found → derive from the current graph (fallback).
            let (report, source, resolved_id, verified_profile) = if is_valid_change_id(id) {
                // Try hash lookup first.
                let hash_oid = hex_to_object_id(id)?;
                if let Some(r) = store.load_verification_report_by_hash(&hash_oid).await? {
                    (r, "persisted_by_hash", id.to_string(), None::<String>)
                } else if let Some((r, hash, profile)) =
                    store.load_verification_report_by_change_id(id).await?
                {
                    (r, "persisted_by_change_id", hash.to_hex(), Some(profile))
                } else {
                    let graph = load_current_graph_for_cli(store).await?;
                    let r = Checker::check(&graph);
                    (
                        r,
                        "derived_from_current_graph",
                        id.to_string(),
                        None::<String>,
                    )
                }
            } else {
                // id is not a valid 64-char hex; try it as a change-id sidecar lookup.
                if let Some((r, hash, profile)) =
                    store.load_verification_report_by_change_id(id).await?
                {
                    (r, "persisted_by_change_id", hash.to_hex(), Some(profile))
                } else {
                    let graph = load_current_graph_for_cli(store).await?;
                    let r = Checker::check(&graph);
                    (
                        r,
                        "derived_from_current_graph",
                        id.to_string(),
                        None::<String>,
                    )
                }
            };

            let summary = format!("{:?}", report.summary());
            let entries_count = report.entries.len();
            let diagnostics_count = report.diagnostics.len();
            let proof_obligations_count = report.proof_obligations.len();
            let entries_json: Vec<Value> = report
                .entries
                .iter()
                .map(|e| {
                    json!({
                        "claim": e.claim,
                        "state": format!("{:?}", e.state),
                        "scope": e.scope,
                        "blocking": e.blocking,
                    })
                })
                .collect();
            let diagnostics_json: Vec<Value> = report
                .diagnostics
                .iter()
                .map(|d| serde_json::to_value(d).unwrap_or_else(|_| json!({ "code": "" })))
                .collect();
            let profile_line = verified_profile
                .as_deref()
                .map(|p| format!("\nverified_profile: {p}"))
                .unwrap_or_default();
            let human_msg = format!(
                "type: report\nid: {resolved_id}\nsource: {source}{profile_line}\nsummary: {summary}\nentries: {entries_count}\ndiagnostics: {diagnostics_count}"
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "type": "report",
                    "id": resolved_id,
                    "source": source,
                    "verified_profile": verified_profile,
                    "status": summary,
                    "entries": entries_json,
                    "diagnostics": diagnostics_json,
                    "proof_obligations": proof_obligations_count,
                }),
            );
        }
        "artifact" => {
            // Try to load a previously persisted artifact.
            // Preference order (extension-aware):
            //   1. persisted WASM artifact (load_wasm_artifact) — suppresses fallback for .o names
            //   2. persisted native artifact (load_native_artifact)
            //   3. on-demand WASM compilation (fallback)
            // load_wasm_artifact will not claim .o-suffixed names via its fallback-to-latest,
            // so .o names skip straight to the native branch.
            if let Some(persisted) = store.load_wasm_artifact(id)? {
                let wasm_hash = persisted.hash.as_str();
                let profile = persisted.profile.as_str();
                let capabilities_manifest_val: Value =
                    serde_json::from_slice(&persisted.capabilities_manifest_json)
                        .unwrap_or(Value::Null);
                let artifact_manifest_val: Value =
                    serde_json::from_slice(&persisted.artifact_manifest_json)
                        .unwrap_or(Value::Null);
                let semantic_source_map_val: Value =
                    serde_json::from_slice(&persisted.source_map_json).unwrap_or(Value::Null);
                let human_msg = format!(
                    "type: artifact\nname: {id}\nsource: persisted_artifact\nprofile: {profile}\nhash: {wasm_hash}",
                );
                print_response(
                    mode,
                    &human_msg,
                    json!({
                        "type": "artifact",
                        "name": id,
                        "source": "persisted_artifact",
                        "hash": wasm_hash,
                        "profile": profile,
                        "target": persisted.target,
                        "wasm_bytes": persisted.wasm_bytes.len(),
                        "capabilities_manifest": capabilities_manifest_val,
                        "capabilities_manifest_source": "persisted_artifact",
                        "semantic_source_map": semantic_source_map_val,
                        "artifact_manifest": artifact_manifest_val,
                        "persisted_paths": {
                            "wasm_path": persisted.paths.wasm_path.to_string_lossy(),
                            "source_map_path": persisted.paths.source_map_path.to_string_lossy(),
                            "manifest_path": persisted.paths.manifest_path.to_string_lossy(),
                            "capabilities_path": persisted.paths.capabilities_path.to_string_lossy(),
                        },
                    }),
                );
            } else if let Some(persisted) = store.load_native_artifact(id)? {
                // Persisted native artifact — returned when no WASM artifact is indexed
                // for the requested name but a native artifact is available.
                let native_hash = persisted.hash.as_str();
                let profile = persisted.profile.as_str();
                let capabilities_manifest_val: Value =
                    serde_json::from_slice(&persisted.capabilities_manifest_json)
                        .unwrap_or(Value::Null);
                let artifact_manifest_val: Value =
                    serde_json::from_slice(&persisted.artifact_manifest_json)
                        .unwrap_or(Value::Null);
                let semantic_source_map_val: Value =
                    serde_json::from_slice(&persisted.source_map_json).unwrap_or(Value::Null);
                let human_msg = format!(
                    "type: artifact\nname: {id}\nsource: persisted_native_artifact\nprofile: {profile}\nhash: {native_hash}",
                );
                print_response(
                    mode,
                    &human_msg,
                    json!({
                        "type": "artifact",
                        "name": id,
                        "source": "persisted_native_artifact",
                        "hash": native_hash,
                        "profile": profile,
                        "target": persisted.target,
                        "native_bytes": persisted.object_bytes.len(),
                        "capabilities_manifest": capabilities_manifest_val,
                        "capabilities_manifest_source": "persisted_artifact",
                        "semantic_source_map": semantic_source_map_val,
                        "artifact_manifest": artifact_manifest_val,
                        "persisted_paths": {
                            "object_path": persisted.paths.object_path.to_string_lossy(),
                            "source_map_path": persisted.paths.source_map_path.to_string_lossy(),
                            "manifest_path": persisted.paths.manifest_path.to_string_lossy(),
                            "capabilities_path": persisted.paths.capabilities_path.to_string_lossy(),
                        },
                    }),
                );
            } else {
                // Compile the current graph on demand and return real artifact metadata.
                // The id is used as the artifact label. Source is explicitly
                // "computed_on_demand" — no persisted artifact is available.
                let graph = load_current_graph_for_cli(store).await?;
                let empty_report = VerificationReport {
                    entries: vec![],
                    ..Default::default()
                };
                let core = lower_to_core_ir(&graph, &empty_report)
                    .map_err(|e| CliError::Domain(format!("inspect artifact (core-ir): {e}")))?;
                let anf = lower_to_anf_with_graph(&core, &graph)
                    .map_err(|e| CliError::Domain(format!("inspect artifact (anf): {e}")))?;
                let artifact = emit_wasm_with_profile(&anf, "dev")
                    .map_err(|e| CliError::Domain(format!("inspect artifact (emit): {e}")))?;
                let wasm_hash = artifact.hash_chain.wasm_hash.map(|h| bytes_to_hex(&h));
                let profile = artifact.artifact_manifest.profile.clone();
                let compiler_version = artifact.artifact_manifest.compiler_version.clone();
                let capabilities_manifest_val =
                    serde_json::to_value(&artifact.capabilities_manifest).unwrap_or(Value::Null);
                let artifact_manifest_val: Value =
                    serde_json::from_slice(&artifact.artifact_manifest_json).unwrap_or(Value::Null);
                let semantic_source_map_val: Value =
                    serde_json::from_slice(&artifact.source_map_json).unwrap_or(Value::Null);
                let human_msg = format!(
                    "type: artifact\nname: {id}\nsource: computed_on_demand\nprofile: {profile}\nhash: {}\ncompiler: {compiler_version}",
                    wasm_hash.as_deref().unwrap_or("(none)")
                );
                print_response(
                    mode,
                    &human_msg,
                    json!({
                        "type": "artifact",
                        "name": id,
                        "source": "computed_on_demand",
                        "hash": wasm_hash,
                        "profile": profile,
                        "compiler_version": compiler_version,
                        "capabilities_manifest": capabilities_manifest_val,
                        "capabilities_manifest_source": "computed_from_wasm_bindings",
                        "semantic_source_map": semantic_source_map_val,
                        "artifact_manifest": artifact_manifest_val,
                    }),
                );
            }
        }
        "capability" => {
            // Query the package registry for real capability/trust/assumption data.
            // id format: "cap_name" or "cap_name:provider_filter"
            // Returns NotFound when no registered package exports this capability.
            let parts: Vec<&str> = id.splitn(2, ':').collect();
            let cap_name = parts[0];
            let provider_filter = parts.get(1).copied();
            let registry = load_package_registry(store)?;
            let exporters: Vec<_> = registry
                .all()
                .iter()
                .filter(|m| m.exported_capabilities.iter().any(|c| c == cap_name))
                .filter(|m| provider_filter.map(|p| m.name.contains(p)).unwrap_or(true))
                .collect();
            if exporters.is_empty() {
                return Err(CliError::NotFound(format!(
                    "capability not found: {cap_name}"
                )));
            }
            let provider = exporters
                .first()
                .map(|m| m.name.as_str())
                .unwrap_or("(unknown)");
            let assumptions: Vec<Value> = exporters
                .iter()
                .flat_map(|m| {
                    m.assumptions
                        .iter()
                        .map(|a| serde_json::to_value(a).unwrap_or_else(|_| json!({})))
                })
                .collect();
            let unsafe_surface: Vec<Value> = exporters
                .iter()
                .flat_map(|m| {
                    m.unsafe_surface
                        .iter()
                        .map(|u| serde_json::to_value(u).unwrap_or_else(|_| json!({})))
                })
                .collect();
            let trust_levels: Vec<String> = exporters
                .iter()
                .map(|m| format!("{:?}", m.trust_level))
                .collect();
            let human_msg = format!(
                "type: capability\nname: {cap_name}\nprovider: {provider}\nregistered: true\ngrant_scope: registry_export\nassumptions: {}\nunsafe_surface: {}",
                assumptions.len(),
                unsafe_surface.len(),
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "type": "capability",
                    "name": cap_name,
                    "provider": provider,
                    "registered": true,
                    "exported_by_registered_package": true,
                    "granted": true,
                    "grant_scope": "registry_export",
                    "trust": trust_levels,
                    "assumptions": assumptions,
                    "unsafe_surface": unsafe_surface,
                }),
            );
        }
        _ => {
            // Unknown kind: fall back to the old ObjectId lookup for backward compatibility.
            if is_valid_change_id(id) {
                let oid = hex_to_object_id(id)?;
                if let Some(snap) = store.load_snapshot(&oid).await? {
                    let parent_hex = snap.parent_id.map(|p| p.to_hex());
                    let change_hex = snap.applied_change_id.map(|c| c.to_hex());
                    let human_msg = format!(
                        "type: snapshot\nid: {}\ngraph_root: {}\nparent: {}\napplied_change: {}\ncreated_at: {}",
                        snap.id,
                        snap.graph_root_hash,
                        parent_hex.as_deref().unwrap_or("(none)"),
                        change_hex.as_deref().unwrap_or("(none)"),
                        snap.created_at,
                    );
                    print_response(
                        mode,
                        &human_msg,
                        json!({
                            "type": "snapshot",
                            "id": snap.id.to_hex(),
                            "graph_root_hash": snap.graph_root_hash.to_hex(),
                            "parent_id": parent_hex,
                            "applied_change_id": change_hex,
                            "created_at": snap.created_at,
                        }),
                    );
                    return Ok(());
                }
            }
            return Err(CliError::NotFound(format!(
                "unknown inspect kind '{kind}' or artifact not found: {id}"
            )));
        }
    }
    Ok(())
}
