// ── ail-cli::context_commands ────────────────────────────────────────────
//
// Handler for the `ail context` command, extracted from cli.rs.
//
// Re-exported in `crate::cli` via `use crate::context_commands::cmd_context`
// so that the dispatch table in `run()` compiles without changes.

use ail_context::{AuthSession, ContextRequest, SnapshotSelector, TrustLevel as ContextTrustLevel};
use serde_json::{Value, json};

use crate::cli_helpers::bytes_to_hex;
use crate::error::CliError;
use crate::graph_loading::{context_server_for_cli, parse_context_query_for_cli};
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;

/// `ail context [target]` — return a hash-bound semantic context slice.
///
/// Rules (from tooling.md):
/// 1. Context commands never mutate the graph.
/// 2. Context output includes snapshot/hash.
/// 3. Context can be used in ChangeSet requires via assert_context.
pub(crate) async fn cmd_context(
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
