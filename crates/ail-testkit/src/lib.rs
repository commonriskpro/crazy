// ── Semantic graph fixture helpers ────────────────────────────────────────

/// Build a minimal but multi-typed [`ail_core::semantic_graph::SemanticGraph`]
/// fixture for use in workspace tests.
///
/// Returns a graph with:
/// - 3 nodes: `NodeRef(0)` (`Module`), `NodeRef(1)` (`Function`), `NodeRef(2)` (`Effect`)
/// - 2 edges: `0 → 1` (`DependsOn`), `1 → 2` (`Emits`)
///
/// The graph is structurally valid (`validate()` returns `Ok(())`).
///
/// # Example
///
/// ```rust
/// let graph = ail_testkit::make_semantic_graph();
/// assert!(graph.validate().is_ok());
/// ```
pub fn make_semantic_graph() -> ail_core::semantic_graph::SemanticGraph {
    use ail_core::semantic_graph::{
        EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };

    SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "core"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "run"),
            GraphNode::new(NodeRef(2), NodeKind::Effect, "io"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Emits),
        ],
    }
}

// ── Large graph fixture ───────────────────────────────────────────────────

/// Build a [`SemanticGraph`](ail_core::semantic_graph::SemanticGraph) with `n`
/// nodes connected by a linear `Calls` chain:
///
/// `NodeRef(0) → NodeRef(1) → NodeRef(2) → … → NodeRef(n-1)`
///
/// The resulting graph is structurally valid (`validate()` returns `Ok(())`).
/// Use this fixture for benchmarks and integration tests that need a realistic,
/// deterministic large graph without hand-crafting hundreds of nodes.
///
/// # Panics
///
/// Does not panic; `n = 0` returns an empty (valid) graph.
///
/// # Example
///
/// ```rust
/// let graph = ail_testkit::make_large_graph(500);
/// assert!(graph.validate().is_ok());
/// assert_eq!(graph.nodes.len(), 500);
/// ```
pub fn make_large_graph(n: usize) -> ail_core::semantic_graph::SemanticGraph {
    use ail_core::semantic_graph::{
        EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };

    let nodes: Vec<GraphNode> = (0..n)
        .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
        .collect();

    // Linear chain: NodeRef(i) Calls NodeRef(i+1) for i in 0..n-1.
    let edges: Vec<GraphEdge> = (0..n.saturating_sub(1))
        .map(|i| GraphEdge::new(NodeRef(i as u32), NodeRef((i + 1) as u32), EdgeKind::Calls))
        .collect();

    SemanticGraph { nodes, edges }
}

// ── Storage fixture helpers ───────────────────────────────────────────────

/// Re-export of [`ail_storage::backends::memory::MemoryObjectStore`] for use
/// in tests across the workspace without an explicit `ail-storage` dependency.
pub use ail_storage::backends::memory::MemoryObjectStore;

/// Re-export of [`ail_storage::graph::ObjectBackedGraphStore`] for use in
/// workspace tests that need a `GraphStore` backed by an in-memory store.
pub use ail_storage::graph::ObjectBackedGraphStore;

/// Build a minimal [`ail_storage::graph::SnapshotEnvelope`] fixture.
///
/// `label` is hashed with BLAKE3 to produce both `id` and `graph_root_hash`,
/// giving a deterministic but unique `ObjectId` per call site.  `parent_id`
/// and `applied_change_id` are `None` (genesis snapshot) and `created_at`
/// is set to `0`.
///
/// # Example
///
/// ```rust
/// let snap = ail_testkit::make_snapshot_envelope("my-root");
/// assert!(snap.parent_id.is_none());
/// assert!(snap.applied_change_id.is_none());
/// ```
pub fn make_snapshot_envelope(label: &str) -> ail_storage::graph::SnapshotEnvelope {
    let id = ail_storage::object::ObjectId::from_bytes(label.as_bytes());
    ail_storage::graph::SnapshotEnvelope {
        id,
        graph_root_hash: id,
        parent_id: None,
        applied_change_id: None,
        created_at: 0,
        verification_report_hash: None,
        ..Default::default()
    }
}

// ── Fixture path macro ────────────────────────────────────────────────────

/// Returns a [`std::path::PathBuf`] pointing to a file inside the **calling
/// crate's** `tests/fixtures/` directory.
///
/// Because this is a `macro_rules!` macro, `env!("CARGO_MANIFEST_DIR")` is
/// expanded at the call site, so the resolved path always belongs to the crate
/// that invokes the macro — not to `ail-testkit` itself.
///
/// # Panics
///
/// Panics with an informative message if the file does not exist at the
/// resolved path.
///
/// # Example
///
/// ```rust,no_run
/// // Inside a test in some other crate that depends on ail-testkit:
/// let path = ail_testkit::fixture!("sample.atl");
/// ```
#[macro_export]
macro_rules! fixture {
    ($name:expr) => {{
        let path = ::std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join($name);
        if !path.exists() {
            panic!(
                "fixture not found: {}\n\
                 Hint: create the file at that path to use it in tests.",
                path.display()
            );
        }
        path
    }};
}

// ── Stable test runner diagnostics ────────────────────────────────────────

use std::path::{Path, PathBuf};

/// A fixture consumed by [`run_runner_diagnostics`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunnerFixture {
    /// Load fixture contents from disk.
    Path(PathBuf),
    /// Use fixture contents supplied by the test.
    Inline { name: String, contents: String },
}

impl RunnerFixture {
    /// Builds a path-backed fixture.
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }

    /// Builds an inline fixture with a stable diagnostic name.
    pub fn inline(name: impl Into<String>, contents: impl Into<String>) -> Self {
        Self::Inline {
            name: name.into(),
            contents: contents.into(),
        }
    }
}

/// Expected and actual values for one testkit assertion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunnerAssertion {
    pub label: String,
    pub expected: String,
    pub actual: String,
}

impl RunnerAssertion {
    /// Builds a named equality assertion.
    pub fn eq(
        label: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }
}

/// Whether a runner diagnostic stream preserves emission order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticOrder {
    /// Diagnostics are emitted in deterministic order.
    Deterministic,
    /// Diagnostics were collected from a source with unstable ordering.
    Nondeterministic,
}

impl Default for DiagnosticOrder {
    fn default() -> Self {
        Self::Deterministic
    }
}

/// Input captured from one testkit runner execution.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunnerCase {
    pub fixture: Option<RunnerFixture>,
    pub assertions: Vec<RunnerAssertion>,
    pub actual_diagnostics: Vec<String>,
    pub expected_diagnostics: Vec<String>,
    pub diagnostic_order: DiagnosticOrder,
}

impl RunnerCase {
    /// Builds an empty runner case.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attaches the fixture under test.
    pub fn with_fixture(mut self, fixture: RunnerFixture) -> Self {
        self.fixture = Some(fixture);
        self
    }

    /// Adds an equality assertion to the runner case.
    pub fn with_assertion(mut self, assertion: RunnerAssertion) -> Self {
        self.assertions.push(assertion);
        self
    }

    /// Adds an actual diagnostic emitted by the runner.
    pub fn with_actual_diagnostic(mut self, diagnostic: impl Into<String>) -> Self {
        self.actual_diagnostics.push(diagnostic.into());
        self
    }

    /// Adds a diagnostic that must be emitted by the runner.
    pub fn with_expected_diagnostic(mut self, diagnostic: impl Into<String>) -> Self {
        self.expected_diagnostics.push(diagnostic.into());
        self
    }

    /// Marks whether diagnostic emission order is deterministic.
    pub fn with_diagnostic_order(mut self, order: DiagnosticOrder) -> Self {
        self.diagnostic_order = order;
        self
    }
}

/// Stable categories for runner diagnostic issues.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RunnerIssueKind {
    FixtureMissing,
    FixtureInvalid,
    AssertionMismatch,
    ExpectedDiagnosticAbsent,
    NondeterministicDiagnosticOrder,
}

impl RunnerIssueKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::FixtureMissing => "fixture-missing",
            Self::FixtureInvalid => "fixture-invalid",
            Self::AssertionMismatch => "assertion-mismatch",
            Self::ExpectedDiagnosticAbsent => "expected-diagnostic-absent",
            Self::NondeterministicDiagnosticOrder => "nondeterministic-diagnostic-order",
        }
    }
}

/// One stable, redacted issue produced by testkit runner diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunnerIssue {
    pub kind: RunnerIssueKind,
    pub fixture: Option<String>,
    pub label: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub message: String,
}

/// Stable report for a testkit runner execution.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunnerReport {
    pub issues: Vec<RunnerIssue>,
}

impl RunnerReport {
    /// Returns true when the runner case produced no issues.
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Renders a deterministic, redacted diagnostic block suitable for panics
    /// or golden assertions.
    pub fn stable_diagnostics(&self) -> String {
        if self.issues.is_empty() {
            return "testkit runner diagnostics: ok".to_string();
        }

        let mut lines = vec!["testkit runner diagnostics:".to_string()];
        for issue in &self.issues {
            lines.push(format!("- kind: {}", issue.kind.as_str()));
            if let Some(fixture) = &issue.fixture {
                lines.push(format!("  fixture: {fixture}"));
            }
            if let Some(label) = &issue.label {
                lines.push(format!("  label: {label}"));
            }
            if let Some(expected) = &issue.expected {
                lines.push(format!("  expected: {expected}"));
            }
            if let Some(actual) = &issue.actual {
                lines.push(format!("  actual: {actual}"));
            }
            lines.push(format!("  message: {}", issue.message));
        }
        lines.join("\n")
    }

    /// Panics with stable, redacted diagnostics when the report contains issues.
    pub fn assert_ok(&self) {
        assert!(self.is_ok(), "{}", self.stable_diagnostics());
    }
}

/// Produces stable, redacted diagnostics for one captured testkit runner case.
pub fn run_runner_diagnostics(case: RunnerCase) -> RunnerReport {
    let redactor = Redactor::for_case(&case);
    let fixture = case
        .fixture
        .as_ref()
        .map(|fixture| redactor.fixture(fixture));
    let mut issues = Vec::new();

    match &case.fixture {
        Some(RunnerFixture::Path(path)) => match std::fs::read_to_string(path) {
            Ok(contents) if contents.trim().is_empty() => issues.push(RunnerIssue {
                kind: RunnerIssueKind::FixtureInvalid,
                fixture: fixture.clone(),
                label: None,
                expected: None,
                actual: Some("non-empty fixture".to_string()),
                message: "fixture exists but is empty".to_string(),
            }),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                issues.push(RunnerIssue {
                    kind: RunnerIssueKind::FixtureMissing,
                    fixture: fixture.clone(),
                    label: None,
                    expected: None,
                    actual: None,
                    message: "fixture could not be found".to_string(),
                })
            }
            Err(error) => issues.push(RunnerIssue {
                kind: RunnerIssueKind::FixtureInvalid,
                fixture: fixture.clone(),
                label: None,
                expected: None,
                actual: Some(redactor.text(error.to_string())),
                message: "fixture could not be read as UTF-8 text".to_string(),
            }),
        },
        Some(RunnerFixture::Inline { contents, .. }) if contents.trim().is_empty() => {
            issues.push(RunnerIssue {
                kind: RunnerIssueKind::FixtureInvalid,
                fixture: fixture.clone(),
                label: None,
                expected: None,
                actual: Some("non-empty fixture".to_string()),
                message: "fixture exists but is empty".to_string(),
            });
        }
        Some(RunnerFixture::Inline { .. }) => {}
        None => issues.push(RunnerIssue {
            kind: RunnerIssueKind::FixtureMissing,
            fixture: None,
            label: None,
            expected: None,
            actual: None,
            message: "runner case did not provide a fixture".to_string(),
        }),
    }

    for assertion in &case.assertions {
        if assertion.expected != assertion.actual {
            issues.push(RunnerIssue {
                kind: RunnerIssueKind::AssertionMismatch,
                fixture: fixture.clone(),
                label: Some(redactor.text(&assertion.label)),
                expected: Some(redactor.text(&assertion.expected)),
                actual: Some(redactor.text(&assertion.actual)),
                message: "assertion expected value did not match actual value".to_string(),
            });
        }
    }

    for expected in &case.expected_diagnostics {
        if !case
            .actual_diagnostics
            .iter()
            .any(|actual| actual.contains(expected))
        {
            issues.push(RunnerIssue {
                kind: RunnerIssueKind::ExpectedDiagnosticAbsent,
                fixture: fixture.clone(),
                label: None,
                expected: Some(redactor.text(expected)),
                actual: Some(redactor.text(case.actual_diagnostics.join(" | "))),
                message: "expected diagnostic was not emitted".to_string(),
            });
        }
    }

    if case.diagnostic_order == DiagnosticOrder::Nondeterministic {
        issues.push(RunnerIssue {
            kind: RunnerIssueKind::NondeterministicDiagnosticOrder,
            fixture,
            label: None,
            expected: Some("deterministic diagnostic order".to_string()),
            actual: Some("nondeterministic diagnostic order".to_string()),
            message: "runner diagnostic order is not stable enough for production tests"
                .to_string(),
        });
    }

    issues.sort_by(|left, right| {
        (
            left.kind,
            left.fixture.as_deref(),
            left.label.as_deref(),
            left.expected.as_deref(),
            left.actual.as_deref(),
            left.message.as_str(),
        )
            .cmp(&(
                right.kind,
                right.fixture.as_deref(),
                right.label.as_deref(),
                right.expected.as_deref(),
                right.actual.as_deref(),
                right.message.as_str(),
            ))
    });

    RunnerReport { issues }
}

#[derive(Clone, Debug, Default)]
struct Redactor {
    roots: Vec<(String, &'static str)>,
}

impl Redactor {
    fn for_case(case: &RunnerCase) -> Self {
        let mut roots = Vec::new();
        if let Some(RunnerFixture::Path(path)) = &case.fixture {
            if let Some(parent) = path.parent() {
                push_root(&mut roots, parent, "<fixture-dir>");
            }
        }
        push_root(&mut roots, std::env::temp_dir(), "<tmp>");
        if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
            push_root(&mut roots, manifest_dir, "<crate>");
        }
        roots.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.0.cmp(&right.0)));
        Self { roots }
    }

    fn fixture(&self, fixture: &RunnerFixture) -> String {
        match fixture {
            RunnerFixture::Path(path) => self.path(path),
            RunnerFixture::Inline { name, .. } => format!("<inline:{}>", clean_fixture_name(name)),
        }
    }

    fn path(&self, path: &Path) -> String {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(clean_fixture_name)
            .unwrap_or_else(|| "unnamed".to_string());
        format!("<fixture:{file_name}>")
    }

    fn text(&self, text: impl AsRef<str>) -> String {
        let mut redacted = text.as_ref().to_string();
        for (root, token) in &self.roots {
            redacted = redacted.replace(root, token);
        }
        redacted
    }
}

fn push_root(roots: &mut Vec<(String, &'static str)>, root: impl AsRef<Path>, token: &'static str) {
    if let Some(root) = root.as_ref().to_str() {
        if !root.is_empty() {
            roots.push((root.to_string(), token));
        }
    }
}

fn clean_fixture_name(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' => ch,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixture_resolves_existing_file() {
        let path = crate::fixture!("sample.atl");
        assert!(path.exists(), "fixture path must exist");
    }

    #[test]
    #[should_panic(expected = "fixture not found")]
    fn fixture_panics_on_missing_file() {
        crate::fixture!("does_not_exist.atl");
    }

    // ── make_semantic_graph_is_valid ──────────────────────────────────────
    // Spec: make_semantic_graph() produces a structurally valid graph.
    //   GIVEN the fixture returned by make_semantic_graph()
    //   WHEN validate() is called on it
    //   THEN it returns Ok(())
    #[test]
    fn make_semantic_graph_is_valid() {
        let graph = crate::make_semantic_graph();
        assert!(
            graph.validate().is_ok(),
            "make_semantic_graph() fixture must pass structural validation"
        );
    }

    // ── Spec scenario: Large graph passes validation ───────────────────────
    // GIVEN make_large_graph(500) is called
    // WHEN graph.validate() is invoked
    // THEN validation returns Ok(())
    #[test]
    fn make_large_graph_500_is_valid() {
        let graph = crate::make_large_graph(500);
        assert_eq!(graph.nodes.len(), 500, "must have exactly 500 nodes");
        assert_eq!(graph.edges.len(), 499, "linear chain must have n-1 edges");
        assert!(
            graph.validate().is_ok(),
            "make_large_graph(500) must pass structural validation"
        );
    }

    // ── TRIANGULATE: make_large_graph(0) is valid ─────────────────────────
    #[test]
    fn make_large_graph_zero_is_valid() {
        let graph = crate::make_large_graph(0);
        assert_eq!(graph.nodes.len(), 0);
        assert_eq!(graph.edges.len(), 0);
        assert!(graph.validate().is_ok());
    }

    // ── TRIANGULATE: make_large_graph(1) has no edges ─────────────────────
    #[test]
    fn make_large_graph_one_has_no_edges() {
        let graph = crate::make_large_graph(1);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.edges.len(), 0, "single node has no edges");
        assert!(graph.validate().is_ok());
    }
}
