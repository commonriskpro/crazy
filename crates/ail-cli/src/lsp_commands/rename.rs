use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::references::references_for_token_with_workspace;
use super::source_helpers::{file_path_from_uri, is_acl_token_char, is_ail_source_uri};
use super::symbols::workspace_symbol_items;
use super::tokens::{TokenRange, token_range_at_position};

pub(super) fn rename_workspace_edit_at_position(
    uri: &str,
    text: &str,
    line: usize,
    character: usize,
    new_name: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    let result =
        rename_edits_at_position(uri, text, line, character, new_name, workspace_documents);
    if result["canRename"].as_bool().unwrap_or(false) {
        result["workspaceEdit"].clone()
    } else {
        Value::Null
    }
}

pub(super) fn rename_edits_at_position(
    uri: &str,
    text: &str,
    line: usize,
    character: usize,
    new_name: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    let new_name = new_name.trim();
    if let Some(blocked) = validate_rename_new_name(new_name) {
        return blocked;
    }

    let candidate = rename_candidate_at_position(uri, text, line, character, workspace_documents);
    if !candidate["canRename"].as_bool().unwrap_or(false) {
        return candidate;
    }

    let reference_token = candidate["referenceToken"].as_str().unwrap_or_default();
    if symbol_local_name(reference_token) == new_name {
        return blocked(
            "same_name",
            "newName must differ from the current symbol name",
        );
    }

    let mut changes: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for reference in candidate["references"].as_array().into_iter().flatten() {
        let Some(reference_uri) = reference["uri"].as_str() else {
            continue;
        };
        let Some((document_uri, document_text)) =
            workspace_document_entry(workspace_documents, reference_uri)
        else {
            continue;
        };
        let Some(edit) =
            text_edit_for_reference(document_text, reference, reference_token, new_name)
        else {
            continue;
        };
        changes
            .entry(document_uri.to_string())
            .or_default()
            .push(edit);
    }

    for edits in changes.values_mut() {
        edits.sort_by(|left, right| {
            location_sort_key(left)
                .cmp(&location_sort_key(right))
                .then_with(|| left.to_string().cmp(&right.to_string()))
        });
    }

    let document_count = changes.len();
    let edit_count = changes.values().map(Vec::len).sum::<usize>();
    json!({
        "canRename": true,
        "token": candidate["token"].clone(),
        "referenceToken": candidate["referenceToken"].clone(),
        "symbolKind": candidate["symbolKind"].clone(),
        "newName": new_name,
        "documentCount": document_count,
        "editCount": edit_count,
        "workspaceEdit": {
            "changes": changes
        }
    })
}

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

fn validate_rename_new_name(name: &str) -> Option<Value> {
    if name.contains('.') {
        return Some(blocked(
            "qualified_new_name",
            "newName must be an unqualified .ail identifier",
        ));
    }
    if !is_rename_local_identifier(name) {
        return Some(blocked(
            "invalid_identifier",
            "newName must be a valid .ail identifier",
        ));
    }
    if is_ail_keyword(name) {
        return Some(blocked(
            "reserved_keyword",
            "newName must not be a reserved .ail keyword",
        ));
    }
    None
}

fn is_rename_local_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_ail_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "capability"
            | "case"
            | "const"
            | "effect"
            | "else"
            | "ensures"
            | "enum"
            | "false"
            | "fn"
            | "foreach"
            | "if"
            | "in"
            | "let"
            | "match"
            | "module"
            | "policy"
            | "proof"
            | "record"
            | "requires"
            | "return"
            | "struct"
            | "test"
            | "then"
            | "true"
            | "type"
            | "use"
            | "where"
    )
}

fn symbol_local_name(symbol_name: &str) -> &str {
    symbol_name
        .rsplit_once('.')
        .map(|(_, local)| local)
        .unwrap_or(symbol_name)
}

fn workspace_document_entry<'a>(
    workspace_documents: &'a BTreeMap<String, String>,
    uri: &str,
) -> Option<(&'a str, &'a str)> {
    workspace_documents
        .get_key_value(uri)
        .map(|(uri, text)| (uri.as_str(), text.as_str()))
        .or_else(|| {
            let reference_path = file_path_from_uri(uri)?;
            let canonical_reference = std::fs::canonicalize(reference_path).ok()?;
            workspace_documents
                .iter()
                .find_map(|(candidate_uri, text)| {
                    let candidate_path = file_path_from_uri(candidate_uri)?;
                    let canonical_candidate = std::fs::canonicalize(candidate_path).ok()?;
                    (canonical_candidate == canonical_reference)
                        .then_some((candidate_uri.as_str(), text.as_str()))
                })
        })
}

fn text_edit_for_reference(
    document_text: &str,
    reference: &Value,
    reference_token: &str,
    new_name: &str,
) -> Option<Value> {
    let snippet = text_for_range(document_text, &reference["range"])?;
    let new_text = replacement_text_for_reference(&snippet, reference_token, new_name)?;
    Some(json!({
        "range": reference["range"].clone(),
        "newText": new_text,
    }))
}

fn text_for_range(text: &str, range: &Value) -> Option<String> {
    let start_line = range["start"]["line"].as_u64()? as usize;
    let start_character = range["start"]["character"].as_u64()? as usize;
    let end_line = range["end"]["line"].as_u64()? as usize;
    let end_character = range["end"]["character"].as_u64()? as usize;
    if start_line != end_line || start_character >= end_character {
        return None;
    }
    let line = text.lines().nth(start_line)?;
    line.get(start_character..end_character)
        .map(ToString::to_string)
}

fn replacement_text_for_reference(
    snippet: &str,
    reference_token: &str,
    new_name: &str,
) -> Option<String> {
    if snippet == reference_token {
        return Some(qualified_replacement(reference_token, new_name));
    }
    if let Some((qualifier, local)) = reference_token.rsplit_once('.') {
        if snippet == local {
            return Some(new_name.to_string());
        }
        if snippet
            .rsplit_once('.')
            .is_some_and(|(snippet_qualifier, snippet_local)| {
                snippet_qualifier == qualifier && snippet_local == local
            })
        {
            return Some(format!("{qualifier}.{new_name}"));
        }
    }
    if let Some(local) = reference_token.strip_prefix("test.")
        && snippet == local
    {
        return Some(new_name.to_string());
    }
    None
}

fn qualified_replacement(reference_token: &str, new_name: &str) -> String {
    reference_token
        .rsplit_once('.')
        .map(|(qualifier, _)| format!("{qualifier}.{new_name}"))
        .unwrap_or_else(|| new_name.to_string())
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
