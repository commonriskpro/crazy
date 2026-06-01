use crate::common::ail;

#[test]
fn lsp_semantic_tokens_are_stable_ordered_and_typed_for_source_documents() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    let source_text = "module math\n\
fn add_pair(x: Int, y: Int) -> Int = x + y // sum\n\
const greeting: Text = \"hello // world\"\n\
fn clamp(x: Int) -> Int = if x == 0 {\n\
    return 1\n\
} else {\n\
    return x\n\
}\n\
test smoke = add_pair(1, 2) == 3\n";
    source
        .write_str(source_text)
        .expect("source fixture must be written");
    let uri = format!("file://{}", source.path().display());

    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    })
    .to_string();
    let open = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "ail",
                "version": 1,
                "text": source_text,
            }
        }
    })
    .to_string();
    let semantic_tokens = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/semanticTokens/full",
        "params": {
            "textDocument": { "uri": format!("file://{}", source.path().display()) }
        }
    })
    .to_string();
    let input = lsp_input(&[initialize, open, semantic_tokens]);

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);

    let initialize_response = messages
        .iter()
        .find(|message| message["id"] == 1)
        .expect("initialize response must be emitted");
    let token_types = initialize_response["result"]["capabilities"]["semanticTokensProvider"]
        ["legend"]["tokenTypes"]
        .as_array()
        .expect("semantic token legend must expose token types")
        .iter()
        .map(|value| value.as_str().expect("legend token type must be string"))
        .collect::<Vec<_>>();
    assert_eq!(
        token_types,
        [
            "namespace",
            "type",
            "function",
            "variable",
            "keyword",
            "operator",
            "string",
            "number",
            "comment"
        ],
        "legend order is part of the semantic-token compatibility contract"
    );

    let token_response = messages
        .iter()
        .find(|message| message["id"] == 2)
        .expect("semantic token response must be emitted");
    let encoded = token_response["result"]["data"]
        .as_array()
        .expect("semantic token result data must be an array")
        .iter()
        .map(|value| value.as_u64().expect("semantic token data must be numeric"))
        .collect::<Vec<_>>();
    assert_eq!(encoded.len() % 5, 0, "semantic tokens use LSP 5-tuples");

    let decoded = decode_semantic_tokens(&encoded);
    assert_ordered_and_non_overlapping(&decoded);
    let labeled_tokens = labeled_tokens(source_text, &decoded, &token_types);

    for expected in [
        ("module", "keyword"),
        ("math", "namespace"),
        ("fn", "keyword"),
        ("add_pair", "function"),
        ("x", "variable"),
        ("Int", "type"),
        ("+", "operator"),
        ("\"hello // world\"", "string"),
        ("// sum", "comment"),
        ("if", "keyword"),
        ("return", "keyword"),
        ("else", "keyword"),
        ("test", "keyword"),
        ("==", "operator"),
        ("3", "number"),
    ] {
        assert!(
            labeled_tokens.contains(&expected),
            "semantic token {expected:?} must be present in {labeled_tokens:?}"
        );
    }
}

#[test]
fn lsp_semantic_tokens_use_utf16_offsets() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    let source_text = "fn main() -> Text = \"🔥\" ++ \"x\"\n";
    source
        .write_str(source_text)
        .expect("source fixture must be written");
    let uri = format!("file://{}", source.path().display());

    let open = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "ail",
                "version": 1,
                "text": source_text,
            }
        }
    })
    .to_string();
    let semantic_tokens = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/semanticTokens/full",
        "params": {
            "textDocument": { "uri": format!("file://{}", source.path().display()) }
        }
    })
    .to_string();
    let input = lsp_input(&[open, semantic_tokens]);

    let output = ail()
        .args(["lsp", "--stdio"])
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .clone();
    let messages = lsp_json_messages(&output.stdout);
    let token_response = messages
        .iter()
        .find(|message| message["id"] == 2)
        .expect("semantic token response must be emitted");
    let encoded = token_response["result"]["data"]
        .as_array()
        .expect("semantic token result data must be an array")
        .iter()
        .map(|value| value.as_u64().expect("semantic token data must be numeric"))
        .collect::<Vec<_>>();
    let decoded = decode_semantic_tokens(&encoded);

    assert!(
        decoded.contains(&(0, 20, 4, 6)),
        "emoji string token length must be UTF-16 units, not bytes: {decoded:?}"
    );
    assert!(
        decoded.contains(&(0, 25, 2, 5)),
        "operator after emoji must start at UTF-16 offset 25, not byte offset 27: {decoded:?}"
    );
}

fn lsp_input(messages: &[String]) -> String {
    messages
        .iter()
        .map(|message| format!("Content-Length: {}\r\n\r\n{}", message.len(), message))
        .collect::<Vec<_>>()
        .join("")
}

fn decode_semantic_tokens(data: &[u64]) -> Vec<(u64, u64, u64, u64)> {
    let mut out = Vec::new();
    let mut line = 0;
    let mut start = 0;
    for chunk in data.chunks_exact(5) {
        line += chunk[0];
        start = if chunk[0] == 0 {
            start + chunk[1]
        } else {
            chunk[1]
        };
        out.push((line, start, chunk[2], chunk[3]));
    }
    out
}

fn assert_ordered_and_non_overlapping(tokens: &[(u64, u64, u64, u64)]) {
    for window in tokens.windows(2) {
        let (left_line, left_start, left_len, _) = window[0];
        let (right_line, right_start, _, _) = window[1];
        assert!(
            right_line > left_line || right_start >= left_start + left_len,
            "semantic tokens must be sorted and non-overlapping: {tokens:?}"
        );
    }
}

fn labeled_tokens<'a>(
    source_text: &'a str,
    decoded: &[(u64, u64, u64, u64)],
    token_types: &'a [&'a str],
) -> Vec<(&'a str, &'a str)> {
    let lines = source_text.lines().collect::<Vec<_>>();
    decoded
        .iter()
        .map(|(line, start, len, token_type)| {
            let line_text = lines
                .get(*line as usize)
                .expect("semantic token line must exist");
            let start = *start as usize;
            let end = start + *len as usize;
            let text = line_text
                .get(start..end)
                .expect("semantic token range must slice source line");
            let kind = token_types
                .get(*token_type as usize)
                .expect("semantic token type index must exist");
            (text, *kind)
        })
        .collect()
}

fn lsp_json_messages(stdout: &[u8]) -> Vec<serde_json::Value> {
    let text = std::str::from_utf8(stdout).expect("LSP stdout must be UTF-8");
    let mut messages = Vec::new();
    let mut rest = text;
    while let Some(header_start) = rest.find("Content-Length: ") {
        rest = &rest[header_start + "Content-Length: ".len()..];
        let Some((len, after_len)) = rest.split_once("\r\n\r\n") else {
            break;
        };
        let len = len
            .trim()
            .parse::<usize>()
            .expect("Content-Length must be numeric");
        let body = &after_len[..len];
        messages.push(serde_json::from_str(body).expect("LSP body must be JSON"));
        rest = &after_len[len..];
    }
    messages
}
