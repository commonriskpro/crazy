use serde_json::{Value, json};

use ail_change::op_schema::validate_op_schemas;
use ail_change::parser::parse_changeset;

use crate::source_commands::{load_source_program_from_text, parse_ail_source};

use super::source_helpers::{file_path_from_uri, is_ail_source_uri};

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
            .map(|err| diagnostic(0, err.to_string(), "ail-acl-schema"))
            .collect(),
        Err(err) => vec![diagnostic(line_from_error(&err), err, "ail-acl-parser")],
    }
}

fn diagnostics_for_ail_source_text(_uri: &str, text: &str) -> Vec<Value> {
    match parse_ail_source(text) {
        Ok(_) => vec![],
        Err(err) => {
            let message = err.to_string();
            vec![diagnostic(
                line_from_error(&message),
                message,
                "ail-source-parser",
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
        Err(err) => vec![diagnostic(
            line_from_error(&err.to_string()),
            err.to_string(),
            "ail-source-import",
        )],
    }
}

fn diagnostic(line: u64, message: String, source: &str) -> Value {
    json!({
        "range": {
            "start": { "line": line, "character": 0 },
            "end": { "line": line, "character": 1 }
        },
        "severity": 1,
        "source": source,
        "message": message,
    })
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
