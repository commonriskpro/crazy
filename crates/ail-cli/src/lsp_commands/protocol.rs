use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};

use serde_json::{Value, json};

use crate::error::CliError;

use super::definition::definition_for_token_with_workspace;
use super::diagnostics::diagnostics_for_document;
use super::references::references_for_token_with_workspace;
use super::symbols::{completion_items, hover_for_token};
use super::tokens::token_at_position;

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
            "initialize" => vec![lsp_response(
                message,
                json!({
                    "capabilities": {
                        "textDocumentSync": 1,
                        "diagnosticProvider": {
                            "interFileDependencies": true,
                            "workspaceDiagnostics": false
                        },
                        "completionProvider": {
                            "triggerCharacters": [" ", "_"]
                        },
                        "hoverProvider": true,
                        "definitionProvider": true,
                        "referencesProvider": true
                    },
                    "serverInfo": {
                        "name": "ail-lsp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )],
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
                    "items": completion_items("")
                }),
            )],
            "textDocument/hover" => {
                let params = &message["params"];
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let token = self
                    .documents
                    .get(uri)
                    .and_then(|text| token_at_position(text, line, character))
                    .unwrap_or_default();
                vec![lsp_response(message, hover_for_token(&token))]
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
            "exit" => vec![],
            _ if message.get("id").is_some() => vec![lsp_response(message, Value::Null)],
            _ => vec![],
        }
    }
}

fn lsp_response(request: &Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request.get("id").cloned().unwrap_or(Value::Null),
        "result": result,
    })
}
