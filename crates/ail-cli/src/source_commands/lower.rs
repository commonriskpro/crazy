use super::model::*;
use super::parse::*;
use super::syntax::*;
use super::types::*;
use super::validate::*;
use super::*;

mod bytes;
mod collections;
mod control;
mod crypto;
mod encoding;
mod env;
mod fs;
mod int;
mod json;
mod match_expr;
mod numeric;
mod option_result;
mod path;
mod random;
mod syntax_helpers;
mod text;
mod time;
mod tuple;

pub(super) use bytes::*;
pub(super) use collections::*;
pub(super) use control::*;
pub(super) use crypto::*;
pub(super) use encoding::*;
pub(super) use env::*;
pub(super) use fs::*;
pub(super) use int::*;
pub(super) use json::*;
pub(super) use match_expr::*;
pub(super) use numeric::*;
pub(super) use option_result::*;
pub(super) use path::*;
pub(super) use random::*;
pub(super) use syntax_helpers::*;
pub(super) use text::*;
pub(super) use time::*;
pub(super) use tuple::*;

#[derive(Clone, Copy)]
pub(super) enum SourceLowerDiagnostic {
    AclMaterialization,
    BindingShape,
    BytesHelper,
    CapabilityReference,
    CollectionArity,
    ControlExpression,
    CryptoHelper,
    EncodingHelper,
    EnvHelper,
    FsHelper,
    RandomHelper,
    Expression,
    FieldAccess,
    IndexExpression,
    IntHelper,
    JsonHelper,
    NumericHelper,
    PathHelper,
    ListLiteral,
    MatchExpression,
    OptionResultHelper,
    PipeExpression,
    RecordLiteral,
    RecordDuplicateField,
    TextHelper,
    TimeHelper,
    TupleHelper,
    TypeShapeMismatch,
    UnaryOperator,
    UnsupportedConstruct,
}

impl SourceLowerDiagnostic {
    fn code(self) -> &'static str {
        match self {
            SourceLowerDiagnostic::AclMaterialization => "AIL_SOURCE_LOWER_TO_ACL",
            SourceLowerDiagnostic::BindingShape => "AIL_SOURCE_LOWER_BINDING_SHAPE",
            SourceLowerDiagnostic::BytesHelper => "AIL_SOURCE_LOWER_BYTES_HELPER",
            SourceLowerDiagnostic::CapabilityReference => "AIL_SOURCE_LOWER_CAPABILITY_REFERENCE",
            SourceLowerDiagnostic::CollectionArity => "AIL_SOURCE_LOWER_COLLECTION_ARITY",
            SourceLowerDiagnostic::ControlExpression => "AIL_SOURCE_LOWER_CONTROL_EXPRESSION",
            SourceLowerDiagnostic::CryptoHelper => "AIL_SOURCE_LOWER_CRYPTO_HELPER",
            SourceLowerDiagnostic::EncodingHelper => "AIL_SOURCE_LOWER_ENCODING_HELPER",
            SourceLowerDiagnostic::EnvHelper => "AIL_SOURCE_LOWER_ENV_HELPER",
            SourceLowerDiagnostic::FsHelper => "AIL_SOURCE_LOWER_FS_HELPER",
            SourceLowerDiagnostic::RandomHelper => "AIL_SOURCE_LOWER_RANDOM_HELPER",
            SourceLowerDiagnostic::Expression => "AIL_SOURCE_LOWER_EXPRESSION",
            SourceLowerDiagnostic::FieldAccess => "AIL_SOURCE_LOWER_FIELD_ACCESS",
            SourceLowerDiagnostic::IndexExpression => "AIL_SOURCE_LOWER_INDEX_EXPRESSION",
            SourceLowerDiagnostic::IntHelper => "AIL_SOURCE_LOWER_INT_HELPER",
            SourceLowerDiagnostic::JsonHelper => "AIL_SOURCE_LOWER_JSON_HELPER",
            SourceLowerDiagnostic::NumericHelper => "AIL_SOURCE_LOWER_NUMERIC_HELPER",
            SourceLowerDiagnostic::PathHelper => "AIL_SOURCE_LOWER_PATH_HELPER",
            SourceLowerDiagnostic::ListLiteral => "AIL_SOURCE_LOWER_LIST_LITERAL",
            SourceLowerDiagnostic::MatchExpression => "AIL_SOURCE_LOWER_MATCH_EXPRESSION",
            SourceLowerDiagnostic::OptionResultHelper => "AIL_SOURCE_LOWER_OPTION_RESULT_HELPER",
            SourceLowerDiagnostic::PipeExpression => "AIL_SOURCE_LOWER_PIPE_EXPRESSION",
            SourceLowerDiagnostic::RecordLiteral => "AIL_SOURCE_LOWER_RECORD_LITERAL",
            SourceLowerDiagnostic::RecordDuplicateField => "AIL_SOURCE_RECORD_FIELD_DUPLICATE",
            SourceLowerDiagnostic::TextHelper => "AIL_SOURCE_LOWER_TEXT_HELPER",
            SourceLowerDiagnostic::TimeHelper => "AIL_SOURCE_LOWER_TIME_HELPER",
            SourceLowerDiagnostic::TupleHelper => "AIL_SOURCE_LOWER_TUPLE_HELPER",
            SourceLowerDiagnostic::TypeShapeMismatch => "AIL_SOURCE_LOWER_TYPE_SHAPE",
            SourceLowerDiagnostic::UnaryOperator => "AIL_SOURCE_LOWER_UNARY_OPERATOR",
            SourceLowerDiagnostic::UnsupportedConstruct => "AIL_SOURCE_LOWER_UNSUPPORTED_CONSTRUCT",
        }
    }

    fn category(self) -> &'static str {
        match self {
            SourceLowerDiagnostic::AclMaterialization => "source.lower.to_acl",
            SourceLowerDiagnostic::BindingShape => "source.lower.binding",
            SourceLowerDiagnostic::BytesHelper => "source.lower.bytes",
            SourceLowerDiagnostic::CapabilityReference => "source.lower.capability",
            SourceLowerDiagnostic::ControlExpression => "source.lower.control",
            SourceLowerDiagnostic::CryptoHelper => "source.lower.crypto",
            SourceLowerDiagnostic::EncodingHelper => "source.lower.encoding",
            SourceLowerDiagnostic::EnvHelper => "source.lower.env",
            SourceLowerDiagnostic::FsHelper => "source.lower.fs",
            SourceLowerDiagnostic::RandomHelper => "source.lower.random",
            SourceLowerDiagnostic::Expression => "source.lower.expression",
            SourceLowerDiagnostic::IntHelper => "source.lower.int",
            SourceLowerDiagnostic::JsonHelper => "source.lower.json",
            SourceLowerDiagnostic::NumericHelper => "source.lower.numeric",
            SourceLowerDiagnostic::PathHelper => "source.lower.path",
            SourceLowerDiagnostic::MatchExpression => "source.lower.match",
            SourceLowerDiagnostic::OptionResultHelper => "source.lower.option_result",
            SourceLowerDiagnostic::RecordDuplicateField => "source.record.duplicate_field",
            SourceLowerDiagnostic::CollectionArity
            | SourceLowerDiagnostic::FieldAccess
            | SourceLowerDiagnostic::IndexExpression
            | SourceLowerDiagnostic::ListLiteral
            | SourceLowerDiagnostic::RecordLiteral => "source.lower.collection",
            SourceLowerDiagnostic::PipeExpression => "source.lower.pipe",
            SourceLowerDiagnostic::TextHelper => "source.lower.text",
            SourceLowerDiagnostic::TimeHelper => "source.lower.time",
            SourceLowerDiagnostic::TupleHelper => "source.lower.tuple",
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

fn source_lower_to_acl_error(error: impl std::fmt::Display) -> CliError {
    let diagnostic = SourceLowerDiagnostic::AclMaterialization;
    CliError::ParseError(format!(
        "failed to lower AIL source to ACL: {error} [{}] category={}",
        diagnostic.code(),
        diagnostic.category()
    ))
}

pub(super) fn source_program_to_graph(
    program: &SourceProgram,
    change_name: impl Into<String>,
) -> Result<SemanticGraph, CliError> {
    let acl = source_program_to_acl(program, change_name.into());
    let parsed = parse_changeset(&acl).map_err(source_lower_to_acl_error)?;
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

std::thread_local! {
    /// When set, the namespaced-helper lowering routines preserve the original
    /// source alias (e.g. `log.write`, `option.is_some`) instead of collapsing it
    /// to the shared core form. This is used exclusively to build the formatter's
    /// `source_body`; the graph/ACL `body` is always produced with this flag clear.
    static PRESERVE_FORMAT_ALIASES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether alias-preserving lowering is currently active (see [`PRESERVE_FORMAT_ALIASES`]).
pub(super) fn format_aliases_preserved() -> bool {
    PRESERVE_FORMAT_ALIASES.with(std::cell::Cell::get)
}

std::thread_local! {
    /// When set, collection-helper lowering routines preserve the original helper
    /// identity (e.g. `is_empty`, `list.length`, `list.get`) instead of collapsing
    /// it to the shared core form. This is used exclusively to build the
    /// type-checker's `type_body`; the graph/ACL `body` is always produced with this
    /// flag clear so the pinned lowering output is unaffected.
    static PRESERVE_TYPE_ALIASES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether type-alias-preserving lowering is currently active.
pub(super) fn type_aliases_preserved() -> bool {
    PRESERVE_TYPE_ALIASES.with(std::cell::Cell::get)
}

struct PreserveTypeAliasesGuard(bool);

impl PreserveTypeAliasesGuard {
    fn enable() -> Self {
        Self(PRESERVE_TYPE_ALIASES.with(|flag| flag.replace(true)))
    }
}

impl Drop for PreserveTypeAliasesGuard {
    fn drop(&mut self) {
        PRESERVE_TYPE_ALIASES.with(|flag| flag.set(self.0));
    }
}

/// Lower `expr` while preserving collection-helper identity so the type checker can
/// report diagnostics against the original helper name. The graph/ACL body produced
/// by [`lower_source_expr`] is unaffected by this entry point.
pub(super) fn lower_source_expr_for_types(
    expr: &str,
    line_num: usize,
) -> Result<String, CliError> {
    let _guard = PreserveTypeAliasesGuard::enable();
    lower_source_expr(expr, line_num)
}

std::thread_local! {
    /// When set, record-literal lowering surfaces the user-facing
    /// `AIL_SOURCE_RECORD_FIELD_DUPLICATE` diagnostic instead of the internal
    /// `AIL_SOURCE_LOWER_BINDING_SHAPE` invariant. The full source-loading pipeline
    /// (`parse_ail_source`) enables this so diagnostics surfaced to users carry the
    /// dedicated record code, while direct `lower_source_expr` callers keep the
    /// lowering-internal binding-shape diagnostic.
    static STRICT_RECORD_DIAGNOSTICS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether record-literal lowering should surface user-facing record diagnostics.
pub(super) fn strict_record_diagnostics() -> bool {
    STRICT_RECORD_DIAGNOSTICS.with(std::cell::Cell::get)
}

/// RAII guard enabling user-facing record diagnostics for the enclosing scope.
pub(super) struct StrictRecordDiagnosticsGuard(bool);

impl StrictRecordDiagnosticsGuard {
    pub(super) fn enable() -> Self {
        Self(STRICT_RECORD_DIAGNOSTICS.with(|flag| flag.replace(true)))
    }
}

impl Drop for StrictRecordDiagnosticsGuard {
    fn drop(&mut self) {
        STRICT_RECORD_DIAGNOSTICS.with(|flag| flag.set(self.0));
    }
}

/// RAII guard that enables alias-preserving lowering and restores the previous
/// state on drop, even across early returns.
struct PreserveFormatAliasesGuard(bool);

impl PreserveFormatAliasesGuard {
    fn enable() -> Self {
        Self(PRESERVE_FORMAT_ALIASES.with(|flag| flag.replace(true)))
    }
}

impl Drop for PreserveFormatAliasesGuard {
    fn drop(&mut self) {
        PRESERVE_FORMAT_ALIASES.with(|flag| flag.set(self.0));
    }
}

/// Lower `expr` while preserving namespaced helper aliases so the formatter can
/// recover the original source spelling. The graph/ACL body produced by
/// [`lower_source_expr`] is unaffected by this entry point.
pub(super) fn lower_source_expr_for_format(
    expr: &str,
    line_num: usize,
) -> Result<String, CliError> {
    let _guard = PreserveFormatAliasesGuard::enable();
    lower_source_expr(expr, line_num)
}

pub(super) fn lower_source_expr(expr: &str, line_num: usize) -> Result<String, CliError> {
    let expr = expr.trim();
    if expr == "()" {
        return Ok("unit()".to_string());
    }
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
    if let Some(lowered) = lower_source_log_write_expr(expr, line_num)? {
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
    if let Some(lowered) = lower_source_length_expr(expr, line_num)? {
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
    if let Some(lowered) = lower_source_list_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_queue_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_set_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_map_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_collection_constructor_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_bytes_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_time_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_random_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_encoding_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_crypto_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_numeric_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_json_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_path_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_env_helper_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_fs_helper_expr(expr, line_num)? {
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
    // Reject the exponentiation-style `**` operator before the multiplicative split
    // would otherwise tear it into `1` and `* 2`, reporting the whole expression.
    if split_top_level_source_binary_str(expr, "**").is_some() {
        return Err(source_expr_unsupported_error(expr));
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
    // Generic call fallback: an unrecognized `name(args)` is a plain function call
    // (user-defined or a core op already in call form). Canonicalize value-position
    // Option/Result constructors nested in its arguments (e.g. `eq(unwrap(None), 0)`)
    // without rewriting helper calls or match patterns, which the formatter relies on.
    canonicalize_source_value_constructors(expr, line_num)
}

/// Lower the arguments of an otherwise-unrecognized `name(args)` call.
///
/// An unrecognized call is either a user-defined function or a core op already in
/// call form (e.g. `add`, `eq`). Its arguments may still contain helper aliases
/// (`text_length`, `is_empty`, ...), list literals, or value-position
/// `Option`/`Result` constructors that must be lowered to their core forms so the
/// emitted ACL is valid and consistently typed. Each argument is lowered through
/// [`lower_source_expr`], which respects the active alias-preservation flags, so
/// the formatter's `source_body` keeps the original spelling while the graph body
/// collapses to the canonical core.
///
/// Structural forms (`match`/`let`) are left untouched, and the original text is
/// returned verbatim when no argument changes so existing spacing is preserved.
fn canonicalize_source_value_constructors(
    expr: &str,
    line_num: usize,
) -> Result<String, CliError> {
    let expr = expr.trim();
    if let Some(lowered) = lower_source_constructor_expr(expr, line_num)? {
        return Ok(lowered);
    }
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(expr.to_string());
    };
    if !is_source_ident(&func)
        || args.is_empty()
        || matches!(func.as_str(), "match" | "let" | "let_typed")
    {
        return Ok(expr.to_string());
    }
    let mut changed = false;
    let canonical_args = args
        .iter()
        .map(|arg| {
            let trimmed = arg.trim();
            let canonical = if format_aliases_preserved() {
                // Format mode: keep helper spellings verbatim so the formatter can
                // round-trip them. Only canonicalize nested constructors, and lower
                // list literals (the formatter reverses `list(..)` back to `[..]`).
                if trimmed.starts_with('[') {
                    lower_source_expr(arg, line_num)?
                } else {
                    canonicalize_source_value_constructors(arg, line_num)?
                }
            } else {
                // Graph/type lowering: fully lower each argument to its core form so
                // nested helper aliases (e.g. `text_length`) collapse consistently.
                lower_source_expr(arg, line_num)?
            };
            if canonical != trimmed {
                changed = true;
            }
            Ok(canonical)
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    if !changed {
        return Ok(expr.to_string());
    }
    Ok(format!("{func}({})", canonical_args.join(", ")))
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
        // Use the dedicated effect-call-shape diagnostic (message-first format) so the
        // rendered diagnostic reads `line N: effect_call ...` rather than embedding the
        // lowering code/category between the line marker and the message.
        return Err(source_error_at_line(
            source_effect_call_shape_error(),
            line_num,
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

fn lower_source_log_write_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if !matches!(func.as_str(), "log.write" | "log_write") {
        return Ok(None);
    }
    if args.len() != 1 {
        return Err(source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::Expression,
            expr,
            "log.write",
            "log.write requires `log.write(message)`",
        ));
    }
    let lowered_arg = lower_source_expr(&args[0], line_num)?;
    if format_aliases_preserved() || type_aliases_preserved() {
        // Preserve the `log.write` spelling for the formatter and the type checker
        // (so a type mismatch is reported against `log.write`, not the lowered
        // `print` effect); the graph body still collapses to `print` by default.
        return Ok(Some(format!("log.write({lowered_arg})")));
    }
    Ok(Some(format!("print({lowered_arg})")))
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
    // A structurally malformed annotation (e.g. `List<Int, Text>`) keeps the redacted
    // lowering shape diagnostic, while a well-formed but unknown type name (e.g.
    // `Mystery`) surfaces the dedicated `AIL_SOURCE_TYPE_UNSUPPORTED_ANNOTATION`.
    if validate_source_type_shape(&args[1]).is_err() {
        return Err(source_lower_expr_error(
            line_num,
            SourceLowerDiagnostic::TypeShapeMismatch,
            expr,
            "typed_let",
            "typed let annotation has unsupported source type shape",
        ));
    }
    // Propagate the dedicated annotation / line-marker diagnostics
    // (`AIL_SOURCE_TYPE_UNSUPPORTED_ANNOTATION`, `AIL_SOURCE_LET_LINE_MARKER`) instead
    // of masking them with generic lowering shape errors.
    validate_source_type_annotation(&args[1])?;
    let ty = normalize_source_type_name(&args[1]);
    let let_line = parse_source_let_line_marker(&args[2])?;
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
