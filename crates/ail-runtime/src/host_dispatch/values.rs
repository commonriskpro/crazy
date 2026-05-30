// ── ail-runtime::host_dispatch::values ────────────────────────────────────

use std::fmt;

use wasmtime::Val;

// ── RuntimeArg / RuntimeValue ─────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeArg {
    I64(i64),
    I32(i32),
    F64(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeValue {
    I64(i64),
    I32(i32),
    F64(f64),
    Unit,
}

impl fmt::Display for RuntimeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeValue::I64(v) => write!(f, "{v}"),
            RuntimeValue::I32(v) => write!(f, "{v}"),
            RuntimeValue::F64(v) => write!(f, "{v}"),
            RuntimeValue::Unit => write!(f, "()"),
        }
    }
}

pub(super) fn runtime_arg_to_val(arg: &RuntimeArg) -> Val {
    match arg {
        RuntimeArg::I64(value) => Val::I64(*value),
        RuntimeArg::I32(value) => Val::I32(*value),
        RuntimeArg::F64(value) => Val::F64((*value).to_bits()),
    }
}
