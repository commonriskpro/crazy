// ── ail-stdlib::json ──────────────────────────────────────────────────────
//
// JSON parse/stringify for the AIL `std.json` module.
//
// # Rules (from docs/stdlib.md)
//
// - no universal auto-serialization
// - decoders return Result
// - encoders declare exported fields
//
// This module provides a minimal JSON value type and hand-written
// parse/stringify, keeping ail-stdlib dependency-free from serde_json.

use std::collections::BTreeMap;

// ── Json ──────────────────────────────────────────────────────────────────

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

/// Error produced during JSON parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError(pub String);

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "json error: {}", self.0)
    }
}
impl std::error::Error for JsonError {}

// ── stringify ─────────────────────────────────────────────────────────────

/// Serialize a `Json` value to a compact JSON string.
pub fn stringify(v: &Json) -> String {
    match v {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Json::Str(s) => {
            let escaped = s
                .chars()
                .flat_map(|c| match c {
                    '"' => vec!['\\', '"'],
                    '\\' => vec!['\\', '\\'],
                    '\n' => vec!['\\', 'n'],
                    '\r' => vec!['\\', 'r'],
                    '\t' => vec!['\\', 't'],
                    other => vec![other],
                })
                .collect::<String>();
            format!("\"{escaped}\"")
        }
        Json::Array(arr) => {
            let items: Vec<String> = arr.iter().map(stringify).collect();
            format!("[{}]", items.join(","))
        }
        Json::Object(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}:{}", stringify(&Json::Str(k.clone())), stringify(v)))
                .collect();
            format!("{{{}}}", items.join(","))
        }
    }
}

// ── parse ─────────────────────────────────────────────────────────────────

/// Parse a JSON string into a `Json` value.
pub fn parse(s: &str) -> Result<Json, JsonError> {
    let s = s.trim();
    let (val, rest) = parse_value(s)?;
    if !rest.trim().is_empty() {
        return Err(JsonError(format!("unexpected trailing input: {rest}")));
    }
    Ok(val)
}

fn parse_value(s: &str) -> Result<(Json, &str), JsonError> {
    let s = s.trim_start();
    if s.is_empty() {
        return Err(JsonError("unexpected end of input".into()));
    }
    match s.as_bytes()[0] {
        b'n' => s
            .strip_prefix("null")
            .map(|rest| (Json::Null, rest))
            .ok_or_else(|| JsonError("expected 'null'".into())),
        b't' => s
            .strip_prefix("true")
            .map(|rest| (Json::Bool(true), rest))
            .ok_or_else(|| JsonError("expected 'true'".into())),
        b'f' => s
            .strip_prefix("false")
            .map(|rest| (Json::Bool(false), rest))
            .ok_or_else(|| JsonError("expected 'false'".into())),
        b'"' => parse_string(&s[1..]).map(|(st, rest)| (Json::Str(st), rest)),
        b'[' => parse_array(&s[1..]),
        b'{' => parse_object(&s[1..]),
        b'-' | b'0'..=b'9' => parse_number(s),
        c => Err(JsonError(format!("unexpected character: {}", c as char))),
    }
}

fn parse_string(s: &str) -> Result<(String, &str), JsonError> {
    let mut result = String::new();
    let mut chars = s.char_indices();
    loop {
        match chars.next() {
            None => return Err(JsonError("unterminated string".into())),
            Some((_, '"')) => {
                return Ok((result, chars.as_str()));
            }
            Some((_, '\\')) => match chars.next() {
                Some((_, '"')) => result.push('"'),
                Some((_, '\\')) => result.push('\\'),
                Some((_, '/')) => result.push('/'),
                Some((_, 'n')) => result.push('\n'),
                Some((_, 'r')) => result.push('\r'),
                Some((_, 't')) => result.push('\t'),
                Some((_, 'b')) => result.push('\x08'),
                Some((_, 'f')) => result.push('\x0C'),
                Some((_, c)) => return Err(JsonError(format!("unknown escape: \\{c}"))),
                None => return Err(JsonError("unterminated escape".into())),
            },
            Some((_, c)) => result.push(c),
        }
    }
}

fn parse_array(s: &str) -> Result<(Json, &str), JsonError> {
    let mut items = Vec::new();
    let mut s = s.trim_start();
    if let Some(rest) = s.strip_prefix(']') {
        return Ok((Json::Array(items), rest));
    }
    loop {
        let (val, rest) = parse_value(s)?;
        items.push(val);
        s = rest.trim_start();
        if let Some(rest) = s.strip_prefix(']') {
            return Ok((Json::Array(items), rest));
        }
        if s.starts_with(',') {
            s = &s[1..];
        } else {
            return Err(JsonError("expected ',' or ']' in array".into()));
        }
    }
}

fn parse_object(s: &str) -> Result<(Json, &str), JsonError> {
    let mut map = BTreeMap::new();
    let mut s = s.trim_start();
    if let Some(rest) = s.strip_prefix('}') {
        return Ok((Json::Object(map), rest));
    }
    loop {
        let s_trim = s.trim_start();
        if !s_trim.starts_with('"') {
            return Err(JsonError("expected string key in object".into()));
        }
        let (key, rest) = parse_string(&s_trim[1..])?;
        let rest = rest.trim_start();
        if !rest.starts_with(':') {
            return Err(JsonError("expected ':' in object".into()));
        }
        let (val, rest) = parse_value(&rest[1..])?;
        map.insert(key, val);
        s = rest.trim_start();
        if let Some(rest) = s.strip_prefix('}') {
            return Ok((Json::Object(map), rest));
        }
        if s.starts_with(',') {
            s = &s[1..];
        } else {
            return Err(JsonError("expected ',' or '}' in object".into()));
        }
    }
}

fn parse_number(s: &str) -> Result<(Json, &str), JsonError> {
    let end = s
        .find(|c: char| {
            !c.is_ascii_digit() && c != '-' && c != '+' && c != '.' && c != 'e' && c != 'E'
        })
        .unwrap_or(s.len());
    let num_str = &s[..end];
    let n: f64 = num_str
        .parse()
        .map_err(|_| JsonError(format!("invalid number: {num_str}")))?;
    Ok((Json::Number(n), &s[end..]))
}
