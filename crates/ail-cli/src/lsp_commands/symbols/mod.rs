mod acl;
mod source_builtins;
mod source_syntax;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::source_helpers::{
    file_path_from_uri, is_ail_source_uri, resolve_lsp_source_import, source_imports_from_text,
    source_module_from_text,
};

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
    let mut seen_labels = BTreeSet::new();
    let mut items = all_symbols()
        .enumerate()
        .filter_map(|(ordinal, symbol)| {
            let rank = completion_match_rank(&prefix, symbol.label)?;
            seen_labels
                .insert(symbol.label)
                .then_some((rank, ordinal, symbol))
        })
        .collect::<Vec<_>>();
    items.sort_by(
        |(left_rank, left_ordinal, left), (right_rank, right_ordinal, right)| {
            left_rank
                .cmp(right_rank)
                .then_with(|| {
                    completion_item_kind_order(left).cmp(&completion_item_kind_order(right))
                })
                .then_with(|| {
                    left.label
                        .to_ascii_lowercase()
                        .cmp(&right.label.to_ascii_lowercase())
                })
                .then_with(|| left_ordinal.cmp(right_ordinal))
        },
    );
    items
        .into_iter()
        .enumerate()
        .map(|(sort_index, (_, _, symbol))| {
            json!({
                "label": symbol.label,
                "kind": completion_item_kind(symbol),
                "detail": symbol.detail,
                "documentation": {
                    "kind": "markdown",
                    "value": symbol.documentation
                },
                "filterText": symbol.label,
                "insertText": symbol.insert_text,
                "insertTextFormat": 2,
                "sortText": format!("{sort_index:04}:{}", symbol.label)
            })
        })
        .collect()
}

fn completion_match_rank(prefix: &str, label: &str) -> Option<u8> {
    let label = label.to_ascii_lowercase();
    if prefix.is_empty() || label == prefix {
        Some(0)
    } else if label.starts_with(prefix) {
        Some(1)
    } else if label.contains(prefix) {
        Some(2)
    } else {
        None
    }
}

fn completion_item_kind(symbol: &AclSymbol) -> u64 {
    match completion_item_kind_order(symbol) {
        0 => 24,
        1 => 4,
        2 => 21,
        3 => 3,
        _ => 14,
    }
}

fn completion_item_kind_order(symbol: &AclSymbol) -> u8 {
    if is_operator_label(symbol.label) || symbol.detail.contains("operator") {
        0
    } else if symbol.detail.contains("constructor") {
        1
    } else if symbol.detail.contains("constant") {
        2
    } else if symbol.label.starts_with("op ")
        || symbol.detail.contains("function")
        || symbol.detail.contains("helper")
    {
        3
    } else {
        4
    }
}

fn is_operator_label(label: &str) -> bool {
    matches!(
        label,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "++"
            | "=="
            | "!="
            | ">"
            | ">="
            | "<"
            | "<="
            | "&&"
            | "||"
            | "!"
            | "."
            | "..."
    )
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
    hover_for_static_symbol(token)
}

pub(super) fn hover_for_token_with_workspace(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    let token = token.trim();
    if token.is_empty() {
        return Value::Null;
    }

    if is_ail_source_uri(uri) {
        match source_hover_for_token(uri, text, token, workspace_documents) {
            SourceHoverLookup::Found(hover) => return hover,
            SourceHoverLookup::Ambiguous => return Value::Null,
            SourceHoverLookup::NotFound => {}
        }
    }

    hover_for_static_symbol(token)
}

fn hover_for_static_symbol(token: &str) -> Value {
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

#[derive(Debug, Clone)]
struct SourceHoverCandidate {
    label: String,
    kind: &'static str,
    signature: String,
    detail: String,
    uri: String,
    line: usize,
    start: usize,
    end: usize,
}

enum SourceHoverLookup {
    Found(Value),
    Ambiguous,
    NotFound,
}

fn source_hover_for_token(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> SourceHoverLookup {
    match source_hover_lookup_from_matches(source_hover_candidates_in_text(uri, text, token)) {
        SourceHoverLookup::Found(hover) => return SourceHoverLookup::Found(hover),
        SourceHoverLookup::Ambiguous => return SourceHoverLookup::Ambiguous,
        SourceHoverLookup::NotFound => {}
    }

    let Some(root_path) = file_path_from_uri(uri) else {
        return SourceHoverLookup::NotFound;
    };
    let Ok(canonical_root) = std::fs::canonicalize(&root_path) else {
        return SourceHoverLookup::NotFound;
    };
    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    source_hover_for_imports(
        &canonical_root,
        text,
        token,
        workspace_documents,
        &mut visited,
    )
}

fn source_hover_for_imports(
    source_path: &Path,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
    visited: &mut BTreeSet<PathBuf>,
) -> SourceHoverLookup {
    let mut candidates = Vec::new();
    let mut ambiguous = false;

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

        match source_hover_lookup_from_matches(source_hover_candidates_in_text(
            &imported_uri,
            &imported_text,
            token,
        )) {
            SourceHoverLookup::Found(hover) => candidates.push(hover),
            SourceHoverLookup::Ambiguous => ambiguous = true,
            SourceHoverLookup::NotFound => {}
        }

        match source_hover_for_imports(
            &canonical,
            &imported_text,
            token,
            workspace_documents,
            visited,
        ) {
            SourceHoverLookup::Found(hover) => candidates.push(hover),
            SourceHoverLookup::Ambiguous => ambiguous = true,
            SourceHoverLookup::NotFound => {}
        }
    }

    if ambiguous || candidates.len() > 1 {
        SourceHoverLookup::Ambiguous
    } else {
        source_hover_lookup_from_values(candidates)
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

fn source_hover_lookup_from_matches(candidates: Vec<SourceHoverCandidate>) -> SourceHoverLookup {
    source_hover_lookup_from_values(candidates.into_iter().map(source_hover_json).collect())
}

fn source_hover_lookup_from_values(mut candidates: Vec<Value>) -> SourceHoverLookup {
    match candidates.len() {
        0 => SourceHoverLookup::NotFound,
        1 => SourceHoverLookup::Found(candidates.remove(0)),
        _ => SourceHoverLookup::Ambiguous,
    }
}

fn source_hover_json(candidate: SourceHoverCandidate) -> Value {
    json!({
        "contents": {
            "kind": "markdown",
            "value": format!(
                "```ail\n{}\n```\n\n**{}** — {}\n\nDefined in `{}`.",
                candidate.signature, candidate.kind, candidate.detail, candidate.uri
            )
        },
        "range": {
            "start": { "line": candidate.line, "character": candidate.start },
            "end": { "line": candidate.line, "character": candidate.end }
        },
        "data": {
            "language": "ail-source",
            "kind": candidate.kind,
            "label": candidate.label,
            "detail": candidate.detail,
            "uri": candidate.uri
        }
    })
}

fn source_hover_candidates_in_text(
    uri: &str,
    text: &str,
    token: &str,
) -> Vec<SourceHoverCandidate> {
    let mut candidates = Vec::new();
    let module = source_module_from_text(text);
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();

        if let Some(rest) = trimmed.strip_prefix("use ") {
            candidates.extend(source_import_hover_candidate(
                uri,
                token,
                rest,
                line_idx,
                leading + "use ".len(),
            ));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("fn ") {
            if let Some(candidate) = source_function_hover_candidate(
                uri,
                token,
                module.as_deref(),
                rest,
                line_idx,
                leading + "fn ".len(),
            ) {
                candidates.push(candidate);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("const ") {
            if let Some(candidate) = source_const_hover_candidate(
                uri,
                token,
                module.as_deref(),
                rest,
                line_idx,
                leading + "const ".len(),
            ) {
                candidates.push(candidate);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("test ") {
            if let Some(candidate) =
                source_test_hover_candidate(uri, token, rest, line_idx, leading + "test ".len())
            {
                candidates.push(candidate);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("capability ") {
            if let Some(candidate) = source_capability_hover_candidate(
                uri,
                token,
                rest,
                line_idx,
                leading + "capability ".len(),
            ) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn source_import_hover_candidate(
    uri: &str,
    token: &str,
    rest: &str,
    line_idx: usize,
    start_offset: usize,
) -> Option<SourceHoverCandidate> {
    let import_start = rest.find('"')? + 1;
    let import_end = rest[import_start..].find('"')? + import_start;
    let import = &rest[import_start..import_end];
    let file_name = Path::new(import)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(import);
    if token != import && token != file_name {
        return None;
    }
    Some(SourceHoverCandidate {
        label: import.to_string(),
        kind: "import",
        signature: format!("use \"{import}\""),
        detail: format!("Imports `{import}` into this source module."),
        uri: uri.to_string(),
        line: line_idx,
        start: start_offset + import_start,
        end: start_offset + import_end,
    })
}

fn source_function_hover_candidate(
    uri: &str,
    token: &str,
    module: Option<&str>,
    rest: &str,
    line_idx: usize,
    start_offset: usize,
) -> Option<SourceHoverCandidate> {
    let name_end = rest.find('(')?;
    let name = rest[..name_end].trim();
    if !source_name_matches_token(name, module, token, "fn.") {
        return None;
    }
    let signature_end = rest.find('=').unwrap_or(rest.len());
    let signature = rest[..signature_end].trim();
    Some(SourceHoverCandidate {
        label: qualified_source_name(name, module, None),
        kind: "function",
        signature: format!("fn {signature}"),
        detail: "Typed AIL source function.".to_string(),
        uri: uri.to_string(),
        line: line_idx,
        start: start_offset,
        end: start_offset + name.len(),
    })
}

fn source_const_hover_candidate(
    uri: &str,
    token: &str,
    module: Option<&str>,
    rest: &str,
    line_idx: usize,
    start_offset: usize,
) -> Option<SourceHoverCandidate> {
    let name_end = rest.find(':')?;
    let name = rest[..name_end].trim();
    if !source_name_matches_token(name, module, token, "fn.") {
        return None;
    }
    let type_end = rest.find('=').unwrap_or(rest.len());
    let signature = rest[..type_end].trim();
    Some(SourceHoverCandidate {
        label: qualified_source_name(name, module, None),
        kind: "constant",
        signature: format!("const {signature}"),
        detail: "Typed top-level AIL source constant.".to_string(),
        uri: uri.to_string(),
        line: line_idx,
        start: start_offset,
        end: start_offset + name.len(),
    })
}

fn source_test_hover_candidate(
    uri: &str,
    token: &str,
    rest: &str,
    line_idx: usize,
    start_offset: usize,
) -> Option<SourceHoverCandidate> {
    let name_end = [rest.find("->"), rest.find('=')]
        .into_iter()
        .flatten()
        .min()?;
    let name = rest[..name_end].trim();
    if !source_test_name_matches_token(name, token) {
        return None;
    }
    let signature_end = rest.find('=').unwrap_or(rest.len());
    let signature = rest[..signature_end].trim();
    Some(SourceHoverCandidate {
        label: qualified_source_name(name, None, Some("test.")),
        kind: "test",
        signature: format!("test {signature}"),
        detail: "Executable AIL source test discovered by `ail test --file`.".to_string(),
        uri: uri.to_string(),
        line: line_idx,
        start: start_offset,
        end: start_offset + name.len(),
    })
}

fn source_capability_hover_candidate(
    uri: &str,
    token: &str,
    rest: &str,
    line_idx: usize,
    start_offset: usize,
) -> Option<SourceHoverCandidate> {
    let name = rest.split_whitespace().next().unwrap_or_default();
    if name != token {
        return None;
    }
    Some(SourceHoverCandidate {
        label: name.to_string(),
        kind: "capability",
        signature: format!("capability {name}"),
        detail: "Declared external capability that source functions or tests may grant."
            .to_string(),
        uri: uri.to_string(),
        line: line_idx,
        start: start_offset,
        end: start_offset + name.len(),
    })
}

fn source_name_matches_token(name: &str, module: Option<&str>, token: &str, prefix: &str) -> bool {
    if name == token || name.strip_prefix(prefix) == Some(token) {
        return true;
    }
    if token == format!("{prefix}{name}") {
        return true;
    }
    let Some(module) = module else {
        return false;
    };
    let bare_name = name.strip_prefix(prefix).unwrap_or(name);
    token == format!("{module}.{bare_name}")
}

fn source_test_name_matches_token(name: &str, token: &str) -> bool {
    name == token || token == format!("test.{name}") || name.strip_prefix("test.") == Some(token)
}
