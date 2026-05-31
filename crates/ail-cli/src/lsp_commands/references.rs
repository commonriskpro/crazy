use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::source_helpers::{
    file_path_from_uri, is_acl_token_char, is_ail_source_uri, resolve_lsp_source_import,
    source_imports_from_text, source_module_from_text,
};

pub(super) fn references_for_token(uri: &str, text: &str, token: &str) -> Vec<Value> {
    references_for_token_with_workspace(uri, text, token, &BTreeMap::new())
}

pub(super) fn references_for_token_with_workspace(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Vec<Value> {
    let token = token.trim();
    if token.is_empty() {
        return vec![];
    }

    if is_ail_source_uri(uri) {
        return references_for_ail_source_token(uri, text, token);
    }

    references_in_text(uri, text, token)
}

fn references_for_ail_source_token(uri: &str, text: &str, token: &str) -> Vec<Value> {
    let mut refs = source_references_in_text(uri, text, token);
    let Some(root_path) = file_path_from_uri(uri) else {
        return refs;
    };
    let Ok(canonical_root) = std::fs::canonicalize(&root_path) else {
        return refs;
    };
    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    collect_ail_source_import_references(
        &canonical_root,
        text,
        token,
        workspace_documents,
        &mut visited,
        &mut refs,
    );
    refs
}

fn collect_ail_source_import_references(
    source_path: &Path,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
    visited: &mut BTreeSet<PathBuf>,
    refs: &mut Vec<Value>,
) {
    for import in source_imports_from_text(text) {
        let path = resolve_lsp_source_import(source_path, &import);
        let Ok(canonical) = std::fs::canonicalize(&path) else {
            continue;
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let imported_uri = format!("file://{}", canonical.display());
        let imported_text =
            match workspace_document_text(workspace_documents, &imported_uri, &canonical) {
                Some(text) => text.to_string(),
                None => match std::fs::read_to_string(&canonical) {
                    Ok(text) => text,
                    Err(_) => continue,
                },
            };
        refs.extend(source_references_in_text(
            &imported_uri,
            &imported_text,
            token,
        ));
        collect_ail_source_import_references(
            &canonical,
            &imported_text,
            token,
            workspace_documents,
            visited,
            refs,
        );
    }
}

fn workspace_document_text<'a>(
    workspace_documents: &'a BTreeMap<String, String>,
    imported_uri: &str,
    imported_path: &Path,
) -> Option<&'a str> {
    workspace_documents
        .get(imported_uri)
        .map(String::as_str)
        .or_else(|| {
            workspace_documents.iter().find_map(|(uri, text)| {
                let path = file_path_from_uri(uri)?;
                let canonical = std::fs::canonicalize(path).ok()?;
                (canonical == imported_path).then_some(text.as_str())
            })
        })
}

fn source_references_in_text(uri: &str, text: &str, token: &str) -> Vec<Value> {
    source_reference_tokens_for_text(text, token)
        .into_iter()
        .flat_map(|needle| references_in_text(uri, text, &needle))
        .collect()
}

fn source_reference_tokens_for_text(text: &str, token: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut push_token = |token: &str| {
        if !tokens.iter().any(|existing| existing == token) {
            tokens.push(token.to_string());
        }
    };

    if let Some(local) = token
        .strip_prefix("fn.")
        .or_else(|| token.strip_prefix("test."))
    {
        push_token(local);
        push_token(token);
    } else if let Some((module, local)) = token.split_once('.')
        && source_module_from_text(text).as_deref() == Some(module)
    {
        push_token(token);
        push_token(local);
    } else {
        push_token(token);
    }

    tokens
}

fn references_in_text(uri: &str, text: &str, token: &str) -> Vec<Value> {
    text.lines()
        .enumerate()
        .flat_map(|(line_idx, line)| token_ranges_in_line(uri, line_idx, line, token))
        .collect()
}

fn token_ranges_in_line(uri: &str, line_idx: usize, line: &str, token: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(offset) = line[search_from..].find(token) {
        let start = search_from + offset;
        let end = start + token.len();
        let before = line[..start].chars().next_back();
        let after = line[end..].chars().next();
        let boundary_before = before.map_or(true, |ch| !is_acl_token_char(ch));
        let boundary_after = after.map_or(true, |ch| !is_acl_token_char(ch));
        if boundary_before && boundary_after {
            out.push(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": end }
                }
            }));
        }
        search_from = end;
    }
    out
}
