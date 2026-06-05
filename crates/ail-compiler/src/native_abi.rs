// ── ail-compiler::native_abi ─────────────────────────────────────────────
//
// Stable, redacted diagnostics for the native backend ABI/lowering boundary.

use std::collections::BTreeMap;

use crate::anf::{AnfExpr, AnfIr};
use crate::core_ir::LiteralValue;
use crate::error::CompileError;
use crate::native_binding::native_export_name;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeAbiIssueCategory {
    SymbolNameShape,
    UnsupportedTypeLayout,
    ArgumentReturnMismatch,
}

impl NativeAbiIssueCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SymbolNameShape => "symbol-name-shape",
            Self::UnsupportedTypeLayout => "unsupported-type-layout",
            Self::ArgumentReturnMismatch => "argument-return-mismatch",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeAbiIssueCode {
    InvalidSymbolShape,
    DuplicateSymbolShape,
    UnsupportedTypeLayout,
    CallArityMismatch,
    MissingArgumentBinding,
    ArgumentShapeMismatch,
    ReturnShapeMismatch,
}

impl NativeAbiIssueCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidSymbolShape => "AIL-NATIVE-ABI-SYMBOL-SHAPE",
            Self::DuplicateSymbolShape => "AIL-NATIVE-ABI-SYMBOL-DUPLICATE",
            Self::UnsupportedTypeLayout => "AIL-NATIVE-ABI-UNSUPPORTED-LAYOUT",
            Self::CallArityMismatch => "AIL-NATIVE-ABI-CALL-ARITY",
            Self::MissingArgumentBinding => "AIL-NATIVE-ABI-MISSING-ARG",
            Self::ArgumentShapeMismatch => "AIL-NATIVE-ABI-ARG-SHAPE",
            Self::ReturnShapeMismatch => "AIL-NATIVE-ABI-RETURN-SHAPE",
        }
    }

    pub const fn category(self) -> NativeAbiIssueCategory {
        match self {
            Self::InvalidSymbolShape | Self::DuplicateSymbolShape => {
                NativeAbiIssueCategory::SymbolNameShape
            }
            Self::UnsupportedTypeLayout => NativeAbiIssueCategory::UnsupportedTypeLayout,
            Self::CallArityMismatch
            | Self::MissingArgumentBinding
            | Self::ArgumentShapeMismatch
            | Self::ReturnShapeMismatch => NativeAbiIssueCategory::ArgumentReturnMismatch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeAbiIssue {
    pub code: NativeAbiIssueCode,
    pub category: NativeAbiIssueCategory,
    pub descriptor: String,
}

impl NativeAbiIssue {
    fn new(code: NativeAbiIssueCode, descriptor: impl Into<String>) -> Self {
        Self {
            code,
            category: code.category(),
            descriptor: descriptor.into(),
        }
    }

    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn category_str(&self) -> &'static str {
        self.category.as_str()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeAbiDiagnostic {
    pub issues: Vec<NativeAbiIssue>,
}

impl NativeAbiDiagnostic {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn to_error_message(&self) -> String {
        let body = self
            .issues
            .iter()
            .map(|issue| {
                format!(
                    "{} category={} descriptor={}",
                    issue.code_str(),
                    issue.category_str(),
                    issue.descriptor
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!("native ABI validation failed: {body}")
    }

    pub fn into_result(self) -> Result<(), CompileError> {
        let blocking = Self {
            issues: self
                .issues
                .into_iter()
                .filter(|issue| issue.code != NativeAbiIssueCode::InvalidSymbolShape)
                .collect(),
        };
        if blocking.is_valid() {
            Ok(())
        } else {
            Err(CompileError::NativeEncodingError(
                blocking.to_error_message(),
            ))
        }
    }
}

pub fn validate_native_abi(anf: &AnfIr) -> NativeAbiDiagnostic {
    let mut issues = Vec::new();
    validate_symbols(anf, &mut issues);
    for (index, binding) in anf.bindings.iter().enumerate() {
        let mut ctx = BindingCtx::new(index, &mut issues);
        let _ = ctx.expr(&binding.expr);
    }
    issues.sort();
    issues.dedup();
    NativeAbiDiagnostic { issues }
}

fn validate_symbols(anf: &AnfIr, issues: &mut Vec<NativeAbiIssue>) {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (index, binding) in anf.bindings.iter().enumerate() {
        let export_name = native_export_name(&binding.name);
        if export_name != binding.name || !is_stable_symbol(&export_name) {
            issues.push(NativeAbiIssue::new(
                NativeAbiIssueCode::InvalidSymbolShape,
                format!(
                    "binding#{index:04}/symbol:requires-sanitization/export-len:{}",
                    export_name.len()
                ),
            ));
        }
        if let Some(first) = seen.insert(export_name.clone(), index) {
            issues.push(NativeAbiIssue::new(
                NativeAbiIssueCode::DuplicateSymbolShape,
                format!(
                    "binding#{first:04}+binding#{index:04}/symbol:duplicate/export-len:{}",
                    export_name.len()
                ),
            ));
        }
    }
}

fn is_stable_symbol(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    I64,
    I8,
    F64,
    Unit,
    Never,
    Unknown,
}

impl Shape {
    const fn is_value(self) -> bool {
        matches!(self, Self::I64 | Self::I8 | Self::F64)
    }

    const fn desc(self) -> &'static str {
        match self {
            Self::I64 => "shape:i64",
            Self::I8 => "shape:i8",
            Self::F64 => "shape:f64",
            Self::Unit => "shape:unit",
            Self::Never => "shape:never",
            Self::Unknown => "shape:unknown",
        }
    }

    const fn compatible(self, other: Self) -> bool {
        matches!(self, Self::Never | Self::Unknown)
            || matches!(other, Self::Never | Self::Unknown)
            || matches!((self, other), (Self::I64, Self::I64))
            || matches!((self, other), (Self::I8, Self::I8))
            || matches!((self, other), (Self::F64, Self::F64))
            || matches!((self, other), (Self::Unit, Self::Unit))
    }
}

struct BindingCtx<'a> {
    binding: usize,
    locals: BTreeMap<String, Shape>,
    loop_breaks: Vec<Option<Shape>>,
    issues: &'a mut Vec<NativeAbiIssue>,
}

impl<'a> BindingCtx<'a> {
    fn new(binding: usize, issues: &'a mut Vec<NativeAbiIssue>) -> Self {
        Self {
            binding,
            locals: BTreeMap::new(),
            loop_breaks: Vec::new(),
            issues,
        }
    }

    fn expr(&mut self, expr: &AnfExpr) -> Shape {
        match expr {
            AnfExpr::Literal(value) => literal_shape(value),
            AnfExpr::Var(name) => self.local(name, "var"),
            AnfExpr::Let { name, value, body } => {
                let value_shape = self.expr(value);
                if value_shape.is_value() {
                    self.locals.insert(name.clone(), value_shape);
                }
                if matches!(value_shape, Shape::Never) {
                    Shape::Never
                } else {
                    self.expr(body)
                }
            }
            AnfExpr::Call { func, args } => self.call(func, args),
            AnfExpr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.local(cond, "if-cond");
                let then_shape = self.expr(then_branch);
                let else_shape = self.expr(else_branch);
                self.merge("if", then_shape, else_shape)
            }
            AnfExpr::Match { scrutinee, arms } => {
                self.local(scrutinee, "match-scrutinee");
                let Some((first, rest)) = arms.split_first() else {
                    return Shape::Never;
                };
                let mut shape = self.expr(&first.body);
                for arm in rest {
                    let arm_shape = self.expr(&arm.body);
                    shape = self.merge("match", shape, arm_shape);
                }
                shape
            }
            AnfExpr::Return(inner) => self.expr(inner),
            AnfExpr::Seq(exprs) => exprs.iter().fold(Shape::Unit, |_, expr| self.expr(expr)),
            AnfExpr::RecordNew { fields } => {
                for (_, value) in fields {
                    self.value_expr("record-field", value);
                }
                Shape::I64
            }
            AnfExpr::TupleNew(items) | AnfExpr::ListNew(items) => {
                for item in items {
                    self.value_expr("compound-item", item);
                }
                Shape::I64
            }
            AnfExpr::VariantNew { payload, .. } => {
                if let Some(payload) = payload {
                    self.value_expr("variant-payload", payload);
                }
                Shape::I64
            }
            AnfExpr::FieldGet { record, .. } => {
                self.value_local(record, "field-get-record");
                Shape::I64
            }
            AnfExpr::FieldUpdate { record, value, .. } => {
                self.value_local(record, "field-update-record");
                self.value_expr("field-update-value", value);
                Shape::I64
            }
            AnfExpr::Loop { body } => {
                self.loop_breaks.push(None);
                let body_shape = self.expr(body);
                self.loop_breaks.pop().flatten().unwrap_or(body_shape)
            }
            AnfExpr::Break { value } => {
                let shape = self.expr(value);
                if let Some(slot) = self.loop_breaks.last_mut() {
                    if let Some(previous) = *slot {
                        if !previous.compatible(shape) {
                            self.return_mismatch("loop-break", previous, shape);
                        }
                    } else {
                        *slot = Some(shape);
                    }
                } else {
                    self.issue(
                        NativeAbiIssueCode::ArgumentShapeMismatch,
                        "break/outside-loop".to_string(),
                    );
                }
                Shape::Never
            }
            AnfExpr::Continue | AnfExpr::Abort { .. } | AnfExpr::Placeholder => Shape::Never,
            AnfExpr::WhileLoop { cond, body } => {
                self.local(cond, "while-cond");
                self.loop_breaks.push(None);
                let _ = self.expr(body);
                self.loop_breaks.pop();
                Shape::Unit
            }
            AnfExpr::ShortCircuitAnd { left, right } | AnfExpr::ShortCircuitOr { left, right } => {
                self.local(left, "short-circuit-left");
                self.value_expr("short-circuit-right", right);
                Shape::I64
            }
            AnfExpr::Lambda {
                params,
                captures,
                body,
            } => {
                for capture in captures {
                    self.value_local(capture, "lambda-capture");
                }
                let saved = self.locals.clone();
                for param in params {
                    self.locals.insert(param.clone(), Shape::I64);
                }
                let body_shape = self.expr(body);
                if matches!(body_shape, Shape::Unknown) {
                    self.unsupported("lambda-body", body_shape);
                }
                self.locals = saved;
                Shape::I64
            }
            AnfExpr::EffectCall { args, .. }
            | AnfExpr::TaskSpawn { args, .. }
            | AnfExpr::ResourceAcquire { args, .. }
            | AnfExpr::Dispatch { args, .. } => {
                for arg in args {
                    self.value_local(arg, "runtime-arg");
                }
                Shape::I64
            }
            AnfExpr::TaskAwait { task } | AnfExpr::TaskCancel { task } => {
                self.value_local(task, "runtime-handle");
                Shape::I64
            }
            AnfExpr::ChannelNew { .. } => Shape::I64,
            AnfExpr::ChannelSend { channel, value } => {
                self.value_local(channel, "channel");
                self.value_local(value, "channel-value");
                Shape::I64
            }
            AnfExpr::ChannelReceive { channel } => {
                self.value_local(channel, "channel");
                Shape::I64
            }
            AnfExpr::RuntimeCheck { cond, .. } => {
                self.local(cond, "runtime-check-cond");
                Shape::Unit
            }
            AnfExpr::ResourceRelease { handle } => {
                self.value_local(handle, "resource-handle");
                Shape::I64
            }
            AnfExpr::TaskGroup { body } => {
                let _ = self.expr(body);
                Shape::I64
            }
            AnfExpr::Select { branches } => {
                if branches.is_empty() {
                    self.unsupported("select-empty", Shape::Unknown);
                }
                for branch in branches {
                    self.value_local(&branch.channel, "select-channel");
                    self.locals.insert(branch.binding.clone(), Shape::I64);
                    self.value_expr("select-body", &branch.body);
                }
                Shape::I64
            }
            AnfExpr::Timeout { duration, body } => {
                self.value_local(duration, "timeout-duration");
                let _ = self.expr(body);
                Shape::I64
            }
            AnfExpr::CellNew { init } => {
                self.value_local(init, "cell-init");
                Shape::I64
            }
            AnfExpr::CellGet { cell } => {
                self.value_local(cell, "cell");
                Shape::I64
            }
            AnfExpr::CellSet { cell, value } => {
                self.value_local(cell, "cell");
                self.value_local(value, "cell-value");
                Shape::Unit
            }
            AnfExpr::Assume { .. } => Shape::Unit,
            AnfExpr::IndexGet { collection, index } => {
                self.value_local(collection, "index-collection");
                self.value_local(index, "index-index");
                Shape::I64
            }
            AnfExpr::MapNew { entries } => {
                for (key, value) in entries {
                    self.value_local(key, "map-key");
                    self.value_local(value, "map-value");
                }
                Shape::I64
            }
            AnfExpr::SetNew { elements } => {
                for element in elements {
                    self.value_local(element, "set-element");
                }
                Shape::I64
            }
            AnfExpr::ForEach {
                binding,
                collection,
                body,
            } => {
                self.value_local(collection, "foreach-collection");
                self.locals.insert(binding.clone(), Shape::I64);
                let _ = self.expr(body);
                Shape::Unit
            }
            AnfExpr::Fold { init, list, func } => {
                self.value_local(init, "fold-init");
                self.value_local(list, "fold-list");
                self.value_local(func, "fold-func");
                Shape::I64
            }
        }
    }

    fn call(&mut self, func: &str, args: &[String]) -> Shape {
        let Some((arity, result)) = call_abi(func) else {
            self.unsupported("call-target", Shape::Unknown);
            return Shape::Unknown;
        };
        if args.len() != arity {
            self.issue(
                NativeAbiIssueCode::CallArityMismatch,
                format!("call:arity/expected:{arity}/actual:{}", args.len()),
            );
        }
        for (index, arg) in args.iter().enumerate() {
            let shape = self.local(arg, "call-arg");
            if shape.is_value() && shape != Shape::I64 {
                self.issue(
                    NativeAbiIssueCode::ArgumentShapeMismatch,
                    format!(
                        "call-arg#{index:02}/expected:{}/actual:{}",
                        Shape::I64.desc(),
                        shape.desc()
                    ),
                );
            }
        }
        result
    }

    fn local(&mut self, name: &str, context: &str) -> Shape {
        self.locals.get(name).copied().unwrap_or_else(|| {
            self.issue(
                NativeAbiIssueCode::MissingArgumentBinding,
                format!("{context}/missing-local"),
            );
            Shape::Unknown
        })
    }

    fn value_local(&mut self, name: &str, context: &str) -> Shape {
        let shape = self.local(name, context);
        if !shape.is_value() && !matches!(shape, Shape::Unknown) {
            self.unsupported(context, shape);
        }
        shape
    }

    fn value_expr(&mut self, context: &str, expr: &AnfExpr) -> Shape {
        let shape = self.expr(expr);
        if !shape.is_value() {
            self.unsupported(context, shape);
        }
        shape
    }

    fn merge(&mut self, context: &str, left: Shape, right: Shape) -> Shape {
        if !left.compatible(right) {
            self.return_mismatch(context, left, right);
            Shape::Unknown
        } else if matches!(left, Shape::Never | Shape::Unknown) {
            right
        } else {
            left
        }
    }

    fn return_mismatch(&mut self, context: &str, expected: Shape, actual: Shape) {
        self.issue(
            NativeAbiIssueCode::ReturnShapeMismatch,
            format!(
                "{context}/expected:{}/actual:{}",
                expected.desc(),
                actual.desc()
            ),
        );
    }

    fn unsupported(&mut self, context: &str, shape: Shape) {
        self.issue(
            NativeAbiIssueCode::UnsupportedTypeLayout,
            format!("{context}/{}", shape.desc()),
        );
    }

    fn issue(&mut self, code: NativeAbiIssueCode, suffix: String) {
        self.issues.push(NativeAbiIssue::new(
            code,
            format!("binding#{:04}/{suffix}", self.binding),
        ));
    }
}

fn literal_shape(value: &LiteralValue) -> Shape {
    match value {
        LiteralValue::Bool(_) => Shape::I8,
        LiteralValue::Int(_) | LiteralValue::Text(_) | LiteralValue::Bytes(_) => Shape::I64,
        LiteralValue::Float(_) => Shape::F64,
        LiteralValue::Unit => Shape::Unit,
    }
}

fn call_abi(func: &str) -> Option<(usize, Shape)> {
    let arity = match func {
        "i64.neg" | "neg" | "negate" | "i64.eqz" | "not" | "!" | "int.bit_not" | "int_bit_not"
        | "int.wrapping_neg" | "int_wrapping_neg" | "int.saturating_neg" | "int_saturating_neg"
        | "bytes.length" | "bytes_length" | "std.bytes.length" | "bytes.empty" | "bytes_empty"
        | "std.bytes.empty" => 1,
        "int.clamp" | "int_clamp" | "int.add_or" | "int_add_or" | "int.sub_or" | "int_sub_or"
        | "int.mul_or" | "int_mul_or" | "int.div_or" | "int_div_or" | "int.rem_or"
        | "int_rem_or" => 3,
        _ => 2,
    };
    let result = match func {
        "i64.eq" | "==" | "eq" | "i64.ne" | "!=" | "ne" | "i64.lt_s" | "<" | "lt" | "i64.le_s"
        | "<=" | "le" | "i64.gt_s" | ">" | "gt" | "i64.ge_s" | ">=" | "ge" | "i64.eqz" | "not"
        | "!" | "bytes.empty" | "bytes_empty" | "std.bytes.empty" => Shape::I8,
        "i64.add"
        | "+"
        | "add"
        | "i64.sub"
        | "-"
        | "sub"
        | "i64.mul"
        | "*"
        | "mul"
        | "i64.div_s"
        | "/"
        | "div"
        | "i64.rem_s"
        | "%"
        | "mod"
        | "i64.and"
        | "and"
        | "i64.or"
        | "or"
        | "i64.xor"
        | "xor"
        | "i64.neg"
        | "neg"
        | "negate"
        | "int.min"
        | "int_min"
        | "int.max"
        | "int_max"
        | "int.clamp"
        | "int_clamp"
        | "int.abs_or"
        | "int_abs_or"
        | "int.neg_or"
        | "int_neg_or"
        | "int.saturating_neg"
        | "int_saturating_neg"
        | "int.wrapping_add"
        | "int_wrapping_add"
        | "int.wrapping_sub"
        | "int_wrapping_sub"
        | "int.wrapping_mul"
        | "int_wrapping_mul"
        | "int.wrapping_neg"
        | "int_wrapping_neg"
        | "int.bit_and"
        | "int_bit_and"
        | "int.bit_or"
        | "int_bit_or"
        | "int.bit_xor"
        | "int_bit_xor"
        | "int.bit_not"
        | "int_bit_not"
        | "int.shift_left"
        | "int_shift_left"
        | "int.shift_right"
        | "int_shift_right"
        | "int.shift_right_unsigned"
        | "int_shift_right_unsigned"
        | "int.add_or"
        | "int_add_or"
        | "int.sub_or"
        | "int_sub_or"
        | "int.mul_or"
        | "int_mul_or"
        | "int.saturating_add"
        | "int_saturating_add"
        | "int.saturating_sub"
        | "int_saturating_sub"
        | "int.saturating_mul"
        | "int_saturating_mul"
        | "int.div_or"
        | "int_div_or"
        | "int.rem_or"
        | "int_rem_or"
        | "bytes.at"
        | "bytes_at"
        | "std.bytes.at"
        | "bytes.length"
        | "bytes_length"
        | "std.bytes.length" => Shape::I64,
        _ => return None,
    };
    Some((arity, result))
}
