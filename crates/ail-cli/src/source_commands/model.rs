use super::{Path, PathBuf, SemanticGraph};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SourceProgram {
    pub(super) module: Option<String>,
    pub(super) imports: Vec<String>,
    pub(super) capabilities: Vec<String>,
    pub(super) constants: Vec<SourceConst>,
    pub(super) functions: Vec<SourceFunction>,
    pub(super) tests: Vec<SourceTest>,
    pub(super) grants: Vec<SourceGrant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceConst {
    pub(super) name: String,
    pub(super) return_type: String,
    pub(super) body: String,
    /// Alias-preserving lowered body used only for canonical formatting. Distinct
    /// source aliases (e.g. `log.write` vs `print`) collapse to the same `body`
    /// during lowering; this field keeps the original spelling so the formatter can
    /// round-trip it faithfully. `None` falls back to `body`.
    pub(super) source_body: Option<String>,
    pub(super) line_num: usize,
    pub(super) source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceFunction {
    pub(super) name: String,
    pub(super) params: Vec<SourceParam>,
    pub(super) return_type: String,
    pub(super) body: String,
    /// Alias-preserving lowered body used only for canonical formatting. See
    /// [`SourceConst::source_body`].
    pub(super) source_body: Option<String>,
    pub(super) line_num: usize,
    pub(super) source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceParam {
    pub(super) name: String,
    pub(super) ty: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceTest {
    pub(super) name: String,
    pub(super) return_type: String,
    pub(super) body: String,
    /// Alias-preserving lowered body used only for canonical formatting. See
    /// [`SourceConst::source_body`].
    pub(super) source_body: Option<String>,
    pub(super) line_num: usize,
    pub(super) source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceGrant {
    pub(super) target: String,
    pub(super) capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceCallable {
    pub(super) param_types: Vec<String>,
    pub(super) return_type: String,
}

pub(crate) struct LoadedSourceGraph {
    pub(crate) graph: SemanticGraph,
    pub(crate) default_entry: String,
}

impl SourceProgram {
    pub(super) fn set_source_path(&mut self, path: &Path) {
        for constant in &mut self.constants {
            constant.source_path = Some(path.to_path_buf());
        }
        for function in &mut self.functions {
            function.source_path = Some(path.to_path_buf());
        }
        for test in &mut self.tests {
            test.source_path = Some(path.to_path_buf());
        }
    }

    pub(super) fn extend(&mut self, other: SourceProgram) {
        self.imports.extend(other.imports);
        self.capabilities.extend(other.capabilities);
        self.constants.extend(other.constants);
        self.functions.extend(other.functions);
        self.tests.extend(other.tests);
        self.grants.extend(other.grants);
    }
}
