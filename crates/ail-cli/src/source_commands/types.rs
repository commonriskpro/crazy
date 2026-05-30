use super::model::*;
use super::parse::*;
use super::syntax::*;
use super::validate::*;
use super::*;

pub(super) fn validate_source_program_types(program: &SourceProgram) -> Result<(), CliError> {
    let functions = source_callable_types(program);

    for constant in &program.constants {
        let mut scope = BTreeMap::new();
        let inferred = infer_source_expr_type(&constant.body, &mut scope, &functions)
            .map_err(|err| source_error_at_line(err, constant.line_num))?;
        validate_source_type_match(&constant.return_type, &inferred, &constant.name)
            .map_err(|err| source_error_at_line(err, constant.line_num))?;
    }
    for function in &program.functions {
        let mut scope = function
            .params
            .iter()
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect::<BTreeMap<_, _>>();
        let inferred = infer_source_expr_type(&function.body, &mut scope, &functions)
            .map_err(|err| source_error_at_line(err, function.line_num))?;
        validate_source_type_match(&function.return_type, &inferred, &function.name)
            .map_err(|err| source_error_at_line(err, function.line_num))?;
    }
    for test in &program.tests {
        let mut scope = BTreeMap::new();
        let inferred = infer_source_expr_type(&test.body, &mut scope, &functions)
            .map_err(|err| source_error_at_line(err, test.line_num))?;
        validate_source_type_match(&test.return_type, &inferred, &test.name)
            .map_err(|err| source_error_at_line(err, test.line_num))?;
    }
    Ok(())
}

pub(super) fn source_callable_types(program: &SourceProgram) -> BTreeMap<&str, SourceCallable> {
    program
        .constants
        .iter()
        .map(|constant| {
            (
                constant.name.as_str(),
                SourceCallable {
                    param_types: vec![],
                    return_type: constant.return_type.clone(),
                },
            )
        })
        .chain(program.functions.iter().map(|function| {
            (
                function.name.as_str(),
                SourceCallable {
                    param_types: function
                        .params
                        .iter()
                        .map(|param| param.ty.clone())
                        .collect(),
                    return_type: function.return_type.clone(),
                },
            )
        }))
        .collect()
}

pub(super) fn source_callable_for_reference<'a>(
    functions: &'a BTreeMap<&str, SourceCallable>,
    name: &str,
) -> Option<&'a SourceCallable> {
    if let Some(callable) = functions.get(name) {
        return Some(callable);
    }
    let normalized = format!("fn.{name}");
    if let Some(callable) = functions.get(normalized.as_str()) {
        return Some(callable);
    }
    if name.contains('.') {
        return None;
    }
    let mut matches = functions
        .iter()
        .filter(|(candidate, _)| {
            candidate
                .strip_prefix("fn.")
                .and_then(|bare| bare.rsplit('.').next())
                == Some(name)
        })
        .map(|(_, callable)| callable);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

pub(super) fn infer_source_expr_type(
    expr: &str,
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let expr = expr.trim();
    if expr == "true" || expr == "false" {
        return Ok("Bool".to_string());
    }
    if is_source_string_literal(expr) {
        return Ok("Text".to_string());
    }
    if is_malformed_source_string(expr) {
        return Err(CliError::ParseError(format!(
            "malformed string literal `{expr}`"
        )));
    }
    if is_unsupported_source_numeric_literal(expr) {
        return Err(CliError::ParseError(format!(
            "unsupported source numeric literal `{expr}`"
        )));
    }
    if expr.parse::<i64>().is_ok() {
        return Ok("Int".to_string());
    }
    if is_source_float_literal(expr) {
        return Ok("Float".to_string());
    }
    if let Some(ty) = scope.get(expr) {
        return Ok(ty.clone());
    }
    if let Some(constant) = source_callable_for_reference(functions, expr)
        && constant.param_types.is_empty()
    {
        return Ok(constant.return_type.clone());
    }
    let Some((func, args)) = parse_source_call(expr) else {
        return Err(CliError::ParseError(format!(
            "unsupported source expression `{expr}`"
        )));
    };
    infer_source_call_type(&func, &args, scope, functions)
}

pub(super) fn infer_source_call_type(
    func: &str,
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    match func {
        "let" if args.len() == 3 => {
            validate_source_local_expr_name(&args[0])?;
            let value_ty = infer_source_expr_type(&args[1], scope, functions)?;
            let mut inner_scope = scope.clone();
            inner_scope.insert(args[0].clone(), value_ty);
            infer_source_expr_type(&args[2], &mut inner_scope, functions)
        }
        "let_typed" if args.len() == 5 => {
            validate_source_local_expr_name(&args[0])?;
            validate_source_type_annotation(&args[1])?;
            let let_line = parse_source_let_line_marker(&args[2])?;
            let value_ty = infer_source_expr_type(&args[3], scope, functions)
                .map_err(|err| source_error_at_line(err, let_line))?;
            validate_source_type_match(&args[1], &value_ty, &format!("let binding {}", args[0]))
                .map_err(|err| source_error_at_line(err, let_line))?;
            let mut inner_scope = scope.clone();
            inner_scope.insert(args[0].clone(), args[1].clone());
            infer_source_expr_type(&args[4], &mut inner_scope, functions)
        }
        "if" if args.len() == 3 => {
            let cond_ty = infer_source_expr_type(&args[0], scope, functions)?;
            validate_source_type_match("Bool", &cond_ty, "if condition")?;
            let then_ty = infer_source_expr_type(&args[1], scope, functions)?;
            let else_ty = infer_source_expr_type(&args[2], scope, functions)?;
            validate_source_type_match(&then_ty, &else_ty, "if branches")?;
            Ok(then_ty)
        }
        "match" if args.len() >= 3 && args.len() % 2 == 1 => {
            infer_source_match_type(args, scope, functions)
        }
        "add"
        | "sub"
        | "mul"
        | "div"
        | "mod"
        | "signed_mod"
        | "int.min"
        | "int.max"
        | "int.abs_or"
        | "int.neg_or"
        | "int.saturating_add"
        | "int.saturating_sub"
        | "int.saturating_mul"
        | "int.wrapping_add"
        | "int.wrapping_sub"
        | "int.wrapping_mul"
        | "int.bit_and"
        | "int.bit_or"
        | "int.bit_xor"
        | "int.shift_left"
        | "int.shift_right"
        | "int.shift_right_unsigned" => {
            validate_source_arg_types(func, args, scope, functions, &["Int", "Int"])?;
            Ok("Int".to_string())
        }
        "int.saturating_neg" | "int.wrapping_neg" | "int.bit_not" => {
            validate_source_arg_types(func, args, scope, functions, &["Int"])?;
            Ok("Int".to_string())
        }
        "int.clamp" | "int.add_or" | "int.sub_or" | "int.mul_or" | "int.div_or" | "int.rem_or" => {
            validate_source_arg_types(func, args, scope, functions, &["Int", "Int", "Int"])?;
            Ok("Int".to_string())
        }
        "gt" | "ge" | "lt" | "le" => {
            validate_source_arg_types(func, args, scope, functions, &["Int", "Int"])?;
            Ok("Bool".to_string())
        }
        "and" | "or" => {
            validate_source_arg_types(func, args, scope, functions, &["Bool", "Bool"])?;
            Ok("Bool".to_string())
        }
        "not" => {
            validate_source_arg_types(func, args, scope, functions, &["Bool"])?;
            Ok("Bool".to_string())
        }
        "eq" | "ne" => {
            let left = infer_source_expr_type(&args[0], scope, functions)?;
            let right = infer_source_expr_type(&args[1], scope, functions)?;
            validate_source_type_match(&left, &right, func)?;
            Ok("Bool".to_string())
        }
        "len" => {
            let arg_ty = infer_source_expr_type(&args[0], scope, functions)?;
            if arg_ty != "Text" && source_list_element_type(&arg_ty).is_none() {
                return Err(CliError::ParseError(format!(
                    "type mismatch in len argument 1: expected Text or List<Unknown>, got {arg_ty}"
                )));
            }
            Ok("Int".to_string())
        }
        "concat" => {
            validate_source_arg_types(func, args, scope, functions, &["Text", "Text"])?;
            Ok("Text".to_string())
        }
        "text.trim" => {
            validate_source_arg_types(func, args, scope, functions, &["Text"])?;
            Ok("Text".to_string())
        }
        "text.eq" => {
            validate_source_arg_types(func, args, scope, functions, &["Text", "Text"])?;
            Ok("Bool".to_string())
        }
        "text.contains" | "text.ends_with" | "text.starts_with" => {
            validate_source_arg_types(func, args, scope, functions, &["Text", "Text"])?;
            Ok("Bool".to_string())
        }
        "text.index_of" => {
            validate_source_arg_types(func, args, scope, functions, &["Text", "Text"])?;
            Ok("Int".to_string())
        }
        "text.parse_int_or" => {
            validate_source_arg_types(func, args, scope, functions, &["Text", "Int"])?;
            Ok("Int".to_string())
        }
        "text.byte_at_or" => {
            validate_source_arg_types(func, args, scope, functions, &["Text", "Int", "Int"])?;
            Ok("Int".to_string())
        }
        "text.slice" => {
            validate_source_arg_types(func, args, scope, functions, &["Text", "Int", "Int"])?;
            Ok("Text".to_string())
        }
        "text.replace_first" => {
            validate_source_arg_types(func, args, scope, functions, &["Text", "Text", "Text"])?;
            Ok("Text".to_string())
        }
        "list" => infer_source_list_type(args, scope, functions),
        "tuple" => infer_source_tuple_type(args, scope, functions),
        "set" => infer_source_set_type(args, scope, functions),
        "map" => infer_source_map_type(args, scope, functions),
        "record" => infer_source_record_type(args, scope, functions),
        "field" => infer_source_field_type(args, scope, functions),
        "update" => infer_source_update_type(args, scope, functions),
        "index" => infer_source_index_type(args, scope, functions),
        "none" => Ok("Option<Unknown>".to_string()),
        "some" => {
            let payload_ty = infer_source_expr_type(&args[0], scope, functions)?;
            Ok(format!("Option<{payload_ty}>"))
        }
        "ok" => {
            let payload_ty = infer_source_expr_type(&args[0], scope, functions)?;
            Ok(format!("Result<{payload_ty},Unknown>"))
        }
        "err" => {
            let payload_ty = infer_source_expr_type(&args[0], scope, functions)?;
            Ok(format!("Result<Unknown,{payload_ty}>"))
        }
        "print" => {
            validate_source_arg_types(func, args, scope, functions, &["Text"])?;
            Ok("Int".to_string())
        }
        "effect_call" => {
            for arg in &args[2..] {
                infer_source_expr_type(arg, scope, functions)?;
            }
            Ok("Int".to_string())
        }
        unsupported if is_untyped_source_builtin(unsupported) => {
            for arg in args {
                infer_source_expr_type(arg, scope, functions)?;
            }
            Err(CliError::ParseError(format!(
                "unsupported source builtin `{unsupported}` has no type inference"
            )))
        }
        _ => {
            if let Some(function) = source_callable_for_reference(functions, func) {
                let expected = function
                    .param_types
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                validate_source_arg_types(func, args, scope, functions, &expected)?;
                return Ok(function.return_type.clone());
            }
            Ok("Unknown".to_string())
        }
    }
}

pub(super) fn is_untyped_source_builtin(func: &str) -> bool {
    matches!(func, "Var" | "fold" | "match")
}

pub(super) fn infer_source_list_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let Some((first, rest)) = args.split_first() else {
        return Ok("List<Unknown>".to_string());
    };
    let element_ty = infer_source_expr_type(first, scope, functions)?;
    for arg in rest {
        let actual = infer_source_expr_type(arg, scope, functions)?;
        validate_source_type_match(&element_ty, &actual, "list element")?;
    }
    Ok(format!("List<{element_ty}>"))
}

pub(super) fn infer_source_tuple_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let element_types = args
        .iter()
        .map(|arg| infer_source_expr_type(arg, scope, functions))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("Tuple<{}>", element_types.join(",")))
}

pub(super) fn infer_source_set_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let Some((first, rest)) = args.split_first() else {
        return Ok("Set<Unknown>".to_string());
    };
    let element_ty = infer_source_expr_type(first, scope, functions)?;
    for arg in rest {
        let actual = infer_source_expr_type(arg, scope, functions)?;
        validate_source_type_match(&element_ty, &actual, "set element")?;
    }
    Ok(format!("Set<{element_ty}>"))
}

pub(super) fn infer_source_map_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    if args.is_empty() {
        return Ok("Map<Unknown,Unknown>".to_string());
    }
    if !args.len().is_multiple_of(2) {
        return Err(CliError::ParseError(format!(
            "function call `map` expects an even number of arguments, got {}",
            args.len()
        )));
    }

    let first_key_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let first_value_ty = infer_source_expr_type(&args[1], scope, functions)?;
    for pair in args[2..].chunks_exact(2) {
        let key_ty = infer_source_expr_type(&pair[0], scope, functions)?;
        validate_source_type_match(&first_key_ty, &key_ty, "map key")?;
        let value_ty = infer_source_expr_type(&pair[1], scope, functions)?;
        validate_source_type_match(&first_value_ty, &value_ty, "map value")?;
    }
    Ok(format!("Map<{first_key_ty},{first_value_ty}>"))
}

pub(super) fn infer_source_record_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let mut seen = BTreeSet::new();
    let mut fields = Vec::new();
    for pair in args.chunks_exact(2) {
        let field = pair[0].trim();
        validate_source_local_expr_name(field)?;
        if !seen.insert(field.to_string()) {
            return Err(CliError::ParseError(format!(
                "duplicate record field `{field}`"
            )));
        }
        let value_ty = infer_source_expr_type(&pair[1], scope, functions)?;
        fields.push(format!("{field}:{value_ty}"));
    }
    Ok(format!("Record<{}>", fields.join(",")))
}

pub(super) fn infer_source_field_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let record_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let field = args[1].trim();
    validate_source_local_expr_name(field)?;
    if record_ty == "Unknown" {
        return Ok("Unknown".to_string());
    }
    let fields = source_record_fields(&record_ty).ok_or_else(|| {
        CliError::ParseError(format!(
            "type mismatch in field argument 1: expected Record<...>, got {record_ty}"
        ))
    })?;
    fields
        .into_iter()
        .find_map(|(name, ty)| (name == field).then_some(ty.to_string()))
        .ok_or_else(|| {
            CliError::ParseError(format!("unknown record field `{field}` for {record_ty}"))
        })
}

pub(super) fn infer_source_update_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let record_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let field = args[1].trim();
    validate_source_local_expr_name(field)?;
    let fields = source_record_fields(&record_ty).ok_or_else(|| {
        CliError::ParseError(format!(
            "type mismatch in update argument 1: expected Record<...>, got {record_ty}"
        ))
    })?;
    let expected_ty = fields
        .into_iter()
        .find_map(|(name, ty)| (name == field).then_some(ty.to_string()))
        .ok_or_else(|| {
            CliError::ParseError(format!("unknown record field `{field}` for {record_ty}"))
        })?;
    let value_ty = infer_source_expr_type(&args[2], scope, functions)?;
    validate_source_type_match(&expected_ty, &value_ty, &format!("update field {field}"))?;
    Ok(record_ty)
}

pub(super) fn infer_source_index_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let collection_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let index_ty = infer_source_expr_type(&args[1], scope, functions)?;
    validate_source_type_match("Int", &index_ty, "index argument 2")?;
    if collection_ty == "Unknown" {
        return Ok("Unknown".to_string());
    }
    source_list_element_type(&collection_ty)
        .map(ToString::to_string)
        .ok_or_else(|| {
            CliError::ParseError(format!(
                "type mismatch in index argument 1: expected List<Unknown>, got {collection_ty}"
            ))
        })
}

pub(super) fn infer_source_match_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let scrutinee_ty = infer_source_expr_type(&args[0], scope, functions)?;
    validate_source_match_reachable(&args[1..])?;
    validate_source_match_exhaustive(&args[1..], &scrutinee_ty)?;
    let mut branch_ty = None;
    for pair in args[1..].chunks_exact(2) {
        validate_source_match_pattern(&pair[0])?;
        validate_source_match_pattern_type(&pair[0], &scrutinee_ty)?;
        let mut arm_scope = scope.clone();
        if let Some((binding, ty)) = source_match_pattern_binding_type(&pair[0], &scrutinee_ty)? {
            arm_scope.insert(binding.to_string(), ty);
        }
        let inferred = infer_source_expr_type(&pair[1], &mut arm_scope, functions)?;
        if let Some(expected) = &branch_ty {
            validate_source_type_match(expected, &inferred, "match arms")?;
        } else {
            branch_ty = Some(inferred);
        }
    }
    Ok(branch_ty.unwrap_or_else(|| "Unknown".to_string()))
}

pub(super) fn validate_source_arg_types(
    call: &str,
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
    expected: &[&str],
) -> Result<(), CliError> {
    for (idx, expected_ty) in expected.iter().enumerate() {
        let actual = infer_source_expr_type(&args[idx], scope, functions)?;
        validate_source_type_match(
            expected_ty,
            &actual,
            &format!("{call} argument {}", idx + 1),
        )?;
    }
    Ok(())
}

pub(super) fn validate_source_type_match(
    expected: &str,
    actual: &str,
    context: &str,
) -> Result<(), CliError> {
    if source_type_matches(expected, actual) {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "type mismatch in {context}: expected {expected}, got {actual}"
    )))
}

pub(super) fn source_type_matches(expected: &str, actual: &str) -> bool {
    let expected = expected.trim();
    let actual = actual.trim();
    if expected == actual || expected == "Unknown" || actual == "Unknown" {
        return true;
    }
    if let (Some(expected_inner), Some(actual_inner)) = (
        source_list_element_type(expected),
        source_list_element_type(actual),
    ) {
        return source_type_matches(expected_inner, actual_inner);
    }
    if let (Some(expected_items), Some(actual_items)) =
        (source_tuple_types(expected), source_tuple_types(actual))
    {
        return expected_items.len() == actual_items.len()
            && expected_items.iter().zip(actual_items.iter()).all(
                |(expected_item, actual_item)| source_type_matches(expected_item, actual_item),
            );
    }
    if let (Some(expected_inner), Some(actual_inner)) = (
        source_set_element_type(expected),
        source_set_element_type(actual),
    ) {
        return source_type_matches(expected_inner, actual_inner);
    }
    if let (Some((expected_key, expected_value)), Some((actual_key, actual_value))) =
        (source_map_types(expected), source_map_types(actual))
    {
        return source_type_matches(expected_key, actual_key)
            && source_type_matches(expected_value, actual_value);
    }
    if let (Some(expected_fields), Some(actual_fields)) =
        (source_record_fields(expected), source_record_fields(actual))
    {
        return expected_fields.len() == actual_fields.len()
            && expected_fields.iter().all(|(expected_name, expected_ty)| {
                actual_fields
                    .iter()
                    .find(|(actual_name, _)| actual_name == expected_name)
                    .is_some_and(|(_, actual_ty)| source_type_matches(expected_ty, actual_ty))
            });
    }
    if let (Some(expected_inner), Some(actual_inner)) = (
        source_option_element_type(expected),
        source_option_element_type(actual),
    ) {
        return source_type_matches(expected_inner, actual_inner);
    }
    if let (Some((expected_ok, expected_err)), Some((actual_ok, actual_err))) =
        (source_result_types(expected), source_result_types(actual))
    {
        return source_type_matches(expected_ok, actual_ok)
            && source_type_matches(expected_err, actual_err);
    }
    false
}

pub(super) fn validate_source_type_annotation(ty: &str) -> Result<(), CliError> {
    if is_supported_source_type(ty) {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "unsupported source type annotation `{ty}`"
    )))
}

pub(super) fn validate_source_let_line_marker(line: &str) -> Result<(), CliError> {
    parse_source_let_line_marker(line).map(|_| ())
}

pub(super) fn parse_source_let_line_marker(line: &str) -> Result<usize, CliError> {
    line.parse::<usize>()
        .map_err(|_| CliError::ParseError(format!("invalid typed let source line marker `{line}`")))
}

pub(super) fn source_error_at_line(err: CliError, line_num: usize) -> CliError {
    match err {
        CliError::ParseError(message) if !message.contains("line ") => {
            CliError::ParseError(format!("line {line_num}: {message}"))
        }
        other => other,
    }
}
