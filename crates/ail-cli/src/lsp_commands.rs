// ── ail-cli::lsp_commands ────────────────────────────────────────────────
//
// Minimal Language Server Protocol surface for ACL and `.ail` source documents.
//
// This is validation-stage editor support: enough for an editor/client to
// initialize the server and receive parser/op-schema diagnostics for ACL text
// plus parser diagnostics for the validation-stage `.ail` source surface.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

use ail_change::op_schema::validate_op_schemas;
use ail_change::parser::parse_changeset;
use serde_json::{Value, json};

use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::source_commands::{load_source_program_from_text, parse_ail_source};

// ── Command handlers ─────────────────────────────────────────────────────

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
    let human_msg = if diagnostics.is_empty() {
        format!("LSP diagnostics: ok\nfile: {}", path.display())
    } else {
        format!(
            "LSP diagnostics: {failed} error(s)\nfile: {}\n{}",
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
            "diagnostics": diagnostics,
            "diagnostic_count": diagnostic_count,
            "error_count": failed,
            "language": language_for_uri(&uri),
        }),
    );
    Ok(())
}

fn cmd_lsp_complete(mode: OutputMode, prefix: &str) -> Result<(), CliError> {
    let items = completion_items(prefix);
    print_response(
        mode,
        &format!("LSP completions: {} item(s)", items.len()),
        json!({
            "prefix": prefix,
            "items": items,
        }),
    );
    Ok(())
}

fn cmd_lsp_hover(mode: OutputMode, token: &str) -> Result<(), CliError> {
    let hover = hover_for_token(token);
    print_response(
        mode,
        if hover.is_null() {
            "LSP hover: no information"
        } else {
            "LSP hover: found"
        },
        json!({
            "token": token,
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
    print_response(
        mode,
        if definition.is_null() {
            "LSP definition: not found"
        } else {
            "LSP definition: found"
        },
        json!({
            "token": token,
            "uri": uri,
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
    print_response(
        mode,
        &format!("LSP references: {} location(s)", references.len()),
        json!({
            "token": token,
            "uri": uri,
            "references": references,
            "reference_count": references.len(),
        }),
    );
    Ok(())
}

// ── Stdio JSON-RPC loop ──────────────────────────────────────────────────

fn run_stdio_lsp() -> Result<(), CliError> {
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
                        token_at_position(text, line, character)
                            .map(|token| definition_for_token(uri, text, &token))
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
                        token_at_position(text, line, character)
                            .map(|token| Value::Array(references_for_token(uri, text, &token)))
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

// ── Diagnostics ──────────────────────────────────────────────────────────

fn diagnostics_for_document(uri: &str, text: &str) -> Vec<Value> {
    if is_ail_source_uri(uri) {
        if let Some(path) = file_path_from_uri(uri) {
            return diagnostics_for_ail_source_document_path(&path, text);
        }
        return diagnostics_for_ail_source_text(uri, text);
    }
    diagnostics_for_acl_text(uri, text)
}

fn diagnostics_for_acl_text(_uri: &str, text: &str) -> Vec<Value> {
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

fn diagnostics_for_ail_source_path(path: &std::path::Path, text: &str) -> Vec<Value> {
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

fn file_path_from_uri(uri: &str) -> Option<PathBuf> {
    uri.strip_prefix("file://").map(PathBuf::from)
}

fn is_ail_source_uri(uri: &str) -> bool {
    uri.trim_end().ends_with(".ail")
}

fn language_for_uri(uri: &str) -> &'static str {
    if is_ail_source_uri(uri) {
        "ail-source"
    } else {
        "acl"
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

// ── Completion / hover metadata ──────────────────────────────────────────

struct AclSymbol {
    label: &'static str,
    detail: &'static str,
    documentation: &'static str,
    insert_text: &'static str,
}

const ACL_SYMBOLS: &[AclSymbol] = &[
    AclSymbol {
        label: "change",
        detail: "ACL header",
        documentation: "Starts an ACL ChangeSet document.",
        insert_text: "change ${1:name}",
    },
    AclSymbol {
        label: "author",
        detail: "ACL metadata",
        documentation: "Declares the human or agent author of the ChangeSet.",
        insert_text: "author ${1:name}",
    },
    AclSymbol {
        label: "description",
        detail: "ACL metadata",
        documentation: "Describes the purpose of the ChangeSet.",
        insert_text: "description ${1:text}",
    },
    AclSymbol {
        label: "base",
        detail: "ACL snapshot guard",
        documentation: "Declares the expected base snapshot id.",
        insert_text: "base ${1:0}",
    },
    AclSymbol {
        label: "op create_function",
        detail: "Create a function node",
        documentation: "Creates a semantic function node. Requires id; return/body are optional but useful for executable code.",
        insert_text: "op create_function id=fn.${1:name} return=${2:Int} body=${3:add(20, 22)}",
    },
    AclSymbol {
        label: "op create_test",
        detail: "Create a test node",
        documentation: "Creates a semantic test node that `ail test` can discover and run.",
        insert_text: "op create_test id=test.${1:name} body=${2:eq(add(20, 22), 42)}",
    },
    AclSymbol {
        label: "op create_capability",
        detail: "Create a capability node",
        documentation: "Declares an external capability such as log.write.",
        insert_text: "op create_capability id=${1:log.write}",
    },
    AclSymbol {
        label: "op grant",
        detail: "Grant a capability requirement",
        documentation: "Adds a capability requirement to a target node.",
        insert_text: "op grant target=${1:fn.main} capability=${2:log.write}",
    },
    AclSymbol {
        label: "op set_body",
        detail: "Set function body",
        documentation: "Updates the body expression for an existing graph node.",
        insert_text: "op set_body target=${1:fn.main} body=${2:add(20, 22)}",
    },
    AclSymbol {
        label: "op add_param",
        detail: "Add function parameter",
        documentation: "Adds a typed parameter to a function node.",
        insert_text: "op add_param target=${1:fn.main} name=${2:x} type=${3:Int}",
    },
    AclSymbol {
        label: "end",
        detail: "ACL terminator",
        documentation: "Ends an ACL ChangeSet document.",
        insert_text: "end",
    },
];

const AIL_SOURCE_SYMBOLS: &[AclSymbol] = &[
    AclSymbol {
        label: "module",
        detail: "AIL source module",
        documentation: "Declares the source module namespace for functions, tests, and grants.",
        insert_text: "module ${1:name}",
    },
    AclSymbol {
        label: "use",
        detail: "AIL source import",
        documentation: "Imports another local .ail source file with an explicit relative path.",
        insert_text: "use \"./${1:file}.ail\"",
    },
    AclSymbol {
        label: "capability",
        detail: "AIL source capability",
        documentation: "Declares an external capability such as log.write before granting it to source items.",
        insert_text: "capability ${1:log.write}",
    },
    AclSymbol {
        label: "fn",
        detail: "AIL source function",
        documentation: "Declares a typed AIL source function that lowers into the semantic graph.",
        insert_text: "fn ${1:name}(${2:x}: ${3:Int}) -> ${4:Int} = ${5:add(x, 1)}",
    },
    AclSymbol {
        label: "test",
        detail: "AIL source test",
        documentation: "Declares an executable source test that `ail test --file` can discover and run.",
        insert_text: "test ${1:name} = ${2:eq(add(20, 22), 42)}",
    },
    AclSymbol {
        label: "grant",
        detail: "AIL source capability grant",
        documentation: "Grants a declared capability to a source function or test before effect calls are allowed.",
        insert_text: "grant ${1:main} ${2:log.write}",
    },
    AclSymbol {
        label: "let",
        detail: "AIL source local binding",
        documentation: "Introduces a simple local binding inside a block-bodied source function.",
        insert_text: "let ${1:name} = ${2:value}",
    },
    AclSymbol {
        label: "if",
        detail: "AIL source conditional",
        documentation: "Evaluates a typed conditional expression with explicit then and else branches.",
        insert_text: "if ${1:condition} { ${2:then_expr} } else { ${3:else_expr} }",
    },
    AclSymbol {
        label: "add",
        detail: "AIL source Int builtin",
        documentation: "Adds two Int values and returns an Int.",
        insert_text: "add(${1:left}, ${2:right})",
    },
    AclSymbol {
        label: "sub",
        detail: "AIL source Int builtin",
        documentation: "Subtracts the second Int from the first and returns an Int.",
        insert_text: "sub(${1:left}, ${2:right})",
    },
    AclSymbol {
        label: "mul",
        detail: "AIL source Int builtin",
        documentation: "Multiplies two Int values and returns an Int.",
        insert_text: "mul(${1:left}, ${2:right})",
    },
    AclSymbol {
        label: "div",
        detail: "AIL source Int builtin",
        documentation: "Divides the first Int by the second and returns an Int.",
        insert_text: "div(${1:left}, ${2:right})",
    },
    AclSymbol {
        label: "eq",
        detail: "AIL source comparison builtin",
        documentation: "Compares two values of the same inferred type and returns Bool.",
        insert_text: "eq(${1:left}, ${2:right})",
    },
    AclSymbol {
        label: "gt",
        detail: "AIL source Int comparison builtin",
        documentation: "Returns Bool after checking whether the first Int is greater than the second.",
        insert_text: "gt(${1:left}, ${2:right})",
    },
    AclSymbol {
        label: "concat",
        detail: "AIL source Text builtin",
        documentation: "Concatenates two Text values and returns Text.",
        insert_text: "concat(${1:left}, ${2:right})",
    },
    AclSymbol {
        label: "len",
        detail: "AIL source Text builtin",
        documentation: "Returns the Int length of a Text value.",
        insert_text: "len(${1:text})",
    },
    AclSymbol {
        label: "effect_call",
        detail: "AIL source capability call",
        documentation: "Calls an external capability operation after the source item has an explicit grant.",
        insert_text: "effect_call(${1:log.write}, ${2:write}, ${3:\"message\"})",
    },
];

fn completion_items(prefix: &str) -> Vec<Value> {
    let prefix = prefix.trim().to_ascii_lowercase();
    ACL_SYMBOLS
        .iter()
        .chain(AIL_SOURCE_SYMBOLS.iter())
        .filter(|symbol| {
            prefix.is_empty() || symbol.label.to_ascii_lowercase().contains(prefix.as_str())
        })
        .map(|symbol| {
            json!({
                "label": symbol.label,
                "kind": 14,
                "detail": symbol.detail,
                "documentation": {
                    "kind": "markdown",
                    "value": symbol.documentation
                },
                "insertText": symbol.insert_text,
                "insertTextFormat": 2
            })
        })
        .collect()
}

fn hover_for_token(token: &str) -> Value {
    let normalized = token.trim();
    let symbol = ACL_SYMBOLS
        .iter()
        .chain(AIL_SOURCE_SYMBOLS.iter())
        .find(|symbol| {
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

fn token_at_position(text: &str, line: usize, character: usize) -> Option<String> {
    let line_text = text.lines().nth(line)?;
    let char_indices: Vec<(usize, char)> = line_text.char_indices().collect();
    let byte_pos = char_indices
        .get(character)
        .map(|(idx, _)| *idx)
        .unwrap_or(line_text.len());
    let start = line_text[..byte_pos]
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_acl_token_char(*ch))
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    let end = line_text[byte_pos..]
        .char_indices()
        .find(|(_, ch)| !is_acl_token_char(*ch))
        .map(|(idx, _)| byte_pos + idx)
        .unwrap_or(line_text.len());
    (start < end).then(|| line_text[start..end].to_string())
}

fn definition_for_token(uri: &str, text: &str, token: &str) -> Value {
    let token = token.trim();
    if token.is_empty() {
        return Value::Null;
    }

    if is_ail_source_uri(uri)
        && let Some(definition) = definition_for_ail_source_token(uri, text, token)
    {
        return definition;
    }

    for (line_idx, line) in text.lines().enumerate() {
        let needle = format!("id={token}");
        if let Some(start) = line.find(&needle).map(|idx| idx + "id=".len()) {
            return json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + token.len() }
                }
            });
        }
    }
    Value::Null
}

fn definition_for_ail_source_token(uri: &str, text: &str, token: &str) -> Option<Value> {
    let token = token.strip_prefix("fn.").unwrap_or(token);
    if let Some(definition) = source_function_definition_in_text(uri, text, token) {
        return Some(definition);
    }

    let root_path = file_path_from_uri(uri)?;
    let canonical_root = std::fs::canonicalize(&root_path).ok()?;
    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    definition_for_ail_source_imports(&canonical_root, text, token, &mut visited)
}

fn definition_for_ail_source_imports(
    source_path: &std::path::Path,
    text: &str,
    token: &str,
    visited: &mut BTreeSet<PathBuf>,
) -> Option<Value> {
    for import in source_imports_from_text(text) {
        let path = resolve_lsp_source_import(source_path, &import);
        let Ok(canonical) = std::fs::canonicalize(&path) else {
            continue;
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let Ok(imported_text) = std::fs::read_to_string(&canonical) else {
            continue;
        };
        let imported_uri = format!("file://{}", canonical.display());
        if let Some(definition) =
            source_function_definition_in_text(&imported_uri, &imported_text, token)
        {
            return Some(definition);
        }
        if let Some(definition) =
            definition_for_ail_source_imports(&canonical, &imported_text, token, visited)
        {
            return Some(definition);
        }
    }
    None
}

fn source_function_definition_in_text(uri: &str, text: &str, token: &str) -> Option<Value> {
    let module = source_module_from_text(text);
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("fn ") else {
            continue;
        };
        let Some(name_end) = rest.find('(') else {
            continue;
        };
        let name = rest[..name_end].trim();
        if source_function_name_matches_token(name, module.as_deref(), token) {
            let leading = line.len() - trimmed.len();
            let start = leading + "fn ".len();
            return Some(json!({
                "uri": uri,
                "range": {
                    "start": { "line": line_idx, "character": start },
                    "end": { "line": line_idx, "character": start + name.len() }
                }
            }));
        }
    }
    None
}

fn source_function_name_matches_token(name: &str, module: Option<&str>, token: &str) -> bool {
    if name == token || name.strip_prefix("fn.") == Some(token) {
        return true;
    }
    let Some(module) = module else {
        return false;
    };
    let bare_name = name.strip_prefix("fn.").unwrap_or(name);
    token == format!("{module}.{bare_name}")
}

fn source_module_from_text(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("module ")
            .map(str::trim)
            .filter(|module| !module.is_empty())
            .map(ToString::to_string)
    })
}

fn source_imports_from_text(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("use ")?;
            rest.trim()
                .strip_prefix('"')
                .and_then(|value| value.split_once('"').map(|(import, _)| import.to_string()))
        })
        .collect()
}

fn resolve_lsp_source_import(source_path: &std::path::Path, import: &str) -> PathBuf {
    let import_path = std::path::Path::new(import);
    if import_path.is_absolute() {
        import_path.to_path_buf()
    } else {
        source_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(import_path)
    }
}

fn references_for_token(uri: &str, text: &str, token: &str) -> Vec<Value> {
    let token = token.trim();
    if token.is_empty() {
        return vec![];
    }

    if is_ail_source_uri(uri) {
        return references_for_ail_source_token(uri, text, token);
    }

    references_in_text(uri, text, token)
}

fn references_for_ail_source_token(uri: &str, text: &str, token: &str) -> Vec<Value> {
    let token = token.strip_prefix("fn.").unwrap_or(token);
    let mut refs = source_references_in_text(uri, text, token);
    let Some(root_path) = file_path_from_uri(uri) else {
        return refs;
    };
    let Ok(canonical_root) = std::fs::canonicalize(&root_path) else {
        return refs;
    };
    let mut visited = BTreeSet::new();
    visited.insert(canonical_root.clone());
    collect_ail_source_import_references(&canonical_root, text, token, &mut visited, &mut refs);
    refs
}

fn collect_ail_source_import_references(
    source_path: &std::path::Path,
    text: &str,
    token: &str,
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
        let Ok(imported_text) = std::fs::read_to_string(&canonical) else {
            continue;
        };
        let imported_uri = format!("file://{}", canonical.display());
        refs.extend(source_references_in_text(
            &imported_uri,
            &imported_text,
            token,
        ));
        collect_ail_source_import_references(&canonical, &imported_text, token, visited, refs);
    }
}

fn source_references_in_text(uri: &str, text: &str, token: &str) -> Vec<Value> {
    source_reference_tokens_for_text(text, token)
        .into_iter()
        .flat_map(|needle| references_in_text(uri, text, &needle))
        .collect()
}

fn source_reference_tokens_for_text(text: &str, token: &str) -> Vec<String> {
    let mut tokens = vec![token.to_string()];
    if let Some((module, local)) = token.split_once('.')
        && source_module_from_text(text).as_deref() == Some(module)
    {
        tokens.push(local.to_string());
    }
    tokens
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
        let before = line[..start].chars().next_back();
        let after = line[end..].chars().next();
        let boundary_before = before.map_or(true, |ch| !is_acl_token_char(ch));
        let boundary_after = after.map_or(true, |ch| !is_acl_token_char(ch));
        if boundary_before && boundary_after {
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

fn is_acl_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')
}
