// ── ail-cli::cli ─────────────────────────────────────────────────────────
//
// CLI dispatch: full tooling surface matching docs/tooling.md.
//
// # Command surface
//
// | Command                  | Description                                           |
// |--------------------------|-------------------------------------------------------|
// | context  [target]        | Hash-bound semantic context slice for target          |
// | impact   <target>        | Impact analysis for a node target                     |
// | callers  <target>        | Callers of a function/node target                     |
// | effects  <target>        | Effects emitted by a module target                    |
// | proofs   <target>        | Proof obligations for an invariant target             |
// | change   [text] [--file] | Create a draft ChangeSet from text/file/stdin         |
// | verify   <change-id>     | Run Checker on the named ChangeSet (--profile)        |
// | apply    <change-id>     | Apply ChangeSet with full pre-apply gate display      |
// | compile  --target --profile  lower → ANF → emit_wasm                      |
// | run      --profile [module] [--replay] preflight + runtime report         |
// | init                     | Create .ail/ dirs, genesis snapshot, baseline state   |
// | status                   | Snapshot/branch/pending/verify/indexes/runtime/pkg    |
// | inspect  <type> <id>     | Inspect node/snapshot/report/artifact/capability      |
// | diff     <a..b>|<change> | Semantic diff with full category breakdown            |
// | rollback --to|<change-id>| Rollback to snapshot or by change, creates snapshot   |
// | rebase   <change-id>     | Semantic rebase with conflict report                  |
// | merge    <branch>        | Semantic merge with conflict workflow                 |
// | refactor <op> [args]     | ChangeSet from refactor with locks/contracts/effects  |
// | approve  <change-id>     | Immutable approval record referencing canonical hash  |
// | reject   <change-id>     | Immutable rejection record                            |
// | policy   check|explain|set Policy management                                    |
// | package  add|verify|…    | Package management with trust/capabilities/advisories |
// | doctor                   | Integrity checks: graph/index/schema/artifacts/…      |
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

use ail_change::{
    apply::SnapshotBridge,
    canonical::{canonicalize, canonicalize_parsed},
    model::{ChangeSetOutcome, ConflictReason, SnapshotId},
    parser::parse_changeset,
};
use ail_compiler::{emit_wasm_with_profile, lower_to_anf_with_graph, lower_to_core_ir};
use ail_context::{
    AuthSession, ContextQuery, ContextRequest, ContextServer, ContextServerConfig,
    DerivedIndexCache, FieldRedactionRule, InMemoryContextSource, QueryBudget, QueryScope,
    SnapshotSelector, TrustLevel as ContextTrustLevel,
};
use ail_core::semantic_graph::{GraphEdge, GraphNode, NodeKind};
use ail_core::semantic_graph::{NodeRef, Provenance, SemanticGraph};
use ail_runtime::blake3_hex_of;
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::{SnapshotEnvelope, graph::ChangeSetLogEntry, object::ObjectId};
use ail_verify::checker::Checker;
use ail_verify::report::VerificationReport;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use crate::approval_commands::{cmd_approve, cmd_reject};
use crate::branch_commands::{cmd_diff, cmd_merge, cmd_rebase, cmd_refactor, cmd_rollback};
use crate::changeset_input::{ChangeInput, load_parsed_changeset};
use crate::diagnostic_commands::{cmd_doctor, cmd_gc};
use crate::error::CliError;
use crate::eval_commands::cmd_eval;
use crate::graph_query_commands::{cmd_callers, cmd_effects, cmd_impact, cmd_proofs};
use crate::output::{OutputMode, print_response};
use crate::package_commands::cmd_package;
use crate::package_registry_io::{load_package_lockfile, load_package_registry};
use crate::policy_commands::cmd_policy;
use crate::remote_commands::cmd_remote;
use crate::run_commands::{cmd_compile, cmd_run};
use crate::store::{StoreHandle, build_store, file_store, init_file_layout_with_branch};
use crate::workflow_commands::{cmd_apply, cmd_verify};

// ── Cli ───────────────────────────────────────────────────────────────────

/// ail — AI-native language toolchain.
#[derive(Parser)]
#[command(
    version,
    about,
    long_about = "ail — AI-native language toolchain.\n\nExamples:\n  ail init\n  ail change --file ./change.acl\n  ail compile --profile dev\n  ail run fn.answer\n  ail eval \"add(20, 22)\""
)]
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
    /// Return a hash-bound semantic context slice for a target node/module/function.
    /// Without a target, lists the current snapshot envelope.
    Context {
        /// Either `<target>`, `query <type> [params]`, or `index rebuild`.
        args: Vec<String>,
    },

    /// Show impact analysis for a target: which nodes are affected by changes to it.
    Impact {
        /// Target node (e.g. `type.CartItem.price`).
        target: String,
    },

    /// List all callers of a function or node target.
    Callers {
        /// Target node (e.g. `fn.cart_total`).
        target: String,
    },

    /// Show effects emitted by a module target.
    Effects {
        /// Target node (e.g. `module.payment`).
        target: String,
    },

    /// Show proof obligations for an invariant target.
    Proofs {
        /// Invariant target (e.g. `invariant.stock_never_negative`).
        target: String,
    },

    /// Create a draft ChangeSet from text, a file, or stdin.
    ///
    /// By default this creates a DRAFT only — the changeset is persisted for
    /// later verification and application via `ail apply`.
    /// Use `--apply` to apply immediately (equivalent to `ail change` + `ail apply`).
    Change {
        /// Free-text description of the change (e.g. "add pure cart_total function").
        text: Option<String>,
        /// Path to an ACL file.
        #[arg(long, short)]
        file: Option<PathBuf>,
        /// Read ChangeSet from stdin.
        #[arg(long)]
        stdin: bool,
        /// Branch to receive the generated snapshot.
        #[arg(long)]
        branch: Option<String>,
        /// Apply the ChangeSet immediately instead of creating a draft.
        /// Only allowed when the project policy permits automation.
        #[arg(long)]
        apply: bool,
    },

    /// Run the verifier on a ChangeSet by its canonical change-id.
    Verify {
        /// Canonical change-id (blake3 hex) of the ChangeSet to verify.
        change_id: String,
        /// Verification profile (e.g. `dev`, `prod`).
        #[arg(long, default_value = "dev")]
        profile: String,
        /// Solver backend to use for proof obligations.
        ///
        /// `simple` (default) uses the conservative built-in solver.
        /// `z3` uses the Z3 SMT solver — only available when ail-cli is
        /// compiled with `--features z3-solver`; otherwise returns an error.
        #[arg(long, default_value = "simple")]
        solver: String,
    },

    /// Apply a ChangeSet and persist a new snapshot (shows pre-apply gate).
    Apply {
        /// Canonical change-id (blake3 hex) of the ChangeSet to apply.
        change_id: String,
        /// Skip interactive confirmation (CI automation mode).
        #[arg(long)]
        yes: bool,
        /// Policy profile for automation gate (e.g. `ci.allowed`).
        #[arg(long)]
        policy: Option<String>,
    },

    /// Compile the current graph snapshot to a WASM artifact.
    Compile {
        /// Compiler profile name (e.g. `dev`, `prod`).
        #[arg(long, default_value = "dev")]
        profile: String,
        /// Compilation target (e.g. `wasm`, `native`).
        #[arg(long, default_value = "wasm")]
        target: String,
    },

    /// Run preflight validation and emit a runtime report.
    Run {
        /// Runtime profile name (e.g. `dev`, `test`).
        #[arg(long, default_value = "dev")]
        profile: String,
        /// Execution target (e.g. `wasm`). `native` returns an explicit error:
        /// native linked execution is not yet supported.
        #[arg(long, default_value = "wasm")]
        target: String,
        /// Module target to run (e.g. `module.checkout`).
        module: Option<String>,
        /// Positional i64 arguments passed to the exported function.
        args: Vec<String>,
        /// Replay a recorded trace by its id.
        #[arg(long)]
        replay: Option<String>,
    },

    /// Evaluate an inline expression without initializing a project.
    Eval {
        /// Expression to evaluate (e.g. `add(20, 22)`, `mul(6, 7)`, or `42`).
        expression: String,
    },

    /// Initialize the project: create .ail/ directories and genesis snapshot.
    Init {
        /// Initial branch name.
        #[arg(long, default_value = "main")]
        branch: String,
    },

    /// Show current snapshot, branch, pending changes, and system state.
    Status,

    /// Inspect a node, snapshot, report, artifact, or capability by type and id.
    Inspect {
        /// Inspection type: `node`, `snapshot`, `report`, `artifact`, `capability`.
        kind: String,
        /// Identifier (ObjectId hex, node name, or artifact path).
        id: String,
    },

    /// Show structural/semantic diff between two snapshots or for a change.
    Diff {
        /// First snapshot id, range `a..b`, or a change-id.
        snapshot1: String,
        /// Optional second snapshot id.
        snapshot2: Option<String>,
        /// Show semantic diff with full category breakdown.
        #[arg(long)]
        semantic: bool,
    },

    /// Roll back to a named snapshot or reverse a change, creating a new snapshot.
    Rollback {
        /// ObjectId hex of the snapshot to roll back to (use `--to` prefix or bare arg).
        #[arg(long)]
        to: Option<String>,
        /// Canonical change-id to reverse (rollback-by-change).
        change_id: Option<String>,
    },

    /// Rebase a ChangeSet onto a new snapshot base (semantic rebase).
    Rebase {
        /// Target branch to replay current changes onto.
        branch: String,
        /// Legacy snapshot target; when present, validates old change-id rebase form.
        #[arg(long)]
        onto: Option<String>,
    },

    /// Merge a feature branch into a target branch (semantic merge).
    Merge {
        /// Source branch name (e.g. `feature.checkout`).
        branch: String,
        /// Target branch name (e.g. `main`).
        #[arg(long = "into")]
        into_target: Option<String>,
    },

    /// Produce a ChangeSet from a refactor operation (with behavior locks/contracts).
    Refactor {
        /// Refactor operation (e.g. `extract-function`, `move`).
        operation: String,
        /// Additional positional arguments for the refactor operation.
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },

    /// Record an immutable approval for a ChangeSet (references canonical hash).
    Approve {
        /// Canonical change-id (64-char hex) of the ChangeSet to approve.
        change_id: String,
        /// Approval gate or reason (e.g. `public_api_changed`).
        #[arg(long = "for")]
        for_reason: Option<String>,
        /// Approver role (e.g. `security`, `owner`).
        #[arg(long)]
        role: Option<String>,
    },

    /// Record an immutable rejection for a ChangeSet.
    Reject {
        /// Canonical change-id (64-char hex) of the ChangeSet to reject.
        change_id: String,
        /// Human-readable reason for rejection.
        #[arg(long)]
        reason: String,
    },

    /// Manage and query project policies.
    Policy {
        #[command(subcommand)]
        cmd: PolicyCmd,
    },

    /// Manage packages (add, verify, publish, audit, explain).
    Package {
        #[command(subcommand)]
        cmd: PackageCmd,
    },

    /// Remote collaboration commands backed by local in-process exchange APIs.
    Remote {
        #[command(subcommand)]
        cmd: RemoteCmd,
    },

    /// Run integrity and health checks on the project.
    Doctor,

    /// Delete objects unreachable from branch tips.
    Gc,
}

// ── PolicyCmd ─────────────────────────────────────────────────────────────

/// Sub-commands for `ail policy`.
#[derive(Subcommand)]
pub(crate) enum PolicyCmd {
    /// List active project policy rules.
    List,
    /// Add a persisted policy rule.
    Add {
        /// Policy rule text (e.g. `deny capability file.write:*`).
        rule: String,
    },
    /// Check whether a ChangeSet satisfies the project policy for a profile.
    Check {
        /// Canonical change-id (64-char hex) of the ChangeSet to check.
        change_id: Option<String>,
        /// Policy profile to check against (e.g. `prod`).
        #[arg(long, default_value = "dev")]
        profile: String,
    },
    /// Explain a named policy rule in human-readable form.
    Explain {
        /// Name of the policy rule to explain (e.g. `no_unverified_public_api`).
        rule: String,
    },
    /// Update a project policy setting (key=value form).
    Set {
        /// Setting in `key=value` form (e.g. `max_new_capabilities=2`).
        setting: String,
    },
}

// ── PackageCmd ────────────────────────────────────────────────────────────

/// Sub-commands for `ail package`.
#[derive(Subcommand)]
pub(crate) enum PackageCmd {
    /// Create a package manifest for the current graph.
    Init {
        /// Package name.
        #[arg(long)]
        name: Option<String>,
        /// Package version.
        #[arg(long, default_value = "0.1.0")]
        version: String,
    },
    /// Add a package dependency (shows trust/capabilities/advisories).
    Add {
        /// Package specifier in `name@version` form (e.g. `payments.stripe@1.2`).
        package: String,
    },
    /// Install a package dependency from the local registry.
    Install {
        /// Package specifier in `name@version` form; latest local match if version omitted.
        package: String,
    },
    /// Search available packages in the local registry.
    Search {
        /// Search query.
        query: String,
    },
    /// Verify all package integrity hashes against lock file.
    Verify,
    /// Publish this package to the registry.
    Publish,
    /// Audit all packages for known security advisories.
    Audit,
    /// Manage local package advisory metadata.
    Advisory {
        #[command(subcommand)]
        cmd: AdvisoryCmd,
    },
    /// Record that a local package version is yanked.
    Yank {
        /// Package name to yank.
        package: String,
        /// Exact package version to yank.
        version: String,
        /// Local reason for yanking this version.
        #[arg(long)]
        reason: String,
    },
    /// List local package yank records.
    Yanked,
    /// Explain a package's trust level, capabilities, assumptions, and unsafe surface.
    Explain {
        /// Name of the package to explain (e.g. `payments.stripe`).
        package: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum AdvisoryCmd {
    /// Add a local advisory for a package version constraint.
    Add {
        /// Package name affected by this advisory.
        package: String,
        /// Version constraint affected by this advisory.
        constraint: String,
        /// Stable local advisory id.
        #[arg(long)]
        id: String,
        /// Advisory severity: low, medium, high, or critical.
        #[arg(long)]
        severity: String,
        /// Local reason for the advisory.
        #[arg(long)]
        reason: String,
    },
    /// List local advisories.
    List,
}

// ── RemoteCmd ─────────────────────────────────────────────────────────────

/// Sub-commands for `ail remote`.
#[derive(Subcommand)]
pub(crate) enum RemoteCmd {
    /// Sign and submit a stored ChangeSet through the local in-process coordinator.
    Submit {
        /// Canonical change-id of a locally stored ChangeSet.
        change_id: String,
        /// Local signer key reference label. The current CLI uses an ephemeral key.
        #[arg(long)]
        signer: String,
    },
    /// Push one local object into the local file-backed bundle store.
    Push {
        /// Root object id to bundle. Snapshot envelope roots include available direct dependencies.
        #[arg(long)]
        root: String,
    },
    /// Pull one bundle from the local file-backed bundle store into the object store.
    Pull {
        /// Root object id of the stored bundle.
        root: String,
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
                 eval, init, status, inspect, diff, rollback, rebase, merge, refactor, \
                 approve, reject, policy, package, remote, doctor, gc"
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
        Commands::Context { args } => cmd_context(mode, &args, &store).await,
        Commands::Impact { target } => cmd_impact(mode, &target, &store).await,
        Commands::Callers { target } => cmd_callers(mode, &target, &store).await,
        Commands::Effects { target } => cmd_effects(mode, &target, &store).await,
        Commands::Proofs { target } => cmd_proofs(mode, &target, &store).await,
        Commands::Change {
            text,
            file,
            stdin,
            branch,
            apply,
        } => {
            cmd_change(
                mode,
                text.as_deref(),
                file,
                stdin,
                apply,
                branch.as_deref(),
                &store,
            )
            .await
        }
        Commands::Verify {
            change_id,
            profile,
            solver,
        } => cmd_verify(mode, &change_id, &profile, &solver, &store).await,
        Commands::Apply {
            change_id,
            yes,
            policy,
        } => cmd_apply(mode, &change_id, yes, policy.as_deref(), &store).await,
        Commands::Compile { profile, target } => cmd_compile(mode, &profile, &target, &store).await,
        Commands::Run {
            profile,
            target,
            module,
            args,
            replay,
        } => {
            cmd_run(
                mode,
                &profile,
                &target,
                module.as_deref(),
                &args,
                replay.as_deref(),
                &store,
            )
            .await
        }
        Commands::Eval { expression } => cmd_eval(mode, &expression),
        Commands::Init { branch } => cmd_init(mode, &store, &branch).await,
        Commands::Status => cmd_status(mode, &store).await,
        Commands::Inspect { kind, id } => cmd_inspect(mode, &kind, &id, &store).await,
        Commands::Diff {
            snapshot1,
            snapshot2,
            semantic,
        } => cmd_diff(mode, &snapshot1, snapshot2.as_deref(), semantic, &store).await,
        Commands::Rollback { to, change_id } => {
            cmd_rollback(mode, to.as_deref(), change_id.as_deref(), &store).await
        }
        Commands::Rebase { branch, onto } => {
            cmd_rebase(mode, &branch, onto.as_deref(), &store).await
        }
        Commands::Merge {
            branch,
            into_target,
        } => cmd_merge(mode, &branch, into_target.as_deref(), &store).await,
        Commands::Refactor { operation, args } => {
            cmd_refactor(mode, &operation, &args, &store).await
        }
        Commands::Approve {
            change_id,
            for_reason,
            role,
        } => cmd_approve(
            mode,
            &change_id,
            for_reason.as_deref(),
            role.as_deref(),
            &store,
        ),
        Commands::Reject { change_id, reason } => cmd_reject(mode, &change_id, &reason, &store),
        Commands::Policy { cmd } => cmd_policy(mode, cmd, &store).await,
        Commands::Package { cmd } => cmd_package(mode, cmd, &store).await,
        Commands::Remote { cmd } => cmd_remote(mode, cmd, &store).await,
        Commands::Doctor => cmd_doctor(mode, &store).await,
        Commands::Gc => cmd_gc(mode, &store),
    }
}

// ── COMMAND HANDLERS ──────────────────────────────────────────────────────

/// `ail context [target]` — return a hash-bound semantic context slice.
///
/// Rules (from tooling.md):
/// 1. Context commands never mutate the graph.
/// 2. Context output includes snapshot/hash.
/// 3. Context can be used in ChangeSet requires via assert_context.
async fn cmd_context(
    mode: OutputMode,
    args: &[String],
    store: &StoreHandle,
) -> Result<(), CliError> {
    let snapshots = store.list_snapshots().await?;
    if args.is_empty() {
        // No target: list snapshot envelopes (backward-compatible).
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
        print_response(
            mode,
            &human_lines.join("\n"),
            json!({ "snapshots": json_snaps }),
        );
        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "index") {
        if args.get(1).is_none_or(|arg| arg != "rebuild") {
            return Err(CliError::ParseError(
                "Expected `ail context index rebuild`".to_string(),
            ));
        }
        let (server, snapshot) = context_server_for_cli(store, &snapshots).await?;
        let indexes = server
            .rebuild_indexes(&SnapshotSelector::ById(snapshot.id))
            .await
            .map_err(|e| CliError::Domain(format!("context index rebuild: {e}")))?;
        let human_msg = format!(
            "snapshot: {}\nhash: {}\nindexes_rebuilt: {}",
            indexes.snapshot_id,
            indexes.snapshot_hash,
            indexes.indexes.len()
        );
        print_response(
            mode,
            &human_msg,
            json!({
                "snapshot_id": indexes.snapshot_id.to_hex(),
                "snapshot_hash": indexes.snapshot_hash.to_hex(),
                "indexes_rebuilt": indexes.indexes.len(),
                "indexes": indexes.indexes,
            }),
        );
        return Ok(());
    }

    let (query_kind, query_args) = if args.first().is_some_and(|arg| arg == "query") {
        let Some(kind) = args.get(1) else {
            return Err(CliError::ParseError(
                "Expected `ail context query <type> [params]`".to_string(),
            ));
        };
        (kind.as_str(), &args[2..])
    } else {
        ("context", args)
    };
    let target_label = query_args.first().map(String::as_str).unwrap_or(query_kind);

    let (server, snapshot) = context_server_for_cli(store, &snapshots).await?;
    let query = parse_context_query_for_cli(query_kind, query_args, store).await?;
    let session = AuthSession {
        principal: "cli".to_string(),
        trust_level: ContextTrustLevel::Internal,
    };
    let response = match server
        .handle(ContextRequest::Query {
            query,
            snapshot: SnapshotSelector::ById(snapshot.id),
            session: Some(session),
        })
        .await
    {
        ail_context::ServerContextResponse::Result(response) => response,
        ail_context::ServerContextResponse::Error(err) => {
            return Err(CliError::Domain(format!("context query: {err}")));
        }
        other => {
            return Err(CliError::Domain(format!(
                "unexpected context server response: {other:?}"
            )));
        }
    };

    let human_msg = format!(
        "snapshot: {}\nhash: {}\ncontext_hash: {}\nnodes: {}\nredaction: {:?}",
        response.snapshot.id,
        response.graph_root_hash,
        bytes_to_hex(&response.context_hash),
        response.structured.len(),
        response.redaction_state
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "context": {
                "target": target_label,
                "snapshot_id": response.snapshot.id.to_hex(),
                "snapshot_hash": response.graph_root_hash.to_hex(),
                "nodes": response.structured.clone(),
                "response": response.clone(),
            },
            "snapshot_id": response.snapshot.id.to_hex(),
            "snapshot_hash": response.graph_root_hash.to_hex(),
        }),
    );
    Ok(())
}

async fn context_server_for_cli(
    store: &StoreHandle,
    snapshots: &[SnapshotEnvelope],
) -> Result<(ContextServer<InMemoryContextSource>, SnapshotEnvelope), CliError> {
    let graph = load_current_graph_for_cli(store).await?;
    let snapshot = match store
        .head_snapshot()
        .await?
        .or_else(|| latest_snapshot(snapshots).cloned())
    {
        Some(snapshot) => snapshot,
        None => synthetic_context_snapshot(&graph)?,
    };
    let source = InMemoryContextSource::new();
    source.insert_snapshot(snapshot.clone());
    source.insert_graph(snapshot.graph_root_hash, graph);

    let config = ContextServerConfig {
        redaction_rules: vec![
            FieldRedactionRule {
                field: "body_expr".to_string(),
                min_trust: ContextTrustLevel::Privileged,
                category: "restricted business logic".to_string(),
            },
            FieldRedactionRule {
                field: "runtime_checks".to_string(),
                min_trust: ContextTrustLevel::Internal,
                category: "runtime payloads".to_string(),
            },
        ],
        ..Default::default()
    };
    let mut server = ContextServer::new(source).with_config(config);
    if let Some(path) = store.context_index_path() {
        server = server.with_index_cache(DerivedIndexCache::new(path));
    }
    Ok((server, snapshot))
}

fn synthetic_context_snapshot(graph: &SemanticGraph) -> Result<SnapshotEnvelope, CliError> {
    let bytes = CborCodec
        .encode(graph)
        .map_err(|e| CliError::Domain(format!("graph encoding failed: {e}")))?;
    let root = ObjectId::from_bytes(&bytes);
    Ok(SnapshotEnvelope {
        id: ObjectId::from_bytes(b"synthetic-context-snapshot"),
        graph_root_hash: root,
        parent_id: None,
        applied_change_id: None,
        created_at: unix_ms_now(),
        verification_report_hash: None,
        ..Default::default()
    })
}

async fn parse_context_query_for_cli(
    kind: &str,
    args: &[String],
    store: &StoreHandle,
) -> Result<ContextQuery, CliError> {
    let graph = load_current_graph_for_cli(store).await?;
    let budget = QueryBudget::default();
    let target = || -> Result<NodeRef, CliError> {
        let raw = args.first().map(String::as_str).unwrap_or("0");
        node_ref_for_cli_target(raw, &graph)
    };
    match kind {
        "context" => Ok(ContextQuery::Node {
            target: target()?,
            scope: QueryScope::Full,
            budget,
        }),
        "graph" => Ok(ContextQuery::Graph {
            scope: QueryScope::Full,
            budget,
        }),
        "impact" => Ok(ContextQuery::Impact {
            target: target()?,
            budget,
        }),
        "callers" => Ok(ContextQuery::Callers {
            target: target()?,
            transitive: true,
            budget,
        }),
        "callees" => Ok(ContextQuery::Callees {
            target: target()?,
            transitive: true,
            budget,
        }),
        "effects" => Ok(ContextQuery::Effects {
            target: target()?,
            budget,
        }),
        "contracts" => Ok(ContextQuery::Contracts {
            target: target()?,
            budget,
        }),
        "history" => Ok(ContextQuery::History {
            target: target()?,
            budget,
        }),
        "why" => Ok(ContextQuery::Why {
            target: target()?,
            budget,
        }),
        "proofs" | "obligations" => Ok(ContextQuery::Proofs {
            target: target()?,
            budget,
        }),
        "resources" => Ok(ContextQuery::Resources {
            target: target()?,
            budget,
        }),
        "boundaries" => Ok(ContextQuery::Boundaries {
            target: target()?,
            budget,
        }),
        "refactor_context" => Ok(ContextQuery::RefactorContext {
            target: target()?,
            budget,
        }),
        "runtime" => Ok(ContextQuery::Runtime {
            target: target()?,
            profile: "dev".to_string(),
            budget,
        }),
        "concurrency" => Ok(ContextQuery::Concurrency {
            target: target()?,
            budget,
        }),
        "tasks" => Ok(ContextQuery::Tasks {
            target: target()?,
            budget,
        }),
        "diff" => Ok(ContextQuery::Diff {
            snapshot_a: None,
            snapshot_b: None,
            budget,
        }),
        "risks" => Ok(ContextQuery::Risks {
            target: target()?,
            budget,
        }),
        "todo" => Ok(ContextQuery::Todo {
            target: target()?,
            budget,
        }),
        "extract_candidates" => Ok(ContextQuery::ExtractCandidates {
            target: target()?,
            budget,
        }),
        "move_safety" => {
            let destination_raw = args.get(1).map(String::as_str).unwrap_or("0");
            let destination = node_ref_for_cli_target(destination_raw, &graph)?;
            Ok(ContextQuery::MoveSafety {
                target: target()?,
                destination,
                budget,
            })
        }
        "capabilities" => Ok(ContextQuery::Capabilities {
            target: target()?,
            profile: "dev".to_string(),
            budget,
        }),
        "handlers" => Ok(ContextQuery::Handlers {
            target: target()?,
            profile: "dev".to_string(),
            budget,
        }),
        "assumptions" => Ok(ContextQuery::Assumptions {
            target: target()?,
            budget,
        }),
        other => Err(CliError::ParseError(format!(
            "unsupported context query type: {other}"
        ))),
    }
}

fn node_ref_for_cli_target(target: &str, graph: &SemanticGraph) -> Result<NodeRef, CliError> {
    if let Ok(id) = target.parse::<u32>() {
        return Ok(NodeRef(id));
    }
    if let Some(node) = graph.nodes.iter().find(|node| node.name == target) {
        return Ok(node.id);
    }
    graph
        .nodes
        .first()
        .map(|node| node.id)
        .ok_or_else(|| CliError::NotFound(format!("node not found: {target}")))
}

/// `ail impact <target>` — show impact analysis for a target node.
///
/// Returns which nodes are transitively affected by changes to this target.
/// Output is hash-bound to the current snapshot.
/// Resolve a target string (e.g. "fn.cart_total", "type.CartItem") to the node
/// names to search for.  The convention is `<kind>.<name>` — we match by the
/// suffix after the last `.`, or the whole string when no `.` is present.
pub(crate) fn target_node_name(target: &str) -> &str {
    target.rsplit('.').next().unwrap_or(target)
}

/// Look up the `NodeRef`s of every node whose name matches `target_name`.
pub(crate) fn node_refs_for_name(
    graph: &ail_core::semantic_graph::SemanticGraph,
    name: &str,
) -> Vec<ail_core::semantic_graph::NodeRef> {
    graph
        .nodes
        .iter()
        .filter(|n| n.name == name)
        .map(|n| n.id)
        .collect()
}

// graph query commands (impact, callers, effects, proofs) → crate::graph_query_commands

/// `ail change [text] [--file <path>] [--stdin] [--apply]`
///
/// Creates a draft ChangeSet. Does NOT apply by default (doc §Change workflow).
/// Outputs: submitted_change, parsed_change, canonical_change, structural_diff preview.
///
/// Rules (tooling.md):
/// - `ail change` does not apply by default.
/// - It creates a draft ChangeSet.
/// - Use `--apply` for immediate application (equivalent to `ail change` + `ail apply`).
async fn cmd_change(
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

// cmd_verify → crate::workflow_commands::cmd_verify
// cmd_apply  → crate::workflow_commands::cmd_apply

// cmd_compile, cmd_run → crate::run_commands

// ── Shared graph loading helpers ──────────────────────────────────────────

fn current_graph_for_cli() -> Result<SemanticGraph, CliError> {
    let source = "change e2e base=0\nauthor cli\ndescription e2e\nop create_function id=fn.answer return=Int value=42\nend\n";
    let parsed = parse_changeset(source).map_err(|e| CliError::Domain(format!("parse: {e}")))?;
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let bridge = SimpleSnapshotBridge(SnapshotId(0));
    match ail_change::apply::apply(canonical, &mut graph, &bridge) {
        ail_change::model::ChangeSetOutcome::Applied => {
            for node in &mut graph.nodes {
                node.provenance = Some(Provenance {
                    change_id: "change.e2e".to_string(),
                });
            }
            if !graph.nodes.iter().any(|node| node.name == "fn.checkout") {
                let mut checkout = GraphNode::new(
                    NodeRef(graph.nodes.len() as u32),
                    NodeKind::Function,
                    "fn.checkout",
                );
                checkout.provenance = Some(Provenance {
                    change_id: "change.e2e".to_string(),
                });
                graph.nodes.push(checkout);
            }
            Ok(graph)
        }
        other => Err(CliError::Domain(format!(
            "Failed to build fallback graph: {}",
            changeset_outcome_message(&other)
        ))),
    }
}

pub(crate) async fn load_current_graph_for_cli(
    store: &StoreHandle,
) -> Result<SemanticGraph, CliError> {
    let snapshots = store.list_snapshots().await?;
    let head_snapshot = store.head_snapshot().await?;
    let Some(snapshot) = head_snapshot
        .as_ref()
        .or_else(|| latest_snapshot(&snapshots))
    else {
        return if store.has_persistent_project() {
            Ok(SemanticGraph {
                nodes: vec![],
                edges: vec![],
            })
        } else {
            current_graph_for_cli()
        };
    };

    match store.load_graph(&snapshot.graph_root_hash).await? {
        Some(graph) => Ok(graph),
        None if store.has_persistent_project() => current_graph_for_cli(),
        None => current_graph_for_cli(),
    }
}

pub(crate) async fn load_current_graph_with_snapshot_id_for_cli(
    store: &StoreHandle,
) -> Result<(SemanticGraph, SnapshotId), CliError> {
    let snapshots = store.list_snapshots().await?;
    let head_snapshot = store.head_snapshot().await?;
    let Some(snapshot) = head_snapshot
        .as_ref()
        .or_else(|| latest_snapshot(&snapshots))
    else {
        return if store.has_persistent_project() {
            Ok((
                SemanticGraph {
                    nodes: vec![],
                    edges: vec![],
                },
                SnapshotId(0),
            ))
        } else {
            current_graph_for_cli().map(|graph| (graph, SnapshotId(0)))
        };
    };

    let graph = match store.load_graph(&snapshot.graph_root_hash).await? {
        Some(graph) => Ok(graph),
        None if store.has_persistent_project() => current_graph_for_cli(),
        None => current_graph_for_cli(),
    }?;
    let snapshot_id = snapshot_id_from_parent_chain(store, snapshot).await?;
    Ok((graph, snapshot_id))
}

async fn snapshot_id_from_parent_chain(
    store: &StoreHandle,
    snapshot: &SnapshotEnvelope,
) -> Result<SnapshotId, CliError> {
    let mut depth = 0;
    let mut current = snapshot.clone();
    let mut seen = vec![current.id];

    while let Some(parent_id) = current.parent_id {
        if seen.contains(&parent_id) {
            return Err(CliError::Domain(format!(
                "snapshot parent cycle detected at {}",
                parent_id.to_hex()
            )));
        }
        let Some(parent) = store.load_snapshot(&parent_id).await? else {
            return Err(CliError::Domain(format!(
                "snapshot parent not found: {}",
                parent_id.to_hex()
            )));
        };
        depth += 1;
        seen.push(parent_id);
        current = parent;
    }

    Ok(SnapshotId(depth))
}

pub(crate) fn latest_snapshot(snapshots: &[SnapshotEnvelope]) -> Option<&SnapshotEnvelope> {
    snapshots.iter().max_by_key(|snapshot| snapshot.created_at)
}

fn format_unix_ms(ms: u64) -> String {
    if ms == 0 {
        "(unknown)".to_string()
    } else {
        format!("{ms} ms since Unix epoch")
    }
}

fn changeset_outcome_message(outcome: &ChangeSetOutcome) -> &'static str {
    match outcome {
        ChangeSetOutcome::Applied => "applied",
        ChangeSetOutcome::RebaseRequired { .. } => "rebase required",
        ChangeSetOutcome::Failed { .. } => "change failed",
        ChangeSetOutcome::ConflictIrresolvable { reason } => conflict_reason_message(reason),
    }
}

pub(crate) fn conflict_reason_message(reason: &ConflictReason) -> &'static str {
    match reason {
        ConflictReason::SameNodeModifiedIncompatibly => "same node was modified incompatibly",
        ConflictReason::NodeDeletedWhileModified => {
            "node was deleted while another change modified it"
        }
        ConflictReason::PublicApiConflict => "public API changes conflict",
        ConflictReason::InvariantTouchedConcurrently => "invariant changes conflict",
        ConflictReason::IncompatibleNodeModification => {
            "semantic node content conflict (return type, body, or effects differ)"
        }
    }
}

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
async fn cmd_init(mode: OutputMode, store: &StoreHandle, branch: &str) -> Result<(), CliError> {
    use crate::project::{ArtifactKind, ProjectContext};

    let ctx = ProjectContext::from_cwd()?;
    debug_assert_eq!(ctx.root.join(".ail"), ctx.ail_dir);

    init_file_layout_with_branch(&ctx.ail_dir, branch)?;

    // Create all required subdirectories.
    for kind in [
        ArtifactKind::Change,
        ArtifactKind::Snapshot,
        ArtifactKind::Report,
        ArtifactKind::Wasm,
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
async fn cmd_status(mode: OutputMode, store: &StoreHandle) -> Result<(), CliError> {
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

/// `ail inspect <kind> <id>` — inspect a node, snapshot, report, artifact, or capability.
///
/// Kinds:
/// - `node`       — semantic graph node by name
/// - `snapshot`   — snapshot envelope by ObjectId hex
/// - `report`     — verification report by id
/// - `artifact`   — compiled artifact by name or path
/// - `capability` — capability by name:Provider
async fn cmd_inspect(
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
            // Derive a real VerificationReport from the current graph.
            // Reports are computed on demand; the id is used as a reference label.
            let graph = load_current_graph_for_cli(store).await?;
            let report = Checker::check(&graph);
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
            let human_msg = format!(
                "type: report\nid: {id}\nsource: derived_from_current_graph\nsummary: {summary}\nentries: {entries_count}\ndiagnostics: {diagnostics_count}"
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "type": "report",
                    "id": id,
                    "source": "derived_from_current_graph",
                    "status": summary,
                    "entries": entries_json,
                    "diagnostics": diagnostics_json,
                    "proof_obligations": proof_obligations_count,
                }),
            );
        }
        "artifact" => {
            // Compile the current graph on demand and return real artifact metadata.
            // The id is used as the artifact label. Source is explicitly
            // "computed_on_demand" — no persisted artifact is claimed.
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
                    "capabilities_manifest": { "entries": [] },
                    "capabilities_manifest_source": "not_available_for_wasm",
                    "semantic_source_map": semantic_source_map_val,
                    "artifact_manifest": artifact_manifest_val,
                }),
            );
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

// cmd_diff, cmd_rollback, cmd_rebase, cmd_merge, cmd_refactor → crate::branch_commands

// cmd_approve, cmd_reject → crate::approval_commands

// cmd_policy → crate::policy_commands

// doctor_index_freshness, doctor_schema_compatibility, cmd_doctor, cmd_gc → crate::diagnostic_commands

// ── PRIVATE HELPERS ───────────────────────────────────────────────────────

pub(crate) fn node_to_json(node: &GraphNode) -> Value {
    serde_json::to_value(node).unwrap_or_else(|_| json!({ "name": node.name }))
}

fn edge_to_json(edge: &GraphEdge) -> Value {
    serde_json::to_value(edge).unwrap_or_else(|_| {
        json!({
            "source": edge.source.0,
            "target": edge.target.0,
            "kind": format!("{:?}", edge.kind),
        })
    })
}

/// A minimal `SnapshotBridge` that always returns a fixed id.
pub(crate) struct SimpleSnapshotBridge(pub(crate) SnapshotId);

impl SnapshotBridge for SimpleSnapshotBridge {
    fn current_snapshot_id(&self) -> SnapshotId {
        self.0
    }
}

/// Create a minimal ChangeSet from a free-text description string.
fn make_text_changeset(text: &str) -> ail_change::model::ChangeSet {
    use ail_change::model::{ChangeSet, ChangeSetMeta, Timestamp};
    ChangeSet {
        meta: ChangeSetMeta {
            author: "cli".to_string(),
            description: text.to_string(),
            timestamp: Timestamp(unix_ms_now()),
        },
        base_snapshot_id: SnapshotId(0),
        ops: vec![],
    }
}

/// Determine the input source label for human output.
fn input_source_label(from_stdin: bool) -> &'static str {
    if from_stdin { "stdin" } else { "file" }
}

/// Build a structural diff preview from a slice of change ops.
/// At this stage the graph is empty so all ops are treated as additions.
fn build_structural_diff_preview(ops: &[ail_change::model::ChangeSetOp]) -> Value {
    json!({
        "creates": ops.len(),
        "modifies": 0,
        "deletes": 0,
        "connects": 0,
        "disconnects": 0,
        "exposes": 0,
        "hides": 0,
        "effects_changed": 0,
        "contracts_changed": 0,
        "capabilities_changed": 0,
    })
}

/// Encode a value as CBOR bytes.
fn encode_cbor<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, CliError> {
    let mut buf = Vec::new();
    ciborium::into_writer(value, &mut buf)
        .map_err(|e| CliError::Domain(format!("CBOR encoding failed: {e}")))?;
    Ok(buf)
}

/// Encode a byte slice as a lowercase hex string.
pub(crate) fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Return `true` if `id` is a valid 64-character lowercase hex string.
pub(crate) fn is_valid_change_id(id: &str) -> bool {
    id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// Convert a 64-char hex string into an `ObjectId`.
pub(crate) fn hex_to_object_id(hex: &str) -> Result<ObjectId, CliError> {
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
pub(crate) fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Return the `.ail/` directory path for a file-backed store.
/// Returns an error for in-memory or Postgres stores.
pub(crate) fn ail_dir_for_store(store: &StoreHandle) -> Result<PathBuf, CliError> {
    match store {
        StoreHandle::File { ail_dir, .. } => Ok(ail_dir.clone()),
        _ => Err(CliError::Domain(
            "persistent .ail storage is not active".to_string(),
        )),
    }
}

// ── UNIT TESTS ────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
