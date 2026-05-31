use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::references::references_for_token_with_workspace;
use super::source_helpers::{is_acl_token_char, is_ail_source_uri};
use super::symbols::workspace_symbol_items;
use super::tokens::{TokenRange, token_range_at_position};

pub(super) fn prepare_rename_at_position(
    uri: &str,
    text: &str,
    line: usize,
    character: usize,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    let candidate = rename_candidate_at_position(uri, text, line, character, workspace_documents);
    if !candidate["canRename"].as_bool().unwrap_or(false) {
        return Value::Null;
    }
    json!({
        "range": candidate["range"].clone(),
        "placeholder": candidate["placeholder"].clone(),
    })
}

pub(super) fn rename_candidate_at_position(
    uri: &str,
    text: &str,
    line: usize,
    character: usize,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    if !is_ail_source_uri(uri) {
        return blocked(
            "unsupported_language",
            "rename is currently available for .ail source documents only",
        );
    }

    let Some(token_range) = token_range_at_position(text, line, character) else {
        return blocked("missing_token", "position is not on an identifier token");
    };
    let token = token_range.token.trim();
    if !is_rename_identifier(token) {
        return blocked("not_identifier", "token is not a renameable identifier");
    }

    let Some(symbol) = resolve_workspace_symbol(token, workspace_documents) else {
        return blocked(
            "unknown_symbol",
            "token does not resolve to a unique workspace symbol",
        );
    };

    let mut references =
        references_for_token_with_workspace(uri, text, &symbol.name, workspace_documents);
    sort_locations(&mut references);
    let reference_count = references.len();
    let range = token_range_json(line, &token_range);
    json!({
        "canRename": true,
        "token": token,
        "referenceToken": symbol.name,
        "symbolKind": symbol.kind,
        "range": range,
        "placeholder": token,
        "references": references,
        "referenceCount": reference_count,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenameSymbol {
    name: String,
    kind: u64,
}

fn resolve_workspace_symbol(
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Option<RenameSymbol> {
    let symbols = workspace_symbol_items("", workspace_documents);
    let mut matches = symbols
        .iter()
        .filter_map(|symbol| rename_symbol_from_workspace_item(symbol))
        .filter(|symbol| symbol_matches_token(symbol, token))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.kind.cmp(&right.kind))
    });
    matches.dedup();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn rename_symbol_from_workspace_item(item: &Value) -> Option<RenameSymbol> {
    let name = item["name"].as_str()?.to_string();
    let kind = item["kind"].as_u64()?;
    Some(RenameSymbol { name, kind })
}

fn symbol_matches_token(symbol: &RenameSymbol, token: &str) -> bool {
    symbol.name == token
        || symbol
            .name
            .strip_prefix("test.")
            .is_some_and(|name| name == token)
        || symbol
            .name
            .rsplit_once('.')
            .is_some_and(|(_, local)| local == token)
}

fn is_rename_identifier(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(is_acl_token_char)
        && token
            .chars()
            .any(|ch| ch.is_ascii_alphabetic() || ch == '_')
}

fn token_range_json(line: usize, token_range: &TokenRange) -> Value {
    json!({
        "start": { "line": line, "character": token_range.start },
        "end": { "line": line, "character": token_range.end }
    })
}

fn sort_locations(locations: &mut [Value]) {
    locations.sort_by(|left, right| {
        location_sort_key(left)
            .cmp(&location_sort_key(right))
            .then_with(|| left.to_string().cmp(&right.to_string()))
    });
}

fn location_sort_key(location: &Value) -> (String, u64, u64, u64, u64) {
    (
        location["uri"].as_str().unwrap_or_default().to_string(),
        location["range"]["start"]["line"].as_u64().unwrap_or(0),
        location["range"]["start"]["character"]
            .as_u64()
            .unwrap_or(0),
        location["range"]["end"]["line"].as_u64().unwrap_or(0),
        location["range"]["end"]["character"].as_u64().unwrap_or(0),
    )
}

fn blocked(code: &str, reason: &str) -> Value {
    json!({
        "canRename": false,
        "reason": code,
        "message": reason,
    })
}
