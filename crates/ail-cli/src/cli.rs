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
// | link     --profile [--output]    link native object → executable           |
// | run      --profile [--file .ail] [module] preflight + runtime report      |
// | test     [--file .ail] [filter]  Discover and run graph/source tests      |
// | check    --file .ail        Validate source program without executing     |
// | new      <path>              Create project scaffold with starter .ail/ACL |
// | lsp      --stdio|--diagnose  Editor diagnostics through LSP-shaped IO     |
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
// | fmt      [--file]       | Format ACL ChangeSet text into canonical form         |
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

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

pub(crate) use crate::cli_helpers::*;
pub(crate) use crate::graph_loading::*;
pub(crate) use crate::project_commands::{cmd_change, cmd_init, cmd_new, cmd_status};

use crate::approval_commands::{cmd_approve, cmd_reject};
use crate::branch_commands::{cmd_diff, cmd_merge, cmd_rebase, cmd_refactor, cmd_rollback};
use crate::compile_commands::cmd_compile;
use crate::context_commands::cmd_context;
use crate::diagnostic_commands::{cmd_doctor, cmd_gc};
use crate::error::CliError;
use crate::eval_commands::cmd_eval;
use crate::fmt_commands::cmd_fmt;
use crate::graph_query_commands::{cmd_callers, cmd_effects, cmd_impact, cmd_proofs};
use crate::inspect_commands::cmd_inspect;
use crate::link_commands::{
    RUNTIME_STUB_SUBDIR, SystemLinker, cmd_emit_runtime_stub, cmd_link, cmd_print_runtime_symbols,
    ensure_runtime_stub_at, validate_link_mode_flags,
};
use crate::lsp_commands::cmd_lsp;
use crate::output::OutputMode;
use crate::package_commands::cmd_package;
use crate::policy_commands::cmd_policy;
use crate::remote_commands::cmd_remote;
use crate::run_commands::cmd_run;
use crate::source_commands::cmd_check_source;
use crate::store::build_store;
use crate::test_commands::cmd_test;
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
        /// Run a `.ail` source file directly instead of loading the current graph.
        #[arg(long, short)]
        file: Option<PathBuf>,
        /// Module target to run (e.g. `module.checkout`).
        module: Option<String>,
        /// Positional i64 arguments passed to the exported function.
        args: Vec<String>,
        /// Grant a capability to this run invocation. Repeat for multiple grants.
        #[arg(long = "grant")]
        grants: Vec<String>,
        /// Replay a recorded trace by its id.
        #[arg(long)]
        replay: Option<String>,
    },

    /// Discover and run test targets from the current graph.
    ///
    /// Tests are graph nodes with `NodeKind::Test`, names like `test.*`, or
    /// function names like `fn.test_*`. Each test passes when its return value
    /// is truthy, `ok`/`pass`, or unit.
    Test {
        /// Runtime profile name (e.g. `dev`, `test`).
        #[arg(long, default_value = "test")]
        profile: String,
        /// Execution target. `native` returns an explicit unsupported error.
        #[arg(long, default_value = "wasm")]
        target: String,
        /// Run tests from a `.ail` source file directly instead of loading the current graph.
        #[arg(long, short)]
        file: Option<PathBuf>,
        /// Optional substring filter for test names.
        filter: Option<String>,
        /// Positional arguments passed to every selected test export.
        args: Vec<String>,
        /// Grant a capability to this test invocation. Repeat for multiple grants.
        #[arg(long = "grant")]
        grants: Vec<String>,
    },

    /// Validate a `.ail` source program without compiling or executing it.
    Check {
        /// Source file to validate, including relative imports.
        #[arg(long, short)]
        file: PathBuf,
    },

    /// Link a compiled native object file into an executable.
    ///
    /// Resolves the latest persisted native artifact for the given profile and
    /// invokes the system linker (`cc` on Unix, `link.exe` on Windows).
    /// Run `ail compile --target native` first to produce the object file.
    ///
    /// The native object imports three symbols that must be supplied at link time:
    ///   host_call        — capability dispatch (ail-runtime host boundary)
    ///   __ail_malloc     — heap allocator stub (ail-runtime allocator)
    ///   ail_runtime_call — concurrency/resource/channel dispatch (Phase 9+)
    ///
    /// Workflows:
    ///   # One-step: auto-generate and cache stub archive, then link
    ///   ail link --profile dev --ensure-runtime-stub
    ///
    ///   # Two-step: emit stub archive explicitly, then link
    ///   ail link --emit-runtime-stub ./ail_runtime.a
    ///   ail link --profile dev --runtime-lib ./ail_runtime.a
    ///
    /// Pass --runtime-lib <path/to/ail_runtime.a> to bundle these into the
    /// linked executable.  Without it the linker may fail with undefined-symbol
    /// errors unless the symbols are supplied through another mechanism.
    Link {
        /// Compilation profile to link (e.g. `dev`, `prod`).
        #[arg(long, default_value = "dev")]
        profile: String,
        /// Output executable path.
        /// Defaults to the profile name in the current directory.
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        /// Path to the ail-runtime static archive (e.g. `ail_runtime.a`).
        ///
        /// When provided, the archive is appended to the linker command so that
        /// the native object's unresolved imports (host_call, __ail_malloc,
        /// ail_runtime_call) are resolved at link time.  Without this flag the
        /// linker may report undefined-symbol errors.
        #[arg(long)]
        runtime_lib: Option<PathBuf>,
        /// Generate the runtime stub archive at the given path and exit.
        ///
        /// Emits a deterministic static archive (`ail_runtime.a`) containing
        /// stub implementations of the three unresolved runtime symbols.
        /// No system `ar` or linker is required to generate it.
        /// After generation, pass the path to --runtime-lib to produce a
        /// self-contained linked executable.
        #[arg(long, value_name = "OUTPUT")]
        emit_runtime_stub: Option<PathBuf>,
        /// Print the names of the three runtime symbols imported by native
        /// objects and exit.  Useful for diagnostics and linker script authoring.
        #[arg(long)]
        print_runtime_symbols: bool,
        /// Auto-generate and cache `.ail/runtime/ail_runtime.a` if absent,
        /// then use it as the runtime library for this link invocation.
        ///
        /// The archive is written once using the same pure-Rust generator as
        /// `--emit-runtime-stub`; subsequent calls reuse the cached file without
        /// regenerating it.  The canonical project-local path is:
        ///   `.ail/runtime/ail_runtime.a`
        ///
        /// Incompatible with `--runtime-lib` (conflicting path sources),
        /// `--emit-runtime-stub`, and `--print-runtime-symbols`.
        #[arg(long)]
        ensure_runtime_stub: bool,
    },

    /// Format an ACL ChangeSet document.
    Fmt {
        /// Path to an ACL file. When omitted, reads from stdin and writes to stdout.
        #[arg(long, short)]
        file: Option<PathBuf>,
        /// Check whether the input is already canonical; exits non-zero if formatting would change it.
        #[arg(long)]
        check: bool,
        /// Rewrite --file in place. Requires --file.
        #[arg(long)]
        write: bool,
    },

    /// Evaluate an inline expression without initializing a project.
    Eval {
        /// Expression to evaluate (e.g. `add(20, 22)`, `mul(6, 7)`, or `42`).
        expression: String,
    },

    /// Create a new AIL project directory with .ail/ metadata and starter ACL.
    New {
        /// Project directory to create.
        path: PathBuf,
        /// Initial branch name.
        #[arg(long, default_value = "main")]
        branch: String,
        /// Allow creating inside a non-empty directory.
        #[arg(long)]
        force: bool,
    },

    /// Run the validation-stage AIL language server or emit LSP diagnostics.
    Lsp {
        /// Run JSON-RPC Language Server Protocol over stdin/stdout.
        #[arg(long)]
        stdio: bool,
        /// Diagnose one ACL file and print LSP-shaped diagnostics.
        #[arg(long)]
        diagnose: Option<PathBuf>,
        /// Emit LSP completion items matching this prefix.
        #[arg(long)]
        complete: Option<String>,
        /// Emit LSP hover information for one ACL token.
        #[arg(long = "hover-token")]
        hover_token: Option<String>,
        /// Emit go-to-definition location for one ACL identifier token.
        #[arg(long = "definition-token")]
        definition_token: Option<String>,
        /// ACL file used by --definition-token.
        #[arg(long = "definition-file")]
        definition_file: Option<PathBuf>,
        /// Emit same-file references for one ACL identifier token.
        #[arg(long = "references-token")]
        references_token: Option<String>,
        /// ACL file used by --references-token.
        #[arg(long = "references-file")]
        references_file: Option<PathBuf>,
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
                "Available subcommands: context, change, verify, apply, compile, run, test, link, \
                 fmt, eval, new, lsp, init, status, inspect, diff, rollback, rebase, merge, refactor, \
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

    let command = match cli.command {
        Commands::New {
            path,
            branch,
            force,
        } => return cmd_new(mode, path, &branch, force).await,
        Commands::Lsp {
            stdio,
            diagnose,
            complete,
            hover_token,
            definition_token,
            definition_file,
            references_token,
            references_file,
        } => {
            return cmd_lsp(
                mode,
                stdio,
                diagnose,
                complete,
                hover_token,
                definition_token,
                definition_file,
                references_token,
                references_file,
            );
        }
        other => other,
    };

    let store = build_store(cli.database_url.as_deref()).await?;

    match command {
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
            file,
            module,
            args,
            grants,
            replay,
        } => {
            cmd_run(
                mode,
                &profile,
                &target,
                file.as_deref(),
                module.as_deref(),
                &args,
                &grants,
                replay.as_deref(),
                &store,
            )
            .await
        }
        Commands::Test {
            profile,
            target,
            file,
            filter,
            args,
            grants,
        } => {
            cmd_test(
                mode,
                &profile,
                &target,
                file.as_deref(),
                filter.as_deref(),
                &args,
                &grants,
                &store,
            )
            .await
        }
        Commands::Check { file } => cmd_check_source(mode, &file),
        Commands::Link {
            profile,
            output,
            runtime_lib,
            emit_runtime_stub,
            print_runtime_symbols,
            ensure_runtime_stub,
        } => {
            validate_link_mode_flags(
                print_runtime_symbols,
                emit_runtime_stub.is_some(),
                ensure_runtime_stub,
                runtime_lib.is_some(),
            )?;
            if print_runtime_symbols {
                cmd_print_runtime_symbols(mode);
                Ok(())
            } else if let Some(stub_out) = emit_runtime_stub {
                cmd_emit_runtime_stub(mode, &stub_out)
            } else {
                // Resolve the effective runtime library path.
                // When --ensure-runtime-stub is set, auto-generate and cache
                // .ail/runtime/ail_runtime.a (idempotent), then use that path.
                // Otherwise, use the explicit --runtime-lib path (may be None).
                let ensured: Option<PathBuf> = if ensure_runtime_stub {
                    let stub_dir = PathBuf::from(".ail").join(RUNTIME_STUB_SUBDIR);
                    Some(ensure_runtime_stub_at(&stub_dir)?)
                } else {
                    None
                };
                let effective_runtime_lib: Option<PathBuf> = ensured.or(runtime_lib);
                cmd_link(
                    mode,
                    &profile,
                    output.as_deref(),
                    effective_runtime_lib.as_deref(),
                    &store,
                    &SystemLinker,
                )
            }
        }
        Commands::Fmt { file, check, write } => cmd_fmt(mode, file, check, write),
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
        Commands::New { .. } => unreachable!("handled before store initialization"),
        Commands::Lsp { .. } => unreachable!("handled before store initialization"),
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

// cmd_compile → crate::compile_commands
// cmd_run     → crate::run_commands
// cmd_fmt     → crate::fmt_commands

// current_graph_for_cli, load_current_graph_for_cli,
// load_current_graph_with_snapshot_id_for_cli, snapshot_id_from_parent_chain
// → crate::graph_loading (re-exported above)

// latest_snapshot, format_unix_ms, changeset_outcome_message, conflict_reason_message
// → crate::cli_helpers (re-exported above)

// cmd_init → crate::project_commands (imported above)

// cmd_status → crate::project_commands (imported above)

// cmd_inspect → crate::inspect_commands (imported above)

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
// Tests are split into domain-scoped files under src/tests/.
// Each file starts with `use super::*;` which gives it the same visibility
// as the old monolithic cli_tests.rs (child module of cli.rs).

#[cfg(test)]
#[path = "tests/identity.rs"]
mod tests_identity;

#[cfg(test)]
#[path = "tests/run.rs"]
mod tests_run;

#[cfg(test)]
#[path = "tests/compile.rs"]
mod tests_compile;

#[cfg(test)]
#[path = "tests/verify.rs"]
mod tests_verify;

#[cfg(test)]
#[path = "tests/graph_query.rs"]
mod tests_graph_query;

#[cfg(test)]
#[path = "tests/apply.rs"]
mod tests_apply;

#[cfg(test)]
#[path = "tests/branch_ops.rs"]
mod tests_branch_ops;

#[cfg(test)]
#[path = "tests/refactor.rs"]
mod tests_refactor;

#[cfg(test)]
#[path = "tests/approval_policy.rs"]
mod tests_approval_policy;

#[cfg(test)]
#[path = "tests/package.rs"]
mod tests_package;

#[cfg(test)]
#[path = "tests/inspect.rs"]
mod tests_inspect;

#[cfg(test)]
#[path = "tests/pipeline.rs"]
mod tests_pipeline;

#[cfg(test)]
#[path = "tests/report_persistence.rs"]
mod tests_report_persistence;

#[cfg(test)]
#[path = "tests/doctor.rs"]
mod tests_doctor;

#[cfg(test)]
#[path = "tests/link.rs"]
mod tests_link;

#[cfg(test)]
#[path = "tests/link_stub.rs"]
mod tests_link_stub;

#[cfg(test)]
#[path = "tests/link_ensure.rs"]
mod tests_link_ensure;
