use std::collections::BTreeMap;

use ail_compiler::{WasmScalarType, WasmTypeDescriptor, export_name};
use ail_core::semantic_graph::{NodeKind, SemanticGraph};

pub(crate) fn source_return_descriptor_for_module(
    graph: &SemanticGraph,
    module_name: &str,
) -> Option<WasmTypeDescriptor> {
    graph.nodes.iter().find_map(|node| {
        (node.kind == NodeKind::Function && node.name == module_name)
            .then(|| node.return_type.as_deref().and_then(source_type_descriptor))
            .flatten()
    })
}

pub(crate) fn source_export_type_descriptors(
    graph: &SemanticGraph,
) -> BTreeMap<String, WasmTypeDescriptor> {
    graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Function)
        .filter_map(|node| {
            let descriptor = node
                .return_type
                .as_deref()
                .and_then(source_type_descriptor)?;
            Some((export_name(&node.name), descriptor))
        })
        .collect()
}

fn source_type_descriptor(ty: &str) -> Option<WasmTypeDescriptor> {
    let ty = ty.trim();
    match ty {
        "Int" | "Bool" => Some(WasmTypeDescriptor::Scalar(WasmScalarType::I64)),
        "Float" => Some(WasmTypeDescriptor::Scalar(WasmScalarType::F64)),
        "Text" => Some(WasmTypeDescriptor::Text),
        "Bytes" => Some(WasmTypeDescriptor::Bytes),
        "Handle" => Some(WasmTypeDescriptor::Handle),
        _ => {
            let (constructor, inner) = source_generic_parts(ty)?;
            match constructor {
                "List" => source_type_descriptor(inner)
                    .map(|item| WasmTypeDescriptor::List(Box::new(item))),
                "Option" => source_type_descriptor(inner)
                    .map(|item| WasmTypeDescriptor::Option(Box::new(item))),
                "Result" => {
                    let parts = split_source_type_args(inner);
                    let [ok, err] = parts.as_slice() else {
                        return None;
                    };
                    Some(WasmTypeDescriptor::Result {
                        ok: Box::new(source_type_descriptor(ok)?),
                        err: Box::new(source_type_descriptor(err)?),
                    })
                }
                "Tuple" => split_source_type_args(inner)
                    .into_iter()
                    .map(source_type_descriptor)
                    .collect::<Option<Vec<_>>>()
                    .map(WasmTypeDescriptor::Tuple),
                "Record" => {
                    let fields = split_source_type_args(inner)
                        .into_iter()
                        .map(|field| {
                            split_source_record_field(field).map(|(name, _)| name.to_string())
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(WasmTypeDescriptor::Record { fields })
                }
                _ => None,
            }
        }
    }
}

fn source_generic_parts(ty: &str) -> Option<(&str, &str)> {
    let open = ty.find('<')?;
    let constructor = ty[..open].trim();
    let inner = ty[open + 1..].strip_suffix('>')?.trim();
    (!constructor.is_empty() && !inner.is_empty()).then_some((constructor, inner))
}

fn split_source_type_args(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let part = input[start..idx].trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let part = input[start..].trim();
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

fn split_source_record_field(field: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (idx, ch) in field.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                let name = field[..idx].trim();
                let ty = field[idx + ch.len_utf8()..].trim();
                return (!name.is_empty() && !ty.is_empty()).then_some((name, ty));
            }
            _ => {}
        }
    }
    None
}
