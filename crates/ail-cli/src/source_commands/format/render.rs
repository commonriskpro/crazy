use super::*;

pub(super) fn render_source_import(out: &mut String, import: &str) {
    out.push_str(&format!("use \"{import}\"\n"));
}

pub(super) fn render_source_module(out: &mut String, module: &str) {
    out.push_str(&format!("module {module}\n"));
}

pub(super) fn render_source_capability(out: &mut String, capability: &str) {
    out.push_str(&format!("capability {capability}\n"));
}

pub(super) fn render_source_const(
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

pub(super) fn render_source_function(
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

pub(super) fn render_source_test(
    out: &mut String,
    test: &SourceTest,
    module: Option<&str>,
    constants: &BTreeMap<String, String>,
) {
    let name = render_source_decl_name(
        test.name.strip_prefix("test.").unwrap_or(&test.name),
        module,
    );
    let (lets, final_expr) = source_let_chain(&test.body);
    if !lets.is_empty() {
        if test.return_type == "Bool" {
            out.push_str(&format!("test {name} {{\n"));
        } else {
            out.push_str(&format!("test {name} -> {} {{\n", test.return_type));
        }
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
        return;
    }

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

pub(super) fn render_source_grant(out: &mut String, grant: &SourceGrant, module: Option<&str>) {
    let raw_target = grant
        .target
        .strip_prefix("fn.")
        .or_else(|| grant.target.strip_prefix("test."))
        .unwrap_or(&grant.target);
    let target = render_source_decl_name(raw_target, module);
    out.push_str(&format!("grant {target} {}\n", grant.capability));
}

pub(super) fn render_source_decl_name(name: &str, module: Option<&str>) -> String {
    module
        .and_then(|module| name.strip_prefix(&format!("{module}.")))
        .unwrap_or(name)
        .to_string()
}

pub(super) fn format_source_call_name(func: &str, module: Option<&str>) -> String {
    render_source_decl_name(func, module)
}

pub(super) struct SourceLetBinding {
    name: String,
    ty: Option<String>,
    value: String,
}

pub(super) fn source_let_chain(body: &str) -> (Vec<SourceLetBinding>, String) {
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
