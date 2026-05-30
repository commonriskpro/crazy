use super::*;

pub(super) fn validate_source_type_name(ty: &str, line_num: usize) -> Result<(), CliError> {
    if is_supported_source_type(ty) {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "line {line_num}: unsupported source type `{ty}`"
    )))
}

pub(super) fn is_supported_source_type(ty: &str) -> bool {
    let ty = normalize_source_type_name(ty);
    let ty = ty.as_str();
    source_primitive_type_alias(ty).is_some()
        || source_list_element_type(ty).is_some_and(is_supported_source_type)
        || source_tuple_types(ty)
            .is_some_and(|items| items.into_iter().all(is_supported_source_type))
        || source_set_element_type(ty).is_some_and(is_supported_source_type)
        || source_map_types(ty).is_some_and(|(key_ty, value_ty)| {
            is_supported_source_type(key_ty) && is_supported_source_type(value_ty)
        })
        || source_record_fields(ty).is_some_and(|fields| {
            fields.into_iter().all(|(field, field_ty)| {
                is_source_ident(field) && is_supported_source_type(field_ty)
            })
        })
        || source_option_element_type(ty).is_some_and(is_supported_source_type)
        || source_result_types(ty).is_some_and(|(ok_ty, err_ty)| {
            is_supported_source_type(ok_ty) && is_supported_source_type(err_ty)
        })
}

pub(super) fn source_primitive_type_alias(ty: &str) -> Option<&'static str> {
    match ty.trim() {
        "Int" | "int" | "i32" | "i64" => Some("Int"),
        "Bool" | "bool" => Some("Bool"),
        "Text" | "String" | "str" => Some("Text"),
        "Float" | "float" | "f64" => Some("Float"),
        _ => None,
    }
}

pub(super) fn source_list_element_type(ty: &str) -> Option<&str> {
    let inner = ty.trim().strip_prefix("List<")?.strip_suffix('>')?.trim();
    (!inner.is_empty()).then_some(inner)
}

pub(super) fn source_tuple_types(ty: &str) -> Option<Vec<&str>> {
    let inner = ty.trim().strip_prefix("Tuple<")?.strip_suffix('>')?.trim();
    Some(split_source_type_args(inner))
}

pub(super) fn source_set_element_type(ty: &str) -> Option<&str> {
    let inner = ty.trim().strip_prefix("Set<")?.strip_suffix('>')?.trim();
    (!inner.is_empty()).then_some(inner)
}

pub(super) fn source_map_types(ty: &str) -> Option<(&str, &str)> {
    let inner = ty.trim().strip_prefix("Map<")?.strip_suffix('>')?.trim();
    let parts = split_source_type_args(inner);
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0], parts[1]))
}

pub(super) fn source_record_fields(ty: &str) -> Option<Vec<(&str, &str)>> {
    let inner = ty.trim().strip_prefix("Record<")?.strip_suffix('>')?.trim();
    let mut seen = BTreeSet::new();
    let mut fields = Vec::new();
    for part in split_source_type_args(inner) {
        let (field, field_ty) = part.split_once(':')?;
        let field = field.trim();
        let field_ty = field_ty.trim();
        if field.is_empty()
            || field_ty.is_empty()
            || !is_source_ident(field)
            || !seen.insert(field.to_string())
        {
            return None;
        }
        fields.push((field, field_ty));
    }
    Some(fields)
}

pub(super) fn source_option_element_type(ty: &str) -> Option<&str> {
    let inner = ty.trim().strip_prefix("Option<")?.strip_suffix('>')?.trim();
    (!inner.is_empty()).then_some(inner)
}

pub(super) fn source_result_types(ty: &str) -> Option<(&str, &str)> {
    let inner = ty.trim().strip_prefix("Result<")?.strip_suffix('>')?.trim();
    let parts = split_source_type_args(inner);
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0], parts[1]))
}

pub(super) fn split_source_type_args(args: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut angle_depth = 0usize;

    for (idx, ch) in args.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ',' if angle_depth == 0 => {
                let part = args[start..idx].trim();
                if !part.is_empty() {
                    out.push(part);
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = args[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

pub(super) fn split_source_param_list(params: &str) -> Vec<&str> {
    split_source_top_level_commas(params)
}

pub(super) fn normalize_source_type_name(ty: &str) -> String {
    let compact = ty
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    normalize_source_type_aliases(&compact)
}

pub(super) fn normalize_source_type_aliases(ty: &str) -> String {
    if let Some(alias) = source_primitive_type_alias(ty) {
        return alias.to_string();
    }
    if let Some(inner) = source_list_element_type(ty) {
        return format!("List<{}>", normalize_source_type_aliases(inner));
    }
    if let Some(items) = source_tuple_types(ty) {
        return format!(
            "Tuple<{}>",
            items
                .iter()
                .map(|item| normalize_source_type_aliases(item))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    if let Some(inner) = source_set_element_type(ty) {
        return format!("Set<{}>", normalize_source_type_aliases(inner));
    }
    if let Some((key_ty, value_ty)) = source_map_types(ty) {
        return format!(
            "Map<{},{}>",
            normalize_source_type_aliases(key_ty),
            normalize_source_type_aliases(value_ty)
        );
    }
    if let Some(fields) = source_record_fields(ty) {
        return format!(
            "Record<{}>",
            fields
                .iter()
                .map(|(field, field_ty)| {
                    format!("{field}:{}", normalize_source_type_aliases(field_ty))
                })
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    if let Some(inner) = source_option_element_type(ty) {
        return format!("Option<{}>", normalize_source_type_aliases(inner));
    }
    if let Some((ok_ty, err_ty)) = source_result_types(ty) {
        return format!(
            "Result<{},{}>",
            normalize_source_type_aliases(ok_ty),
            normalize_source_type_aliases(err_ty)
        );
    }
    ty.to_string()
}
