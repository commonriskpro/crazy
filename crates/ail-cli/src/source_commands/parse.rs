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
mod declarations;
mod names;
mod patterns;
mod types;

pub(super) use declarations::*;
pub(super) use names::*;
pub(super) use patterns::*;
pub(super) use types::*;

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
                return Err(source_parse_error_for_fragment(
                    *line_num,
                    SourceParseDiagnostic::InvalidDeclaration,
                    statement,
                    "module declaration may appear only once",
                ));
            }
            module = Some(parse_source_module(rest, *line_num)?);
        } else if let Some(rest) = statement.strip_prefix("use ") {
            let import = parse_source_import(rest, *line_num)?;
            if imports.iter().any(|existing| existing == &import) {
                return Err(source_parse_error_for_fragment(
                    *line_num,
                    SourceParseDiagnostic::InvalidDeclaration,
                    statement,
                    format!("duplicate import declaration `{import}`"),
                ));
            }
            imports.push(import);
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
        } else if statement == "export" || statement.starts_with("export ") {
            return Err(source_parse_error_for_fragment(
                *line_num,
                SourceParseDiagnostic::InvalidDeclaration,
                statement,
                "unsupported source export syntax; imported `.ail` files expose declarations by name automatically",
            ));
        } else {
            let token = statement.split_whitespace().next().unwrap_or(statement);
            return Err(source_parse_error_for_fragment(
                *line_num,
                SourceParseDiagnostic::UnexpectedToken,
                statement,
                format!(
                    "expected `module`, `use`, `capability`, `const`, `fn`, `test`, or `grant`, got `{token}`"
                ),
            ));
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
        return Err(source_parse_error(
            1,
            SourceParseDiagnostic::InvalidDeclaration,
            "AIL source file has no declarations",
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
    let mut visiting = Vec::new();
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
    let mut visiting = Vec::new();
    let mut visited = BTreeSet::new();
    load_source_program_from_text_inner(&canonical_path, src, &mut visiting, &mut visited)
}

pub(super) fn load_source_program_inner(
    path: &Path,
    visiting: &mut Vec<PathBuf>,
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
    visiting: &mut Vec<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
) -> Result<SourceProgram, CliError> {
    let canonical_path = canonical_path.to_path_buf();
    if let Some(start) = visiting.iter().position(|path| path == &canonical_path) {
        return Err(CliError::ParseError(format_source_import_cycle(
            visiting,
            start,
            &canonical_path,
        )));
    }
    if !visited.insert(canonical_path.clone()) {
        return Ok(SourceProgram::default());
    }

    visiting.push(canonical_path.clone());
    let result =
        load_source_program_from_text_inner_with_stack(&canonical_path, src, visiting, visited);
    visiting.pop();
    result
}

fn load_source_program_from_text_inner_with_stack(
    canonical_path: &Path,
    src: &str,
    visiting: &mut Vec<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
) -> Result<SourceProgram, CliError> {
    let mut program = parse_ail_source(src)?;
    program.set_source_path(canonical_path);
    let root_module = program.module.clone();
    let mut combined = SourceProgram::default();
    for import in &program.imports {
        let import_path = resolve_source_import(canonical_path, import);
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
    Ok(combined)
}

fn format_source_import_cycle(visiting: &[PathBuf], start: usize, repeated: &Path) -> String {
    let mut chain = visiting[start..]
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    chain.push(repeated.display().to_string());
    format!("cyclic AIL source import detected: {}", chain.join(" -> "))
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
