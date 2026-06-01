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
        return Err(source_expr_error(
            "AIL_SOURCE_EXPR_MALFORMED_STRING",
            "source.expr.literal",
            format!("malformed string literal `{expr}`"),
        ));
    }
    if is_unsupported_source_numeric_literal(expr) {
        return Err(source_expr_error(
            "AIL_SOURCE_EXPR_UNSUPPORTED_NUMERIC",
            "source.expr.literal",
            format!("unsupported source numeric literal `{expr}`"),
        ));
    }
    if expr.parse::<i64>().is_ok() {
        return Ok("Int".to_string());
    }
    if is_source_float_literal(expr) {
        return Ok("Float".to_string());
    }
    if expr == "None" {
        return Ok("Option<Unknown>".to_string());
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
        return Err(source_expr_error(
            "AIL_SOURCE_EXPR_UNSUPPORTED",
            "source.expr.unsupported",
            format!("unsupported source expression `{expr}`"),
        ));
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
        "len" | "text.len" | "text.length" | "text_length" | "list.length" | "list_length" => {
            infer_source_length_type(func, args, scope, functions)
        }
        "is_empty" | "text.is_empty" | "text_is_empty" | "list.is_empty" | "list_is_empty" => {
            infer_source_is_empty_type(func, args, scope, functions)
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
        "list.get" | "list_get" => infer_source_list_get_type(args, scope, functions),
        "list.push" | "list_push" => infer_source_list_push_type(args, scope, functions),
        "list.concat" | "list_concat" => infer_source_list_concat_type(args, scope, functions),
        "queue.push_back" | "queue_push_back" => {
            infer_source_queue_push_back_type(args, scope, functions)
        }
        "queue.pop_front" | "queue_pop_front" => {
            infer_source_queue_pop_front_type(args, scope, functions)
        }
        "queue.peek_front" | "queue_peek_front" => {
            infer_source_queue_peek_front_type(args, scope, functions)
        }
        "queue.length" | "queue_length" => infer_source_queue_length_type(args, scope, functions),
        "queue.is_empty" | "queue_is_empty" => {
            infer_source_queue_is_empty_type(args, scope, functions)
        }
        "tuple" => infer_source_tuple_type(args, scope, functions),
        "tuple.length" | "tuple_length" => infer_source_tuple_length_type(args, scope, functions),
        "tuple.get" | "tuple_get" => infer_source_tuple_get_type(args, scope, functions),
        "tuple.first" | "tuple_first" => {
            infer_source_tuple_positional_type(args, scope, functions, 0, func)
        }
        "tuple.second" | "tuple_second" => {
            infer_source_tuple_positional_type(args, scope, functions, 1, func)
        }
        "unwrap_or" | "option.unwrap_or" | "option_unwrap_or" => {
            infer_source_option_unwrap_or_type(args, scope, functions)
        }
        "ok_or" | "option.ok_or" | "option_ok_or" => {
            infer_source_option_ok_or_type(args, scope, functions)
        }
        "result.unwrap_or" | "result_unwrap_or" => {
            infer_source_result_unwrap_or_type(args, scope, functions)
        }
        "is_some" | "is_none" | "option.is_some" | "option_is_some" | "option.is_none"
        | "option_is_none" => infer_source_option_predicate_type(args, scope, functions, func),
        "is_ok" | "is_err" | "result.is_ok" | "result_is_ok" | "result.is_err"
        | "result_is_err" => infer_source_result_predicate_type(args, scope, functions, func),
        "set" => infer_source_set_type(args, scope, functions),
        "set.contains" | "set_contains" => infer_source_set_contains_type(args, scope, functions),
        "set.length" | "set_length" => infer_source_set_length_type(args, scope, functions),
        "set.insert" | "set_insert" => infer_source_set_insert_type(args, scope, functions),
        "map" => infer_source_map_type(args, scope, functions),
        "map.get" | "map_get" => infer_source_map_get_type(args, scope, functions),
        "map.contains_key" | "map_contains_key" => {
            infer_source_map_contains_key_type(args, scope, functions)
        }
        "map.length" | "map_length" => infer_source_map_length_type(args, scope, functions),
        "map.insert" | "map_insert" => infer_source_map_insert_type(args, scope, functions),
        "record" => infer_source_record_type(args, scope, functions),
        "field" => infer_source_field_type(args, scope, functions),
        "update" => infer_source_update_type(args, scope, functions),
        "index" => infer_source_index_type(args, scope, functions),
        "none" | "None" => Ok("Option<Unknown>".to_string()),
        "some" | "Some" => {
            let payload_ty = infer_source_expr_type(&args[0], scope, functions)?;
            Ok(format!("Option<{payload_ty}>"))
        }
        "ok" | "Ok" => {
            let payload_ty = infer_source_expr_type(&args[0], scope, functions)?;
            Ok(format!("Result<{payload_ty},Unknown>"))
        }
        "err" | "Err" => {
            let payload_ty = infer_source_expr_type(&args[0], scope, functions)?;
            Ok(format!("Result<Unknown,{payload_ty}>"))
        }
        "print" | "log.write" | "log_write" => {
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
            Err(source_builtin_untyped_error(unsupported))
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

pub(super) fn infer_source_is_empty_type(
    func: &str,
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let arg_ty = infer_source_expr_type(&args[0], scope, functions)?;
    match func {
        "text.is_empty" | "text_is_empty" => {
            validate_source_type_match("Text", &arg_ty, &format!("{func} argument 1"))?;
        }
        "list.is_empty" | "list_is_empty" => {
            require_source_list_type(&arg_ty, &format!("{func} argument 1"))?;
        }
        _ if arg_ty != "Text" && source_list_element_type(&arg_ty).is_none() => {
            return Err(source_type_shape_mismatch_error(
                &format!("{func} argument 1"),
                "Text or List<Unknown>",
                &arg_ty,
            ));
        }
        _ => {}
    }
    Ok("Bool".to_string())
}

pub(super) fn infer_source_length_type(
    func: &str,
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let arg_ty = infer_source_expr_type(&args[0], scope, functions)?;
    match func {
        "text.len" | "text.length" | "text_length" => {
            validate_source_type_match("Text", &arg_ty, &format!("{func} argument 1"))?;
        }
        "list.length" | "list_length" => {
            require_source_list_type(&arg_ty, &format!("{func} argument 1"))?;
        }
        _ if arg_ty != "Text" && source_list_element_type(&arg_ty).is_none() => {
            return Err(source_type_shape_mismatch_error(
                &format!("{func} argument 1"),
                "Text or List<Unknown>",
                &arg_ty,
            ));
        }
        _ => {}
    }
    Ok("Int".to_string())
}

pub(super) fn infer_source_list_push_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let list_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let value_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let expected = require_source_list_type(&list_ty, "list.push argument 1")?.unwrap_or("Unknown");
    validate_source_type_match(expected, &value_ty, "list.push argument 2")?;
    Ok(list_ty)
}

pub(super) fn infer_source_list_concat_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let left_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let right_ty = infer_source_expr_type(&args[1], scope, functions)?;
    require_source_list_type(&left_ty, "list.concat argument 1")?;
    require_source_list_type(&right_ty, "list.concat argument 2")?;
    validate_source_type_match(&left_ty, &right_ty, "list.concat argument 2")?;
    Ok(left_ty)
}

pub(super) fn infer_source_queue_push_back_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let queue_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let value_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let expected =
        require_source_list_type(&queue_ty, "queue.push_back argument 1")?.unwrap_or("Unknown");
    validate_source_type_match(expected, &value_ty, "queue.push_back argument 2")?;
    Ok(queue_ty)
}

pub(super) fn infer_source_queue_pop_front_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let queue_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let item_ty =
        require_source_list_type(&queue_ty, "queue.pop_front argument 1")?.unwrap_or("Unknown");
    Ok(format!("Option<Tuple<{item_ty},List<{item_ty}>>>"))
}

pub(super) fn infer_source_queue_peek_front_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let queue_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let item_ty =
        require_source_list_type(&queue_ty, "queue.peek_front argument 1")?.unwrap_or("Unknown");
    Ok(format!("Option<{item_ty}>"))
}

pub(super) fn infer_source_queue_length_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let queue_ty = infer_source_expr_type(&args[0], scope, functions)?;
    require_source_list_type(&queue_ty, "queue.length argument 1")?;
    Ok("Int".to_string())
}

pub(super) fn infer_source_queue_is_empty_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let queue_ty = infer_source_expr_type(&args[0], scope, functions)?;
    require_source_list_type(&queue_ty, "queue.is_empty argument 1")?;
    Ok("Bool".to_string())
}

fn require_source_list_type<'a>(ty: &'a str, context: &str) -> Result<Option<&'a str>, CliError> {
    if ty == "Unknown" {
        return Ok(None);
    }
    source_list_element_type(ty)
        .map(Some)
        .ok_or_else(|| source_type_shape_mismatch_error(context, "List<Unknown>", ty))
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

pub(super) fn infer_source_tuple_length_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let tuple_ty = infer_source_expr_type(&args[0], scope, functions)?;
    require_source_tuple_type(&tuple_ty, "tuple.length argument 1")?;
    Ok("Int".to_string())
}

pub(super) fn infer_source_tuple_get_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let tuple_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let items = require_source_tuple_type(&tuple_ty, "tuple.get argument 1")?;
    let index_ty = infer_source_expr_type(&args[1], scope, functions)?;
    validate_source_type_match("Int", &index_ty, "tuple.get argument 2")?;

    let item_ty = args[1]
        .parse::<i64>()
        .ok()
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| items.and_then(|items| items.get(index).copied()))
        .unwrap_or("Unknown");
    Ok(format!("Option<{item_ty}>"))
}

pub(super) fn infer_source_tuple_positional_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
    index: usize,
    context: &str,
) -> Result<String, CliError> {
    let tuple_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let items = require_source_tuple_type(&tuple_ty, &format!("{context} argument 1"))?;
    let item_ty = items
        .and_then(|items| items.get(index).copied())
        .unwrap_or("Unknown");
    Ok(format!("Option<{item_ty}>"))
}

fn require_source_tuple_type<'a>(
    ty: &'a str,
    context: &str,
) -> Result<Option<Vec<&'a str>>, CliError> {
    if ty == "Unknown" {
        return Ok(None);
    }
    source_tuple_types(ty)
        .map(Some)
        .ok_or_else(|| source_type_shape_mismatch_error(context, "Tuple<Unknown>", ty))
}

pub(super) fn infer_source_option_unwrap_or_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let option_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let fallback_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let Some(inner_ty) = require_source_option_type(&option_ty, "option.unwrap_or argument 1")?
    else {
        return Ok(fallback_ty);
    };
    validate_source_type_match(inner_ty, &fallback_ty, "option.unwrap_or argument 2")?;
    Ok(if inner_ty == "Unknown" {
        fallback_ty
    } else {
        inner_ty.to_string()
    })
}

pub(super) fn infer_source_option_ok_or_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let option_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let err_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let ok_ty =
        require_source_option_type(&option_ty, "option.ok_or argument 1")?.unwrap_or("Unknown");
    Ok(format!("Result<{ok_ty},{err_ty}>"))
}

pub(super) fn infer_source_result_unwrap_or_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let result_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let fallback_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let Some((ok_ty, _err_ty)) =
        require_source_result_type(&result_ty, "result.unwrap_or argument 1")?
    else {
        return Ok(fallback_ty);
    };
    validate_source_type_match(ok_ty, &fallback_ty, "result.unwrap_or argument 2")?;
    Ok(if ok_ty == "Unknown" {
        fallback_ty
    } else {
        ok_ty.to_string()
    })
}

pub(super) fn infer_source_option_predicate_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
    context: &str,
) -> Result<String, CliError> {
    let option_ty = infer_source_expr_type(&args[0], scope, functions)?;
    require_source_option_type(&option_ty, &format!("{context} argument 1"))?;
    Ok("Bool".to_string())
}

pub(super) fn infer_source_result_predicate_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
    context: &str,
) -> Result<String, CliError> {
    let result_ty = infer_source_expr_type(&args[0], scope, functions)?;
    require_source_result_type(&result_ty, &format!("{context} argument 1"))?;
    Ok("Bool".to_string())
}

fn require_source_option_type<'a>(ty: &'a str, context: &str) -> Result<Option<&'a str>, CliError> {
    if ty == "Unknown" {
        return Ok(None);
    }
    source_option_element_type(ty)
        .map(Some)
        .ok_or_else(|| source_type_shape_mismatch_error(context, "Option<Unknown>", ty))
}

fn require_source_result_type<'a>(
    ty: &'a str,
    context: &str,
) -> Result<Option<(&'a str, &'a str)>, CliError> {
    if ty == "Unknown" {
        return Ok(None);
    }
    source_result_types(ty)
        .map(Some)
        .ok_or_else(|| source_type_shape_mismatch_error(context, "Result<Unknown,Unknown>", ty))
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

pub(super) fn infer_source_set_contains_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let set_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let element_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let expected =
        require_source_set_type(&set_ty, "set.contains argument 1")?.unwrap_or("Unknown");
    validate_source_type_match(expected, &element_ty, "set.contains argument 2")?;
    Ok("Bool".to_string())
}

pub(super) fn infer_source_set_length_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let set_ty = infer_source_expr_type(&args[0], scope, functions)?;
    require_source_set_type(&set_ty, "set.length argument 1")?;
    Ok("Int".to_string())
}

pub(super) fn infer_source_set_insert_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let set_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let element_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let expected = require_source_set_type(&set_ty, "set.insert argument 1")?.unwrap_or("Unknown");
    validate_source_type_match(expected, &element_ty, "set.insert argument 2")?;
    Ok(set_ty)
}

fn require_source_set_type<'a>(ty: &'a str, context: &str) -> Result<Option<&'a str>, CliError> {
    if ty == "Unknown" {
        return Ok(None);
    }
    source_set_element_type(ty)
        .map(Some)
        .ok_or_else(|| source_type_shape_mismatch_error(context, "Set<Unknown>", ty))
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
        return Err(source_map_arity_error(args.len()));
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

pub(super) fn infer_source_map_get_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let map_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let key_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let value_ty =
        require_source_map_text_key_type(&map_ty, "map.get argument 1")?.unwrap_or("Unknown");
    validate_source_type_match("Text", &key_ty, "map.get argument 2")?;
    Ok(format!("Option<{value_ty}>"))
}

pub(super) fn infer_source_map_contains_key_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let map_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let key_ty = infer_source_expr_type(&args[1], scope, functions)?;
    require_source_map_text_key_type(&map_ty, "map.contains_key argument 1")?;
    validate_source_type_match("Text", &key_ty, "map.contains_key argument 2")?;
    Ok("Bool".to_string())
}

pub(super) fn infer_source_map_length_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let map_ty = infer_source_expr_type(&args[0], scope, functions)?;
    require_source_map_text_key_type(&map_ty, "map.length argument 1")?;
    Ok("Int".to_string())
}

pub(super) fn infer_source_map_insert_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let map_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let key_ty = infer_source_expr_type(&args[1], scope, functions)?;
    let value_ty = infer_source_expr_type(&args[2], scope, functions)?;
    let expected_value_ty =
        require_source_map_text_key_type(&map_ty, "map.insert argument 1")?.unwrap_or("Unknown");
    validate_source_type_match("Text", &key_ty, "map.insert argument 2")?;
    validate_source_type_match(expected_value_ty, &value_ty, "map.insert argument 3")?;
    Ok(map_ty)
}

fn require_source_map_text_key_type<'a>(
    ty: &'a str,
    context: &str,
) -> Result<Option<&'a str>, CliError> {
    if ty == "Unknown" {
        return Ok(None);
    }
    let Some((key_ty, value_ty)) = source_map_types(ty) else {
        return Err(source_type_shape_mismatch_error(
            context,
            "Map<Text,Unknown>",
            ty,
        ));
    };
    if !source_type_matches("Text", key_ty) {
        return Err(source_type_shape_mismatch_error(
            context,
            "Map<Text,Unknown>",
            ty,
        ));
    }
    Ok(Some(value_ty))
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
            return Err(source_record_duplicate_field_error(field));
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
        source_type_shape_mismatch_error("field argument 1", "Record<...>", &record_ty)
    })?;
    fields
        .into_iter()
        .find_map(|(name, ty)| (name == field).then_some(ty.to_string()))
        .ok_or_else(|| source_record_field_error(field, &record_ty))
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
        source_type_shape_mismatch_error("update argument 1", "Record<...>", &record_ty)
    })?;
    let expected_ty = fields
        .into_iter()
        .find_map(|(name, ty)| (name == field).then_some(ty.to_string()))
        .ok_or_else(|| source_record_field_error(field, &record_ty))?;
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
            source_type_shape_mismatch_error("index argument 1", "List<Unknown>", &collection_ty)
        })
}

pub(super) fn infer_source_list_get_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let collection_ty = infer_source_expr_type(&args[0], scope, functions)?;
    let index_ty = infer_source_expr_type(&args[1], scope, functions)?;
    validate_source_type_match("Int", &index_ty, "list.get argument 2")?;
    if collection_ty == "Unknown" {
        return Ok("Option<Unknown>".to_string());
    }
    let element_ty = source_list_element_type(&collection_ty).ok_or_else(|| {
        source_type_shape_mismatch_error("list.get argument 1", "List<Unknown>", &collection_ty)
    })?;
    Ok(format!("Option<{element_ty}>"))
}

pub(super) fn infer_source_match_type(
    args: &[String],
    scope: &mut BTreeMap<String, String>,
    functions: &BTreeMap<&str, SourceCallable>,
) -> Result<String, CliError> {
    let scrutinee_ty = infer_source_expr_type(&args[0], scope, functions)?;
    for pattern in args[1..].iter().step_by(2) {
        validate_source_match_pattern(pattern)?;
    }
    validate_source_match_reachable(&args[1..])?;
    validate_source_match_exhaustive(&args[1..], &scrutinee_ty)?;
    let mut branch_ty = None;
    for pair in args[1..].chunks_exact(2) {
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
    Err(source_type_mismatch_error(context, expected, actual))
}

fn source_expr_error(code: &str, category: &str, message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!("{} [{code}] category={category}", message.as_ref()))
}

fn source_record_field_error(field: &str, record_ty: &str) -> CliError {
    source_expr_error(
        "AIL_SOURCE_RECORD_FIELD_UNKNOWN",
        "source.record.field",
        format!("unknown record field `{field}` for {record_ty}"),
    )
}

fn source_record_duplicate_field_error(field: &str) -> CliError {
    source_expr_error(
        "AIL_SOURCE_RECORD_FIELD_DUPLICATE",
        "source.record.duplicate_field",
        format!("duplicate record field `{field}`"),
    )
}

fn source_map_arity_error(actual: usize) -> CliError {
    source_expr_error(
        "AIL_SOURCE_MAP_ARITY",
        "source.map.arity",
        format!("function call `map` expects an even number of arguments, got {actual}"),
    )
}

fn source_builtin_untyped_error(func: &str) -> CliError {
    source_expr_error(
        "AIL_SOURCE_BUILTIN_UNTYPED",
        "source.builtin.untyped",
        format!("unsupported source builtin `{func}` has no type inference"),
    )
}

fn source_type_mismatch_error(context: &str, expected: &str, actual: &str) -> CliError {
    source_type_error(
        "AIL_SOURCE_TYPE_MISMATCH",
        "source.type.mismatch",
        format!("type mismatch in {context}: expected {expected}, got {actual}"),
    )
}

fn source_type_shape_mismatch_error(context: &str, expected: &str, actual: &str) -> CliError {
    source_type_error(
        "AIL_SOURCE_TYPE_SHAPE_MISMATCH",
        "source.type.shape",
        format!("type mismatch in {context}: expected {expected}, got {actual}"),
    )
}

fn source_type_error(code: &str, category: &str, message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!("{} [{code}] category={category}", message.as_ref()))
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
