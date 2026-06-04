use super::*;

pub(super) const TEXT_BOUNDARY_INVALID_UTF8: &str = "wasm_text_invalid_utf8";
pub(super) const TEXT_BOUNDARY_INDEX_OUT_OF_RANGE: &str = "wasm_text_index_out_of_range";
pub(super) const TEXT_BOUNDARY_SLICE_SPLITS_UTF8: &str = "wasm_text_slice_splits_utf8";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TextBoundaryIssue {
    pub code: &'static str,
    pub operation: &'static str,
    pub boundary: &'static str,
    pub offset: usize,
    pub message: &'static str,
}

impl TextBoundaryIssue {
    fn new(
        code: &'static str,
        operation: &'static str,
        boundary: &'static str,
        offset: usize,
        message: &'static str,
    ) -> Self {
        Self {
            code,
            operation,
            boundary,
            offset,
            message,
        }
    }

    fn sort_key(&self) -> (&'static str, &'static str, &'static str, usize) {
        (self.code, self.operation, self.boundary, self.offset)
    }
}

pub(super) fn validate_text_slice_boundaries(
    value: &[u8],
    start: i64,
    requested_len: i64,
) -> Vec<TextBoundaryIssue> {
    let mut issues = Vec::new();

    let text = match std::str::from_utf8(value) {
        Ok(text) => Some(text),
        Err(err) => {
            issues.push(TextBoundaryIssue::new(
                TEXT_BOUNDARY_INVALID_UTF8,
                "text.slice",
                "value",
                err.valid_up_to(),
                "text bytes must be valid UTF-8 before slice boundary checks",
            ));
            None
        }
    };

    if start < 0 {
        issues.push(TextBoundaryIssue::new(
            TEXT_BOUNDARY_INDEX_OUT_OF_RANGE,
            "text.slice",
            "start",
            0,
            "slice start must be non-negative",
        ));
    }

    if requested_len < 0 {
        issues.push(TextBoundaryIssue::new(
            TEXT_BOUNDARY_INDEX_OUT_OF_RANGE,
            "text.slice",
            "length",
            0,
            "slice length must be non-negative",
        ));
    }

    let Some(text) = text else {
        issues.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        return issues;
    };

    let Ok(start) = usize::try_from(start) else {
        issues.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        return issues;
    };
    let Ok(requested_len) = usize::try_from(requested_len) else {
        issues.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        return issues;
    };

    if start > value.len() {
        issues.push(TextBoundaryIssue::new(
            TEXT_BOUNDARY_INDEX_OUT_OF_RANGE,
            "text.slice",
            "start",
            start,
            "slice start must not exceed text length",
        ));
    } else if !text.is_char_boundary(start) {
        issues.push(TextBoundaryIssue::new(
            TEXT_BOUNDARY_SLICE_SPLITS_UTF8,
            "text.slice",
            "start",
            start,
            "slice start must be a UTF-8 character boundary",
        ));
    }

    if let Some(end) = start.checked_add(requested_len) {
        let clamped_end = end.min(value.len());
        if !text.is_char_boundary(clamped_end) {
            issues.push(TextBoundaryIssue::new(
                TEXT_BOUNDARY_SLICE_SPLITS_UTF8,
                "text.slice",
                "end",
                clamped_end,
                "slice end must be a UTF-8 character boundary",
            ));
        }
    } else {
        issues.push(TextBoundaryIssue::new(
            TEXT_BOUNDARY_INDEX_OUT_OF_RANGE,
            "text.slice",
            "end",
            value.len(),
            "slice end must not overflow",
        ));
    }

    issues.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
    issues
}

pub(crate) fn emit_text_boundary_match<'a>(
    args: &[String],
    ctx: &mut WasmCodegenCtx<'a>,
    insns: &mut Vec<Instruction<'a>>,
    at_end: bool,
) -> Option<ValType> {
    let [haystack, needle] = args else {
        insns.push(Instruction::Unreachable);
        return None;
    };

    let haystack_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, haystack, insns);
    insns.push(Instruction::LocalSet(haystack_len));

    let needle_len = ctx.bind_temp(ValType::I32);
    emit_text_len_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_len));

    let haystack_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, haystack, insns);
    insns.push(Instruction::LocalSet(haystack_ptr));

    let needle_ptr = ctx.bind_temp(ValType::I32);
    emit_text_ptr_from_local(ctx, needle, insns);
    insns.push(Instruction::LocalSet(needle_ptr));

    let start = ctx.bind_temp(ValType::I32);
    let offset = ctx.bind_temp(ValType::I32);
    let result = ctx.bind_temp(ValType::I64);

    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(result));

    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32Eqz);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Else);

    insns.push(Instruction::LocalGet(haystack_len));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::If(BlockType::Empty));

    if at_end {
        insns.push(Instruction::LocalGet(haystack_len));
        insns.push(Instruction::LocalGet(needle_len));
        insns.push(Instruction::I32Sub);
    } else {
        insns.push(Instruction::I32Const(0));
    }
    insns.push(Instruction::LocalSet(start));

    insns.push(Instruction::I64Const(1));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::I32Const(0));
    insns.push(Instruction::LocalSet(offset));

    insns.push(Instruction::Block(BlockType::Empty));
    insns.push(Instruction::Loop(BlockType::Empty));

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::LocalGet(needle_len));
    insns.push(Instruction::I32GeU);
    insns.push(Instruction::BrIf(1));

    insns.push(Instruction::LocalGet(haystack_ptr));
    insns.push(Instruction::LocalGet(start));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::LocalGet(needle_ptr));
    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Add);
    load_i32_u8_at(0, insns);
    insns.push(Instruction::I32Ne);
    insns.push(Instruction::If(BlockType::Empty));
    insns.push(Instruction::I64Const(0));
    insns.push(Instruction::LocalSet(result));
    insns.push(Instruction::Br(2));
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(offset));
    insns.push(Instruction::I32Const(1));
    insns.push(Instruction::I32Add);
    insns.push(Instruction::LocalSet(offset));
    insns.push(Instruction::Br(0));

    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);
    insns.push(Instruction::End);

    insns.push(Instruction::LocalGet(result));
    Some(ValType::I64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_slice_boundary_issues_use_stable_codes_and_order() {
        let issues = validate_text_slice_boundaries("éé".as_bytes(), 1, 2);
        let keys: Vec<(&str, &str, &str, usize)> = issues
            .iter()
            .map(|issue| (issue.code, issue.operation, issue.boundary, issue.offset))
            .collect();

        assert_eq!(
            keys,
            vec![
                (TEXT_BOUNDARY_SLICE_SPLITS_UTF8, "text.slice", "end", 3),
                (TEXT_BOUNDARY_SLICE_SPLITS_UTF8, "text.slice", "start", 1),
            ],
            "slice boundary diagnostics must stay deterministic"
        );
    }

    #[test]
    fn text_slice_boundary_issues_report_invalid_utf8_before_boundaries() {
        let issues = validate_text_slice_boundaries(&[0x66, 0x80, 0x6f], 1, 1);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, TEXT_BOUNDARY_INVALID_UTF8);
        assert_eq!(issues[0].operation, "text.slice");
        assert_eq!(issues[0].boundary, "value");
        assert_eq!(issues[0].offset, 1);
    }

    #[test]
    fn text_slice_boundary_issues_report_index_bounds() {
        let issues = validate_text_slice_boundaries("hello".as_bytes(), -1, -2);
        let keys: Vec<(&str, &str, &str)> = issues
            .iter()
            .map(|issue| (issue.code, issue.operation, issue.boundary))
            .collect();

        assert_eq!(
            keys,
            vec![
                (TEXT_BOUNDARY_INDEX_OUT_OF_RANGE, "text.slice", "length"),
                (TEXT_BOUNDARY_INDEX_OUT_OF_RANGE, "text.slice", "start"),
            ]
        );
    }
}
