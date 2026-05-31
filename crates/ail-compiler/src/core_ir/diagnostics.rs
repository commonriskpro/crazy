// ── ail-compiler::core_ir::diagnostics ───────────────────────────────────
//
// Stable, redacted Core IR diagnostics for production compiler reliability.

use std::collections::{BTreeMap, BTreeSet};

use ail_core::semantic_graph::NodeRef;
use serde::{Deserialize, Serialize};

use super::expr::{CoreExpr, MatchArm, SelectClause};
use super::nodes::{CoreIr, CoreNode};
use super::primitives::{CoreNodeKind, LiteralValue};
use super::types::CoreType;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CoreIrIssueCategory {
    NodeShape,
    TypeShape,
    SymbolBinding,
    MissingReference,
    UnsupportedPrimitive,
}

impl CoreIrIssueCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NodeShape => "node-shape",
            Self::TypeShape => "type-shape",
            Self::SymbolBinding => "symbol-binding",
            Self::MissingReference => "missing-reference",
            Self::UnsupportedPrimitive => "unsupported-primitive",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CoreIrIssueCode {
    InvalidNodeShape,
    InvalidTypeShape,
    DuplicateSymbol,
    DuplicateBinding,
    MissingEntry,
    MissingReference,
    UnsupportedPrimitive,
}

impl CoreIrIssueCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidNodeShape => "AIL-CORE-IR-NODE-SHAPE",
            Self::InvalidTypeShape => "AIL-CORE-IR-TYPE-SHAPE",
            Self::DuplicateSymbol => "AIL-CORE-IR-SYMBOL-DUPLICATE",
            Self::DuplicateBinding => "AIL-CORE-IR-BINDING-DUPLICATE",
            Self::MissingEntry => "AIL-CORE-IR-ENTRY-MISSING",
            Self::MissingReference => "AIL-CORE-IR-REFERENCE-MISSING",
            Self::UnsupportedPrimitive => "AIL-CORE-IR-UNSUPPORTED-PRIMITIVE",
        }
    }

    pub const fn category(self) -> CoreIrIssueCategory {
        match self {
            Self::InvalidNodeShape => CoreIrIssueCategory::NodeShape,
            Self::InvalidTypeShape => CoreIrIssueCategory::TypeShape,
            Self::DuplicateSymbol | Self::DuplicateBinding => CoreIrIssueCategory::SymbolBinding,
            Self::MissingEntry | Self::MissingReference => CoreIrIssueCategory::MissingReference,
            Self::UnsupportedPrimitive => CoreIrIssueCategory::UnsupportedPrimitive,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CoreIrDiagnosticIssue {
    pub code: CoreIrIssueCode,
    pub category: CoreIrIssueCategory,
    pub descriptor: String,
    pub detail: String,
}

impl CoreIrDiagnosticIssue {
    fn new(
        code: CoreIrIssueCode,
        descriptor: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            category: code.category(),
            descriptor: descriptor.into(),
            detail: detail.into(),
        }
    }

    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn category_str(&self) -> &'static str {
        self.category.as_str()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreIrDiagnostic {
    pub issues: Vec<CoreIrDiagnosticIssue>,
}

impl CoreIrDiagnostic {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn to_error_message(&self) -> String {
        let body = self
            .issues
            .iter()
            .map(|issue| {
                format!(
                    "{} category={} descriptor={} detail={}",
                    issue.code_str(),
                    issue.category_str(),
                    issue.descriptor,
                    issue.detail
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!("core IR validation failed: {body}")
    }
}

impl CoreIr {
    /// Return stable, redacted diagnostics using `main` as the production entry symbol.
    pub fn diagnostic_issues(&self) -> Vec<CoreIrDiagnosticIssue> {
        self.diagnostic_issues_for_entry("main")
    }

    /// Return stable, redacted diagnostics for a caller-selected entry symbol.
    pub fn diagnostic_issues_for_entry(&self, entry_symbol: &str) -> Vec<CoreIrDiagnosticIssue> {
        let mut issues = Vec::new();
        let symbols = collect_symbols(self, &mut issues);

        if !entry_symbol.is_empty() && !symbols.contains(entry_symbol) {
            issues.push(CoreIrDiagnosticIssue::new(
                CoreIrIssueCode::MissingEntry,
                "core-ir/entry",
                redacted_symbol_descriptor("entry", entry_symbol),
            ));
        }

        for (index, node) in self.nodes.iter().enumerate() {
            validate_node(index, node, &symbols, &mut issues);
        }

        issues.sort();
        issues.dedup();
        issues
    }

    pub fn diagnostics(&self) -> CoreIrDiagnostic {
        CoreIrDiagnostic {
            issues: self.diagnostic_issues(),
        }
    }

    pub fn diagnostics_for_entry(&self, entry_symbol: &str) -> CoreIrDiagnostic {
        CoreIrDiagnostic {
            issues: self.diagnostic_issues_for_entry(entry_symbol),
        }
    }
}

fn collect_symbols(ir: &CoreIr, issues: &mut Vec<CoreIrDiagnosticIssue>) -> BTreeSet<String> {
    let mut first_by_name: BTreeMap<&str, (usize, NodeRef)> = BTreeMap::new();
    let mut symbols = BTreeSet::new();

    for (index, node) in ir.nodes.iter().enumerate() {
        if node.name.trim().is_empty() {
            continue;
        }
        if let Some((first_index, first_ref)) = first_by_name.get(node.name.as_str()).copied() {
            issues.push(CoreIrDiagnosticIssue::new(
                CoreIrIssueCode::DuplicateSymbol,
                node_descriptor(index, node),
                format!(
                    "{} first={} duplicate={}",
                    redacted_symbol_descriptor("symbol", &node.name),
                    redacted_ref_descriptor(first_ref, first_index),
                    redacted_ref_descriptor(node.source_ref, index)
                ),
            ));
        } else {
            first_by_name.insert(&node.name, (index, node.source_ref));
        }
        symbols.insert(node.name.clone());
    }

    symbols
}

fn validate_node(
    index: usize,
    node: &CoreNode,
    symbols: &BTreeSet<String>,
    issues: &mut Vec<CoreIrDiagnosticIssue>,
) {
    if node.name.trim().is_empty() {
        issues.push(CoreIrDiagnosticIssue::new(
            CoreIrIssueCode::InvalidNodeShape,
            node_descriptor(index, node),
            "node name must be non-empty",
        ));
    }

    if let Some(ty) = &node.ty {
        validate_type(
            ty,
            &format!("{}/type", node_descriptor(index, node)),
            issues,
        );
    }

    if let Some(expr) = &node.expr {
        let mut ctx = ExprValidationCtx::new(index, node.source_ref, symbols, issues);
        ctx.validate_expr(expr);
    }
}

fn validate_type(ty: &CoreType, path: &str, issues: &mut Vec<CoreIrDiagnosticIssue>) {
    match ty {
        CoreType::List(inner)
        | CoreType::Set(inner)
        | CoreType::Option(inner)
        | CoreType::PatchField(inner)
        | CoreType::Vector(inner)
        | CoreType::OrderedSet(inner)
        | CoreType::Array(inner)
        | CoreType::Task(inner)
        | CoreType::Channel(inner)
        | CoreType::Decoded(inner) => validate_type(inner, path, issues),
        CoreType::Map(key, value)
        | CoreType::Result(key, value)
        | CoreType::OrderedMap(key, value) => {
            validate_type(key, path, issues);
            validate_type(value, path, issues);
        }
        CoreType::Function {
            params,
            ret,
            effects,
        } => {
            for param in params {
                validate_type(param, path, issues);
            }
            validate_type(ret, path, issues);
            push_duplicate_strings(effects, path, "effect", issues);
            for effect in effects {
                if effect.trim().is_empty() {
                    issues.push(CoreIrDiagnosticIssue::new(
                        CoreIrIssueCode::InvalidTypeShape,
                        path,
                        "function effect name must be non-empty",
                    ));
                }
            }
        }
        CoreType::Handle { resource, .. } => validate_type(resource, path, issues),
        CoreType::Refinement { base, predicate } => {
            validate_type(base, path, issues);
            if predicate.trim().is_empty() {
                issues.push(CoreIrDiagnosticIssue::new(
                    CoreIrIssueCode::InvalidTypeShape,
                    path,
                    "refinement predicate must be non-empty",
                ));
            }
        }
        CoreType::Generic(None) => issues.push(CoreIrDiagnosticIssue::new(
            CoreIrIssueCode::UnsupportedPrimitive,
            path,
            "generic type must be resolved before production lowering",
        )),
        CoreType::Generic(Some(inner)) => validate_type(inner, path, issues),
        CoreType::NormalizedText(form) => {
            if !matches!(form.as_str(), "NFC" | "NFD" | "NFKC" | "NFKD") {
                issues.push(CoreIrDiagnosticIssue::new(
                    CoreIrIssueCode::InvalidTypeShape,
                    path,
                    format!(
                        "normalized text form must be canonical {}",
                        redacted_symbol_descriptor("form", form)
                    ),
                ));
            }
        }
        CoreType::ForeignType(name)
        | CoreType::Encoded(name)
        | CoreType::Dyn(name)
        | CoreType::BoundarySchema(name)
        | CoreType::AdapterContract(name) => {
            if name.trim().is_empty() {
                issues.push(CoreIrDiagnosticIssue::new(
                    CoreIrIssueCode::InvalidTypeShape,
                    path,
                    "named type payload must be non-empty",
                ));
            }
        }
        CoreType::Unit
        | CoreType::Never
        | CoreType::Bool
        | CoreType::Int
        | CoreType::UInt
        | CoreType::Float
        | CoreType::Text
        | CoreType::Bytes
        | CoreType::Record
        | CoreType::Variant
        | CoreType::Tuple
        | CoreType::Decimal
        | CoreType::Existential
        | CoreType::CodePoint
        | CoreType::Grapheme
        | CoreType::Int32
        | CoreType::Int64
        | CoreType::UInt32
        | CoreType::UInt64
        | CoreType::TaskGroup => {}
    }
}

struct ExprValidationCtx<'a, 'b> {
    node_index: usize,
    source_ref: NodeRef,
    symbols: &'a BTreeSet<String>,
    scopes: Vec<BTreeSet<String>>,
    issues: &'b mut Vec<CoreIrDiagnosticIssue>,
}

impl<'a, 'b> ExprValidationCtx<'a, 'b> {
    fn new(
        node_index: usize,
        source_ref: NodeRef,
        symbols: &'a BTreeSet<String>,
        issues: &'b mut Vec<CoreIrDiagnosticIssue>,
    ) -> Self {
        Self {
            node_index,
            source_ref,
            symbols,
            scopes: vec![BTreeSet::new()],
            issues,
        }
    }

    fn validate_expr(&mut self, expr: &CoreExpr) {
        match expr {
            CoreExpr::Literal(value) => self.validate_literal(value),
            CoreExpr::Var(name) => self.validate_reference(name),
            CoreExpr::Let { name, value, body } => {
                self.validate_expr(value);
                self.with_bindings(&[name.as_str()], |ctx| ctx.validate_expr(body));
            }
            CoreExpr::If { cond, then_, else_ } => {
                self.validate_expr(cond);
                self.validate_expr(then_);
                self.validate_expr(else_);
            }
            CoreExpr::Match { scrutinee, arms } => {
                self.validate_expr(scrutinee);
                for arm in arms {
                    self.validate_match_arm(arm);
                }
            }
            CoreExpr::Call { func, args } => {
                self.validate_reference(func);
                self.validate_exprs(args);
            }
            CoreExpr::Add(left, right)
            | CoreExpr::Sub(left, right)
            | CoreExpr::Mul(left, right)
            | CoreExpr::Div(left, right)
            | CoreExpr::Mod(left, right)
            | CoreExpr::Eq(left, right)
            | CoreExpr::Lt(left, right)
            | CoreExpr::Gt(left, right)
            | CoreExpr::Ne(left, right)
            | CoreExpr::Le(left, right)
            | CoreExpr::Ge(left, right) => {
                self.validate_expr(left);
                self.validate_expr(right);
            }
            CoreExpr::Not(inner)
            | CoreExpr::TaskAwait { task: inner }
            | CoreExpr::TaskCancel { task: inner }
            | CoreExpr::TaskGroup { body: inner }
            | CoreExpr::ChannelReceive { channel: inner }
            | CoreExpr::ResourceRelease { handle: inner }
            | CoreExpr::CellNew { init: inner }
            | CoreExpr::CellGet { cell: inner }
            | CoreExpr::Return { value: inner } => self.validate_expr(inner),
            CoreExpr::IndexGet { collection, index } => {
                self.validate_expr(collection);
                self.validate_expr(index);
            }
            CoreExpr::Lambda { params, body } => {
                self.with_bindings(
                    &params.iter().map(String::as_str).collect::<Vec<_>>(),
                    |ctx| ctx.validate_expr(body),
                );
            }
            CoreExpr::RecordNew { fields } => {
                push_duplicate_strings(
                    &fields
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect::<Vec<_>>(),
                    &self.node_descriptor(),
                    "field",
                    self.issues,
                );
                for (_, value) in fields {
                    self.validate_expr(value);
                }
            }
            CoreExpr::FieldGet { record, field } => {
                self.validate_expr(record);
                self.validate_named_payload(field, "field");
            }
            CoreExpr::FieldUpdate {
                record,
                field,
                value,
            } => {
                self.validate_expr(record);
                self.validate_named_payload(field, "field");
                self.validate_expr(value);
            }
            CoreExpr::TupleNew(items)
            | CoreExpr::ListNew(items)
            | CoreExpr::SetNew { elements: items } => {
                self.validate_exprs(items);
            }
            CoreExpr::VariantNew { tag, payload } => {
                self.validate_named_payload(tag, "tag");
                if let Some(payload) = payload {
                    self.validate_expr(payload);
                }
            }
            CoreExpr::Loop { body, .. } => self.validate_expr(body),
            CoreExpr::Break { value } => self.validate_expr(value),
            CoreExpr::Continue | CoreExpr::ChannelNew { .. } => {}
            CoreExpr::WhileLoop { cond, body, .. } => {
                self.validate_expr(cond);
                self.validate_expr(body);
            }
            CoreExpr::And { left, right } | CoreExpr::Or { left, right } => {
                self.validate_expr(left);
                self.validate_expr(right);
            }
            CoreExpr::EffectCall {
                capability,
                func,
                args,
            } => {
                self.validate_reference(capability);
                self.validate_named_payload(func, "function");
                self.validate_exprs(args);
            }
            CoreExpr::Dispatch {
                handler,
                method,
                args,
            } => {
                self.validate_reference(handler);
                self.validate_named_payload(method, "method");
                self.validate_exprs(args);
            }
            CoreExpr::TaskSpawn { func, args } => {
                self.validate_reference(func);
                self.validate_exprs(args);
            }
            CoreExpr::ChannelSend { channel, value }
            | CoreExpr::CellSet {
                cell: channel,
                value,
            } => {
                self.validate_expr(channel);
                self.validate_expr(value);
            }
            CoreExpr::RuntimeCheck {
                check_ref,
                cond,
                msg,
            } => {
                self.validate_named_payload(check_ref, "check");
                self.validate_named_payload(msg, "message");
                self.validate_expr(cond);
            }
            CoreExpr::ResourceAcquire { resource, args } => {
                self.validate_reference(resource);
                self.validate_exprs(args);
            }
            CoreExpr::Select { branches } => {
                if branches.is_empty() {
                    self.issues.push(CoreIrDiagnosticIssue::new(
                        CoreIrIssueCode::InvalidNodeShape,
                        self.node_descriptor(),
                        "select must contain at least one branch",
                    ));
                }
                for branch in branches {
                    self.validate_select_clause(branch);
                }
            }
            CoreExpr::Timeout { duration, body } => {
                self.validate_expr(duration);
                self.validate_expr(body);
            }
            CoreExpr::Placeholder => self.issues.push(CoreIrDiagnosticIssue::new(
                CoreIrIssueCode::UnsupportedPrimitive,
                self.node_descriptor(),
                "placeholder expression must be lowered before production diagnostics pass",
            )),
            CoreExpr::ForEach {
                binding,
                collection,
                body,
            } => {
                self.validate_expr(collection);
                self.with_bindings(&[binding.as_str()], |ctx| ctx.validate_expr(body));
            }
            CoreExpr::Fold { init, list, func } => {
                self.validate_expr(init);
                self.validate_expr(list);
                self.validate_expr(func);
            }
            CoreExpr::MapNew { entries } => {
                for (key, value) in entries {
                    self.validate_expr(key);
                    self.validate_expr(value);
                }
            }
            CoreExpr::BoundaryCall {
                boundary,
                func,
                args,
            } => {
                self.validate_reference(boundary);
                self.validate_named_payload(func, "function");
                self.validate_exprs(args);
            }
            CoreExpr::Assume { predicate, reason } => {
                self.validate_named_payload(predicate, "predicate");
                self.validate_named_payload(reason, "reason");
            }
            CoreExpr::Abort { message } => self.validate_named_payload(message, "message"),
            CoreExpr::DynCall {
                interface,
                method,
                args,
            } => {
                self.validate_named_payload(interface, "interface");
                self.validate_named_payload(method, "method");
                self.validate_exprs(args);
            }
            CoreExpr::CapabilityUse { capability, args } => {
                self.validate_reference(capability);
                self.validate_exprs(args);
            }
            CoreExpr::ResourceUse { handle, body } => {
                self.validate_expr(handle);
                self.validate_expr(body);
            }
            CoreExpr::ResourceUsing {
                resource,
                binding,
                body,
            } => {
                self.validate_expr(resource);
                self.with_bindings(&[binding.as_str()], |ctx| ctx.validate_expr(body));
            }
            CoreExpr::ResourceTransfer { handle, target } => {
                self.validate_expr(handle);
                self.validate_expr(target);
            }
            CoreExpr::ForeignFunctionCall { func, args } => {
                self.validate_reference(func);
                self.validate_exprs(args);
            }
            CoreExpr::PatchFieldConstruct { state, value } => {
                if !matches!(state.as_str(), "Unchanged" | "Set" | "Clear") {
                    self.issues.push(CoreIrDiagnosticIssue::new(
                        CoreIrIssueCode::InvalidNodeShape,
                        self.node_descriptor(),
                        format!(
                            "patch field state must be canonical {}",
                            redacted_symbol_descriptor("state", state)
                        ),
                    ));
                }
                if let Some(value) = value {
                    self.validate_expr(value);
                }
            }
            CoreExpr::PatchFieldMatch { scrutinee, arms } => {
                self.validate_expr(scrutinee);
                for arm in arms {
                    self.validate_match_arm(arm);
                }
            }
        }
    }

    fn validate_exprs(&mut self, exprs: &[CoreExpr]) {
        for expr in exprs {
            self.validate_expr(expr);
        }
    }

    fn validate_literal(&mut self, value: &LiteralValue) {
        if let LiteralValue::Float(value) = value {
            if !value.is_finite() {
                self.issues.push(CoreIrDiagnosticIssue::new(
                    CoreIrIssueCode::InvalidNodeShape,
                    self.node_descriptor(),
                    "float literal must be finite",
                ));
            }
        }
    }

    fn validate_match_arm(&mut self, arm: &MatchArm) {
        self.validate_named_payload(&arm.pattern, "pattern");
        self.validate_expr(&arm.body);
    }

    fn validate_select_clause(&mut self, clause: &SelectClause) {
        self.validate_expr(&clause.channel);
        self.with_bindings(&[clause.binding.as_str()], |ctx| {
            ctx.validate_expr(&clause.body)
        });
    }

    fn validate_reference(&mut self, name: &str) {
        if name.trim().is_empty() {
            self.issues.push(CoreIrDiagnosticIssue::new(
                CoreIrIssueCode::InvalidNodeShape,
                self.node_descriptor(),
                "reference name must be non-empty",
            ));
            return;
        }
        if self.scopes.iter().rev().any(|scope| scope.contains(name)) || self.symbols.contains(name)
        {
            return;
        }
        self.issues.push(CoreIrDiagnosticIssue::new(
            CoreIrIssueCode::MissingReference,
            self.node_descriptor(),
            redacted_symbol_descriptor("reference", name),
        ));
    }

    fn validate_named_payload(&mut self, value: &str, kind: &'static str) {
        if value.trim().is_empty() {
            self.issues.push(CoreIrDiagnosticIssue::new(
                CoreIrIssueCode::InvalidNodeShape,
                self.node_descriptor(),
                format!("{kind} payload must be non-empty"),
            ));
        }
    }

    fn with_bindings(&mut self, bindings: &[&str], f: impl FnOnce(&mut Self)) {
        push_duplicate_strs(bindings, &self.node_descriptor(), "binding", self.issues);
        let mut scope = BTreeSet::new();
        for binding in bindings {
            if binding.trim().is_empty() {
                self.issues.push(CoreIrDiagnosticIssue::new(
                    CoreIrIssueCode::InvalidNodeShape,
                    self.node_descriptor(),
                    "binding name must be non-empty",
                ));
            } else {
                scope.insert((*binding).to_string());
            }
        }
        self.scopes.push(scope);
        f(self);
        self.scopes.pop();
    }

    fn node_descriptor(&self) -> String {
        format!("node#{:04}/ref:{}/expr", self.node_index, self.source_ref.0)
    }
}

fn push_duplicate_strings(
    values: &[String],
    descriptor: &str,
    kind: &'static str,
    issues: &mut Vec<CoreIrDiagnosticIssue>,
) {
    let refs = values.iter().map(String::as_str).collect::<Vec<_>>();
    push_duplicate_strs(&refs, descriptor, kind, issues);
}

fn push_duplicate_strs(
    values: &[&str],
    descriptor: &str,
    kind: &'static str,
    issues: &mut Vec<CoreIrDiagnosticIssue>,
) {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() {
            continue;
        }
        if !seen.insert(*value) {
            issues.push(CoreIrDiagnosticIssue::new(
                CoreIrIssueCode::DuplicateBinding,
                descriptor,
                redacted_symbol_descriptor(kind, value),
            ));
        }
    }
}

fn node_descriptor(index: usize, node: &CoreNode) -> String {
    format!(
        "node#{index:04}/ref:{}/kind:{}/name-len:{}",
        node.source_ref.0,
        node_kind_name(node.kind),
        node.name.chars().count()
    )
}

fn redacted_ref_descriptor(source_ref: NodeRef, index: usize) -> String {
    format!("node#{index:04}/ref:{}", source_ref.0)
}

fn redacted_symbol_descriptor(kind: &str, value: &str) -> String {
    format!(
        "{kind}/len:{}/hash:{:016x}",
        value.chars().count(),
        stable_hash64(value.as_bytes())
    )
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn node_kind_name(kind: CoreNodeKind) -> &'static str {
    match kind {
        CoreNodeKind::Module => "module",
        CoreNodeKind::Function => "function",
        CoreNodeKind::Type => "type",
        CoreNodeKind::Effect => "effect",
        CoreNodeKind::Capability => "capability",
        CoreNodeKind::Contract => "contract",
        CoreNodeKind::Invariant => "invariant",
        CoreNodeKind::Test => "test",
        CoreNodeKind::Boundary => "boundary",
        CoreNodeKind::Package => "package",
        CoreNodeKind::Interface => "interface",
        CoreNodeKind::Impl => "impl",
        CoreNodeKind::EffectAlias => "effect-alias",
        CoreNodeKind::Import => "import",
        CoreNodeKind::Export => "export",
        CoreNodeKind::VersionConstraint => "version-constraint",
        CoreNodeKind::CapabilityExport => "capability-export",
        CoreNodeKind::ContractExport => "contract-export",
    }
}
