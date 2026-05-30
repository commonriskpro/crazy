use super::lower::*;
use super::model::*;
use super::syntax::*;
use super::types::*;
use super::validate::*;
use super::*;

/// Parse a minimal `.ail` source file.
///
/// Supported initial syntax:
///
/// ```text
/// module app
/// use "./math.ail"
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
    let mut module = None;
    let mut imports = Vec::new();
    let mut capabilities = Vec::new();
    let mut constants = Vec::new();
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

        if let Some(rest) = statement.strip_prefix("module ") {
            if module.is_some() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: module declaration may appear only once"
                )));
            }
            module = Some(parse_source_module(rest, *line_num)?);
        } else if let Some(rest) = statement.strip_prefix("use ") {
            imports.push(parse_source_import(rest, *line_num)?);
        } else if let Some(rest) = statement.strip_prefix("capability ") {
            capabilities.push(parse_source_capability(rest, *line_num)?);
        } else if let Some(rest) = statement.strip_prefix("const ") {
            constants.push(parse_source_const(rest, *line_num)?);
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
                "line {line_num}: expected `module`, `use`, `capability`, `const`, `fn`, `test`, or `grant`, got `{statement}`"
            )));
        }
        idx += 1;
    }

    if imports.is_empty()
        && capabilities.is_empty()
        && constants.is_empty()
        && module.is_none()
        && functions.is_empty()
        && tests.is_empty()
        && grants.is_empty()
    {
        return Err(CliError::ParseError(
            "AIL source file has no declarations".to_string(),
        ));
    }

    let mut program = SourceProgram {
        module,
        imports,
        capabilities,
        constants,
        functions,
        tests,
        grants,
    };
    qualify_source_program_module(&mut program);
    resolve_source_program_grants(&mut program)?;
    validate_source_program_symbols(&program)?;
    Ok(program)
}

pub(crate) fn load_source_program(path: &Path) -> Result<SourceProgram, CliError> {
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    load_source_program_inner(path, &mut visiting, &mut visited)
}

pub(crate) fn load_source_program_from_text(
    path: &Path,
    src: &str,
) -> Result<SourceProgram, CliError> {
    let canonical_path = std::fs::canonicalize(path).map_err(|e| {
        CliError::Domain(format!(
            "failed to resolve AIL source {}: {e}",
            path.display()
        ))
    })?;
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    load_source_program_from_text_inner(&canonical_path, src, &mut visiting, &mut visited)
}

pub(super) fn load_source_program_inner(
    path: &Path,
    visiting: &mut BTreeSet<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
) -> Result<SourceProgram, CliError> {
    let canonical_path = std::fs::canonicalize(path).map_err(|e| {
        CliError::Domain(format!(
            "failed to resolve AIL source {}: {e}",
            path.display()
        ))
    })?;
    let src = std::fs::read_to_string(&canonical_path).map_err(|e| {
        CliError::Domain(format!(
            "failed to read AIL source {}: {e}",
            canonical_path.display()
        ))
    })?;
    load_source_program_from_text_inner(&canonical_path, &src, visiting, visited)
}

pub(super) fn load_source_program_from_text_inner(
    canonical_path: &Path,
    src: &str,
    visiting: &mut BTreeSet<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
) -> Result<SourceProgram, CliError> {
    let canonical_path = canonical_path.to_path_buf();
    if visiting.contains(&canonical_path) {
        return Err(CliError::ParseError(format!(
            "cyclic AIL source import detected at {}",
            canonical_path.display()
        )));
    }
    if !visited.insert(canonical_path.clone()) {
        return Ok(SourceProgram::default());
    }

    visiting.insert(canonical_path.clone());
    let program = parse_ail_source(src)?;
    let root_module = program.module.clone();
    let mut combined = SourceProgram::default();
    for import in &program.imports {
        let import_path = resolve_source_import(&canonical_path, import);
        let imported = load_source_program_inner(&import_path, visiting, visited)?;
        combined.extend(imported);
    }
    combined.extend(program);
    combined.module = root_module;
    resolve_source_program_grants(&mut combined)?;
    validate_source_program_symbols(&combined)?;
    validate_source_program_grants(&combined)?;
    validate_source_program_calls(&combined)?;
    validate_source_program_effect_calls(&combined)?;
    validate_source_program_types(&combined)?;
    visiting.remove(&canonical_path);
    Ok(combined)
}

pub(super) fn resolve_source_import(source_path: &Path, import: &str) -> PathBuf {
    let import_path = Path::new(import);
    if import_path.is_absolute() {
        import_path.to_path_buf()
    } else {
        source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(import_path)
    }
}

pub(super) fn parse_source_module(rest: &str, line_num: usize) -> Result<String, CliError> {
    let module = rest.trim();
    validate_source_name(module, line_num)?;
    Ok(module.to_string())
}

pub(super) fn parse_source_import(rest: &str, line_num: usize) -> Result<String, CliError> {
    let import = rest.trim();
    let Some(import) = import.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import declaration requires `use \"relative/path.ail\"`"
        )));
    };
    if import.is_empty() || import.contains('\0') || Path::new(import).is_absolute() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path must be a non-empty relative path"
        )));
    }
    if import.contains('\\') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must use `/` separators"
        )));
    }
    if import.contains(':') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must not contain `:`"
        )));
    }
    if import.contains("//") {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must not contain empty path segments"
        )));
    }
    if import.chars().any(char::is_whitespace) {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must not contain whitespace"
        )));
    }
    if Path::new(import)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must not contain `..`"
        )));
    }
    if !import.starts_with("./") {
        return Err(CliError::ParseError(format!(
            "line {line_num}: local import path `{import}` must start with `./`"
        )));
    }
    if Path::new(import).extension().and_then(|ext| ext.to_str()) != Some("ail") {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must end with `.ail`"
        )));
    }
    Ok(import.to_string())
}

pub(super) fn parse_source_capability(rest: &str, line_num: usize) -> Result<String, CliError> {
    let capability = rest.trim();
    validate_source_name(capability, line_num)?;
    Ok(capability.to_string())
}

pub(super) fn parse_source_const(rest: &str, line_num: usize) -> Result<SourceConst, CliError> {
    let (head, body) = rest.split_once('=').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: const declaration requires `= body`"
        ))
    })?;
    let (name, return_type) = head.split_once(':').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: const declaration requires `name: Type`"
        ))
    })?;
    let name = name.trim();
    let return_type = return_type.trim();
    let body = body.trim();
    validate_source_name(name, line_num)?;
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);
    if body.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: const body must be non-empty"
        )));
    }
    Ok(SourceConst {
        name: normalize_function_name(name),
        return_type,
        body: lower_source_expr(body, line_num)?,
        line_num,
    })
}

pub(super) fn parse_source_function(
    rest: &str,
    line_num: usize,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_and_body) = parse_source_function_signature(rest, line_num)?;
    let (return_type, body) = return_and_body.split_once('=').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: function declaration requires `= body`"
        ))
    })?;

    build_source_function(name, params, return_type.trim(), body.trim(), line_num)
}

pub(super) fn parse_source_function_with_body(
    rest: &str,
    line_num: usize,
    body: String,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_type) = parse_source_function_signature(rest, line_num)?;
    build_source_function(name, params, return_type.trim(), body.trim(), line_num)
}

pub(super) fn parse_source_function_signature(
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

pub(super) fn build_source_function(
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
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);

    Ok(SourceFunction {
        name,
        params,
        return_type,
        body: lower_source_expr(body, line_num)?,
        line_num,
    })
}

pub(super) fn parse_source_params(
    params: &str,
    line_num: usize,
) -> Result<Vec<SourceParam>, CliError> {
    let params = params.trim();
    if params.is_empty() {
        return Ok(vec![]);
    }

    let mut seen = BTreeSet::new();
    split_source_param_list(params)
        .into_iter()
        .map(|raw| {
            let param = raw.trim();
            let (name, ty) = param.split_once(':').ok_or_else(|| {
                CliError::ParseError(format!(
                    "line {line_num}: function parameters must use `name: Type`"
                ))
            })?;
            let name = name.trim();
            let ty = ty.trim();
            validate_source_local_name(name, line_num)?;
            if !seen.insert(name.to_string()) {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: duplicate parameter `{name}`"
                )));
            }
            if ty.is_empty() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: parameter `{name}` requires a type"
                )));
            }
            validate_source_type_name(ty, line_num)?;
            let ty = normalize_source_type_name(ty);
            Ok(SourceParam {
                name: name.to_string(),
                ty,
            })
        })
        .collect()
}

pub(super) fn collect_braced_body(
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

pub(super) fn source_block_to_expr(lines: &[(usize, String)]) -> Result<String, CliError> {
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
        let (binding, value) = rest.split_once('=').ok_or_else(|| {
            CliError::ParseError(format!(
                "line {line_num}: let statement requires `let name = expression`"
            ))
        })?;
        let binding = binding.trim();
        let value = value.trim();
        let (name, ty) = if let Some((name, ty)) = binding.split_once(':') {
            let name = name.trim();
            let ty = ty.trim();
            if ty.is_empty() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: typed let statement requires a type annotation"
                )));
            }
            validate_source_type_name(ty, *line_num)?;
            (name, Some(normalize_source_type_name(ty)))
        } else {
            (binding, None)
        };
        validate_source_local_name(name, *line_num)?;
        if value.is_empty() {
            return Err(CliError::ParseError(format!(
                "line {line_num}: let statement requires a value expression"
            )));
        }
        let lowered_value = lower_source_expr(value, *line_num)?;
        body = if let Some(ty) = ty.as_deref() {
            format!("let_typed({name}, {ty}, {line_num}, {lowered_value}, {body})")
        } else {
            format!("let({name}, {lowered_value}, {body})")
        };
    }

    Ok(body)
}

pub(super) fn parse_source_test(rest: &str, line_num: usize) -> Result<SourceTest, CliError> {
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
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);

    Ok(SourceTest {
        name: normalize_test_name(raw_name),
        return_type,
        body: lower_source_expr(body, line_num)?,
        line_num,
    })
}

pub(super) fn parse_source_grant(rest: &str, line_num: usize) -> Result<SourceGrant, CliError> {
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

pub(super) fn validate_source_match_pattern(pattern: &str) -> Result<(), CliError> {
    let pattern = pattern.trim();
    if pattern == "_" || pattern == "None" || pattern == "true" || pattern == "false" {
        return Ok(());
    }
    if pattern.parse::<i64>().is_ok() {
        return Ok(());
    }
    if let Some((tag, binding)) = source_constructor_pattern(pattern) {
        validate_source_constructor_tag(tag)?;
        if let Some(binding) = binding
            && binding != "_"
        {
            validate_source_local_expr_name(binding)?;
        }
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "unsupported source match pattern `{pattern}`"
    )))
}

pub(super) fn validate_source_constructor_tag(tag: &str) -> Result<(), CliError> {
    if matches!(tag, "Some" | "None" | "Ok" | "Err") {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "unsupported source match constructor `{tag}`"
    )))
}

pub(super) fn source_match_pattern_binding(pattern: &str) -> Option<&str> {
    let (_, binding) = source_constructor_pattern(pattern)?;
    let binding = binding?;
    (binding != "_").then_some(binding)
}

pub(super) fn source_match_pattern_binding_type<'a>(
    pattern: &'a str,
    scrutinee_ty: &str,
) -> Result<Option<(&'a str, String)>, CliError> {
    let Some(binding) = source_match_pattern_binding(pattern) else {
        return Ok(None);
    };
    let Some((tag, _)) = source_constructor_pattern(pattern) else {
        return Ok(None);
    };
    match tag {
        "Some" => source_option_element_type(scrutinee_ty)
            .map(|ty| Some((binding, ty.to_string())))
            .ok_or_else(|| {
                CliError::ParseError(format!(
                    "type mismatch in match pattern `{pattern}`: expected Option<Unknown>, got {scrutinee_ty}"
                ))
            }),
        "Ok" => source_result_types(scrutinee_ty)
            .map(|(ok_ty, _)| Some((binding, ok_ty.to_string())))
            .ok_or_else(|| {
                CliError::ParseError(format!(
                    "type mismatch in match pattern `{pattern}`: expected Result<Unknown,Unknown>, got {scrutinee_ty}"
                ))
            }),
        "Err" => source_result_types(scrutinee_ty)
            .map(|(_, err_ty)| Some((binding, err_ty.to_string())))
            .ok_or_else(|| {
                CliError::ParseError(format!(
                    "type mismatch in match pattern `{pattern}`: expected Result<Unknown,Unknown>, got {scrutinee_ty}"
                ))
            }),
        _ => Ok(None),
    }
}

pub(super) fn validate_source_match_pattern_type(
    pattern: &str,
    scrutinee_ty: &str,
) -> Result<(), CliError> {
    let Some((tag, _)) = source_constructor_pattern(pattern) else {
        return Ok(());
    };
    match tag {
        "Some" | "None" if source_option_element_type(scrutinee_ty).is_some() => Ok(()),
        "Some" | "None" => Err(CliError::ParseError(format!(
            "type mismatch in match pattern `{pattern}`: expected Option<Unknown>, got {scrutinee_ty}"
        ))),
        "Ok" | "Err" if source_result_types(scrutinee_ty).is_some() => Ok(()),
        "Ok" | "Err" => Err(CliError::ParseError(format!(
            "type mismatch in match pattern `{pattern}`: expected Result<Unknown,Unknown>, got {scrutinee_ty}"
        ))),
        _ => Ok(()),
    }
}

pub(super) fn validate_source_match_reachable(arms: &[String]) -> Result<(), CliError> {
    let mut seen = BTreeSet::new();
    let mut saw_wildcard = false;

    for pattern in arms.iter().step_by(2).map(String::as_str) {
        let normalized = source_match_pattern_key(pattern);
        if saw_wildcard {
            return Err(CliError::ParseError(format!(
                "unreachable match arm `{pattern}` after wildcard `_`"
            )));
        }
        if normalized == "_" {
            saw_wildcard = true;
            continue;
        }
        if !seen.insert(normalized.clone()) {
            return Err(CliError::ParseError(format!(
                "duplicate match arm pattern `{normalized}`"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_source_match_exhaustive(
    arms: &[String],
    scrutinee_ty: &str,
) -> Result<(), CliError> {
    let patterns = arms.iter().step_by(2).map(String::as_str);
    if patterns.clone().any(|pattern| pattern.trim() == "_") {
        return Ok(());
    }

    if source_option_element_type(scrutinee_ty).is_some() {
        let has_some = patterns
            .clone()
            .any(|pattern| source_pattern_tag(pattern) == Some("Some"));
        let has_none = patterns
            .clone()
            .any(|pattern| source_pattern_tag(pattern) == Some("None"));
        if has_some && has_none {
            return Ok(());
        }
        return Err(CliError::ParseError(format!(
            "non-exhaustive match for {scrutinee_ty}: expected Some and None arms or `_`"
        )));
    }

    if source_result_types(scrutinee_ty).is_some() {
        let has_ok = patterns
            .clone()
            .any(|pattern| source_pattern_tag(pattern) == Some("Ok"));
        let has_err = patterns
            .clone()
            .any(|pattern| source_pattern_tag(pattern) == Some("Err"));
        if has_ok && has_err {
            return Ok(());
        }
        return Err(CliError::ParseError(format!(
            "non-exhaustive match for {scrutinee_ty}: expected Ok and Err arms or `_`"
        )));
    }

    if scrutinee_ty == "Bool" {
        let has_true = patterns.clone().any(|pattern| pattern.trim() == "true");
        let has_false = patterns.clone().any(|pattern| pattern.trim() == "false");
        if has_true && has_false {
            return Ok(());
        }
        return Err(CliError::ParseError(
            "non-exhaustive match for Bool: expected true and false arms or `_`".to_string(),
        ));
    }

    Ok(())
}

pub(super) fn source_match_pattern_key(pattern: &str) -> String {
    source_pattern_tag(pattern)
        .map(ToString::to_string)
        .unwrap_or_else(|| pattern.trim().to_string())
}

pub(super) fn source_pattern_tag(pattern: &str) -> Option<&str> {
    if pattern.trim() == "None" {
        return Some("None");
    }
    source_constructor_pattern(pattern).map(|(tag, _)| tag)
}

pub(super) fn source_constructor_pattern(pattern: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = pattern.trim();
    let first = trimmed.chars().next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    if let Some(open) = trimmed.find('(') {
        let tag = trimmed[..open].trim();
        let binding = trimmed[open + 1..].trim().strip_suffix(')')?.trim();
        if binding.contains('(') || binding.contains(',') || binding.is_empty() {
            return None;
        }
        Some((tag, Some(binding)))
    } else {
        Some((trimmed, None))
    }
}

pub(super) fn normalize_source_match_pattern(
    pattern: &str,
    line_num: usize,
) -> Result<String, CliError> {
    let pattern = pattern.trim();
    let normalized = if pattern == "none" || pattern == "none()" {
        "None".to_string()
    } else if let Some(inner) = pattern
        .strip_prefix("some(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        format!("Some({})", inner.trim())
    } else if let Some(inner) = pattern
        .strip_prefix("ok(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        format!("Ok({})", inner.trim())
    } else if let Some(inner) = pattern
        .strip_prefix("err(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        format!("Err({})", inner.trim())
    } else {
        pattern.to_string()
    };
    validate_source_match_pattern(&normalized)
        .map_err(|err| source_error_at_line(err, line_num))?;
    Ok(normalized)
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

pub(super) fn split_source_top_level_commas(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut angle_depth = 0usize;

    for (idx, ch) in input.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ',' if angle_depth == 0 => {
                out.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(input[start..].trim());
    out.into_iter().filter(|part| !part.is_empty()).collect()
}

pub(super) fn normalize_grant_target(target: &str) -> String {
    target.to_string()
}

pub(super) fn validate_source_local_name(name: &str, line_num: usize) -> Result<(), CliError> {
    validate_source_name(name, line_num)?;
    if name.contains('.') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: local binding name `{name}` must not contain `.`"
        )));
    }
    Ok(())
}

pub(super) fn validate_source_name(name: &str, line_num: usize) -> Result<(), CliError> {
    if name.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: declaration name cannot be empty"
        )));
    }
    if !is_valid_source_name_chars(name) {
        return Err(CliError::ParseError(format!(
            "line {line_num}: declaration name `{name}` contains unsupported characters"
        )));
    }
    if name.split('.').any(str::is_empty) {
        return Err(CliError::ParseError(format!(
            "line {line_num}: declaration name `{name}` contains an empty path segment"
        )));
    }
    if let Some(segment) = first_invalid_source_name_segment(name) {
        return Err(CliError::ParseError(format!(
            "line {line_num}: declaration name `{name}` segment `{segment}` must start with a letter or `_`"
        )));
    }
    Ok(())
}

pub(super) fn is_valid_source_name_chars(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

pub(super) fn source_name_segments_are_valid(name: &str) -> bool {
    !name.split('.').any(str::is_empty) && first_invalid_source_name_segment(name).is_none()
}

pub(super) fn first_invalid_source_name_segment(name: &str) -> Option<&str> {
    name.split('.').find(|segment| {
        !segment
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
    })
}

pub(super) fn normalize_function_name(name: &str) -> String {
    if name.starts_with("fn.") {
        name.to_string()
    } else {
        format!("fn.{name}")
    }
}

pub(super) fn normalize_test_name(name: &str) -> String {
    if name.starts_with("test.") {
        name.to_string()
    } else {
        format!("test.{name}")
    }
}

pub(super) fn source_default_entry(program: &SourceProgram) -> String {
    program
        .module
        .as_deref()
        .map(|module| format!("fn.{module}.main"))
        .unwrap_or_else(|| "fn.main".to_string())
}

pub(super) fn source_change_name(path: &Path) -> String {
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
