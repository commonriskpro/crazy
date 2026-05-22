// ── ail-context::server ───────────────────────────────────────────────────
//
// In-process transport layer for context requests.
//
// This module intentionally stays transport-agnostic: callers can put these
// request/response DTOs behind JSON-RPC, stdin/stdout, MCP, or an in-process
// API without changing the context builder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ail_core::semantic_graph::{GraphNode, NodeRef, SemanticGraph};
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::graph::SnapshotEnvelope;
use ail_storage::object::ObjectId;
use serde::{Deserialize, Serialize};

use crate::builder::{BuildOptions, ResponseBuilder};
use crate::dto::{ContextQuery, IndexInfo, RedactionPolicy, SnapshotSelector};
use crate::error::{ContextError, ContextResult};
use crate::source::ContextSource;

// ── Transport DTOs ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextRequest {
    Query {
        query: ContextQuery,
        snapshot: SnapshotSelector,
        session: Option<AuthSession>,
    },
    Subscribe {
        query: ContextQuery,
        snapshot: SnapshotSelector,
        session: Option<AuthSession>,
    },
    Auth {
        token: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ContextResponse {
    Result(crate::dto::ContextResponse),
    Error(String),
    Stream(Vec<crate::dto::ContextResponse>),
    Authenticated(AuthSession),
}

// ── Auth / redaction ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustLevel {
    #[default]
    Public,
    Internal,
    Privileged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    pub principal: String,
    pub trust_level: TrustLevel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldRedactionRule {
    pub field: String,
    pub min_trust: TrustLevel,
    pub category: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextServerConfig {
    pub require_auth: bool,
    pub tokens: BTreeMap<String, AuthSession>,
    pub redaction_rules: Vec<FieldRedactionRule>,
}

// ── Durable derived indexes ───────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedIndexes {
    pub snapshot_id: ObjectId,
    pub snapshot_hash: ObjectId,
    pub indexes: Vec<IndexInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedIndexCache {
    path: PathBuf,
}

impl DerivedIndexCache {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_fresh(&self, snapshot: &SnapshotEnvelope) -> ContextResult<Option<DerivedIndexes>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&self.path).map_err(|e| ContextError::Codec(e.to_string()))?;
        let cached: DerivedIndexes = CborCodec
            .decode(&bytes)
            .map_err(|e| ContextError::Codec(e.to_string()))?;
        if cached.snapshot_id == snapshot.id && cached.snapshot_hash == snapshot.graph_root_hash {
            Ok(Some(cached))
        } else {
            Ok(None)
        }
    }

    pub fn store(&self, indexes: &DerivedIndexes) -> ContextResult<()> {
        let bytes = CborCodec
            .encode(indexes)
            .map_err(|e| ContextError::Codec(e.to_string()))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ContextError::Codec(e.to_string()))?;
        }
        let tmp_path = self.path.with_extension("tmp");
        std::fs::write(&tmp_path, bytes).map_err(|e| ContextError::Codec(e.to_string()))?;
        std::fs::rename(tmp_path, &self.path).map_err(|e| ContextError::Codec(e.to_string()))?;
        Ok(())
    }

    pub fn load_or_rebuild(
        &self,
        snapshot: &SnapshotEnvelope,
        graph: &SemanticGraph,
    ) -> ContextResult<DerivedIndexes> {
        if let Some(indexes) = self.load_fresh(snapshot)? {
            return Ok(indexes);
        }
        let indexes = build_indexes(snapshot, graph);
        self.store(&indexes)?;
        Ok(indexes)
    }

    pub fn rebuild(
        &self,
        snapshot: &SnapshotEnvelope,
        graph: &SemanticGraph,
    ) -> ContextResult<DerivedIndexes> {
        let indexes = build_indexes(snapshot, graph);
        self.store(&indexes)?;
        Ok(indexes)
    }
}

// ── ContextServer ─────────────────────────────────────────────────────────

pub struct ContextServer<S> {
    source: S,
    config: ContextServerConfig,
    index_cache: Option<DerivedIndexCache>,
}

impl<S> ContextServer<S> {
    pub fn new(source: S) -> Self {
        Self {
            source,
            config: ContextServerConfig::default(),
            index_cache: None,
        }
    }

    pub fn with_config(mut self, config: ContextServerConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_index_cache(mut self, cache: DerivedIndexCache) -> Self {
        self.index_cache = Some(cache);
        self
    }
}

impl<S> ContextServer<S>
where
    S: ContextSource + Send + Sync,
{
    pub async fn handle(&self, request: ContextRequest) -> ContextResponse {
        match request {
            ContextRequest::Auth { token } => match self.authenticate(&token) {
                Ok(session) => ContextResponse::Authenticated(session),
                Err(err) => ContextResponse::Error(err.to_string()),
            },
            ContextRequest::Query {
                query,
                snapshot,
                session,
            } => match self.query(&query, &snapshot, session.as_ref()).await {
                Ok(response) => ContextResponse::Result(response),
                Err(err) => ContextResponse::Error(err.to_string()),
            },
            ContextRequest::Subscribe {
                query,
                snapshot,
                session,
            } => match self.query(&query, &snapshot, session.as_ref()).await {
                Ok(response) => ContextResponse::Stream(vec![response]),
                Err(err) => ContextResponse::Error(err.to_string()),
            },
        }
    }

    pub async fn query(
        &self,
        query: &ContextQuery,
        selector: &SnapshotSelector,
        session: Option<&AuthSession>,
    ) -> ContextResult<crate::dto::ContextResponse> {
        if self.config.require_auth && session.is_none() {
            return Err(ContextError::AccessDenied);
        }

        let snapshot = self.source.resolve_snapshot(selector).await?;
        let graph = self.source.load_graph(&snapshot.graph_root_hash).await?;
        let indexes = match &self.index_cache {
            Some(cache) => cache.load_or_rebuild(&snapshot, &graph)?.indexes,
            None => build_indexes(&snapshot, &graph).indexes,
        };
        let redacted_refs = redacted_refs_for_session(&graph.nodes, &self.config, session);
        let redaction_policy = redaction_policy_for_session(&self.config, session);
        let provenance_sources = vec!["semantic_graph".to_string(), "derived_indexes".to_string()];
        let opts = BuildOptions {
            redaction_policy: redaction_policy.as_ref(),
            authorized: true,
            generated_at: 0,
            provenance_sources: &provenance_sources,
            index_info: &indexes,
            ..Default::default()
        };
        ResponseBuilder::build_full(query, &graph, &snapshot, &redacted_refs, &opts)
    }

    pub async fn rebuild_indexes(
        &self,
        selector: &SnapshotSelector,
    ) -> ContextResult<DerivedIndexes> {
        let snapshot = self.source.resolve_snapshot(selector).await?;
        let graph = self.source.load_graph(&snapshot.graph_root_hash).await?;
        match &self.index_cache {
            Some(cache) => cache.rebuild(&snapshot, &graph),
            None => Ok(build_indexes(&snapshot, &graph)),
        }
    }

    fn authenticate(&self, token: &str) -> ContextResult<AuthSession> {
        self.config
            .tokens
            .get(token)
            .cloned()
            .ok_or(ContextError::AccessDenied)
    }
}

fn redacted_refs_for_session(
    nodes: &[GraphNode],
    config: &ContextServerConfig,
    session: Option<&AuthSession>,
) -> BTreeSet<NodeRef> {
    let trust = session.map(|s| s.trust_level).unwrap_or_default();
    nodes
        .iter()
        .filter(|node| {
            config
                .redaction_rules
                .iter()
                .any(|rule| trust < rule.min_trust && node_has_field(node, &rule.field))
        })
        .map(|node| node.id)
        .collect()
}

fn redaction_policy_for_session(
    config: &ContextServerConfig,
    session: Option<&AuthSession>,
) -> Option<RedactionPolicy> {
    let trust = session.map(|s| s.trust_level).unwrap_or_default();
    let categories: Vec<String> = config
        .redaction_rules
        .iter()
        .filter(|rule| trust < rule.min_trust)
        .map(|rule| rule.category.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if categories.is_empty() {
        None
    } else {
        Some(RedactionPolicy {
            label: "session-field-redaction".to_string(),
            categories,
            requires_approval: trust < TrustLevel::Privileged,
        })
    }
}

fn node_has_field(node: &GraphNode, field: &str) -> bool {
    match field {
        "body_expr" => node.body_expr.is_some(),
        "capability_reqs" => node.capability_reqs.is_some(),
        "contract_clauses" => node.contract_clauses.is_some(),
        "effect_row" => node.effect_row.is_some(),
        "runtime_checks" => node
            .runtime_checks
            .as_ref()
            .is_some_and(|checks| !checks.is_empty()),
        "trust_metadata" => node.trust_metadata.is_some(),
        _ => false,
    }
}

fn build_indexes(snapshot: &SnapshotEnvelope, graph: &SemanticGraph) -> DerivedIndexes {
    let kinds = [
        "call_graph",
        "effect_graph",
        "proof_obligation",
        "resource_graph",
        "boundary_graph",
        "runtime_audit",
    ];
    let indexes = kinds
        .iter()
        .map(|kind| IndexInfo {
            kind: (*kind).to_string(),
            hash: index_hash(kind, snapshot, graph),
            stale: false,
        })
        .collect();
    DerivedIndexes {
        snapshot_id: snapshot.id,
        snapshot_hash: snapshot.graph_root_hash,
        indexes,
    }
}

fn index_hash(kind: &str, snapshot: &SnapshotEnvelope, graph: &SemanticGraph) -> [u8; 32] {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(kind.as_bytes());
    bytes.extend_from_slice(snapshot.id.as_bytes());
    bytes.extend_from_slice(snapshot.graph_root_hash.as_bytes());
    if let Ok(graph_bytes) = CborCodec.encode(graph) {
        bytes.extend_from_slice(&graph_bytes);
    }
    *blake3::hash(&bytes).as_bytes()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_storage::graph::SnapshotEnvelope;
    use futures::executor::block_on;

    use crate::QueryScope;
    use crate::source::InMemoryContextSource;

    fn snapshot() -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(b"server-snapshot");
        SnapshotEnvelope {
            id,
            graph_root_hash: id,
            parent_id: None,
            applied_change_id: None,
            created_at: 1,
            verification_report_hash: None,
        }
    }

    fn graph() -> SemanticGraph {
        let mut public = GraphNode::new(NodeRef(0), NodeKind::Function, "public");
        let mut sensitive = GraphNode::new(NodeRef(1), NodeKind::Function, "secret");
        public.body_expr = None;
        sensitive.body_expr = Some("token".to_string());
        SemanticGraph {
            nodes: vec![public, sensitive],
            edges: vec![],
        }
    }

    fn source(snapshot: &SnapshotEnvelope, graph: &SemanticGraph) -> InMemoryContextSource {
        let source = InMemoryContextSource::new();
        source.insert_snapshot(snapshot.clone());
        source.insert_graph(snapshot.graph_root_hash, graph.clone());
        source
    }

    #[test]
    fn auth_token_returns_session() {
        block_on(async {
            let snapshot = snapshot();
            let graph = graph();
            let mut tokens = BTreeMap::new();
            tokens.insert(
                "dev-token".to_string(),
                AuthSession {
                    principal: "dev".to_string(),
                    trust_level: TrustLevel::Privileged,
                },
            );
            let server =
                ContextServer::new(source(&snapshot, &graph)).with_config(ContextServerConfig {
                    tokens,
                    ..Default::default()
                });

            let response = server
                .handle(ContextRequest::Auth {
                    token: "dev-token".to_string(),
                })
                .await;

            assert!(matches!(response, ContextResponse::Authenticated(_)));
        });
    }

    #[test]
    fn query_redacts_sensitive_fields_for_public_session() {
        block_on(async {
            let snapshot = snapshot();
            let graph = graph();
            let server =
                ContextServer::new(source(&snapshot, &graph)).with_config(ContextServerConfig {
                    redaction_rules: vec![FieldRedactionRule {
                        field: "body_expr".to_string(),
                        min_trust: TrustLevel::Privileged,
                        category: "restricted business logic".to_string(),
                    }],
                    ..Default::default()
                });

            let response = server
                .query(
                    &ContextQuery::Graph {
                        scope: QueryScope::Full,
                        budget: usize::MAX,
                    },
                    &SnapshotSelector::ById(snapshot.id),
                    None,
                )
                .await
                .expect("query");

            assert!(response.redacted);
            assert_eq!(response.structured.len(), 1);
            assert_eq!(response.structured[0].id, NodeRef(0));
        });
    }

    #[test]
    fn index_cache_rebuilds_when_snapshot_hash_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache = DerivedIndexCache::new(temp.path().join("context-indexes.cbor"));
        let graph = graph();
        let first = snapshot();
        let mut second = snapshot();
        second.graph_root_hash = ObjectId::from_bytes(b"new-root");

        let first_indexes = cache.rebuild(&first, &graph).expect("rebuild first");
        let loaded = cache.load_fresh(&first).expect("load first");
        assert_eq!(loaded, Some(first_indexes));

        let stale = cache.load_fresh(&second).expect("load second");
        assert_eq!(stale, None, "hash mismatch must be stale");
    }
}
