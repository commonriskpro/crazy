use serde_json::{Value, json};

use ail_change::op_schema::validate_op_schemas;
use ail_change::parser::parse_changeset;

use crate::source_commands::{load_source_program_from_text, parse_ail_source};

use super::source_helpers::{file_path_from_uri, is_ail_source_uri};

pub(super) const LSP_DIAGNOSTIC_ACL_PARSER: &str = "AIL_ACL_PARSER";
pub(super) const LSP_DIAGNOSTIC_ACL_SCHEMA: &str = "AIL_ACL_SCHEMA";
pub(super) const LSP_DIAGNOSTIC_SOURCE_IMPORT: &str = "AIL_SOURCE_IMPORT";
pub(super) const LSP_DIAGNOSTIC_SOURCE_PARSER: &str = "AIL_SOURCE_PARSER";

pub(super) fn diagnostics_for_document(uri: &str, text: &str) -> Vec<Value> {
    if is_ail_source_uri(uri) {
        if let Some(path) = file_path_from_uri(uri) {
            return diagnostics_for_ail_source_document_path(&path, text);
        }
        return diagnostics_for_ail_source_text(uri, text);
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

pub(super) fn diagnostics_for_ail_source_path(path: &std::path::Path, text: &str) -> Vec<Value> {
    diagnostics_for_ail_source_document_path(path, text)
}

fn diagnostics_for_ail_source_document_path(path: &std::path::Path, text: &str) -> Vec<Value> {
    let syntax_diagnostics =
        diagnostics_for_ail_source_text(&format!("file://{}", path.display()), text);
    if !syntax_diagnostics.is_empty() {
        return syntax_diagnostics;
    }

    match load_source_program_from_text(path, text) {
        Ok(_) => vec![],
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
