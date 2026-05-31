use crate::anf::{AnfBinding, AnfExpr};

use super::{
    anf_node_count, cse_bindings, eliminate_dead_pure, inline_small_pure, is_pure,
    optimize_bindings, purity_blocking_reason, uses_var,
};

const INLINE_NODE_LIMIT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptimizerPass {
    EliminateDeadPure,
    InlineSmallPure,
    CseBindings,
    OptimizeBindings,
}

impl OptimizerPass {
    pub const ALL: [Self; 4] = [
        Self::EliminateDeadPure,
        Self::InlineSmallPure,
        Self::CseBindings,
        Self::OptimizeBindings,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::EliminateDeadPure => "eliminate_dead_pure",
            Self::InlineSmallPure => "inline_small_pure",
            Self::CseBindings => "cse_bindings",
            Self::OptimizeBindings => "optimize_bindings",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptimizerIssueKind {
    PassDisabled,
    UnsupportedIrShape,
    PurityBlocked,
    OptimizationBlocked,
    NonIdempotentPass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptimizerSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptimizerDiagnostic {
    pub pass: OptimizerPass,
    pub kind: OptimizerIssueKind,
    pub severity: OptimizerSeverity,
    pub binding: String,
    pub node: String,
    pub function: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OptimizerDiagnostics {
    pub issues: Vec<OptimizerDiagnostic>,
}

impl OptimizerDiagnostics {
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &OptimizerDiagnostic> {
        self.issues.iter()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptimizerDiagnosticConfig {
    disabled_passes: Vec<OptimizerPass>,
}

impl OptimizerDiagnosticConfig {
    pub fn all_enabled() -> Self {
        Self {
            disabled_passes: Vec::new(),
        }
    }

    pub fn with_disabled_pass(mut self, pass: OptimizerPass) -> Self {
        if !self.disabled_passes.contains(&pass) {
            self.disabled_passes.push(pass);
            self.disabled_passes.sort();
        }
        self
    }

    pub fn pass_enabled(&self, pass: OptimizerPass) -> bool {
        !self.disabled_passes.contains(&pass)
    }
}

impl Default for OptimizerDiagnosticConfig {
    fn default() -> Self {
        Self::all_enabled()
    }
}

pub fn diagnose_optimizer(bindings: &[AnfBinding]) -> OptimizerDiagnostics {
    diagnose_optimizer_with_config(bindings, &OptimizerDiagnosticConfig::default())
}

pub fn diagnose_optimizer_with_config(
    bindings: &[AnfBinding],
    config: &OptimizerDiagnosticConfig,
) -> OptimizerDiagnostics {
    let mut issues = Vec::new();

    for pass in OptimizerPass::ALL {
        if !config.pass_enabled(pass) {
            issues.push(OptimizerDiagnostic {
                pass,
                kind: OptimizerIssueKind::PassDisabled,
                severity: OptimizerSeverity::Info,
                binding: "optimizer.pipeline".to_string(),
                node: "optimizer.pass".to_string(),
                function: None,
                detail: format!("{} disabled by diagnostic configuration", pass.name()),
            });
            continue;
        }

        diagnose_unsupported_shapes(pass, bindings, &mut issues);
        diagnose_blockers(pass, bindings, &mut issues);
        diagnose_idempotence(pass, bindings, &mut issues);
    }

    sort_deterministically(&mut issues);
    OptimizerDiagnostics { issues }
}

pub fn optimize_bindings_with_diagnostics(
    bindings: Vec<AnfBinding>,
) -> (Vec<AnfBinding>, OptimizerDiagnostics) {
    let diagnostics = diagnose_optimizer(&bindings);
    (optimize_bindings(bindings), diagnostics)
}

fn diagnose_unsupported_shapes(
    pass: OptimizerPass,
    bindings: &[AnfBinding],
    issues: &mut Vec<OptimizerDiagnostic>,
) {
    for binding in bindings {
        visit_expr(&binding.expr, &mut |expr| {
            if let Some(reason) = unsupported_reason(pass, expr) {
                issues.push(issue(
                    pass,
                    OptimizerIssueKind::UnsupportedIrShape,
                    OptimizerSeverity::Warning,
                    binding,
                    expr,
                    reason.to_string(),
                ));
            }
        });
    }
}

fn diagnose_blockers(
    pass: OptimizerPass,
    bindings: &[AnfBinding],
    issues: &mut Vec<OptimizerDiagnostic>,
) {
    for binding in bindings {
        match pass {
            OptimizerPass::EliminateDeadPure => visit_expr(&binding.expr, &mut |expr| {
                if let AnfExpr::Seq(exprs) = expr {
                    for expr in exprs.iter().take(exprs.len().saturating_sub(1)) {
                        if let Some(reason) = purity_blocking_reason(expr) {
                            issues.push(issue(
                                pass,
                                OptimizerIssueKind::PurityBlocked,
                                OptimizerSeverity::Info,
                                binding,
                                expr,
                                format!(
                                    "non-final sequence element retained: {} ({})",
                                    reason.shape, reason.reason
                                ),
                            ));
                        }
                    }
                }
            }),
            OptimizerPass::InlineSmallPure => {
                if let AnfExpr::Lambda { body, .. } = &binding.expr {
                    if let Some(reason) = purity_blocking_reason(body) {
                        issues.push(issue(
                            pass,
                            OptimizerIssueKind::PurityBlocked,
                            OptimizerSeverity::Info,
                            binding,
                            body,
                            format!(
                                "lambda not inlined: {} blocks purity ({})",
                                reason.shape, reason.reason
                            ),
                        ));
                    } else if anf_node_count(body) > INLINE_NODE_LIMIT {
                        issues.push(issue(
                            pass,
                            OptimizerIssueKind::OptimizationBlocked,
                            OptimizerSeverity::Info,
                            binding,
                            body,
                            format!(
                                "lambda not inlined: node_count={} exceeds limit={}",
                                anf_node_count(body),
                                INLINE_NODE_LIMIT
                            ),
                        ));
                    }
                }
            }
            OptimizerPass::CseBindings => visit_expr(&binding.expr, &mut |expr| {
                if let AnfExpr::Let { value, .. } = expr
                    && !is_pure(value)
                    && let Some(reason) = purity_blocking_reason(value)
                {
                    issues.push(issue(
                        pass,
                        OptimizerIssueKind::PurityBlocked,
                        OptimizerSeverity::Info,
                        binding,
                        value,
                        format!(
                            "let value not shared: {} blocks purity ({})",
                            reason.shape, reason.reason
                        ),
                    ));
                }
            }),
            OptimizerPass::OptimizeBindings => visit_expr(&binding.expr, &mut |expr| {
                if let AnfExpr::Let { name, value, body } = expr
                    && !uses_var(body, name)
                    && !is_pure(value)
                    && let Some(reason) = purity_blocking_reason(value)
                {
                    issues.push(issue(
                        pass,
                        OptimizerIssueKind::PurityBlocked,
                        OptimizerSeverity::Info,
                        binding,
                        value,
                        format!(
                            "dead let retained: {} blocks purity ({})",
                            reason.shape, reason.reason
                        ),
                    ));
                }
            }),
        }
    }
}

fn diagnose_idempotence(
    pass: OptimizerPass,
    bindings: &[AnfBinding],
    issues: &mut Vec<OptimizerDiagnostic>,
) {
    let once = apply_pass(pass, bindings.to_vec());
    let twice = apply_pass(pass, once.clone());
    if once != twice {
        issues.push(OptimizerDiagnostic {
            pass,
            kind: OptimizerIssueKind::NonIdempotentPass,
            severity: OptimizerSeverity::Error,
            binding: "optimizer.pipeline".to_string(),
            node: "optimizer.pass".to_string(),
            function: None,
            detail: format!(
                "{} produced additional changes when applied twice",
                pass.name()
            ),
        });
    }
}

fn apply_pass(pass: OptimizerPass, bindings: Vec<AnfBinding>) -> Vec<AnfBinding> {
    match pass {
        OptimizerPass::EliminateDeadPure => eliminate_dead_pure(bindings),
        OptimizerPass::InlineSmallPure => inline_small_pure(bindings),
        OptimizerPass::CseBindings => cse_bindings(bindings),
        OptimizerPass::OptimizeBindings => optimize_bindings(bindings),
    }
}

fn unsupported_reason(pass: OptimizerPass, expr: &AnfExpr) -> Option<&'static str> {
    match pass {
        OptimizerPass::EliminateDeadPure => match expr {
            AnfExpr::Select { .. } | AnfExpr::ForEach { .. } => {
                Some("pass does not rewrite this control-flow shape")
            }
            _ => None,
        },
        OptimizerPass::InlineSmallPure | OptimizerPass::CseBindings => match expr {
            AnfExpr::Select { .. }
            | AnfExpr::ForEach { .. }
            | AnfExpr::Fold { .. }
            | AnfExpr::WhileLoop { .. }
            | AnfExpr::ShortCircuitAnd { .. }
            | AnfExpr::ShortCircuitOr { .. } => {
                Some("pass does not inspect this IR shape for opportunities")
            }
            _ => None,
        },
        OptimizerPass::OptimizeBindings => match expr {
            AnfExpr::Select { .. } | AnfExpr::ForEach { .. } | AnfExpr::Fold { .. } => {
                Some("constant folding does not inspect this IR shape")
            }
            _ => None,
        },
    }
}

fn issue(
    pass: OptimizerPass,
    kind: OptimizerIssueKind,
    severity: OptimizerSeverity,
    binding: &AnfBinding,
    expr: &AnfExpr,
    detail: String,
) -> OptimizerDiagnostic {
    OptimizerDiagnostic {
        pass,
        kind,
        severity,
        binding: redacted_binding_descriptor(binding),
        node: redacted_node_descriptor(expr),
        function: function_descriptor(expr),
        detail,
    }
}

fn sort_deterministically(issues: &mut Vec<OptimizerDiagnostic>) {
    issues.sort_by(|left, right| {
        (
            left.pass,
            left.kind,
            left.severity,
            &left.binding,
            &left.node,
            &left.function,
            &left.detail,
        )
            .cmp(&(
                right.pass,
                right.kind,
                right.severity,
                &right.binding,
                &right.node,
                &right.function,
                &right.detail,
            ))
    });
    issues.dedup();
}

pub fn redacted_function_descriptor(name: &str) -> String {
    format!("fn#{}", stable_hash_hex(name))
}

pub fn redacted_binding_descriptor(binding: &AnfBinding) -> String {
    format!(
        "binding#{}@node#{}",
        stable_hash_hex(&binding.name),
        binding.source_ref.0
    )
}

pub fn redacted_node_descriptor(expr: &AnfExpr) -> String {
    format!(
        "anf.{}#{}",
        expr_shape(expr),
        stable_hash_hex(&format!("{expr:?}"))
    )
}

fn function_descriptor(expr: &AnfExpr) -> Option<String> {
    match expr {
        AnfExpr::Call { func, .. } | AnfExpr::TaskSpawn { func, .. } => {
            Some(redacted_function_descriptor(func))
        }
        AnfExpr::EffectCall {
            capability, func, ..
        } => Some(redacted_function_descriptor(&format!(
            "{capability}.{func}"
        ))),
        AnfExpr::Dispatch {
            handler, method, ..
        } => Some(redacted_function_descriptor(&format!("{handler}.{method}"))),
        AnfExpr::ResourceAcquire { resource, .. } => Some(redacted_function_descriptor(resource)),
        AnfExpr::Fold { func, .. } => Some(redacted_function_descriptor(func)),
        _ => None,
    }
}

fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn visit_expr(expr: &AnfExpr, visitor: &mut impl FnMut(&AnfExpr)) {
    visitor(expr);
    match expr {
        AnfExpr::Let { value, body, .. } => {
            visit_expr(value, visitor);
            visit_expr(body, visitor);
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            visit_expr(then_branch, visitor);
            visit_expr(else_branch, visitor);
        }
        AnfExpr::Return(inner)
        | AnfExpr::Loop { body: inner }
        | AnfExpr::Break { value: inner }
        | AnfExpr::TaskGroup { body: inner }
        | AnfExpr::Timeout { body: inner, .. }
        | AnfExpr::Lambda { body: inner, .. } => visit_expr(inner, visitor),
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            for expr in exprs {
                visit_expr(expr, visitor);
            }
        }
        AnfExpr::RecordNew { fields } => {
            for (_, expr) in fields {
                visit_expr(expr, visitor);
            }
        }
        AnfExpr::FieldUpdate { value, .. } => visit_expr(value, visitor),
        AnfExpr::VariantNew { payload, .. } => {
            if let Some(payload) = payload {
                visit_expr(payload, visitor);
            }
        }
        AnfExpr::Match { arms, .. } => {
            for arm in arms {
                visit_expr(&arm.body, visitor);
            }
        }
        AnfExpr::Select { branches } => {
            for branch in branches {
                visit_expr(&branch.body, visitor);
            }
        }
        AnfExpr::WhileLoop { body, .. } | AnfExpr::ForEach { body, .. } => {
            visit_expr(body, visitor);
        }
        AnfExpr::ShortCircuitAnd { right, .. } | AnfExpr::ShortCircuitOr { right, .. } => {
            visit_expr(right, visitor);
        }
        AnfExpr::Literal(_)
        | AnfExpr::Var(_)
        | AnfExpr::Call { .. }
        | AnfExpr::FieldGet { .. }
        | AnfExpr::Continue
        | AnfExpr::EffectCall { .. }
        | AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::IndexGet { .. }
        | AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::Fold { .. }
        | AnfExpr::Placeholder => {}
    }
}

fn expr_shape(expr: &AnfExpr) -> &'static str {
    match expr {
        AnfExpr::Literal(_) => "Literal",
        AnfExpr::Var(_) => "Var",
        AnfExpr::Let { .. } => "Let",
        AnfExpr::If { .. } => "If",
        AnfExpr::Call { .. } => "Call",
        AnfExpr::FieldGet { .. } => "FieldGet",
        AnfExpr::Return(_) => "Return",
        AnfExpr::Seq(_) => "Seq",
        AnfExpr::Match { .. } => "Match",
        AnfExpr::Lambda { .. } => "Lambda",
        AnfExpr::RecordNew { .. } => "RecordNew",
        AnfExpr::FieldUpdate { .. } => "FieldUpdate",
        AnfExpr::TupleNew(_) => "TupleNew",
        AnfExpr::VariantNew { .. } => "VariantNew",
        AnfExpr::ListNew(_) => "ListNew",
        AnfExpr::Loop { .. } => "Loop",
        AnfExpr::Break { .. } => "Break",
        AnfExpr::Continue => "Continue",
        AnfExpr::WhileLoop { .. } => "WhileLoop",
        AnfExpr::ShortCircuitAnd { .. } => "ShortCircuitAnd",
        AnfExpr::ShortCircuitOr { .. } => "ShortCircuitOr",
        AnfExpr::EffectCall { .. } => "EffectCall",
        AnfExpr::Dispatch { .. } => "Dispatch",
        AnfExpr::TaskSpawn { .. } => "TaskSpawn",
        AnfExpr::ChannelSend { .. } => "ChannelSend",
        AnfExpr::ChannelReceive { .. } => "ChannelReceive",
        AnfExpr::RuntimeCheck { .. } => "RuntimeCheck",
        AnfExpr::ResourceAcquire { .. } => "ResourceAcquire",
        AnfExpr::ResourceRelease { .. } => "ResourceRelease",
        AnfExpr::TaskAwait { .. } => "TaskAwait",
        AnfExpr::TaskCancel { .. } => "TaskCancel",
        AnfExpr::TaskGroup { .. } => "TaskGroup",
        AnfExpr::ChannelNew { .. } => "ChannelNew",
        AnfExpr::Select { .. } => "Select",
        AnfExpr::Timeout { .. } => "Timeout",
        AnfExpr::CellNew { .. } => "CellNew",
        AnfExpr::CellGet { .. } => "CellGet",
        AnfExpr::CellSet { .. } => "CellSet",
        AnfExpr::Assume { .. } => "Assume",
        AnfExpr::Abort { .. } => "Abort",
        AnfExpr::IndexGet { .. } => "IndexGet",
        AnfExpr::MapNew { .. } => "MapNew",
        AnfExpr::SetNew { .. } => "SetNew",
        AnfExpr::ForEach { .. } => "ForEach",
        AnfExpr::Fold { .. } => "Fold",
        AnfExpr::Placeholder => "Placeholder",
    }
}
