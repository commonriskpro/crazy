use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};

use serde_json::{Value, json};

use crate::error::CliError;

use super::definition::{
    definition_diagnostic_at_position, definition_for_token_with_workspace,
    missing_document_definition_failure,
};
use super::diagnostics::{
    LSP_DIAGNOSTIC_ACL_PARSER, LSP_DIAGNOSTIC_ACL_SCHEMA, LSP_DIAGNOSTIC_SOURCE_IMPORT,
    LSP_DIAGNOSTIC_SOURCE_PARSER, diagnostics_for_document,
};
use super::references::{
    missing_document_references_failure, references_diagnostic_at_position,
    references_for_token_with_workspace,
};
use super::rename::{
    missing_document_rename_failure, prepare_rename_at_position, rename_candidate_at_position,
    rename_edits_at_position, rename_workspace_edit_at_position,
};
use super::source_helpers::{is_acl_token_char, is_ail_source_uri};
use super::symbols::{
    completion_items, hover_for_token_with_workspace, workspace_symbol_diagnostic_response,
    workspace_symbol_items_with_root,
};
use super::tokens::{SEMANTIC_TOKEN_TYPES, semantic_token_data_for_source, token_at_position};

pub(super) fn run_stdio_lsp() -> Result<(), CliError> {
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    let mut session = LspSession::default();

    while let Some(message) = read_lsp_message(&mut reader)? {
        let response = session.handle_lsp_message(&message);
        for outbound in response {
            write_lsp_message(&mut writer, &outbound)?;
        }
        writer.flush()?;
    }
    Ok(())
}

#[derive(Default)]
struct LspSession {
    documents: BTreeMap<String, String>,
    workspace_root_uri: Option<String>,
}

fn read_lsp_message(reader: &mut impl BufRead) -> Result<Option<Value>, CliError> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse::<usize>().map_err(|_| {
                CliError::ParseError(format!("invalid LSP Content-Length header: {trimmed}"))
            })?);
        }
    }

    let Some(len) = content_length else {
        return Err(CliError::ParseError(
            "missing LSP Content-Length header".to_string(),
        ));
    };
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| CliError::ParseError(format!("invalid LSP JSON message: {e}")))
}

fn write_lsp_message(writer: &mut impl Write, value: &Value) -> Result<(), CliError> {
    let body = serde_json::to_vec(value)
        .map_err(|e| CliError::Domain(format!("failed to encode LSP response: {e}")))?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    Ok(())
}

impl LspSession {
    fn handle_lsp_message(&mut self, message: &Value) -> Vec<Value> {
        let method = message["method"].as_str().unwrap_or_default();
        match method {
            "initialize" => {
                self.workspace_root_uri = workspace_root_uri_from_initialize(message);
                vec![lsp_response(
                    message,
                    json!({
                    "capabilities": {
                        "textDocumentSync": 1,
                        "diagnosticProvider": {
                            "interFileDependencies": true,
                            "workspaceDiagnostics": false
                        },
                        "completionProvider": {
                            "triggerCharacters": [" ", "_", ".", "+", "-", "*", "/", "%", "!", "=", ">", "<", "&", "|", "{"]
                        },
                        "hoverProvider": true,
                        "codeActionProvider": {
                            "codeActionKinds": ["quickfix"]
                        },
                        "definitionProvider": true,
                        "referencesProvider": true,
                        "renameProvider": {
                            "prepareProvider": true
                        },
                        "workspaceSymbolProvider": true,
                        "semanticTokensProvider": {
                            "legend": {
                                "tokenTypes": SEMANTIC_TOKEN_TYPES,
                                "tokenModifiers": []
                            },
                            "full": true
                        }
                    },
                    "serverInfo": {
                        "name": "ail-lsp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                    }),
                )]
            }
            "shutdown" => vec![lsp_response(message, Value::Null)],
            "textDocument/didOpen" | "textDocument/didChange" => {
                let params = &message["params"];
                let doc = &params["textDocument"];
                let uri = doc["uri"].as_str().unwrap_or("file://unknown");
                let text = if method == "textDocument/didOpen" {
                    doc["text"].as_str().unwrap_or_default().to_string()
                } else {
                    params["contentChanges"]
                        .as_array()
                        .and_then(|changes| changes.last())
                        .and_then(|change| change["text"].as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                self.documents.insert(uri.to_string(), text.clone());
                vec![json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": {
                        "uri": uri,
                        "diagnostics": diagnostics_for_document(uri, &text)
                    }
                })]
            }
            "textDocument/completion" => vec![lsp_response(
                message,
                json!({
                    "isIncomplete": false,
                    "items": completion_items(&self.completion_prefix(message))
                }),
            )],
            "textDocument/codeAction" => {
                vec![lsp_response(message, self.code_actions(message))]
            }
            "textDocument/hover" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let hover = self
                    .documents
                    .get(uri)
                    .and_then(|text| {
                        token_at_position(text, line, character).map(|token| {
                            hover_for_token_with_workspace(uri, text, &token, &self.documents)
                        })
                    })
                    .unwrap_or(Value::Null);
                vec![lsp_response(message, hover)]
            }
            "textDocument/definition" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = self
                    .documents
                    .get(uri)
                    .and_then(|text| {
                        token_at_position(text, line, character).map(|token| {
                            definition_for_token_with_workspace(uri, text, &token, &self.documents)
                        })
                    })
                    .unwrap_or(Value::Null);
                vec![lsp_response(message, result)]
            }
            "ail/definition" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = self
                    .documents
                    .get(uri)
                    .map(|text| {
                        definition_diagnostic_at_position(
                            uri,
                            text,
                            line,
                            character,
                            &self.documents,
                        )
                    })
                    .unwrap_or_else(missing_document_definition_failure);
                vec![lsp_response(message, result)]
            }

            "workspace/symbol" => {
                let query = message["params"]["query"].as_str().unwrap_or_default();
                vec![lsp_response(
                    message,
                    Value::Array(workspace_symbol_items_with_root(
                        query,
                        self.workspace_root_uri.as_deref(),
                        &self.documents,
                    )),
                )]
            }
            "ail/workspaceSymbols" => {
                let result = workspace_symbol_diagnostic_response(
                    &message["params"],
                    self.workspace_root_uri.as_deref(),
                    &self.documents,
                );
                vec![lsp_response(message, result)]
            }
            "textDocument/prepareRename" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = self
                    .documents
                    .get(uri)
                    .map(|text| {
                        prepare_rename_at_position(uri, text, line, character, &self.documents)
                    })
                    .unwrap_or(Value::Null);
                vec![lsp_response(message, result)]
            }
            "ail/renameCandidates" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = self
                    .documents
                    .get(uri)
                    .map(|text| {
                        rename_candidate_at_position(uri, text, line, character, &self.documents)
                    })
                    .unwrap_or_else(missing_document_rename_failure);
                vec![lsp_response(message, result)]
            }

            "textDocument/rename" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let new_name = params["newName"].as_str().unwrap_or_default();
                let result = self
                    .documents
                    .get(uri)
                    .map(|text| {
                        rename_workspace_edit_at_position(
                            uri,
                            text,
                            line,
                            character,
                            new_name,
                            &self.documents,
                        )
                    })
                    .unwrap_or(Value::Null);
                vec![lsp_response(message, result)]
            }
            "ail/renameEdits" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let new_name = params["newName"].as_str().unwrap_or_default();
                let result = self
                    .documents
                    .get(uri)
                    .map(|text| {
                        rename_edits_at_position(
                            uri,
                            text,
                            line,
                            character,
                            new_name,
                            &self.documents,
                        )
                    })
                    .unwrap_or_else(missing_document_rename_failure);
                vec![lsp_response(message, result)]
            }
            "textDocument/semanticTokens/full" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let data = self
                    .documents
                    .get(uri)
                    .filter(|_| is_ail_source_uri(uri))
                    .map(|text| semantic_token_data_for_source(text))
                    .unwrap_or_default();
                vec![lsp_response(message, json!({ "data": data }))]
            }
            "textDocument/references" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = self
                    .documents
                    .get(uri)
                    .and_then(|text| {
                        token_at_position(text, line, character).map(|token| {
                            Value::Array(references_for_token_with_workspace(
                                uri,
                                text,
                                &token,
                                &self.documents,
                            ))
                        })
                    })
                    .unwrap_or_else(|| Value::Array(vec![]));
                vec![lsp_response(message, result)]
            }
            "ail/references" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = self
                    .documents
                    .get(uri)
                    .map(|text| {
                        references_diagnostic_at_position(
                            uri,
                            text,
                            line,
                            character,
                            &self.documents,
                        )
                    })
                    .unwrap_or_else(missing_document_references_failure);
                vec![lsp_response(message, result)]
            }
            "exit" => vec![],
            _ if message.get("id").is_some() => vec![lsp_response(message, Value::Null)],
            _ => vec![],
        }
    }

    fn code_actions(&self, message: &Value) -> Value {
        let params = &message["params"];
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        if !self.documents.contains_key(uri) {
            return Value::Array(vec![disabled_code_action(
                "AIL: code action unavailable",
                "document is not open in this LSP session",
                code_action_failure(
                    "missing_document",
                    "AIL_CODE_ACTION_MISSING_DOCUMENT",
                    "document_state",
                    json!({
                        "documentState": "not_open",
                        "diagnosticCount": code_action_context_diagnostics(params).len(),
                    }),
                ),
            )]);
        }

        let mut actions = code_action_context_diagnostics(params)
            .into_iter()
            .flat_map(|diagnostic| code_actions_for_diagnostic(&diagnostic))
            .collect::<Vec<_>>();
        actions.sort_by(|left, right| {
            code_action_sort_key(left)
                .cmp(&code_action_sort_key(right))
                .then_with(|| left.to_string().cmp(&right.to_string()))
        });
        Value::Array(actions)
    }

    fn completion_prefix(&self, message: &Value) -> String {
        let params = &message["params"];
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
        let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
        self.documents
            .get(uri)
            .map(|text| completion_prefix_at_position(text, line, character))
            .unwrap_or_default()
    }
}

fn workspace_root_uri_from_initialize(message: &Value) -> Option<String> {
    let params = &message["params"];
    params["workspaceFolders"]
        .as_array()
        .and_then(|folders| folders.first())
        .and_then(|folder| folder["uri"].as_str())
        .or_else(|| params["rootUri"].as_str())
        .map(str::to_string)
}

fn lsp_response(request: &Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request.get("id").cloned().unwrap_or(Value::Null),
        "result": result,
    })
}

fn completion_prefix_at_position(text: &str, line: usize, character: usize) -> String {
    let Some(line_text) = text.lines().nth(line) else {
        return String::new();
    };
    let char_indices = line_text.char_indices().collect::<Vec<_>>();
    let byte_pos = char_indices
        .get(character)
        .map(|(idx, _)| *idx)
        .unwrap_or(line_text.len());
    let start = line_text[..byte_pos]
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_completion_prefix_char(*ch))
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    line_text[start..byte_pos].trim().to_string()
}

fn is_completion_prefix_char(ch: char) -> bool {
    is_acl_token_char(ch)
        || matches!(
            ch,
            '+' | '-' | '*' | '/' | '%' | '!' | '=' | '>' | '<' | '&' | '|' | '{'
        )
}

fn code_action_context_diagnostics(params: &Value) -> Vec<Value> {
    params["context"]["diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn code_actions_for_diagnostic(diagnostic: &Value) -> Vec<Value> {
    let Some(code) = diagnostic_code(diagnostic) else {
        return vec![disabled_code_action(
            "AIL: unsupported diagnostic",
            "diagnostic does not carry a supported AIL diagnostic code",
            code_action_failure(
                "unsupported_diagnostic_code",
                "AIL_CODE_ACTION_UNSUPPORTED_DIAGNOSTIC",
                "unsupported",
                diagnostic_descriptor(diagnostic),
            ),
        )];
    };

    if !supported_diagnostic_code(code) {
        return vec![disabled_code_action(
            "AIL: unsupported diagnostic",
            "diagnostic code is not supported by AIL code actions",
            code_action_failure(
                "unsupported_diagnostic_code",
                "AIL_CODE_ACTION_UNSUPPORTED_DIAGNOSTIC",
                "unsupported",
                diagnostic_descriptor(diagnostic),
            ),
        )];
    }

    let Some(repair) = diagnostic["data"]["ailRepair"].as_object() else {
        return vec![disabled_code_action(
            "AIL: no repair available",
            "diagnostic has no automated repair available",
            code_action_failure(
                "no_repair_available",
                "AIL_CODE_ACTION_NO_REPAIR_AVAILABLE",
                "unsupported",
                diagnostic_descriptor(diagnostic),
            ),
        )];
    };

    let direct_edit = repair.get("edit").filter(|edit| edit.is_object());
    let edit_choices = repair
        .get("edits")
        .and_then(|edits| edits.as_array())
        .cloned()
        .unwrap_or_default();
    let edit_count = usize::from(direct_edit.is_some()) + edit_choices.len();
    if edit_count != 1 {
        return vec![disabled_code_action(
            "AIL: ambiguous diagnostic repair",
            "diagnostic repair must resolve to exactly one workspace edit",
            code_action_failure(
                "ambiguous_repair_edit",
                "AIL_CODE_ACTION_AMBIGUOUS_REPAIR_EDIT",
                "ambiguous",
                json!({
                    "diagnostic": diagnostic_descriptor(diagnostic),
                    "repairEditCount": edit_count,
                    "hasDirectEdit": direct_edit.is_some(),
                }),
            ),
        )];
    }

    let edit = direct_edit
        .cloned()
        .or_else(|| edit_choices.into_iter().next())
        .unwrap_or_else(|| json!({}));
    let repair_code = repair
        .get("code")
        .and_then(|value| value.as_str())
        .filter(|code| is_stable_identifier(code))
        .unwrap_or("inline_edit");
    vec![json!({
        "title": "AIL: apply diagnostic repair",
        "kind": "quickfix",
        "isPreferred": true,
        "diagnostics": [diagnostic.clone()],
        "edit": edit,
        "data": {
            "code": "AIL_CODE_ACTION_REPAIR",
            "diagnosticCode": code,
            "repairCode": repair_code,
        }
    })]
}

fn diagnostic_code(diagnostic: &Value) -> Option<&str> {
    diagnostic["code"].as_str()
}

fn supported_diagnostic_code(code: &str) -> bool {
    matches!(
        code,
        LSP_DIAGNOSTIC_ACL_PARSER
            | LSP_DIAGNOSTIC_ACL_SCHEMA
            | LSP_DIAGNOSTIC_SOURCE_IMPORT
            | LSP_DIAGNOSTIC_SOURCE_PARSER
    )
}

fn disabled_code_action(title: &str, reason: &str, diagnostic: Value) -> Value {
    json!({
        "title": title,
        "kind": "quickfix",
        "disabled": { "reason": reason },
        "data": { "diagnostic": diagnostic }
    })
}

fn code_action_failure(reason: &str, code: &str, category: &str, descriptor: Value) -> Value {
    json!({
        "reason": reason,
        "code": code,
        "category": category,
        "severity": "error",
        "descriptor": descriptor,
    })
}

fn diagnostic_descriptor(diagnostic: &Value) -> Value {
    json!({
        "diagnosticCode": diagnostic["code"].as_str().filter(|code| supported_diagnostic_code(code)).unwrap_or("unsupported"),
        "diagnosticCodeLength": diagnostic["code"].as_str().map(str::len).unwrap_or(0),
        "source": diagnostic["source"].as_str().filter(|source| is_stable_identifier(source)).unwrap_or("unknown"),
        "sourceLength": diagnostic["source"].as_str().map(str::len).unwrap_or(0),
        "hasData": diagnostic.get("data").is_some(),
        "range": {
            "start": {
                "line": diagnostic["range"]["start"]["line"].as_u64().unwrap_or(0),
                "character": diagnostic["range"]["start"]["character"].as_u64().unwrap_or(0),
            },
            "end": {
                "line": diagnostic["range"]["end"]["line"].as_u64().unwrap_or(0),
                "character": diagnostic["range"]["end"]["character"].as_u64().unwrap_or(0),
            }
        }
    })
}

fn code_action_sort_key(action: &Value) -> (String, String, u64, u64) {
    (
        action["title"].as_str().unwrap_or_default().to_string(),
        action["data"]["diagnostic"]["code"]
            .as_str()
            .or_else(|| action["data"]["diagnosticCode"].as_str())
            .unwrap_or_default()
            .to_string(),
        action["diagnostics"][0]["range"]["start"]["line"]
            .as_u64()
            .unwrap_or(0),
        action["diagnostics"][0]["range"]["start"]["character"]
            .as_u64()
            .unwrap_or(0),
    )
}

fn is_stable_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}
