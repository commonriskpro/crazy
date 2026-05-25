# Context Server protocol

<!-- Status: Implemented subset. `ail-context` provides an in-process server-shaped API with a stdio/MCP-like newline-delimited JSON-RPC transport; HTTP transport, distributed auth, and distributed freshness remain target design. -->

> Target design. Current implementation scope is called out in the status note and Implementation Notes. Related: [Storage](storage.md), [Verification](verification.md), [AI Change Language](change-language.md), [Tooling](tooling.md).

## Context Server

Para que el LLM no lea muchos archivos, el lenguaje necesita contexto nativo.

En vez de:

```txt
read src/cart.ts
read src/order.ts
read src/payment.ts
```

La IA consulta:

```txt
context uc.checkout
impact field.cart_item.price
contracts module.payment
callers fn.reserve_stock
```

Y recibe slices mínimos:

```txt
UseCase: Checkout

Inputs:
- cartId: CartId

Reads:
- Cart
- Product.stock

Writes:
- Order
- Payment

Calls:
- PaymentProvider.charge

Invariants:
- stock_never_negative
- paid_order_has_payment
- order_total_matches_cart_total
```

El contexto deja de ser textual y pasa a ser computable.

## Context Server protocol: target design

El Context Server es la capa que permite que LLMs trabajen con contexto semántico preciso sin leer archivos tradicionales.

### Tesis

```txt
El LLM no debería buscar contexto en archivos.
El sistema debe entregarle el slice mínimo correcto, verificable y actualizado.
```

Regla fundamental:

```txt
Context Server no es RAG sobre archivos.
Es query semántica sobre Semantic Graph + verification/runtime/storage indexes.
```

### Objetivos

```txt
1. Reducir contexto textual irrelevante.
2. Evitar contexto viejo/stale.
3. Dar dependencias, effects, contracts y riesgos explícitos.
4. Ayudar a refactors seguros.
5. Explicar por qué algo existe o falla.
6. Entregar respuestas parseables por LLM/tooling.
7. Respetar permisos, redaction y policies.
```

### Fuentes de verdad

El Context Server lee de:

```txt
Semantic Graph snapshots
Core IR / ANF indexes
Verification reports
Structural diffs
ChangeSet history
Runtime profiles
Capability manifests
Boundary/assumption registry
Package trust metadata
Audit/runtime reports when allowed
Derived indexes
```

No inventa facts. Si algo no está en estas fuentes, debe marcarlo como missing/unknown.

### Respuesta en dos capas

Decisión:

```txt
Responses tienen dos capas:
- structured: autoritativa
- summary: ayuda no autoritativa
```

Ejemplo:

```txt
context_slice fn.checkout
snapshot snapshot_123
hash ctx_abc

structured
  signature ...
  effects ...
  contracts ...
  dependencies ...
  assumptions ...
  verification_state ...
end

summary
  "checkout reads Cart, charges PaymentProvider, writes Order, and depends on stripe_idempotency assumption."
end
```

Regla:

```txt
Si summary y structured difieren, gana structured.
```

### Snapshot binding and freshness

Toda respuesta debe estar ligada a snapshot/hash.

```txt
snapshot snapshot_123
context_hash ctx_abc
graph_root_hash hash:root123
generated_at 2026-05-21T00:00:00Z
```

Si el LLM usa ese contexto en un ChangeSet, debe referenciarlo:

```txt
requires
  assert_context hash=ctx_abc
  assert_base snapshot_123
end
```

Si el graph cambió:

```txt
E_CONTEXT_STALE
```

Repair:

```txt
query context fn.checkout at latest
rebase change onto snapshot_124
```

### Query model

Forma conceptual:

```txt
context_server.query
  kind <query_kind>
  target <NodeRef|Pattern|ChangeRef|ClaimRef>
  snapshot <SnapshotRef|latest>
  scope <scope>
  budget <budget>
  redaction <policy>
end
```

Common queries:

```txt
context <id>
impact <id>
callers <id>
callees <id>
effects <id>
contracts <id>
proofs <id>
resources <id>
boundaries <id>
history <id>
why <claim|node|edge>
diff <change|snapshotA..snapshotB>
risks <id|change>
todo <id|change>
```

### Context query

Devuelve slice general de un nodo.

```txt
context fn.checkout
```

Response fields:

```txt
identity
signature
visibility
effects
contracts
dependencies
callers/callees summary
resources
boundaries
assumptions
verification_state
recent_changes
risks
```

### Impact query

Devuelve qué se puede romper si cambia un nodo.

```txt
impact type.CartItem.price
```

Response:

```txt
direct_dependents
transitive_dependents
affected_contracts
affected_invariants
affected_tests
affected_capabilities
affected_public_apis
required_reverification
risk_level
```

### Call graph queries

```txt
callers fn.cart_total
callees fn.checkout
```

Debe distinguir:

```txt
direct
transitive
dynamic via Dyn<Interface>
handler calls
effect calls
```

Dynamic dispatch entries deben mostrar interface contract y possible impls by profile.

### Effect/capability queries

```txt
effects module.checkout
capabilities profile=prod module.checkout
handlers cap.payment.charge:PaymentProvider profile=prod
```

Response:

```txt
declared_effects
inferred_effects
granted_capabilities
missing_grants
handler_bindings
handler_internal_effects
manifest_requirements
runtime_denials if known
```

### Contract/proof queries

```txt
contracts fn.checkout
proofs invariant.stock_never_negative
obligations fn.checkout
```

Response:

```txt
requires
ensures
invariants
proof_obligations
proof_status
evidence
failed_attempts
repair_options
dependent_nodes
```

### Resource/concurrency queries

```txt
resources fn.process_file
concurrency module.checkout
tasks fn.fetch_prices
```

Response:

```txt
handles_acquired
ownership_modes
release/transfer points
task groups
await/cancel status
channels
shared state
violations or warnings
```

### Boundary/runtime queries

```txt
boundaries module.payment
assumptions boundary.Stripe
runtime profile=prod module.checkout
```

Response:

```txt
boundaries
trust levels
assumptions
expiration/review status
handlers
runtime profile grants
limits
audit availability
```

### History queries

```txt
history fn.checkout
why fn.checkout.effects.payment.charge
diff snapshot_120..snapshot_124
```

Response:

```txt
created_by
modified_by
relevant_changes
approval_records
verification_reports
structural_diffs
rationale annotations
```

`why` queries should trace provenance:

```txt
why does fn.checkout require payment.charge?
  because body node call.payment.charge uses cap.payment.charge
  added_by change.add_checkout
  verified_by report ver_123
```

### Refactor support queries

For safe refactors:

```txt
refactor_context fn.checkout split_by=effects
extract_candidates fn.checkout
move_safety fn.cart_total to=module.pricing
```

Response:

```txt
behavior_locks_needed
contracts_to_preserve
effects_to_preserve
callers_to_update
proofs_to_rerun
possible_conflicts
suggested_refactor_ops
```

### Response schema

Base envelope:

```txt
context_response <id>
schema context/1.0
query_hash hash:query123
snapshot snapshot_123
graph_root_hash hash:root123
context_hash hash:ctx123
generated_at 2026-05-21T00:00:00Z
freshness fresh | stale | unknown
redaction none | partial | restricted

structured
  ...
end

summary
  "..."
end

limits
  tokens_estimated <n>
  truncated true|false
  omitted_sections [...]
end

provenance
  sources [...]
  indexes [...]
  reports [...]
end

end
```

### Budgets and scoping

Queries must be scoped to avoid dumping the world.

Budget fields:

```txt
max_depth
max_nodes
max_tokens
include_private true|false
include_transitive true|false
include_runtime_logs true|false
profile prod|dev|test
```

If budget is exceeded:

```txt
truncated true
omitted_sections [transitive_callers, runtime_logs]
next_queries suggested
```

The server should suggest follow-up queries instead of flooding context.

### Security and redaction

Context Server enforces access policies.

Redaction applies to:

```txt
secrets
PII
restricted business logic
security-sensitive handlers
runtime payloads
audit logs
```

Rules:

```txt
1. Context query cannot bypass project permissions.
2. Redacted fields must be marked as redacted, not omitted silently.
3. Summary cannot reveal redacted structured data.
4. Security-sensitive context may require approval/session capability.
```

### Freshness and invalidation

Context slices are invalidated by:

```txt
snapshot change
node hash change
dependency hash change
verification report change
runtime profile change
assumption expiration/revocation
policy change
```

ChangeSets using context must include:

```txt
assert_context hash=ctx123
assert_base snapshot_123
```

### Caching and indexes

Context Server can use derived indexes:

```txt
call graph index
type graph index
effect graph index
capability graph index
proof obligation index
invariant dependency index
history/provenance index
runtime audit index
```

Rules:

```txt
1. Indexes are derived, not source of truth.
2. Each response lists index versions/hashes used.
3. Stale index returns freshness=unknown/stale or triggers rebuild.
```

### Failure modes

Possible errors:

```txt
E_CONTEXT_STALE
E_NODE_NOT_FOUND
E_ACCESS_DENIED
E_BUDGET_EXCEEDED
E_INDEX_STALE
E_SNAPSHOT_NOT_FOUND
E_REDACTION_REQUIRED
E_QUERY_AMBIGUOUS
```

Errors should include repair options:

```txt
repair_options
  option query_latest
    context fn.checkout snapshot=latest
  end
  option narrow_scope
    impact type.Cart max_depth=2
  end
end
```

### LLM usage protocol

Before writing a ChangeSet, LLM should request:

```txt
context target
impact target
contracts target
effects target
```

For refactor:

```txt
refactor_context target
impact target
callers target
proofs target
```

For runtime/capability changes:

```txt
effects target
capabilities profile=<profile> target
boundaries target
runtime profile=<profile> target
```

### Non-goals

```txt
- Not a file search engine.
- Not free-form RAG over source text.
- Not an authority for semantic facts outside the graph/reports.
- Not a way to bypass verification.
- Not a replacement for ChangeSets.
```

### Final rules

```txt
1. Every context response is snapshot/hash-bound.
2. Structured data is authoritative; summary is helper only.
3. Context Server reads graph/reports/indexes, not vibes.
4. Redaction is explicit.
5. Stale context must be detected by ChangeSets.
6. Queries are scoped and budgeted.
7. Refactor and impact support are first-class.
8. Context does not approve changes; verifier/policy do.
9. Indexes are rebuildable derived artifacts.
10. The LLM should consume context slices, not source files.
```

### Implementation Notes

The first implementation is an in-process semantic context API rather than a transport server. This preserves the protocol model while avoiding premature network/auth/server complexity.

| Topic | Implementation status |
|-------|-----------------------|
| Query syntax | All protocol-doc query variants (`context`, `impact`, `callers`, `callees`, `effects`, `contracts`, `proofs`, `resources`, `boundaries`, `history`, `why`, `diff`, `risks`, `todo`, `refactor_context`, `extract_candidates`, `move_safety`, `capabilities`, `handlers`, `concurrency`, `tasks`, `assumptions`, plus `graph`) are implemented as `ContextQuery` enum variants with graph traversal handlers in `selection.rs`. CLI/transport syntax remains future work. |
| Transport adapter | `StdioTransport` in `crates/ail-context/src/transport.rs` provides newline-delimited JSON-RPC framing over `ContextServer::handle_rpc`. `ContextRpcRequest` / `ContextRpcResponse` remain the shared envelope; HTTP and MCP adapters can reuse them without changing the server. |
| HTTP transport | `HttpTransport` in `crates/ail-context/src/http_transport.rs` provides a minimal HTTP/1.1 JSON-RPC adapter for local tooling (IDE extensions, integration tests). See **Local transport hardening** below. |
| Summaries | Deterministic renderer in `crates/ail-context/src/summary.rs`. Structured data remains authoritative. |
| Signing | Distributed signing is handled in remote/bundle primitives, not context responses yet. |
| Budgets | Response DTOs include limits and budget-related errors; model-tier defaults remain policy work. |
| Freshness | `ContextServer::query` resolves `Latest` when available and sets `freshness_status=stale` plus `query_latest` repair metadata for older snapshots. |
| Audit/runtime exposure | Field-based node redaction is enforced before slice rendering. Direct queries for redacted targets return `E_ACCESS_DENIED` unless the session trust level satisfies the configured policy; full audit exposure policy remains future work. |

Code references: `crates/ail-context/src/lib.rs`, `builder.rs`, `dto.rs`, `server.rs`, `summary.rs`.

### Local transport hardening (current)

`HttpTransport` is designed for **local/dev use only** (IDE extensions, CLI integrations, integration tests).  Its current hardening is conservative but sufficient for that scope:

| Guard | Default | Notes |
|-------|---------|-------|
| Loopback-only peer filter | **enabled** | `serve_one` checks `stream.peer_addr()` and drops non-loopback connections without sending any response body.  Protects against accidental public exposure when the caller binds to `0.0.0.0` instead of `127.0.0.1`.  Disable via `HttpTransport::with_loopback_only(false)` only in tests or future remote wrappers. |
| Request body size limit | 512 KiB | Checked against `Content-Length` before any body bytes are read; returns HTTP 413. |
| Header size limit | 8 KiB | Cumulative bytes across request line + all headers; stops reading and returns an I/O error if exceeded. |
| Per-connection read timeout | 30 s | Set via `TcpStream::set_read_timeout`. |
| Per-connection write timeout | 30 s | Set via `TcpStream::set_write_timeout`. |
| Method enforcement | POST only | Any other method returns HTTP 405. |
| One request per connection | enforced | Each accepted connection handles exactly one request/response pair then closes. |

### Production hardening (future)

The following are **not** implemented and are explicitly out of scope until the protocol is validated in local use:

```txt
- TLS (mTLS for remote deployments)
- Distributed authentication (OAuth2, API keys, capability tokens)
- Rate limiting and per-client quotas
- Distributed trust / multi-tenant isolation
- Audit log for context queries
- MCP transport adapter
```

The `ContextRpcRequest` / `ContextRpcResponse` envelope is transport-agnostic: adding TLS or a different auth layer does not require changing `ContextServer`.
