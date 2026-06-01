use super::*;

pub(super) fn validate_source_type_name(ty: &str, line_num: usize) -> Result<(), CliError> {
    validate_source_type_name_at(ty, line_num, 1)
}

pub(super) fn validate_source_type_name_at(
    ty: &str,
    line_num: usize,
    column: usize,
) -> Result<(), CliError> {
    if let Err(reason) = validate_source_type_shape(ty) {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            column,
            SourceParseDiagnostic::InvalidType,
            ty,
            reason,
        ));
    }
    if is_supported_source_type(ty) {
        return Ok(());
    }
    Err(source_parse_error_for_fragment_at(
        line_num,
        column,
        SourceParseDiagnostic::InvalidType,
        ty,
        format!("unsupported source type `{ty}`"),
    ))
}

pub(super) fn is_supported_source_type(ty: &str) -> bool {
    if validate_source_type_shape(ty).is_err() {
        return false;
    }
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

fn validate_source_type_shape(ty: &str) -> Result<(), String> {
    let ty = compact_source_type_name(ty);
    validate_source_type_shape_inner(&ty)
}

fn compact_source_type_name(ty: &str) -> String {
    ty.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn validate_source_type_shape_inner(ty: &str) -> Result<(), String> {
    if ty.is_empty() {
        return Err("source type cannot be empty".to_string());
    }
    validate_source_type_angle_balance(ty)?;
    if source_primitive_type_alias(ty).is_some() {
        return Ok(());
    }

    let Some((constructor, inner)) = source_generic_type_parts(ty) else {
        return Ok(());
    };

    match constructor {
        "List" | "Set" | "Option" => {
            let parts = split_source_type_args_checked(inner, constructor, ty)?;
            expect_source_type_arity(constructor, ty, parts.len(), 1)?;
            validate_source_type_shape_inner(parts[0])
        }
        "Tuple" => {
            let parts = split_source_type_args_checked(inner, constructor, ty)?;
            for part in parts {
                validate_source_type_shape_inner(part)?;
            }
            Ok(())
        }
        "Map" | "Result" => {
            let parts = split_source_type_args_checked(inner, constructor, ty)?;
            expect_source_type_arity(constructor, ty, parts.len(), 2)?;
            validate_source_type_shape_inner(parts[0])?;
            validate_source_type_shape_inner(parts[1])
        }
        "Record" => validate_source_record_type_shape(inner, ty),
        _ => Ok(()),
    }
}

fn validate_source_type_angle_balance(ty: &str) -> Result<(), String> {
    let mut depth = 0usize;
    for ch in ty.chars() {
        match ch {
            '<' => depth += 1,
            '>' if depth == 0 => {
                return Err(format!("unbalanced angle brackets in source type `{ty}`"));
            }
            '>' => depth -= 1,
            _ => {}
        }
    }
    if depth != 0 {
        return Err(format!("unbalanced angle brackets in source type `{ty}`"));
    }
    Ok(())
}

fn source_generic_type_parts(ty: &str) -> Option<(&str, &str)> {
    let open = ty.find('<')?;
    let constructor = &ty[..open];
    let inner = ty[open + 1..].strip_suffix('>')?;
    if constructor.is_empty() {
        return None;
    }
    Some((constructor, inner))
}

fn split_source_type_args_checked<'a>(
    args: &'a str,
    constructor: &str,
    full_ty: &str,
) -> Result<Vec<&'a str>, String> {
    if args.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    let mut angle_depth = 0usize;

    for (idx, ch) in args.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' if angle_depth == 0 => {
                return Err(format!(
                    "unbalanced angle brackets in source type `{full_ty}`"
                ));
            }
            '>' => angle_depth -= 1,
            ',' if angle_depth == 0 => {
                push_source_type_arg(&mut out, args, start, idx, constructor, full_ty)?;
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if angle_depth != 0 {
        return Err(format!(
            "unbalanced angle brackets in source type `{full_ty}`"
        ));
    }
    push_source_type_arg(&mut out, args, start, args.len(), constructor, full_ty)?;
    Ok(out)
}

fn push_source_type_arg<'a>(
    out: &mut Vec<&'a str>,
    args: &'a str,
    start: usize,
    end: usize,
    constructor: &str,
    full_ty: &str,
) -> Result<(), String> {
    let part = args[start..end].trim();
    if part.is_empty() {
        return Err(format!(
            "source type `{constructor}` has empty type argument at position {} in `{full_ty}`",
            out.len() + 1
        ));
    }
    out.push(part);
    Ok(())
}

fn expect_source_type_arity(
    constructor: &str,
    full_ty: &str,
    actual: usize,
    expected: usize,
) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "source type `{constructor}` expects {expected} type argument(s), got {actual} in `{full_ty}`"
    ))
}

fn validate_source_record_type_shape(inner: &str, full_ty: &str) -> Result<(), String> {
    if inner.is_empty() {
        return Ok(());
    }
    let mut seen = BTreeSet::new();
    for raw_field in split_source_type_args_checked(inner, "Record", full_ty)? {
        let Some((field, field_ty)) = split_source_record_field(raw_field) else {
            return Err(format!(
                "source type `Record` field `{raw_field}` must use `field: Type` in `{full_ty}`"
            ));
        };
        if field.is_empty() {
            return Err(format!(
                "source type `Record` field `{raw_field}` requires a field name in `{full_ty}`"
            ));
        }
        if !is_source_ident(field) {
            return Err(format!(
                "source type `Record` field name `{field}` is invalid in `{full_ty}`"
            ));
        }
        if field_ty.is_empty() {
            return Err(format!(
                "source type `Record` field `{field}` requires a type in `{full_ty}`"
            ));
        }
        if !seen.insert(field.to_string()) {
            return Err(format!(
                "source type `Record` has duplicate field `{field}` in `{full_ty}`"
            ));
        }
        validate_source_type_shape_inner(field_ty)?;
    }
    Ok(())
}

fn split_source_record_field(field: &str) -> Option<(&str, &str)> {
    let mut angle_depth = 0usize;
    for (idx, ch) in field.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ':' if angle_depth == 0 => {
                return Some((field[..idx].trim(), field[idx + ch.len_utf8()..].trim()));
            }
            _ => {}
        }
    }
    None
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
    if args.trim().is_empty() {
        return Vec::new();
    }

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
    let compact = compact_source_type_name(ty);
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
