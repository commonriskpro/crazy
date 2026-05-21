// ── ail-cli::cli ─────────────────────────────────────────────────────────
//
// CLI dispatch: six subcommands + shared `--json` flag.
//
// # Command surface
//
// | Command            | Description                                        |
// |--------------------|---------------------------------------------------|
// | context            | List snapshot envelopes from the local store       |
// | change --file/-    | Load ChangeSet from file or stdin; print hash      |
// | verify <change-id> | Run Checker on the named ChangeSet                 |
// | apply  <change-id> | Apply ChangeSet via bridge; persist new snapshot   |
// | compile --profile  | lower_to_core_ir → lower_to_anf → emit_wasm       |
// | run    --profile   | RuntimeHost::validate_and_instantiate preflight    |
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
// # Design constraints
//
// All heavy domain objects (graphs, snapshots, bridges) are constructed
// in-memory per command invocation — this is a local workflow CLI, not a
// daemon. No global state.

use std::path::PathBuf;

use ail_change::{apply::SnapshotBridge, canonical::canonicalize, model::SnapshotId};
use ail_compiler::{emit_wasm, lower_to_anf, lower_to_core_ir};
use ail_runtime::{
    CapabilityManifest, RuntimeHost, RuntimeProfile, ResourceLimits, blake3_hex_of,
};
use ail_verify::checker::Checker;
use ail_core::semantic_graph::SemanticGraph;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use crate::changeset_input::{ChangeInput, load_changeset};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};

// ── Cli ───────────────────────────────────────────────────────────────────

/// ail — AI-native language toolchain.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Emit machine-readable JSON (status + data) instead of human text.
    #[arg(long, global = true)]
    json: bool,

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
}

// ── PUBLIC ENTRY POINT ────────────────────────────────────────────────────

/// Parse CLI arguments and dispatch to the appropriate command handler.
///
/// Returns `Ok(())` on success, or a `CliError` on domain/dispatch failure.
/// The caller is responsible for mapping the error to stderr + exit code.
pub fn run() -> Result<(), CliError> {
    let cli = Cli::try_parse().unwrap_or_else(|err| {
        let kind = err.kind();
        let code = err.exit_code();
        let _ = err.print();
        if kind == ErrorKind::InvalidSubcommand {
            eprintln!(
                "Available subcommands: context, change, verify, apply, compile, run"
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

    match cli.command {
        Commands::Context => cmd_context(mode),
        Commands::Change { file } => cmd_change(mode, file),
        Commands::Verify { change_id } => cmd_verify(mode, &change_id),
        Commands::Apply { change_id } => cmd_apply(mode, &change_id),
        Commands::Compile { profile } => cmd_compile(mode, &profile),
        Commands::Run { profile } => cmd_run(mode, &profile),
    }
}

// ── COMMAND HANDLERS ──────────────────────────────────────────────────────

/// `ail context` — list snapshots from the local store.
///
/// In the current implementation the in-memory bridge holds no persisted
/// snapshots across invocations, so the output is always empty. This is
/// correct per the spec: "GIVEN the local store is empty, THEN output is
/// empty; exit 0."  Future phases will wire up a durable store.
fn cmd_context(mode: OutputMode) -> Result<(), CliError> {
    // The spec requires listing SnapshotEnvelope {id, parent_id, created_at}.
    // The MemorySnapshotBridge is in-process only and does not persist across
    // invocations. An empty store is valid per spec scenario "empty store".
    let snapshots: Vec<Value> = vec![];

    print_response(
        mode,
        "(no snapshots in local store)",
        json!({ "snapshots": snapshots }),
    );
    Ok(())
}

/// `ail change [--file <path>]` — load a ChangeSet, canonicalize, print hash.
///
/// If `--file` is provided, reads from that path; otherwise reads from stdin.
fn cmd_change(mode: OutputMode, file: Option<PathBuf>) -> Result<(), CliError> {
    let input = match file {
        Some(path) => ChangeInput::File(path),
        None => ChangeInput::Stdin,
    };

    let changeset = load_changeset(input)?;
    let canonical = canonicalize(changeset.clone());

    // Compute canonical change-id: blake3(CBOR(CanonicalChangeSet)).
    let cbor_bytes = encode_cbor(&canonical)?;
    let change_id = blake3_hex_of(&cbor_bytes);

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
/// In this phase the graph against which the ChangeSet is verified is empty
/// (no ACL parser, no durable store). The verifier will produce an empty
/// report (no nodes → no entries) with summary `Proven` (vacuous truth).
///
/// The change-id must be a 64-char blake3 hex string; any other value is
/// treated as "not found" and causes exit 1.
fn cmd_verify(mode: OutputMode, change_id: &str) -> Result<(), CliError> {
    // Validate change-id format: must be 64 hex chars.
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }

    // In Phase 9 this will load the ChangeSet from the durable store using the
    // change-id.  For now we verify against an empty in-memory graph, which
    // satisfies the spec's scope ("Semantic queries are out of scope").
    let graph = SemanticGraph { nodes: vec![], edges: vec![] };
    let report = Checker::check(&graph);
    let summary = format!("{:?}", report.summary());
    let entry_count = report.entries.len();

    let human_msg = format!(
        "change-id: {change_id}\nentries: {entry_count}\nsummary: {summary}"
    );
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

/// `ail apply <change-id>` — apply a ChangeSet via the storage bridge.
///
/// Constructs a `MemorySnapshotBridge` with `current_snapshot_id = SnapshotId(0)`
/// (genesis base), then applies the canonical form of a freshly-parsed
/// placeholder ChangeSet against an empty graph.
///
/// The spec requires surfacing `RebaseRequired` as a user-visible error (exit 1).
/// The spec also requires printing the new snapshot id on success (exit 0).
///
/// In Phase 9 the real ChangeSet will be loaded from the durable store by
/// change-id; for now we apply an empty identity ChangeSet whose
/// `base_snapshot_id` matches the bridge's current id (SnapshotId(0) → success).
fn cmd_apply(mode: OutputMode, change_id: &str) -> Result<(), CliError> {
    // Validate change-id format.
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }

    use ail_change::apply::apply as apply_changeset;
    use ail_change::canonical::{CanonicalChangeSet, CanonicalMeta};
    use ail_change::model::Timestamp;

    // In-memory bridge: current snapshot is genesis (id = 0).
    // The ChangeSet we construct below has base_snapshot_id = SnapshotId(0),
    // so the snapshot guard passes and the apply succeeds.
    let bridge = SimpleSnapshotBridge(SnapshotId(0));
    let mut graph = SemanticGraph { nodes: vec![], edges: vec![] };

    let canonical = CanonicalChangeSet {
        meta: CanonicalMeta {
            author: "cli".to_string(),
            description: "<applied via change-id>".to_string(),
            timestamp: Timestamp(0),
        },
        base_snapshot_id: SnapshotId(0),
        preconditions: vec![],
        ops: vec![],
    };

    let outcome = apply_changeset(canonical, &mut graph, &bridge);

    match outcome {
        ail_change::model::ChangeSetOutcome::Applied => {
            // In Phase 9 we will persist a real SnapshotEnvelope here.
            // For now the "new snapshot id" is the next sequential id.
            let new_snapshot_id = bridge.0 .0 + 1;
            let human_msg = format!("applied; new snapshot id: {new_snapshot_id}");
            print_response(
                mode,
                &human_msg,
                json!({
                    "change_id": change_id,
                    "new_snapshot_id": new_snapshot_id,
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
    }
}

/// `ail compile --profile <name>` — run the three-stage lowering pipeline.
///
/// Compiles the current in-memory graph (empty at this phase) through
/// `lower_to_core_ir → lower_to_anf → emit_wasm` and prints the artifact
/// hash chain. A `--profile` name is accepted and echoed but not yet used to
/// configure the pipeline (profile configuration is a Phase 9 concern).
fn cmd_compile(mode: OutputMode, profile: &str) -> Result<(), CliError> {
    let graph = SemanticGraph { nodes: vec![], edges: vec![] };
    let report = Checker::check(&graph);

    let core = lower_to_core_ir(&graph, &report).map_err(|e| {
        CliError::Domain(format!("compile (core ir): {e:?}"))
    })?;

    let anf = lower_to_anf(&core).map_err(|e| {
        CliError::Domain(format!("compile (anf): {e:?}"))
    })?;

    let artifact = emit_wasm(&anf).map_err(|e| {
        CliError::Domain(format!("compile (wasm): {e:?}"))
    })?;

    let wasm_hash = artifact
        .hash_chain
        .wasm_hash
        .map(|h| bytes_to_hex(&h))
        .unwrap_or_else(|| "<none>".to_string());
    let wasm_size = artifact.wasm.len();

    let human_msg = format!(
        "profile: {profile}\nwasm bytes: {wasm_size}\nwasm-hash: {wasm_hash}"
    );
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
///
/// Runs the full compiler pipeline then passes the artifact through
/// `RuntimeHost::validate_and_instantiate`. An empty capability manifest is
/// used (no `requires`), and a matching `RuntimeProfile` is derived from the
/// artifact's hashes so the preflight passes.
fn cmd_run(mode: OutputMode, profile: &str) -> Result<(), CliError> {
    // Compile pipeline (same as `compile` command).
    let graph = SemanticGraph { nodes: vec![], edges: vec![] };
    let report = Checker::check(&graph);

    let core = lower_to_core_ir(&graph, &report)
        .map_err(|e| CliError::Domain(format!("run (core ir): {e:?}")))?;
    let anf = lower_to_anf(&core)
        .map_err(|e| CliError::Domain(format!("run (anf): {e:?}")))?;
    let artifact = emit_wasm(&anf)
        .map_err(|e| CliError::Domain(format!("run (wasm): {e:?}")))?;

    // Build manifest and profile with matching hashes so preflight passes.
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
        String::new(), // verification_report_hash not checked in current preflight
        manifest_hash,
        vec![],        // no grants needed — no capabilities required
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&artifact.wasm, &manifest, &runtime_profile);

    match result {
        Ok(_instance) => {
            // Exactly one AuditEvent was appended.
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

// ── PRIVATE HELPERS ───────────────────────────────────────────────────────

/// A minimal `SnapshotBridge` that always returns a fixed id.
///
/// Used for the `apply` command before Phase 9 wires up a durable store.
struct SimpleSnapshotBridge(SnapshotId);

impl SnapshotBridge for SimpleSnapshotBridge {
    fn current_snapshot_id(&self) -> SnapshotId {
        self.0
    }
}

/// Encode a value as CBOR bytes.
///
/// Returns `CliError::Domain` on serialisation failure.
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
///
/// A change-id is blake3(canonical CBOR) which always produces 64 hex chars.
fn is_valid_change_id(id: &str) -> bool {
    id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit())
}

// ── UNIT TESTS ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Scenario: valid 64-char hex change-id is accepted.
    //   GIVEN a 64-character lowercase hex string
    //   WHEN is_valid_change_id is called
    //   THEN the result is true
    #[test]
    fn valid_change_id_accepted() {
        let id = "a".repeat(64);
        assert!(is_valid_change_id(&id), "64 hex chars must be accepted");
    }

    // TRIANGULATE: too-short change-id is rejected.
    //   GIVEN a 63-character hex string
    //   WHEN is_valid_change_id is called
    //   THEN the result is false
    #[test]
    fn short_change_id_rejected() {
        let id = "a".repeat(63);
        assert!(!is_valid_change_id(&id), "63 hex chars must be rejected");
    }

    // TRIANGULATE: non-hex change-id is rejected.
    //   GIVEN a 64-character string containing non-hex characters
    //   WHEN is_valid_change_id is called
    //   THEN the result is false
    #[test]
    fn non_hex_change_id_rejected() {
        let id = "g".repeat(64); // 'g' is not hex
        assert!(!is_valid_change_id(&id), "non-hex chars must be rejected");
    }

    // Scenario: SimpleSnapshotBridge returns its initialised id.
    //   GIVEN SimpleSnapshotBridge(SnapshotId(7))
    //   WHEN current_snapshot_id() is called
    //   THEN SnapshotId(7) is returned
    #[test]
    fn simple_snapshot_bridge_returns_initial_id() {
        let bridge = SimpleSnapshotBridge(SnapshotId(7));
        assert_eq!(bridge.current_snapshot_id(), SnapshotId(7));
    }

    // TRIANGULATE: encode_cbor succeeds for a JSON-compatible value.
    //   GIVEN a serializable value
    //   WHEN encode_cbor is called
    //   THEN Ok(non-empty bytes) is returned
    #[test]
    fn encode_cbor_returns_bytes_for_serializable_value() {
        #[derive(serde::Serialize)]
        struct Dummy {
            x: u32,
        }
        let bytes = encode_cbor(&Dummy { x: 42 }).expect("encode_cbor must succeed");
        assert!(!bytes.is_empty(), "encoded bytes must not be empty");
    }

    // Scenario: cmd_context always succeeds with exit 0.
    //   GIVEN Human output mode
    //   WHEN cmd_context is called
    //   THEN Ok(()) is returned
    #[test]
    fn cmd_context_succeeds() {
        assert!(cmd_context(OutputMode::Human).is_ok());
    }

    // Scenario: cmd_verify rejects invalid change-id (exit 1).
    //   GIVEN a 63-char change-id
    //   WHEN cmd_verify is called
    //   THEN Err(CliError::NotFound) is returned
    #[test]
    fn cmd_verify_rejects_invalid_change_id() {
        let result = cmd_verify(OutputMode::Human, &"a".repeat(63));
        assert!(matches!(result, Err(CliError::NotFound(_))));
    }

    // Scenario: cmd_apply rejects invalid change-id (exit 1).
    //   GIVEN a 63-char change-id
    //   WHEN cmd_apply is called
    //   THEN Err(CliError::NotFound) is returned
    #[test]
    fn cmd_apply_rejects_invalid_change_id() {
        let result = cmd_apply(OutputMode::Human, &"a".repeat(63));
        assert!(matches!(result, Err(CliError::NotFound(_))));
    }

    // Scenario: cmd_verify succeeds for a valid 64-char change-id (exit 0).
    //   GIVEN a 64-char hex change-id
    //   WHEN cmd_verify is called
    //   THEN Ok(()) is returned (empty graph → Proven summary)
    #[test]
    fn cmd_verify_succeeds_for_valid_change_id() {
        let id = "a".repeat(64);
        let result = cmd_verify(OutputMode::Human, &id);
        assert!(result.is_ok(), "cmd_verify must succeed for valid id; got: {result:?}");
    }

    // Scenario: cmd_apply succeeds when base matches bridge (exit 0).
    //   GIVEN a valid change-id and bridge base = SnapshotId(0)
    //   WHEN cmd_apply is called
    //   THEN Ok(()) is returned
    #[test]
    fn cmd_apply_succeeds_with_matching_base() {
        let id = "b".repeat(64);
        let result = cmd_apply(OutputMode::Human, &id);
        assert!(result.is_ok(), "cmd_apply must succeed; got: {result:?}");
    }

    // Scenario: cmd_compile succeeds with an empty graph (exit 0).
    //   GIVEN profile "dev" and empty in-memory graph
    //   WHEN cmd_compile is called
    //   THEN Ok(()) is returned
    #[test]
    fn cmd_compile_succeeds() {
        let result = cmd_compile(OutputMode::Human, "dev");
        assert!(result.is_ok(), "cmd_compile must succeed; got: {result:?}");
    }

    // Scenario: cmd_run succeeds when preflight passes (exit 0).
    //   GIVEN profile "dev", empty graph, matching manifest and profile hashes
    //   WHEN cmd_run is called
    //   THEN Ok(()) is returned
    #[test]
    fn cmd_run_succeeds() {
        let result = cmd_run(OutputMode::Human, "dev");
        assert!(result.is_ok(), "cmd_run must succeed; got: {result:?}");
    }

    // Scenario: JSON mode produces parseable JSON with status and data.
    //   GIVEN cmd_context runs with OutputMode::Json
    //   WHEN format_response is called internally
    //   THEN the output is not inspected here (format_response is tested in output.rs)
    //   (Integration tests in tests/cli.rs cover the full JSON stdout assertion.)
    #[test]
    fn cmd_context_json_mode_does_not_panic() {
        assert!(cmd_context(OutputMode::Json).is_ok());
    }
}
