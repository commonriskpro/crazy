use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::source_helpers::{
    byte_index_to_lsp_character, file_path_from_uri, is_acl_token_char, is_ail_source_uri,
    resolve_lsp_source_import, source_imports_from_text, source_module_from_text,
    source_test_name_end,
};
use super::tokens::token_range_at_position;

const CATEGORY_DOCUMENT_STATE: &str = "document_state";
const CATEGORY_SYMBOL_RESOLUTION: &str = "symbol_resolution";
const CATEGORY_UNSUPPORTED: &str = "unsupported";

pub(super) fn references_for_token(uri: &str, text: &str, token: &str) -> Vec<Value> {
    references_for_token_with_workspace(uri, text, token, &BTreeMap::new())
}

pub(super) fn references_for_token_with_workspace(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Vec<Value> {
    reference_lookup_for_token_with_workspace(uri, text, token, workspace_documents).references
}

pub(super) fn references_diagnostic_at_position(
    uri: &str,
    text: &str,
    line: usize,
    character: usize,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    if !is_ail_source_uri(uri) {
        return blocked(
            "unsupported_target",
            "AIL_REFERENCES_UNSUPPORTED_TARGET",
            CATEGORY_UNSUPPORTED,
            "reference diagnostics are currently available for .ail source documents only",
            json!({
                "language": "unsupported",
                "documentUriRedacted": true,
            }),
        );
    }

    let Some(token_range) = token_range_at_position(text, line, character) else {
        return blocked(
            "missing_token",
            "AIL_REFERENCES_MISSING_TOKEN",
            CATEGORY_SYMBOL_RESOLUTION,
            "position is not on an identifier token",
            json!({ "line": line, "character": character }),
        );
    };
    let token = token_range.token.trim();
    if !is_reference_identifier(token) {
        return blocked(
            "unsupported_target",
            "AIL_REFERENCES_UNSUPPORTED_TARGET",
            CATEGORY_UNSUPPORTED,
            "reference target is not a supported .ail identifier",
            json!({
                "target": token_descriptor(token),
                "range": token_range_json(line, token_range.start_character, token_range.end_character),
            }),
        );
    }

    let result = reference_lookup_for_token_with_workspace(uri, text, token, workspace_documents);
    let mut diagnostics = result.diagnostics;
    if result.definition_count > 1 {
        diagnostics.push(reference_diagnostic(
            "ambiguous_symbol",
            "AIL_REFERENCES_AMBIGUOUS_SYMBOL",
            CATEGORY_SYMBOL_RESOLUTION,
            "token resolves to multiple reference targets",
            json!({
                "token": token_descriptor(token),
                "candidateCount": result.definition_count,
                "candidateLocationsRedacted": true,
            }),
        ));
    } else if result.definition_count == 0 {
        diagnostics.push(reference_diagnostic(
            "unresolved_symbol",
            "AIL_REFERENCES_UNRESOLVED_SYMBOL",
            CATEGORY_SYMBOL_RESOLUTION,
            "token does not resolve to a reference target",
            json!({
                "token": token_descriptor(token),
                "importDiagnosticCount": diagnostics.len(),
            }),
        ));
    }
    sort_reference_diagnostics(&mut diagnostics);
    let diagnostic_count = diagnostics.len();
    let reference_count = result.references.len();
    json!({
        "ok": !has_error_diagnostic(&diagnostics),
        "references": result.references,
        "referenceCount": reference_count,
        "diagnostics": diagnostics,
        "diagnosticCount": diagnostic_count,
    })
}

pub(super) fn missing_document_references_failure() -> Value {
    blocked(
        "missing_document",
        "AIL_REFERENCES_MISSING_DOCUMENT",
        CATEGORY_DOCUMENT_STATE,
        "document is not open in this LSP session",
        json!({ "documentState": "not_open", "documentUriRedacted": true }),
    )
}

struct ReferenceResult {
    references: Vec<Value>,
    diagnostics: Vec<Value>,
    definition_count: usize,
}

impl ReferenceResult {
    fn new(references: Vec<Value>, diagnostics: Vec<Value>, definition_count: usize) -> Self {
        Self {
            references,
            diagnostics,
            definition_count,
        }
    }
}

fn reference_lookup_for_token_with_workspace(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> ReferenceResult {
    let token = token.trim();
    if token.is_empty() {
        return ReferenceResult::new(Vec::new(), Vec::new(), 0);
    }

    if is_ail_source_uri(uri) {
        return references_for_ail_source_token(uri, text, token, workspace_documents);
    }

    ReferenceResult::new(references_in_text(uri, text, token), Vec::new(), 1)
}

fn references_for_ail_source_token(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> ReferenceResult {
    let query = SourceReferenceQuery::from_token(token);
    let canonical_root = file_path_from_uri(uri).and_then(|path| std::fs::canonicalize(path).ok());
    // Use the canonical URI for the root document so reference ordering stays
    // deterministic across files: imported documents are always discovered via
    // their canonical path, and mixing a non-canonical root URI (e.g. /var vs
    // /private/var on macOS) would sort references by filesystem aliasing.
    let root_uri = canonical_root
        .as_ref()
        .map(|path| format!("file://{}", path.display()))
        .unwrap_or_else(|| uri.to_string());
    let mut refs = source_references_in_text(&root_uri, text, &query);
    let mut definition_count = source_definitions_in_text(text, &query);
    let Some(canonical_root) = canonical_root else {
        return ReferenceResult::new(refs, Vec::new(), definition_count);
    };

    let mut diagnostics = Vec::new();
    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    collect_ail_source_import_definition_count(
        &canonical_root,
        text,
        &query,
        workspace_documents,
        &mut visited,
        &mut definition_count,
        &mut diagnostics,
    );
    if definition_count > 1 {
        return ReferenceResult::new(Vec::new(), diagnostics, definition_count);
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
    sort_locations(&mut refs);
    refs.dedup_by(|left, right| location_sort_key(left) == location_sort_key(right));
    ReferenceResult::new(refs, diagnostics, definition_count)
}

fn collect_ail_source_import_definition_count(
    source_path: &Path,
    text: &str,
    query: &SourceReferenceQuery<'_>,
    workspace_documents: &BTreeMap<String, String>,
    visited: &mut BTreeSet<PathBuf>,
    definition_count: &mut usize,
    diagnostics: &mut Vec<Value>,
) {
    for import in sorted_source_imports(text) {
        let path = resolve_lsp_source_import(source_path, &import);
        let Ok(canonical) = std::fs::canonicalize(&path) else {
            diagnostics.push(skipped_import_diagnostic(&import, "unresolved_import"));
            continue;
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let imported_uri = format!("file://{}", canonical.display());
        if !is_ail_source_uri(&imported_uri) {
            diagnostics.push(skipped_import_diagnostic(&import, "unsupported_language"));
            continue;
        }
        let imported_text =
            match workspace_document_text(workspace_documents, &imported_uri, &canonical) {
                Some(text) => text.to_string(),
                None => match std::fs::read_to_string(&canonical) {
                    Ok(text) => text,
                    Err(_) => {
                        diagnostics.push(skipped_import_diagnostic(&import, "unreadable_import"));
                        continue;
                    }
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
            diagnostics,
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
    for import in sorted_source_imports(text) {
        let path = resolve_lsp_source_import(source_path, &import);
        let Ok(canonical) = std::fs::canonicalize(&path) else {
            continue;
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let imported_uri = format!("file://{}", canonical.display());
        if !is_ail_source_uri(&imported_uri) {
            continue;
        }
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

fn sorted_source_imports(text: &str) -> Vec<String> {
    let mut imports = source_imports_from_text(text);
    imports.sort();
    imports.dedup();
    imports
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
    sort_locations(&mut refs);
    refs.dedup_by(|left, right| location_sort_key(left) == location_sort_key(right));
    refs
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
            out.push(location_for_byte_range(uri, line_idx, line, start, end));
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
        .filter(|(kind, name, _)| {
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
        let name_end = source_test_name_end(rest)?;
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
    let mut refs: Vec<_> = text
        .lines()
        .enumerate()
        .flat_map(|(line_idx, line)| token_ranges_in_line(uri, line_idx, line, token))
        .collect();
    sort_locations(&mut refs);
    refs
}

fn token_ranges_in_line(uri: &str, line_idx: usize, line: &str, token: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(offset) = line[search_from..].find(token) {
        let start = search_from + offset;
        let end = start + token.len();
        if token_has_boundaries(line, start, end) {
            out.push(location_for_byte_range(uri, line_idx, line, start, end));
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
        "ok": false,
        "references": [],
        "referenceCount": 0,
        "diagnostics": [reference_diagnostic(reason, code, category, message, descriptor)],
        "diagnosticCount": 1,
    })
}

fn reference_diagnostic(
    reason: &str,
    code: &str,
    category: &str,
    message: &str,
    descriptor: Value,
) -> Value {
    let severity = match reason {
        "skipped_import" => "warning",
        _ => "error",
    };
    json!({
        "reason": reason,
        "code": code,
        "category": category,
        "severity": severity,
        "message": message,
        "descriptor": descriptor,
    })
}

fn skipped_import_diagnostic(import: &str, state: &str) -> Value {
    reference_diagnostic(
        "skipped_import",
        "AIL_REFERENCES_SKIPPED_IMPORT",
        CATEGORY_UNSUPPORTED,
        "imported document was skipped during reference lookup",
        json!({
            "importLength": import.chars().count(),
            "importKind": if Path::new(import).is_absolute() { "absolute" } else { "relative" },
            "importState": state,
            "importPathRedacted": true,
            "documentUriRedacted": true,
        }),
    )
}

fn has_error_diagnostic(diagnostics: &[Value]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic["severity"] == "error")
}

fn sort_reference_diagnostics(diagnostics: &mut [Value]) {
    diagnostics.sort_by(|left, right| {
        reference_diagnostic_sort_key(left)
            .cmp(&reference_diagnostic_sort_key(right))
            .then_with(|| left.to_string().cmp(&right.to_string()))
    });
}

fn reference_diagnostic_sort_key(diagnostic: &Value) -> (String, String, String) {
    (
        diagnostic["code"].as_str().unwrap_or_default().to_string(),
        diagnostic["reason"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        diagnostic["descriptor"]["importState"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

fn is_reference_identifier(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(is_acl_token_char)
        && token
            .chars()
            .any(|ch| ch.is_ascii_alphabetic() || ch == '_')
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

fn token_range_json(line: usize, start: usize, end: usize) -> Value {
    json!({
        "start": { "line": line, "character": start },
        "end": { "line": line, "character": end }
    })
}

fn location_for_byte_range(
    uri: &str,
    line_idx: usize,
    line: &str,
    start: usize,
    end: usize,
) -> Value {
    json!({
        "uri": uri,
        "range": {
            "start": { "line": line_idx, "character": byte_index_to_lsp_character(line, start) },
            "end": { "line": line_idx, "character": byte_index_to_lsp_character(line, end) }
        }
    })
}
