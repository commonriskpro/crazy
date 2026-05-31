use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::source_helpers::{
    file_path_from_uri, is_acl_token_char, is_ail_source_uri, resolve_lsp_source_import,
    source_imports_from_text, source_module_from_text,
};
use super::tokens::token_range_at_position;

pub(super) fn definition_for_token(uri: &str, text: &str, token: &str) -> Value {
    definition_for_token_with_workspace(uri, text, token, &BTreeMap::new())
}

pub(super) fn definition_for_token_with_workspace(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    match definition_lookup_for_token_with_workspace(uri, text, token, workspace_documents).lookup {
        DefinitionLookup::Found(definition) => definition,
        DefinitionLookup::Ambiguous(_) => empty_definition_result(),
        DefinitionLookup::NotFound => Value::Null,
    }
}

pub(super) fn definition_diagnostic_at_position(
    uri: &str,
    text: &str,
    line: usize,
    character: usize,
    workspace_documents: &BTreeMap<String, String>,
) -> Value {
    if !is_ail_source_uri(uri) {
        return blocked(
            "unsupported_target",
            "AIL_DEFINITION_UNSUPPORTED_TARGET",
            CATEGORY_UNSUPPORTED,
            "definition diagnostics are currently available for .ail source documents only",
            json!({
                "language": "unsupported",
                "documentUriRedacted": true,
            }),
        );
    }

    let Some(token_range) = token_range_at_position(text, line, character) else {
        return blocked(
            "missing_token",
            "AIL_DEFINITION_MISSING_TOKEN",
            CATEGORY_SYMBOL_RESOLUTION,
            "position is not on an identifier token",
            json!({ "line": line, "character": character }),
        );
    };
    let token = token_range.token.trim();
    if !is_definition_identifier(token) {
        return blocked(
            "unsupported_target",
            "AIL_DEFINITION_UNSUPPORTED_TARGET",
            CATEGORY_UNSUPPORTED,
            "definition target is not a supported .ail identifier",
            json!({
                "target": token_descriptor(token),
                "range": token_range_json(line, token_range.start, token_range.end),
            }),
        );
    }

    let result = definition_lookup_for_token_with_workspace(uri, text, token, workspace_documents);
    let mut diagnostics = result.diagnostics;
    let definition = match result.lookup {
        DefinitionLookup::Found(definition) => definition,
        DefinitionLookup::Ambiguous(candidates) => {
            diagnostics.push(definition_diagnostic(
                "ambiguous_symbol",
                "AIL_DEFINITION_AMBIGUOUS_SYMBOL",
                CATEGORY_SYMBOL_RESOLUTION,
                "token resolves to multiple definition candidates",
                json!({
                    "token": token_descriptor(token),
                    "candidateCount": candidates.len(),
                    "candidateLocationsRedacted": true,
                }),
            ));
            Value::Null
        }
        DefinitionLookup::NotFound => {
            diagnostics.push(definition_diagnostic(
                "unresolved_symbol",
                "AIL_DEFINITION_UNRESOLVED_SYMBOL",
                CATEGORY_SYMBOL_RESOLUTION,
                "token does not resolve to a definition",
                json!({
                    "token": token_descriptor(token),
                    "importDiagnosticCount": diagnostics.len(),
                }),
            ));
            Value::Null
        }
    };
    sort_definition_diagnostics(&mut diagnostics);
    let diagnostic_count = diagnostics.len();
    json!({
        "ok": definition.is_object() && !has_error_diagnostic(&diagnostics),
        "definition": definition,
        "diagnostics": diagnostics,
        "diagnosticCount": diagnostic_count,
    })
}

pub(super) fn missing_document_definition_failure() -> Value {
    blocked(
        "missing_document",
        "AIL_DEFINITION_MISSING_DOCUMENT",
        CATEGORY_DOCUMENT_STATE,
        "document is not open in this LSP session",
        json!({ "documentState": "not_open", "documentUriRedacted": true }),
    )
}

fn definition_lookup_for_token_with_workspace(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> DefinitionResult {
    let token = token.trim();
    if token.is_empty() {
        return DefinitionResult::not_found();
    }

    if is_ail_source_uri(uri) {
        let result = definition_for_ail_source_token(uri, text, token, workspace_documents);
        if !matches!(&result.lookup, DefinitionLookup::NotFound) || !result.diagnostics.is_empty() {
            return result;
        }
    }

    let mut definitions = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let needle = format!("id={token}");
        if let Some(start) = line.find(&needle).map(|idx| idx + "id=".len()) {
            definitions.push(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + token.len() }
                }
            }));
        }
    }

    DefinitionResult {
        lookup: definition_lookup_from_matches(definitions),
        diagnostics: Vec::new(),
    }
}

fn definition_for_ail_source_token(
    uri: &str,
    text: &str,
    token: &str,
    workspace_documents: &BTreeMap<String, String>,
) -> DefinitionResult {
    let query = SourceDefinitionQuery::from_token(token);
    match definition_lookup_from_matches(source_definitions_in_text(uri, text, &query)) {
        DefinitionLookup::Found(definition) => {
            return DefinitionResult::found(definition);
        }
        DefinitionLookup::Ambiguous(candidates) => {
            return DefinitionResult::ambiguous(candidates);
        }
        DefinitionLookup::NotFound => {}
    }

    let Some(root_path) = file_path_from_uri(uri) else {
        return DefinitionResult::not_found();
    };
    let Ok(canonical_root) = std::fs::canonicalize(&root_path) else {
        return DefinitionResult::not_found();
    };
    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    let mut diagnostics = Vec::new();
    let lookup = definition_for_ail_source_imports(
        &canonical_root,
        text,
        &query,
        workspace_documents,
        &mut visited,
        &mut diagnostics,
    );
    DefinitionResult {
        lookup,
        diagnostics,
    }
}

fn definition_for_ail_source_imports(
    source_path: &Path,
    text: &str,
    query: &SourceDefinitionQuery<'_>,
    workspace_documents: &BTreeMap<String, String>,
    visited: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Value>,
) -> DefinitionLookup {
    let mut definitions = Vec::new();
    let mut ambiguous_candidates = Vec::new();

    let mut imports = source_imports_from_text(text);
    imports.sort();
    imports.dedup();
    for import in imports {
        let path = resolve_lsp_source_import(source_path, &import);
        let Ok(canonical) = std::fs::canonicalize(&path) else {
            diagnostics.push(unsupported_import_diagnostic(&import, "unresolved_import"));
            continue;
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let imported_uri = format!("file://{}", canonical.display());
        if !is_ail_source_uri(&imported_uri) {
            diagnostics.push(unsupported_import_diagnostic(
                &import,
                "unsupported_language",
            ));
            continue;
        }
        let imported_text =
            match workspace_document_text(workspace_documents, &imported_uri, &canonical) {
                Some(text) => text.to_string(),
                None => match std::fs::read_to_string(&canonical) {
                    Ok(text) => text,
                    Err(_) => {
                        diagnostics
                            .push(unsupported_import_diagnostic(&import, "unreadable_import"));
                        continue;
                    }
                },
            };

        match definition_lookup_from_matches(source_definitions_in_text(
            &imported_uri,
            &imported_text,
            query,
        )) {
            DefinitionLookup::Found(definition) => definitions.push(definition),
            DefinitionLookup::Ambiguous(candidates) => ambiguous_candidates.extend(candidates),
            DefinitionLookup::NotFound => {}
        }

        match definition_for_ail_source_imports(
            &canonical,
            &imported_text,
            query,
            workspace_documents,
            visited,
            diagnostics,
        ) {
            DefinitionLookup::Found(definition) => definitions.push(definition),
            DefinitionLookup::Ambiguous(candidates) => ambiguous_candidates.extend(candidates),
            DefinitionLookup::NotFound => {}
        }
    }

    if !ambiguous_candidates.is_empty() {
        ambiguous_candidates.extend(definitions);
        definition_lookup_from_matches(ambiguous_candidates)
    } else {
        definition_lookup_from_matches(definitions)
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
enum SourceDefinitionKind {
    Function,
    Const,
    Test,
    Capability,
}

#[derive(Debug, Clone, Copy)]
struct SourceDefinitionQuery<'a> {
    token: &'a str,
    kind: Option<SourceDefinitionKind>,
}

impl<'a> SourceDefinitionQuery<'a> {
    fn from_token(token: &'a str) -> Self {
        if let Some(token) = token.strip_prefix("fn.") {
            Self {
                token,
                kind: Some(SourceDefinitionKind::Function),
            }
        } else if let Some(token) = token.strip_prefix("test.") {
            Self {
                token,
                kind: Some(SourceDefinitionKind::Test),
            }
        } else {
            Self { token, kind: None }
        }
    }

    fn allows(self, kind: SourceDefinitionKind) -> bool {
        self.kind.map_or(true, |expected| expected == kind)
    }
}

enum DefinitionLookup {
    Found(Value),
    Ambiguous(Vec<Value>),
    NotFound,
}

struct DefinitionResult {
    lookup: DefinitionLookup,
    diagnostics: Vec<Value>,
}

impl DefinitionResult {
    fn found(definition: Value) -> Self {
        Self {
            lookup: DefinitionLookup::Found(definition),
            diagnostics: Vec::new(),
        }
    }

    fn ambiguous(candidates: Vec<Value>) -> Self {
        Self {
            lookup: DefinitionLookup::Ambiguous(candidates),
            diagnostics: Vec::new(),
        }
    }

    fn not_found() -> Self {
        Self {
            lookup: DefinitionLookup::NotFound,
            diagnostics: Vec::new(),
        }
    }
}

fn definition_lookup_from_matches(mut definitions: Vec<Value>) -> DefinitionLookup {
    sort_definition_locations(&mut definitions);
    definitions.dedup();
    match definitions.len() {
        0 => DefinitionLookup::NotFound,
        1 => DefinitionLookup::Found(definitions.remove(0)),
        _ => DefinitionLookup::Ambiguous(definitions),
    }
}

fn empty_definition_result() -> Value {
    Value::Array(Vec::new())
}

const CATEGORY_DOCUMENT_STATE: &str = "document_state";
const CATEGORY_SYMBOL_RESOLUTION: &str = "symbol_resolution";
const CATEGORY_UNSUPPORTED: &str = "unsupported";

fn unsupported_import_diagnostic(import: &str, reason: &str) -> Value {
    definition_diagnostic(
        "unsupported_import",
        "AIL_DEFINITION_UNSUPPORTED_IMPORT",
        CATEGORY_UNSUPPORTED,
        "import could not be used for definition lookup",
        json!({
            "importLength": import.chars().count(),
            "importKind": if std::path::Path::new(import).is_absolute() {
                "absolute"
            } else {
                "relative"
            },
            "importState": reason,
            "importPathRedacted": true,
        }),
    )
}

fn blocked(reason: &str, code: &str, category: &str, message: &str, descriptor: Value) -> Value {
    json!({
        "ok": false,
        "definition": Value::Null,
        "diagnostics": [definition_diagnostic(reason, code, category, message, descriptor)],
        "diagnosticCount": 1,
    })
}

fn definition_diagnostic(
    reason: &str,
    code: &str,
    category: &str,
    message: &str,
    descriptor: Value,
) -> Value {
    let severity = match reason {
        "unsupported_import" => "warning",
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

fn has_error_diagnostic(diagnostics: &[Value]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic["severity"] == "error")
}

fn sort_definition_diagnostics(diagnostics: &mut [Value]) {
    diagnostics.sort_by(|left, right| {
        definition_diagnostic_sort_key(left)
            .cmp(&definition_diagnostic_sort_key(right))
            .then_with(|| left.to_string().cmp(&right.to_string()))
    });
}

fn definition_diagnostic_sort_key(diagnostic: &Value) -> (String, String) {
    (
        diagnostic["code"].as_str().unwrap_or_default().to_string(),
        diagnostic["reason"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

fn sort_definition_locations(definitions: &mut [Value]) {
    definitions.sort_by(|left, right| {
        definition_location_sort_key(left)
            .cmp(&definition_location_sort_key(right))
            .then_with(|| left.to_string().cmp(&right.to_string()))
    });
}

fn definition_location_sort_key(definition: &Value) -> (String, u64, u64, u64, u64) {
    (
        definition["uri"].as_str().unwrap_or_default().to_string(),
        definition["range"]["start"]["line"].as_u64().unwrap_or(0),
        definition["range"]["start"]["character"]
            .as_u64()
            .unwrap_or(0),
        definition["range"]["end"]["line"].as_u64().unwrap_or(0),
        definition["range"]["end"]["character"]
            .as_u64()
            .unwrap_or(0),
    )
}

fn is_definition_identifier(token: &str) -> bool {
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

fn source_definitions_in_text(
    uri: &str,
    text: &str,
    query: &SourceDefinitionQuery<'_>,
) -> Vec<Value> {
    let mut definitions = Vec::new();
    if query.allows(SourceDefinitionKind::Function) {
        definitions.extend(source_function_definitions_in_text(uri, text, query.token));
    }
    if query.allows(SourceDefinitionKind::Const) {
        definitions.extend(source_const_definitions_in_text(uri, text, query.token));
    }
    if query.allows(SourceDefinitionKind::Test) {
        definitions.extend(source_test_definitions_in_text(uri, text, query.token));
    }
    if query.allows(SourceDefinitionKind::Capability) {
        definitions.extend(source_capability_definitions_in_text(
            uri,
            text,
            query.token,
        ));
    }
    definitions
}

fn source_function_definitions_in_text(uri: &str, text: &str, token: &str) -> Vec<Value> {
    let module = source_module_from_text(text);
    let mut definitions = Vec::new();
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
            definitions.push(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    definitions
}

fn source_const_definitions_in_text(uri: &str, text: &str, token: &str) -> Vec<Value> {
    let module = source_module_from_text(text);
    let mut definitions = Vec::new();
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
            definitions.push(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    definitions
}

fn source_test_definitions_in_text(uri: &str, text: &str, token: &str) -> Vec<Value> {
    let module = source_module_from_text(text);
    let mut definitions = Vec::new();
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
            definitions.push(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    definitions
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

fn source_capability_definitions_in_text(uri: &str, text: &str, token: &str) -> Vec<Value> {
    let mut definitions = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("capability ") else {
            continue;
        };
        let name = rest.split_whitespace().next().unwrap_or_default();
        if name == token {
            let leading = line.len() - trimmed.len();
            let start = leading + "capability ".len();
            definitions.push(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    definitions
}
