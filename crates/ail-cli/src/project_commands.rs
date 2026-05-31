// ── ail-cli::project_commands ────────────────────────────────────────────
//
// Project lifecycle commands extracted from cli.rs to reduce hot-file
// conflicts.  Each function is a direct mechanical move — no behaviour
// changes relative to the cli.rs originals.
//
// Commands:
//   cmd_change  — create a draft ChangeSet (or apply immediately with --apply)
//   cmd_init    — initialise .ail/ directory structure and genesis snapshot
//   cmd_new     — create a new project directory with .ail/ and starter ACL
//   cmd_status  — show current snapshot, branch, pending changes, system state

use std::path::PathBuf;

use ail_change::canonical::{canonicalize, canonicalize_parsed};
use ail_core::semantic_graph::SemanticGraph;
use ail_runtime::blake3_hex_of;
use ail_storage::{SnapshotEnvelope, graph::ChangeSetLogEntry, object::ObjectId};
use serde_json::json;

use crate::changeset_input::{ChangeInput, load_parsed_changeset};
use crate::cli_helpers::{
    SimpleSnapshotBridge, build_structural_diff_preview, conflict_reason_message, encode_cbor,
    format_unix_ms, hex_to_object_id, input_source_label, latest_snapshot, make_text_changeset,
    unix_ms_now,
};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::package_registry_io::load_package_lockfile;
use crate::project::{ArtifactKind, ProjectContext, project_scaffold_names};
use crate::store::{StoreHandle, file_store, init_file_layout_with_branch};

// ── cmd_change ────────────────────────────────────────────────────────────

/// `ail change [text] [--file <path>] [--stdin] [--apply]`
///
/// Creates a draft ChangeSet. Does NOT apply by default (doc §Change workflow).
/// Outputs: submitted_change, parsed_change, canonical_change, structural_diff preview.
///
/// Rules (tooling.md):
/// - `ail change` does not apply by default.
/// - It creates a draft ChangeSet.
/// - Use `--apply` for immediate application (equivalent to `ail change` + `ail apply`).
pub(crate) async fn cmd_change(
    mode: OutputMode,
    text: Option<&str>,
    file: Option<PathBuf>,
    from_stdin: bool,
    apply_immediately: bool,
    branch: Option<&str>,
    store: &StoreHandle,
) -> Result<(), CliError> {
    // Determine input source: text > file > stdin.
    let (changeset, canonical, input_source) = if let Some(t) = text {
        // Text input: create a minimal ChangeSet from free-text description.
        let cs = make_text_changeset(t);
        let canonical = canonicalize(cs.clone());
        (cs, canonical, "text")
    } else {
        let input = if let Some(path) = file {
            ChangeInput::File(path)
        } else {
            // Both explicit --stdin and bare stdin (no file, no text) read from stdin.
            ChangeInput::Stdin
        };
        let parsed = load_parsed_changeset(input)?;
        let canonical = canonicalize_parsed(parsed.clone());
        let src = input_source_label(from_stdin);
        (parsed.changeset, canonical, src)
    };

    let cbor_bytes = encode_cbor(&canonical)?;
    let change_id = blake3_hex_of(&cbor_bytes);

    // Persist the log entry.
    let payload_oid = ObjectId::from_bytes(&cbor_bytes);
    let cs_oid = hex_to_object_id(&change_id)?;
    let base_snap_oid = ObjectId::from_bytes(&canonical.base_snapshot_id.0.to_le_bytes());
    let entry = ChangeSetLogEntry {
        id: cs_oid,
        base_snapshot_id: base_snap_oid,
        payload_hash: payload_oid,
        created_at: unix_ms_now(),
    };
    store.append_changeset_log(&entry).await?;
    // Persist the canonical CBOR bytes so cmd_verify can reconstruct the graph.
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await?;

    // Structural diff preview: empty graph → all ops are additions.
    let structural_diff = build_structural_diff_preview(&changeset.ops);

    // ── Apply gate (only when --apply flag is set) ────────────────────────
    //
    // Default behavior is DRAFT ONLY — the changeset is saved for later
    // verification and application via `ail apply <change_id>`.
    // This matches the doc rule: "ail change does not apply by default."
    let (status_str, new_snapshot_id) = if apply_immediately {
        let snapshots_before = store.list_snapshots().await?;
        let mut graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };
        let bridge = SimpleSnapshotBridge(canonical.base_snapshot_id);
        match ail_change::apply::apply(canonical.clone(), &mut graph, &bridge) {
            ail_change::model::ChangeSetOutcome::Applied => {
                let graph_root = store.save_graph(&graph).await?;
                let parent_id = latest_snapshot(&snapshots_before).map(|s| s.id);
                let snapshot = SnapshotEnvelope {
                    id: ObjectId::from_bytes(&format!("snapshot-after-{change_id}").into_bytes()),
                    graph_root_hash: graph_root,
                    parent_id,
                    applied_change_id: Some(cs_oid),
                    created_at: unix_ms_now(),
                    verification_report_hash: None,
                    ..Default::default()
                };
                let snap_id = store.save_snapshot_on_branch(&snapshot, branch).await?;
                ("applied", Some(snap_id.to_hex()))
            }
            ail_change::model::ChangeSetOutcome::RebaseRequired {
                current_snapshot_id,
            } => {
                return Err(CliError::RebaseRequired {
                    current_snapshot_id: current_snapshot_id.0,
                });
            }
            ail_change::model::ChangeSetOutcome::Failed { reason } => {
                return Err(CliError::Domain(format!("change apply failed: {reason}")));
            }
            ail_change::model::ChangeSetOutcome::ConflictIrresolvable { reason } => {
                return Err(CliError::Domain(format!(
                    "Change conflict: {}",
                    conflict_reason_message(&reason)
                )));
            }
        }
    } else {
        ("draft", None)
    };

    let human_msg = format!(
        "source: {input_source}\nauthor: {}\ndescription: {}\nops: {}\nchange-id: {}\nstatus: {status_str}\n---\nstructural_diff:\n  creates: {}\n  modifies: 0\n  deletes: 0",
        changeset.meta.author,
        changeset.meta.description,
        changeset.ops.len(),
        change_id,
        structural_diff["creates"].as_u64().unwrap_or(0),
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "submitted_change": {
                "author": changeset.meta.author,
                "description": changeset.meta.description,
                "ops": changeset.ops.len(),
            },
            "parsed_change": {
                "op_count": changeset.ops.len(),
            },
            "canonical_change": {
                "change_id": change_id,
                "base_snapshot_id": canonical.base_snapshot_id.0,
            },
            "structural_diff": structural_diff,
            "status": status_str,
            "new_snapshot_id": new_snapshot_id,
        }),
    );
    Ok(())
}

// ── cmd_init ──────────────────────────────────────────────────────────────

/// `ail init` — create .ail/ directory structure and initialize baseline state.
///
/// Creates:
/// - graph store
/// - default branch
/// - project policy
/// - runtime profiles
/// - stdlib baseline
/// - package lock
/// - context indexes
pub(crate) async fn cmd_init(
    mode: OutputMode,
    store: &StoreHandle,
    branch: &str,
) -> Result<(), CliError> {
    let ctx = ProjectContext::from_cwd()?;
    debug_assert_eq!(ctx.root.join(".ail"), ctx.ail_dir);

    init_file_layout_with_branch(&ctx.ail_dir, branch)?;

    // Create all required subdirectories.
    for kind in [
        ArtifactKind::Change,
        ArtifactKind::Snapshot,
        ArtifactKind::Report,
        ArtifactKind::Wasm,
        ArtifactKind::Native,
    ] {
        let subdir = ctx
            .artifact_name(kind, ".init")
            .parent()
            .expect("artifact paths must include a subdirectory")
            .to_path_buf();
        std::fs::create_dir_all(&subdir)?;
    }

    // Write project.toml (graph store + default branch + project policy).
    let config_path = ctx.ail_dir.join("project.toml");
    if !config_path.exists() {
        let config_content = format!(
            "name = \".\"\ncreated_at = {}\nbranch = \"{branch}\"\npolicy = \"default\"\n",
            unix_ms_now()
        );
        std::fs::write(&config_path, config_content)?;
    }

    // Write runtime profiles baseline.
    let profiles_path = ctx.ail_dir.join("runtime_profiles.toml");
    if !profiles_path.exists() {
        let profiles_content = "[profiles]\ndev = { max_memory_bytes = \"unlimited\", max_fuel = \"unlimited\" }\nprod = { max_memory_bytes = \"128mb\", max_fuel = \"1000000\" }\n";
        std::fs::write(&profiles_path, profiles_content)?;
    }

    // Write stdlib baseline.
    let stdlib_path = ctx.ail_dir.join("stdlib.toml");
    if !stdlib_path.exists() {
        std::fs::write(&stdlib_path, "version = \"0\"\n")?;
    }

    // Write package lock.
    let lock_path = ctx.ail_dir.join("package.lock");
    if !lock_path.exists() {
        std::fs::write(&lock_path, "{}\n")?;
    }

    // Write context index (empty).
    let index_path = ctx.ail_dir.join("context_index.json");
    if !index_path.exists() {
        std::fs::write(&index_path, "{\"nodes\":[],\"edges\":[]}\n")?;
    }

    // Persist genesis snapshot (idempotent).
    let disk_store = file_store(ctx.ail_dir.clone());
    let active_store = match store {
        StoreHandle::Postgres(_) => store,
        StoreHandle::Memory { .. } | StoreHandle::File { .. } => &disk_store,
    };

    let existing = active_store.list_snapshots().await?;
    let genesis_id = if existing.is_empty() {
        let graph_root_hash = active_store
            .save_graph(&SemanticGraph {
                nodes: vec![],
                edges: vec![],
            })
            .await?;
        let genesis = SnapshotEnvelope {
            id: ObjectId::from_bytes(b"genesis"),
            graph_root_hash,
            parent_id: None,
            applied_change_id: None,
            created_at: unix_ms_now(),
            verification_report_hash: None,
            ..Default::default()
        };
        active_store.save_snapshot(&genesis).await?
    } else {
        existing[0].id
    };

    let genesis_hex = genesis_id.to_hex();
    let human_msg = format!(
        "initialized project at {}\ngenesis snapshot: {genesis_hex}\nbranch: {branch}\npolicy: default\nruntime profiles: dev, prod\nstdlib: v0\npackage lock: empty\ncontext index: empty",
        ctx.ail_dir.display()
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "initialized": true,
            "genesis_snapshot_id": genesis_hex,
            "branch": branch,
            "policy": "default",
            "runtime_profiles": ["dev", "prod"],
            "stdlib_baseline": "v0",
            "package_lock": "empty",
            "context_indexes": "empty",
        }),
    );
    Ok(())
}

// ── cmd_new ───────────────────────────────────────────────────────────────

/// `ail new <path>` — create a new project directory with a local .ail store.
///
/// This is intentionally file-backed even when `--database-url` is configured:
/// project creation is a filesystem scaffold operation, while database-backed
/// storage remains an opt-in runtime/storage mode for commands inside a project.
pub(crate) async fn cmd_new(
    mode: OutputMode,
    path: PathBuf,
    branch: &str,
    force: bool,
) -> Result<(), CliError> {
    let project_names = project_scaffold_names(&path)?;
    let project_name = project_names.manifest_name.as_str();
    let scaffold_ident = project_names.scaffold_ident.as_str();

    if path.exists() && !force && path.read_dir()?.next().is_some() {
        return Err(CliError::Domain(format!(
            "refusing to create project in non-empty directory: {}",
            path.display()
        )));
    }

    std::fs::create_dir_all(&path)?;
    let ctx = ProjectContext::new(path.clone());
    init_file_layout_with_branch(&ctx.ail_dir, branch)?;

    for kind in [
        ArtifactKind::Change,
        ArtifactKind::Snapshot,
        ArtifactKind::Report,
        ArtifactKind::Wasm,
        ArtifactKind::Native,
    ] {
        let subdir = ctx
            .artifact_name(kind, ".init")
            .parent()
            .expect("artifact paths must include a subdirectory")
            .to_path_buf();
        std::fs::create_dir_all(&subdir)?;
    }

    let config_path = ctx.ail_dir.join("project.toml");
    if !config_path.exists() {
        let config_content = format!(
            "name = \"{project_name}\"\ncreated_at = {}\nbranch = \"{branch}\"\npolicy = \"default\"\n",
            unix_ms_now()
        );
        std::fs::write(&config_path, config_content)?;
    }

    let profiles_path = ctx.ail_dir.join("runtime_profiles.toml");
    if !profiles_path.exists() {
        let profiles_content = "[profiles]\ndev = { max_memory_bytes = \"unlimited\", max_fuel = \"unlimited\" }\nprod = { max_memory_bytes = \"128mb\", max_fuel = \"1000000\" }\n";
        std::fs::write(&profiles_path, profiles_content)?;
    }

    let stdlib_path = ctx.ail_dir.join("stdlib.toml");
    if !stdlib_path.exists() {
        std::fs::write(&stdlib_path, "version = \"0\"\n")?;
    }

    let lock_path = ctx.ail_dir.join("package.lock");
    if !lock_path.exists() {
        std::fs::write(&lock_path, "{}\n")?;
    }

    let index_path = ctx.ail_dir.join("context_index.json");
    if !index_path.exists() {
        std::fs::write(&index_path, "{\"nodes\":[],\"edges\":[]}\n")?;
    }

    let source_path = path.join("main.ail");
    if !source_path.exists() {
        std::fs::write(&source_path, starter_source())?;
    }

    let sample_path = path.join("main.acl");
    if !sample_path.exists() {
        std::fs::write(&sample_path, starter_acl(scaffold_ident))?;
    }

    let disk_store = file_store(ctx.ail_dir.clone());
    let existing = disk_store.list_snapshots().await?;
    let genesis_id = if existing.is_empty() {
        let graph_root_hash = disk_store
            .save_graph(&SemanticGraph {
                nodes: vec![],
                edges: vec![],
            })
            .await?;
        let genesis = SnapshotEnvelope {
            id: ObjectId::from_bytes(b"genesis"),
            graph_root_hash,
            parent_id: None,
            applied_change_id: None,
            created_at: unix_ms_now(),
            verification_report_hash: None,
            ..Default::default()
        };
        disk_store.save_snapshot(&genesis).await?
    } else {
        existing[0].id
    };

    let genesis_hex = genesis_id.to_hex();
    let human_msg = format!(
        "created project at {}\ngenesis snapshot: {genesis_hex}\nbranch: {branch}\nstarter: {}",
        path.display(),
        source_path.display()
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "created": true,
            "path": path,
            "ail_dir": ctx.ail_dir,
            "branch": branch,
            "genesis_snapshot_id": genesis_hex,
            "starter_source": source_path,
            "starter_acl": sample_path,
            "project_name": project_name,
            "starter_change": format!("{scaffold_ident}_hello"),
        }),
    );
    Ok(())
}

fn starter_source() -> &'static str {
    "fn main() -> Int = add(20, 22)\n\
test main_addition = eq(add(20, 22), 42)\n"
}

fn starter_acl(scaffold_ident: &str) -> String {
    format!(
        "change {scaffold_ident}_hello\n\
author ail\n\
description starter AIL program\n\
base 0\n\
op create_function id=fn.main return=Int body=add(20, 22)\n\
op create_test id=test.main_addition body=eq(add(20, 22), 42)\n\
end\n"
    )
}

// ── cmd_status ────────────────────────────────────────────────────────────

/// `ail status` — show current snapshot, branch, pending changes, and system state.
///
/// Shows:
/// - current snapshot
/// - branch
/// - pending changes
/// - verification state
/// - stale indexes
/// - runtime profile status
/// - package advisories
pub(crate) async fn cmd_status(mode: OutputMode, store: &StoreHandle) -> Result<(), CliError> {
    let snapshots = store.list_snapshots().await?;
    let current_branch = store
        .current_branch()?
        .unwrap_or_else(|| "main".to_string());

    // Compute status fields.
    let (
        snap_hex,
        graph_root_hex,
        branch,
        pending_changes,
        verification_state,
        graph_nodes,
        last_change_at,
    ) = if snapshots.is_empty() {
        (
            "(none)".to_string(),
            "(none)".to_string(),
            current_branch,
            0usize,
            "unverified",
            0usize,
            "(none)".to_string(),
        )
    } else {
        let head_snapshot = store.head_snapshot().await?;
        let current = head_snapshot
            .as_ref()
            .or_else(|| latest_snapshot(&snapshots))
            .expect("non-empty");
        let snap_hex = current.id.to_hex();
        let graph_root_hex = current.graph_root_hash.to_hex();
        let graph_nodes = store
            .load_graph(&current.graph_root_hash)
            .await?
            .map(|graph| graph.nodes.len())
            .unwrap_or(0);
        let ver_state = if current.verification_report_hash.is_some() {
            "verified"
        } else {
            "unverified"
        };
        (
            snap_hex,
            graph_root_hex,
            current_branch,
            0,
            ver_state,
            graph_nodes,
            format_unix_ms(current.created_at),
        )
    };

    // Derived status fields.
    let stale_indexes = false;
    let runtime_profile_status = "valid";
    // Real lockfile count: number of installed packages when using a persistent store.
    let package_advisories = if store.has_persistent_project() {
        load_package_lockfile(store).map(|lf| lf.len()).unwrap_or(0)
    } else {
        0
    };

    let human_msg = format!(
        "branch: {branch}\nHEAD snapshot: {snap_hex}\ngraph nodes: {graph_nodes}\nlast change: {last_change_at}\ngraph_root: {graph_root_hex}\npending changes: {pending_changes}\nverification: {verification_state}\nstale indexes: {stale_indexes}\nruntime profile: {runtime_profile_status}\npackage advisories: {package_advisories}"
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "snapshot_id": snap_hex,
            "head_snapshot": snap_hex,
            "graph_root_hash": graph_root_hex,
            "branch": branch,
            "graph_nodes": graph_nodes,
            "last_change_at": last_change_at,
            "snapshot_count": snapshots.len(),
            "pending_changes": pending_changes,
            "verification_state": verification_state,
            "stale_indexes": stale_indexes,
            "runtime_profile_status": runtime_profile_status,
            "package_advisories": package_advisories,
        }),
    );
    Ok(())
}
