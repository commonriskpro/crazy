use serde_json::{Value, json};

use ail_change::op_schema::validate_op_schemas;
use ail_change::parser::parse_changeset;

use crate::source_commands::{
    SourceIgnoredExpressionStatement, SourceUnusedBinding, load_source_program_from_text,
    parse_ail_source, source_ignored_expression_statement_diagnostics,
    source_unused_binding_diagnostics,
};

use super::source_helpers::{file_path_from_uri, is_ail_source_uri};

pub(super) const LSP_DIAGNOSTIC_ACL_PARSER: &str = "AIL_ACL_PARSER";
pub(super) const LSP_DIAGNOSTIC_ACL_SCHEMA: &str = "AIL_ACL_SCHEMA";
pub(super) const LSP_DIAGNOSTIC_SOURCE_IMPORT: &str = "AIL_SOURCE_IMPORT";
pub(super) const LSP_DIAGNOSTIC_SOURCE_IGNORED_EXPRESSION: &str =
    "AIL_SOURCE_LSP_IGNORED_EXPRESSION";
pub(super) const LSP_DIAGNOSTIC_SOURCE_PARSER: &str = "AIL_SOURCE_PARSER";
pub(super) const LSP_DIAGNOSTIC_SOURCE_UNUSED_BINDING: &str = "AIL_SOURCE_LSP_UNUSED_BINDING";

pub(super) fn diagnostics_for_document(uri: &str, text: &str) -> Vec<Value> {
    if is_ail_source_uri(uri) {
        if let Some(path) = file_path_from_uri(uri) {
            if path.exists() {
                return diagnostics_for_ail_source_document_path(&path, text);
            }
            return diagnostics_for_ail_source_inline(uri, text);
        }
        return diagnostics_for_ail_source_inline(uri, text);
    }
    diagnostics_for_acl_text(uri, text)
}

pub(super) fn diagnostics_for_acl_text(_uri: &str, text: &str) -> Vec<Value> {
    match parse_changeset(text) {
        Ok(parsed) => validate_op_schemas(&parsed)
            .into_iter()
            .map(|err| {
                diagnostic(
                    0,
                    0,
                    1,
                    err.to_string(),
                    "ail-acl-schema",
                    LSP_DIAGNOSTIC_ACL_SCHEMA,
                )
            })
            .collect(),
        Err(err) => vec![diagnostic_for_text(
            text,
            line_from_error(&err),
            err,
            "ail-acl-parser",
            LSP_DIAGNOSTIC_ACL_PARSER,
        )],
    }
}

fn diagnostics_for_ail_source_text(_uri: &str, text: &str) -> Vec<Value> {
    match parse_ail_source(text) {
        Ok(_) => vec![],
        Err(err) => {
            let message = err.to_string();
            vec![diagnostic_for_source_error(
                text,
                line_from_error(&message),
                message,
                "ail-source-parser",
                LSP_DIAGNOSTIC_SOURCE_PARSER,
            )]
        }
    }
}

fn diagnostics_for_ail_source_inline(uri: &str, text: &str) -> Vec<Value> {
    let syntax_diagnostics = diagnostics_for_ail_source_text(uri, text);
    if !syntax_diagnostics.is_empty() {
        return syntax_diagnostics;
    }
    source_ignored_expression_statement_diagnostics(text)
        .into_iter()
        .map(|ignored| ignored_expression_statement_diagnostic(uri, text, ignored))
        .chain(
            source_unused_binding_diagnostics(text)
                .into_iter()
                .map(|unused| unused_binding_diagnostic(uri, text, unused)),
        )
        .collect()
}

pub(super) fn diagnostics_for_ail_source_path(path: &std::path::Path, text: &str) -> Vec<Value> {
    diagnostics_for_ail_source_document_path(path, text)
}

fn diagnostics_for_ail_source_document_path(path: &std::path::Path, text: &str) -> Vec<Value> {
    let uri = format!("file://{}", path.display());
    let syntax_diagnostics = diagnostics_for_ail_source_text(&uri, text);
    if !syntax_diagnostics.is_empty() {
        return syntax_diagnostics;
    }

    match load_source_program_from_text(path, text) {
        Ok(_) => source_ignored_expression_statement_diagnostics(text)
            .into_iter()
            .map(|ignored| ignored_expression_statement_diagnostic(&uri, text, ignored))
            .chain(
                source_unused_binding_diagnostics(text)
                    .into_iter()
                    .map(|unused| unused_binding_diagnostic(&uri, text, unused)),
            )
            .collect(),
        Err(err) => {
            let message = err.to_string();
            vec![diagnostic_for_source_error(
                text,
                line_from_error(&message),
                message,
                "ail-source-import",
                LSP_DIAGNOSTIC_SOURCE_IMPORT,
            )]
        }
    }
}

fn diagnostic_for_text(text: &str, line: u64, message: String, source: &str, code: &str) -> Value {
    let (start_character, end_character) =
        diagnostic_character_range(text, line, &message).unwrap_or((0, 1));
    diagnostic(line, start_character, end_character, message, source, code)
}

fn diagnostic_for_source_error(
    text: &str,
    line: u64,
    message: String,
    source: &str,
    fallback_code: &str,
) -> Value {
    let metadata = source_diagnostic_metadata(&message);
    let code = metadata
        .as_ref()
        .map(|metadata| metadata.code.as_str())
        .unwrap_or(fallback_code);
    let range = metadata
        .as_ref()
        .and_then(|metadata| metadata.span.as_ref())
        .map(|span| {
            (
                span.start_line,
                span.start_character,
                span.end_line,
                span.end_character,
            )
        });
    let mut diagnostic = if let Some((start_line, start_character, end_line, end_character)) = range
    {
        diagnostic_with_range(
            start_line,
            start_character,
            end_line,
            end_character,
            message,
            source,
            code,
        )
    } else {
        diagnostic_for_text(text, line, message, source, code)
    };
    if let Some(metadata) = metadata {
        let span = metadata.span.as_ref().map(|span| {
            json!({
                "start": { "line": span.start_line, "character": span.start_character },
                "end": { "line": span.end_line, "character": span.end_character },
            })
        });
        diagnostic["data"] = json!({
            "ailDiagnostic": {
                "code": metadata.code,
                "category": metadata.category,
                "family": fallback_code,
                "span": span,
            }
        });
    }
    diagnostic
}

fn ignored_expression_statement_diagnostic(
    uri: &str,
    text: &str,
    ignored: SourceIgnoredExpressionStatement,
) -> Value {
    let line = ignored.line_num.saturating_sub(1) as u64;
    let (start_character, end_character) = line_character_range(text, line).unwrap_or((0, 1));
    let message = "ignored expression statement has no direct effect; assign it with `let` or make it the returned expression".to_string();
    let mut diagnostic = diagnostic_with_range_and_severity(
        line,
        start_character,
        line,
        end_character,
        message,
        "ail-source-lint",
        LSP_DIAGNOSTIC_SOURCE_IGNORED_EXPRESSION,
        2,
    );
    let span = json!({
        "start": { "line": line, "character": start_character },
        "end": { "line": line, "character": end_character },
    });
    diagnostic["data"] = json!({
        "ailDiagnostic": {
            "code": LSP_DIAGNOSTIC_SOURCE_IGNORED_EXPRESSION,
            "category": "source.lsp.ignored_expression",
            "family": "AIL_SOURCE_LSP",
            "span": span,
        },
        "ailRepair": {
            "code": "remove.ignored_expression_statement",
            "edit": delete_line_workspace_edit(uri, line),
        }
    });
    diagnostic
}

fn unused_binding_diagnostic(uri: &str, text: &str, unused: SourceUnusedBinding) -> Value {
    let line = unused.line_num.saturating_sub(1) as u64;
    let (start_character, end_character) = line_character_range(text, line).unwrap_or((0, 1));
    let message = format!(
        "unused local binding `{}`; remove it or prefix with `_` to mark it intentionally unused",
        unused.name
    );
    let mut diagnostic = diagnostic_with_range_and_severity(
        line,
        start_character,
        line,
        end_character,
        message,
        "ail-source-lint",
        LSP_DIAGNOSTIC_SOURCE_UNUSED_BINDING,
        2,
    );
    let span = json!({
        "start": { "line": line, "character": start_character },
        "end": { "line": line, "character": end_character },
    });
    let repair = unused_binding_prefix_workspace_edit(uri, text, &unused, line);
    diagnostic["data"] = json!({
        "ailDiagnostic": {
            "code": LSP_DIAGNOSTIC_SOURCE_UNUSED_BINDING,
            "category": "source.lsp.unused_binding",
            "family": "AIL_SOURCE_LSP",
            "span": span,
        },
        "ailRepair": {
            "code": "prefix.unused_binding_with_underscore",
            "edit": repair,
        }
    });
    diagnostic
}

fn unused_binding_prefix_workspace_edit(
    uri: &str,
    text: &str,
    unused: &SourceUnusedBinding,
    line: u64,
) -> Value {
    let character = unused_binding_name_character(text, line, &unused.name).unwrap_or(0);
    let mut changes = serde_json::Map::new();
    changes.insert(
        uri.to_string(),
        json!([{
                "range": {
                    "start": { "line": line, "character": character },
                    "end": { "line": line, "character": character }
                },
                "newText": "_"
        }]),
    );
    json!({ "changes": changes })
}

fn unused_binding_name_character(text: &str, line: u64, name: &str) -> Option<u64> {
    let line_text = text.lines().nth(line as usize)?;
    let byte_idx = line_text.find(name)?;
    Some(line_text[..byte_idx].chars().count() as u64)
}

fn delete_line_workspace_edit(uri: &str, line: u64) -> Value {
    let mut changes = serde_json::Map::new();
    changes.insert(
        uri.to_string(),
        json!([{
            "range": {
                "start": { "line": line, "character": 0 },
                "end": { "line": line + 1, "character": 0 }
            },
            "newText": ""
        }]),
    );
    json!({ "changes": changes })
}

fn line_character_range(text: &str, line: u64) -> Option<(u64, u64)> {
    let line_text = text.lines().nth(line as usize)?;
    let start = line_text
        .chars()
        .position(|ch| !ch.is_whitespace())
        .unwrap_or(0) as u64;
    let end = line_text.chars().count().max(start as usize + 1) as u64;
    Some((start, end))
}

struct SourceDiagnosticMetadata {
    code: String,
    category: String,
    span: Option<SourceDiagnosticSpan>,
}

struct SourceDiagnosticSpan {
    start_line: u64,
    start_character: u64,
    end_line: u64,
    end_character: u64,
}

fn source_diagnostic_metadata(message: &str) -> Option<SourceDiagnosticMetadata> {
    let code_start = message.find("[AIL_SOURCE_")? + 1;
    let code_end = message[code_start..].find(']')? + code_start;
    let code = &message[code_start..code_end];
    if !is_lsp_diagnostic_identifier(code) {
        return None;
    }

    let category_start = message[code_end..].find("category=")? + code_end + "category=".len();
    let category = message[category_start..]
        .split(|ch: char| ch.is_whitespace() || ch == ':' || ch == ',')
        .next()
        .unwrap_or_default();
    if !is_lsp_diagnostic_identifier(category) {
        return None;
    }

    Some(SourceDiagnosticMetadata {
        code: code.to_string(),
        category: category.to_string(),
        span: source_diagnostic_span(message),
    })
}

fn diagnostic(
    line: u64,
    start_character: u64,
    end_character: u64,
    message: String,
    source: &str,
    code: &str,
) -> Value {
    json!({
        "range": {
            "start": { "line": line, "character": start_character },
            "end": { "line": line, "character": end_character }
        },
        "severity": 1,
        "source": source,
        "code": code,
        "message": message,
    })
}

fn diagnostic_with_range(
    start_line: u64,
    start_character: u64,
    end_line: u64,
    end_character: u64,
    message: String,
    source: &str,
    code: &str,
) -> Value {
    json!({
        "range": {
            "start": { "line": start_line, "character": start_character },
            "end": { "line": end_line, "character": end_character }
        },
        "severity": 1,
        "source": source,
        "code": code,
        "message": message,
    })
}

fn diagnostic_with_range_and_severity(
    start_line: u64,
    start_character: u64,
    end_line: u64,
    end_character: u64,
    message: String,
    source: &str,
    code: &str,
    severity: u64,
) -> Value {
    json!({
        "range": {
            "start": { "line": start_line, "character": start_character },
            "end": { "line": end_line, "character": end_character }
        },
        "severity": severity,
        "source": source,
        "code": code,
        "message": message,
    })
}

fn diagnostic_character_range(text: &str, line: u64, message: &str) -> Option<(u64, u64)> {
    let token = diagnostic_focus_token(message)?;
    let line_text = text.lines().nth(line as usize)?;
    let byte_start = line_text.find(token)?;
    let start = line_text[..byte_start].chars().count() as u64;
    let end = start + token.chars().count() as u64;
    Some((start, end))
}

fn diagnostic_focus_token(message: &str) -> Option<&str> {
    [
        "unknown variable `",
        "unknown function call `",
        "function call `",
    ]
    .into_iter()
    .find_map(|prefix| quoted_value_after(message, prefix))
}

fn quoted_value_after<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let start = message.find(prefix)? + prefix.len();
    let rest = &message[start..];
    let end = rest.find('`')?;
    Some(&rest[..end])
}

fn is_lsp_diagnostic_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

fn source_diagnostic_span(message: &str) -> Option<SourceDiagnosticSpan> {
    let start = message.find("span=")? + "span=".len();
    let value = message[start..].split_whitespace().next()?;
    if value == "<unknown>" {
        return None;
    }
    let (start_position, end_position) = value.split_once("..")?;
    let (start_line, start_character) = source_diagnostic_position(start_position)?;
    let (end_line, mut end_character) = source_diagnostic_position(end_position)?;
    if end_line == start_line {
        end_character = end_character.max(start_character + 1);
    }
    Some(SourceDiagnosticSpan {
        start_line,
        start_character,
        end_line,
        end_character,
    })
}

fn source_diagnostic_position(position: &str) -> Option<(u64, u64)> {
    let (line, character) = position.split_once(':')?;
    Some((
        line.parse::<u64>().ok()?.checked_sub(1)?,
        character.parse::<u64>().ok()?.checked_sub(1)?,
    ))
}

fn line_from_error(err: &str) -> u64 {
    let Some(rest) = err.split_once("line ") else {
        return 0;
    };
    let number = rest
        .1
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    number
        .parse::<u64>()
        .ok()
        .and_then(|line| line.checked_sub(1))
        .unwrap_or(0)
}
