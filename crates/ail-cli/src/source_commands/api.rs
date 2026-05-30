use super::format::*;
use super::lower::*;
use super::model::*;
use super::parse::*;
use super::validate::*;
use super::*;

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
