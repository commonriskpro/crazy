// ── ail-cli::source_commands ──────────────────────────────────────────────
//
// Minimal AIL source-language frontend.
//
// This is intentionally small, but it is not ACL: users can write `.ail`
// source files and run/test them without authoring ChangeSet ops directly.
// The current frontend lowers that source into the existing semantic graph
// pipeline so the compiler/runtime path stays real end-to-end.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use ail_change::canonical::canonicalize_parsed;
use ail_change::model::{ChangeSetOutcome, SnapshotId};
use ail_change::parser::parse_changeset;
use ail_core::semantic_graph::SemanticGraph;
use serde_json::json;

use crate::cli_helpers::SimpleSnapshotBridge;
use crate::error::CliError;
use crate::output::{OutputMode, print_response};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SourceProgram {
    module: Option<String>,
    imports: Vec<String>,
    capabilities: Vec<String>,
    constants: Vec<SourceConst>,
    functions: Vec<SourceFunction>,
    tests: Vec<SourceTest>,
    grants: Vec<SourceGrant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceConst {
    name: String,
    return_type: String,
    body: String,
    line_num: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceFunction {
    name: String,
    params: Vec<SourceParam>,
    return_type: String,
    body: String,
    line_num: usize,
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
    line_num: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceGrant {
    target: String,
    capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceCallable {
    param_types: Vec<String>,
    return_type: String,
}

pub(crate) struct LoadedSourceGraph {
    pub(crate) graph: SemanticGraph,
    pub(crate) default_entry: String,
}

/// Load a `.ail` source file and lower it to a semantic graph.
pub(crate) fn load_source_graph(path: &Path) -> Result<SemanticGraph, CliError> {
    Ok(load_source_graph_with_entry(path)?.graph)
}

pub(crate) fn load_source_graph_with_entry(path: &Path) -> Result<LoadedSourceGraph, CliError> {
    let program = load_source_program(path)?;
    let default_entry = source_default_entry(&program);
    let graph = source_program_to_graph(&program, source_change_name(path))?;
    Ok(LoadedSourceGraph {
        graph,
        default_entry,
    })
}

pub(crate) fn cmd_check_source(mode: OutputMode, path: &Path) -> Result<(), CliError> {
    let program = load_source_program(path)?;
    let default_entry = source_default_entry(&program);
    let graph = source_program_to_graph(&program, source_change_name(path))?;
    let item_count = program.imports.len()
        + program.capabilities.len()
        + program.constants.len()
        + program.functions.len()
        + program.tests.len()
        + program.grants.len();
    let human_msg = format!(
        "AIL check: ok\nfile: {}\nitems: {item_count}\nfunctions: {}\ntests: {}\ndefault_entry: {}\ngraph_nodes: {}\ngraph_edges: {}",
        path.display(),
        program.functions.len(),
        program.tests.len(),
        default_entry,
        graph.nodes.len(),
        graph.edges.len()
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "language": "ail-source",
            "file": path.display().to_string(),
            "item_count": item_count,
            "module": program.module.as_deref(),
            "default_entry": default_entry,
            "imports": program.imports.len(),
            "capabilities": program.capabilities.len(),
            "functions": program.functions.len(),
            "tests": program.tests.len(),
            "grants": program.grants.len(),
            "graph_nodes": graph.nodes.len(),
            "graph_edges": graph.edges.len(),
        }),
    );
    Ok(())
}

/// Format a supported `.ail` source file into stable canonical source text.
pub(crate) fn format_ail_source(src: &str) -> Result<(String, usize), CliError> {
    let program = parse_ail_source(src)?;
    let constants = source_constant_names(&program);
    let mut out = String::new();

    if let Some(module) = &program.module {
        render_source_module(&mut out, module);
    }
    for import in &program.imports {
        render_source_import(&mut out, import);
    }
    for capability in &program.capabilities {
        render_source_capability(&mut out, capability);
    }
    for constant in &program.constants {
        render_source_const(&mut out, constant, program.module.as_deref(), &constants);
    }
    for function in &program.functions {
        render_source_function(&mut out, function, program.module.as_deref(), &constants);
    }
    for test in &program.tests {
        render_source_test(&mut out, test, program.module.as_deref(), &constants);
    }
    for grant in &program.grants {
        render_source_grant(&mut out, grant, program.module.as_deref());
    }

    Ok((
        out,
        usize::from(program.module.is_some())
            + program.imports.len()
            + program.capabilities.len()
            + program.constants.len()
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

fn load_source_program_inner(
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

fn load_source_program_from_text_inner(
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

fn resolve_source_import(source_path: &Path, import: &str) -> PathBuf {
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

impl SourceProgram {
    fn extend(&mut self, other: SourceProgram) {
        self.imports.extend(other.imports);
        self.capabilities.extend(other.capabilities);
        self.constants.extend(other.constants);
        self.functions.extend(other.functions);
        self.tests.extend(other.tests);
        self.grants.extend(other.grants);
    }
}

fn qualify_source_program_module(program: &mut SourceProgram) {
    let Some(module) = program.module.clone() else {
        return;
    };

    let local_functions = program
        .functions
        .iter()
        .filter_map(|function| unqualified_source_name(&function.name, "fn."))
        .chain(
            program
                .constants
                .iter()
                .filter_map(|constant| unqualified_source_name(&constant.name, "fn.")),
        )
        .collect::<BTreeSet<_>>();

    for constant in &mut program.constants {
        constant.name = qualify_source_name(&constant.name, "fn.", &module);
        constant.body = qualify_source_expr_calls(&constant.body, &module, &local_functions);
    }
    for function in &mut program.functions {
        function.name = qualify_source_name(&function.name, "fn.", &module);
        function.body = qualify_source_expr_calls(&function.body, &module, &local_functions);
    }
    for test in &mut program.tests {
        test.name = qualify_source_name(&test.name, "test.", &module);
        test.body = qualify_source_expr_calls(&test.body, &module, &local_functions);
    }
}

fn resolve_source_program_grants(program: &mut SourceProgram) -> Result<(), CliError> {
    let functions = program
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<BTreeSet<_>>();
    let tests = program
        .tests
        .iter()
        .map(|test| test.name.as_str())
        .collect::<BTreeSet<_>>();
    let module = program.module.as_deref();

    for grant in &mut program.grants {
        grant.target = resolve_source_grant_target(&grant.target, module, &functions, &tests)?;
    }
    Ok(())
}

fn resolve_source_grant_target(
    target: &str,
    module: Option<&str>,
    functions: &BTreeSet<&str>,
    tests: &BTreeSet<&str>,
) -> Result<String, CliError> {
    let mut matches = BTreeSet::new();
    let mut add_candidate = |candidate: String, declared: &BTreeSet<&str>| {
        if declared.contains(candidate.as_str()) {
            matches.insert(candidate);
        }
    };

    if target.starts_with("fn.") {
        if functions.contains(target) {
            return Ok(target.to_string());
        }
        add_candidate(
            qualify_source_name_for_module(target, "fn.", module),
            functions,
        );
    } else if target.starts_with("test.") {
        if tests.contains(target) {
            return Ok(target.to_string());
        }
        add_candidate(
            qualify_source_name_for_module(target, "test.", module),
            tests,
        );
    } else {
        add_candidate(
            qualify_source_name_for_module(&normalize_function_name(target), "fn.", module),
            functions,
        );
        add_candidate(
            qualify_source_name_for_module(&normalize_test_name(target), "test.", module),
            tests,
        );
    }

    match matches.len() {
        0 => Ok(default_source_grant_target(target, module)),
        1 => Ok(matches
            .into_iter()
            .next()
            .expect("single grant target match")),
        _ => Err(CliError::ParseError(format!(
            "grant target `{target}` is ambiguous; use `fn.{target}` or `test.{target}`"
        ))),
    }
}

fn default_source_grant_target(target: &str, module: Option<&str>) -> String {
    if target.starts_with("fn.") {
        qualify_source_name_for_module(target, "fn.", module)
    } else if target.starts_with("test.") {
        qualify_source_name_for_module(target, "test.", module)
    } else {
        qualify_source_name_for_module(&normalize_function_name(target), "fn.", module)
    }
}

fn qualify_source_name_for_module(name: &str, prefix: &str, module: Option<&str>) -> String {
    module
        .map(|module| qualify_source_name(name, prefix, module))
        .unwrap_or_else(|| name.to_string())
}

fn unqualified_source_name(name: &str, prefix: &str) -> Option<String> {
    let bare = name.strip_prefix(prefix)?;
    if bare.contains('.') {
        None
    } else {
        Some(bare.to_string())
    }
}

fn qualify_source_name(name: &str, prefix: &str, module: &str) -> String {
    let Some(bare) = name.strip_prefix(prefix) else {
        return name.to_string();
    };
    if bare.contains('.') {
        name.to_string()
    } else {
        format!("{prefix}{module}.{bare}")
    }
}

fn qualify_source_expr_calls(
    expr: &str,
    module: &str,
    local_functions: &BTreeSet<String>,
) -> String {
    let Some((func, args)) = parse_source_call(expr) else {
        return expr.trim().to_string();
    };
    let qualified_func = if !func.contains('.')
        && known_source_builtin_arity(&func).is_none()
        && local_functions.contains(&func)
    {
        format!("{module}.{func}")
    } else {
        func
    };

    let rewritten_args = if qualified_func == "let"
        && args.len() == 3
        && is_source_local_ident(&args[0])
    {
        vec![
            args[0].clone(),
            qualify_source_expr_calls(&args[1], module, local_functions),
            qualify_source_expr_calls(&args[2], module, local_functions),
        ]
    } else if qualified_func == "let_typed" && args.len() == 5 && is_source_local_ident(&args[0]) {
        vec![
            args[0].clone(),
            args[1].clone(),
            args[2].clone(),
            qualify_source_expr_calls(&args[3], module, local_functions),
            qualify_source_expr_calls(&args[4], module, local_functions),
        ]
    } else {
        args.iter()
            .map(|arg| qualify_source_expr_calls(arg, module, local_functions))
            .collect::<Vec<_>>()
    };

    format!("{}({})", qualified_func, rewritten_args.join(", "))
}

fn validate_source_program_symbols(program: &SourceProgram) -> Result<(), CliError> {
    if let Some(name) = duplicate_name(program.capabilities.iter().map(String::as_str)) {
        return Err(CliError::ParseError(format!(
            "duplicate capability declaration `{name}`"
        )));
    }
    if let Some(name) = duplicate_name(
        program
            .constants
            .iter()
            .map(|constant| constant.name.as_str()),
    ) {
        return Err(CliError::ParseError(format!(
            "duplicate const declaration `{name}`"
        )));
    }
    let function_names = program
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(name) = program
        .constants
        .iter()
        .map(|constant| constant.name.as_str())
        .find(|name| function_names.contains(name))
    {
        return Err(CliError::ParseError(format!(
            "duplicate function or const declaration `{name}`"
        )));
    }
    if let Some(name) = duplicate_name(
        program
            .functions
            .iter()
            .map(|function| function.name.as_str()),
    ) {
        return Err(CliError::ParseError(format!(
            "duplicate function declaration `{name}`"
        )));
    }
    if let Some(name) = duplicate_name(program.tests.iter().map(|test| test.name.as_str())) {
        return Err(CliError::ParseError(format!(
            "duplicate test declaration `{name}`"
        )));
    }
    for constant in &program.constants {
        if let Some(builtin) = source_function_builtin_shadow(&constant.name) {
            return Err(CliError::ParseError(format!(
                "const declaration `{}` shadows builtin `{builtin}`",
                constant.name
            )));
        }
    }
    for function in &program.functions {
        if let Some(builtin) = source_function_builtin_shadow(&function.name) {
            return Err(CliError::ParseError(format!(
                "function declaration `{}` shadows builtin `{builtin}`",
                function.name
            )));
        }
    }
    Ok(())
}

fn source_function_builtin_shadow(name: &str) -> Option<&str> {
    let bare = name.strip_prefix("fn.")?.rsplit('.').next()?;
    known_source_builtin_arity(bare).map(|_| bare)
}

fn duplicate_name<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            return Some(name.to_string());
        }
    }
    None
}

fn validate_source_program_grants(program: &SourceProgram) -> Result<(), CliError> {
    let capabilities = program
        .capabilities
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let grant_targets = program
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .chain(program.tests.iter().map(|test| test.name.as_str()))
        .collect::<BTreeSet<_>>();

    for grant in &program.grants {
        if !grant_targets.contains(grant.target.as_str()) {
            return Err(CliError::ParseError(format!(
                "grant target `{}` is not declared as a function or test",
                grant.target
            )));
        }
        if !capabilities.contains(grant.capability.as_str()) {
            return Err(CliError::ParseError(format!(
                "grant capability `{}` is not declared",
                grant.capability
            )));
        }
    }
    Ok(())
}

fn validate_source_program_effect_calls(program: &SourceProgram) -> Result<(), CliError> {
    let capabilities = program
        .capabilities
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let grants = source_grants_by_target(program);

    for constant in &program.constants {
        validate_source_const_effect_free(&constant.name, &constant.body, &capabilities)
            .map_err(|err| source_error_at_line(err, constant.line_num))?;
    }
    for function in &program.functions {
        validate_source_item_effect_grants(&function.name, &function.body, &capabilities, &grants)
            .map_err(|err| source_error_at_line(err, function.line_num))?;
    }
    for test in &program.tests {
        validate_source_item_effect_grants(&test.name, &test.body, &capabilities, &grants)
            .map_err(|err| source_error_at_line(err, test.line_num))?;
    }
    Ok(())
}

fn validate_source_const_effect_free(
    target: &str,
    body: &str,
    capabilities: &BTreeSet<&str>,
) -> Result<(), CliError> {
    let mut direct_effects = BTreeSet::new();
    collect_source_direct_effect_capabilities(body, capabilities, &mut direct_effects)?;
    if let Some(capability) = direct_effects.into_iter().next() {
        return Err(CliError::ParseError(format!(
            "source const `{target}` uses capability `{capability}`; const declarations must be effect-free"
        )));
    }
    Ok(())
}

fn source_grants_by_target(program: &SourceProgram) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut grants = BTreeMap::<&str, BTreeSet<&str>>::new();
    for grant in &program.grants {
        grants
            .entry(grant.target.as_str())
            .or_default()
            .insert(grant.capability.as_str());
    }
    grants
}

fn validate_source_item_effect_grants(
    target: &str,
    body: &str,
    capabilities: &BTreeSet<&str>,
    grants: &BTreeMap<&str, BTreeSet<&str>>,
) -> Result<(), CliError> {
    let mut direct_effects = BTreeSet::new();
    collect_source_direct_effect_capabilities(body, capabilities, &mut direct_effects)?;
    let granted = grants.get(target);
    for capability in direct_effects {
        if !granted.is_some_and(|caps| caps.contains(capability.as_str())) {
            return Err(CliError::ParseError(format!(
                "source item `{target}` uses capability `{capability}` without a grant"
            )));
        }
    }
    Ok(())
}

fn collect_source_direct_effect_capabilities(
    expr: &str,
    capabilities: &BTreeSet<&str>,
    out: &mut BTreeSet<String>,
) -> Result<(), CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(());
    };
    if func == "print" {
        validate_declared_source_effect_capability("log.write", capabilities, "print")?;
        out.insert("log.write".to_string());
    }
    if func == "effect_call" && args.len() >= 2 {
        let capability = args[0].as_str();
        let operation = args[1].as_str();
        if !is_source_ident(capability) || !is_source_ident(operation) {
            return Err(CliError::ParseError(
                "effect_call capability and operation must be identifiers".to_string(),
            ));
        }
        validate_declared_source_effect_capability(capability, capabilities, "effect_call")?;
        out.insert(capability.to_string());
        for arg in &args[2..] {
            collect_source_direct_effect_capabilities(arg, capabilities, out)?;
        }
        return Ok(());
    }
    for arg in args {
        collect_source_direct_effect_capabilities(&arg, capabilities, out)?;
    }
    Ok(())
}

fn validate_declared_source_effect_capability(
    capability: &str,
    capabilities: &BTreeSet<&str>,
    context: &str,
) -> Result<(), CliError> {
    if capabilities.contains(capability) {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "{context} capability `{capability}` is not declared"
    )))
}

fn validate_source_program_calls(program: &SourceProgram) -> Result<(), CliError> {
    let functions = program
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function.params.len()))
        .chain(
            program
                .constants
                .iter()
                .map(|constant| (constant.name.as_str(), 0usize)),
        )
        .collect::<BTreeMap<_, _>>();

    let constants = source_constant_names(program);

    for constant in &program.constants {
        validate_source_expr_calls(&constant.body, &functions)
            .map_err(|err| source_error_at_line(err, constant.line_num))?;
        validate_source_expr_vars(&constant.body, &BTreeSet::new(), &constants)
            .map_err(|err| source_error_at_line(err, constant.line_num))?;
    }
    for function in &program.functions {
        validate_source_expr_calls(&function.body, &functions)
            .map_err(|err| source_error_at_line(err, function.line_num))?;
        validate_source_expr_vars(
            &function.body,
            &function
                .params
                .iter()
                .map(|param| param.name.as_str())
                .collect::<BTreeSet<_>>(),
            &constants,
        )
        .map_err(|err| source_error_at_line(err, function.line_num))?;
    }
    for test in &program.tests {
        validate_source_expr_calls(&test.body, &functions)
            .map_err(|err| source_error_at_line(err, test.line_num))?;
        validate_source_expr_vars(&test.body, &BTreeSet::new(), &constants)
            .map_err(|err| source_error_at_line(err, test.line_num))?;
    }
    Ok(())
}

fn source_constant_names(program: &SourceProgram) -> BTreeMap<String, String> {
    program
        .constants
        .iter()
        .flat_map(|constant| source_const_reference_names(&constant.name))
        .collect()
}

fn source_const_reference_names(name: &str) -> Vec<(String, String)> {
    let bare = name.strip_prefix("fn.").unwrap_or(name);
    let target = bare.to_string();
    let mut names = vec![
        (bare.to_string(), target.clone()),
        (name.to_string(), target.clone()),
    ];
    if let Some((_, local)) = bare.split_once('.') {
        names.push((local.to_string(), target));
    }
    names
}

fn source_const_reference_target(
    name: &str,
    constants: &BTreeMap<String, String>,
) -> Option<String> {
    constants
        .get(name)
        .cloned()
        .or_else(|| constants.get(&format!("fn.{name}")).cloned())
}

fn validate_source_expr_calls(
    expr: &str,
    functions: &BTreeMap<&str, usize>,
) -> Result<(), CliError> {
    let mut calls = Vec::new();
    collect_source_calls(expr, &mut calls);
    for (call, argc) in calls {
        if let Some(arity) = known_source_builtin_arity(&call) {
            validate_source_call_arity(&call, argc, arity)?;
            continue;
        }
        if let Some(expected) = source_function_arity(functions, &call) {
            validate_source_call_arity(&call, argc, SourceArity::Exact(expected))?;
            continue;
        }
        return Err(CliError::ParseError(format!(
            "unknown function call `{call}` in AIL source"
        )));
    }
    Ok(())
}

fn collect_source_calls(expr: &str, calls: &mut Vec<(String, usize)>) {
    let Some((func, args)) = parse_source_call(expr) else {
        return;
    };
    calls.push((func, args.len()));
    if func == "match" {
        if let Some(scrutinee) = args.first() {
            collect_source_calls(scrutinee, calls);
        }
        for body in args.iter().skip(2).step_by(2) {
            collect_source_calls(body, calls);
        }
        return;
    }
    for arg in args {
        collect_source_calls(&arg, calls);
    }
}

fn source_function_arity(functions: &BTreeMap<&str, usize>, call: &str) -> Option<usize> {
    let normalized = if call.starts_with("fn.") {
        call.to_string()
    } else {
        format!("fn.{call}")
    };
    functions.get(normalized.as_str()).copied()
}

#[derive(Debug, Clone, Copy)]
enum SourceArity {
    Exact(usize),
    Min(usize),
    Even,
    Any,
    Match,
}

fn validate_source_call_arity(
    call: &str,
    actual: usize,
    arity: SourceArity,
) -> Result<(), CliError> {
    match arity {
        SourceArity::Exact(expected) if actual != expected => Err(CliError::ParseError(format!(
            "function call `{call}` expects {expected} argument(s), got {actual}"
        ))),
        SourceArity::Min(expected) if actual < expected => Err(CliError::ParseError(format!(
            "function call `{call}` expects at least {expected} argument(s), got {actual}"
        ))),
        SourceArity::Even if !actual.is_multiple_of(2) => Err(CliError::ParseError(format!(
            "function call `{call}` expects an even number of arguments, got {actual}"
        ))),
        SourceArity::Match if actual < 3 || actual.is_multiple_of(2) => {
            Err(CliError::ParseError(format!(
                "function call `{call}` expects a scrutinee plus pattern/body pairs, got {actual} argument(s)"
            )))
        }
        _ => Ok(()),
    }
}

fn known_source_builtin_arity(call: &str) -> Option<SourceArity> {
    let arity = match call {
        "add" | "sub" | "mul" | "div" | "mod" | "signed_mod" | "eq" | "ne" | "gt" | "ge" | "lt"
        | "le" | "and" | "or" | "concat" | "int.min" | "int.max" | "int.abs_or" | "int.neg_or"
        | "int.saturating_add" | "int.saturating_sub" | "int.saturating_mul" | "int_min"
        | "int_max" | "int_abs_or" | "int_neg_or" | "int_saturating_add" | "int_saturating_sub"
        | "int_saturating_mul" => SourceArity::Exact(2),
        "text.contains" | "text.ends_with" | "text.index_of" | "text.starts_with"
        | "text_contains" | "text_ends_with" | "text_index_of" | "text_starts_with" | "text.eq"
        | "text_eq" => SourceArity::Exact(2),
        "text.parse_int_or" | "text_parse_int_or" => SourceArity::Exact(2),
        "text.byte_at_or" | "text_byte_at_or" | "text.replace_first" | "text_replace_first"
        | "text.slice" | "text_slice" | "int.clamp" | "int.add_or" | "int.div_or"
        | "int.rem_or" | "int.sub_or" | "int.mul_or" | "int_clamp" | "int_add_or"
        | "int_sub_or" | "int_mul_or" | "int_div_or" | "int_rem_or" => SourceArity::Exact(3),
        "field" | "index" | "unwrap_or" | "first_or" | "last_or" => SourceArity::Exact(2),
        "get_or" => SourceArity::Exact(3),
        "update" => SourceArity::Exact(3),
        "none" => SourceArity::Exact(0),
        "not" | "len" | "print" | "text.trim" | "text_trim" | "Var" | "is_empty" | "is_some"
        | "is_none" | "is_ok" | "is_err" => SourceArity::Exact(1),
        "some" | "ok" | "err" => SourceArity::Exact(1),
        "if" | "let" | "fold" => SourceArity::Exact(3),
        "let_typed" => SourceArity::Exact(5),
        "effect_call" => SourceArity::Min(2),
        "map" | "record" => SourceArity::Even,
        "tuple" | "list" | "set" => SourceArity::Any,
        "match" => SourceArity::Match,
        _ => return None,
    };
    Some(arity)
}

fn validate_source_expr_vars(
    expr: &str,
    scope: &BTreeSet<&str>,
    constants: &BTreeMap<String, String>,
) -> Result<(), CliError> {
    let expr = expr.trim();
    if is_source_literal(expr) {
        return Ok(());
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
    if let Some((func, args)) = parse_source_call(expr) {
        if func == "let" && args.len() == 3 {
            validate_source_local_expr_name(&args[0])?;
            validate_source_expr_vars(&args[1], scope, constants)?;
            let mut inner_scope = scope.clone();
            inner_scope.insert(args[0].as_str());
            return validate_source_expr_vars(&args[2], &inner_scope, constants);
        }
        if func == "let_typed" && args.len() == 5 {
            validate_source_local_expr_name(&args[0])?;
            validate_source_type_annotation(&args[1])?;
            validate_source_let_line_marker(&args[2])?;
            validate_source_expr_vars(&args[3], scope, constants)?;
            let mut inner_scope = scope.clone();
            inner_scope.insert(args[0].as_str());
            return validate_source_expr_vars(&args[4], &inner_scope, constants);
        }
        if func == "match" && args.len() >= 3 && args.len() % 2 == 1 {
            validate_source_expr_vars(&args[0], scope, constants)?;
            for pair in args[1..].chunks_exact(2) {
                validate_source_match_pattern(&pair[0])?;
                let mut arm_scope = scope.clone();
                if let Some(binding) = source_match_pattern_binding(&pair[0]) {
                    arm_scope.insert(binding);
                }
                validate_source_expr_vars(&pair[1], &arm_scope, constants)?;
            }
            return Ok(());
        }
        if func == "record" && args.len().is_multiple_of(2) {
            for pair in args.chunks_exact(2) {
                validate_source_local_expr_name(&pair[0])?;
                validate_source_expr_vars(&pair[1], scope, constants)?;
            }
            return Ok(());
        }
        if func == "field" && args.len() == 2 {
            validate_source_expr_vars(&args[0], scope, constants)?;
            validate_source_local_expr_name(&args[1])?;
            return Ok(());
        }
        if func == "update" && args.len() == 3 {
            validate_source_expr_vars(&args[0], scope, constants)?;
            validate_source_local_expr_name(&args[1])?;
            validate_source_expr_vars(&args[2], scope, constants)?;
            return Ok(());
        }
        let args_to_validate: &[String] = if func == "effect_call" && args.len() >= 2 {
            &args[2..]
        } else {
            &args
        };
        for arg in args_to_validate {
            validate_source_expr_vars(arg, scope, constants)?;
        }
        return Ok(());
    }
    if is_source_ident(expr) {
        if scope.contains(expr) || source_const_reference_target(expr, constants).is_some() {
            return Ok(());
        }
        return Err(CliError::ParseError(format!(
            "unknown variable `{expr}` in AIL source"
        )));
    }
    Err(CliError::ParseError(format!(
        "unsupported source expression `{expr}`"
    )))
}

fn is_source_literal(expr: &str) -> bool {
    let expr = expr.trim();
    expr == "true"
        || expr == "false"
        || is_source_string_literal(expr)
        || expr.parse::<i64>().is_ok()
        || is_source_float_literal(expr)
}

fn is_unsupported_source_numeric_literal(expr: &str) -> bool {
    expr.trim()
        .parse::<f64>()
        .map(|value| !value.is_finite())
        .unwrap_or(false)
}

fn is_source_float_literal(expr: &str) -> bool {
    expr.trim()
        .parse::<f64>()
        .map(f64::is_finite)
        .unwrap_or(false)
}

fn is_malformed_source_string(expr: &str) -> bool {
    expr.trim().starts_with('"') && !is_source_string_literal(expr)
}

fn is_source_string_literal(expr: &str) -> bool {
    let expr = expr.trim();
    if expr.len() < 2 || !expr.starts_with('"') || !expr.ends_with('"') {
        return false;
    }

    let mut prev_was_escape = false;
    for ch in expr[1..expr.len() - 1].chars() {
        if prev_was_escape {
            if !matches!(ch, '"' | '\\') {
                return false;
            }
            prev_was_escape = false;
            continue;
        }
        if ch == '\\' {
            prev_was_escape = true;
            continue;
        }
        if ch == '"' {
            return false;
        }
    }

    !prev_was_escape
}

fn validate_source_program_types(program: &SourceProgram) -> Result<(), CliError> {
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

fn source_callable_types(program: &SourceProgram) -> BTreeMap<&str, SourceCallable> {
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

fn source_callable_for_reference<'a>(
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

fn infer_source_expr_type(
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

fn infer_source_call_type(
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
        "add" | "sub" | "mul" | "div" | "mod" | "signed_mod" | "int.min" | "int.max"
        | "int.abs_or" | "int.neg_or" | "int.saturating_add" | "int.saturating_sub"
        | "int.saturating_mul" => {
            validate_source_arg_types(func, args, scope, functions, &["Int", "Int"])?;
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

fn is_untyped_source_builtin(func: &str) -> bool {
    matches!(func, "Var" | "fold" | "match")
}

fn infer_source_list_type(
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

fn infer_source_tuple_type(
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

fn infer_source_set_type(
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

fn infer_source_map_type(
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

fn infer_source_record_type(
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

fn infer_source_field_type(
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

fn infer_source_update_type(
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

fn infer_source_index_type(
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

fn infer_source_match_type(
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

fn validate_source_arg_types(
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

fn validate_source_type_match(expected: &str, actual: &str, context: &str) -> Result<(), CliError> {
    if source_type_matches(expected, actual) {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "type mismatch in {context}: expected {expected}, got {actual}"
    )))
}

fn source_type_matches(expected: &str, actual: &str) -> bool {
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

fn validate_source_type_annotation(ty: &str) -> Result<(), CliError> {
    if is_supported_source_type(ty) {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "unsupported source type annotation `{ty}`"
    )))
}

fn validate_source_let_line_marker(line: &str) -> Result<(), CliError> {
    parse_source_let_line_marker(line).map(|_| ())
}

fn parse_source_let_line_marker(line: &str) -> Result<usize, CliError> {
    line.parse::<usize>()
        .map_err(|_| CliError::ParseError(format!("invalid typed let source line marker `{line}`")))
}

fn source_error_at_line(err: CliError, line_num: usize) -> CliError {
    match err {
        CliError::ParseError(message) if !message.contains("line ") => {
            CliError::ParseError(format!("line {line_num}: {message}"))
        }
        other => other,
    }
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

fn source_expr_to_acl_body(expr: &str, constants: &BTreeMap<String, String>) -> String {
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

fn render_source_import(out: &mut String, import: &str) {
    out.push_str(&format!("use \"{import}\"\n"));
}

fn render_source_module(out: &mut String, module: &str) {
    out.push_str(&format!("module {module}\n"));
}

fn render_source_capability(out: &mut String, capability: &str) {
    out.push_str(&format!("capability {capability}\n"));
}

fn render_source_const(
    out: &mut String,
    constant: &SourceConst,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) {
    let name = render_source_decl_name(
        constant.name.strip_prefix("fn.").unwrap_or(&constant.name),
        module,
    );
    out.push_str(&format!(
        "const {name}: {} = {}\n",
        constant.return_type,
        format_source_expr(&constant.body, module, constants)
    ));
}

fn render_source_function(
    out: &mut String,
    function: &SourceFunction,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) {
    let name = render_source_decl_name(
        function.name.strip_prefix("fn.").unwrap_or(&function.name),
        module,
    );
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
            format_source_expr(&function.body, module, constants)
        ));
        return;
    }

    out.push_str(&format!("{signature} {{\n"));
    for binding in lets {
        let annotation = binding
            .ty
            .as_ref()
            .map(|ty| format!(": {ty}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  let {}{} = {}\n",
            binding.name,
            annotation,
            format_source_expr(&binding.value, module, constants)
        ));
    }
    out.push_str(&format!(
        "  return {}\n",
        format_source_expr(&final_expr, module, constants)
    ));
    out.push_str("}\n");
}

fn render_source_test(
    out: &mut String,
    test: &SourceTest,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) {
    let name = render_source_decl_name(
        test.name.strip_prefix("test.").unwrap_or(&test.name),
        module,
    );
    if test.return_type == "Bool" {
        out.push_str(&format!(
            "test {name} = {}\n",
            format_source_expr(&test.body, module, constants)
        ));
    } else {
        out.push_str(&format!(
            "test {name} -> {} = {}\n",
            test.return_type,
            format_source_expr(&test.body, module, constants)
        ));
    }
}

fn render_source_grant(out: &mut String, grant: &SourceGrant, module: Option<&str>) {
    let raw_target = grant
        .target
        .strip_prefix("fn.")
        .or_else(|| grant.target.strip_prefix("test."))
        .unwrap_or(&grant.target);
    let target = render_source_decl_name(raw_target, module);
    out.push_str(&format!("grant {target} {}\n", grant.capability));
}

fn render_source_decl_name(name: &str, module: Option<&str>) -> String {
    module
        .and_then(|module| name.strip_prefix(&format!("{module}.")))
        .unwrap_or(name)
        .to_string()
}

fn format_source_call_name(func: &str, module: Option<&str>) -> String {
    render_source_decl_name(func, module)
}

struct SourceLetBinding {
    name: String,
    ty: Option<String>,
    value: String,
}

fn source_let_chain(body: &str) -> (Vec<SourceLetBinding>, String) {
    let mut lets = Vec::new();
    let mut current = body.trim().to_string();
    while let Some((func, args)) = parse_source_call(&current) {
        match (func.as_str(), args.as_slice()) {
            ("let", [name, value, next]) if is_source_local_ident(name) => {
                lets.push(SourceLetBinding {
                    name: name.clone(),
                    ty: None,
                    value: value.clone(),
                });
                current = next.clone();
            }
            ("let_typed", [name, ty, _line, value, next]) if is_source_local_ident(name) => {
                lets.push(SourceLetBinding {
                    name: name.clone(),
                    ty: Some(ty.clone()),
                    value: value.clone(),
                });
                current = next.clone();
            }
            _ => break,
        }
    }
    (lets, current)
}

fn format_source_expr(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) -> String {
    format_source_expr_node(expr, module, constants).0
}

fn format_source_expr_node(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) -> (String, u8) {
    const IF_PRECEDENCE: u8 = 0;
    const UNARY_PRECEDENCE: u8 = 7;
    const CALL_PRECEDENCE: u8 = 8;

    let expr = expr.trim();
    let Some((func, args)) = parse_source_call(expr) else {
        return (expr.to_string(), CALL_PRECEDENCE);
    };

    if args.is_empty()
        && let Some(target) = source_const_reference_target(&func, constants)
    {
        return (render_source_decl_name(&target, module), CALL_PRECEDENCE);
    }

    if func == "if" && args.len() == 3 {
        if let Some((list, fallback)) = source_if_as_first_or(&args) {
            return (
                format!(
                    "first_or({}, {})",
                    format_source_expr(&list, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((list, fallback)) = source_if_as_last_or(&args) {
            return (
                format!(
                    "last_or({}, {})",
                    format_source_expr(&list, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((list, index, fallback)) = source_if_as_get_or(&args) {
            return (
                format!(
                    "get_or({}, {}, {})",
                    format_source_expr(&list, module, constants),
                    format_source_expr(&index, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        return (
            format!(
                "if {} {{ {} }} else {{ {} }}",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            IF_PRECEDENCE,
        );
    }

    if func == "match" && args.len() >= 3 && args.len() % 2 == 1 {
        if let Some((helper, value)) = source_match_as_option_predicate(&args) {
            return (
                format!(
                    "{helper}({})",
                    format_source_expr(&value, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((helper, value)) = source_match_as_result_predicate(&args) {
            return (
                format!(
                    "{helper}({})",
                    format_source_expr(&value, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        if let Some((value, fallback)) = source_match_as_unwrap_or(&args) {
            return (
                format!(
                    "unwrap_or({}, {})",
                    format_source_expr(&value, module, constants),
                    format_source_expr(&fallback, module, constants)
                ),
                CALL_PRECEDENCE,
            );
        }
        return (
            format_source_match_expr(&args, module, constants),
            IF_PRECEDENCE,
        );
    }

    if func == "none" && args.is_empty() {
        return ("None".to_string(), CALL_PRECEDENCE);
    }

    if matches!(func.as_str(), "some" | "ok" | "err") && args.len() == 1 {
        let constructor = match func.as_str() {
            "some" => "Some",
            "ok" => "Ok",
            "err" => "Err",
            _ => unreachable!("checked source constructor"),
        };
        return (
            format!(
                "{constructor}({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "not" && args.len() == 1 {
        return (
            format!(
                "!{}",
                format_source_child_expr(&args[0], module, constants, UNARY_PRECEDENCE, false)
            ),
            UNARY_PRECEDENCE,
        );
    }

    if func == "sub" && args.len() == 2 && args[0].trim() == "0" {
        return (
            format!(
                "-{}",
                format_source_child_expr(&args[1], module, constants, UNARY_PRECEDENCE, false)
            ),
            UNARY_PRECEDENCE,
        );
    }

    if func == "eq"
        && args.len() == 2
        && let Some(value) = source_eq_as_is_empty(&args)
    {
        return (
            format!(
                "is_empty({})",
                format_source_expr(&value, module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if matches!(func.as_str(), "int.min" | "int.max") && args.len() == 2 {
        let helper = if func == "int.min" {
            "int_min"
        } else {
            "int_max"
        };
        return (
            format!(
                "{helper}({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.abs_or" && args.len() == 2 {
        return (
            format!(
                "int_abs_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.neg_or" && args.len() == 2 {
        return (
            format!(
                "int_neg_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.saturating_add" && args.len() == 2 {
        return (
            format!(
                "int_saturating_add({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.saturating_sub" && args.len() == 2 {
        return (
            format!(
                "int_saturating_sub({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.saturating_mul" && args.len() == 2 {
        return (
            format!(
                "int_saturating_mul({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.clamp" && args.len() == 3 {
        return (
            format!(
                "int_clamp({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.add_or" && args.len() == 3 {
        return (
            format!(
                "int_add_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.sub_or" && args.len() == 3 {
        return (
            format!(
                "int_sub_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.mul_or" && args.len() == 3 {
        return (
            format!(
                "int_mul_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.div_or" && args.len() == 3 {
        return (
            format!(
                "int_div_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "int.rem_or" && args.len() == 3 {
        return (
            format!(
                "int_rem_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.eq" && args.len() == 2 {
        return (
            format!(
                "text_eq({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.trim" && args.len() == 1 {
        return (
            format!(
                "text_trim({})",
                format_source_expr(&args[0], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.contains" && args.len() == 2 {
        return (
            format!(
                "text_contains({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.index_of" && args.len() == 2 {
        return (
            format!(
                "text_index_of({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.parse_int_or" && args.len() == 2 {
        return (
            format!(
                "text_parse_int_or({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.byte_at_or" && args.len() == 3 {
        return (
            format!(
                "text_byte_at_or({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.slice" && args.len() == 3 {
        return (
            format!(
                "text_slice({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.replace_first" && args.len() == 3 {
        return (
            format!(
                "text_replace_first({}, {}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants),
                format_source_expr(&args[2], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.starts_with" && args.len() == 2 {
        return (
            format!(
                "text_starts_with({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "text.ends_with" && args.len() == 2 {
        return (
            format!(
                "text_ends_with({}, {})",
                format_source_expr(&args[0], module, constants),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "list" {
        return (
            format!(
                "[{}]",
                args.iter()
                    .map(|arg| format_source_expr(arg, module, constants))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "record"
        && args.len().is_multiple_of(2)
        && args
            .chunks_exact(2)
            .all(|pair| is_source_local_ident(&pair[0]))
    {
        let fields = args
            .chunks_exact(2)
            .map(|pair| {
                format!(
                    "{}: {}",
                    pair[0].trim(),
                    format_source_expr(&pair[1], module, constants)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return (
            if fields.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {fields} }}")
            },
            CALL_PRECEDENCE,
        );
    }

    if func == "index" && args.len() == 2 {
        return (
            format!(
                "{}[{}]",
                format_source_child_expr(&args[0], module, constants, CALL_PRECEDENCE, false),
                format_source_expr(&args[1], module, constants)
            ),
            CALL_PRECEDENCE,
        );
    }

    if func == "field" && args.len() == 2 {
        return (
            format!(
                "{}.{}",
                format_source_child_expr(&args[0], module, constants, CALL_PRECEDENCE, false),
                args[1].trim()
            ),
            CALL_PRECEDENCE,
        );
    }

    if let Some((base, fields)) = collect_source_update_literal(expr) {
        let mut parts = vec![format!(
            "...{}",
            format_source_expr(&base, module, constants)
        )];
        parts.extend(fields.iter().map(|(field, value)| {
            format!("{field}: {}", format_source_expr(value, module, constants))
        }));
        return (format!("{{ {} }}", parts.join(", ")), CALL_PRECEDENCE);
    }

    if args.len() == 2 {
        if let Some((operator, precedence)) = source_infix_operator(&func) {
            return (
                format!(
                    "{} {operator} {}",
                    format_source_child_expr(&args[0], module, constants, precedence, false),
                    format_source_child_expr(&args[1], module, constants, precedence, true)
                ),
                precedence,
            );
        }
    }

    (
        format!(
            "{}({})",
            format_source_call_name(&func, module),
            args.iter()
                .map(|arg| format_source_expr(arg, module, constants))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CALL_PRECEDENCE,
    )
}

fn source_if_as_first_or(args: &[String]) -> Option<(String, String)> {
    if args.len() != 3 {
        return None;
    }
    let (cond_func, cond_args) = parse_source_call(&args[0])?;
    if cond_func != "gt" || cond_args.len() != 2 || cond_args[1].trim() != "0" {
        return None;
    }
    let (len_func, len_args) = parse_source_call(&cond_args[0])?;
    if len_func != "len" || len_args.len() != 1 {
        return None;
    }
    let list = len_args[0].trim();
    let (index_func, index_args) = parse_source_call(&args[1])?;
    if index_func != "index"
        || index_args.len() != 2
        || index_args[0].trim() != list
        || index_args[1].trim() != "0"
    {
        return None;
    }
    Some((list.to_string(), args[2].trim().to_string()))
}

fn source_if_as_last_or(args: &[String]) -> Option<(String, String)> {
    if args.len() != 3 {
        return None;
    }
    let (cond_func, cond_args) = parse_source_call(&args[0])?;
    if cond_func != "gt" || cond_args.len() != 2 || cond_args[1].trim() != "0" {
        return None;
    }
    let (len_func, len_args) = parse_source_call(&cond_args[0])?;
    if len_func != "len" || len_args.len() != 1 {
        return None;
    }
    let list = len_args[0].trim();
    let (index_func, index_args) = parse_source_call(&args[1])?;
    if index_func != "index" || index_args.len() != 2 || index_args[0].trim() != list {
        return None;
    }
    let (sub_func, sub_args) = parse_source_call(&index_args[1])?;
    if sub_func != "sub" || sub_args.len() != 2 || sub_args[1].trim() != "1" {
        return None;
    }
    let (idx_len_func, idx_len_args) = parse_source_call(&sub_args[0])?;
    if idx_len_func != "len" || idx_len_args.len() != 1 || idx_len_args[0].trim() != list {
        return None;
    }
    Some((list.to_string(), args[2].trim().to_string()))
}

fn source_if_as_get_or(args: &[String]) -> Option<(String, String, String)> {
    if args.len() != 3 {
        return None;
    }
    let (then_func, then_args) = parse_source_call(&args[1])?;
    if then_func != "index" || then_args.len() != 2 {
        return None;
    }
    let list = then_args[0].trim();
    let index = then_args[1].trim();
    let (cond_list, cond_index) = source_get_or_guard_parts(&args[0])?;
    if cond_list.trim() != list || cond_index.trim() != index {
        return None;
    }
    Some((
        list.to_string(),
        index.to_string(),
        args[2].trim().to_string(),
    ))
}

fn source_get_or_guard_parts(cond: &str) -> Option<(String, String)> {
    let (func, args) = parse_source_call(cond)?;
    if func != "and" || args.len() != 2 {
        return None;
    }
    let ge = source_get_or_ge_zero(&args[0]).or_else(|| source_get_or_ge_zero(&args[1]))?;
    let (lt_index, lt_list) =
        source_get_or_lt_len(&args[0]).or_else(|| source_get_or_lt_len(&args[1]))?;
    if ge.trim() == lt_index.trim() {
        Some((lt_list, ge))
    } else {
        None
    }
}

fn source_get_or_ge_zero(expr: &str) -> Option<String> {
    let (func, args) = parse_source_call(expr)?;
    if func == "ge" && args.len() == 2 && args[1].trim() == "0" {
        Some(args[0].trim().to_string())
    } else {
        None
    }
}

fn source_get_or_lt_len(expr: &str) -> Option<(String, String)> {
    let (func, args) = parse_source_call(expr)?;
    if func != "lt" || args.len() != 2 {
        return None;
    }
    let (len_func, len_args) = parse_source_call(&args[1])?;
    if len_func == "len" && len_args.len() == 1 {
        Some((args[0].trim().to_string(), len_args[0].trim().to_string()))
    } else {
        None
    }
}

fn source_eq_as_is_empty(args: &[String]) -> Option<String> {
    if args.len() != 2 {
        return None;
    }
    source_len_zero_arg(&args[0], &args[1]).or_else(|| source_len_zero_arg(&args[1], &args[0]))
}

fn source_len_zero_arg(len_expr: &str, zero_expr: &str) -> Option<String> {
    if zero_expr.trim() != "0" {
        return None;
    }
    let (func, args) = parse_source_call(len_expr.trim())?;
    if func == "len" && args.len() == 1 {
        Some(args[0].trim().to_string())
    } else {
        None
    }
}

fn source_match_as_option_predicate(args: &[String]) -> Option<(&'static str, String)> {
    if args.len() != 5 {
        return None;
    }
    let scrutinee = args[0].trim().to_string();
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| (pair[0].trim(), pair[1].trim()))
        .collect::<Vec<_>>();

    let mut some_body = None;
    let mut none_body = None;
    for (pattern, body) in arms {
        match source_constructor_pattern(pattern) {
            Some(("Some", Some("_"))) => some_body = Some(body),
            Some(("None", None)) => none_body = Some(body),
            _ => return None,
        }
    }

    match (some_body?, none_body?) {
        ("true", "false") => Some(("is_some", scrutinee)),
        ("false", "true") => Some(("is_none", scrutinee)),
        _ => None,
    }
}

fn source_match_as_result_predicate(args: &[String]) -> Option<(&'static str, String)> {
    if args.len() != 5 {
        return None;
    }
    let scrutinee = args[0].trim().to_string();
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| (pair[0].trim(), pair[1].trim()))
        .collect::<Vec<_>>();

    let mut ok_body = None;
    let mut err_body = None;
    for (pattern, body) in arms {
        match source_constructor_pattern(pattern) {
            Some(("Ok", Some("_"))) => ok_body = Some(body),
            Some(("Err", Some("_"))) => err_body = Some(body),
            _ => return None,
        }
    }

    match (ok_body?, err_body?) {
        ("true", "false") => Some(("is_ok", scrutinee)),
        ("false", "true") => Some(("is_err", scrutinee)),
        _ => None,
    }
}

fn source_match_as_unwrap_or(args: &[String]) -> Option<(String, String)> {
    if args.len() != 5 {
        return None;
    }
    let scrutinee = args[0].trim().to_string();
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| (pair[0].trim(), pair[1].trim()))
        .collect::<Vec<_>>();

    let mut success = None;
    let mut fallback = None;
    for (pattern, body) in arms {
        match source_constructor_pattern(pattern) {
            Some(("Some", Some(binding))) if body == binding => success = Some(()),
            Some(("None", None)) => fallback = Some(body.to_string()),
            _ => return None,
        }
    }
    if success.is_some() {
        Some((scrutinee, fallback?))
    } else {
        None
    }
}

fn collect_source_update_literal(expr: &str) -> Option<(String, Vec<(String, String)>)> {
    let (func, args) = parse_source_call(expr)?;
    if func != "update" || args.len() != 3 || !is_source_local_ident(&args[1]) {
        return None;
    }
    let field = args[1].trim().to_string();
    let value = args[2].trim().to_string();
    if let Some((base, mut fields)) = collect_source_update_literal(&args[0]) {
        fields.push((field, value));
        Some((base, fields))
    } else {
        Some((args[0].trim().to_string(), vec![(field, value)]))
    }
}

fn source_infix_operator(func: &str) -> Option<(&'static str, u8)> {
    match func {
        "or" => Some(("||", 1)),
        "and" => Some(("&&", 2)),
        "eq" => Some(("==", 3)),
        "ne" => Some(("!=", 3)),
        "gt" => Some((">", 4)),
        "ge" => Some((">=", 4)),
        "lt" => Some(("<", 4)),
        "le" => Some(("<=", 4)),
        "concat" => Some(("++", 5)),
        "add" => Some(("+", 5)),
        "sub" => Some(("-", 5)),
        "mul" => Some(("*", 6)),
        "div" => Some(("/", 6)),
        "mod" => Some(("%", 6)),
        _ => None,
    }
}

fn format_source_match_expr(
    args: &[String],
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) -> String {
    let arms = args[1..]
        .chunks_exact(2)
        .map(|pair| {
            format!(
                "{} => {}",
                pair[0].trim(),
                format_source_expr(&pair[1], module, constants)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "match {} {{ {arms} }}",
        format_source_expr(&args[0], module, constants)
    )
}

fn format_source_child_expr(
    expr: &str,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
    parent_precedence: u8,
    parenthesize_equal_precedence: bool,
) -> String {
    let (formatted, precedence) = format_source_expr_node(expr, module, constants);
    if precedence < parent_precedence
        || (parenthesize_equal_precedence && precedence == parent_precedence)
    {
        format!("({formatted})")
    } else {
        formatted
    }
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
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
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
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ',' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
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

fn matching_bracket(s: &str, open_idx: usize) -> Option<usize> {
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
            '[' => depth += 1,
            ']' => {
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
    !name.is_empty() && is_valid_source_name_chars(name) && source_name_segments_are_valid(name)
}

fn is_source_local_ident(name: &str) -> bool {
    is_source_ident(name) && !name.contains('.')
}

fn validate_source_local_expr_name(name: &str) -> Result<(), CliError> {
    if !is_source_ident(name) {
        return Err(CliError::ParseError(format!(
            "local binding name `{name}` is not a valid identifier"
        )));
    }
    if name.contains('.') {
        return Err(CliError::ParseError(format!(
            "local binding name `{name}` must not contain `.`"
        )));
    }
    Ok(())
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

fn parse_source_module(rest: &str, line_num: usize) -> Result<String, CliError> {
    let module = rest.trim();
    validate_source_name(module, line_num)?;
    Ok(module.to_string())
}

fn parse_source_import(rest: &str, line_num: usize) -> Result<String, CliError> {
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

fn parse_source_capability(rest: &str, line_num: usize) -> Result<String, CliError> {
    let capability = rest.trim();
    validate_source_name(capability, line_num)?;
    Ok(capability.to_string())
}

fn parse_source_const(rest: &str, line_num: usize) -> Result<SourceConst, CliError> {
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

fn parse_source_params(params: &str, line_num: usize) -> Result<Vec<SourceParam>, CliError> {
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

fn lower_source_expr(expr: &str, line_num: usize) -> Result<String, CliError> {
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
    if let Some(lowered) = lower_source_constructor_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_unwrap_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_option_predicate_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_result_predicate_expr(expr, line_num)? {
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
            return Err(CliError::ParseError(format!(
                "line {line_num}: unary `-` requires an expression"
            )));
        }
        return Ok(format!("sub(0, {})", lower_source_expr(inner, line_num)?));
    }
    if let Some(inner) = expr.strip_prefix('!') {
        let inner = inner.trim();
        if inner.is_empty() || inner.starts_with('=') {
            return Err(CliError::ParseError(format!(
                "line {line_num}: unary `!` requires an expression"
            )));
        }
        return Ok(format!("not({})", lower_source_expr(inner, line_num)?));
    }
    Ok(expr.to_string())
}

fn lower_source_constructor_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    if expr == "None" || expr == "None()" {
        return Ok(Some("none()".to_string()));
    }
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func == "None" {
        if args.is_empty() {
            return Ok(Some("none()".to_string()));
        }
        return Err(CliError::ParseError(format!(
            "line {line_num}: source constructor `None` requires no values"
        )));
    }
    let lowered_func = match func.as_str() {
        "Some" => "some",
        "Ok" => "ok",
        "Err" => "err",
        _ => return Ok(None),
    };
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: source constructor `{func}` requires exactly one value"
        )));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        lower_source_expr(&args[0], line_num)?
    )))
}

fn lower_source_option_predicate_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let (some_body, none_body) = match func.as_str() {
        "is_some" => ("true", "false"),
        "is_none" => ("false", "true"),
        _ => return Ok(None),
    };
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{func}(option)`"
        )));
    }
    Ok(Some(format!(
        "match({}, Some(_), {some_body}, None, {none_body})",
        lower_source_expr(&args[0], line_num)?
    )))
}

fn lower_source_result_predicate_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let (ok_body, err_body) = match func.as_str() {
        "is_ok" => ("true", "false"),
        "is_err" => ("false", "true"),
        _ => return Ok(None),
    };
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{func}(result)`"
        )));
    }
    Ok(Some(format!(
        "match({}, Ok(_), {ok_body}, Err(_), {err_body})",
        lower_source_expr(&args[0], line_num)?
    )))
}

fn lower_source_is_empty_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "is_empty" {
        return Ok(None);
    }
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: is_empty requires `is_empty(value)`"
        )));
    }
    Ok(Some(format!(
        "eq(len({}), 0)",
        lower_source_expr(&args[0], line_num)?
    )))
}

fn lower_source_int_bounds_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let (lowered_func, expected) = match func.as_str() {
        "int_min" => ("int.min", "int_min(left, right)"),
        "int_max" => ("int.max", "int_max(left, right)"),
        "int_clamp" => ("int.clamp", "int_clamp(value, low, high)"),
        "int_add_or" => ("int.add_or", "int_add_or(left, right, fallback)"),
        "int_sub_or" => ("int.sub_or", "int_sub_or(left, right, fallback)"),
        "int_mul_or" => ("int.mul_or", "int_mul_or(left, right, fallback)"),
        "int_saturating_add" => ("int.saturating_add", "int_saturating_add(left, right)"),
        "int_saturating_sub" => ("int.saturating_sub", "int_saturating_sub(left, right)"),
        "int_saturating_mul" => ("int.saturating_mul", "int_saturating_mul(left, right)"),
        "int_abs_or" => ("int.abs_or", "int_abs_or(value, fallback)"),
        "int_neg_or" => ("int.neg_or", "int_neg_or(value, fallback)"),
        "int_div_or" => ("int.div_or", "int_div_or(value, divisor, fallback)"),
        "int_rem_or" => ("int.rem_or", "int_rem_or(value, divisor, fallback)"),
        _ => return Ok(None),
    };
    let expected_len = if matches!(
        func.as_str(),
        "int_clamp" | "int_add_or" | "int_sub_or" | "int_mul_or" | "int_div_or" | "int_rem_or"
    ) {
        3
    } else {
        2
    };
    if args.len() != expected_len {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{expected}`"
        )));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

fn lower_source_text_eq_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_eq" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_eq requires `text_eq(left, right)`"
        )));
    }
    Ok(Some(format!(
        "text.eq({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

fn lower_source_text_trim_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_trim" {
        return Ok(None);
    }
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_trim requires `text_trim(value)`"
        )));
    }
    Ok(Some(format!(
        "text.trim({})",
        lower_source_expr(&args[0], line_num)?
    )))
}

fn lower_source_text_contains_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_contains" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_contains requires `text_contains(haystack, needle)`"
        )));
    }
    Ok(Some(format!(
        "text.contains({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

fn lower_source_text_index_of_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_index_of" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_index_of requires `text_index_of(haystack, needle)`"
        )));
    }
    Ok(Some(format!(
        "text.index_of({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

fn lower_source_text_parse_int_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_parse_int_or" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_parse_int_or requires `text_parse_int_or(value, fallback)`"
        )));
    }
    Ok(Some(format!(
        "text.parse_int_or({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

fn lower_source_text_byte_at_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_byte_at_or" {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_byte_at_or requires `text_byte_at_or(value, index, fallback)`"
        )));
    }
    Ok(Some(format!(
        "text.byte_at_or({}, {}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?,
        lower_source_expr(&args[2], line_num)?
    )))
}

fn lower_source_text_slice_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_slice" {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_slice requires `text_slice(value, start, length)`"
        )));
    }
    Ok(Some(format!(
        "text.slice({}, {}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?,
        lower_source_expr(&args[2], line_num)?
    )))
}

fn lower_source_text_replace_first_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_replace_first" {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_replace_first requires `text_replace_first(value, needle, replacement)`"
        )));
    }
    Ok(Some(format!(
        "text.replace_first({}, {}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?,
        lower_source_expr(&args[2], line_num)?
    )))
}

fn lower_source_text_boundary_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let (lowered_func, expected) = match func.as_str() {
        "text_starts_with" => ("text.starts_with", "text_starts_with(haystack, prefix)"),
        "text_ends_with" => ("text.ends_with", "text_ends_with(haystack, suffix)"),
        _ => return Ok(None),
    };
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{expected}`"
        )));
    }
    Ok(Some(format!(
        "{lowered_func}({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

fn lower_source_first_or_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "first_or" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: first_or requires `first_or(list, fallback)`"
        )));
    }
    let list = lower_source_expr(&args[0], line_num)?;
    Ok(Some(format!(
        "if(gt(len({list}), 0), index({list}, 0), {})",
        lower_source_expr(&args[1], line_num)?
    )))
}

fn lower_source_last_or_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "last_or" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: last_or requires `last_or(list, fallback)`"
        )));
    }
    let list = lower_source_expr(&args[0], line_num)?;
    Ok(Some(format!(
        "if(gt(len({list}), 0), index({list}, sub(len({list}), 1)), {})",
        lower_source_expr(&args[1], line_num)?
    )))
}

fn lower_source_get_or_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "get_or" {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: get_or requires `get_or(list, index, fallback)`"
        )));
    }
    let list = lower_source_expr(&args[0], line_num)?;
    let index = lower_source_expr(&args[1], line_num)?;
    Ok(Some(format!(
        "if(and(ge({index}, 0), lt({index}, len({list}))), index({list}, {index}), {})",
        lower_source_expr(&args[2], line_num)?
    )))
}

fn lower_source_unwrap_or_expr(expr: &str, line_num: usize) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "unwrap_or" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: unwrap_or requires `unwrap_or(value, fallback)`"
        )));
    }
    Ok(Some(format!(
        "match({}, Some(__ail_unwrap), __ail_unwrap, None, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

fn strip_wrapping_source_parens(expr: &str) -> Option<&str> {
    if !expr.starts_with('(') || !expr.ends_with(')') {
        return None;
    }
    (matching_paren(expr, 0)? == expr.len() - 1).then(|| expr[1..expr.len() - 1].trim())
}

fn split_top_level_source_binary_str<'a>(expr: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
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
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && idx > 0
                && expr[idx..].starts_with(op) =>
            {
                split_at = Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let idx = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

fn split_top_level_source_binary_any<'a>(
    expr: &'a str,
    ops: &[char],
) -> Option<(&'a str, char, &'a str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
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
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if ops.contains(&ch)
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && source_binary_char_has_left_operand(expr, idx) =>
            {
                split_at = Some((idx, ch));
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let (idx, op) = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len_utf8()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, op, right))
}

fn source_binary_char_has_left_operand(expr: &str, idx: usize) -> bool {
    expr[..idx]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ')' | '}' | ']' | '"'))
}

fn split_top_level_source_binary(expr: &str, op: char) -> Option<(&str, &str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
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
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if ch == op
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && idx > 0 =>
            {
                split_at = Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let idx = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len_utf8()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

fn parse_source_list_literal(expr: &str, line_num: usize) -> Result<Option<Vec<String>>, CliError> {
    if !expr.starts_with('[') {
        return Ok(None);
    }
    if !expr.ends_with(']') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: list literal has unclosed `[`"
        )));
    }
    if matching_bracket(expr, 0) != Some(expr.len() - 1) {
        return Ok(None);
    }
    let inner = expr[1..expr.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Some(vec![]));
    }
    Ok(Some(split_source_args(inner)))
}

struct SourceRecordLiteral {
    base: Option<String>,
    fields: Vec<(String, String)>,
}

fn parse_source_record_literal(
    expr: &str,
    line_num: usize,
) -> Result<Option<SourceRecordLiteral>, CliError> {
    if !expr.starts_with('{') {
        return Ok(None);
    }
    let close = matching_brace(expr, 0);
    if close.is_none() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: record literal has unclosed `{{`"
        )));
    }
    if close != Some(expr.len() - 1) {
        return Ok(None);
    }
    let inner = expr[1..expr.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Some(SourceRecordLiteral {
            base: None,
            fields: vec![],
        }));
    }

    let mut base = None;
    let mut fields = Vec::new();
    for (entry_idx, field_expr) in split_source_args(inner).into_iter().enumerate() {
        let entry = field_expr.trim();
        if let Some(spread) = entry.strip_prefix("...") {
            if entry_idx != 0 || base.is_some() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: record update spread must appear first"
                )));
            }
            let spread = spread.trim();
            if spread.is_empty() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: record update spread requires a base expression"
                )));
            }
            base = Some(spread.to_string());
            continue;
        }

        let colon = find_top_level_source_colon(entry).ok_or_else(|| {
            CliError::ParseError(format!(
                "line {line_num}: record literal field requires `name: expression`"
            ))
        })?;
        let field = entry[..colon].trim();
        let value = entry[colon + ':'.len_utf8()..].trim();
        if field.is_empty() || value.is_empty() {
            return Err(CliError::ParseError(format!(
                "line {line_num}: record literal field and value must be non-empty"
            )));
        }
        validate_source_local_name(field, line_num)?;
        fields.push((field.to_string(), value.to_string()));
    }

    Ok(Some(SourceRecordLiteral { base, fields }))
}

fn find_top_level_source_colon(expr: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in expr.char_indices() {
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
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ':' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                return Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    None
}

fn parse_source_index_expr<'a>(
    expr: &'a str,
    line_num: usize,
) -> Result<Option<(&'a str, &'a str)>, CliError> {
    if !expr.ends_with(']') || expr.starts_with('[') {
        return Ok(None);
    }
    let open = find_top_level_source_index_bracket(expr);
    let Some(open) = open else {
        return Ok(None);
    };
    if matching_bracket(expr, open) != Some(expr.len() - 1) {
        return Ok(None);
    }
    let collection = expr[..open].trim();
    let index = expr[open + 1..expr.len() - 1].trim();
    if collection.is_empty() || index.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: index expression requires `collection[index]`"
        )));
    }
    Ok(Some((collection, index)))
}

fn parse_source_dot_field_expr<'a>(
    expr: &'a str,
    line_num: usize,
) -> Result<Option<(&'a str, &'a str)>, CliError> {
    let Some(dot) = find_top_level_source_dot(expr) else {
        return Ok(None);
    };
    let record = expr[..dot].trim();
    let field = expr[dot + 1..].trim();
    if record.is_empty() || field.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: field access requires `record.field`"
        )));
    }
    if !is_source_local_ident(field) {
        return Ok(None);
    }
    Ok(Some((record, field)))
}

fn find_top_level_source_dot(expr: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut candidate = None;

    for (idx, ch) in expr.char_indices() {
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
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '.' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && source_dot_has_record_operand(expr, idx) =>
            {
                candidate = Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    candidate
}

fn source_dot_has_record_operand(expr: &str, idx: usize) -> bool {
    expr[..idx]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ')' | '}' | ']' | '"'))
}

fn find_top_level_source_index_bracket(expr: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut candidate = None;

    for (idx, ch) in expr.char_indices() {
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
            '[' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                candidate = Some(idx);
                bracket_depth += 1;
            }
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {}
        }
        prev_was_escape = false;
    }

    candidate
}

fn lower_match_expr(rest: &str, line_num: usize) -> Result<String, CliError> {
    let open = rest.find('{').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: match expression requires `match value {{ Pattern => expr, ... }}`"
        ))
    })?;
    let scrutinee = rest[..open].trim();
    if scrutinee.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: match expression requires a scrutinee"
        )));
    }
    let close = matching_brace(rest, open).ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: match expression has unclosed arm block"
        ))
    })?;
    if !rest[close + 1..].trim().is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: unexpected tokens after match expression"
        )));
    }
    let arms = rest[open + 1..close].trim();
    if arms.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: match expression requires at least one arm"
        )));
    }

    let mut lowered = vec![lower_source_expr(scrutinee, line_num)?];
    for arm in split_source_match_arms(arms) {
        let (pattern, body) = split_source_match_arm(arm, line_num)?;
        let pattern = normalize_source_match_pattern(pattern, line_num)?;
        lowered.push(pattern);
        lowered.push(lower_source_expr(body, line_num)?);
    }
    Ok(format!("match({})", lowered.join(", ")))
}

fn split_source_match_arms(arms: &str) -> Vec<String> {
    split_source_args(arms)
}

fn split_source_match_arm<'a>(
    arm: &'a str,
    line_num: usize,
) -> Result<(&'a str, &'a str), CliError> {
    let Some(idx) = find_top_level_source_arrow(arm) else {
        return Err(CliError::ParseError(format!(
            "line {line_num}: match arm requires `Pattern => expression`"
        )));
    };
    let pattern = arm[..idx].trim();
    let body = arm[idx + 2..].trim();
    if pattern.is_empty() || body.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: match arm pattern and body must be non-empty"
        )));
    }
    Ok((pattern, body))
}

fn find_top_level_source_arrow(input: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in input.char_indices() {
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
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '=' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && input[idx..].starts_with("=>") =>
            {
                return Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
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
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);

    Ok(SourceTest {
        name: normalize_test_name(raw_name),
        return_type,
        body: lower_source_expr(body, line_num)?,
        line_num,
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

fn validate_source_type_name(ty: &str, line_num: usize) -> Result<(), CliError> {
    if is_supported_source_type(ty) {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "line {line_num}: unsupported source type `{ty}`"
    )))
}

fn is_supported_source_type(ty: &str) -> bool {
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

fn source_primitive_type_alias(ty: &str) -> Option<&'static str> {
    match ty.trim() {
        "Int" | "int" | "i32" | "i64" => Some("Int"),
        "Bool" | "bool" => Some("Bool"),
        "Text" | "String" | "str" => Some("Text"),
        "Float" | "float" | "f64" => Some("Float"),
        _ => None,
    }
}

fn source_list_element_type(ty: &str) -> Option<&str> {
    let inner = ty.trim().strip_prefix("List<")?.strip_suffix('>')?.trim();
    (!inner.is_empty()).then_some(inner)
}

fn source_tuple_types(ty: &str) -> Option<Vec<&str>> {
    let inner = ty.trim().strip_prefix("Tuple<")?.strip_suffix('>')?.trim();
    Some(split_source_type_args(inner))
}

fn source_set_element_type(ty: &str) -> Option<&str> {
    let inner = ty.trim().strip_prefix("Set<")?.strip_suffix('>')?.trim();
    (!inner.is_empty()).then_some(inner)
}

fn source_map_types(ty: &str) -> Option<(&str, &str)> {
    let inner = ty.trim().strip_prefix("Map<")?.strip_suffix('>')?.trim();
    let parts = split_source_type_args(inner);
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0], parts[1]))
}

fn source_record_fields(ty: &str) -> Option<Vec<(&str, &str)>> {
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

fn source_option_element_type(ty: &str) -> Option<&str> {
    let inner = ty.trim().strip_prefix("Option<")?.strip_suffix('>')?.trim();
    (!inner.is_empty()).then_some(inner)
}

fn source_result_types(ty: &str) -> Option<(&str, &str)> {
    let inner = ty.trim().strip_prefix("Result<")?.strip_suffix('>')?.trim();
    let parts = split_source_type_args(inner);
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0], parts[1]))
}

fn validate_source_match_pattern(pattern: &str) -> Result<(), CliError> {
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

fn validate_source_constructor_tag(tag: &str) -> Result<(), CliError> {
    if matches!(tag, "Some" | "None" | "Ok" | "Err") {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "unsupported source match constructor `{tag}`"
    )))
}

fn source_match_pattern_binding(pattern: &str) -> Option<&str> {
    let (_, binding) = source_constructor_pattern(pattern)?;
    let binding = binding?;
    (binding != "_").then_some(binding)
}

fn source_match_pattern_binding_type<'a>(
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

fn validate_source_match_pattern_type(pattern: &str, scrutinee_ty: &str) -> Result<(), CliError> {
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

fn validate_source_match_reachable(arms: &[String]) -> Result<(), CliError> {
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

fn validate_source_match_exhaustive(arms: &[String], scrutinee_ty: &str) -> Result<(), CliError> {
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

fn source_match_pattern_key(pattern: &str) -> String {
    source_pattern_tag(pattern)
        .map(ToString::to_string)
        .unwrap_or_else(|| pattern.trim().to_string())
}

fn source_pattern_tag(pattern: &str) -> Option<&str> {
    if pattern.trim() == "None" {
        return Some("None");
    }
    source_constructor_pattern(pattern).map(|(tag, _)| tag)
}

fn source_constructor_pattern(pattern: &str) -> Option<(&str, Option<&str>)> {
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

fn normalize_source_match_pattern(pattern: &str, line_num: usize) -> Result<String, CliError> {
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

fn split_source_type_args(args: &str) -> Vec<&str> {
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

fn split_source_param_list(params: &str) -> Vec<&str> {
    split_source_top_level_commas(params)
}

fn normalize_source_type_name(ty: &str) -> String {
    let compact = ty
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    normalize_source_type_aliases(&compact)
}

fn normalize_source_type_aliases(ty: &str) -> String {
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

fn split_source_top_level_commas(input: &str) -> Vec<&str> {
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

fn normalize_grant_target(target: &str) -> String {
    target.to_string()
}

fn validate_source_local_name(name: &str, line_num: usize) -> Result<(), CliError> {
    validate_source_name(name, line_num)?;
    if name.contains('.') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: local binding name `{name}` must not contain `.`"
        )));
    }
    Ok(())
}

fn validate_source_name(name: &str, line_num: usize) -> Result<(), CliError> {
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

fn is_valid_source_name_chars(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn source_name_segments_are_valid(name: &str) -> bool {
    !name.split('.').any(str::is_empty) && first_invalid_source_name_segment(name).is_none()
}

fn first_invalid_source_name_segment(name: &str) -> Option<&str> {
    name.split('.').find(|segment| {
        !segment
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
    })
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

fn source_default_entry(program: &SourceProgram) -> String {
    program
        .module
        .as_deref()
        .map(|module| format!("fn.{module}.main"))
        .unwrap_or_else(|| "fn.main".to_string())
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
    fn lowers_source_consts_to_zero_arg_functions() {
        let program = parse_ail_source(
            "const answer: Int = 40 + 2\nfn main() -> Int = answer\ntest answer = answer == 42",
        )
        .expect("source must parse");
        let acl = source_program_to_acl(&program, "source_const".to_string());

        assert_eq!(program.constants[0].name, "fn.answer");
        assert!(acl.contains("op create_function id=fn.answer return=Int body=add(40, 2)"));
        assert!(acl.contains("op create_function id=fn.main return=Int body=answer()"));
        assert!(acl.contains("op create_test id=test.answer return=Bool body=eq(answer(), 42)"));
    }

    #[test]
    fn lowers_source_set_and_map_collections() {
        let program = parse_ail_source(
            r#"
fn ids() -> Set<Int> = set(1, 2 + 3)
fn labels() -> Map<Text, Int> = map("one", 1, "two", 2)
"#,
        )
        .expect("source set/map must parse");
        let acl = source_program_to_acl(&program, "source_collections".to_string());

        assert_eq!(program.functions[0].return_type, "Set<Int>");
        assert_eq!(program.functions[1].return_type, "Map<Text,Int>");
        assert!(
            acl.contains("op create_function id=fn.ids return=Set<Int> body=set(1, add(2, 3))")
        );
        assert!(acl.contains(
            r#"op create_function id=fn.labels return=Map<Text,Int> body=map("one", 1, "two", 2)"#
        ));
    }

    #[test]
    fn lowers_source_tuple_collections() {
        let program = parse_ail_source(
            r#"
fn pair() -> Tuple<Int, Text> = tuple(42, "answer")
"#,
        )
        .expect("source tuple must parse");
        let acl = source_program_to_acl(&program, "source_tuple".to_string());

        assert_eq!(program.functions[0].return_type, "Tuple<Int,Text>");
        assert!(acl.contains(
            r#"op create_function id=fn.pair return=Tuple<Int,Text> body=tuple(42, "answer")"#
        ));
    }

    #[test]
    fn lowers_source_record_field_access_and_update() {
        let program = parse_ail_source(
            r#"
fn person() -> Record<age: Int, name: Text> = { age: 42, name: "Ada" }
fn age() -> Int = field(person(), age)
fn age_dot() -> Int = person().age
fn older() -> Record<age: Int, name: Text> = { ...person(), age: 43 }
fn older_renamed() -> Record<age: Int, name: Text> = { ...person(), age: 43, name: "Grace" }
"#,
        )
        .expect("source record must parse");
        let acl = source_program_to_acl(&program, "source_record".to_string());

        assert_eq!(
            program.functions[0].return_type,
            "Record<age:Int,name:Text>"
        );
        assert!(acl.contains(
            r#"op create_function id=fn.person return=Record<age:Int,name:Text> body=record(age, 42, name, "Ada")"#
        ));
        assert!(acl.contains("op create_function id=fn.age return=Int body=field(person(), age)"));
        assert!(
            acl.contains("op create_function id=fn.age_dot return=Int body=field(person(), age)")
        );
        assert!(
            acl.contains(
                "op create_function id=fn.older return=Record<age:Int,name:Text> body=update(person(), age, 43)"
            )
        );
        assert!(acl.contains(
            r#"op create_function id=fn.older_renamed return=Record<age:Int,name:Text> body=update(update(person(), age, 43), name, "Grace")"#
        ));
    }

    #[test]
    fn lowers_source_option_result_constructors() {
        let program = parse_ail_source(
            r#"
fn maybe(flag: Bool) -> Option<Int> = if flag { Some(42) } else { None }
fn ok_value() -> Result<Int, Text> = Ok(42)
fn err_value() -> Result<Int, Text> = Err("boom")
"#,
        )
        .expect("source constructors must parse");
        let acl = source_program_to_acl(&program, "source_constructors".to_string());

        assert!(acl.contains(
            "op create_function id=fn.maybe return=Option<Int> body=if(flag, some(42), none())"
        ));
        assert!(
            acl.contains("op create_function id=fn.ok_value return=Result<Int,Text> body=ok(42)")
        );
        assert!(acl.contains(
            r#"op create_function id=fn.err_value return=Result<Int,Text> body=err("boom")"#
        ));
    }

    #[test]
    fn lowers_source_unwrap_or_helper() {
        let program = parse_ail_source(
            r#"
fn value(input: Option<Int>) -> Int = unwrap_or(input, 0)
"#,
        )
        .expect("source unwrap_or must parse");
        let acl = source_program_to_acl(&program, "source_unwrap_or".to_string());

        assert!(acl.contains(
            "op create_function id=fn.value return=Int body=match(input, Some(__ail_unwrap), __ail_unwrap, None, 0)"
        ));
    }

    #[test]
    fn lowers_source_option_predicate_helpers() {
        let program = parse_ail_source(
            r#"
fn has_value(input: Option<Int>) -> Bool = is_some(input)
fn missing(input: Option<Int>) -> Bool = is_none(input)
"#,
        )
        .expect("source option predicates must parse");
        let acl = source_program_to_acl(&program, "source_option_predicates".to_string());

        assert!(acl.contains(
            "op create_function id=fn.has_value return=Bool body=match(input, Some(_), true, None, false)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.missing return=Bool body=match(input, Some(_), false, None, true)"
        ));
    }

    #[test]
    fn lowers_source_result_predicate_helpers() {
        let program = parse_ail_source(
            r#"
fn succeeded(input: Result<Int, Text>) -> Bool = is_ok(input)
fn failed(input: Result<Int, Text>) -> Bool = is_err(input)
"#,
        )
        .expect("source result predicates must parse");
        let acl = source_program_to_acl(&program, "source_result_predicates".to_string());

        assert!(acl.contains(
            "op create_function id=fn.succeeded return=Bool body=match(input, Ok(_), true, Err(_), false)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.failed return=Bool body=match(input, Ok(_), false, Err(_), true)"
        ));
    }

    #[test]
    fn lowers_source_first_or_helper() {
        let program = parse_ail_source(
            r#"
fn first(values: List<Int>) -> Int = first_or(values, 0)
"#,
        )
        .expect("source first_or must parse");
        let acl = source_program_to_acl(&program, "source_first_or".to_string());

        assert!(acl.contains(
            "op create_function id=fn.first return=Int body=if(gt(len(values), 0), index(values, 0), 0)"
        ));
    }

    #[test]
    fn lowers_source_last_or_helper() {
        let program = parse_ail_source(
            r#"
fn last(values: List<Int>) -> Int = last_or(values, 0)
"#,
        )
        .expect("source last_or must parse");
        let acl = source_program_to_acl(&program, "source_last_or".to_string());

        assert!(acl.contains(
            "op create_function id=fn.last return=Int body=if(gt(len(values), 0), index(values, sub(len(values), 1)), 0)"
        ));
    }

    #[test]
    fn lowers_source_get_or_helper() {
        let program = parse_ail_source(
            r#"
fn item(values: List<Int>, idx: Int) -> Int = get_or(values, idx, 0)
"#,
        )
        .expect("source get_or must parse");
        let acl = source_program_to_acl(&program, "source_get_or".to_string());

        assert!(acl.contains(
            "op create_function id=fn.item return=Int body=if(and(ge(idx, 0), lt(idx, len(values))), index(values, idx), 0)"
        ));
    }

    #[test]
    fn lowers_source_is_empty_helper() {
        let program = parse_ail_source(
            r#"
fn no_items(values: List<Int>) -> Bool = is_empty(values)
fn no_text(value: Text) -> Bool = is_empty(value)
"#,
        )
        .expect("source is_empty must parse");
        let acl = source_program_to_acl(&program, "source_is_empty".to_string());

        assert!(
            acl.contains("op create_function id=fn.no_items return=Bool body=eq(len(values), 0)")
        );
        assert!(
            acl.contains("op create_function id=fn.no_text return=Bool body=eq(len(value), 0)")
        );
    }

    #[test]
    fn lowers_source_infix_arithmetic_with_precedence() {
        let program = parse_ail_source("test math = 10 - 2 * 3 + 8 / 4 + 7 % 4 == 9")
            .expect("source must parse");
        let acl = source_program_to_acl(&program, "source_math".to_string());

        assert!(acl.contains(
            "op create_test id=test.math return=Bool body=eq(add(add(sub(10, mul(2, 3)), div(8, 4)), mod(7, 4)), 9)"
        ));
    }

    #[test]
    fn lowers_source_text_concat_operator() {
        let program = parse_ail_source(
            r#"
fn greeting(name: Text) -> Text = "Hello, " ++ name
test greeting = "Hello, " ++ "AIL" == "Hello, AIL"
"#,
        )
        .expect("source text concat operator must parse");
        let acl = source_program_to_acl(&program, "source_text_concat_operator".to_string());

        assert!(acl.contains(
            r#"op create_function id=fn.greeting return=Text body=concat("Hello, ", name)"#
        ));
        assert!(acl.contains(
            r#"op create_test id=test.greeting return=Bool body=eq(concat("Hello, ", "AIL"), "Hello, AIL")"#
        ));
    }

    #[test]
    fn lowers_source_text_eq_helper() {
        let program = parse_ail_source(
            r#"
fn same(left: Text, right: Text) -> Bool = text_eq(left, right)
"#,
        )
        .expect("source text_eq must parse");
        let acl = source_program_to_acl(&program, "source_text_eq".to_string());

        assert!(
            acl.contains("op create_function id=fn.same return=Bool body=text.eq(left, right)")
        );
    }

    #[test]
    fn lowers_source_int_bounds_helpers() {
        let program = parse_ail_source(
            r#"
fn low(left: Int, right: Int) -> Int = int_min(left, right)
fn high(left: Int, right: Int) -> Int = int_max(left, right)
fn bounded(value: Int, low: Int, high: Int) -> Int = int_clamp(value, low, high)
fn magnitude(value: Int, fallback: Int) -> Int = int_abs_or(value, fallback)
fn negated(value: Int, fallback: Int) -> Int = int_neg_or(value, fallback)
fn summed(left: Int, right: Int, fallback: Int) -> Int = int_add_or(left, right, fallback)
fn difference(left: Int, right: Int, fallback: Int) -> Int = int_sub_or(left, right, fallback)
fn product(left: Int, right: Int, fallback: Int) -> Int = int_mul_or(left, right, fallback)
fn saturated(left: Int, right: Int) -> Int = int_saturating_add(left, right)
fn saturated_difference(left: Int, right: Int) -> Int = int_saturating_sub(left, right)
fn saturated_product(left: Int, right: Int) -> Int = int_saturating_mul(left, right)
fn quotient(value: Int, divisor: Int, fallback: Int) -> Int = int_div_or(value, divisor, fallback)
fn remainder(value: Int, divisor: Int, fallback: Int) -> Int = int_rem_or(value, divisor, fallback)
"#,
        )
        .expect("source int bounds helpers must parse");
        let acl = source_program_to_acl(&program, "source_int_bounds".to_string());

        assert!(acl.contains("op create_function id=fn.low return=Int body=int.min(left, right)"));
        assert!(acl.contains("op create_function id=fn.high return=Int body=int.max(left, right)"));
        assert!(acl.contains(
            "op create_function id=fn.bounded return=Int body=int.clamp(value, low, high)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.magnitude return=Int body=int.abs_or(value, fallback)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.negated return=Int body=int.neg_or(value, fallback)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.summed return=Int body=int.add_or(left, right, fallback)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.difference return=Int body=int.sub_or(left, right, fallback)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.product return=Int body=int.mul_or(left, right, fallback)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.saturated return=Int body=int.saturating_add(left, right)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.saturated_difference return=Int body=int.saturating_sub(left, right)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.saturated_product return=Int body=int.saturating_mul(left, right)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.quotient return=Int body=int.div_or(value, divisor, fallback)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.remainder return=Int body=int.rem_or(value, divisor, fallback)"
        ));
    }

    #[test]
    fn lowers_source_text_trim_helper() {
        let program = parse_ail_source(
            r#"
fn cleaned(value: Text) -> Text = text_trim(value)
"#,
        )
        .expect("source text_trim must parse");
        let acl = source_program_to_acl(&program, "source_text_trim".to_string());

        assert!(acl.contains("op create_function id=fn.cleaned return=Text body=text.trim(value)"));
    }

    #[test]
    fn lowers_source_text_contains_helper() {
        let program = parse_ail_source(
            r#"
fn has(haystack: Text, needle: Text) -> Bool = text_contains(haystack, needle)
"#,
        )
        .expect("source text_contains must parse");
        let acl = source_program_to_acl(&program, "source_text_contains".to_string());

        assert!(acl.contains(
            "op create_function id=fn.has return=Bool body=text.contains(haystack, needle)"
        ));
    }

    #[test]
    fn lowers_source_text_index_of_helper() {
        let program = parse_ail_source(
            r#"
fn find(haystack: Text, needle: Text) -> Int = text_index_of(haystack, needle)
"#,
        )
        .expect("source text_index_of must parse");
        let acl = source_program_to_acl(&program, "source_text_index_of".to_string());

        assert!(acl.contains(
            "op create_function id=fn.find return=Int body=text.index_of(haystack, needle)"
        ));
    }

    #[test]
    fn lowers_source_text_byte_at_or_helper() {
        let program = parse_ail_source(
            r#"
fn byte(value: Text, index: Int, fallback: Int) -> Int = text_byte_at_or(value, index, fallback)
"#,
        )
        .expect("source text_byte_at_or must parse");
        let acl = source_program_to_acl(&program, "source_text_byte_at_or".to_string());

        assert!(acl.contains(
            "op create_function id=fn.byte return=Int body=text.byte_at_or(value, index, fallback)"
        ));
    }

    #[test]
    fn lowers_source_text_parse_int_or_helper() {
        let program = parse_ail_source(
            r#"
fn parsed(value: Text, fallback: Int) -> Int = text_parse_int_or(value, fallback)
"#,
        )
        .expect("source text_parse_int_or must parse");
        let acl = source_program_to_acl(&program, "source_text_parse_int_or".to_string());

        assert!(acl.contains(
            "op create_function id=fn.parsed return=Int body=text.parse_int_or(value, fallback)"
        ));
    }

    #[test]
    fn lowers_source_text_slice_helper() {
        let program = parse_ail_source(
            r#"
fn piece(value: Text, start: Int, length: Int) -> Text = text_slice(value, start, length)
"#,
        )
        .expect("source text_slice must parse");
        let acl = source_program_to_acl(&program, "source_text_slice".to_string());

        assert!(acl.contains(
            "op create_function id=fn.piece return=Text body=text.slice(value, start, length)"
        ));
    }

    #[test]
    fn lowers_source_text_replace_first_helper() {
        let program = parse_ail_source(
            r#"
fn changed(value: Text, needle: Text, replacement: Text) -> Text = text_replace_first(value, needle, replacement)
"#,
        )
        .expect("source text_replace_first must parse");
        let acl = source_program_to_acl(&program, "source_text_replace_first".to_string());

        assert!(acl.contains(
            "op create_function id=fn.changed return=Text body=text.replace_first(value, needle, replacement)"
        ));
    }

    #[test]
    fn lowers_source_text_boundary_helpers() {
        let program = parse_ail_source(
            r#"
fn prefixed(haystack: Text, prefix: Text) -> Bool = text_starts_with(haystack, prefix)
fn suffixed(haystack: Text, suffix: Text) -> Bool = text_ends_with(haystack, suffix)
"#,
        )
        .expect("source text boundary helpers must parse");
        let acl = source_program_to_acl(&program, "source_text_boundary".to_string());

        assert!(acl.contains(
            "op create_function id=fn.prefixed return=Bool body=text.starts_with(haystack, prefix)"
        ));
        assert!(acl.contains(
            "op create_function id=fn.suffixed return=Bool body=text.ends_with(haystack, suffix)"
        ));
    }

    #[test]
    fn formats_source_builtin_calls_as_infix() {
        let (formatted, item_count) = format_ail_source(
            "test math = eq(add(sub(10, mul(2, 3)), add(div(8, 4), mod(7, 4))), 9)\n\
             test grouped = and(eq(sub(10, add(2, 3)), 5), not(false))\n\
             fn greeting(name: Text) -> Text = concat(\"Hello, \", name)\n",
        )
        .expect("source must format");

        assert_eq!(item_count, 3);
        assert!(formatted.contains("test math = 10 - 2 * 3 + (8 / 4 + 7 % 4) == 9\n"));
        assert!(formatted.contains("test grouped = 10 - (2 + 3) == 5 && !false\n"));
        assert!(formatted.contains("fn greeting(name: Text) -> Text = \"Hello, \" ++ name\n"));
    }

    #[test]
    fn formats_source_int_bounds_helpers() {
        let (formatted, item_count) = format_ail_source(
            "fn low(left:Int,right:Int)->Int=int.min(left,right)\nfn high(left:Int,right:Int)->Int=int.max(left,right)\nfn bounded(value:Int,low:Int,high:Int)->Int=int.clamp(value,low,high)\nfn magnitude(value:Int,fallback:Int)->Int=int.abs_or(value,fallback)\nfn negated(value:Int,fallback:Int)->Int=int.neg_or(value,fallback)\nfn summed(left:Int,right:Int,fallback:Int)->Int=int.add_or(left,right,fallback)\nfn difference(left:Int,right:Int,fallback:Int)->Int=int.sub_or(left,right,fallback)\nfn product(left:Int,right:Int,fallback:Int)->Int=int.mul_or(left,right,fallback)\nfn saturated(left:Int,right:Int)->Int=int.saturating_add(left,right)\nfn saturated_difference(left:Int,right:Int)->Int=int.saturating_sub(left,right)\nfn quotient(value:Int,divisor:Int,fallback:Int)->Int=int.div_or(value,divisor,fallback)\nfn remainder(value:Int,divisor:Int,fallback:Int)->Int=int.rem_or(value,divisor,fallback)\n",
        )
        .expect("source int bounds helpers must format");

        assert_eq!(item_count, 13);
        assert!(
            formatted.contains("fn low(left: Int, right: Int) -> Int = int_min(left, right)\n")
        );
        assert!(
            formatted.contains("fn high(left: Int, right: Int) -> Int = int_max(left, right)\n")
        );
        assert!(formatted.contains(
            "fn bounded(value: Int, low: Int, high: Int) -> Int = int_clamp(value, low, high)\n"
        ));
        assert!(formatted.contains(
            "fn magnitude(value: Int, fallback: Int) -> Int = int_abs_or(value, fallback)\n"
        ));
        assert!(formatted.contains(
            "fn negated(value: Int, fallback: Int) -> Int = int_neg_or(value, fallback)\n"
        ));
        assert!(formatted.contains(
            "fn summed(left: Int, right: Int, fallback: Int) -> Int = int_add_or(left, right, fallback)\n"
        ));
        assert!(formatted.contains(
            "fn difference(left: Int, right: Int, fallback: Int) -> Int = int_sub_or(left, right, fallback)\n"
        ));
        assert!(formatted.contains(
            "fn product(left: Int, right: Int, fallback: Int) -> Int = int_mul_or(left, right, fallback)\n"
        ));
        assert!(formatted.contains(
            "fn saturated(left: Int, right: Int) -> Int = int_saturating_add(left, right)\n"
        ));
        assert!(formatted.contains(
            "fn saturated_difference(left: Int, right: Int) -> Int = int_saturating_sub(left, right)\n"
        ));
        assert!(formatted.contains(
            "fn quotient(value: Int, divisor: Int, fallback: Int) -> Int = int_div_or(value, divisor, fallback)\n"
        ));
        assert!(formatted.contains(
            "fn remainder(value: Int, divisor: Int, fallback: Int) -> Int = int_rem_or(value, divisor, fallback)\n"
        ));
    }

    #[test]
    fn formats_source_text_eq_helper() {
        let (formatted, item_count) =
            format_ail_source("fn same(left:Text,right:Text)->Bool=text.eq(left,right)\n")
                .expect("source text_eq must format");

        assert_eq!(item_count, 1);
        assert!(
            formatted.contains("fn same(left: Text, right: Text) -> Bool = text_eq(left, right)\n")
        );
    }

    #[test]
    fn formats_source_text_trim_helper() {
        let (formatted, item_count) =
            format_ail_source("fn cleaned(value:Text)->Text=text.trim(value)\n")
                .expect("source text_trim must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains("fn cleaned(value: Text) -> Text = text_trim(value)\n"));
    }

    #[test]
    fn formats_source_text_contains_helper() {
        let (formatted, item_count) = format_ail_source(
            "fn has(haystack:Text,needle:Text)->Bool=text.contains(haystack,needle)\n",
        )
        .expect("source text_contains must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains(
            "fn has(haystack: Text, needle: Text) -> Bool = text_contains(haystack, needle)\n"
        ));
    }

    #[test]
    fn formats_source_text_index_of_helper() {
        let (formatted, item_count) = format_ail_source(
            "fn find(haystack:Text,needle:Text)->Int=text.index_of(haystack,needle)\n",
        )
        .expect("source text_index_of must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains(
            "fn find(haystack: Text, needle: Text) -> Int = text_index_of(haystack, needle)\n"
        ));
    }

    #[test]
    fn formats_source_text_byte_at_or_helper() {
        let (formatted, item_count) = format_ail_source(
            "fn byte(value:Text,index:Int,fallback:Int)->Int=text.byte_at_or(value,index,fallback)\n",
        )
        .expect("source text_byte_at_or must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains(
            "fn byte(value: Text, index: Int, fallback: Int) -> Int = text_byte_at_or(value, index, fallback)\n"
        ));
    }

    #[test]
    fn formats_source_text_parse_int_or_helper() {
        let (formatted, item_count) = format_ail_source(
            "fn parsed(value:Text,fallback:Int)->Int=text.parse_int_or(value,fallback)\n",
        )
        .expect("source text_parse_int_or must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains(
            "fn parsed(value: Text, fallback: Int) -> Int = text_parse_int_or(value, fallback)\n"
        ));
    }

    #[test]
    fn formats_source_text_slice_helper() {
        let (formatted, item_count) = format_ail_source(
            "fn piece(value:Text,start:Int,length:Int)->Text=text.slice(value,start,length)\n",
        )
        .expect("source text_slice must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains(
            "fn piece(value: Text, start: Int, length: Int) -> Text = text_slice(value, start, length)\n"
        ));
    }

    #[test]
    fn formats_source_text_replace_first_helper() {
        let (formatted, item_count) = format_ail_source(
            "fn changed(value:Text,needle:Text,replacement:Text)->Text=text.replace_first(value,needle,replacement)\n",
        )
        .expect("source text_replace_first must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains(
            "fn changed(value: Text, needle: Text, replacement: Text) -> Text = text_replace_first(value, needle, replacement)\n"
        ));
    }

    #[test]
    fn formats_source_text_boundary_helpers() {
        let (formatted, item_count) = format_ail_source(
            "fn prefixed(haystack:Text,prefix:Text)->Bool=text.starts_with(haystack,prefix)\n\
             fn suffixed(haystack:Text,suffix:Text)->Bool=text.ends_with(haystack,suffix)\n",
        )
        .expect("source text boundary helpers must format");

        assert_eq!(item_count, 2);
        assert!(formatted.contains(
            "fn prefixed(haystack: Text, prefix: Text) -> Bool = text_starts_with(haystack, prefix)\n"
        ));
        assert!(formatted.contains(
            "fn suffixed(haystack: Text, suffix: Text) -> Bool = text_ends_with(haystack, suffix)\n"
        ));
    }

    #[test]
    fn lowers_source_unary_minus() {
        let program = parse_ail_source(
            "fn negated(x: Int) -> Int = -x
test grouped = -(1 + 2) == -3",
        )
        .expect("source must parse");
        let acl = source_program_to_acl(&program, "source_negate".to_string());

        assert!(acl.contains("op create_function id=fn.negated return=Int body=sub(0, x)"));
        assert!(
            acl.contains(
                "op create_test id=test.grouped return=Bool body=eq(sub(0, add(1, 2)), -3)"
            )
        );
    }

    #[test]
    fn formats_source_unary_minus() {
        let (formatted, item_count) = format_ail_source(
            "fn negated(x:Int)->Int=sub(0,x)
             test grouped=eq(sub(0,add(1,2)),-3)
",
        )
        .expect("source must format");

        assert_eq!(item_count, 2);
        assert!(formatted.contains(
            "fn negated(x: Int) -> Int = -x
"
        ));
        assert!(formatted.contains(
            "test grouped = -(1 + 2) == -3
"
        ));
    }

    #[test]
    fn formats_source_set_and_map_types() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn ids()->Set<i64>=set(1,add(2,3))
fn labels()->Map<String,int>=map("one",1,"two",2)
"#,
        )
        .expect("source set/map must format");

        assert_eq!(item_count, 2);
        assert!(formatted.contains("fn ids() -> Set<Int> = set(1, 2 + 3)\n"));
        assert!(formatted.contains("fn labels() -> Map<Text,Int> = map(\"one\", 1, \"two\", 2)\n"));
    }

    #[test]
    fn formats_source_tuple_types() {
        let (formatted, item_count) =
            format_ail_source(r#"fn pair()->Tuple<i64,String>=tuple(42,"answer")"#)
                .expect("source tuple must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains("fn pair() -> Tuple<Int,Text> = tuple(42, \"answer\")\n"));
    }

    #[test]
    fn formats_source_record_types() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn person()->Record<age:i64,name:String>=record(age,42,name,"Ada")
fn age()->Int=field(person(),age)
fn age_dot()->Int=person().age
fn older()->Record<age:Int,name:Text>=update(person(),age,43)
"#,
        )
        .expect("source record must format");

        assert_eq!(item_count, 4);
        assert!(
            formatted.contains(
                "fn person() -> Record<age:Int,name:Text> = { age: 42, name: \"Ada\" }\n"
            )
        );
        assert!(formatted.contains("fn age() -> Int = person().age\n"));
        assert!(formatted.contains("fn age_dot() -> Int = person().age\n"));
        assert!(
            formatted
                .contains("fn older() -> Record<age:Int,name:Text> = { ...person(), age: 43 }\n")
        );
    }

    #[test]
    fn formats_source_option_result_constructors() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn maybe(flag:Bool)->Option<Int>=if flag { some(42) } else { none() }
fn ok_value()->Result<Int,Text>=ok(42)
fn err_value()->Result<Int,Text>=err("boom")
"#,
        )
        .expect("source constructors must format");

        assert_eq!(item_count, 3);
        assert!(formatted.contains(
            "fn maybe(flag: Bool) -> Option<Int> = if flag { Some(42) } else { None }\n"
        ));
        assert!(formatted.contains("fn ok_value() -> Result<Int,Text> = Ok(42)\n"));
        assert!(formatted.contains("fn err_value() -> Result<Int,Text> = Err(\"boom\")\n"));
    }

    #[test]
    fn formats_source_unwrap_or_helper() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn value(input:Option<Int>)->Int=match(input,Some(v),v,None,0)
"#,
        )
        .expect("source unwrap_or must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains("fn value(input: Option<Int>) -> Int = unwrap_or(input, 0)\n"));
    }

    #[test]
    fn formats_source_option_predicate_helpers() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn has_value(input:Option<Int>)->Bool=match(input,Some(_),true,None,false)
fn missing(input:Option<Int>)->Bool=match(input,Some(_),false,None,true)
"#,
        )
        .expect("source option predicates must format");

        assert_eq!(item_count, 2);
        assert!(formatted.contains("fn has_value(input: Option<Int>) -> Bool = is_some(input)\n"));
        assert!(formatted.contains("fn missing(input: Option<Int>) -> Bool = is_none(input)\n"));
    }

    #[test]
    fn formats_source_result_predicate_helpers() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn succeeded(input:Result<Int,Text>)->Bool=match(input,Ok(_),true,Err(_),false)
fn failed(input:Result<Int,Text>)->Bool=match(input,Ok(_),false,Err(_),true)
"#,
        )
        .expect("source result predicates must format");

        assert_eq!(item_count, 2);
        assert!(
            formatted.contains("fn succeeded(input: Result<Int,Text>) -> Bool = is_ok(input)\n")
        );
        assert!(formatted.contains("fn failed(input: Result<Int,Text>) -> Bool = is_err(input)\n"));
    }

    #[test]
    fn formats_source_first_or_helper() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn first(values:List<Int>)->Int=if(gt(len(values),0),index(values,0),0)
"#,
        )
        .expect("source first_or must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains("fn first(values: List<Int>) -> Int = first_or(values, 0)\n"));
    }

    #[test]
    fn formats_source_last_or_helper() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn last(values:List<Int>)->Int=if(gt(len(values),0),index(values,sub(len(values),1)),0)
"#,
        )
        .expect("source last_or must format");

        assert_eq!(item_count, 1);
        assert!(formatted.contains("fn last(values: List<Int>) -> Int = last_or(values, 0)\n"));
    }

    #[test]
    fn formats_source_get_or_helper() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn item(values:List<Int>,idx:Int)->Int=if(and(ge(idx,0),lt(idx,len(values))),index(values,idx),0)
"#,
        )
        .expect("source get_or must format");

        assert_eq!(item_count, 1);
        assert!(
            formatted
                .contains("fn item(values: List<Int>, idx: Int) -> Int = get_or(values, idx, 0)\n")
        );
    }

    #[test]
    fn formats_source_is_empty_helper() {
        let (formatted, item_count) = format_ail_source(
            r#"
fn no_items(values:List<Int>)->Bool=eq(len(values),0)
fn no_text(value:Text)->Bool=eq(0,len(value))
"#,
        )
        .expect("source is_empty must format");

        assert_eq!(item_count, 2);
        assert!(formatted.contains("fn no_items(values: List<Int>) -> Bool = is_empty(values)\n"));
        assert!(formatted.contains("fn no_text(value: Text) -> Bool = is_empty(value)\n"));
    }

    #[test]
    fn rejects_duplicate_source_function_declarations() {
        let err = parse_ail_source(
            r#"
fn main() -> Int = 1
fn main() -> Int = 2
"#,
        )
        .expect_err("duplicate source functions must be rejected");

        assert!(
            err.to_string()
                .contains("duplicate function declaration `fn.main`")
        );
    }

    #[test]
    fn qualifies_source_module_declarations_and_local_calls() {
        let program = parse_ail_source(
            r#"
module math
fn add_pair(x: Int, y: Int) -> Int = add(x, y)
fn main() -> Int = add_pair(20, 22)
test main_addition = eq(main(), 42)
"#,
        )
        .expect("source module must parse");
        let acl = source_program_to_acl(&program, "source_module".to_string());
        let (formatted, item_count) = format_ail_source(
            r#"
module math
fn add_pair(x:Int,y:Int)->Int=add(x,y)
fn main()->Int=add_pair(20,22)
test main_addition=eq(main(),42)
"#,
        )
        .expect("source module must format");

        assert_eq!(program.module.as_deref(), Some("math"));
        assert!(acl.contains("op create_function id=fn.math.add_pair return=Int body=add(x, y)"));
        assert!(
            acl.contains(
                "op create_function id=fn.math.main return=Int body=math.add_pair(20, 22)"
            )
        );
        assert!(acl.contains(
            "op create_test id=test.math.main_addition return=Bool body=eq(math.main(), 42)"
        ));
        assert_eq!(item_count, 4);
        assert!(formatted.contains("module math\n"));
        assert!(formatted.contains("fn main() -> Int = add_pair(20, 22)\n"));
        assert!(formatted.contains("test main_addition = main() == 42\n"));
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
    fn lowers_source_typed_let_annotations() {
        let program = parse_ail_source(
            r#"
fn main() -> Int {
  let base: Int = 20 + 20
  return base + 2
}
"#,
        )
        .expect("source must parse");
        let acl = source_program_to_acl(&program, "source_typed_let".to_string());

        assert_eq!(
            program.functions[0].body,
            "let_typed(base, Int, 3, add(20, 20), add(base, 2))"
        );
        assert!(acl.contains(
            "op create_function id=fn.main return=Int body=let(base, add(20, 20), add(base, 2))"
        ));
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
    fn parses_and_formats_relative_source_imports() {
        let src = r#"
use "./math.ail"
fn main() -> Int = add_pair(20, 22)
"#;
        let program = parse_ail_source(src).expect("source imports must parse");
        let acl = source_program_to_acl(&program, "source_import".to_string());
        let (formatted, item_count) = format_ail_source(src).expect("source must format");

        assert_eq!(program.imports, vec!["./math.ail".to_string()]);
        assert!(!acl.contains("use"));
        assert!(acl.contains("op create_function id=fn.main return=Int body=add_pair(20, 22)"));
        assert_eq!(item_count, 2);
        assert_eq!(
            formatted,
            "use \"./math.ail\"\nfn main() -> Int = add_pair(20, 22)\n"
        );
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
            "fn message() -> Text = \"https://ail.local\" ++ \" {ok}\"\n"
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
        assert!(formatted.contains("fn add_pair(x: Int, y: Int) -> Int = x + y\n"));
        assert!(formatted.contains("fn main() -> Int {\n"));
        assert!(formatted.contains("  let base = 20 + 20\n"));
        assert!(formatted.contains("  return if base > 40 { base + 2 } else { 0 }\n"));
        assert!(formatted.contains("test addition = add_pair(20, 22) == 42\n"));
    }
}
