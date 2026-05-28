// ── ail-cli::source_commands ──────────────────────────────────────────────
//
// Minimal AIL source-language frontend.
//
// This is intentionally small, but it is not ACL: users can write `.ail`
// source files and run/test them without authoring ChangeSet ops directly.
// The current frontend lowers that source into the existing semantic graph
// pipeline so the compiler/runtime path stays real end-to-end.

use std::path::Path;

use ail_change::canonical::canonicalize_parsed;
use ail_change::model::{ChangeSetOutcome, SnapshotId};
use ail_change::parser::parse_changeset;
use ail_core::semantic_graph::SemanticGraph;

use crate::cli_helpers::SimpleSnapshotBridge;
use crate::error::CliError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceProgram {
    capabilities: Vec<String>,
    functions: Vec<SourceFunction>,
    tests: Vec<SourceTest>,
    grants: Vec<SourceGrant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceFunction {
    name: String,
    params: Vec<SourceParam>,
    return_type: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceParam {
    name: String,
    ty: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceTest {
    name: String,
    return_type: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceGrant {
    target: String,
    capability: String,
}

/// Load a `.ail` source file and lower it to a semantic graph.
pub(crate) fn load_source_graph(path: &Path) -> Result<SemanticGraph, CliError> {
    let src = std::fs::read_to_string(path).map_err(|e| {
        CliError::Domain(format!("failed to read AIL source {}: {e}", path.display()))
    })?;
    let program = parse_ail_source(&src)?;
    source_program_to_graph(&program, source_change_name(path))
}

/// Format a supported `.ail` source file into stable canonical source text.
pub(crate) fn format_ail_source(src: &str) -> Result<(String, usize), CliError> {
    let program = parse_ail_source(src)?;
    let mut out = String::new();

    for capability in &program.capabilities {
        render_source_capability(&mut out, capability);
    }
    for function in &program.functions {
        render_source_function(&mut out, function);
    }
    for test in &program.tests {
        render_source_test(&mut out, test);
    }
    for grant in &program.grants {
        render_source_grant(&mut out, grant);
    }

    Ok((
        out,
        program.capabilities.len()
            + program.functions.len()
            + program.tests.len()
            + program.grants.len(),
    ))
}

/// Parse a minimal `.ail` source file.
///
/// Supported initial syntax:
///
/// ```text
/// capability log.write
/// fn main() -> Int = add(20, 22)
/// grant main log.write
/// fn add_pair(x: Int, y: Int) -> Int = add(x, y)
/// fn with_local() -> Int {
///   let base = add(20, 20)
///   if gt(base, 40) { add(base, 2) } else { 0 }
/// }
/// test main_addition = eq(add(20, 22), 42)
/// ```
pub(crate) fn parse_ail_source(src: &str) -> Result<SourceProgram, CliError> {
    let mut capabilities = Vec::new();
    let mut functions = Vec::new();
    let mut tests = Vec::new();
    let mut grants = Vec::new();
    let statements = src
        .lines()
        .enumerate()
        .filter_map(|(idx, raw_line)| normalize_source_line(raw_line).map(|line| (idx + 1, line)))
        .collect::<Vec<_>>();

    let mut idx = 0usize;
    while idx < statements.len() {
        let (line_num, statement) = &statements[idx];

        if let Some(rest) = statement.strip_prefix("capability ") {
            capabilities.push(parse_source_capability(rest, *line_num)?);
        } else if let Some(rest) = statement.strip_prefix("fn ") {
            if rest.trim_end().ends_with('{') {
                let header = rest.trim_end().trim_end_matches('{').trim();
                let (body_lines, next_idx) = collect_braced_body(&statements, idx + 1, *line_num)?;
                let body = source_block_to_expr(&body_lines)?;
                functions.push(parse_source_function_with_body(header, *line_num, body)?);
                idx = next_idx;
                continue;
            }
            functions.push(parse_source_function(rest, *line_num)?);
        } else if let Some(rest) = statement.strip_prefix("test ") {
            tests.push(parse_source_test(rest, *line_num)?);
        } else if let Some(rest) = statement.strip_prefix("grant ") {
            grants.push(parse_source_grant(rest, *line_num)?);
        } else {
            return Err(CliError::ParseError(format!(
                "line {line_num}: expected `capability`, `fn`, `test`, or `grant`, got `{statement}`"
            )));
        }
        idx += 1;
    }

    if capabilities.is_empty() && functions.is_empty() && tests.is_empty() && grants.is_empty() {
        return Err(CliError::ParseError(
            "AIL source file has no declarations".to_string(),
        ));
    }

    Ok(SourceProgram {
        capabilities,
        functions,
        tests,
        grants,
    })
}

fn source_program_to_graph(
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

fn source_program_to_acl(program: &SourceProgram, change_name: String) -> String {
    let mut acl = format!(
        "change {change_name}\n\
author ail-source\n\
description AIL source file\n\
base 0\n"
    );

    for capability in &program.capabilities {
        acl.push_str(&format!("op create_capability id={capability}\n"));
    }
    for function in &program.functions {
        acl.push_str(&format!(
            "op create_function id={} return={} body={}\n",
            function.name, function.return_type, function.body
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
            test.name, test.return_type, test.body
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

fn render_source_capability(out: &mut String, capability: &str) {
    out.push_str(&format!("capability {capability}\n"));
}

fn render_source_function(out: &mut String, function: &SourceFunction) {
    let name = function.name.strip_prefix("fn.").unwrap_or(&function.name);
    let params = function
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let signature = format!("fn {name}({params}) -> {}", function.return_type);

    let (lets, final_expr) = source_let_chain(&function.body);
    if lets.is_empty() {
        out.push_str(&format!(
            "{signature} = {}\n",
            format_source_expr(&function.body)
        ));
        return;
    }

    out.push_str(&format!("{signature} {{\n"));
    for (name, value) in lets {
        out.push_str(&format!("  let {name} = {}\n", format_source_expr(&value)));
    }
    out.push_str(&format!("  return {}\n", format_source_expr(&final_expr)));
    out.push_str("}\n");
}

fn render_source_test(out: &mut String, test: &SourceTest) {
    let name = test.name.strip_prefix("test.").unwrap_or(&test.name);
    if test.return_type == "Bool" {
        out.push_str(&format!(
            "test {name} = {}\n",
            format_source_expr(&test.body)
        ));
    } else {
        out.push_str(&format!(
            "test {name} -> {} = {}\n",
            test.return_type,
            format_source_expr(&test.body)
        ));
    }
}

fn render_source_grant(out: &mut String, grant: &SourceGrant) {
    let target = grant.target.strip_prefix("fn.").unwrap_or(&grant.target);
    out.push_str(&format!("grant {target} {}\n", grant.capability));
}

fn source_let_chain(body: &str) -> (Vec<(String, String)>, String) {
    let mut lets = Vec::new();
    let mut current = body.trim().to_string();
    while let Some((func, args)) = parse_source_call(&current) {
        if func != "let" || args.len() != 3 || !is_source_ident(&args[0]) {
            break;
        }
        lets.push((args[0].clone(), args[1].clone()));
        current = args[2].clone();
    }
    (lets, current)
}

fn format_source_expr(expr: &str) -> String {
    let expr = expr.trim();
    let Some((func, args)) = parse_source_call(expr) else {
        return expr.to_string();
    };

    if func == "if" && args.len() == 3 {
        return format!(
            "if {} {{ {} }} else {{ {} }}",
            format_source_expr(&args[0]),
            format_source_expr(&args[1]),
            format_source_expr(&args[2])
        );
    }

    format!(
        "{}({})",
        func,
        args.iter()
            .map(|arg| format_source_expr(arg))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn parse_source_call(expr: &str) -> Option<(String, Vec<String>)> {
    let expr = expr.trim();
    let open = expr.find('(')?;
    if !expr.ends_with(')') {
        return None;
    }
    if matching_paren(expr, open)? != expr.len() - 1 {
        return None;
    }
    let func = expr[..open].trim();
    if !is_source_ident(func) {
        return None;
    }
    let inner = &expr[open + 1..expr.len() - 1];
    Some((func.to_string(), split_source_args(inner)))
}

fn split_source_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in args.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if paren_depth == 0 && brace_depth == 0 => {
                out.push(args[start..idx].trim().to_string());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let tail = args[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

fn matching_paren(s: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in s.char_indices().skip_while(|(idx, _)| *idx < open_idx) {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
}

fn is_source_ident(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn normalize_source_line(raw_line: &str) -> Option<String> {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return None;
    }
    let without_comment = strip_source_comment(trimmed);
    let statement = without_comment.trim().trim_end_matches(';').trim();
    if statement.is_empty() {
        None
    } else {
        Some(statement.to_string())
    }
}

fn strip_source_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut chars = line.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                prev_was_escape = false;
            }
            '/' if chars.peek().is_some_and(|(_, next)| *next == '/') => return &line[..idx],
            _ => {
                prev_was_escape = false;
            }
        }
    }

    line
}

fn parse_source_capability(rest: &str, line_num: usize) -> Result<String, CliError> {
    let capability = rest.trim();
    validate_source_name(capability, line_num)?;
    Ok(capability.to_string())
}

fn parse_source_function(rest: &str, line_num: usize) -> Result<SourceFunction, CliError> {
    let (name, params, return_and_body) = parse_source_function_signature(rest, line_num)?;
    let (return_type, body) = return_and_body.split_once('=').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: function declaration requires `= body`"
        ))
    })?;

    build_source_function(name, params, return_type.trim(), body.trim(), line_num)
}

fn parse_source_function_with_body(
    rest: &str,
    line_num: usize,
    body: String,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_type) = parse_source_function_signature(rest, line_num)?;
    build_source_function(name, params, return_type.trim(), body.trim(), line_num)
}

fn parse_source_function_signature(
    rest: &str,
    line_num: usize,
) -> Result<(String, Vec<SourceParam>, String), CliError> {
    let open_paren = rest.find('(').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: function declaration requires `()`"
        ))
    })?;
    let raw_name = rest[..open_paren].trim();
    validate_source_name(raw_name, line_num)?;

    let params_start = open_paren + 1;
    let close_paren = rest[params_start..].find(')').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: function declaration requires closing `)`"
        ))
    })? + params_start;
    let params = parse_source_params(&rest[params_start..close_paren], line_num)?;
    let after_params = &rest[close_paren + 1..];
    let after_arrow = after_params
        .trim_start()
        .strip_prefix("->")
        .ok_or_else(|| {
            CliError::ParseError(format!(
                "line {line_num}: function declaration requires `-> Type`"
            ))
        })?;
    Ok((
        normalize_function_name(raw_name),
        params,
        after_arrow.trim().to_string(),
    ))
}

fn build_source_function(
    name: String,
    params: Vec<SourceParam>,
    return_type: &str,
    body: &str,
    line_num: usize,
) -> Result<SourceFunction, CliError> {
    if return_type.is_empty() || body.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: function return type and body must be non-empty"
        )));
    }

    Ok(SourceFunction {
        name,
        params,
        return_type: return_type.to_string(),
        body: lower_source_expr(body, line_num)?,
    })
}

fn parse_source_params(params: &str, line_num: usize) -> Result<Vec<SourceParam>, CliError> {
    let params = params.trim();
    if params.is_empty() {
        return Ok(vec![]);
    }

    params
        .split(',')
        .map(|raw| {
            let param = raw.trim();
            let (name, ty) = param.split_once(':').ok_or_else(|| {
                CliError::ParseError(format!(
                    "line {line_num}: function parameters must use `name: Type`"
                ))
            })?;
            let name = name.trim();
            let ty = ty.trim();
            validate_source_name(name, line_num)?;
            if ty.is_empty() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: parameter `{name}` requires a type"
                )));
            }
            Ok(SourceParam {
                name: name.to_string(),
                ty: ty.to_string(),
            })
        })
        .collect()
}

fn collect_braced_body(
    statements: &[(usize, String)],
    mut idx: usize,
    opener_line: usize,
) -> Result<(Vec<(usize, String)>, usize), CliError> {
    let mut body = Vec::new();
    while idx < statements.len() {
        let (line_num, statement) = &statements[idx];
        if statement == "}" {
            return Ok((body, idx + 1));
        }
        if statement.ends_with('{') {
            return Err(CliError::ParseError(format!(
                "line {line_num}: nested source blocks are not supported yet"
            )));
        }
        body.push((*line_num, statement.clone()));
        idx += 1;
    }

    Err(CliError::ParseError(format!(
        "line {opener_line}: function block requires closing `}}`"
    )))
}

fn source_block_to_expr(lines: &[(usize, String)]) -> Result<String, CliError> {
    let Some((last_line, last_statement)) = lines.last() else {
        return Err(CliError::ParseError(
            "function block body cannot be empty".to_string(),
        ));
    };
    let mut final_expr = last_statement.as_str();
    if let Some(rest) = final_expr.strip_prefix("return ") {
        final_expr = rest.trim();
    }
    if final_expr.starts_with("let ") {
        return Err(CliError::ParseError(format!(
            "line {last_line}: function block must end with an expression or `return expression`"
        )));
    }

    let mut body = lower_source_expr(final_expr, *last_line)?;
    for (line_num, statement) in lines[..lines.len().saturating_sub(1)].iter().rev() {
        let Some(rest) = statement.strip_prefix("let ") else {
            return Err(CliError::ParseError(format!(
                "line {line_num}: only `let name = expression` statements may precede the final expression"
            )));
        };
        let (name, value) = rest.split_once('=').ok_or_else(|| {
            CliError::ParseError(format!(
                "line {line_num}: let statement requires `let name = expression`"
            ))
        })?;
        let name = name.trim();
        let value = value.trim();
        validate_source_name(name, *line_num)?;
        if value.is_empty() {
            return Err(CliError::ParseError(format!(
                "line {line_num}: let statement requires a value expression"
            )));
        }
        body = format!(
            "let({name}, {}, {body})",
            lower_source_expr(value, *line_num)?
        );
    }

    Ok(body)
}

fn lower_source_expr(expr: &str, line_num: usize) -> Result<String, CliError> {
    let expr = expr.trim();
    if let Some(rest) = expr.strip_prefix("if ") {
        return lower_if_expr(rest, line_num);
    }
    Ok(expr.to_string())
}

fn lower_if_expr(rest: &str, line_num: usize) -> Result<String, CliError> {
    let open_then = rest.find('{').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: if expression requires `{{ then }} else {{ else }}`"
        ))
    })?;
    let cond = rest[..open_then].trim();
    if cond.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: if expression requires a condition"
        )));
    }

    let then_close = matching_brace(rest, open_then).ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: if expression has unclosed then block"
        ))
    })?;
    let then_expr = rest[open_then + 1..then_close].trim();
    let after_then = rest[then_close + 1..].trim_start();
    let after_else = after_then.strip_prefix("else").ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: if expression requires an else branch"
        ))
    })?;
    let after_else = after_else.trim_start();
    if !after_else.starts_with('{') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: else branch requires `{{ expression }}`"
        )));
    }
    let else_close = matching_brace(after_else, 0).ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: if expression has unclosed else block"
        ))
    })?;
    if !after_else[else_close + 1..].trim().is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: unexpected tokens after if expression"
        )));
    }
    let else_expr = after_else[1..else_close].trim();

    if then_expr.is_empty() || else_expr.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: if branches must be non-empty expressions"
        )));
    }

    Ok(format!(
        "if({}, {}, {})",
        lower_source_expr(cond, line_num)?,
        lower_source_expr(then_expr, line_num)?,
        lower_source_expr(else_expr, line_num)?
    ))
}

fn matching_brace(s: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in s.char_indices().skip_while(|(idx, _)| *idx < open_idx) {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
}

fn parse_source_test(rest: &str, line_num: usize) -> Result<SourceTest, CliError> {
    let (head, body) = rest.split_once('=').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: test declaration requires `= body`"
        ))
    })?;
    let (raw_name, return_type) = if let Some((name, ty)) = head.split_once("->") {
        (name.trim(), ty.trim())
    } else {
        (head.trim(), "Bool")
    };
    validate_source_name(raw_name, line_num)?;

    let body = body.trim();
    if return_type.is_empty() || body.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: test return type and body must be non-empty"
        )));
    }

    Ok(SourceTest {
        name: normalize_test_name(raw_name),
        return_type: return_type.to_string(),
        body: lower_source_expr(body, line_num)?,
    })
}

fn parse_source_grant(rest: &str, line_num: usize) -> Result<SourceGrant, CliError> {
    let parts = rest.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: grant declaration requires `grant target capability`"
        )));
    }
    let target = normalize_grant_target(parts[0]);
    let capability = parts[1].to_string();
    validate_source_name(&target, line_num)?;
    validate_source_name(&capability, line_num)?;

    Ok(SourceGrant { target, capability })
}

fn normalize_grant_target(target: &str) -> String {
    if target.contains('.') {
        target.to_string()
    } else {
        normalize_function_name(target)
    }
}

fn validate_source_name(name: &str, line_num: usize) -> Result<(), CliError> {
    if name.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: declaration name cannot be empty"
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err(CliError::ParseError(format!(
            "line {line_num}: declaration name `{name}` contains unsupported characters"
        )));
    }
    Ok(())
}

fn normalize_function_name(name: &str) -> String {
    if name.starts_with("fn.") {
        name.to_string()
    } else {
        format!("fn.{name}")
    }
}

fn normalize_test_name(name: &str) -> String {
    if name.starts_with("test.") {
        name.to_string()
    } else {
        format!("test.{name}")
    }
}

fn source_change_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("source");
    let sanitized = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("source_{sanitized}")
}

#[cfg(test)]
mod tests {
    use super::{format_ail_source, parse_ail_source, source_program_to_acl};

    #[test]
    fn parses_functions_and_tests_from_ail_source() {
        let program = parse_ail_source(
            r#"
// real source, not ACL
fn main() -> Int = add(20, 22)
fn add_pair(x: Int, y: Int) -> Int = add(x, y)
fn with_local() -> Int {
  let base = add(20, 20)
  if gt(base, 40) { add(base, 2) } else { 0 }
}
test main_addition = eq(add(20, 22), 42);
"#,
        )
        .expect("source must parse");

        assert_eq!(program.functions[0].name, "fn.main");
        assert_eq!(program.functions[0].return_type, "Int");
        assert_eq!(program.functions[1].name, "fn.add_pair");
        assert_eq!(program.functions[1].params[0].name, "x");
        assert_eq!(program.functions[1].params[0].ty, "Int");
        assert_eq!(program.functions[1].params[1].name, "y");
        assert_eq!(program.functions[1].params[1].ty, "Int");
        assert_eq!(program.functions[2].name, "fn.with_local");
        assert_eq!(
            program.functions[2].body,
            "let(base, add(20, 20), if(gt(base, 40), add(base, 2), 0))"
        );
        assert_eq!(program.tests[0].name, "test.main_addition");
        assert_eq!(program.tests[0].return_type, "Bool");
    }

    #[test]
    fn lowers_source_to_acl_create_ops() {
        let program =
            parse_ail_source("fn main() -> Int = add(20, 22)\ntest add = eq(add(20, 22), 42)")
                .expect("source must parse");
        let acl = source_program_to_acl(&program, "source_main".to_string());

        assert!(acl.contains("op create_function id=fn.main return=Int body=add(20, 22)"));
        assert!(acl.contains("op create_test id=test.add return=Bool body=eq(add(20, 22), 42)"));
    }

    #[test]
    fn lowers_source_params_to_acl_add_param_ops() {
        let program =
            parse_ail_source("fn add_pair(x: Int, y: Int) -> Int = add(x, y)").expect("source");
        let acl = source_program_to_acl(&program, "source_params".to_string());

        assert!(acl.contains("op create_function id=fn.add_pair return=Int body=add(x, y)"));
        assert!(acl.contains("op add_param target=fn.add_pair name=x type=Int"));
        assert!(acl.contains("op add_param target=fn.add_pair name=y type=Int"));
    }

    #[test]
    fn lowers_source_block_let_to_acl_body_expr() {
        let program = parse_ail_source(
            r#"
fn main() -> Int {
  let base = add(20, 20)
  return add(base, 2)
}
"#,
        )
        .expect("source");
        let acl = source_program_to_acl(&program, "source_block".to_string());

        assert!(acl.contains(
            "op create_function id=fn.main return=Int body=let(base, add(20, 20), add(base, 2))"
        ));
    }

    #[test]
    fn lowers_source_if_expression_to_compiler_if_call() {
        let program = parse_ail_source(
            r#"
fn clamp_positive(x: Int) -> Int {
  if gt(x, 0) { x } else { 0 }
}
test clamp = eq(clamp_positive(-5), 0)
"#,
        )
        .expect("source");
        let acl = source_program_to_acl(&program, "source_if".to_string());

        assert!(acl.contains(
            "op create_function id=fn.clamp_positive return=Int body=if(gt(x, 0), x, 0)"
        ));
        assert!(
            acl.contains("op create_test id=test.clamp return=Bool body=eq(clamp_positive(-5), 0)")
        );
    }

    #[test]
    fn lowers_source_capabilities_and_grants_to_acl_ops() {
        let program = parse_ail_source(
            r#"
capability log.write
fn print_hello() -> Int = print("Hello from source!")
grant print_hello log.write
"#,
        )
        .expect("source capability program must parse");
        let acl = source_program_to_acl(&program, "source_capability".to_string());

        assert!(acl.contains("op create_capability id=log.write"));
        assert!(acl.contains(
            r#"op create_function id=fn.print_hello return=Int body=print("Hello from source!")"#
        ));
        assert!(acl.contains("op grant target=fn.print_hello capability=log.write"));
    }

    #[test]
    fn preserves_string_literals_with_comment_markers_and_braces() {
        let program = parse_ail_source(
            r#"
fn message() -> Text = concat("Hello, //", " {world}")
fn choose(flag: Bool) -> Text = if flag { "left } brace" } else { "right // slash" }
"#,
        )
        .expect("source string literals must parse");
        let acl = source_program_to_acl(&program, "source_strings".to_string());

        assert!(acl.contains(
            r#"op create_function id=fn.message return=Text body=concat("Hello, //", " {world}")"#
        ));
        assert!(acl.contains(
            r#"op create_function id=fn.choose return=Text body=if(flag, "left } brace", "right // slash")"#
        ));
    }

    #[test]
    fn formats_source_strings_without_treating_slashes_as_comments() {
        let src = r#"
fn message()->Text=concat("https://ail.local", " {ok}") // trailing comment
"#;
        let (formatted, item_count) = format_ail_source(src).expect("source must format");

        assert_eq!(item_count, 1);
        assert_eq!(
            formatted,
            "fn message() -> Text = concat(\"https://ail.local\", \" {ok}\")\n"
        );
    }

    #[test]
    fn formats_source_capabilities_and_grants() {
        let src = r#"
grant fn.print_hello log.write
fn print_hello()->Int=print("Hello")
capability log.write
"#;
        let (formatted, item_count) = format_ail_source(src).expect("source must format");

        assert_eq!(item_count, 3);
        assert_eq!(
            formatted,
            "capability log.write\nfn print_hello() -> Int = print(\"Hello\")\ngrant print_hello log.write\n"
        );
    }

    #[test]
    fn formats_ail_source_with_params_blocks_and_if() {
        let src = r#"
fn add_pair(x:Int,y:Int)->Int=add(x,y)
fn main()->Int{
let base=add(20,20)
return if gt(base,40){add(base,2)} else {0}
}
test addition=eq(add_pair(20,22),42)
"#;
        let (formatted, item_count) = format_ail_source(src).expect("source must format");

        assert_eq!(item_count, 3);
        assert!(formatted.contains("fn add_pair(x: Int, y: Int) -> Int = add(x, y)\n"));
        assert!(formatted.contains("fn main() -> Int {\n"));
        assert!(formatted.contains("  let base = add(20, 20)\n"));
        assert!(formatted.contains("  return if gt(base, 40) { add(base, 2) } else { 0 }\n"));
        assert!(formatted.contains("test addition = eq(add_pair(20, 22), 42)\n"));
    }
}
