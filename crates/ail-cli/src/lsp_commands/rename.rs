use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::references::references_for_token_with_workspace;
use super::source_helpers::{
    file_path_from_uri, is_acl_token_char, is_ail_source_uri, lsp_character_to_byte_index,
};
use super::symbols::workspace_symbol_items;
use super::tokens::{TokenRange, token_range_at_position};

const CATEGORY_INVALID_NEW_NAME: &str = "invalid_new_name";
const CATEGORY_SYMBOL_RESOLUTION: &str = "symbol_resolution";
const CATEGORY_UNSUPPORTED: &str = "unsupported";
const CATEGORY_DOCUMENT_STATE: &str = "document_state";

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
            "AIL_RENAME_SAME_NAME",
            CATEGORY_INVALID_NEW_NAME,
            "newName must differ from the current symbol name",
            json!({
                "newNameLength": new_name.chars().count(),
                "symbolNameLength": symbol_local_name(reference_token).chars().count(),
            }),
        );
    }

    let references = candidate["references"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let missing_document_count = references
        .iter()
        .filter(|reference| {
            reference["uri"]
                .as_str()
                .is_some_and(|uri| workspace_document_entry(workspace_documents, uri).is_none())
        })
        .count();
    if missing_document_count > 0 {
        return blocked(
            "cross_file_import_unsupported",
            "AIL_RENAME_CROSS_FILE_IMPORT_UNSUPPORTED",
            CATEGORY_UNSUPPORTED,
            "rename cannot edit references from unopened imported documents",
            json!({
                "referenceCount": references.len(),
                "missingDocumentCount": missing_document_count,
                "openDocumentCount": workspace_documents.len(),
            }),
        );
    }

    let mut changes: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for reference in &references {
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
        sort_locations(edits);
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
            "AIL_RENAME_UNSUPPORTED_LANGUAGE",
            CATEGORY_UNSUPPORTED,
            "rename is currently available for .ail source documents only",
            json!({ "language": "unsupported" }),
        );
    }

    let Some(token_range) = token_range_at_position(text, line, character) else {
        return blocked(
            "missing_token",
            "AIL_RENAME_MISSING_TOKEN",
            CATEGORY_SYMBOL_RESOLUTION,
            "position is not on an identifier token",
            json!({ "line": line, "character": character }),
        );
    };
    let token = token_range.token.trim();
    if !is_rename_identifier(token) {
        return blocked(
            "not_identifier",
            "AIL_RENAME_NOT_IDENTIFIER",
            CATEGORY_SYMBOL_RESOLUTION,
            "token is not a renameable identifier",
            token_descriptor(token),
        );
    }

    let symbol = match resolve_workspace_symbol(token, workspace_documents) {
        RenameSymbolResolution::Found(symbol) => symbol,
        RenameSymbolResolution::Unresolved => {
            return blocked(
                "unresolved_symbol",
                "AIL_RENAME_UNRESOLVED_SYMBOL",
                CATEGORY_SYMBOL_RESOLUTION,
                "token does not resolve to a workspace symbol",
                json!({
                    "token": token_descriptor(token),
                    "workspaceSymbolCount": workspace_symbol_items("", workspace_documents).len(),
                }),
            );
        }
        RenameSymbolResolution::Ambiguous(matches) => {
            return blocked(
                "ambiguous_symbol",
                "AIL_RENAME_AMBIGUOUS_SYMBOL",
                CATEGORY_SYMBOL_RESOLUTION,
                "token resolves to multiple workspace symbols",
                ambiguous_symbol_descriptor(token, &matches),
            );
        }
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
        "placeholder": symbol_local_name(&symbol.name),
        "references": references,
        "referenceCount": reference_count,
    })
}

pub(super) fn missing_document_rename_failure() -> Value {
    blocked(
        "missing_document",
        "AIL_RENAME_MISSING_DOCUMENT",
        CATEGORY_DOCUMENT_STATE,
        "document is not open in this LSP session",
        json!({ "documentState": "not_open" }),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenameSymbol {
    name: String,
    kind: u64,
    uri: String,
    line: u64,
    character: u64,
}

enum RenameSymbolResolution {
    Found(RenameSymbol),
    Unresolved,
    Ambiguous(Vec<RenameSymbol>),
}

fn resolve_workspace_symbol(
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> RenameSymbolResolution {
    let symbols = workspace_symbol_items("", workspace_documents);
    let mut matches = symbols
        .iter()
        .filter_map(rename_symbol_from_workspace_item)
        .filter(|symbol| symbol_matches_token(symbol, token))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.uri.cmp(&right.uri))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.character.cmp(&right.character))
    });
    matches.dedup();
    match matches.len() {
        0 => RenameSymbolResolution::Unresolved,
        1 => RenameSymbolResolution::Found(matches.remove(0)),
        _ => RenameSymbolResolution::Ambiguous(matches),
    }
}

fn rename_symbol_from_workspace_item(item: &Value) -> Option<RenameSymbol> {
    let name = item["name"].as_str()?.to_string();
    let kind = item["kind"].as_u64()?;
    let uri = item["location"]["uri"].as_str()?.to_string();
    let line = item["location"]["range"]["start"]["line"].as_u64()?;
    let character = item["location"]["range"]["start"]["character"].as_u64()?;
    Some(RenameSymbol {
        name,
        kind,
        uri,
        line,
        character,
    })
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
            "AIL_RENAME_QUALIFIED_NEW_NAME",
            CATEGORY_INVALID_NEW_NAME,
            "newName must be an unqualified .ail identifier",
            json!({
                "newNameLength": name.chars().count(),
                "containsQualifier": true,
            }),
        ));
    }
    if !is_rename_local_identifier(name) {
        return Some(blocked(
            "invalid_identifier",
            "AIL_RENAME_INVALID_IDENTIFIER",
            CATEGORY_INVALID_NEW_NAME,
            "newName must be a valid .ail identifier",
            new_name_descriptor(name),
        ));
    }
    if is_ail_keyword(name) {
        return Some(blocked(
            "reserved_keyword",
            "AIL_RENAME_RESERVED_KEYWORD",
            CATEGORY_INVALID_NEW_NAME,
            "newName must not be a reserved .ail keyword",
            new_name_descriptor(name),
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
    let start = lsp_character_to_byte_index(line, start_character);
    let end = lsp_character_to_byte_index(line, end_character);
    line.get(start..end).map(ToString::to_string)
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
        "start": { "line": line, "character": token_range.start_character },
        "end": { "line": line, "character": token_range.end_character }
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

fn blocked(reason: &str, code: &str, category: &str, message: &str, descriptor: Value) -> Value {
    json!({
        "canRename": false,
        "reason": reason,
        "message": message,
        "diagnostic": {
            "code": code,
            "category": category,
            "severity": "error",
            "descriptor": descriptor,
        }
    })
}

fn token_descriptor(token: &str) -> Value {
    json!({
        "tokenLength": token.chars().count(),
        "containsQualifier": token.contains('.'),
        "tokenClass": if token.chars().all(|ch| ch.is_ascii_digit()) {
            "numeric"
        } else {
            "identifier_like"
        },
    })
}

fn new_name_descriptor(name: &str) -> Value {
    json!({
        "newNameLength": name.chars().count(),
        "containsQualifier": name.contains('.'),
        "startsWithValidIdentifierCharacter": name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_'),
    })
}

fn ambiguous_symbol_descriptor(token: &str, matches: &[RenameSymbol]) -> Value {
    let symbol_kinds = matches
        .iter()
        .map(|symbol| symbol.kind)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    json!({
        "token": token_descriptor(token),
        "candidateCount": matches.len(),
        "symbolKinds": symbol_kinds,
    })
}
