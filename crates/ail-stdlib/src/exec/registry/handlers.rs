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

// ── Tuple adapters ────────────────────────────────────────────────────────

pub(super) fn tuple_length(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Tuple(items) => Ok(StdlibValue::Int(items.len() as i64)),
        _ => Err(StdlibExecError::Type { expected: "Tuple" }),
    }
}

pub(super) fn tuple_get(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Tuple(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Tuple" });
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

pub(super) fn tuple_first(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Tuple(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Tuple" });
    };
    Ok(StdlibValue::Option(items.first().cloned().map(Box::new)))
}

pub(super) fn tuple_second(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Tuple(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Tuple" });
    };
    Ok(StdlibValue::Option(items.get(1).cloned().map(Box::new)))
}

// ── Collection adapters ───────────────────────────────────────────────────

pub(super) fn list_length(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::List(items) => Ok(StdlibValue::Int(items.len() as i64)),
        _ => Err(StdlibExecError::Type { expected: "List" }),
    }
}

pub(super) fn list_is_empty(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::List(items) => Ok(StdlibValue::Bool(items.is_empty())),
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

pub(super) fn map_contains_key(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Map(map) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Map" });
    };
    let StdlibValue::Text(key) = &args[1] else {
        return Err(StdlibExecError::Type { expected: "Text" });
    };
    Ok(StdlibValue::Bool(map.contains_key(key)))
}

pub(super) fn map_length(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Map(map) => Ok(StdlibValue::Int(map.len() as i64)),
        _ => Err(StdlibExecError::Type { expected: "Map" }),
    }
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

pub(super) fn set_length(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::List(items) => Ok(StdlibValue::Int(items.len() as i64)),
        _ => Err(StdlibExecError::Type { expected: "List" }),
    }
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

pub(super) fn queue_push_back(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(mut items) = args[0].clone() else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    items.push(args[1].clone());
    Ok(StdlibValue::List(items))
}

pub(super) fn queue_pop_front(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    if items.is_empty() {
        return Ok(StdlibValue::Option(None));
    }
    let mut rest = items.clone();
    let front = rest.remove(0);
    Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Tuple(
        vec![front, StdlibValue::List(rest)],
    )))))
}

pub(super) fn queue_peek_front(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    Ok(StdlibValue::Option(items.first().cloned().map(Box::new)))
}

pub(super) fn queue_length(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::List(items) => Ok(StdlibValue::Int(items.len() as i64)),
        _ => Err(StdlibExecError::Type { expected: "List" }),
    }
}

pub(super) fn queue_is_empty(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::List(items) => Ok(StdlibValue::Bool(items.is_empty())),
        _ => Err(StdlibExecError::Type { expected: "List" }),
    }
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

/// `std.text.normalize` — optional second arg selects the normalization form.
///
/// - 1 arg `(text)`: normalizes to NFC (default).
/// - 2 args `(text, form)`: `form` must be the string `"nfc"` or `"nfd"`.
///   Any other value returns `StdlibExecError::Message`.
pub(super) fn text_normalize(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    let form = match args.len() {
        1 => text::NormalizeForm::Nfc,
        2 => {
            let StdlibValue::Text(form_str) = &args[1] else {
                return Err(StdlibExecError::Type { expected: "Text" });
            };
            match form_str.to_lowercase().as_str() {
                "nfc" => text::NormalizeForm::Nfc,
                "nfd" => text::NormalizeForm::Nfd,
                other => {
                    return Err(StdlibExecError::Message(format!(
                        "unknown normalization form: {other}; expected \"nfc\" or \"nfd\""
                    )));
                }
            }
        }
        n => {
            return Err(StdlibExecError::Arity {
                expected: 1,
                actual: n,
            });
        }
    };
    match &args[0] {
        StdlibValue::Text(value) => Ok(StdlibValue::Text(text::text_normalize(value, form))),
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

pub(super) fn text_starts_with_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(s), StdlibValue::Text(prefix)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text",
        });
    };
    Ok(StdlibValue::Bool(text::text_starts_with(s, prefix)))
}

pub(super) fn text_ends_with_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(s), StdlibValue::Text(suffix)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text",
        });
    };
    Ok(StdlibValue::Bool(text::text_ends_with(s, suffix)))
}

pub(super) fn text_contains_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(s), StdlibValue::Text(needle)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text",
        });
    };
    Ok(StdlibValue::Bool(text::text_contains(s, needle)))
}

pub(super) fn text_byte_at_or_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Text(s), StdlibValue::Int(index), StdlibValue::Int(fallback)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type {
            expected: "Text, Int, Int",
        });
    };
    Ok(StdlibValue::Int(text::text_byte_at_or(
        s, *index, *fallback,
    )))
}

pub(super) fn text_slice_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Text(s), StdlibValue::Int(start), StdlibValue::Int(length)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type {
            expected: "Text, Int, Int",
        });
    };
    Ok(StdlibValue::Text(text::text_slice(s, *start, *length)))
}

pub(super) fn text_replace_first_exec(
    args: &[StdlibValue],
) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Text(s), StdlibValue::Text(needle), StdlibValue::Text(replacement)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text, Text",
        });
    };
    Ok(StdlibValue::Text(text::text_replace_first(
        s,
        needle,
        replacement,
    )))
}

pub(super) fn text_index_of_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(s), StdlibValue::Text(needle)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text",
        });
    };
    Ok(StdlibValue::Int(text::text_index_of(s, needle)))
}

pub(super) fn text_parse_int_or_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Text(s), StdlibValue::Int(fallback)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type {
            expected: "Text, Int",
        });
    };
    Ok(StdlibValue::Int(text::text_parse_int_or(s, *fallback)))
}

pub(super) fn text_replace_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Text(s), StdlibValue::Text(from), StdlibValue::Text(to)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type {
            expected: "Text, Text, Text",
        });
    };
    Ok(StdlibValue::Text(text::text_replace(s, from, to)))
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
        StdlibValue::Tuple(items) => json::Json::Array(items.iter().map(stdlib_to_json).collect()),
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

pub(super) fn numeric_narrow_to_u64(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Int(n) => Ok(StdlibValue::Result(
            numeric::narrow_i64_to_u64(*n)
                .map(|v| Box::new(StdlibValue::Int(v as i64)))
                .map_err(|e| Box::new(StdlibValue::Text(e.to_string()))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

pub(super) fn numeric_narrow_to_i16(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Int(n) => Ok(StdlibValue::Result(
            numeric::narrow_i64_to_i16(*n)
                .map(|v| Box::new(StdlibValue::Int(v as i64)))
                .map_err(|e| Box::new(StdlibValue::Text(e.to_string()))),
        )),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

pub(super) fn numeric_narrow_to_u8(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Int(n) => Ok(StdlibValue::Result(
            numeric::narrow_i64_to_u8(*n)
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

pub(super) fn numeric_min(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::min(*a, *b)))
}

pub(super) fn numeric_max(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::max(*a, *b)))
}

pub(super) fn numeric_clamp(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Int(value), StdlibValue::Int(low), StdlibValue::Int(high)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::clamp(*value, *low, *high)))
}

pub(super) fn numeric_abs_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(value), StdlibValue::Int(fallback)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::abs_or(*value, *fallback)))
}

pub(super) fn numeric_neg_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(value), StdlibValue::Int(fallback)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::neg_or(*value, *fallback)))
}

pub(super) fn numeric_add_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b), StdlibValue::Int(fallback)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::add_or(*a, *b, *fallback)))
}

pub(super) fn numeric_sub_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b), StdlibValue::Int(fallback)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::sub_or(*a, *b, *fallback)))
}

pub(super) fn numeric_mul_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b), StdlibValue::Int(fallback)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::mul_or(*a, *b, *fallback)))
}

pub(super) fn numeric_div_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Int(value), StdlibValue::Int(divisor), StdlibValue::Int(fallback)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::div_or(
        *value, *divisor, *fallback,
    )))
}

pub(super) fn numeric_rem_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let (StdlibValue::Int(value), StdlibValue::Int(divisor), StdlibValue::Int(fallback)) =
        (&args[0], &args[1], &args[2])
    else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::rem_or(
        *value, *divisor, *fallback,
    )))
}

pub(super) fn numeric_bit_and(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::bit_and(*a, *b)))
}

pub(super) fn numeric_bit_or(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::bit_or(*a, *b)))
}

pub(super) fn numeric_bit_xor(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::bit_xor(*a, *b)))
}

pub(super) fn numeric_bit_not(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Int(value) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::bit_not(*value)))
}

pub(super) fn numeric_shift_left(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(value), StdlibValue::Int(amount)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::shift_left(*value, *amount)))
}

pub(super) fn numeric_shift_right(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(value), StdlibValue::Int(amount)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::shift_right(*value, *amount)))
}

pub(super) fn numeric_shift_right_unsigned(
    args: &[StdlibValue],
) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(value), StdlibValue::Int(amount)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::shift_right_unsigned(
        *value, *amount,
    )))
}

pub(super) fn numeric_wrapping_add(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::wrapping_add(*a, *b)))
}

pub(super) fn numeric_wrapping_sub(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::wrapping_sub(*a, *b)))
}

pub(super) fn numeric_wrapping_mul(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::wrapping_mul(*a, *b)))
}

pub(super) fn numeric_wrapping_neg(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Int(value) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::wrapping_neg(*value)))
}

pub(super) fn numeric_saturating_add(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::saturating_add(*a, *b)))
}

pub(super) fn numeric_saturating_sub(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::saturating_sub(*a, *b)))
}

pub(super) fn numeric_saturating_mul(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let (StdlibValue::Int(a), StdlibValue::Int(b)) = (&args[0], &args[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::saturating_mul(*a, *b)))
}

pub(super) fn numeric_saturating_neg(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    let StdlibValue::Int(value) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(numeric::saturating_neg(*value)))
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

// ── Bytes adapters ────────────────────────────────────────────────────────
//
// All five handlers operate on `StdlibValue::Bytes(Vec<u8>)` directly; they
// do not depend on `crate::bytes::Bytes` in order to avoid a layer boundary
// that would add no semantic value here.

/// `std.bytes.length` — byte count of the buffer.
///
/// Returns `Int(n)` where `n >= 0`.
///
/// # Errors
///
/// Returns [`StdlibExecError::Message`] if the buffer length cannot be
/// represented as `i64` (requires >9 EiB; unreachable in practice but
/// handled honestly rather than truncating silently).
pub(super) fn bytes_length(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(b) => {
            let len = i64::try_from(b.len()).map_err(|_| {
                StdlibExecError::Message("byte buffer length overflows i64".to_string())
            })?;
            Ok(StdlibValue::Int(len))
        }
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
}

/// `std.bytes.at` — single byte at `index`.
///
/// Returns `Option<Int>`:
/// - `Some(v)` where `v` is the byte value in `0..=255` when `0 <= index < length`.
/// - `None` when `index` is negative or out of bounds.
pub(super) fn bytes_at(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Bytes(b) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Bytes" });
    };
    let StdlibValue::Int(idx) = args[1] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    let value = usize::try_from(idx)
        .ok()
        .and_then(|i| b.get(i).copied())
        .map(|byte| Box::new(StdlibValue::Int(i64::from(byte))));
    Ok(StdlibValue::Option(value))
}

/// `std.bytes.slice` — sub-buffer `[start..end]`.
///
/// Returns `Option<Bytes>`:
/// - `Some(Bytes)` containing the sub-buffer when `0 <= start <= end <= length`.
/// - `None` when either index is negative, `start > end`, or `end > length`.
pub(super) fn bytes_slice(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 3)?;
    let StdlibValue::Bytes(b) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Bytes" });
    };
    let StdlibValue::Int(start) = args[1] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    let StdlibValue::Int(end) = args[2] else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    // `Option::zip` + `slice::get` returns None for any out-of-range or
    // start > end combination without panicking.
    let result = usize::try_from(start)
        .ok()
        .zip(usize::try_from(end).ok())
        .and_then(|(s, e)| b.get(s..e))
        .map(|slice| Box::new(StdlibValue::Bytes(slice.to_vec())));
    Ok(StdlibValue::Option(result))
}

/// `std.bytes.concat` — concatenate two byte buffers.
///
/// Returns a new `Bytes` containing all bytes of `a` followed by all bytes of `b`.
/// Pure: neither input is mutated.
pub(super) fn bytes_concat(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::Bytes(a) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "Bytes" });
    };
    let StdlibValue::Bytes(b) = &args[1] else {
        return Err(StdlibExecError::Type { expected: "Bytes" });
    };
    let mut result = a.clone();
    result.extend_from_slice(b);
    Ok(StdlibValue::Bytes(result))
}

/// `std.bytes.empty` — predicate: is the buffer empty?
///
/// Returns `Bool(true)` when `length == 0`; `Bool(false)` otherwise.
pub(super) fn bytes_empty(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 1)?;
    match &args[0] {
        StdlibValue::Bytes(b) => Ok(StdlibValue::Bool(b.is_empty())),
        _ => Err(StdlibExecError::Type { expected: "Bytes" }),
    }
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

pub(super) fn iter_any_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Function(f) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    for item in items.clone() {
        match f(item)? {
            StdlibValue::Bool(true) => return Ok(StdlibValue::Bool(true)),
            StdlibValue::Bool(false) => {}
            _ => return Err(StdlibExecError::Type { expected: "Bool" }),
        }
    }
    Ok(StdlibValue::Bool(false))
}

pub(super) fn iter_all_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Function(f) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    for item in items.clone() {
        match f(item)? {
            StdlibValue::Bool(true) => {}
            StdlibValue::Bool(false) => return Ok(StdlibValue::Bool(false)),
            _ => return Err(StdlibExecError::Type { expected: "Bool" }),
        }
    }
    Ok(StdlibValue::Bool(true))
}

pub(super) fn iter_find_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Function(f) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    for item in items.clone() {
        match f(item.clone())? {
            StdlibValue::Bool(true) => {
                return Ok(StdlibValue::Option(Some(Box::new(item))));
            }
            StdlibValue::Bool(false) => {}
            _ => return Err(StdlibExecError::Type { expected: "Bool" }),
        }
    }
    Ok(StdlibValue::Option(None))
}

pub(super) fn iter_position_exec(args: &[StdlibValue]) -> Result<StdlibValue, StdlibExecError> {
    expect_arity(args, 2)?;
    let StdlibValue::List(items) = &args[0] else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    let StdlibValue::Function(f) = args[1] else {
        return Err(StdlibExecError::Type {
            expected: "Function",
        });
    };
    for (index, item) in items.clone().into_iter().enumerate() {
        match f(item)? {
            StdlibValue::Bool(true) => {
                let index = i64::try_from(index).map_err(|_| {
                    StdlibExecError::Message("matching index overflows i64".to_string())
                })?;
                return Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(index)))));
            }
            StdlibValue::Bool(false) => {}
            _ => return Err(StdlibExecError::Type { expected: "Bool" }),
        }
    }
    Ok(StdlibValue::Option(None))
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
