use super::*;

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

pub(super) fn text_result_from_structured_value(
    instance: &mut ail_runtime::RuntimeInstance,
    value: StructuredValue,
) -> Result<String, CliError> {
    let StructuredValue::Text { ptr, len } = value else {
        return Err(CliError::Domain(format!(
            "typed Text invocation returned non-Text value: {value:?}"
        )));
    };

    let len = usize::try_from(len)
        .map_err(|_| CliError::Domain(format!("typed Text return has negative length: {len}")))?;
    let bytes = instance.read_wasm_memory(ptr, len).ok_or_else(|| {
        CliError::Domain("typed Text return points outside WASM memory".to_string())
    })?;
    String::from_utf8(bytes)
        .map_err(|e| CliError::Domain(format!("typed Text return is not valid UTF-8: {e}")))
}

pub(crate) fn invoke_export_for_cli(
    instance: &mut ail_runtime::RuntimeInstance,
    export_name: &str,
    runtime_args: &[RuntimeArg],
    export_type: Option<&WasmTypeDescriptor>,
) -> Result<(String, Value), String> {
    match export_type {
        Some(desc @ WasmTypeDescriptor::Text) => {
            let layout = value_layout_from_wasm_descriptor(desc);
            let typed = instance
                .invoke_typed(export_name, runtime_args, &layout)
                .map_err(|e| e.to_string())?;
            let text =
                text_result_from_structured_value(instance, typed).map_err(|e| e.to_string())?;
            Ok((text.clone(), json!(text)))
        }
        Some(_) => {
            let value = instance
                .invoke(export_name, runtime_args)
                .map_err(|e| e.to_string())?;
            Ok((value.to_string(), runtime_value_to_json(&value)))
        }
        None => {
            let value = instance
                .invoke(export_name, runtime_args)
                .map_err(|e| e.to_string())?;
            Ok((value.to_string(), runtime_value_to_json(&value)))
        }
    }
}
