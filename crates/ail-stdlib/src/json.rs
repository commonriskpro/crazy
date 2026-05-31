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

use std::collections::{BTreeMap, BTreeSet};

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

/// Stable parse failure categories that do not echo user JSON payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JsonParseIssueKind {
    EmptyInput,
    UnexpectedEnd,
    UnexpectedTrailingInput,
    UnexpectedCharacter,
    InvalidNumber,
    UnterminatedString,
    UnterminatedEscape,
    UnknownEscape,
    ExpectedLiteral,
    ExpectedStringKey,
    ExpectedObjectColon,
    ExpectedArraySeparator,
    ExpectedObjectSeparator,
}

impl JsonParseIssueKind {
    pub fn code(self) -> &'static str {
        match self {
            JsonParseIssueKind::EmptyInput => "JSON_EMPTY_INPUT",
            JsonParseIssueKind::UnexpectedEnd => "JSON_UNEXPECTED_END",
            JsonParseIssueKind::UnexpectedTrailingInput => "JSON_UNEXPECTED_TRAILING_INPUT",
            JsonParseIssueKind::UnexpectedCharacter => "JSON_UNEXPECTED_CHARACTER",
            JsonParseIssueKind::InvalidNumber => "JSON_INVALID_NUMBER",
            JsonParseIssueKind::UnterminatedString => "JSON_UNTERMINATED_STRING",
            JsonParseIssueKind::UnterminatedEscape => "JSON_UNTERMINATED_ESCAPE",
            JsonParseIssueKind::UnknownEscape => "JSON_UNKNOWN_ESCAPE",
            JsonParseIssueKind::ExpectedLiteral => "JSON_EXPECTED_LITERAL",
            JsonParseIssueKind::ExpectedStringKey => "JSON_EXPECTED_STRING_KEY",
            JsonParseIssueKind::ExpectedObjectColon => "JSON_EXPECTED_OBJECT_COLON",
            JsonParseIssueKind::ExpectedArraySeparator => "JSON_EXPECTED_ARRAY_SEPARATOR",
            JsonParseIssueKind::ExpectedObjectSeparator => "JSON_EXPECTED_OBJECT_SEPARATOR",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            JsonParseIssueKind::EmptyInput | JsonParseIssueKind::UnexpectedEnd => "input-boundary",
            JsonParseIssueKind::UnexpectedTrailingInput => "document-boundary",
            JsonParseIssueKind::UnexpectedCharacter | JsonParseIssueKind::InvalidNumber => {
                "token-shape"
            }
            JsonParseIssueKind::UnterminatedString
            | JsonParseIssueKind::UnterminatedEscape
            | JsonParseIssueKind::UnknownEscape => "string-shape",
            JsonParseIssueKind::ExpectedLiteral
            | JsonParseIssueKind::ExpectedStringKey
            | JsonParseIssueKind::ExpectedObjectColon
            | JsonParseIssueKind::ExpectedArraySeparator
            | JsonParseIssueKind::ExpectedObjectSeparator => "grammar-shape",
        }
    }
}

/// Redacted parse issue for logs/LSP/registry checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonParseIssue {
    pub kind: JsonParseIssueKind,
    pub code: &'static str,
    pub category: &'static str,
}

impl JsonParseIssue {
    pub fn new(kind: JsonParseIssueKind) -> Self {
        Self {
            kind,
            code: kind.code(),
            category: kind.category(),
        }
    }

    pub fn diagnostic_key(&self) -> String {
        format!("std.json.parse:{}:{}", self.category, self.code)
    }
}

/// Error produced during JSON parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError(pub String);

impl JsonError {
    pub fn issue(&self) -> JsonParseIssue {
        JsonParseIssue::new(classify_parse_error(&self.0))
    }

    pub fn code(&self) -> &'static str {
        self.issue().code
    }

    pub fn diagnostic_key(&self) -> String {
        self.issue().diagnostic_key()
    }
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "json error: {}", self.0)
    }
}
impl std::error::Error for JsonError {}

/// Classify existing parse messages into a stable, redacted issue kind.
pub fn classify_parse_error(message: &str) -> JsonParseIssueKind {
    if message == "unexpected end of input" {
        JsonParseIssueKind::UnexpectedEnd
    } else if message.starts_with("unexpected trailing input") {
        JsonParseIssueKind::UnexpectedTrailingInput
    } else if message.starts_with("unexpected character") {
        JsonParseIssueKind::UnexpectedCharacter
    } else if message.starts_with("invalid number") {
        JsonParseIssueKind::InvalidNumber
    } else if message == "unterminated string" {
        JsonParseIssueKind::UnterminatedString
    } else if message == "unterminated escape" {
        JsonParseIssueKind::UnterminatedEscape
    } else if message.starts_with("unknown escape") {
        JsonParseIssueKind::UnknownEscape
    } else if message.starts_with("expected 'null'")
        || message.starts_with("expected 'true'")
        || message.starts_with("expected 'false'")
    {
        JsonParseIssueKind::ExpectedLiteral
    } else if message == "expected string key in object" {
        JsonParseIssueKind::ExpectedStringKey
    } else if message == "expected ':' in object" {
        JsonParseIssueKind::ExpectedObjectColon
    } else if message == "expected ',' or ']' in array" {
        JsonParseIssueKind::ExpectedArraySeparator
    } else if message == "expected ',' or '}' in object" {
        JsonParseIssueKind::ExpectedObjectSeparator
    } else if message == "empty input" {
        JsonParseIssueKind::EmptyInput
    } else {
        JsonParseIssueKind::UnexpectedCharacter
    }
}

/// Stable JSON value categories used by contract diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonKind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

impl JsonKind {
    /// Return the lowercase diagnostic label for this JSON category.
    pub fn as_str(self) -> &'static str {
        match self {
            JsonKind::Null => "null",
            JsonKind::Bool => "bool",
            JsonKind::Number => "number",
            JsonKind::String => "string",
            JsonKind::Array => "array",
            JsonKind::Object => "object",
        }
    }
}

/// Return the stable JSON category for a value.
pub fn kind_of(v: &Json) -> JsonKind {
    match v {
        Json::Null => JsonKind::Null,
        Json::Bool(_) => JsonKind::Bool,
        Json::Number(_) => JsonKind::Number,
        Json::Str(_) => JsonKind::String,
        Json::Array(_) => JsonKind::Array,
        Json::Object(_) => JsonKind::Object,
    }
}

/// A visible JSON contract for decoder/encoder boundaries.
///
/// This is intentionally small and dependency-free: applications can declare
/// the expected JSON shape before deriving or hand-writing decoders, and AIL
/// can report deterministic shape errors without relying on ambient
/// auto-serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonContract {
    Any,
    Null,
    Bool,
    Number,
    String,
    Array(Box<JsonContract>),
    Object(Vec<JsonFieldContract>),
}

impl JsonContract {
    /// A contract accepting any JSON value.
    pub fn any() -> Self {
        JsonContract::Any
    }

    /// A contract requiring an array whose elements all satisfy `item`.
    pub fn array(item: JsonContract) -> Self {
        JsonContract::Array(Box::new(item))
    }

    /// A contract requiring an object with the supplied field contracts.
    pub fn object(fields: Vec<JsonFieldContract>) -> Self {
        JsonContract::Object(fields)
    }

    fn label(&self) -> String {
        match self {
            JsonContract::Any => "any".to_string(),
            JsonContract::Null => "null".to_string(),
            JsonContract::Bool => "bool".to_string(),
            JsonContract::Number => "number".to_string(),
            JsonContract::String => "string".to_string(),
            JsonContract::Array(item) => format!("array<{}>", item.label()),
            JsonContract::Object(fields) => {
                let fields = fields
                    .iter()
                    .map(|field| {
                        let suffix = if field.required { "" } else { "?" };
                        format!(
                            "{}{}:{}",
                            diagnostic_key(&field.name),
                            suffix,
                            field.contract.label()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!("object{{{fields}}}")
            }
        }
    }
}

/// A single object-field contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonFieldContract {
    pub name: String,
    pub contract: JsonContract,
    pub required: bool,
}

impl JsonFieldContract {
    /// Require a field with the provided contract.
    pub fn required(name: impl Into<String>, contract: JsonContract) -> Self {
        Self {
            name: name.into(),
            contract,
            required: true,
        }
    }

    /// Accept a missing field, but validate it when present.
    pub fn optional(name: impl Into<String>, contract: JsonContract) -> Self {
        Self {
            name: name.into(),
            contract,
            required: false,
        }
    }
}

/// Error produced when a JSON value does not satisfy a visible contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonShapeError {
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl std::fmt::Display for JsonShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "json shape mismatch at {}: expected {}, got {}",
            self.path, self.expected, self.actual
        )
    }
}

impl std::error::Error for JsonShapeError {}

/// Stable JSON contract issue kinds for production diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JsonContractIssueKind {
    MissingField,
    TypeMismatch,
}

impl JsonContractIssueKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingField => "JSON_CONTRACT_MISSING_FIELD",
            Self::TypeMismatch => "JSON_CONTRACT_TYPE_MISMATCH",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::MissingField => "object-shape",
            Self::TypeMismatch => "type-shape",
        }
    }
}

/// Redacted, machine-readable JSON contract issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonContractIssue {
    pub kind: JsonContractIssueKind,
    pub code: &'static str,
    pub category: &'static str,
    pub path_shape: String,
    pub expected_shape: String,
    pub actual_shape: String,
}

impl JsonContractIssue {
    fn new(
        kind: JsonContractIssueKind,
        path_shape: impl Into<String>,
        expected_shape: impl Into<String>,
        actual_shape: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            code: kind.code(),
            category: kind.category(),
            path_shape: path_shape.into(),
            expected_shape: expected_shape.into(),
            actual_shape: actual_shape.into(),
        }
    }

    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.json.contract:{}:{}:{}",
            self.category, self.code, self.path_shape
        )
    }
}

/// Return a deterministic shape descriptor for diagnostics and registry checks.
pub fn shape_descriptor(v: &Json) -> String {
    match v {
        Json::Null | Json::Bool(_) | Json::Number(_) | Json::Str(_) => {
            kind_of(v).as_str().to_string()
        }
        Json::Array(items) => {
            if items.is_empty() {
                return "array<empty>".to_string();
            }

            let shapes = items
                .iter()
                .map(shape_descriptor)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();

            if shapes.len() == 1 {
                format!("array<{}>", shapes[0])
            } else {
                format!("array<mixed:{}>", shapes.join("|"))
            }
        }
        Json::Object(map) => {
            let fields = map
                .iter()
                .map(|(key, value)| format!("{}:{}", diagnostic_key(key), shape_descriptor(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("object{{{fields}}}")
        }
    }
}

/// Return every contract issue using stable, redacted descriptors.
pub fn diagnose_contract(value: &Json, contract: &JsonContract) -> Vec<JsonContractIssue> {
    let mut issues = Vec::new();
    diagnose_contract_at(value, contract, "$", &mut issues);
    sort_contract_issues(&mut issues);
    issues
}

fn diagnose_contract_at(
    value: &Json,
    contract: &JsonContract,
    path_shape: &str,
    issues: &mut Vec<JsonContractIssue>,
) {
    match contract {
        JsonContract::Any => {}
        JsonContract::Null => diagnose_kind(value, JsonKind::Null, contract, path_shape, issues),
        JsonContract::Bool => diagnose_kind(value, JsonKind::Bool, contract, path_shape, issues),
        JsonContract::Number => {
            diagnose_kind(value, JsonKind::Number, contract, path_shape, issues)
        }
        JsonContract::String => {
            diagnose_kind(value, JsonKind::String, contract, path_shape, issues)
        }
        JsonContract::Array(item_contract) => match value {
            Json::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    diagnose_contract_at(
                        item,
                        item_contract,
                        &format!("{path_shape}[{index}]"),
                        issues,
                    );
                }
            }
            _ => issues.push(contract_issue(
                JsonContractIssueKind::TypeMismatch,
                path_shape,
                contract,
                value,
            )),
        },
        JsonContract::Object(fields) => match value {
            Json::Object(map) => {
                for (ordinal, field) in fields.iter().enumerate() {
                    let field_path = format!("{path_shape}.field#{ordinal}");
                    match map.get(&field.name) {
                        Some(field_value) => {
                            diagnose_contract_at(field_value, &field.contract, &field_path, issues);
                        }
                        None if field.required => issues.push(JsonContractIssue::new(
                            JsonContractIssueKind::MissingField,
                            field_path,
                            contract_shape_label(&field.contract),
                            "missing",
                        )),
                        None => {}
                    }
                }
            }
            _ => issues.push(contract_issue(
                JsonContractIssueKind::TypeMismatch,
                path_shape,
                contract,
                value,
            )),
        },
    }
}

fn diagnose_kind(
    value: &Json,
    expected: JsonKind,
    contract: &JsonContract,
    path_shape: &str,
    issues: &mut Vec<JsonContractIssue>,
) {
    if kind_of(value) != expected {
        issues.push(contract_issue(
            JsonContractIssueKind::TypeMismatch,
            path_shape,
            contract,
            value,
        ));
    }
}

fn contract_issue(
    kind: JsonContractIssueKind,
    path_shape: &str,
    contract: &JsonContract,
    value: &Json,
) -> JsonContractIssue {
    JsonContractIssue::new(
        kind,
        path_shape,
        contract_shape_label(contract),
        value_shape_label(value),
    )
}

fn sort_contract_issues(issues: &mut Vec<JsonContractIssue>) {
    issues.sort_by(|a, b| {
        (
            a.path_shape.as_str(),
            a.kind,
            a.expected_shape.as_str(),
            a.actual_shape.as_str(),
        )
            .cmp(&(
                b.path_shape.as_str(),
                b.kind,
                b.expected_shape.as_str(),
                b.actual_shape.as_str(),
            ))
    });
    issues.dedup();
}

fn contract_shape_label(contract: &JsonContract) -> String {
    match contract {
        JsonContract::Any => "any".to_string(),
        JsonContract::Null => "null".to_string(),
        JsonContract::Bool => "bool".to_string(),
        JsonContract::Number => "number".to_string(),
        JsonContract::String => "string".to_string(),
        JsonContract::Array(item) => format!("array<{}>", contract_shape_label(item)),
        JsonContract::Object(fields) => format!("object<fields:{}>", fields.len()),
    }
}

fn value_shape_label(value: &Json) -> String {
    match value {
        Json::Array(items) => {
            if items.is_empty() {
                "array<empty>".to_string()
            } else {
                let shapes = items
                    .iter()
                    .map(value_shape_label)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                if shapes.len() == 1 {
                    format!("array<{}>", shapes[0])
                } else {
                    "array<mixed>".to_string()
                }
            }
        }
        Json::Object(map) => format!("object<fields:{}>", map.len()),
        _ => kind_of(value).as_str().to_string(),
    }
}

/// Validate a value against a visible JSON contract.
pub fn validate_contract(value: &Json, contract: &JsonContract) -> Result<(), JsonShapeError> {
    validate_contract_at(value, contract, "$")
}

fn validate_contract_at(
    value: &Json,
    contract: &JsonContract,
    path: &str,
) -> Result<(), JsonShapeError> {
    match contract {
        JsonContract::Any => Ok(()),
        JsonContract::Null => validate_kind(value, JsonKind::Null, contract, path),
        JsonContract::Bool => validate_kind(value, JsonKind::Bool, contract, path),
        JsonContract::Number => validate_kind(value, JsonKind::Number, contract, path),
        JsonContract::String => validate_kind(value, JsonKind::String, contract, path),
        JsonContract::Array(item_contract) => match value {
            Json::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    validate_contract_at(item, item_contract, &format!("{path}[{index}]"))?;
                }
                Ok(())
            }
            _ => Err(shape_error(path, contract, value)),
        },
        JsonContract::Object(fields) => match value {
            Json::Object(map) => {
                for field in fields {
                    let field_path = append_field_path(path, &field.name);
                    match map.get(&field.name) {
                        Some(field_value) => {
                            validate_contract_at(field_value, &field.contract, &field_path)?
                        }
                        None if field.required => {
                            return Err(JsonShapeError {
                                path: field_path,
                                expected: field.contract.label(),
                                actual: "missing".to_string(),
                            });
                        }
                        None => {}
                    }
                }
                Ok(())
            }
            _ => Err(shape_error(path, contract, value)),
        },
    }
}

fn validate_kind(
    value: &Json,
    expected: JsonKind,
    contract: &JsonContract,
    path: &str,
) -> Result<(), JsonShapeError> {
    if kind_of(value) == expected {
        Ok(())
    } else {
        Err(shape_error(path, contract, value))
    }
}

fn shape_error(path: &str, contract: &JsonContract, value: &Json) -> JsonShapeError {
    JsonShapeError {
        path: path.to_string(),
        expected: contract.label(),
        actual: shape_descriptor(value),
    }
}

fn append_field_path(path: &str, field: &str) -> String {
    if is_dot_path_segment(field) {
        format!("{path}.{field}")
    } else {
        format!("{path}[{}]", stringify(&Json::Str(field.to_string())))
    }
}

fn is_dot_path_segment(field: &str) -> bool {
    let mut chars = field.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn diagnostic_key(key: &str) -> String {
    if is_dot_path_segment(key) {
        key.to_string()
    } else {
        stringify(&Json::Str(key.to_string()))
    }
}

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
    if s.is_empty() {
        return Err(JsonError("empty input".into()));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn object(fields: &[(&str, Json)]) -> Json {
        Json::Object(
            fields
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn shape_descriptor_is_stable_for_nested_values() {
        let value = object(&[
            ("active", Json::Bool(true)),
            ("name", Json::Str("Ada".to_string())),
            (
                "roles",
                Json::Array(vec![
                    Json::Str("admin".to_string()),
                    Json::Str("ops".to_string()),
                ]),
            ),
        ]);

        assert_eq!(
            shape_descriptor(&value),
            "object{active:bool,name:string,roles:array<string>}"
        );
    }

    #[test]
    fn shape_descriptor_reports_mixed_arrays() {
        let value = Json::Array(vec![
            Json::Number(1.0),
            Json::Str("two".to_string()),
            Json::Bool(false),
        ]);

        assert_eq!(shape_descriptor(&value), "array<mixed:bool|number|string>");
    }

    #[test]
    fn validate_contract_accepts_visible_object_schema() {
        let value = object(&[
            ("id", Json::Number(42.0)),
            ("name", Json::Str("Ada".to_string())),
            ("tags", Json::Array(vec![Json::Str("compiler".to_string())])),
        ]);
        let contract = JsonContract::object(vec![
            JsonFieldContract::required("id", JsonContract::Number),
            JsonFieldContract::required("name", JsonContract::String),
            JsonFieldContract::optional("tags", JsonContract::array(JsonContract::String)),
        ]);

        assert_eq!(validate_contract(&value, &contract), Ok(()));
    }

    #[test]
    fn validate_contract_reports_missing_required_field_path() {
        let value = object(&[("id", Json::Number(42.0))]);
        let contract = JsonContract::object(vec![
            JsonFieldContract::required("id", JsonContract::Number),
            JsonFieldContract::required("name", JsonContract::String),
        ]);

        assert_eq!(
            validate_contract(&value, &contract),
            Err(JsonShapeError {
                path: "$.name".to_string(),
                expected: "string".to_string(),
                actual: "missing".to_string(),
            })
        );
    }

    #[test]
    fn validate_contract_reports_nested_array_item_path() {
        let value = object(&[(
            "tags",
            Json::Array(vec![Json::Str("compiler".to_string()), Json::Number(7.0)]),
        )]);
        let contract = JsonContract::object(vec![JsonFieldContract::required(
            "tags",
            JsonContract::array(JsonContract::String),
        )]);

        assert_eq!(
            validate_contract(&value, &contract),
            Err(JsonShapeError {
                path: "$.tags[1]".to_string(),
                expected: "string".to_string(),
                actual: "number".to_string(),
            })
        );
    }

    #[test]
    fn diagnose_contract_reports_all_issues_without_field_names() {
        let value = object(&[
            ("secret_name", Json::Number(42.0)),
            (
                "token",
                Json::Array(vec![Json::Bool(true), Json::Number(9.0)]),
            ),
        ]);
        let contract = JsonContract::object(vec![
            JsonFieldContract::required("secret_name", JsonContract::String),
            JsonFieldContract::required("missing_password", JsonContract::Bool),
            JsonFieldContract::required("token", JsonContract::array(JsonContract::String)),
        ]);

        let issues = diagnose_contract(&value, &contract);
        let keys = issues
            .iter()
            .map(JsonContractIssue::diagnostic_key)
            .collect::<Vec<_>>();

        assert_eq!(
            keys,
            vec![
                "std.json.contract:type-shape:JSON_CONTRACT_TYPE_MISMATCH:$.field#0",
                "std.json.contract:object-shape:JSON_CONTRACT_MISSING_FIELD:$.field#1",
                "std.json.contract:type-shape:JSON_CONTRACT_TYPE_MISMATCH:$.field#2[0]",
                "std.json.contract:type-shape:JSON_CONTRACT_TYPE_MISMATCH:$.field#2[1]",
            ]
        );
        assert!(keys.iter().all(|key| !key.contains("secret_name")));
        assert!(keys.iter().all(|key| !key.contains("missing_password")));
        assert!(keys.iter().all(|key| !key.contains("token")));
        assert_eq!(issues[1].expected_shape, "bool");
        assert_eq!(issues[1].actual_shape, "missing");
    }

    #[test]
    fn diagnose_contract_redacts_object_shapes() {
        let value = object(&[("api_key", Json::Null), ("secret", Json::Bool(true))]);
        let contract = JsonContract::String;

        let issues = diagnose_contract(&value, &contract);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "JSON_CONTRACT_TYPE_MISMATCH");
        assert_eq!(issues[0].category, "type-shape");
        assert_eq!(issues[0].path_shape, "$".to_string());
        assert_eq!(issues[0].expected_shape, "string");
        assert_eq!(issues[0].actual_shape, "object<fields:2>");
        assert!(!issues[0].diagnostic_key().contains("api_key"));
        assert!(!issues[0].diagnostic_key().contains("secret"));
    }

    #[test]
    fn parse_errors_expose_redacted_stable_issue_codes() {
        let err = parse("{\"token\":\"secret\"} trailing").expect_err("trailing input");
        let issue = err.issue();

        assert_eq!(issue.kind, JsonParseIssueKind::UnexpectedTrailingInput);
        assert_eq!(issue.code, "JSON_UNEXPECTED_TRAILING_INPUT");
        assert_eq!(issue.category, "document-boundary");
        assert_eq!(
            issue.diagnostic_key(),
            "std.json.parse:document-boundary:JSON_UNEXPECTED_TRAILING_INPUT"
        );
        assert!(!issue.diagnostic_key().contains("secret"));
    }

    #[test]
    fn parse_error_classification_covers_string_and_number_shapes() {
        let bad_escape = parse("\"\\x\"").expect_err("bad escape");
        assert_eq!(bad_escape.code(), "JSON_UNKNOWN_ESCAPE");
        assert_eq!(
            bad_escape.diagnostic_key(),
            "std.json.parse:string-shape:JSON_UNKNOWN_ESCAPE"
        );

        let bad_number = parse("1e+").expect_err("bad number");
        assert_eq!(bad_number.code(), "JSON_INVALID_NUMBER");
        assert_eq!(
            bad_number.diagnostic_key(),
            "std.json.parse:token-shape:JSON_INVALID_NUMBER"
        );
    }

    #[test]
    fn empty_input_has_specific_boundary_code() {
        let err = parse("   ").expect_err("empty input");

        assert_eq!(err.code(), "JSON_EMPTY_INPUT");
        assert_eq!(
            err.diagnostic_key(),
            "std.json.parse:input-boundary:JSON_EMPTY_INPUT"
        );
    }
}
