use super::source_helpers::{
    byte_index_to_lsp_character, is_acl_token_char, lsp_character_to_byte_index,
};

pub(super) const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "function",
    "variable",
    "keyword",
    "operator",
    "string",
    "number",
    "comment",
];

const SEMANTIC_NAMESPACE: u32 = 0;
const SEMANTIC_TYPE: u32 = 1;
const SEMANTIC_FUNCTION: u32 = 2;
const SEMANTIC_VARIABLE: u32 = 3;
const SEMANTIC_KEYWORD: u32 = 4;
const SEMANTIC_OPERATOR: u32 = 5;
const SEMANTIC_STRING: u32 = 6;
const SEMANTIC_NUMBER: u32 = 7;
const SEMANTIC_COMMENT: u32 = 8;

pub(super) fn token_at_position(text: &str, line: usize, character: usize) -> Option<String> {
    token_range_at_position(text, line, character).map(|range| range.token)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TokenRange {
    pub(super) token: String,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) start_character: usize,
    pub(super) end_character: usize,
}

pub(super) fn token_range_at_position(
    text: &str,
    line: usize,
    character: usize,
) -> Option<TokenRange> {
    let line_text = text.lines().nth(line)?;
    let byte_pos = lsp_character_to_byte_index(line_text, character);
    if let Some(operator) = source_operator_token_at_position(line_text, byte_pos) {
        let start = line_text
            .match_indices(operator)
            .find_map(|(start, _)| {
                let end = start + operator.len();
                (byte_pos >= start && byte_pos <= end).then_some(start)
            })
            .unwrap_or(byte_pos);
        let end = start + operator.len();
        return Some(TokenRange {
            token: operator.to_string(),
            start,
            end,
            start_character: byte_index_to_lsp_character(line_text, start),
            end_character: byte_index_to_lsp_character(line_text, end),
        });
    }
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
    (start < end).then(|| TokenRange {
        token: line_text[start..end].to_string(),
        start,
        end,
        start_character: byte_index_to_lsp_character(line_text, start),
        end_character: byte_index_to_lsp_character(line_text, end),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SemanticToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
}

impl SemanticToken {
    fn end(&self) -> u32 {
        self.start + self.length
    }
}

pub(super) fn semantic_token_data_for_source(text: &str) -> Vec<u32> {
    encode_semantic_tokens(canonical_semantic_tokens(text))
}

fn canonical_semantic_tokens(text: &str) -> Vec<SemanticToken> {
    let mut tokens = text
        .lines()
        .enumerate()
        .flat_map(|(line_idx, line)| semantic_tokens_in_line(line_idx as u32, line))
        .filter(|token| token.length > 0)
        .collect::<Vec<_>>();
    tokens.sort();

    let mut stable = Vec::<SemanticToken>::new();
    for token in tokens {
        if let Some(previous) = stable.last() {
            if previous.line == token.line
                && previous.start == token.start
                && previous.length == token.length
            {
                continue;
            }
            if previous.line == token.line && token.start < previous.end() {
                continue;
            }
        }
        stable.push(token);
    }
    stable
}

fn encode_semantic_tokens(tokens: Vec<SemanticToken>) -> Vec<u32> {
    let mut data = Vec::with_capacity(tokens.len() * 5);
    let mut previous_line = 0;
    let mut previous_start = 0;

    for token in tokens {
        let delta_line = token.line - previous_line;
        let delta_start = if delta_line == 0 {
            token.start - previous_start
        } else {
            token.start
        };
        data.extend([delta_line, delta_start, token.length, token.token_type, 0]);
        previous_line = token.line;
        previous_start = token.start;
    }

    data
}

fn semantic_tokens_in_line(line_idx: u32, line: &str) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut byte_idx = 0;
    let mut previous_identifier: Option<&str> = None;

    while byte_idx < line.len() {
        let rest = &line[byte_idx..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if ch.is_whitespace() {
            byte_idx += ch.len_utf8();
            continue;
        }

        if rest.starts_with("//") {
            tokens.push(semantic_token(
                line_idx,
                byte_idx,
                line.len(),
                SEMANTIC_COMMENT,
            ));
            break;
        }

        if ch == '"' {
            let end = string_literal_end(line, byte_idx);
            tokens.push(semantic_token(line_idx, byte_idx, end, SEMANTIC_STRING));
            byte_idx = end;
            previous_identifier = None;
            continue;
        }

        if ch.is_ascii_digit() {
            let end = scan_while(line, byte_idx, |ch| ch.is_ascii_digit() || ch == '_');
            tokens.push(semantic_token(line_idx, byte_idx, end, SEMANTIC_NUMBER));
            byte_idx = end;
            previous_identifier = None;
            continue;
        }

        if let Some(operator) = source_operator_at_start(rest) {
            let end = byte_idx + operator.len();
            tokens.push(semantic_token(line_idx, byte_idx, end, SEMANTIC_OPERATOR));
            byte_idx = end;
            previous_identifier = None;
            continue;
        }

        if is_acl_token_char(ch) {
            let end = scan_while(line, byte_idx, is_acl_token_char);
            let text = &line[byte_idx..end];
            let token_type = semantic_type_for_identifier(
                text,
                previous_identifier,
                next_non_whitespace_char(line, end),
            );
            tokens.push(semantic_token(line_idx, byte_idx, end, token_type));
            byte_idx = end;
            previous_identifier = Some(text);
            continue;
        }

        byte_idx += ch.len_utf8();
        previous_identifier = None;
    }

    tokens
}

fn semantic_token(line: u32, start: usize, end: usize, token_type: u32) -> SemanticToken {
    SemanticToken {
        line,
        start: start as u32,
        length: (end - start) as u32,
        token_type,
    }
}

fn semantic_type_for_identifier(
    text: &str,
    previous_identifier: Option<&str>,
    next_char: Option<char>,
) -> u32 {
    if is_source_keyword(text) {
        SEMANTIC_KEYWORD
    } else if previous_identifier == Some("module") {
        SEMANTIC_NAMESPACE
    } else if is_source_type_name(text) {
        SEMANTIC_TYPE
    } else if previous_identifier == Some("fn")
        || previous_identifier == Some("test")
        || next_char == Some('(')
    {
        SEMANTIC_FUNCTION
    } else {
        SEMANTIC_VARIABLE
    }
}

fn is_source_keyword(text: &str) -> bool {
    matches!(
        text,
        "module"
            | "use"
            | "fn"
            | "const"
            | "test"
            | "let"
            | "return"
            | "if"
            | "else"
            | "match"
            | "grant"
            | "capability"
            | "true"
            | "false"
    )
}

fn is_source_type_name(text: &str) -> bool {
    matches!(text, "Int" | "Text" | "Bool" | "Unit")
        || text
            .chars()
            .next()
            .map_or(false, |ch| ch.is_ascii_uppercase())
}

fn string_literal_end(line: &str, start: usize) -> usize {
    let mut escaped = false;
    for (offset, ch) in line[start + 1..].char_indices() {
        let idx = start + 1 + offset;
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return idx + ch.len_utf8();
        }
    }
    line.len()
}

fn scan_while(line: &str, start: usize, predicate: impl Fn(char) -> bool) -> usize {
    line[start..]
        .char_indices()
        .find(|(_, ch)| !predicate(*ch))
        .map(|(idx, _)| start + idx)
        .unwrap_or(line.len())
}

fn next_non_whitespace_char(line: &str, start: usize) -> Option<char> {
    line[start..].chars().find(|ch| !ch.is_whitespace())
}

fn source_operator_token_at_position(line: &str, byte_pos: usize) -> Option<&'static str> {
    SOURCE_OPERATORS.iter().copied().find(|operator| {
        line.match_indices(operator).any(|(start, _)| {
            let end = start + operator.len();
            byte_pos >= start && byte_pos <= end
        })
    })
}

fn source_operator_at_start(text: &str) -> Option<&'static str> {
    SOURCE_OPERATORS
        .iter()
        .copied()
        .find(|operator| text.starts_with(operator))
}

const SOURCE_OPERATORS: &[&str] = &[
    "...", "->", "++", "&&", "||", "==", "!=", ">=", "<=", "+", "-", "*", "/", "%", "!", ">", "<",
    "=",
];
