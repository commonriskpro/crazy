use super::*;

pub(super) fn lower_source_int_bounds_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
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
        "int_wrapping_add" => ("int.wrapping_add", "int_wrapping_add(left, right)"),
        "int_wrapping_sub" => ("int.wrapping_sub", "int_wrapping_sub(left, right)"),
        "int_wrapping_mul" => ("int.wrapping_mul", "int_wrapping_mul(left, right)"),
        "int_wrapping_neg" => ("int.wrapping_neg", "int_wrapping_neg(value)"),
        "int_bit_and" => ("int.bit_and", "int_bit_and(left, right)"),
        "int_bit_or" => ("int.bit_or", "int_bit_or(left, right)"),
        "int_bit_xor" => ("int.bit_xor", "int_bit_xor(left, right)"),
        "int_bit_not" => ("int.bit_not", "int_bit_not(value)"),
        "int_shift_left" => ("int.shift_left", "int_shift_left(value, amount)"),
        "int_shift_right" => ("int.shift_right", "int_shift_right(value, amount)"),
        "int_shift_right_unsigned" => (
            "int.shift_right_unsigned",
            "int_shift_right_unsigned(value, amount)",
        ),
        "int_saturating_neg" => ("int.saturating_neg", "int_saturating_neg(value)"),
        "int_abs_or" => ("int.abs_or", "int_abs_or(value, fallback)"),
        "int_neg_or" => ("int.neg_or", "int_neg_or(value, fallback)"),
        "int_div_or" => ("int.div_or", "int_div_or(value, divisor, fallback)"),
        "int_rem_or" => ("int.rem_or", "int_rem_or(value, divisor, fallback)"),
        _ => return Ok(None),
    };
    let expected_len = if matches!(
        func.as_str(),
        "int_saturating_neg" | "int_wrapping_neg" | "int_bit_not"
    ) {
        1
    } else if matches!(
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
