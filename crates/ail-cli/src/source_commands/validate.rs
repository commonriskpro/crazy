use super::model::*;
use super::parse::*;
use super::syntax::*;
use super::*;

pub(super) fn qualify_source_program_module(program: &mut SourceProgram) {
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

pub(super) fn resolve_source_program_grants(program: &mut SourceProgram) -> Result<(), CliError> {
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

pub(super) fn resolve_source_grant_target(
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
        _ => Err(source_grant_target_ambiguous_error(target)),
    }
}

pub(super) fn default_source_grant_target(target: &str, module: Option<&str>) -> String {
    if target.starts_with("fn.") {
        qualify_source_name_for_module(target, "fn.", module)
    } else if target.starts_with("test.") {
        qualify_source_name_for_module(target, "test.", module)
    } else {
        qualify_source_name_for_module(&normalize_function_name(target), "fn.", module)
    }
}

pub(super) fn qualify_source_name_for_module(
    name: &str,
    prefix: &str,
    module: Option<&str>,
) -> String {
    module
        .map(|module| qualify_source_name(name, prefix, module))
        .unwrap_or_else(|| name.to_string())
}

pub(super) fn unqualified_source_name(name: &str, prefix: &str) -> Option<String> {
    let bare = name.strip_prefix(prefix)?;
    if bare.contains('.') {
        None
    } else {
        Some(bare.to_string())
    }
}

pub(super) fn qualify_source_name(name: &str, prefix: &str, module: &str) -> String {
    let Some(bare) = name.strip_prefix(prefix) else {
        return name.to_string();
    };
    if bare.contains('.') {
        name.to_string()
    } else {
        format!("{prefix}{module}.{bare}")
    }
}

pub(super) fn qualify_source_expr_calls(
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

pub(super) fn validate_source_program_symbols(program: &SourceProgram) -> Result<(), CliError> {
    if let Some(name) = duplicate_name(program.capabilities.iter().map(String::as_str)) {
        return Err(source_symbol_duplicate_capability_error(&name));
    }
    if let Some((first, second)) =
        duplicate_source_declaration(program.constants.iter().map(|constant| {
            SourceDeclarationRef {
                kind: "const",
                name: constant.name.as_str(),
                line_num: constant.line_num,
                source_path: constant.source_path.as_deref(),
            }
        }))
    {
        return Err(source_symbol_duplicate_error(
            "const",
            "duplicate const declaration",
            first,
            second,
        ));
    }
    let function_names = program
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(constant) = program
        .constants
        .iter()
        .find(|constant| function_names.contains(constant.name.as_str()))
    {
        let function = program
            .functions
            .iter()
            .find(|function| function.name == constant.name)
            .expect("matching function exists");
        return Err(source_symbol_duplicate_error(
            "declaration",
            "duplicate function or const declaration",
            SourceDeclarationRef {
                kind: "const",
                name: constant.name.as_str(),
                line_num: constant.line_num,
                source_path: constant.source_path.as_deref(),
            },
            SourceDeclarationRef {
                kind: "function",
                name: function.name.as_str(),
                line_num: function.line_num,
                source_path: function.source_path.as_deref(),
            },
        ));
    }
    if let Some((first, second)) =
        duplicate_source_declaration(program.functions.iter().map(|function| {
            SourceDeclarationRef {
                kind: "function",
                name: function.name.as_str(),
                line_num: function.line_num,
                source_path: function.source_path.as_deref(),
            }
        }))
    {
        return Err(source_symbol_duplicate_error(
            "function",
            "duplicate function declaration",
            first,
            second,
        ));
    }
    if let Some((first, second)) =
        duplicate_source_declaration(program.tests.iter().map(|test| SourceDeclarationRef {
            kind: "test",
            name: test.name.as_str(),
            line_num: test.line_num,
            source_path: test.source_path.as_deref(),
        }))
    {
        return Err(source_symbol_duplicate_error(
            "test",
            "duplicate test declaration",
            first,
            second,
        ));
    }
    for constant in &program.constants {
        if let Some(builtin) = source_function_builtin_shadow(&constant.name) {
            return Err(source_symbol_builtin_shadow_error(
                "const",
                &constant.name,
                builtin,
            ));
        }
    }
    for function in &program.functions {
        if let Some(builtin) = source_function_builtin_shadow(&function.name) {
            return Err(source_symbol_builtin_shadow_error(
                "function",
                &function.name,
                builtin,
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct SourceDeclarationRef<'a> {
    kind: &'static str,
    name: &'a str,
    line_num: usize,
    source_path: Option<&'a Path>,
}

fn duplicate_source_declaration<'a>(
    declarations: impl Iterator<Item = SourceDeclarationRef<'a>>,
) -> Option<(SourceDeclarationRef<'a>, SourceDeclarationRef<'a>)> {
    let mut seen = BTreeMap::new();
    for declaration in declarations {
        if let Some(first) = seen.insert(declaration.name, declaration) {
            return Some((first, declaration));
        }
    }
    None
}

fn format_duplicate_source_declaration(
    imported_kind: &str,
    legacy_prefix: &str,
    first: SourceDeclarationRef<'_>,
    second: SourceDeclarationRef<'_>,
) -> String {
    if first.source_path.is_none() && second.source_path.is_none() {
        return format!("{legacy_prefix} `{}`", first.name);
    }

    format!(
        "duplicate imported source {imported_kind} `{}`: {} conflicts with {}",
        first.name,
        format_source_declaration_origin(first),
        format_source_declaration_origin(second)
    )
}

fn source_symbol_duplicate_capability_error(name: &str) -> CliError {
    source_symbol_error(
        "AIL_SOURCE_SYMBOL_DUPLICATE",
        "source.symbol.duplicate",
        format!("duplicate capability declaration `{name}`"),
    )
}

fn source_symbol_duplicate_error(
    imported_kind: &str,
    legacy_prefix: &str,
    first: SourceDeclarationRef<'_>,
    second: SourceDeclarationRef<'_>,
) -> CliError {
    source_symbol_error(
        "AIL_SOURCE_SYMBOL_DUPLICATE",
        "source.symbol.duplicate",
        format_duplicate_source_declaration(imported_kind, legacy_prefix, first, second),
    )
}

fn source_symbol_builtin_shadow_error(kind: &str, name: &str, builtin: &str) -> CliError {
    source_symbol_error(
        "AIL_SOURCE_SYMBOL_BUILTIN_SHADOW",
        "source.symbol.builtin_shadow",
        format!("{kind} declaration `{name}` shadows builtin `{builtin}`"),
    )
}

fn source_symbol_error(code: &str, category: &str, message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!("{} [{code}] category={category}", message.as_ref()))
}

fn format_source_declaration_origin(declaration: SourceDeclarationRef<'_>) -> String {
    match declaration.source_path {
        Some(path) => format!(
            "{} in {} line {}",
            declaration.kind,
            path.display(),
            declaration.line_num
        ),
        None => format!("{} on line {}", declaration.kind, declaration.line_num),
    }
}

pub(super) fn source_function_builtin_shadow(name: &str) -> Option<&str> {
    let bare = name.strip_prefix("fn.")?.rsplit('.').next()?;
    known_source_builtin_arity(bare).map(|_| bare)
}

pub(super) fn duplicate_name<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            return Some(name.to_string());
        }
    }
    None
}

pub(super) fn validate_source_program_grants(program: &SourceProgram) -> Result<(), CliError> {
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
            return Err(source_grant_target_unknown_error(&grant.target));
        }
        if !capabilities.contains(grant.capability.as_str()) {
            return Err(source_grant_capability_unknown_error(&grant.capability));
        }
    }
    Ok(())
}

pub(super) fn validate_source_program_effect_calls(
    program: &SourceProgram,
) -> Result<(), CliError> {
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

pub(super) fn validate_source_const_effect_free(
    target: &str,
    body: &str,
    capabilities: &BTreeSet<&str>,
) -> Result<(), CliError> {
    let mut direct_effects = BTreeSet::new();
    collect_source_direct_effect_capabilities(body, capabilities, &mut direct_effects)?;
    if let Some(capability) = direct_effects.into_iter().next() {
        return Err(source_effect_const_error(target, &capability));
    }
    Ok(())
}

pub(super) fn source_grants_by_target(program: &SourceProgram) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut grants = BTreeMap::<&str, BTreeSet<&str>>::new();
    for grant in &program.grants {
        grants
            .entry(grant.target.as_str())
            .or_default()
            .insert(grant.capability.as_str());
    }
    grants
}

pub(super) fn validate_source_item_effect_grants(
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
            return Err(source_effect_grant_missing_error(target, &capability));
        }
    }
    Ok(())
}

pub(super) fn collect_source_direct_effect_capabilities(
    expr: &str,
    capabilities: &BTreeSet<&str>,
    out: &mut BTreeSet<String>,
) -> Result<(), CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(());
    };
    if matches!(func.as_str(), "print" | "log.write" | "log_write") {
        validate_declared_source_effect_capability("log.write", capabilities, &func)?;
        out.insert("log.write".to_string());
    }
    if func == "effect_call" && args.len() >= 2 {
        let capability = args[0].as_str();
        let operation = args[1].as_str();
        if !is_source_ident(capability) || !is_source_ident(operation) {
            return Err(source_effect_call_shape_error());
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

pub(super) fn validate_declared_source_effect_capability(
    capability: &str,
    capabilities: &BTreeSet<&str>,
    context: &str,
) -> Result<(), CliError> {
    if capabilities.contains(capability) {
        return Ok(());
    }
    Err(source_effect_capability_unknown_error(context, capability))
}

pub(super) fn validate_source_program_calls(program: &SourceProgram) -> Result<(), CliError> {
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

pub(super) fn source_constant_names(program: &SourceProgram) -> BTreeMap<String, String> {
    program
        .constants
        .iter()
        .flat_map(|constant| source_const_reference_names(&constant.name))
        .collect()
}

pub(super) fn source_const_reference_names(name: &str) -> Vec<(String, String)> {
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

pub(super) fn source_const_reference_target(
    name: &str,
    constants: &BTreeMap<String, String>,
) -> Option<String> {
    constants
        .get(name)
        .cloned()
        .or_else(|| constants.get(&format!("fn.{name}")).cloned())
}

pub(super) fn validate_source_expr_calls(
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
        return Err(source_name_unknown_function_error(&call));
    }
    Ok(())
}

pub(super) fn collect_source_calls(expr: &str, calls: &mut Vec<(String, usize)>) {
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

pub(super) fn source_function_arity(
    functions: &BTreeMap<&str, usize>,
    call: &str,
) -> Option<usize> {
    let normalized = if call.starts_with("fn.") {
        call.to_string()
    } else {
        format!("fn.{call}")
    };
    functions.get(normalized.as_str()).copied()
}

#[derive(Debug, Clone, Copy)]
pub(super) enum SourceArity {
    Exact(usize),
    Min(usize),
    Even,
    Any,
    Match,
}

pub(super) fn validate_source_call_arity(
    call: &str,
    actual: usize,
    arity: SourceArity,
) -> Result<(), CliError> {
    match arity {
        SourceArity::Exact(expected) if actual != expected => Err(source_call_arity_error(
            format!("function call `{call}` expects {expected} argument(s), got {actual}"),
        )),
        SourceArity::Min(expected) if actual < expected => Err(source_call_arity_error(format!(
            "function call `{call}` expects at least {expected} argument(s), got {actual}"
        ))),
        SourceArity::Even if !actual.is_multiple_of(2) => Err(source_call_arity_error(format!(
            "function call `{call}` expects an even number of arguments, got {actual}"
        ))),
        SourceArity::Match if actual < 3 || actual.is_multiple_of(2) => {
            Err(source_call_arity_error(format!(
                "function call `{call}` expects a scrutinee plus pattern/body pairs, got {actual} argument(s)"
            )))
        }
        _ => Ok(()),
    }
}

fn source_call_arity_error(message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!(
        "{} [AIL_SOURCE_CALL_ARITY] category=source.call.arity",
        message.as_ref()
    ))
}

fn source_name_unknown_function_error(call: &str) -> CliError {
    source_name_error(
        "AIL_SOURCE_NAME_UNKNOWN_FUNCTION",
        "source.name.function",
        format!("unknown function call `{call}` in AIL source"),
    )
}

fn source_name_unknown_variable_error(name: &str) -> CliError {
    source_name_error(
        "AIL_SOURCE_NAME_UNKNOWN_VARIABLE",
        "source.name.variable",
        format!("unknown variable `{name}` in AIL source"),
    )
}

fn source_name_error(code: &str, category: &str, message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!("{} [{code}] category={category}", message.as_ref()))
}

fn source_expr_malformed_string_error(expr: &str) -> CliError {
    source_expr_error(
        "AIL_SOURCE_EXPR_MALFORMED_STRING",
        "source.expr.literal",
        format!("malformed string literal `{expr}`"),
    )
}

fn source_expr_unsupported_numeric_error(expr: &str) -> CliError {
    source_expr_error(
        "AIL_SOURCE_EXPR_UNSUPPORTED_NUMERIC",
        "source.expr.literal",
        format!("unsupported source numeric literal `{expr}`"),
    )
}

fn source_expr_unsupported_error(expr: &str) -> CliError {
    source_expr_error(
        "AIL_SOURCE_EXPR_UNSUPPORTED",
        "source.expr.unsupported",
        format!("unsupported source expression `{expr}`"),
    )
}

fn source_expr_error(code: &str, category: &str, message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!("{} [{code}] category={category}", message.as_ref()))
}

fn source_effect_const_error(target: &str, capability: &str) -> CliError {
    source_effect_error(
        "AIL_SOURCE_EFFECT_CONST",
        "source.effect.const",
        format!(
            "source const `{target}` uses capability `{capability}`; const declarations must be effect-free"
        ),
    )
}

fn source_effect_grant_missing_error(target: &str, capability: &str) -> CliError {
    source_effect_error(
        "AIL_SOURCE_EFFECT_GRANT_MISSING",
        "source.effect.grant",
        format!("source item `{target}` uses capability `{capability}` without a grant"),
    )
}

fn source_effect_capability_unknown_error(context: &str, capability: &str) -> CliError {
    source_effect_error(
        "AIL_SOURCE_EFFECT_CAPABILITY_UNKNOWN",
        "source.effect.capability",
        format!("{context} capability `{capability}` is not declared"),
    )
}

fn source_effect_call_shape_error() -> CliError {
    source_effect_error(
        "AIL_SOURCE_EFFECT_CALL_SHAPE",
        "source.effect.call_shape",
        "effect_call capability and operation must be identifiers",
    )
}

fn source_effect_error(code: &str, category: &str, message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!("{} [{code}] category={category}", message.as_ref()))
}

fn source_grant_target_unknown_error(target: &str) -> CliError {
    source_grant_error(
        "AIL_SOURCE_GRANT_TARGET_UNKNOWN",
        "source.grant.target",
        format!("grant target `{target}` is not declared as a function or test"),
    )
}

fn source_grant_capability_unknown_error(capability: &str) -> CliError {
    source_grant_error(
        "AIL_SOURCE_GRANT_CAPABILITY_UNKNOWN",
        "source.grant.capability",
        format!("grant capability `{capability}` is not declared"),
    )
}

fn source_grant_target_ambiguous_error(target: &str) -> CliError {
    source_grant_error(
        "AIL_SOURCE_GRANT_TARGET_AMBIGUOUS",
        "source.grant.ambiguous_target",
        format!("grant target `{target}` is ambiguous; use `fn.{target}` or `test.{target}`"),
    )
}

fn source_grant_error(code: &str, category: &str, message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!("{} [{code}] category={category}", message.as_ref()))
}

pub(super) fn known_source_builtin_arity(call: &str) -> Option<SourceArity> {
    let arity = match call {
        "add"
        | "sub"
        | "mul"
        | "div"
        | "mod"
        | "signed_mod"
        | "eq"
        | "ne"
        | "gt"
        | "ge"
        | "lt"
        | "le"
        | "and"
        | "or"
        | "concat"
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
        | "int_min"
        | "int_max"
        | "int_abs_or"
        | "int_neg_or"
        | "int_saturating_add"
        | "int_saturating_sub"
        | "int_saturating_mul"
        | "int_wrapping_add"
        | "int_wrapping_sub"
        | "int_wrapping_mul"
        | "int.bit_and"
        | "int.bit_or"
        | "int.bit_xor"
        | "int.shift_left"
        | "int.shift_right"
        | "int.shift_right_unsigned"
        | "int_bit_and"
        | "int_bit_or"
        | "int_bit_xor"
        | "int_shift_left"
        | "int_shift_right"
        | "int_shift_right_unsigned" => SourceArity::Exact(2),
        "int.saturating_neg" | "int.wrapping_neg" | "int.bit_not" | "int_saturating_neg"
        | "int_wrapping_neg" | "int_bit_not" => SourceArity::Exact(1),
        "text.contains" | "text.ends_with" | "text.index_of" | "text.starts_with"
        | "text_contains" | "text_ends_with" | "text_index_of" | "text_starts_with" | "text.eq"
        | "text_eq" => SourceArity::Exact(2),
        "text.parse_int_or" | "text_parse_int_or" => SourceArity::Exact(2),
        "text.byte_at_or" | "text_byte_at_or" | "text.replace_first" | "text_replace_first"
        | "text.slice" | "text_slice" | "int.clamp" | "int.add_or" | "int.div_or"
        | "int.rem_or" | "int.sub_or" | "int.mul_or" | "int_clamp" | "int_add_or"
        | "int_sub_or" | "int_mul_or" | "int_div_or" | "int_rem_or" | "map.insert"
        | "map_insert" => SourceArity::Exact(3),
        "field" | "index" | "list.get" | "list_get" | "list.push" | "list_push" | "list.concat"
        | "list_concat" | "queue.push_back" | "queue_push_back" | "set.contains"
        | "set_contains" | "set.insert" | "set_insert" | "map.get" | "map_get"
        | "map.contains_key" | "map_contains_key" | "unwrap_or" | "option.unwrap_or"
        | "option_unwrap_or" | "result.unwrap_or" | "result_unwrap_or" | "ok_or"
        | "option.ok_or" | "option_ok_or" | "first_or" | "last_or" => SourceArity::Exact(2),
        "tuple.get" | "tuple_get" => SourceArity::Exact(2),
        "get_or" => SourceArity::Exact(3),
        "update" => SourceArity::Exact(3),
        "none" | "None" => SourceArity::Exact(0),
        "not"
        | "len"
        | "text.len"
        | "text.length"
        | "text_length"
        | "list.length"
        | "list_length"
        | "print"
        | "log.write"
        | "log_write"
        | "text.trim"
        | "text_trim"
        | "Var"
        | "is_empty"
        | "text.is_empty"
        | "text_is_empty"
        | "list.is_empty"
        | "list_is_empty"
        | "queue.pop_front"
        | "queue_pop_front"
        | "queue.peek_front"
        | "queue_peek_front"
        | "queue.length"
        | "queue_length"
        | "queue.is_empty"
        | "queue_is_empty"
        | "set.length"
        | "set_length"
        | "map.length"
        | "map_length"
        | "json.parse"
        | "json_parse"
        | "std.json.parse"
        | "json.stringify"
        | "json_stringify"
        | "std.json.stringify"
        | "numeric.narrow_to_i32"
        | "numeric_narrow_to_i32"
        | "std.numeric.narrow_to_i32"
        | "numeric.narrow_to_u32"
        | "numeric_narrow_to_u32"
        | "std.numeric.narrow_to_u32"
        | "numeric.narrow_to_u64"
        | "numeric_narrow_to_u64"
        | "std.numeric.narrow_to_u64"
        | "numeric.narrow_to_i16"
        | "numeric_narrow_to_i16"
        | "std.numeric.narrow_to_i16"
        | "numeric.narrow_to_u8"
        | "numeric_narrow_to_u8"
        | "std.numeric.narrow_to_u8"
        | "is_some"
        | "is_none"
        | "is_ok"
        | "is_err"
        | "option.is_some"
        | "option_is_some"
        | "option.is_none"
        | "option_is_none"
        | "result.is_ok"
        | "result_is_ok"
        | "result.is_err"
        | "result_is_err"
        | "tuple.length"
        | "tuple_length"
        | "tuple.first"
        | "tuple_first"
        | "tuple.second"
        | "tuple_second" => SourceArity::Exact(1),
        "some" | "Some" | "ok" | "Ok" | "err" | "Err" => SourceArity::Exact(1),
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

pub(super) fn validate_source_expr_vars(
    expr: &str,
    scope: &BTreeSet<&str>,
    constants: &BTreeMap<String, String>,
) -> Result<(), CliError> {
    let expr = expr.trim();
    if expr == "None" {
        return Ok(());
    }
    if is_source_literal(expr) {
        return Ok(());
    }
    if is_malformed_source_string(expr) {
        return Err(source_expr_malformed_string_error(expr));
    }
    if is_unsupported_source_numeric_literal(expr) {
        return Err(source_expr_unsupported_numeric_error(expr));
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
        return Err(source_name_unknown_variable_error(expr));
    }
    Err(source_expr_unsupported_error(expr))
}

pub(super) fn is_source_literal(expr: &str) -> bool {
    let expr = expr.trim();
    expr == "true"
        || expr == "false"
        || is_source_string_literal(expr)
        || expr.parse::<i64>().is_ok()
        || is_source_float_literal(expr)
}

pub(super) fn is_unsupported_source_numeric_literal(expr: &str) -> bool {
    expr.trim()
        .parse::<f64>()
        .map(|value| !value.is_finite())
        .unwrap_or(false)
}

pub(super) fn is_source_float_literal(expr: &str) -> bool {
    expr.trim()
        .parse::<f64>()
        .map(f64::is_finite)
        .unwrap_or(false)
}

pub(super) fn is_malformed_source_string(expr: &str) -> bool {
    expr.trim().starts_with('"') && !is_source_string_literal(expr)
}

pub(super) fn is_source_string_literal(expr: &str) -> bool {
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
