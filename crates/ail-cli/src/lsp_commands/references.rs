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
        return references_for_ail_source_token(uri, text, token, workspace_documents);
    }

    references_in_text(uri, text, token)
}

fn references_for_ail_source_token(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Vec<Value> {
    let query = SourceReferenceQuery::from_token(token);
    let mut refs = source_references_in_text(uri, text, &query);
    let Some(root_path) = file_path_from_uri(uri) else {
        return refs;
    };
    let Ok(canonical_root) = std::fs::canonicalize(&root_path) else {
        return refs;
    };

    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    let mut definition_count = source_definitions_in_text(text, &query);
    collect_ail_source_import_definition_count(
        &canonical_root,
        text,
        &query,
        workspace_documents,
        &mut visited,
        &mut definition_count,
    );
    if definition_count > 1 {
        return Vec::new();
    }

    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    collect_ail_source_import_references(
        &canonical_root,
        text,
        &query,
        workspace_documents,
        &mut visited,
        &mut refs,
    );
    refs
}

fn collect_ail_source_import_definition_count(
    source_path: &Path,
    text: &str,
    query: &SourceReferenceQuery<'_>,
    workspace_documents: &BTreeMap<String, String>,
    visited: &mut BTreeSet<PathBuf>,
    definition_count: &mut usize,
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
        *definition_count += source_definitions_in_text(&imported_text, query);
        collect_ail_source_import_definition_count(
            &canonical,
            &imported_text,
            query,
            workspace_documents,
            visited,
            definition_count,
        );
    }
}

fn collect_ail_source_import_references(
    source_path: &Path,
    text: &str,
    query: &SourceReferenceQuery<'_>,
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
            query,
        ));
        collect_ail_source_import_references(
            &canonical,
            &imported_text,
            query,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceSymbolKind {
    Function,
    Const,
    Test,
    Capability,
}

#[derive(Debug, Clone, Copy)]
struct SourceReferenceQuery<'a> {
    raw: &'a str,
    local: &'a str,
    module: Option<&'a str>,
    kind: Option<SourceSymbolKind>,
}

impl<'a> SourceReferenceQuery<'a> {
    fn from_token(raw: &'a str) -> Self {
        if let Some(local) = raw.strip_prefix("fn.") {
            Self {
                raw,
                local,
                module: None,
                kind: Some(SourceSymbolKind::Function),
            }
        } else if let Some(local) = raw.strip_prefix("test.") {
            Self {
                raw,
                local,
                module: None,
                kind: Some(SourceSymbolKind::Test),
            }
        } else if let Some((module, local)) = raw.split_once('.') {
            Self {
                raw,
                local,
                module: Some(module),
                kind: None,
            }
        } else {
            Self {
                raw,
                local: raw,
                module: None,
                kind: None,
            }
        }
    }

    fn allows(self, kind: SourceSymbolKind) -> bool {
        self.kind.map_or(true, |expected| expected == kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceReferenceNeedleScope {
    Exact,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceReferenceNeedle {
    token: String,
    scope: SourceReferenceNeedleScope,
}

fn source_references_in_text(
    uri: &str,
    text: &str,
    query: &SourceReferenceQuery<'_>,
) -> Vec<Value> {
    let needles = source_reference_needles_for_text(text, query);
    let mut refs: Vec<_> = needles
        .iter()
        .flat_map(|needle| source_references_for_needle_in_text(uri, text, query, needle))
        .collect();
    refs.sort_by_key(reference_sort_key);
    refs.dedup_by(|left, right| reference_sort_key(left) == reference_sort_key(right));
    refs
}

fn reference_sort_key(reference: &Value) -> (u64, u64, u64) {
    (
        reference["range"]["start"]["line"].as_u64().unwrap_or(0),
        reference["range"]["start"]["character"]
            .as_u64()
            .unwrap_or(0),
        reference["range"]["end"]["character"].as_u64().unwrap_or(0),
    )
}

fn source_reference_needles_for_text(
    text: &str,
    query: &SourceReferenceQuery<'_>,
) -> Vec<SourceReferenceNeedle> {
    let module = source_module_from_text(text);
    let mut needles = Vec::new();
    push_source_reference_needle(&mut needles, query.raw, SourceReferenceNeedleScope::Exact);

    let should_search_local = query.kind.is_some()
        || query
            .module
            .is_some_and(|query_module| module.as_deref() == Some(query_module));
    if should_search_local {
        push_source_reference_needle(&mut needles, query.local, SourceReferenceNeedleScope::Local);
    }

    needles
}

fn push_source_reference_needle(
    needles: &mut Vec<SourceReferenceNeedle>,
    token: &str,
    scope: SourceReferenceNeedleScope,
) {
    let needle = SourceReferenceNeedle {
        token: token.to_string(),
        scope,
    };
    if !needles.iter().any(|existing| existing == &needle) {
        needles.push(needle);
    }
}

fn source_references_for_needle_in_text(
    uri: &str,
    text: &str,
    query: &SourceReferenceQuery<'_>,
    needle: &SourceReferenceNeedle,
) -> Vec<Value> {
    text.lines()
        .enumerate()
        .flat_map(|(line_idx, line)| {
            source_reference_ranges_in_line(uri, line_idx, line, query, needle)
        })
        .collect()
}

fn source_reference_ranges_in_line(
    uri: &str,
    line_idx: usize,
    line: &str,
    query: &SourceReferenceQuery<'_>,
    needle: &SourceReferenceNeedle,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(offset) = line[search_from..].find(&needle.token) {
        let start = search_from + offset;
        let end = start + needle.token.len();
        if token_has_boundaries(line, start, end)
            && source_reference_match_allowed(line, start, end, query, needle)
        {
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

fn source_reference_match_allowed(
    line: &str,
    start: usize,
    end: usize,
    query: &SourceReferenceQuery<'_>,
    needle: &SourceReferenceNeedle,
) -> bool {
    if needle.scope == SourceReferenceNeedleScope::Exact {
        return true;
    }

    if let Some(kind) = source_declaration_kind_for_range(line, start, end) {
        return query.allows(kind);
    }

    query.kind != Some(SourceSymbolKind::Test)
}

fn source_definitions_in_text(text: &str, query: &SourceReferenceQuery<'_>) -> usize {
    let module = source_module_from_text(text);
    text.lines()
        .filter_map(source_declaration_in_line)
        .filter(|(kind, name)| {
            query.allows(*kind)
                && source_decl_name_matches_query(name, module.as_deref(), query, *kind)
        })
        .count()
}

fn source_declaration_kind_for_range(
    line: &str,
    start: usize,
    end: usize,
) -> Option<SourceSymbolKind> {
    let (kind, name, name_start) = source_declaration_in_line(line)?;
    let name_end = name_start + name.len();
    (start >= name_start && end <= name_end).then_some(kind)
}

fn source_declaration_in_line(line: &str) -> Option<(SourceSymbolKind, &str, usize)> {
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();

    if let Some(rest) = trimmed.strip_prefix("fn ") {
        let name_end = rest.find('(')?;
        let name = rest[..name_end].trim();
        return Some((SourceSymbolKind::Function, name, leading + "fn ".len()));
    }
    if let Some(rest) = trimmed.strip_prefix("const ") {
        let name_end = rest.find(':')?;
        let name = rest[..name_end].trim();
        return Some((SourceSymbolKind::Const, name, leading + "const ".len()));
    }
    if let Some(rest) = trimmed.strip_prefix("test ") {
        let name_end = [rest.find("->"), rest.find('=')]
            .into_iter()
            .flatten()
            .min()?;
        let name = rest[..name_end].trim();
        return Some((SourceSymbolKind::Test, name, leading + "test ".len()));
    }
    if let Some(rest) = trimmed.strip_prefix("capability ") {
        let name = rest.split_whitespace().next().unwrap_or_default();
        return Some((
            SourceSymbolKind::Capability,
            name,
            leading + "capability ".len(),
        ));
    }

    None
}

fn source_decl_name_matches_query(
    name: &str,
    module: Option<&str>,
    query: &SourceReferenceQuery<'_>,
    kind: SourceSymbolKind,
) -> bool {
    if kind == SourceSymbolKind::Capability {
        return name == query.raw;
    }

    let prefix = match kind {
        SourceSymbolKind::Function | SourceSymbolKind::Const => "fn.",
        SourceSymbolKind::Test => "test.",
        SourceSymbolKind::Capability => "",
    };

    if name == query.raw || name.strip_prefix(prefix) == Some(query.raw) {
        return true;
    }

    let bare_name = name.strip_prefix(prefix).unwrap_or(name);
    if let Some(query_module) = query.module {
        module == Some(query_module) && bare_name == query.local
    } else {
        bare_name == query.local
    }
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
        if token_has_boundaries(line, start, end) {
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

fn token_has_boundaries(line: &str, start: usize, end: usize) -> bool {
    let before = line[..start].chars().next_back();
    let after = line[end..].chars().next();
    let boundary_before = before.map_or(true, |ch| !is_acl_token_char(ch));
    let boundary_after = after.map_or(true, |ch| !is_acl_token_char(ch));
    boundary_before && boundary_after
}
