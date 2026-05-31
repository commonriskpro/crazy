mod acl;
mod source_builtins;
mod source_syntax;

use serde_json::{Value, json};

use super::source_helpers::{is_ail_source_uri, source_module_from_text};

use acl::ACL_SYMBOLS;
use source_builtins::AIL_SOURCE_BUILTIN_SYMBOLS;
use source_syntax::AIL_SOURCE_SYNTAX_SYMBOLS;

pub(super) struct AclSymbol {
    pub(super) label: &'static str,
    pub(super) detail: &'static str,
    pub(super) documentation: &'static str,
    pub(super) insert_text: &'static str,
}

fn all_symbols() -> impl Iterator<Item = &'static AclSymbol> {
    ACL_SYMBOLS
        .iter()
        .chain(AIL_SOURCE_SYNTAX_SYMBOLS.iter())
        .chain(AIL_SOURCE_BUILTIN_SYMBOLS.iter())
}

pub(super) fn completion_items(prefix: &str) -> Vec<Value> {
    let prefix = prefix.trim().to_ascii_lowercase();
    all_symbols()
        .filter(|symbol| {
            prefix.is_empty() || symbol.label.to_ascii_lowercase().contains(prefix.as_str())
        })
        .map(|symbol| {
            json!({
                "label": symbol.label,
                "kind": 14,
                "detail": symbol.detail,
                "documentation": {
                    "kind": "markdown",
                    "value": symbol.documentation
                },
                "insertText": symbol.insert_text,
                "insertTextFormat": 2
            })
        })
        .collect()
}

pub(super) fn workspace_symbol_items(
    query: &str,
    workspace_documents: &std::collections::BTreeMap<String, String>,
) -> Vec<Value> {
    let query = query.trim().to_ascii_lowercase();
    let mut symbols = workspace_documents
        .iter()
        .filter(|(uri, _)| is_ail_source_uri(uri))
        .flat_map(|(uri, text)| ail_source_symbols_for_document(uri, text))
        .filter(|symbol| {
            query.is_empty() || symbol.name.to_ascii_lowercase().contains(query.as_str())
        })
        .collect::<Vec<_>>();
    symbols.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.uri.cmp(&right.uri))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.character.cmp(&right.character))
    });
    symbols
        .into_iter()
        .map(|symbol| {
            let mut item = json!({
                "name": symbol.name,
                "kind": symbol.kind,
                "location": {
                    "uri": symbol.uri,
                    "range": {
                        "start": { "line": symbol.line, "character": symbol.character },
                        "end": { "line": symbol.line, "character": symbol.character + symbol.selection_len }
                    }
                }
            });
            if let Some(container_name) = symbol.container_name {
                item["containerName"] = json!(container_name);
            }
            item
        })
        .collect()
}

#[derive(Debug)]
struct WorkspaceSymbol {
    name: String,
    kind: u64,
    uri: String,
    line: usize,
    character: usize,
    selection_len: usize,
    container_name: Option<String>,
}

fn ail_source_symbols_for_document(uri: &str, text: &str) -> Vec<WorkspaceSymbol> {
    let module = source_module_from_text(text);
    let mut symbols = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();
        if let Some(name) = trimmed
            .strip_prefix("module ")
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            symbols.push(source_symbol(
                uri,
                name,
                2,
                line_idx,
                leading + "module ".len(),
                name.len(),
                None,
            ));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("fn ")
            && let Some(name_end) = rest.find('(')
        {
            let raw_name = rest[..name_end].trim();
            if !raw_name.is_empty() {
                symbols.push(source_symbol(
                    uri,
                    &qualified_source_name(raw_name, module.as_deref(), None),
                    12,
                    line_idx,
                    leading + "fn ".len(),
                    raw_name.len(),
                    module.clone(),
                ));
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("const ")
            && let Some(name_end) = rest.find(':')
        {
            let raw_name = rest[..name_end].trim();
            if !raw_name.is_empty() {
                symbols.push(source_symbol(
                    uri,
                    &qualified_source_name(raw_name, module.as_deref(), None),
                    14,
                    line_idx,
                    leading + "const ".len(),
                    raw_name.len(),
                    module.clone(),
                ));
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("test ")
            && let Some(name_end) = [rest.find("->"), rest.find('=')]
                .into_iter()
                .flatten()
                .min()
        {
            let raw_name = rest[..name_end].trim();
            if !raw_name.is_empty() {
                symbols.push(source_symbol(
                    uri,
                    &qualified_source_name(raw_name, module.as_deref(), Some("test.")),
                    12,
                    line_idx,
                    leading + "test ".len(),
                    raw_name.len(),
                    module.clone(),
                ));
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("capability ") {
            let raw_name = rest.split_whitespace().next().unwrap_or_default();
            if !raw_name.is_empty() {
                symbols.push(source_symbol(
                    uri,
                    raw_name,
                    5,
                    line_idx,
                    leading + "capability ".len(),
                    raw_name.len(),
                    module.clone(),
                ));
            }
        }
    }
    symbols
}

fn source_symbol(
    uri: &str,
    name: &str,
    kind: u64,
    line: usize,
    character: usize,
    selection_len: usize,
    container_name: Option<String>,
) -> WorkspaceSymbol {
    WorkspaceSymbol {
        name: name.to_string(),
        kind,
        uri: uri.to_string(),
        line,
        character,
        selection_len,
        container_name,
    }
}

fn qualified_source_name(name: &str, module: Option<&str>, prefix: Option<&str>) -> String {
    let prefixed = match prefix {
        Some(prefix) if !name.starts_with(prefix) => format!("{prefix}{name}"),
        _ => name.to_string(),
    };
    if prefixed.contains('.') {
        prefixed
    } else if let Some(module) = module {
        format!("{module}.{prefixed}")
    } else {
        prefixed
    }
}

pub(super) fn hover_for_token(token: &str) -> Value {
    let normalized = token.trim();
    let symbol = all_symbols().find(|symbol| {
        symbol.label == normalized
            || symbol
                .label
                .split_whitespace()
                .last()
                .is_some_and(|last| last == normalized)
    });
    match symbol {
        Some(symbol) => json!({
            "contents": {
                "kind": "markdown",
                "value": format!("**{}**\n\n{}\n\n`{}`", symbol.label, symbol.documentation, symbol.insert_text)
            }
        }),
        None => Value::Null,
    }
}
