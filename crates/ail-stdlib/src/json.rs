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

/// Error produced during JSON parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError(pub String);

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "json error: {}", self.0)
    }
}
impl std::error::Error for JsonError {}

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
}
