use super::SemanticGraph;

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
    pub(super) line_num: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceFunction {
    pub(super) name: String,
    pub(super) params: Vec<SourceParam>,
    pub(super) return_type: String,
    pub(super) body: String,
    pub(super) line_num: usize,
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
    pub(super) line_num: usize,
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
    pub(super) fn extend(&mut self, other: SourceProgram) {
        self.imports.extend(other.imports);
        self.capabilities.extend(other.capabilities);
        self.constants.extend(other.constants);
        self.functions.extend(other.functions);
        self.tests.extend(other.tests);
        self.grants.extend(other.grants);
    }
}
