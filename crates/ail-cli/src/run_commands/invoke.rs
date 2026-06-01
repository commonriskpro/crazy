use super::*;

static SCALAR_VALUE_LAYOUT: ValueLayout = ValueLayout::Scalar;

pub(super) fn value_layout_from_wasm_descriptor(desc: &WasmTypeDescriptor) -> ValueLayout {
    match desc {
        WasmTypeDescriptor::Scalar(_) => ValueLayout::Scalar,
        WasmTypeDescriptor::Text => ValueLayout::Text,
        WasmTypeDescriptor::Bytes => ValueLayout::Bytes,
        WasmTypeDescriptor::Record { fields } => ValueLayout::Record {
            fields: fields.clone(),
        },
        WasmTypeDescriptor::Variant { tags } => ValueLayout::Variant { tags: tags.clone() },
        WasmTypeDescriptor::Tuple(elems) => ValueLayout::Tuple(
            elems
                .iter()
                .map(value_layout_from_wasm_descriptor)
                .collect(),
        ),
        WasmTypeDescriptor::List(inner) => {
            ValueLayout::List(Box::new(value_layout_from_wasm_descriptor(inner)))
        }
        WasmTypeDescriptor::Option(inner) => {
            ValueLayout::Option(Box::new(value_layout_from_wasm_descriptor(inner)))
        }
        WasmTypeDescriptor::Result { ok, err } => ValueLayout::Result {
            ok: Box::new(value_layout_from_wasm_descriptor(ok)),
            err: Box::new(value_layout_from_wasm_descriptor(err)),
        },
        WasmTypeDescriptor::Handle => ValueLayout::Handle,
    }
}

pub(super) fn runtime_value_to_json(value: &RuntimeValue) -> Value {
    match value {
        RuntimeValue::I64(v) => json!(v),
        RuntimeValue::I32(v) => json!(v),
        RuntimeValue::F64(v) => json!(v),
        RuntimeValue::Unit => Value::Null,
    }
}

fn read_text_structured_value(
    instance: &mut ail_runtime::RuntimeInstance,
    value: &StructuredValue,
) -> Result<String, CliError> {
    let StructuredValue::Text { ptr, len } = value else {
        return Err(CliError::Domain(format!(
            "typed Text value decoded as non-Text value: {value:?}"
        )));
    };

    let len = usize::try_from(*len)
        .map_err(|_| CliError::Domain(format!("typed Text value has negative length: {len}")))?;
    let bytes = instance.read_wasm_memory(*ptr, len).ok_or_else(|| {
        CliError::Domain("typed Text value points outside WASM memory".to_string())
    })?;
    String::from_utf8(bytes)
        .map_err(|e| CliError::Domain(format!("typed Text value is not valid UTF-8: {e}")))
}

fn read_bytes_structured_value(
    instance: &mut ail_runtime::RuntimeInstance,
    value: &StructuredValue,
) -> Result<Vec<u8>, CliError> {
    let StructuredValue::Bytes { ptr, len } = value else {
        return Err(CliError::Domain(format!(
            "typed Bytes value decoded as non-Bytes value: {value:?}"
        )));
    };

    let len = usize::try_from(*len)
        .map_err(|_| CliError::Domain(format!("typed Bytes value has negative length: {len}")))?;
    instance
        .read_wasm_memory(*ptr, len)
        .ok_or_else(|| CliError::Domain("typed Bytes value points outside WASM memory".to_string()))
}

fn structured_value_to_cli_result(
    instance: &mut ail_runtime::RuntimeInstance,
    layout: &ValueLayout,
    value: &StructuredValue,
) -> Result<(String, Value), CliError> {
    match layout {
        ValueLayout::Scalar => match value {
            StructuredValue::Scalar(v) => Ok((v.to_string(), json!(v))),
            StructuredValue::Float(v) => Ok((v.to_string(), json!(v))),
            StructuredValue::Unit => Ok(("()".to_string(), Value::Null)),
            other => Err(CliError::Domain(format!(
                "typed Scalar invocation returned non-scalar value: {other:?}"
            ))),
        },
        ValueLayout::Text => {
            let text = read_text_structured_value(instance, value)?;
            Ok((text.clone(), json!(text)))
        }
        ValueLayout::Bytes => {
            let bytes = read_bytes_structured_value(instance, value)?;
            let label = format!(
                "bytes[{}]",
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            Ok((label, json!(bytes)))
        }
        ValueLayout::Record { fields } => {
            let StructuredValue::Record(values) = value else {
                return Err(CliError::Domain(format!(
                    "typed Record invocation returned non-record value: {value:?}"
                )));
            };
            let mut parts = Vec::with_capacity(values.len());
            let mut object = serde_json::Map::new();
            for (idx, field) in fields.iter().enumerate() {
                let Some((actual_field, field_value)) = values.get(idx) else {
                    return Err(CliError::Domain(format!(
                        "typed Record value missing field `{field}`"
                    )));
                };
                let name = if actual_field == field {
                    field
                } else {
                    actual_field
                };
                let (label, json_value) =
                    structured_value_to_cli_result(instance, &ValueLayout::Scalar, field_value)?;
                parts.push(format!("{name}: {label}"));
                object.insert(name.clone(), json_value);
            }
            Ok((format!("{{{}}}", parts.join(", ")), Value::Object(object)))
        }
        ValueLayout::Tuple(items) => {
            let StructuredValue::List(values) = value else {
                return Err(CliError::Domain(format!(
                    "typed Tuple invocation returned non-tuple value: {value:?}"
                )));
            };
            if values.len() != items.len() {
                return Err(CliError::Domain(format!(
                    "typed Tuple value arity mismatch: expected {}, got {}",
                    items.len(),
                    values.len()
                )));
            }
            let mut labels = Vec::with_capacity(items.len());
            let mut json_values = Vec::with_capacity(items.len());
            for (layout, item) in items.iter().zip(values.iter()) {
                let (label, json_value) = structured_value_to_cli_result(instance, layout, item)?;
                labels.push(label);
                json_values.push(json_value);
            }
            Ok((
                format!("({})", labels.join(", ")),
                Value::Array(json_values),
            ))
        }
        ValueLayout::List(inner) => {
            let StructuredValue::List(values) = value else {
                return Err(CliError::Domain(format!(
                    "typed List invocation returned non-list value: {value:?}"
                )));
            };
            let mut labels = Vec::with_capacity(values.len());
            let mut json_values = Vec::with_capacity(values.len());
            for item in values {
                let (label, json_value) = structured_value_to_cli_result(instance, inner, item)?;
                labels.push(label);
                json_values.push(json_value);
            }
            Ok((
                format!("[{}]", labels.join(", ")),
                Value::Array(json_values),
            ))
        }
        ValueLayout::Option(inner) => {
            structured_variant_to_cli_result(instance, value, |tag| match tag {
                "None" => None,
                "Some" => Some(inner.as_ref()),
                _ => Some(&SCALAR_VALUE_LAYOUT),
            })
        }
        ValueLayout::Result { ok, err } => {
            structured_variant_to_cli_result(instance, value, |tag| match tag {
                "Ok" => Some(ok.as_ref()),
                "Err" => Some(err.as_ref()),
                _ => Some(&SCALAR_VALUE_LAYOUT),
            })
        }
        ValueLayout::Variant { .. } => {
            structured_variant_to_cli_result(instance, value, |_| Some(&SCALAR_VALUE_LAYOUT))
        }
        ValueLayout::Handle => match value {
            StructuredValue::Handle(id) => Ok((format!("handle({})", id.0), json!(id.0))),
            other => Err(CliError::Domain(format!(
                "typed Handle invocation returned non-handle value: {other:?}"
            ))),
        },
    }
}

fn structured_variant_to_cli_result<'a>(
    instance: &mut ail_runtime::RuntimeInstance,
    value: &StructuredValue,
    payload_layout: impl Fn(&str) -> Option<&'a ValueLayout>,
) -> Result<(String, Value), CliError> {
    let StructuredValue::Variant { tag, payload } = value else {
        return Err(CliError::Domain(format!(
            "typed Variant invocation returned non-variant value: {value:?}"
        )));
    };
    let Some(payload) = payload else {
        return Ok((tag.clone(), json!({ "tag": tag })));
    };
    let Some(layout) = payload_layout(tag) else {
        return Ok((tag.clone(), json!({ "tag": tag })));
    };
    let (payload_label, payload_json) = structured_value_to_cli_result(instance, layout, payload)?;
    Ok((
        format!("{tag}({payload_label})"),
        json!({ "tag": tag, "value": payload_json }),
    ))
}

fn typed_result_from_structured_value(
    instance: &mut ail_runtime::RuntimeInstance,
    layout: &ValueLayout,
    value: StructuredValue,
) -> Result<(String, Value), CliError> {
    structured_value_to_cli_result(instance, layout, &value)
}

pub(crate) fn invoke_export_for_cli(
    instance: &mut ail_runtime::RuntimeInstance,
    export_name: &str,
    runtime_args: &[RuntimeArg],
    export_type: Option<&WasmTypeDescriptor>,
) -> Result<(String, Value), String> {
    match export_type {
        Some(desc) if !matches!(desc, WasmTypeDescriptor::Scalar(_)) => {
            let layout = value_layout_from_wasm_descriptor(desc);
            let typed = instance
                .invoke_typed(export_name, runtime_args, &layout)
                .map_err(|e| e.to_string())?;
            typed_result_from_structured_value(instance, &layout, typed).map_err(|e| e.to_string())
        }
        Some(_) | None => {
            let value = instance
                .invoke(export_name, runtime_args)
                .map_err(|e| e.to_string())?;
            Ok((value.to_string(), runtime_value_to_json(&value)))
        }
    }
}
