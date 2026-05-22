# AGENT.md — SCP (Selective Context Protocol) Implementation Guide

> **Version:** 0.3-draft  
> **Last Updated:** 2026-05-22  
> **Target Audience:** Agents implementing SCP phases 0–7

This document is a dense, reference-quality guide for agents building the SCP project. It synthesizes the full plan.md into actionable implementation guidance.

---

## 1. Project Summary

**SCP** is an MCP-compatible proxy hub that sits between LLM clients and MCP servers. It intercepts all MCP traffic, applies intelligent context selection, and forwards filtered responses. Clients connect to SCP as if it were a standard MCP server; backend servers are unmodified.

**Core principle:** Context is a budget, not a dump. Every token reaching the model must earn its place through measured relevance.

**License:** MIT OR Apache-2.0  
**Target MCP Spec:** 2025-03-26 (Streamable HTTP)  
**Language:** Rust (Cargo workspace)  
**Ports:** MCP listener 3100, Admin API 3101

---

## 2. Architecture Overview

\\\
Clients ? Listener ? Session Mgr ? Router ? Filter Pipeline ? Tool Index + Budget Mgr ? Backend MCP Servers
Admin API (port 3101) runs separately from MCP listener (port 3100)
\\\

**Key layers:**

- **Listener** — accepts stdio, SSE, and Streamable HTTP client connections
- **Session Manager** — creates isolated sessions per client, tracks budgets and request IDs
- **Router** — dispatches requests to backend servers, handles fan-out for list operations
- **Filter Pipeline** — 8-stage processing: content type ? token count ? dedup ? chunking ? relevance ? budget ? progressive disclosure ? delivery log
- **Tool Index** — merged registry of all tools from all servers, scored by relevance
- **Pool Manager** — manages backend connections (shared, pooled, dedicated strategies)
- **Admin API** — HTTP server for runtime management (add/remove/disable servers, config reload, metrics)

---

## 3. Crate Structure

\\\
scp/
+-- scp-core/          # protocol, session, budget, server, filter, tool, error, config
+-- scp-transport/     # stdio, sse, http, listener
+-- scp-pool/          # manager, shared, pooled, dedicated, health, lifecycle
+-- scp-index/         # registry, scorer, tags, tfidf, embedding, usage, alias
+-- scp-filter/        # pipeline, content_type, token_count, chunker, dedup, relevance, budget, summarize, progressive, delivery_log
+-- scp-hub/           # main, hub, session_store, router, server_manager, admin, metrics, tracing_setup, reload
+-- scp-cli/           # main (start, status, servers, sessions, tools, reload)
+-- tests/             # common, passthrough, multi_server, server_lifecycle, multi_client, tool_index, filter_pipeline, budget
\\\

---

## 4. Implementation Phases (Status Tracking)

All phases start at **[NOT STARTED]**. Update status as work progresses.

### Phase 0 (v0.1) — MCP Passthrough
**Status:** [NOT STARTED]

Deliverables:
- Cargo workspace with all crates stubbed
- JSON-RPC 2.0 types (Request, Response, Notification)
- MCP method types (initialize, tools/list, tools/call, ping)
- stdio transport (spawn process, read/write JSON-RPC)
- Single-client passthrough (no sessions, no filtering)
- Request ID remapping (client IDs ? server IDs)
- Tracing setup (JSON or pretty output)
- Integration test: passthrough_test.rs

**Exit criteria:** OpenCode can use SCP as a drop-in replacement for direct MCP server connection.

### Phase 1 (v0.2) — Multi-Server + Basic Filtering
**Status:** [NOT STARTED]

Deliverables:
- TOML config parsing and validation
- SSE transport (client-side)
- SharedPool (single connection, request serialization)
- PoolManager (owns all server connections)
- Lifecycle states (cold/warm/hot/draining/disabled/failed)
- ToolRegistry (qualified names, collision detection)
- Router (tools/call routing, tools/list fan-out)
- Heuristic token counter (bytes / 3.5)
- Hard budget truncation (200-token minimum)
- ServerManager trait (add/remove/disable/enable at runtime)
- Admin API (server management, config reload)
- SIGHUP hot-reload (diff + apply)
- CLI (start, status, servers, sessions, tools, reload)
- Integration tests: multi_server, server_lifecycle

**Exit criteria:** SCP proxies 3+ real servers. Tool routing works. Large responses truncated. Servers manageable at runtime.

### Phase 2 (v0.3) — Multi-Client + Sessions
**Status:** [NOT STARTED]

Deliverables:
- Streamable HTTP server (accept client connections)
- ClientListener (stdio + HTTP)
- SessionStore (create, lookup, expire)
- Session struct (per-client budget, request ID map, capabilities, roots)
- WorkerPool (N instances, least-outstanding dispatch)
- DedicatedPool (per-session instances)
- Bearer token auth (token ? profile mapping)
- Request ID mapping per session
- Cancellation forwarding
- Sampling request routing (server?client)
- Roots handling (per-session)
- Integration tests: multi_client, session isolation

**Exit criteria:** 3 concurrent clients (2 HTTP, 1 stdio) with fully isolated sessions.

### Phase 3 (v0.4) — Tool Index + Smart Selection
**Status:** [NOT STARTED]

Deliverables:
- TagScorer (Jaccard similarity)
- TfIdfScorer (cosine similarity on descriptions)
- UsageTracker (call frequency, Bayesian scoring)
- ScoringPipeline (weighted combination)
- KeywordAccumulator (extract from tool args, decay over time)
- tools/list returns top-N scored tools per session
- tools/list cache (TTL 5min, invalidated on list_changed)
- Handle notifications/tools/list_changed
- CLI: tools list/search
- Integration test: 50 tools, verify =20 returned

**Exit criteria:** Filesystem work shows filesystem tools ranked higher. Search work shows search tools ranked higher.

### Phase 4 (v0.5) — Relevance Filtering
**Status:** [NOT STARTED]

Deliverables:
- ChunkSplitter (paragraph, line, json_element, fixed_size)
- ContentTypeRouter (text/json/image/binary classification)
- RelevanceScorer (TF-IDF chunks vs. session context)
- Smart budget enforcer (select top-k chunks by score)
- DedupFilter (SHA-256 hashing, delivery log check)
- Bounded LRU delivery log (10k entries)
- Full 8-stage pipeline
- scp_tokens_saved_total metric
- Integration tests: large log filtering, dedup

**Exit criteria:** Measurable token savings. 10KB log filtered to relevant lines.

### Phase 5 (v0.6) — Embeddings + Progressive Disclosure
**Status:** [NOT STARTED]

Deliverables:
- EmbeddingScorer (Ollama /api/embed, cached)
- Embedding-based chunk scoring
- ProgressiveDisclosure (cache full response, inject scp_get_more hint)
- scp_get_more tool (retrieve cached response, next batch)
- SCP extension tools (scp_info, scp_budget, scp_budget_reset, scp_get_more)
- Intent hint support (_scp field in tool args)
- Graceful fallback (embedding down ? TF-IDF)
- Integration test: semantic scoring vs. TF-IDF

**Exit criteria:** Progressive disclosure works end-to-end. Embedding scoring measurably better than TF-IDF.

### Phase 6 (v0.7) — Observability + Production Monitoring
**Status:** [NOT STARTED]

Deliverables:
- Full admin API (sessions, tools search, health)
- Prometheus metrics endpoint (all metrics from plan §5.7)
- OpenTelemetry (OTLP) trace export
- HealthChecker (periodic pings, failure counting, state transitions)
- CLI: session management, tools search, metrics
- Structured logging (trace_id on every request)
- Grafana dashboard JSON
- Integration test: trace single request end-to-end

**Exit criteria:** Metrics show token savings, latency, server health. Single request traceable via Jaeger/Tempo.

### Phase 7 (v1.0) — Hardening + Production Stability
**Status:** [NOT STARTED]

Deliverables:
- Graceful shutdown (SIGTERM ? drain ? exit)
- Connection retry with exponential backoff + jitter
- Comprehensive error handling (no panics)
- Streamable HTTP transport for backends (client-side)
- Fuzzing (cargo-fuzz on JSON-RPC parser)
- Load testing (50 clients × 100 servers)
- Resource limits verification (bounded memory)
- Full integration test suite (real MCP servers)
- Documentation (README, config reference, CHANGELOG)
- 72-hour stability test

**Exit criteria:** Runs continuously 72h under load without memory growth, panics, or leaked connections.

---

## 5. Key Design Decisions & Constraints

### Token Counting
- **Heuristic:** bytes / 3.5 (English/code), bytes / 2.5 (non-Latin)
- **Intentionally conservative:** overestimates slightly to stay under budget
- **Swappable:** trait-based, can swap for tiktoken-rs if client declares model
- **Decision point:** Phase 1 (keep heuristic vs. model-specific tokenizer)

### Filter Pipeline Order (Fixed, 8 Stages)
1. **Content Type Router** — classify: text | json | image | binary | mixed
2. **Token Measurement** — count tokens; short-circuit if under budget
3. **Dedup Check** — hash chunks, check delivery log, drop duplicates
4. **Chunk Splitter** — split large text (paragraph/line/json_element/fixed_size)
5. **Relevance Scorer** — score chunks against session context
6. **Budget Enforcer** — select top-k chunks that fit budget (truncate/summarize/hybrid)
7. **Progressive Disclosure Annotator** — append metadata if chunks dropped
8. **Delivery Logger** — record hashes in session delivery log

**Why this order matters:** Content type first (can't score images), token measurement short-circuits small responses, dedup before scoring, chunking before scoring, relevance before budget, progressive disclosure after budget, delivery logging last.

### Budget Hierarchy
`
request_budget = min(
    session.remaining_budget,
    config.max_tokens_per_request,
    estimated_need(tool) * 1.2  // 20% headroom
)
`

- **Global ? Session ? Request** hierarchy
- **Exhausted budget:** still delivers but truncates to 200-token minimum
- **Replenishment strategies:** per_request (default), per_turn, sliding_window, manual

### Session Isolation
- **Full isolation:** no cross-session leakage
- **Request ID mapping:** client IDs ? SCP internal IDs per session
- **Delivery log:** per-session, capped at 10k hashes with LRU eviction
- **Call history:** per-session, capped at 100 entries
- **Context keywords:** per-session, fixed-size TF-IDF accumulator with decay

### Name Stripping on Forward
`
Client calls:        tools/call { name: "chromadb.search", ... }
SCP forwards:        tools/call { name: "search", ... }

Client calls:        resources/read { uri: "scp://filesystem/file:///data" }
SCP forwards:        resources/read { uri: "file:///data" }

Client calls:        prompts/get { name: "notion.summarize", ... }
SCP forwards:        prompts/get { name: "summarize", ... }
`

### Sharing Strategies
- **Shared:** one connection, serialized requests, pipelined by request ID
- **Pooled:** N instances, least-outstanding-requests dispatch, lazy startup
- **Dedicated:** per-session instances, full isolation

### Health Checking
- **Ping interval:** 30s
- **Failure thresholds:** 3 missed pings ? degraded, 5 ? unhealthy ? cold
- **Reconnect:** on next request, up to max_retries

### Rate Limiting
- **Per-session token bucket:** 100 req/min, burst 20 (configurable)
- **Per-profile overrides:** via auth token mapping
- **Exceeded:** return JSON-RPC error (-32003)

### Memory Limits
- **delivery_log:** 10k hashes LRU
- **call_history:** 100 entries
- **TF-IDF accumulator:** fixed-size with decay

### Summarization (Optional)
- **Strategy:** truncate (default) | summarize | hybrid
- **Model:** qwen3:1.7b via Ollama
- **Latency:** max 500ms, fallback to truncate if slow
- **Thresholds (hybrid):** >0.7 relevance pass through, 0.3-0.7 summarize, <0.3 drop

### Fan-out Timeout
- **Default:** 5s per server
- **Behavior:** slow/failed servers logged but don't block response
- **Consistency:** tool list always consistent (update internal state ? update index ? notify clients)

### tools/list Cache
- **Per-server:** TTL 5min
- **Invalidation:** on list_changed notification
- **Fallback:** TTL if no notification received

### Config Hot-Reload
- **Trigger:** SIGHUP or POST /admin/config/reload
- **Concurrency:** tokio::Mutex, one at a time, 409 Conflict if in progress
- **Env var interpolation:** , fail-fast on missing
- **Sequence:** read ? validate ? diff ? apply (remove ? disable ? update ? add ? enable) ? rebuild indexes ? notify clients

### Network & Deployment
- **Binds:** 127.0.0.1 by default (localhost only)
- **TLS:** none (use reverse proxy)
- **Single static binary:** no runtime deps beyond libc
- **Docker:** multi-stage alpine build to ghcr.io/eliasstepanik/scp-hub

---

## 6. Open Design Questions (Must Resolve Before Implementation)

### Blocking (Phase 0)
**Q1:** Use mcp crate or hand-roll JSON-RPC?
- Evaluate: maturity, coverage of all MCP primitives, transport support
- Decision required before Phase 0 starts

### Blocking (Phase 1)
**Q2:** Budget unit — heuristic bytes/3.5 vs. model-specific tokenizer?
- (a) Keep heuristic — simple, good enough
- (b) Client declares model via _scp extension, select matching tokenizer
- (c) Budget in abstract "SCP units" decoupled from tokenizer
- Decision point: Phase 1

### Blocking (Phase 2)
**Q3:** Sampling routing for shared servers — which client?
- (a) Track in-flight session per server, route to that session
- (b) Require dedicated strategy for servers using sampling
- (c) Include SCP request ID in custom field (breaks MCP purity)
- Decision point: Phase 2

---

## 7. Config Format Reference

**File:** TOML, config_version = 1

**Key sections:**

- **hub:** listen_address/port (3100), transports, max_clients, session_timeout, defaults (token budgets, max_tools_exposed=20, fanout_timeout=5s, rate limits)
- **hub.auth:** bearer tokens file (hot-reloadable)
- **hub.profiles:** named session profiles (default, opencode, lightweight)
- **tool_index:** engine (tags/tfidf/embedding), max_tools_per_list, always_include, embedding config
- **tool_aliases:** collision resolution
- **filter:** enabled, chunking strategy, relevance engine, budget strategy (truncate/summarize/hybrid)
- **filter.progressive_disclosure:** enabled, hint_text template
- **[[servers]]:** name, transport, command/url, sharing, pool_size, priority, tags, timeouts, retries, enabled, env, headers
- **admin:** port 3101, auth_token
- **logging:** level, format (json/pretty), file
- **telemetry:** OTLP export, service_name, sample_rate
- **namespacing:** resources/prompts/tools (always/on_collision/never, default on_collision)

See plan.md §7.1 for full example.

---

## 8. MCP Protocol Details

### 6 Primitives
- **Tools** (tools/list, tools/call) — highest impact for filtering
- **Resources** (resources/list, resources/read, resources/subscribe) — URI-addressed content
- **Prompts** (prompts/list, prompts/get) — parameterized message templates
- **Sampling** (sampling/createMessage) — server asks client's LLM for completion
- **Roots** (roots/list) — client declares filesystem roots
- **Logging** (logging/setLevel, notifications/message) — server sends log messages

### 8 Notification Types
- notifications/initialized
- notifications/tools/list_changed
- notifications/resources/list_changed
- notifications/resources/updated
- notifications/prompts/list_changed
- notifications/progress
- notifications/message (logging)
- notifications/cancelled

### Request ID Mapping
`
Client A sends:  { id: 1, method: "tools/call", ... }
SCP generates:   { id: "scp-a3f9-001", method: "tools/call", ... }  ? server
Server responds: { id: "scp-a3f9-001", result: ... }
SCP maps back:   { id: 1, result: ... }  ? Client A
`

Prevents ID collisions when multiple clients use overlapping ID spaces.

### Routing Table
| Request | Logic |
|---------|-------|
| tools/list | Fan-out all servers, merge, filter through tool index |
| tools/call | Look up tool name ? get server ? dispatch to pool |
| resources/list | Fan-out, merge, apply URI namespacing |
| resources/read | Strip scp://{server}/ prefix ? route to owning server |
| resources/subscribe | Route to owning server, track subscription |
| prompts/list | Fan-out, merge, apply name qualification |
| prompts/get | Look up qualified name ? strip prefix ? route |
| sampling/createMessage | Route back to originating client session |
| roots/list | Respond directly from session state |
| ping | Respond directly |
| logging/setLevel | Forward to all servers (or per-server) |

---

## 9. SCP Extensions (SCP-Aware Clients Only)

### Intent Hints
`json
{
    "method": "tools/call",
    "params": {
        "name": "filesystem.read_file",
        "arguments": {
            "path": "/var/log/app.log",
            "_scp": {
                "intent": "find error messages from the last hour",
                "max_tokens": 500
            }
        }
    }
}
`

Stripped before forwarding to backend. Used for relevance scoring and budget override.

### Budget Control
`json
{ "method": "tools/call", "params": { "name": "scp_budget", "arguments": {} } }
// Returns: { "remaining": 3200, "total": 4000, "strategy": "per_request" }

{ "method": "tools/call", "params": { "name": "scp_budget_reset", "arguments": {} } }
`

### Progressive Disclosure Retrieval
`json
{ "method": "tools/call", "params": { "name": "scp_get_more", "arguments": { "request_id": "scp-a3f9-001", "offset": 15, "limit": 15 } } }
`

Returns next batch of chunks from cached full response.

### Model Declaration
`json
{
    "method": "initialize",
    "params": {
        "clientInfo": { "name": "opencode", "version": "1.0" },
        "_scp": {
            "model": "claude-sonnet-4-20250514",
            "context_window": 200000
        }
    }
}
`

Enables more accurate token counting.

### Extension Discovery
`json
{ "method": "tools/call", "params": { "name": "scp_info", "arguments": {} } }
// Returns: { "version": "0.3.0", "extensions": ["intent_hints", "budget_control", "progressive_disclosure", "model_declaration"], "servers": 42, "tools": 187 }
`

---

## 10. Prometheus Metrics

**Primary metric:** scp_tokens_saved_total — shows exactly how much context budget SCP is saving.

**Full list:**
- scp_sessions_active (gauge)
- scp_servers_total (gauge)
- scp_servers_healthy (gauge)
- scp_servers_disabled (gauge)
- scp_requests_total{server,tool} (counter)
- scp_request_duration_seconds{server} (histogram)
- scp_tokens_received_total{server} (counter)
- scp_tokens_delivered_total{server} (counter)
- scp_tokens_saved_total{server} (counter)
- scp_filter_ratio{server} (gauge)
- scp_tool_index_size (gauge)
- scp_tool_index_rebuild_total (counter)
- scp_pool_connections_active{server} (gauge)
- scp_pool_queue_depth{server} (gauge)

---

## 11. Development Workflow

- **Format:** cargo fmt before committing
- **Lint:** cargo clippy -- -D warnings must pass
- **Tests:** cargo test, integration tests in tests/
- **No panics:** in production paths (Phase 7 hardens this)
- **CI:** fmt + clippy + test on push/PR
- **Deployment:** Docker multi-stage alpine build to ghcr.io/eliasstepanik/scp-hub

---

## 12. Tool Index Algorithm (tools/list)

1. **Scope filter** — remove tools not in session's allowlist
2. **Health filter** — remove tools from failed/unhealthy servers
3. **Tag pre-filter** — remove tools with zero tag overlap (cheap, coarse)
4. **Relevance scoring** — score by active engine:
   - tags: Jaccard similarity (0.0-1.0)
   - tfidf: cosine similarity on descriptions (0.0-1.0)
   - embedding: cosine similarity on embeddings (0.0-1.0)
   - usage: Bayesian score by call frequency (0.0-1.0)
   - Final: primary × 0.7 + usage × 0.3
5. **Top-N selection** — return top N tools (default 20)
6. **Mandatory tools** — always_include tools bypass scoring

**Context sources:**
- Tool call arguments (what client is searching for)
- Tool names called (filesystem calls suggest code work)
- Intent hints (optional SCP extension)
- Accumulated keywords (decayed over time)

---

## 13. Lifecycle States

| State | Meaning | Transitions |
|-------|---------|-------------|
| cold | Not connected, process not running | Initial; after idle timeout; after max failures |
| starting | Connection/process being established | Request arrives for cold server |
| warm | Connected, idle, awaiting requests | After completing request; after startup |
| hot | Actively processing requests | Request dispatched |
| draining | No new requests; waiting for in-flight | Shutdown; removal; config reload; deactivation |
| disabled | Administratively deactivated, tools hidden | Explicit admin action |
| failed | Consecutive failures exceeded threshold | N consecutive errors (default 5) |

---

## 14. Error Handling

### Backend Server Errors
| Error | SCP behavior |
|-------|-------------|
| Process crashes (stdio) | Return MCP error, mark failed, clean up in-flight, log crash |
| Connection refused (SSE/HTTP) | Retry with backoff, after max_retries return error, mark failed |
| Timeout (no response within request_timeout) | Return MCP error, cancel request on server if possible |
| Malformed JSON-RPC | Return MCP internal error, log malformed response |
| Server returns MCP error | Forward unchanged (SCP is transparent) |
| Pool exhausted + queue full | Return "server overloaded" error |

### Error Codes
- -32000: Server error (crash, timeout)
- -32001: Server overloaded (pool exhausted)
- -32002: Server unhealthy (failed health check)
- -32003: Rate limit exceeded (per-session throttle)
- -32603: Internal error (SCP bug)

### Graceful Degradation
SCP prefers delivering unfiltered content over delivering nothing. If any filtering stage fails, it is bypassed, not retried.

---

## 15. Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Passthrough latency (no filtering) | < 2ms added | JSON parse + ID remap + serialize |
| Filter pipeline latency (TF-IDF) | < 10ms | Typical response < 5KB |
| Filter pipeline latency (embedding) | < 100ms | Dominated by Ollama API call |
| Summarization latency | < 500ms | Small local model |
| tools/list latency (cached) | < 5ms | Scoring + sorting |
| tools/list latency (fan-out, 100 servers) | < 6s | 5s timeout + 1s overhead |
| Memory per session | < 1MB | Capped data structures |
| Memory baseline (hub, no sessions) | < 50MB | Includes tool index, config, runtime |
| Max concurrent sessions | 50+ | Configurable, bounded |
| Max backend servers | 200+ | Lazy connections, bounded pools |

---

## 16. Quick Reference: Where to Add Code

### New Transport Type
1. Create scp-transport/src/new_transport.rs
2. Implement Transport trait (send, receive, close)
3. Add variant to TransportConfig enum in scp-core/config.rs
4. Update ClientListener in scp-transport/listener.rs to accept new transport
5. Add integration test in 	ests/

### New Filter Stage
1. Create scp-filter/src/new_stage.rs
2. Implement ContextFilter trait (filter, name)
3. Add to FilterPipeline in scp-filter/pipeline.rs (respecting order)
4. Update config to expose new stage settings
5. Add integration test in 	ests/filter_pipeline_test.rs

### New Scoring Engine
1. Create scp-index/src/new_scorer.rs
2. Implement ToolScorer trait (score, name)
3. Add variant to ScoringEngine enum in scp-core/config.rs
4. Update ScoringPipeline in scp-index/scorer.rs to use new engine
5. Add integration test in 	ests/tool_index_test.rs

### New Admin API Endpoint
1. Add handler function in scp-hub/admin.rs
2. Register route in axum router
3. Update scp-cli to call new endpoint
4. Add integration test in 	ests/

### New CLI Subcommand
1. Add subcommand to scp-cli/main.rs
2. Implement handler (likely calls admin API)
3. Add help text and examples
4. Test manually

---

## 17. Testing Strategy

### Unit Tests
- Per-crate: #[cfg(test)] mod tests { ... }
- Test individual components (TokenEstimator, ChunkSplitter, etc.)
- Mock dependencies where needed

### Integration Tests
- 	ests/common/ — shared utilities (mock_mcp_server, test_client)
- 	ests/passthrough_test.rs — Phase 0 (transparent proxy)
- 	ests/multi_server_test.rs — Phase 1 (routing, fan-out)
- 	ests/server_lifecycle_test.rs — Phase 1 (add/remove/disable/enable)
- 	ests/multi_client_test.rs — Phase 2 (session isolation)
- 	ests/tool_index_test.rs — Phase 3 (scoring, filtering)
- 	ests/filter_pipeline_test.rs — Phase 4 (relevance filtering)
- 	ests/budget_test.rs — budget allocation and enforcement

### Manual Testing
- OpenCode integration (Phase 0 exit criteria)
- Real MCP servers (filesystem, fetch, memory)
- Load testing (50 clients × 100 servers)
- 72-hour stability test (Phase 7)

---

## 18. Debugging Tips

### Tracing
- Set RUST_LOG=debug or RUST_LOG=scp_hub=trace for detailed logs
- Every request gets a trace_id (Phase 6+)
- Logs include server name, latency, request ID

### Metrics
- GET /metrics (Prometheus format)
- Primary metric: scp_tokens_saved_total
- Check scp_servers_healthy, scp_pool_queue_depth for bottlenecks

### Admin API
- GET /health — hub health status
- GET /servers — list all servers with state
- GET /sessions — list active sessions
- GET /tools — global tool index

### Config Validation
- scp-hub start --config config.toml validates on startup
- POST /config/reload validates before applying
- Errors logged with clear messages

---

## 19. Deployment Checklist

- [ ] Binary built and tested
- [ ] Config file created and validated
- [ ] Auth tokens file created (if using bearer auth)
- [ ] Backend MCP servers configured and reachable
- [ ] Admin API token set (SCP_ADMIN_TOKEN env var)
- [ ] Logging configured (level, format, file)
- [ ] Telemetry configured (if using OTLP)
- [ ] Systemd service file created (if deploying as service)
- [ ] Docker image built and pushed (if using Docker)
- [ ] Reverse proxy configured (if using TLS)
- [ ] Firewall rules configured (ports 3100, 3101)
- [ ] Monitoring/alerting configured (Prometheus, Grafana)
- [ ] Graceful shutdown tested (SIGTERM behavior)
- [ ] Config hot-reload tested (SIGHUP behavior)

---

## 20. References

- **plan.md** — full specification (this document synthesizes it)
- **config.example.toml** — documented example configuration
- **MCP Spec 2025-03-26** — official protocol specification
- **Rust Async Book** — tokio, async/await patterns
- **Prometheus Docs** — metrics format and best practices

---

**Last Updated:** 2026-05-22  
**Maintained by:** Elias Stepanik
