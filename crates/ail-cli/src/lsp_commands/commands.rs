use std::path::PathBuf;

use serde_json::{Value, json};

use crate::error::CliError;
use crate::output::{OutputMode, print_response};

use super::definition::definition_for_token;
use super::diagnostics::{diagnostics_for_acl_text, diagnostics_for_ail_source_path};
use super::protocol::run_stdio_lsp;
use super::references::references_for_token;
use super::source_helpers::{is_ail_source_uri, language_for_uri};
use super::symbols::{completion_items, hover_for_token};

pub(crate) fn cmd_lsp(
    mode: OutputMode,
    stdio: bool,
    diagnose: Option<PathBuf>,
    complete: Option<String>,
    hover_token: Option<String>,
    definition_token: Option<String>,
    definition_file: Option<PathBuf>,
    references_token: Option<String>,
    references_file: Option<PathBuf>,
) -> Result<(), CliError> {
    if let Some(path) = diagnose {
        return cmd_lsp_diagnose(mode, path);
    }
    if let Some(prefix) = complete {
        return cmd_lsp_complete(mode, &prefix);
    }
    if let Some(token) = hover_token {
        return cmd_lsp_hover(mode, &token);
    }
    if let Some(token) = definition_token {
        return cmd_lsp_definition(mode, definition_file, &token);
    }
    if let Some(token) = references_token {
        return cmd_lsp_references(mode, references_file, &token);
    }
    if stdio {
        return run_stdio_lsp();
    }
    Err(CliError::Domain(
        "lsp requires --stdio, --diagnose <file>, --complete <prefix>, --hover-token <token>, --definition-token <token> --definition-file <file>, or --references-token <token> --references-file <file>".to_string(),
    ))
}

fn cmd_lsp_diagnose(mode: OutputMode, path: PathBuf) -> Result<(), CliError> {
    let text = std::fs::read_to_string(&path)?;
    let uri = format!("file://{}", path.display());
    let diagnostics = if is_ail_source_uri(&uri) {
        diagnostics_for_ail_source_path(&path, &text)
    } else {
        diagnostics_for_acl_text(&uri, &text)
    };
    let diagnostic_count = diagnostics.len();
    let failed = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["severity"] == 1)
        .count();
    let warning_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["severity"] == 2)
        .count();
    let diagnostic_codes = diagnostic_codes(&diagnostics);
    let diagnostic_categories = diagnostic_categories(&diagnostics);
    let repair_codes = diagnostic_repair_codes(&diagnostics);
    let repair_suggestions = diagnostic_repair_suggestions(&diagnostics);
    let repair_count = repair_suggestions.len();
    let diagnostics_status = if failed > 0 {
        "error"
    } else if warning_count > 0 {
        "warning"
    } else {
        "clean"
    };
    let human_msg = if diagnostics.is_empty() {
        format!("LSP diagnostics: clean\nfile: {}", path.display())
    } else {
        format!(
            "LSP diagnostics: {diagnostics_status}\nerrors: {failed}\nwarnings: {warning_count}\nrepairs: {repair_count}\nfile: {}\n{}",
            path.display(),
            diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic["message"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    print_response(
        mode,
        &human_msg,
        json!({
            "uri": uri,
            "diagnostics_status": diagnostics_status,
            "diagnostics": diagnostics,
            "diagnostic_count": diagnostic_count,
            "error_count": failed,
            "warning_count": warning_count,
            "diagnostic_codes": diagnostic_codes,
            "diagnostic_categories": diagnostic_categories,
            "repair_count": repair_count,
            "repair_codes": repair_codes,
            "repair_suggestions": repair_suggestions,
            "language": language_for_uri(&uri),
        }),
    );
    Ok(())
}

fn diagnostic_codes(diagnostics: &[Value]) -> Vec<String> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic["code"].as_str())
        .fold(Vec::new(), push_unique)
}

fn diagnostic_categories(diagnostics: &[Value]) -> Vec<String> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic["data"]["ailDiagnostic"]["category"].as_str())
        .fold(Vec::new(), push_unique)
}

fn diagnostic_repair_codes(diagnostics: &[Value]) -> Vec<String> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic["data"]["ailRepair"]["code"].as_str())
        .fold(Vec::new(), push_unique)
}

fn diagnostic_repair_suggestions(diagnostics: &[Value]) -> Vec<Value> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let repair = &diagnostic["data"]["ailRepair"];
            let repair_code = repair["code"].as_str()?;
            Some(json!({
                "diagnostic_code": diagnostic["code"].clone(),
                "diagnostic_message": diagnostic["message"].clone(),
                "repair_code": repair_code,
                "range": diagnostic["range"].clone(),
                "edit": repair["edit"].clone(),
            }))
        })
        .collect()
}

fn push_unique(mut values: Vec<String>, value: &str) -> Vec<String> {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
    values
}

fn cmd_lsp_complete(mode: OutputMode, prefix: &str) -> Result<(), CliError> {
    let items = completion_items(prefix);
    let completion_count = items.len();
    let completion_labels = completion_labels(&items);
    let completion_kinds = completion_kinds(&items);
    print_response(
        mode,
        &format!("LSP completions: {completion_count} item(s)"),
        json!({
            "prefix": prefix,
            "completion_count": completion_count,
            "completion_labels": completion_labels,
            "completion_kinds": completion_kinds,
            "items": items,
        }),
    );
    Ok(())
}

fn completion_labels(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item["label"].as_str())
        .map(str::to_string)
        .collect()
}

fn completion_kinds(items: &[Value]) -> Vec<u64> {
    items
        .iter()
        .filter_map(|item| item["kind"].as_u64())
        .fold(Vec::new(), push_unique_u64)
}

fn push_unique_u64(mut values: Vec<u64>, value: u64) -> Vec<u64> {
    if !values.contains(&value) {
        values.push(value);
    }
    values
}

fn cmd_lsp_hover(mode: OutputMode, token: &str) -> Result<(), CliError> {
    let hover = hover_for_token(token);
    let hover_found = !hover.is_null();
    let hover_kind = hover["contents"]["kind"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| "none".to_string());
    let hover_markdown = hover["contents"]["value"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_default();
    print_response(
        mode,
        if !hover_found {
            "LSP hover: no information"
        } else {
            "LSP hover: found"
        },
        json!({
            "token": token,
            "hover_found": hover_found,
            "hover_kind": hover_kind,
            "hover_markdown": hover_markdown,
            "hover": hover,
        }),
    );
    Ok(())
}

fn cmd_lsp_definition(
    mode: OutputMode,
    file: Option<PathBuf>,
    token: &str,
) -> Result<(), CliError> {
    let Some(path) = file else {
        return Err(CliError::Domain(
            "lsp --definition-token requires --definition-file <file>".to_string(),
        ));
    };
    let text = std::fs::read_to_string(&path)?;
    let uri = format!("file://{}", path.display());
    let definition = definition_for_token(&uri, &text, token);
    let definition_uri = definition
        .get("uri")
        .and_then(Value::as_str)
        .map(str::to_string);
    let definition_line = definition
        .pointer("/range/start/line")
        .and_then(Value::as_u64);
    let definition_character = definition
        .pointer("/range/start/character")
        .and_then(Value::as_u64);
    let definition_found =
        definition_uri.is_some() && definition_line.is_some() && definition_character.is_some();
    print_response(
        mode,
        if definition_found {
            "LSP definition: found"
        } else {
            "LSP definition: not found"
        },
        json!({
            "token": token,
            "uri": uri,
            "definition_found": definition_found,
            "definition_uri": definition_uri,
            "definition_line": definition_line,
            "definition_character": definition_character,
            "definition": definition,
        }),
    );
    Ok(())
}

fn cmd_lsp_references(
    mode: OutputMode,
    file: Option<PathBuf>,
    token: &str,
) -> Result<(), CliError> {
    let Some(path) = file else {
        return Err(CliError::Domain(
            "lsp --references-token requires --references-file <file>".to_string(),
        ));
    };
    let text = std::fs::read_to_string(&path)?;
    let uri = format!("file://{}", path.display());
    let references = references_for_token(&uri, &text, token);
    let reference_count = references.len();
    let reference_uris = reference_uris(&references);
    let references_found = reference_count > 0;
    print_response(
        mode,
        &format!("LSP references: {reference_count} location(s)"),
        json!({
            "token": token,
            "uri": uri,
            "references_found": references_found,
            "reference_count": reference_count,
            "reference_uris": reference_uris,
            "references": references,
        }),
    );
    Ok(())
}

fn reference_uris(references: &[Value]) -> Vec<String> {
    references
        .iter()
        .filter_map(|reference| reference["uri"].as_str())
        .fold(Vec::new(), push_unique)
}
