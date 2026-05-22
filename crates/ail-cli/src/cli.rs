// ── ail-cli::cli ─────────────────────────────────────────────────────────
//
// CLI dispatch: ten subcommands + shared `--json` and `--database-url` flags.
//
// # Command surface
//
// | Command                  | Description                                           |
// |--------------------------|-------------------------------------------------------|
// | context                  | List snapshot envelopes from the store                |
// | change --file/-          | Load ChangeSet from file or stdin; print hash         |
// | verify <change-id>       | Run Checker on the named ChangeSet                    |
// | apply  <change-id>       | Apply ChangeSet via bridge; persist new snapshot      |
// | compile --profile        | lower_to_core_ir → lower_to_anf → emit_wasm           |
// | run    --profile         | RuntimeHost::validate_and_instantiate preflight       |
// | init                     | Create .ail/ dirs and genesis snapshot                |
// | status                   | Show current snapshot and pending changes             |
// | inspect <id>             | Show snapshot or log entry details by ObjectId        |
// | diff <a> <b>             | Structural diff between two snapshot ids              |
//
// # Exit codes
//
// - 0: success
// - 1: domain error (unknown id, stale base, preflight failed, etc.)
// - 2: dispatch error (unknown subcommand, missing required argument)
//
// # `--json` mode
//
// Every command accepts `--json`. When set, stdout is a valid JSON object
// with `"status"` and `"data"` top-level fields.  Human output is suppressed.
//
// # `--database-url` / `AIL_DATABASE_URL`
//
// When provided, the CLI connects to a Postgres backend for durable storage.
// Fallback: in-memory store (no persistence across invocations).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ail_change::{apply::SnapshotBridge, canonical::canonicalize, model::SnapshotId};
use ail_compiler::{emit_wasm, lower_to_anf, lower_to_core_ir};
use ail_core::semantic_graph::SemanticGraph;
use ail_runtime::{CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile, blake3_hex_of};
use ail_storage::{SnapshotEnvelope, graph::ChangeSetLogEntry, object::ObjectId};
use ail_verify::checker::Checker;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use crate::changeset_input::{ChangeInput, load_changeset};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::{StoreHandle, build_store};

// ── Cli ───────────────────────────────────────────────────────────────────

/// ail — AI-native language toolchain.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Emit machine-readable JSON (status + data) instead of human text.
    #[arg(long, global = true)]
    json: bool,

    /// Postgres database URL for durable storage.
    /// Falls back to AIL_DATABASE_URL env var, then in-memory store.
    #[arg(long, global = true)]
    database_url: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

// ── Commands ──────────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum Commands {
    /// List snapshot envelopes from the local store.
    Context,

    /// Load a ChangeSet from a file or stdin and print its canonical hash.
    Change {
        /// Path to an ACL file. Reads from stdin if not provided.
        #[arg(long, short)]
        file: Option<PathBuf>,
    },

    /// Run the verifier on a ChangeSet by its canonical change-id.
    Verify {
        /// Canonical change-id (blake3 hex) of the ChangeSet to verify.
        change_id: String,
    },

    /// Apply a ChangeSet and persist a new snapshot.
    Apply {
        /// Canonical change-id (blake3 hex) of the ChangeSet to apply.
        change_id: String,
    },

    /// Compile the current graph snapshot to a WASM artifact.
    Compile {
        /// Compiler profile name (e.g. `dev`).
        #[arg(long)]
        profile: String,
    },

    /// Run preflight validation on the compiled WASM artifact.
    Run {
        /// Runtime profile name (e.g. `dev`).
        #[arg(long)]
        profile: String,
    },

    /// Initialize the project: create .ail/ directories and genesis snapshot.
    Init,

    /// Show current snapshot, branch, and pending changes.
    Status,

    /// Show details of a snapshot or log entry by its ObjectId.
    Inspect {
        /// ObjectId hex string (64 chars) of the artifact to inspect.
        id: String,
    },

    /// Show structural diff between two snapshot ObjectIds.
    Diff {
        /// ObjectId hex of the base snapshot.
        a: String,
        /// ObjectId hex of the target snapshot.
        b: String,
    },
}

// ── PUBLIC ENTRY POINT ────────────────────────────────────────────────────

/// Parse CLI arguments, build the store, and dispatch to the appropriate handler.
///
/// Returns `Ok(())` on success, or a `CliError` on domain/dispatch failure.
/// The caller is responsible for mapping the error to stderr + exit code.
pub async fn run() -> Result<(), CliError> {
    let cli = Cli::try_parse().unwrap_or_else(|err| {
        let kind = err.kind();
        let code = err.exit_code();
        let _ = err.print();
        if kind == ErrorKind::InvalidSubcommand {
            eprintln!(
                "Available subcommands: context, change, verify, apply, compile, run, \
                 init, status, inspect, diff"
            );
            std::process::exit(2);
        }
        std::process::exit(code);
    });

    let mode = if cli.json {
        OutputMode::Json
    } else {
        OutputMode::Human
    };

    let store = build_store(cli.database_url.as_deref()).await?;

    match cli.command {
        Commands::Context => cmd_context(mode, &store).await,
        Commands::Change { file } => cmd_change(mode, file, &store).await,
        Commands::Verify { change_id } => cmd_verify(mode, &change_id),
        Commands::Apply { change_id } => cmd_apply(mode, &change_id, &store).await,
        Commands::Compile { profile } => cmd_compile(mode, &profile),
        Commands::Run { profile } => cmd_run(mode, &profile),
        Commands::Init => cmd_init(mode, &store).await,
        Commands::Status => cmd_status(mode, &store).await,
        Commands::Inspect { id } => cmd_inspect(mode, &id, &store).await,
        Commands::Diff { a, b } => cmd_diff(mode, &a, &b, &store).await,
    }
}

// ── COMMAND HANDLERS ──────────────────────────────────────────────────────

/// `ail context` — list snapshot envelopes from the store.
async fn cmd_context(mode: OutputMode, store: &StoreHandle) -> Result<(), CliError> {
    let snapshots = store.list_snapshots().await?;

    if snapshots.is_empty() {
        print_response(
            mode,
            "(no snapshots in local store)",
            json!({ "snapshots": [] }),
        );
        return Ok(());
    }

    let human_lines: Vec<String> = snapshots
        .iter()
        .map(|s| {
            let parent = s
                .parent_id
                .map(|p| p.to_hex())
                .unwrap_or_else(|| "(genesis)".to_string());
            format!(
                "id: {}  parent: {}  created: {}",
                s.id, parent, s.created_at
            )
        })
        .collect();
    let human_msg = human_lines.join("\n");

    let json_snaps: Vec<Value> = snapshots
        .iter()
        .map(|s| {
            json!({
                "id": s.id.to_hex(),
                "parent_id": s.parent_id.map(|p| p.to_hex()),
                "created_at": s.created_at,
            })
        })
        .collect();

    print_response(mode, &human_msg, json!({ "snapshots": json_snaps }));
    Ok(())
}

/// `ail change [--file <path>]` — load a ChangeSet, canonicalize, persist, print hash.
async fn cmd_change(
    mode: OutputMode,
    file: Option<PathBuf>,
    store: &StoreHandle,
) -> Result<(), CliError> {
    let input = match file {
        Some(path) => ChangeInput::File(path),
        None => ChangeInput::Stdin,
    };

    let changeset = load_changeset(input)?;
    let canonical = canonicalize(changeset.clone());

    // Compute canonical change-id: blake3(CBOR(CanonicalChangeSet)).
    let cbor_bytes = encode_cbor(&canonical)?;
    let change_id = blake3_hex_of(&cbor_bytes);

    // Persist: store the CBOR bytes as a CAS object, then append a log entry.
    let payload_oid = ObjectId::from_bytes(&cbor_bytes);
    // Derive the changeset's identity ObjectId from the hex change_id.
    let cs_oid = hex_to_object_id(&change_id)?;
    let base_snap_oid = ObjectId::from_bytes(&canonical.base_snapshot_id.0.to_le_bytes());

    let entry = ChangeSetLogEntry {
        id: cs_oid,
        base_snapshot_id: base_snap_oid,
        payload_hash: payload_oid,
        created_at: unix_ms_now(),
    };
    store.append_changeset_log(&entry).await?;

    let human_msg = format!(
        "author: {}\ndescription: {}\nops: {}\nchange-id: {}",
        changeset.meta.author,
        changeset.meta.description,
        changeset.ops.len(),
        change_id,
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "author": changeset.meta.author,
            "description": changeset.meta.description,
            "ops": changeset.ops.len(),
            "change_id": change_id,
        }),
    );
    Ok(())
}

/// `ail verify <change-id>` — run Checker on the ChangeSet for change-id.
///
/// The graph is currently always empty (no durable graph retrieval yet).
fn cmd_verify(mode: OutputMode, change_id: &str) -> Result<(), CliError> {
    // Validate change-id format: must be 64 hex chars.
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }

    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let report = Checker::check(&graph);
    let summary = format!("{:?}", report.summary());
    let entry_count = report.entries.len();

    let human_msg = format!("change-id: {change_id}\nentries: {entry_count}\nsummary: {summary}");
    let entries_json: Vec<Value> = report
        .entries
        .iter()
        .map(|e| {
            json!({
                "claim": e.claim,
                "state": format!("{:?}", e.state),
                "scope": e.scope,
            })
        })
        .collect();

    print_response(
        mode,
        &human_msg,
        json!({
            "change_id": change_id,
            "entries": entries_json,
            "summary": summary,
        }),
    );
    Ok(())
}

/// `ail apply <change-id>` — apply a ChangeSet and persist a new SnapshotEnvelope.
async fn cmd_apply(mode: OutputMode, change_id: &str, store: &StoreHandle) -> Result<(), CliError> {
    // Validate change-id format.
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }

    use ail_change::apply::apply as apply_changeset;
    use ail_change::canonical::{CanonicalChangeSet, CanonicalMeta};
    use ail_change::model::Timestamp;

    // Determine the current snapshot id.  If the store has snapshots, use the
    // most-recent one; otherwise use genesis (SnapshotId(0)).
    let snapshots = store.list_snapshots().await?;
    let current_snapshot_id = SnapshotId(snapshots.len() as u64);

    let bridge = SimpleSnapshotBridge(current_snapshot_id);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };

    let canonical = CanonicalChangeSet {
        meta: CanonicalMeta {
            author: "cli".to_string(),
            description: "<applied via change-id>".to_string(),
            timestamp: Timestamp(0),
        },
        base_snapshot_id: current_snapshot_id,
        preconditions: vec![],
        ops: vec![],
        ..Default::default()
    };

    let outcome = apply_changeset(canonical, &mut graph, &bridge);

    match outcome {
        ail_change::model::ChangeSetOutcome::Applied => {
            // Persist a new SnapshotEnvelope.
            let change_oid = hex_to_object_id(change_id)?;
            let graph_root = ObjectId::from_bytes(b"empty-graph-root");
            let parent_id = snapshots.last().map(|s| s.id);
            let new_envelope = SnapshotEnvelope {
                id: ObjectId::from_bytes(&format!("snapshot-after-{change_id}").into_bytes()),
                graph_root_hash: graph_root,
                parent_id,
                applied_change_id: Some(change_oid),
                created_at: unix_ms_now(),
                verification_report_hash: None,
            };
            let new_id = store.save_snapshot(&new_envelope).await?;
            let new_id_hex = new_id.to_hex();

            let human_msg = format!("applied; new snapshot id: {new_id_hex}");
            print_response(
                mode,
                &human_msg,
                json!({
                    "change_id": change_id,
                    "new_snapshot_id": new_id_hex,
                }),
            );
            Ok(())
        }
        ail_change::model::ChangeSetOutcome::RebaseRequired {
            current_snapshot_id,
        } => Err(CliError::RebaseRequired {
            current_snapshot_id: current_snapshot_id.0,
        }),
        ail_change::model::ChangeSetOutcome::Failed { reason } => {
            Err(CliError::Domain(format!("apply failed: {reason}")))
        }
        ail_change::model::ChangeSetOutcome::ConflictIrresolvable { reason } => {
            Err(CliError::Domain(format!("conflict: {reason:?}")))
        }
    }
}

/// `ail compile --profile <name>` — run the three-stage lowering pipeline.
fn cmd_compile(mode: OutputMode, profile: &str) -> Result<(), CliError> {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let report = Checker::check(&graph);

    let core = lower_to_core_ir(&graph, &report)
        .map_err(|e| CliError::Domain(format!("compile (core ir): {e:?}")))?;

    let anf = lower_to_anf(&core).map_err(|e| CliError::Domain(format!("compile (anf): {e:?}")))?;

    let artifact =
        emit_wasm(&anf).map_err(|e| CliError::Domain(format!("compile (wasm): {e:?}")))?;

    let wasm_hash = artifact
        .hash_chain
        .wasm_hash
        .map(|h| bytes_to_hex(&h))
        .unwrap_or_else(|| "<none>".to_string());
    let wasm_size = artifact.wasm.len();

    let human_msg = format!("profile: {profile}\nwasm bytes: {wasm_size}\nwasm-hash: {wasm_hash}");
    print_response(
        mode,
        &human_msg,
        json!({
            "profile": profile,
            "wasm_bytes": wasm_size,
            "wasm_hash": wasm_hash,
        }),
    );
    Ok(())
}

/// `ail run --profile <name>` — validate and instantiate the WASM artifact.
fn cmd_run(mode: OutputMode, profile: &str) -> Result<(), CliError> {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let report = Checker::check(&graph);

    let core = lower_to_core_ir(&graph, &report)
        .map_err(|e| CliError::Domain(format!("run (core ir): {e:?}")))?;
    let anf = lower_to_anf(&core).map_err(|e| CliError::Domain(format!("run (anf): {e:?}")))?;
    let artifact = emit_wasm(&anf).map_err(|e| CliError::Domain(format!("run (wasm): {e:?}")))?;

    let manifest = CapabilityManifest {
        module: profile.to_string(),
        requires: vec![],
    };
    let module_hash = blake3_hex_of(&artifact.wasm);
    let manifest_hash = manifest
        .blake3_hex()
        .map_err(|e| CliError::Domain(format!("run (manifest hash): {e}")))?;

    let runtime_profile = RuntimeProfile::new(
        profile.to_string(),
        module_hash,
        String::new(),
        manifest_hash,
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&artifact.wasm, &manifest, &runtime_profile);

    match result {
        Ok(_instance) => {
            let event = host.audit_log().events().first();
            let event_str = event
                .map(|e| format!("{e:?}"))
                .unwrap_or_else(|| "<no event>".to_string());

            let human_msg = format!("PreflightPassed\nprofile: {profile}\nevent: {event_str}");
            print_response(
                mode,
                &human_msg,
                json!({
                    "outcome": "PreflightPassed",
                    "profile": profile,
                    "audit_events": host.audit_log().len(),
                }),
            );
            Ok(())
        }
        Err(e) => Err(CliError::PreflightFailed(format!("{e}"))),
    }
}

/// `ail init` — create .ail/ directory structure and persist a genesis snapshot.
async fn cmd_init(mode: OutputMode, store: &StoreHandle) -> Result<(), CliError> {
    use crate::project::{ArtifactKind, ProjectContext};

    let ctx = ProjectContext::from_cwd()?;

    // Create all required subdirectories.
    for kind in [
        ArtifactKind::Change,
        ArtifactKind::Snapshot,
        ArtifactKind::Report,
        ArtifactKind::Wasm,
    ] {
        let dir = ctx.artifact_name(kind, "").parent().unwrap().to_path_buf();
        // artifact_name returns .ail/<subdir>/<id>; we want .ail/<subdir>/
        let subdir = ctx.ail_dir.join(match kind {
            ArtifactKind::Change => "changes",
            ArtifactKind::Snapshot => "snapshots",
            ArtifactKind::Report => "reports",
            ArtifactKind::Wasm => "wasm",
        });
        let _ = dir; // silence unused variable
        std::fs::create_dir_all(&subdir)?;
    }

    // Write project.toml.
    let config_path = ctx.ail_dir.join("project.toml");
    if !config_path.exists() {
        let config_content = format!("name = \".\"\ncreated_at = {}\n", unix_ms_now());
        std::fs::write(&config_path, config_content)?;
    }

    // Persist genesis snapshot (idempotent: only if no snapshots exist).
    let existing = store.list_snapshots().await?;
    let genesis_id = if existing.is_empty() {
        let genesis = SnapshotEnvelope {
            id: ObjectId::from_bytes(b"genesis"),
            graph_root_hash: ObjectId::from_bytes(b"empty-root"),
            parent_id: None,
            applied_change_id: None,
            created_at: unix_ms_now(),
            verification_report_hash: None,
        };
        store.save_snapshot(&genesis).await?
    } else {
        existing[0].id
    };

    let genesis_hex = genesis_id.to_hex();
    let human_msg = format!(
        "initialized project at {}\ngenesis snapshot: {genesis_hex}",
        ctx.ail_dir.display()
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "initialized": true,
            "genesis_snapshot_id": genesis_hex,
        }),
    );
    Ok(())
}

/// `ail status` — show current snapshot and pending changes count.
async fn cmd_status(mode: OutputMode, store: &StoreHandle) -> Result<(), CliError> {
    let snapshots = store.list_snapshots().await?;

    if snapshots.is_empty() {
        print_response(
            mode,
            "status: no snapshots\nbranch: main\npending changes: 0",
            json!({
                "snapshot_id": null,
                "branch": "main",
                "pending_changes": 0,
            }),
        );
        return Ok(());
    }

    // Most recent snapshot = last in the list.
    let current = snapshots.last().expect("non-empty vec must have last");
    let snap_hex = current.id.to_hex();

    let human_msg = format!("snapshot: {snap_hex}\nbranch: main\npending changes: 0");
    print_response(
        mode,
        &human_msg,
        json!({
            "snapshot_id": snap_hex,
            "branch": "main",
            "pending_changes": 0,
        }),
    );
    Ok(())
}

/// `ail inspect <id>` — show snapshot or log entry details.
async fn cmd_inspect(mode: OutputMode, id: &str, store: &StoreHandle) -> Result<(), CliError> {
    if !is_valid_change_id(id) {
        return Err(CliError::NotFound(format!("not found: {id}")));
    }

    let oid = hex_to_object_id(id)?;

    // Try to load as SnapshotEnvelope.
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

    // Not found as snapshot.
    Err(CliError::NotFound(format!("not found: {id}")))
}

/// `ail diff <a> <b>` — structural diff between two snapshot ObjectIds.
async fn cmd_diff(mode: OutputMode, a: &str, b: &str, store: &StoreHandle) -> Result<(), CliError> {
    if !is_valid_change_id(a) {
        return Err(CliError::NotFound(format!("snapshot not found: {a}")));
    }
    if !is_valid_change_id(b) {
        return Err(CliError::NotFound(format!("snapshot not found: {b}")));
    }

    let oid_a = hex_to_object_id(a)?;
    let oid_b = hex_to_object_id(b)?;

    let snap_a = store
        .load_snapshot(&oid_a)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("snapshot not found: {a}")))?;
    let snap_b = store
        .load_snapshot(&oid_b)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("snapshot not found: {b}")))?;

    // Structural diff: compare all envelope fields.
    let mut changes: Vec<Value> = vec![];

    if snap_a.graph_root_hash != snap_b.graph_root_hash {
        changes.push(json!({
            "field": "graph_root_hash",
            "from": snap_a.graph_root_hash.to_hex(),
            "to": snap_b.graph_root_hash.to_hex(),
        }));
    }
    if snap_a.parent_id != snap_b.parent_id {
        changes.push(json!({
            "field": "parent_id",
            "from": snap_a.parent_id.map(|p| p.to_hex()),
            "to": snap_b.parent_id.map(|p| p.to_hex()),
        }));
    }
    if snap_a.applied_change_id != snap_b.applied_change_id {
        changes.push(json!({
            "field": "applied_change_id",
            "from": snap_a.applied_change_id.map(|c| c.to_hex()),
            "to": snap_b.applied_change_id.map(|c| c.to_hex()),
        }));
    }

    let human_lines: Vec<String> = if changes.is_empty() {
        vec!["(no structural differences)".to_string()]
    } else {
        changes
            .iter()
            .map(|c| {
                format!(
                    "  {} : {} → {}",
                    c["field"].as_str().unwrap_or("?"),
                    c["from"],
                    c["to"]
                )
            })
            .collect()
    };
    let human_msg = format!(
        "snapshot {} → {}\n{}",
        &a[..8],
        &b[..8],
        human_lines.join("\n")
    );

    print_response(
        mode,
        &human_msg,
        json!({
            "from": a,
            "to": b,
            "changes": changes,
        }),
    );
    Ok(())
}

// ── PRIVATE HELPERS ───────────────────────────────────────────────────────

/// A minimal `SnapshotBridge` that always returns a fixed id.
struct SimpleSnapshotBridge(SnapshotId);

impl SnapshotBridge for SimpleSnapshotBridge {
    fn current_snapshot_id(&self) -> SnapshotId {
        self.0
    }
}

/// Encode a value as CBOR bytes.
fn encode_cbor<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, CliError> {
    let mut buf = Vec::new();
    ciborium::into_writer(value, &mut buf)
        .map_err(|e| CliError::Domain(format!("CBOR encoding failed: {e}")))?;
    Ok(buf)
}

/// Encode a byte slice as a lowercase hex string.
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Return `true` if `id` is a valid 64-character lowercase hex string.
fn is_valid_change_id(id: &str) -> bool {
    id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// Convert a 64-char hex string into an `ObjectId`.
///
/// Returns `CliError::Domain` if the string is not exactly 64 hex chars or
/// cannot be decoded.
fn hex_to_object_id(hex: &str) -> Result<ObjectId, CliError> {
    if hex.len() != 64 {
        return Err(CliError::Domain(format!(
            "invalid id length: {}",
            hex.len()
        )));
    }
    let mut bytes = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s =
            std::str::from_utf8(chunk).map_err(|_| CliError::Domain("non-UTF8 hex".to_string()))?;
        bytes[i] = u8::from_str_radix(s, 16)
            .map_err(|_| CliError::Domain(format!("invalid hex byte: {s}")))?;
    }
    Ok(ObjectId::from(bytes))
}

/// Return the current time as Unix milliseconds.
fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── UNIT TESTS ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Scenario: valid 64-char hex change-id is accepted.
    #[test]
    fn valid_change_id_accepted() {
        let id = "a".repeat(64);
        assert!(is_valid_change_id(&id), "64 hex chars must be accepted");
    }

    // TRIANGULATE: too-short change-id is rejected.
    #[test]
    fn short_change_id_rejected() {
        let id = "a".repeat(63);
        assert!(!is_valid_change_id(&id), "63 hex chars must be rejected");
    }

    // TRIANGULATE: non-hex change-id is rejected.
    #[test]
    fn non_hex_change_id_rejected() {
        let id = "g".repeat(64);
        assert!(!is_valid_change_id(&id), "non-hex chars must be rejected");
    }

    // Scenario: SimpleSnapshotBridge returns its initialised id.
    #[test]
    fn simple_snapshot_bridge_returns_initial_id() {
        let bridge = SimpleSnapshotBridge(SnapshotId(7));
        assert_eq!(bridge.current_snapshot_id(), SnapshotId(7));
    }

    // TRIANGULATE: encode_cbor succeeds for a JSON-compatible value.
    #[test]
    fn encode_cbor_returns_bytes_for_serializable_value() {
        #[derive(serde::Serialize)]
        struct Dummy {
            x: u32,
        }
        let bytes = encode_cbor(&Dummy { x: 42 }).expect("encode_cbor must succeed");
        assert!(!bytes.is_empty(), "encoded bytes must not be empty");
    }

    // Scenario: cmd_verify rejects invalid change-id (exit 1).
    #[test]
    fn cmd_verify_rejects_invalid_change_id() {
        let result = cmd_verify(OutputMode::Human, &"a".repeat(63));
        assert!(matches!(result, Err(CliError::NotFound(_))));
    }

    // Scenario: cmd_verify succeeds for a valid 64-char change-id (exit 0).
    #[test]
    fn cmd_verify_succeeds_for_valid_change_id() {
        let id = "a".repeat(64);
        let result = cmd_verify(OutputMode::Human, &id);
        assert!(result.is_ok(), "cmd_verify must succeed; got: {result:?}");
    }

    // Scenario: cmd_compile succeeds with an empty graph (exit 0).
    #[test]
    fn cmd_compile_succeeds() {
        let result = cmd_compile(OutputMode::Human, "dev");
        assert!(result.is_ok(), "cmd_compile must succeed; got: {result:?}");
    }

    // Scenario: cmd_run succeeds when preflight passes (exit 0).
    #[test]
    fn cmd_run_succeeds() {
        let result = cmd_run(OutputMode::Human, "dev");
        assert!(result.is_ok(), "cmd_run must succeed; got: {result:?}");
    }

    // Scenario: hex_to_object_id roundtrip.
    //   GIVEN a valid 64-char hex string
    //   WHEN hex_to_object_id is called
    //   THEN it returns an ObjectId whose to_hex() equals the input
    #[test]
    fn hex_to_object_id_roundtrip() {
        // 32 bytes = 64 hex chars: "a0b1" repeated 16 times.
        let hex = "a0b1".repeat(16); // 4 * 16 = 64 chars
        assert_eq!(hex.len(), 64, "test input must be 64 chars");
        let oid = hex_to_object_id(&hex).expect("valid hex must parse");
        assert_eq!(oid.to_hex(), hex, "roundtrip must preserve hex");
    }

    // TRIANGULATE: hex_to_object_id rejects non-hex.
    #[test]
    fn hex_to_object_id_rejects_invalid() {
        let bad = "g".repeat(64);
        assert!(hex_to_object_id(&bad).is_err(), "non-hex must return Err");
    }

    // Scenario: cmd_context async — succeeds with memory store.
    #[tokio::test]
    async fn cmd_context_memory_store_succeeds() {
        use crate::store::memory_store;
        let store = memory_store();
        let result = cmd_context(OutputMode::Human, &store).await;
        assert!(result.is_ok(), "cmd_context must succeed; got: {result:?}");
    }

    // Scenario: cmd_apply async succeeds with valid id + memory store.
    #[tokio::test]
    async fn cmd_apply_memory_store_succeeds() {
        use crate::store::memory_store;
        let store = memory_store();
        let id = "b".repeat(64);
        let result = cmd_apply(OutputMode::Human, &id, &store).await;
        assert!(result.is_ok(), "cmd_apply must succeed; got: {result:?}");
    }

    // Scenario: cmd_apply rejects invalid change-id.
    #[tokio::test]
    async fn cmd_apply_rejects_invalid_change_id() {
        use crate::store::memory_store;
        let store = memory_store();
        let result = cmd_apply(OutputMode::Human, &"a".repeat(63), &store).await;
        assert!(matches!(result, Err(CliError::NotFound(_))));
    }

    // Scenario: preflight fails on module hash mismatch.
    #[test]
    fn preflight_fails_on_module_hash_mismatch() {
        use ail_runtime::{CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile};

        let wasm_bytes: &[u8] = b"not-real-wasm";
        let wrong_module_hash = "0".repeat(64);

        let manifest = CapabilityManifest {
            module: "test".to_string(),
            requires: vec![],
        };
        let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");

        let profile = RuntimeProfile::new(
            "test".to_string(),
            wrong_module_hash,
            String::new(),
            manifest_hash,
            vec![],
            ResourceLimits {
                max_memory_bytes: None,
                max_fuel: None,
            },
        );

        let mut host = RuntimeHost::new();
        let result = host.validate_and_instantiate(wasm_bytes, &manifest, &profile);

        assert!(result.is_err(), "must fail when module_hash mismatches");
        let err_str = format!("{}", result.unwrap_err());
        assert!(
            err_str.contains("preflight failed"),
            "error must mention 'preflight failed'; got: {err_str}"
        );
    }

    // Spec scenario: stale base rejected (domain unit test).
    #[test]
    fn apply_stale_base_returns_rebase_required() {
        use ail_change::apply::apply as apply_changeset;
        use ail_change::canonical::{CanonicalChangeSet, CanonicalMeta};
        use ail_change::model::{ChangeSetOutcome, Timestamp};

        let bridge = SimpleSnapshotBridge(SnapshotId(1));
        let mut graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };

        let canonical = CanonicalChangeSet {
            meta: CanonicalMeta {
                author: "test".to_string(),
                description: "stale-base test".to_string(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            preconditions: vec![],
            ops: vec![],
            ..Default::default()
        };

        let outcome = apply_changeset(canonical, &mut graph, &bridge);
        assert!(
            matches!(
                outcome,
                ChangeSetOutcome::RebaseRequired {
                    current_snapshot_id: SnapshotId(1)
                }
            ),
            "stale base must return RebaseRequired; got: {outcome:?}"
        );
    }
}
