use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::source_helpers::{
    file_path_from_uri, is_ail_source_uri, resolve_lsp_source_import, source_imports_from_text,
    source_module_from_text,
};

pub(super) fn definition_for_token(uri: &str, text: &str, token: &str) -> Value {
    definition_for_token_with_workspace(uri, text, token, &BTreeMap::new())
}

pub(super) fn definition_for_token_with_workspace(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    let token = token.trim();
    if token.is_empty() {
        return Value::Null;
    }

    if is_ail_source_uri(uri)
        && let Some(definition) = definition_for_ail_source_token(uri, text, token)
    {
        return definition;
    }

    for (line_idx, line) in text.lines().enumerate() {
        let needle = format!("id={token}");
        if let Some(start) = line.find(&needle).map(|idx| idx + "id=".len()) {
            return json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + token.len() }
                }
            });
        }
    }
    Value::Null
}

fn definition_for_ail_source_token(uri: &str, text: &str, token: &str) -> Option<Value> {
    let token = token
        .strip_prefix("fn.")
        .or_else(|| token.strip_prefix("test."))
        .unwrap_or(token);
    if let Some(definition) = source_definition_in_text(uri, text, token) {
        return Some(definition);
    }

    let root_path = file_path_from_uri(uri)?;
    let canonical_root = std::fs::canonicalize(&root_path).ok()?;
    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    definition_for_ail_source_imports(
        &canonical_root,
        text,
        token,
        workspace_documents,
        &mut visited,
    )
}

fn definition_for_ail_source_imports(
    source_path: &Path,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
    visited: &mut BTreeSet<PathBuf>,
) -> Option<Value> {
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
        if let Some(definition) = source_definition_in_text(&imported_uri, &imported_text, token) {
            return Some(definition);
        }
        if let Some(definition) = definition_for_ail_source_imports(
            &canonical,
            &imported_text,
            token,
            workspace_documents,
            visited,
        ) {
            return Some(definition);
        }
    }
    None
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

fn source_definition_in_text(uri: &str, text: &str, token: &str) -> Option<Value> {
    source_function_definition_in_text(uri, text, token)
        .or_else(|| source_const_definition_in_text(uri, text, token))
        .or_else(|| source_test_definition_in_text(uri, text, token))
        .or_else(|| source_capability_definition_in_text(uri, text, token))
}

fn source_function_definition_in_text(uri: &str, text: &str, token: &str) -> Option<Value> {
    let module = source_module_from_text(text);
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("fn ") else {
            continue;
        };
        let Some(name_end) = rest.find('(') else {
            continue;
        };
        let name = rest[..name_end].trim();
        if source_decl_name_matches_token(name, module.as_deref(), token, "fn.") {
            let leading = line.len() - trimmed.len();
            let start = leading + "fn ".len();
            return Some(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    None
}

fn source_const_definition_in_text(uri: &str, text: &str, token: &str) -> Option<Value> {
    let module = source_module_from_text(text);
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("const ") else {
            continue;
        };
        let Some(name_end) = rest.find(':') else {
            continue;
        };
        let name = rest[..name_end].trim();
        if source_decl_name_matches_token(name, module.as_deref(), token, "fn.") {
            let leading = line.len() - trimmed.len();
            let start = leading + "const ".len();
            return Some(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    None
}

fn source_test_definition_in_text(uri: &str, text: &str, token: &str) -> Option<Value> {
    let module = source_module_from_text(text);
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("test ") else {
            continue;
        };
        let Some(name_end) = [rest.find("->"), rest.find('=')]
            .into_iter()
            .flatten()
            .min()
        else {
            continue;
        };
        let name = rest[..name_end].trim();
        if source_decl_name_matches_token(name, module.as_deref(), token, "test.") {
            let leading = line.len() - trimmed.len();
            let start = leading + "test ".len();
            return Some(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    None
}

fn source_decl_name_matches_token(
    name: &str,
    module: Option<&str>,
    token: &str,
    prefix: &str,
) -> bool {
    if name == token || name.strip_prefix(prefix) == Some(token) {
        return true;
    }
    let Some(module) = module else {
        return false;
    };
    let bare_name = name.strip_prefix(prefix).unwrap_or(name);
    token == format!("{module}.{bare_name}")
}

fn source_capability_definition_in_text(uri: &str, text: &str, token: &str) -> Option<Value> {
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("capability ") else {
            continue;
        };
        let name = rest.split_whitespace().next().unwrap_or_default();
        if name == token {
            let leading = line.len() - trimmed.len();
            let start = leading + "capability ".len();
            return Some(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    None
}
