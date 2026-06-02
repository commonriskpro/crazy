use super::*;

pub(super) fn lower_source_fs_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((capability, operation, arity, usage)) = source_fs_helper_lowering(&func) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::FsHelper,
            format!("{func} requires `{usage}`"),
        ));
    }
    let mut lowered = vec![capability.to_string(), operation.to_string()];
    lowered.extend(
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(Some(format!("effect_call({})", lowered.join(", "))))
}

fn source_fs_helper_lowering(
    func: &str,
) -> Option<(&'static str, &'static str, usize, &'static str)> {
    match func {
        "fs.read_file" | "fs_read_file" | "std.fs.read_file" => {
            Some(("file.read", "read", 1, "fs_read_file(path)"))
        }
        "fs.write" | "fs_write" | "std.fs.write" => {
            Some(("file.write", "write", 2, "fs_write(path, bytes)"))
        }
        "fs.delete" | "fs_delete" | "std.fs.delete" => {
            Some(("file.delete", "delete", 1, "fs_delete(path)"))
        }
        "fs.list" | "fs_list" | "std.fs.list" => Some(("file.list", "list", 1, "fs_list(path)")),
        _ => None,
    }
}
