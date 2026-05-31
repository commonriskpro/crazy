use super::model::*;
use super::parse::*;
use super::syntax::*;
use super::types::*;
use super::*;

mod collections;
mod control;
mod int;
mod match_expr;
mod option_result;
mod syntax_helpers;
mod text;
mod tuple;

pub(super) use collections::*;
pub(super) use control::*;
pub(super) use int::*;
pub(super) use match_expr::*;
pub(super) use option_result::*;
pub(super) use syntax_helpers::*;
pub(super) use text::*;
pub(super) use tuple::*;

#[derive(Clone, Copy)]
pub(super) enum SourceLowerDiagnostic {
    BindingShape,
    CapabilityReference,
    CollectionArity,
    FieldAccess,
    IndexExpression,
    ListLiteral,
    PipeExpression,
    RecordLiteral,
    TypeShapeMismatch,
    UnaryOperator,
    UnsupportedConstruct,
}

impl SourceLowerDiagnostic {
    fn code(self) -> &'static str {
        match self {
            SourceLowerDiagnostic::BindingShape => "AIL_SOURCE_LOWER_BINDING_SHAPE",
            SourceLowerDiagnostic::CapabilityReference => "AIL_SOURCE_LOWER_CAPABILITY_REFERENCE",
            SourceLowerDiagnostic::CollectionArity => "AIL_SOURCE_LOWER_COLLECTION_ARITY",
            SourceLowerDiagnostic::FieldAccess => "AIL_SOURCE_LOWER_FIELD_ACCESS",
            SourceLowerDiagnostic::IndexExpression => "AIL_SOURCE_LOWER_INDEX_EXPRESSION",
            SourceLowerDiagnostic::ListLiteral => "AIL_SOURCE_LOWER_LIST_LITERAL",
            SourceLowerDiagnostic::PipeExpression => "AIL_SOURCE_LOWER_PIPE_EXPRESSION",
            SourceLowerDiagnostic::RecordLiteral => "AIL_SOURCE_LOWER_RECORD_LITERAL",
            SourceLowerDiagnostic::TypeShapeMismatch => "AIL_SOURCE_LOWER_TYPE_SHAPE",
            SourceLowerDiagnostic::UnaryOperator => "AIL_SOURCE_LOWER_UNARY_OPERATOR",
            SourceLowerDiagnostic::UnsupportedConstruct => "AIL_SOURCE_LOWER_UNSUPPORTED_CONSTRUCT",
        }
    }

    fn category(self) -> &'static str {
        match self {
            SourceLowerDiagnostic::BindingShape => "source.lower.binding",
            SourceLowerDiagnostic::CapabilityReference => "source.lower.capability",
            SourceLowerDiagnostic::CollectionArity
            | SourceLowerDiagnostic::FieldAccess
            | SourceLowerDiagnostic::IndexExpression
            | SourceLowerDiagnostic::ListLiteral
            | SourceLowerDiagnostic::RecordLiteral => "source.lower.collection",
            SourceLowerDiagnostic::PipeExpression => "source.lower.pipe",
            SourceLowerDiagnostic::TypeShapeMismatch => "source.lower.type",
            SourceLowerDiagnostic::UnaryOperator => "source.lower.operator",
            SourceLowerDiagnostic::UnsupportedConstruct => "source.lower.unsupported",
        }
    }
}

pub(super) fn source_lower_error(
    line_num: usize,
    diagnostic: SourceLowerDiagnostic,
    message: impl AsRef<str>,
) -> CliError {
    CliError::ParseError(source_lower_message(
        line_num,
        diagnostic,
        None,
        message.as_ref(),
    ))
}

pub(super) fn source_lower_expr_error(
    line_num: usize,
    diagnostic: SourceLowerDiagnostic,
    expr: &str,
    construct: &'static str,
    message: impl AsRef<str>,
) -> CliError {
    CliError::ParseError(source_lower_message(
        line_num,
        diagnostic,
        Some(source_lower_descriptor(line_num, construct, expr)),
        message.as_ref(),
    ))
}

fn source_lower_message(
    line_num: usize,
    diagnostic: SourceLowerDiagnostic,
    descriptor: Option<String>,
    message: &str,
) -> String {
    let descriptor = descriptor
        .map(|descriptor| format!(" {descriptor}"))
        .unwrap_or_default();
    format!(
        "line {line_num}: [{}] category={}{}: {message}",
        diagnostic.code(),
        diagnostic.category(),
        descriptor
    )
}

fn source_lower_descriptor(line_num: usize, construct: &'static str, expr: &str) -> String {
    format!(
        "descriptor={{line={line_num},construct={construct},sourceLength={},sourceHash={:016x}}}",
        expr.chars().count(),
        source_lower_redacted_hash(expr)
    )
}

fn source_lower_redacted_hash(expr: &str) -> u64 {
    expr.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

pub(super) fn source_program_to_graph(
    program: &SourceProgram,
    change_name: impl Into<String>,
) -> Result<SemanticGraph, CliError> {
    let acl = source_program_to_acl(program, change_name.into());
    let parsed = parse_changeset(&acl)
        .map_err(|e| CliError::ParseError(format!("failed to lower AIL source to ACL: {e}")))?;
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let bridge = SimpleSnapshotBridge(SnapshotId(0));

    match ail_change::apply::apply(canonical, &mut graph, &bridge) {
        ChangeSetOutcome::Applied => Ok(graph),
        ChangeSetOutcome::RebaseRequired {
            current_snapshot_id,
        } => Err(CliError::RebaseRequired {
            current_snapshot_id: current_snapshot_id.0,
        }),
        ChangeSetOutcome::Failed { reason } => Err(CliError::Domain(format!(
            "AIL source graph materialization failed: {reason}"
        ))),
        ChangeSetOutcome::ConflictIrresolvable { reason } => Err(CliError::Domain(format!(
            "AIL source graph conflict: {reason:?}"
        ))),
    }
}

pub(super) fn source_program_to_acl(program: &SourceProgram, change_name: String) -> String {
    let constants = source_constant_names(program);
    let mut acl = format!(
        "change {change_name}\n\
author ail-source\n\
description AIL source file\n\
base 0\n"
    );

    for capability in &program.capabilities {
        acl.push_str(&format!("op create_capability id={capability}\n"));
    }
    for constant in &program.constants {
        acl.push_str(&format!(
            "op create_function id={} return={} body={}\n",
            constant.name,
            constant.return_type,
            source_expr_to_acl_body(&constant.body, &constants)
        ));
    }
    for function in &program.functions {
        acl.push_str(&format!(
            "op create_function id={} return={} body={}\n",
            function.name,
            function.return_type,
            source_expr_to_acl_body(&function.body, &constants)
        ));
        for param in &function.params {
            acl.push_str(&format!(
                "op add_param target={} name={} type={}\n",
                function.name, param.name, param.ty
            ));
        }
    }
    for test in &program.tests {
        acl.push_str(&format!(
            "op create_test id={} return={} body={}\n",
            test.name,
            test.return_type,
            source_expr_to_acl_body(&test.body, &constants)
        ));
    }
    for grant in &program.grants {
        acl.push_str(&format!(
            "op grant target={} capability={}\n",
            grant.target, grant.capability
        ));
    }
    acl.push_str("end\n");
    acl
}

pub(super) fn source_expr_to_acl_body(expr: &str, constants: &BTreeMap<String, String>) -> String {
    let expr = expr.trim();
    let Some((func, args)) = parse_source_call(expr) else {
        return source_const_reference_target(expr, constants)
            .map(|constant| format!("{constant}()"))
            .unwrap_or_else(|| expr.to_string());
    };
    if func == "let_typed" && args.len() == 5 && is_source_local_ident(&args[0]) {
        return format!(
            "let({}, {}, {})",
            args[0],
            source_expr_to_acl_body(&args[3], constants),
            source_expr_to_acl_body(&args[4], constants)
        );
    }
    if func == "record" && args.len().is_multiple_of(2) {
        let fields = args
            .chunks_exact(2)
            .map(|pair| {
                format!(
                    "{}, {}",
                    pair[0].trim(),
                    source_expr_to_acl_body(&pair[1], constants)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return format!("record({fields})");
    }
    if func == "field" && args.len() == 2 {
        return format!(
            "field({}, {})",
            source_expr_to_acl_body(&args[0], constants),
            args[1].trim()
        );
    }
    if func == "update" && args.len() == 3 {
        return format!(
            "update({}, {}, {})",
            source_expr_to_acl_body(&args[0], constants),
            args[1].trim(),
            source_expr_to_acl_body(&args[2], constants)
        );
    }
    format!(
        "{}({})",
        func,
        args.iter()
            .map(|arg| source_expr_to_acl_body(arg, constants))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(super) fn lower_source_expr(expr: &str, line_num: usize) -> Result<String, CliError> {
    let expr = expr.trim();
    if let Some(rest) = expr.strip_prefix("if ") {
        return lower_if_expr(rest, line_num);
    }
    if let Some(rest) = expr.strip_prefix("match ") {
        return lower_match_expr(rest, line_num);
    }
    if let Some(inner) = strip_wrapping_source_parens(expr) {
        return lower_source_expr(inner, line_num);
    }
    if let Some(lowered) = lower_source_typed_let_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_constructor_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_unwrap_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_option_ok_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_result_unwrap_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_option_predicate_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_result_predicate_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_tuple_accessor_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_effect_call_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_is_empty_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_int_bounds_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_eq_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_trim_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_contains_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_index_of_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_parse_int_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_byte_at_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_slice_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_replace_first_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_boundary_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_first_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_last_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_get_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_list_get_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_set_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_map_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(literal) = parse_source_record_literal(expr, line_num)? {
        if let Some(base) = literal.base {
            let mut lowered = lower_source_expr(&base, line_num)?;
            for (field, value) in literal.fields {
                lowered = format!(
                    "update({lowered}, {field}, {})",
                    lower_source_expr(&value, line_num)?
                );
            }
            return Ok(lowered);
        }
        return Ok(format!(
            "record({})",
            literal
                .fields
                .iter()
                .map(|(field, value)| {
                    lower_source_expr(value, line_num)
                        .map(|lowered_value| format!("{field}, {lowered_value}"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ));
    }
    if let Some(items) = parse_source_list_literal(expr, line_num)? {
        return Ok(format!(
            "list({})",
            items
                .iter()
                .map(|item| lower_source_expr(item, line_num))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ));
    }
    if let Some((collection, index)) = parse_source_index_expr(expr, line_num)? {
        return Ok(format!(
            "index({}, {})",
            lower_source_expr(collection, line_num)?,
            lower_source_expr(index, line_num)?
        ));
    }
    if let Some((record, field)) = parse_source_dot_field_expr(expr, line_num)? {
        return Ok(format!(
            "field({}, {field})",
            lower_source_expr(record, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "|>") {
        return lower_source_pipe_expr(left, right, line_num);
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "||") {
        return Ok(format!(
            "or({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "&&") {
        return Ok(format!(
            "and({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "==") {
        return Ok(format!(
            "eq({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "!=") {
        return Ok(format!(
            "ne({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, ">=") {
        return Ok(format!(
            "ge({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "<=") {
        return Ok(format!(
            "le({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary(expr, '>') {
        return Ok(format!(
            "gt({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary(expr, '<') {
        return Ok(format!(
            "lt({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "++") {
        return Ok(format!(
            "concat({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, op, right)) = split_top_level_source_binary_any(expr, &['+', '-']) {
        let func = match op {
            '+' => "add",
            '-' => "sub",
            _ => unreachable!("unsupported additive operator"),
        };
        return Ok(format!(
            "{}({}, {})",
            func,
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, op, right)) = split_top_level_source_binary_any(expr, &['*', '/', '%']) {
        let func = match op {
            '*' => "mul",
            '/' => "div",
            '%' => "mod",
            _ => unreachable!("unsupported multiplicative operator"),
        };
        return Ok(format!(
            "{}({}, {})",
            func,
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some(inner) = expr.strip_prefix('-') {
        if expr.parse::<i64>().is_ok() || is_source_float_literal(expr) {
            return Ok(expr.to_string());
        }
        let inner = inner.trim();
        if inner.is_empty() {
            return Err(source_lower_error(
                line_num,
                SourceLowerDiagnostic::UnaryOperator,
                "unary `-` requires an expression",
            ));
        }
        return Ok(format!("sub(0, {})", lower_source_expr(inner, line_num)?));
    }
    if let Some(inner) = expr.strip_prefix('!') {
        let inner = inner.trim();
        if inner.is_empty() || inner.starts_with('=') {
            return Err(source_lower_error(
                line_num,
                SourceLowerDiagnostic::UnaryOperator,
                "unary `!` requires an expression",
            ));
        }
        return Ok(format!("not({})", lower_source_expr(inner, line_num)?));
    }
    if let Some(construct) = unsupported_source_construct(expr) {
        return Err(source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::UnsupportedConstruct,
            expr,
            construct,
            format!("unsupported source construct `{construct}` during lowering"),
        ));
    }
    Ok(expr.to_string())
}

fn lower_source_pipe_expr(left: &str, right: &str, line_num: usize) -> Result<String, CliError> {
    let right = right.trim();
    if right.is_empty() {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::PipeExpression,
            "pipe expression requires a target function",
        ));
    }

    let piped_expr = if let Some((func, args)) = parse_source_call(right) {
        let mut piped_args = Vec::with_capacity(args.len() + 1);
        piped_args.push(left.trim().to_string());
        piped_args.extend(args.into_iter().map(|arg| arg.trim().to_string()));
        format!("{}({})", func, piped_args.join(", "))
    } else if is_source_ident(right) {
        format!("{}({})", right, left.trim())
    } else {
        return Err(source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::PipeExpression,
            right,
            "pipe",
            "pipe target requires an identifier or function call",
        ));
    };

    lower_source_expr(&piped_expr, line_num)
}

fn lower_source_effect_call_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "effect_call" {
        return Ok(None);
    }
    if args.len() < 2 || args[0].trim().is_empty() {
        return Err(source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::CapabilityReference,
            expr,
            "effect_call",
            "effect_call requires `effect_call(capability, operation, ...)`",
        ));
    }
    let capability = args[0].trim();
    let operation = args[1].trim();
    if !is_source_ident(capability) || !is_source_ident(operation) {
        return Err(source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::CapabilityReference,
            expr,
            "effect_call",
            "effect_call capability and operation must be identifiers",
        ));
    }
    let mut lowered = vec![capability.to_string(), operation.to_string()];
    lowered.extend(
        args[2..]
            .iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(Some(format!("effect_call({})", lowered.join(", "))))
}

fn lower_source_typed_let_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "let_typed" {
        return Ok(None);
    }
    if args.len() != 5 {
        return Err(source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::BindingShape,
            expr,
            "typed_let",
            "typed let lowering requires `let_typed(name, Type, line, value, next)`",
        ));
    }
    if !is_source_local_ident(&args[0]) {
        return Err(source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::BindingShape,
            expr,
            "typed_let",
            "typed let lowering requires a local binding name",
        ));
    }
    validate_source_type_annotation(&args[1]).map_err(|_| {
        source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::TypeShapeMismatch,
            expr,
            "typed_let",
            "typed let annotation has unsupported source type shape",
        )
    })?;
    let ty = normalize_source_type_name(&args[1]);
    let let_line = parse_source_let_line_marker(&args[2]).map_err(|_| {
        source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::BindingShape,
            expr,
            "typed_let",
            "typed let lowering requires a numeric source line marker",
        )
    })?;
    Ok(Some(format!(
        "let_typed({}, {ty}, {let_line}, {}, {})",
        args[0].trim(),
        lower_source_expr(&args[3], line_num)?,
        lower_source_expr(&args[4], line_num)?
    )))
}

fn unsupported_source_construct(expr: &str) -> Option<&'static str> {
    let expr = expr.trim_start();
    ["for", "while", "loop", "async", "await", "try", "return"]
        .into_iter()
        .find(|construct| {
            expr == *construct
                || expr
                    .strip_prefix(*construct)
                    .is_some_and(|rest| rest.chars().next().is_some_and(char::is_whitespace))
        })
}
