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

use ail_compiler::{emit_wasm_with_profile, lower_to_anf_with_graph, lower_to_core_ir};

use ail_verify::checker::Checker;
use ail_verify::report::VerificationReport;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

pub(crate) use crate::cli_helpers::*;
pub(crate) use crate::graph_loading::*;
pub(crate) use crate::project_commands::{cmd_change, cmd_init, cmd_status};

use crate::approval_commands::{cmd_approve, cmd_reject};
use crate::branch_commands::{cmd_diff, cmd_merge, cmd_rebase, cmd_refactor, cmd_rollback};
use crate::context_commands::cmd_context;
use crate::diagnostic_commands::{cmd_doctor, cmd_gc};
use crate::error::CliError;
use crate::eval_commands::cmd_eval;
use crate::graph_query_commands::{cmd_callers, cmd_effects, cmd_impact, cmd_proofs};
use crate::output::{OutputMode, print_response};
use crate::package_commands::cmd_package;
use crate::package_registry_io::load_package_registry;
use crate::policy_commands::cmd_policy;
use crate::remote_commands::cmd_remote;
use crate::run_commands::{cmd_compile, cmd_run};
use crate::store::{StoreHandle, build_store};
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

// cmd_context → crate::context_commands (imported above)

// context_server_for_cli, synthetic_context_snapshot, parse_context_query_for_cli,
// node_ref_for_cli_target → crate::graph_loading (re-exported above)

// target_node_name, node_refs_for_name → crate::cli_helpers (re-exported above)

// graph query commands (impact, callers, effects, proofs) → crate::graph_query_commands

// cmd_change → crate::project_commands (imported above)

// cmd_verify → crate::workflow_commands::cmd_verify
// cmd_apply  → crate::workflow_commands::cmd_apply

// cmd_compile, cmd_run → crate::run_commands

// current_graph_for_cli, load_current_graph_for_cli,
// load_current_graph_with_snapshot_id_for_cli, snapshot_id_from_parent_chain
// → crate::graph_loading (re-exported above)

// latest_snapshot, format_unix_ms, changeset_outcome_message, conflict_reason_message
// → crate::cli_helpers (re-exported above)

// cmd_init → crate::project_commands (imported above)

// cmd_status → crate::project_commands (imported above)

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
            // Try to load a persisted VerificationReport:
            //   1. If `id` is a 64-char hex hash → load by hash from the object store.
            //   2. Otherwise treat `id` as a change-id and load via the sidecar index.
            //   3. If neither is found → derive from the current graph (fallback).
            let (report, source, resolved_id) = if is_valid_change_id(id) {
                // Try hash lookup first.
                let hash_oid = hex_to_object_id(id)?;
                if let Some(r) = store.load_verification_report_by_hash(&hash_oid).await? {
                    (r, "persisted_by_hash", id.to_string())
                } else if let Some((r, hash)) =
                    store.load_verification_report_by_change_id(id).await?
                {
                    (r, "persisted_by_change_id", hash.to_hex())
                } else {
                    let graph = load_current_graph_for_cli(store).await?;
                    let r = Checker::check(&graph);
                    (r, "derived_from_current_graph", id.to_string())
                }
            } else {
                // id is not a valid 64-char hex; try it as a change-id sidecar lookup.
                if let Some((r, hash)) = store.load_verification_report_by_change_id(id).await? {
                    (r, "persisted_by_change_id", hash.to_hex())
                } else {
                    let graph = load_current_graph_for_cli(store).await?;
                    let r = Checker::check(&graph);
                    (r, "derived_from_current_graph", id.to_string())
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
            let human_msg = format!(
                "type: report\nid: {resolved_id}\nsource: {source}\nsummary: {summary}\nentries: {entries_count}\ndiagnostics: {diagnostics_count}"
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "type": "report",
                    "id": resolved_id,
                    "source": source,
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

// cmd_diff, cmd_rollback, cmd_rebase, cmd_merge, cmd_refactor → crate::branch_commands

// cmd_approve, cmd_reject → crate::approval_commands

// cmd_policy → crate::policy_commands

// doctor_index_freshness, doctor_schema_compatibility, cmd_doctor, cmd_gc → crate::diagnostic_commands

// ── PRIVATE HELPERS ───────────────────────────────────────────────────────
//
// All pure helpers (bytes_to_hex, hex_to_object_id, is_valid_change_id,
// unix_ms_now, format_unix_ms, ail_dir_for_store, conflict_reason_message,
// changeset_outcome_message, encode_cbor, make_text_changeset,
// input_source_label, build_structural_diff_preview, node_to_json,
// edge_to_json, target_node_name, node_refs_for_name, latest_snapshot,
// SimpleSnapshotBridge) have been moved to crate::cli_helpers and are
// re-exported above via `pub(crate) use crate::cli_helpers::*`.

// ── UNIT TESTS ────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
