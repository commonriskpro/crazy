use ail_stdlib::json::{Json, parse, stringify};
use std::collections::BTreeMap;

#[test]
fn json_stringify_null() {
    assert_eq!(stringify(&Json::Null), "null");
}

#[test]
fn json_stringify_bool() {
    assert_eq!(stringify(&Json::Bool(true)), "true");
    assert_eq!(stringify(&Json::Bool(false)), "false");
}

#[test]
fn json_stringify_number_integer() {
    assert_eq!(stringify(&Json::Number(42.0)), "42");
    assert_eq!(stringify(&Json::Number(-7.0)), "-7");
}

#[test]
fn json_stringify_string() {
    assert_eq!(stringify(&Json::Str("hello".into())), r#""hello""#);
    assert_eq!(stringify(&Json::Str("a\"b".into())), r#""a\"b""#);
}

#[test]
fn json_stringify_array() {
    let arr = Json::Array(vec![
        Json::Number(1.0),
        Json::Number(2.0),
        Json::Number(3.0),
    ]);
    assert_eq!(stringify(&arr), "[1,2,3]");
}

#[test]
fn json_stringify_object() {
    let mut map = BTreeMap::new();
    map.insert("x".to_string(), Json::Number(1.0));
    let obj = Json::Object(map);
    assert_eq!(stringify(&obj), r#"{"x":1}"#);
}

#[test]
fn json_parse_null() {
    assert_eq!(parse("null").unwrap(), Json::Null);
}

#[test]
fn json_parse_bool() {
    assert_eq!(parse("true").unwrap(), Json::Bool(true));
    assert_eq!(parse("false").unwrap(), Json::Bool(false));
}

#[test]
fn json_parse_number() {
    assert_eq!(parse("42").unwrap(), Json::Number(42.0));
    assert_eq!(parse("-2.5").unwrap(), Json::Number(-2.5));
}

#[test]
fn json_parse_string() {
    assert_eq!(parse(r#""hello""#).unwrap(), Json::Str("hello".into()));
}

#[test]
fn json_parse_string_with_escape() {
    assert_eq!(parse(r#""a\"b""#).unwrap(), Json::Str("a\"b".into()));
}

#[test]
fn json_parse_array() {
    let v = parse("[1,2,3]").unwrap();
    assert_eq!(
        v,
        Json::Array(vec![
            Json::Number(1.0),
            Json::Number(2.0),
            Json::Number(3.0)
        ])
    );
}

#[test]
fn json_parse_object() {
    let v = parse(r#"{"a":1}"#).unwrap();
    let mut expected = BTreeMap::new();
    expected.insert("a".to_string(), Json::Number(1.0));
    assert_eq!(v, Json::Object(expected));
}

#[test]
fn json_parse_empty_array() {
    assert_eq!(parse("[]").unwrap(), Json::Array(vec![]));
}

#[test]
fn json_parse_empty_object() {
    assert_eq!(parse("{}").unwrap(), Json::Object(BTreeMap::new()));
}

#[test]
fn json_parse_error_on_invalid() {
    assert!(parse("xyz").is_err());
    assert!(parse("{unclosed").is_err());
}

#[test]
fn json_roundtrip() {
    let original = r#"{"name":"Alice","age":30,"active":true}"#;
    let parsed = parse(original).unwrap();
    let stringified = stringify(&parsed);
    let re_parsed = parse(&stringified).unwrap();
    assert_eq!(parsed, re_parsed);
}
