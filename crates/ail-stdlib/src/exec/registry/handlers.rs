// ── ail-stdlib::exec::registry::handlers ─────────────────────────────────
//
// Pure function implementations for the stdlib v1 entry table.
//
// All exported symbols are `pub(super)` — visible to registry.rs only.
// Internal helpers (expect_arity, json_to_stdlib, stdlib_to_json) are private.

use std::sync::{Arc, Mutex};

use crate::{concurrent, crypto, encoding, json, numeric, text};

// super = registry, super::super = exec (which re-exports StdlibValue/StdlibExecError)
use super::super::{StdlibExecError, StdlibValue};

// ── Validation helper ─────────────────────────────────────────────────────

fn expect_arity(args: &[StdlibValue], expected: usize) -> Result<(), StdlibExecError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(StdlibExecError::Arity {
            expected,
            actual: args.len(),
        })
    }
}

// ── Option combinators ────────────────────────────────────────────────────

pub(super) fn option_map(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    let StdlibValue::Function(function) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    option
        .clone()
        .map(|value| function(*value).map(|mapped| StdlibValue::Option(Some(Box::new(mapped)))))
        .unwrap_or(Ok(StdlibValue::Option(None)))
}

pub(super) fn option_and_then(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    let StdlibValue::Function(function) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    match option.clone() {
        Some(value) => match function(*value)? {
            StdlibValue::Option(next) => Ok(StdlibValue::Option(next)),
            _ => Err(StdlibExecError::Type { expected: "Option" }),
        },
        None => Ok(StdlibValue::Option(None)),
    }
}

pub(super) fn option_unwrap_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    Ok(option
        .clone()
        .map(|value| *value)
        .unwrap_or_else(|| args[1].clone()))
}

pub(super) fn option_ok_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    Ok(match option.clone() {
        Some(value) => StdlibValue::Result(Ok(value)),
        None => StdlibValue::Result(Err(Box::new(args[1].clone()))),
    })
}

/// `option.transpose`: `Option<Result<T, E>>` → `Result<Option<T>, E>`
///
/// - `None`        → `Ok(None)`
/// - `Some(Ok(v))` → `Ok(Some(v))`
/// - `Some(Err(e))` → `Err(e)`
pub(super) fn option_transpose(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Option(option) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Option" });
    };
    Ok(match option.clone() {
        None => StdlibValue::Result(Ok(Box::new(StdlibValue::Option(None)))),
        Some(inner) => match *inner {
            StdlibValue::Result(Ok(v)) => {
                StdlibValue::Result(Ok(Box::new(StdlibValue::Option(Some(v)))))
            }
            StdlibValue::Result(Err(e)) => StdlibValue::Result(Err(e)),
            _ => return Err(StdlibExecError::Type { expected: "Result" }),
        },
    })
}

/// `option.collect_results`: `List<Result<T, E>>` → `Result<List<T>, E>`
///
/// Short-circuits on the first `Err`, otherwise collects all `Ok` values.
pub(super) fn option_collect_results(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let mut collected = Vec::with_capacity(items.len());
    for item in items {
        match item {
            StdlibValue::Result(Ok(v)) => collected.push(*v.clone()),
            StdlibValue::Result(Err(e)) => {
                return Ok(StdlibValue::Result(Err(e.clone())));
            }
            _ => return Err(StdlibExecError::Type { expected: "Result" }),
        }
    }
    Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::List(
        collected,
    )))))
}

/// `result.transpose`: `Result<Option<T>, E>` → `Option<Result<T, E>>`
///
/// - `Ok(Some(v))` → `Some(Ok(v))`
/// - `Ok(None)`    → `None`
/// - `Err(e)`      → `Some(Err(e))`
pub(super) fn result_transpose(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Result(result) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Result" });
    };
    Ok(match result.clone() {
        Ok(inner) => match *inner {
            StdlibValue::Option(Some(v)) => {
                StdlibValue::Option(Some(Box::new(StdlibValue::Result(Ok(v)))))
            }
            StdlibValue::Option(None) => StdlibValue::Option(None),
            _ => return Err(StdlibExecError::Type { expected: "Option" }),
        },
        Err(e) => StdlibValue::Option(Some(Box::new(StdlibValue::Result(Err(e))))),
    })
}

// ── Result combinators ────────────────────────────────────────────────────

pub(super) fn result_map(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Result(result) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Result" });
    };
    let StdlibValue::Function(function) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    Ok(match result.clone() {
        Ok(value) => StdlibValue::Result(Ok(Box::new(function(*value)?))),
        Err(error) => StdlibValue::Result(Err(error)),
    })
}

pub(super) fn result_and_then(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Result(result) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Result" });
    };
    let StdlibValue::Function(function) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    match result.clone() {
        Ok(value) => match function(*value)? {
            StdlibValue::Result(next) => Ok(StdlibValue::Result(next)),
            _ => Err(StdlibExecError::Type { expected: "Result" }),
        },
        Err(error) => Ok(StdlibValue::Result(Err(error))),
    }
}

pub(super) fn result_unwrap_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Result(result) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Result" });
    };
    Ok(result
        .clone()
        .map(|value| *value)
        .unwrap_or_else(|_| args[1].clone()))
}

// ── Collection adapters ───────────────────────────────────────────────────

pub(super) fn list_length(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::List(items) => Ok(StdlibValue::Int(items.len() as i64)),
        _ => Err(StdlibExecError::Type { expected: "List" }),
    }
}

pub(super) fn list_push(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(mut items) = args[0].clone() else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    items.push(args[1].clone());
    Ok(StdlibValue::List(items))
}

pub(super) fn list_get(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Int(index) = args[1] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    let value = usize::try_from(index)
        .ok()
        .and_then(|index| items.get(index).cloned())
        .map(Box::new);
    Ok(StdlibValue::Option(value))
}

pub(super) fn map_get(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Map(map) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Map" });
    };
    let StdlibValue::Text(key) = &args[1] else {
        return Err(StdlibExecError::Type { expected: "Text" });
    };
    Ok(StdlibValue::Option(map.get(key).cloned().map(Box::new)))
}

pub(super) fn map_insert(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let StdlibValue::Map(mut map) = args[0].clone() else {
        return Err(StdlibExecError::Type { expected: "Map" });
    };
    let StdlibValue::Text(key) = &args[1] else {
        return Err(StdlibExecError::Type { expected: "Text" });
    };
    map.insert(key.clone(), args[2].clone());
    Ok(StdlibValue::Map(map))
}

pub(super) fn set_contains(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    Ok(StdlibValue::Bool(items.contains(&args[1])))
}

pub(super) fn set_insert(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(mut items) = args[0].clone() else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    if !items.contains(&args[1]) {
        items.push(args[1].clone());
    }
    Ok(StdlibValue::List(items))
}

// ── Text adapters ─────────────────────────────────────────────────────────

pub(super) fn text_trim(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(value) => Ok(StdlibValue::Text(text::text_trim(value))),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

pub(super) fn text_split(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(value), StdlibValue::Text(delimiter)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text",
        });
    };
    Ok(StdlibValue::List(
        text::text_split(value, delimiter)
            .into_iter()
            .map(StdlibValue::Text)
            .collect(),
    ))
}

pub(super) fn text_join(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::List(parts), StdlibValue::Text(separator)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "List<Text>, Text",
        });
    };
    let strings = parts
        .iter()
        .map(|part| match part {
            StdlibValue::Text(value) => Ok(value.as_str()),
            _ => Err(StdlibExecError::Type { expected: "Text" }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(StdlibValue::Text(text::text_join(&strings, separator)))
}

pub(super) fn text_normalize(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(value) => Ok(StdlibValue::Text(text::text_normalize(
            value,
            text::NormalizeForm::Nfc,
        ))),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

pub(super) fn text_encode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(value) => Ok(StdlibValue::Bytes(text::text_to_bytes(value))),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

pub(super) fn text_decode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(bytes) => Ok(match text::text_from_bytes(bytes) {
            Ok(value) => StdlibValue::Result(Ok(Box::new(StdlibValue::Text(value)))),
            Err(error) => StdlibValue::Result(Err(Box::new(StdlibValue::Text(error)))),
        }),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

pub(super) fn text_format(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(template), StdlibValue::List(values)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, List<Text>",
        });
    };
    let mut output = template.clone();
    for value in values {
        let StdlibValue::Text(value) = value else {
            return Err(StdlibExecError::Type { expected: "Text" });
        };
        output = output.replacen("{}", value, 1);
    }
    Ok(StdlibValue::Text(output))
}

pub(super) fn text_regex(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(value), StdlibValue::Text(pattern)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text",
        });
    };
    let re = regex::Regex::new(pattern)
        .map_err(|e| StdlibExecError::Message(format!("invalid regex: {e}")))?;
    Ok(StdlibValue::Bool(re.is_match(value)))
}

pub(super) fn text_length_graphemes_exec(
    args: &[StdlibValue],
) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(s) => Ok(StdlibValue::Int(text::text_length_graphemes(s) as i64)),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

// ── Crypto adapters ───────────────────────────────────────────────────────

pub(super) fn crypto_hash(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(bytes) => Ok(StdlibValue::Bytes(crypto::Hash::blake3(bytes).0.to_vec())),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

pub(super) fn crypto_hmac(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Bytes(key), StdlibValue::Bytes(msg)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Bytes, Bytes",
        });
    };
    Ok(StdlibValue::Bytes(
        crypto::Hmac::compute(key, msg).0.to_vec(),
    ))
}

pub(super) fn crypto_constant_time_eq(
    args: &[StdlibValue],
) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Bytes(a), StdlibValue::Bytes(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Bytes, Bytes",
        });
    };
    Ok(StdlibValue::Bool(crypto::constant_time_eq(a, b)))
}

// ── Encoding adapters ─────────────────────────────────────────────────────

pub(super) fn encoding_base64_encode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(bytes) => Ok(StdlibValue::Text(encoding::base64_encode(bytes))),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

pub(super) fn encoding_base64_decode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(s) => Ok(StdlibValue::Result(
            encoding::base64_decode(s)
                .map(|bytes| Box::new(StdlibValue::Bytes(bytes)))
                .map_err(|e| Box::new(StdlibValue::Text(e.0))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

pub(super) fn encoding_hex_encode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(bytes) => Ok(StdlibValue::Text(encoding::hex_encode(bytes))),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

pub(super) fn encoding_hex_decode(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(s) => Ok(StdlibValue::Result(
            encoding::hex_decode(s)
                .map(|bytes| Box::new(StdlibValue::Bytes(bytes)))
                .map_err(|e| Box::new(StdlibValue::Text(e.0))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

// ── JSON adapters ─────────────────────────────────────────────────────────

/// Convert a `json::Json` value into a `StdlibValue`.
fn json_to_stdlib(v: json::Json) -> StdlibValue {
    match v {
        json::Json::Null => StdlibValue::Unit,
        json::Json::Bool(b) => StdlibValue::Bool(b),
        json::Json::Number(n) => {
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                StdlibValue::Int(n as i64)
            } else {
                StdlibValue::Float(n)
            }
        }
        json::Json::Str(s) => StdlibValue::Text(s),
        json::Json::Array(arr) => StdlibValue::List(arr.into_iter().map(json_to_stdlib).collect()),
        json::Json::Object(map) => StdlibValue::Map(
            map.into_iter()
                .map(|(k, v)| (k, json_to_stdlib(v)))
                .collect(),
        ),
    }
}

/// Convert a `StdlibValue` into a `json::Json` for stringification.
fn stdlib_to_json(v: &StdlibValue) -> json::Json {
    match v {
        StdlibValue::Unit => json::Json::Null,
        StdlibValue::Bool(b) => json::Json::Bool(*b),
        StdlibValue::Int(n) => json::Json::Number(*n as f64),
        StdlibValue::Float(f) => json::Json::Number(*f),
        StdlibValue::Text(s) => json::Json::Str(s.clone()),
        StdlibValue::Bytes(b) => json::Json::Str(encoding::hex_encode(b)),
        StdlibValue::List(items) => json::Json::Array(items.iter().map(stdlib_to_json).collect()),
        StdlibValue::Map(map) => json::Json::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), stdlib_to_json(v)))
                .collect(),
        ),
        StdlibValue::Option(None) => json::Json::Null,
        StdlibValue::Option(Some(v)) => stdlib_to_json(v),
        StdlibValue::Result(Ok(v)) => stdlib_to_json(v),
        StdlibValue::Result(Err(e)) => stdlib_to_json(e),
        StdlibValue::Function(_) | StdlibValue::Channel(_) => json::Json::Null,
    }
}

pub(super) fn json_parse(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Text(s) => Ok(StdlibValue::Result(
            json::parse(s)
                .map(|v| Box::new(json_to_stdlib(v)))
                .map_err(|e| Box::new(StdlibValue::Text(e.0))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Text" }),
    }
}

pub(super) fn json_stringify(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    Ok(StdlibValue::Text(json::stringify(&stdlib_to_json(
        &args[0],
    ))))
}

// ── Numeric adapters ──────────────────────────────────────────────────────

pub(super) fn numeric_narrow_to_i32(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match args[0] {
        StdlibValue::Int(n) => Ok(StdlibValue::Result(
            i32::try_from(n)
                .map(|v| Box::new(StdlibValue::Int(v as i64)))
                .map_err(|e| Box::new(StdlibValue::Text(e.to_string()))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

pub(super) fn numeric_narrow_to_u32(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match args[0] {
        StdlibValue::Int(n) => Ok(StdlibValue::Result(
            u32::try_from(n)
                .map(|v| Box::new(StdlibValue::Int(v as i64)))
                .map_err(|e| Box::new(StdlibValue::Text(e.to_string()))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

pub(super) fn numeric_checked_add(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Option(
        numeric::checked_add(*a, *b).map(|v| Box::new(StdlibValue::Int(v))),
    ))
}

pub(super) fn numeric_checked_sub(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Option(
        numeric::checked_sub(*a, *b).map(|v| Box::new(StdlibValue::Int(v))),
    ))
}

pub(super) fn numeric_checked_mul(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Option(
        numeric::checked_mul(*a, *b).map(|v| Box::new(StdlibValue::Int(v))),
    ))
}

pub(super) fn numeric_wrapping_add(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::wrapping_add(*a, *b)))
}

pub(super) fn numeric_saturating_add(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::saturating_add(*a, *b)))
}

// ── Concurrent channel adapters ───────────────────────────────────────────

pub(super) fn concurrent_channel_new(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Int(capacity) = args[0] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    let cap = capacity.max(0) as usize;
    let channel = concurrent::Channel::new(cap);
    Ok(StdlibValue::Channel(Arc::new(Mutex::new(channel))))
}

pub(super) fn concurrent_channel_send(
    args: &[StdlibValue],
) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Channel(ref arc) = args[0] else {
        return Err(StdlibExecError::Type {
            expected: "Channel",
        });
    };
    let value = args[1].clone();
    let ch = arc.lock().unwrap();
    match ch.send(value) {
        Ok(()) => Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Unit)))),
        Err(_) => Ok(StdlibValue::Result(Err(Box::new(StdlibValue::Text(
            "channel full".to_string(),
        ))))),
    }
}

pub(super) fn concurrent_channel_recv(
    args: &[StdlibValue],
) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Channel(ref arc) = args[0] else {
        return Err(StdlibExecError::Type {
            expected: "Channel",
        });
    };
    let ch = arc.lock().unwrap();
    Ok(StdlibValue::Option(ch.recv().map(Box::new)))
}

pub(super) fn concurrent_channel_len(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Channel(ref arc) = args[0] else {
        return Err(StdlibExecError::Type {
            expected: "Channel",
        });
    };
    let ch = arc.lock().unwrap();
    Ok(StdlibValue::Int(ch.len() as i64))
}

// ── Time pure adapters ────────────────────────────────────────────────────
//
// Instants are represented as Int(epoch_ms), consistent with clock.now.

pub(super) fn time_duration_since_exec(
    args: &[StdlibValue],
) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(later), StdlibValue::Int(earlier)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(later - earlier))
}

pub(super) fn time_add_duration_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(instant), StdlibValue::Int(delta)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(instant + delta))
}

pub(super) fn time_instant_to_ms_exec(
    args: &[StdlibValue],
) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Int(ms) => Ok(StdlibValue::Int(*ms)),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

// ── Collections list functional adapters ─────────────────────────────────
//
// list.map, list.filter, list.fold share identical semantics with the iter
// variants; they delegate to the same shared helpers.

pub(super) fn list_map_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    iter_map_exec(args)
}

pub(super) fn list_filter_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    iter_filter_exec(args)
}

pub(super) fn list_fold_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    iter_fold_exec(args)
}

pub(super) fn list_concat_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(a) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::List(b) = &args[1] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let mut result = a.clone();
    result.extend_from_slice(b);
    Ok(StdlibValue::List(result))
}

// ── Iter functional adapters ──────────────────────────────────────────────

pub(super) fn iter_map_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Function(f) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    let mapped = items
        .clone()
        .into_iter()
        .map(f)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(StdlibValue::List(mapped))
}

pub(super) fn iter_filter_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Function(f) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    let mut kept = Vec::new();
    for item in items.clone() {
        match f(item.clone())? {
            StdlibValue::Bool(true) => kept.push(item),
            StdlibValue::Bool(false) => {}
            _ => return Err(StdlibExecError::Type { expected: "Bool" }),
        }
    }
    Ok(StdlibValue::List(kept))
}

pub(super) fn iter_fold_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Function(f) = args[2] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    let mut acc = args[1].clone();
    for item in items.clone() {
        // Binary encoding: fn receives List([acc, item])
        let pair = StdlibValue::List(vec![acc, item]);
        acc = f(pair)?;
    }
    Ok(acc)
}

pub(super) fn iter_traverse_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Function(f) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    let mut collected = Vec::new();
    for item in items.clone() {
        match f(item)? {
            StdlibValue::Result(Ok(v)) => collected.push(*v),
            StdlibValue::Result(Err(e)) => {
                return Ok(StdlibValue::Result(Err(e)));
            }
            _ => return Err(StdlibExecError::Type { expected: "Result" }),
        }
    }
    Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::List(
        collected,
    )))))
}
