mod acl;
mod source_builtins;
mod source_syntax;

use serde_json::{Value, json};

use acl::ACL_SYMBOLS;
use source_builtins::AIL_SOURCE_BUILTIN_SYMBOLS;
use source_syntax::AIL_SOURCE_SYNTAX_SYMBOLS;

pub(super) struct AclSymbol {
    pub(super) label: &'static str,
    pub(super) detail: &'static str,
    pub(super) documentation: &'static str,
    pub(super) insert_text: &'static str,
}

fn all_symbols() -> impl Iterator<Item = &'static AclSymbol> {
    ACL_SYMBOLS
        .iter()
        .chain(AIL_SOURCE_SYNTAX_SYMBOLS.iter())
        .chain(AIL_SOURCE_BUILTIN_SYMBOLS.iter())
}

pub(super) fn completion_items(prefix: &str) -> Vec<Value> {
    let prefix = prefix.trim().to_ascii_lowercase();
    all_symbols()
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

pub(super) fn hover_for_token(token: &str) -> Value {
    let normalized = token.trim();
    let symbol = all_symbols().find(|symbol| {
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
