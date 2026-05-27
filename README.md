# SCP — Selective Context Protocol

[![Build Status](https://github.com/eliasstepanik/scp/actions/workflows/ci.yml/badge.svg)](https://github.com/eliasstepanik/scp/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/eliasstepanik/scp#license)
[![Version](https://img.shields.io/badge/version-0.2.0-brightgreen.svg)](https://github.com/eliasstepanik/scp/releases)
[![Status](https://img.shields.io/badge/status-Production%20Ready-brightgreen.svg)](#status)

**MCP-compatible proxy hub that intelligently filters tool responses to stay within LLM context window budgets.**

SCP sits between your LLM client and MCP servers, treating context as a budget, not a dump. Every token must earn its place through measured relevance. Clients connect to SCP as if it's a normal MCP server — zero changes required.

---

## What SCP Does

### Core Value Proposition

**Context is a budget, not a dump.** Modern LLM applications face a fundamental problem: MCP servers return raw data with no concept of relevance or cost. A single `filesystem/read_file` call can consume your entire context window on content you may need three lines from.

SCP solves this by:

1. **Sitting in the middle** — clients connect to SCP, SCP connects to backend MCP servers
2. **Measuring every response** — tokenizing and budgeting all content
3. **Filtering intelligently** — scoring responses by relevance and truncating/summarizing to fit your budget
4. **Multiplexing servers** — one SCP instance connects to many backends, sharing connections efficiently
5. **Staying transparent** — clients need zero changes; non-SCP-aware clients work as-is

**Core principle:** Every token that reaches the model must earn its place through measured relevance.

---

## Quick Example

### The problem

You ask your AI assistant: "Find all TODO comments in my project."

Without SCP, the assistant calls `filesystem/read_file` for each source file and receives the full contents of every file — potentially hundreds of kilobytes of irrelevant code. A medium-sized project can easily consume 12,000+ tokens for a query whose answer is 20 lines.

**Without SCP — raw response (excerpt):**

```
// src/server.rs (847 lines, 9,200 tokens)

use std::net::TcpListener;
use std::io::{BufReader, BufWriter};
// ... 840 more lines of irrelevant code ...
// TODO: add connection timeout
// ... 200 more lines ...
```

Context window fills up fast. You may need to read 10 files before the assistant has seen all the TODOs.

---

### With SCP in the middle

SCP intercepts the `filesystem/read_file` response before it reaches the model, runs it through the filter pipeline, and returns only the chunks that scored highest against the session context ("TODO comments").

```
LLM Client ──► SCP Hub ──► filesystem MCP server
                  │
            Filter Pipeline
            (chunk → score → select)
                  │
            ◀──── top-k relevant chunks only
```

**With SCP — filtered response:**

```
[scp: 3 of 847 chunks delivered, request_id=req_7f3a, 11,600 tokens saved]

src/server.rs:214  // TODO: add connection timeout
src/auth.rs:88     // TODO: rotate signing key before v1.0
src/db.rs:331      // TODO: index on user_id column for perf
```

**Token count: 12,000 → 400 delivered.**

The assistant gets exactly what it needs. You can ask for more with `scp_get_more` if the results are incomplete.

---

### Built-in SCP tools

SCP exposes four synthetic tools that are always available to the client, regardless of which backend servers are connected:

| Tool | What it does |
|---|---|
| `scp_get_more` | Fetches the next batch of filtered chunks for a previous response. Pass `request_id` and `offset` to paginate results that were truncated by the budget. |
| `scp_info` | Returns hub version, active extensions, and the count of connected servers and indexed tools. Useful for debugging and introspection. |
| `scp_budget` | Shows the current session token budget: total, remaining, and the enforcement strategy in effect. |
| `scp_budget_reset` | Resets the session budget back to its initial value. Use this after a large operation to restore headroom for the next task. |

These tools let the model stay informed and in control of context: it can check budget before a heavy operation, paginate through large result sets, and inspect what SCP is doing — without any changes to the client application.

---

## Architecture

```
LLM Client  ──────────►  SCP Hub (port 3100)  ──────────►  Backend MCP Servers
                                │                                    ▲
                                │  Filter Pipeline                   │
                                │  ┌──────────────────────┐          │
                                │  │ 1. Content Type       │          │
                                │  │ 2. Token Measure      │          │
                                │  │ 3. Dedup Check        │          │
                                │  │ 4. Chunk Split        │──────────┘
                                │  │ 5. Relevance Score    │
                                │  │ 6. Budget Enforce     │
                                │  │ 7. Progressive Hint   │
                                │  │ 8. Delivery Log       │
                                │  └──────────────────────┘
                                │
                                ▼
                       Admin API (port 3101)
```

### Filter Pipeline

Every response flows through an 8-stage pipeline:

1. **Content Type Router** — classify text, JSON, images, binary
2. **Token Measurement** — count tokens; short-circuit if under budget
3. **Dedup Check** — drop content already delivered in this session
4. **Chunk Splitter** — split large text into paragraphs/lines/JSON elements
5. **Relevance Scorer** — score chunks against session context using embeddings, TF-IDF, or tags
6. **Budget Enforcer** — select top-k chunks that fit budget (truncate/summarize/hybrid)
7. **Progressive Disclosure** — append metadata if chunks were dropped
8. **Delivery Logger** — record what was sent to prevent re-delivery

---

## Features

- ✅ **Transparent MCP proxy** — clients need zero changes
- ✅ **Multi-client sessions** — request ID isolation, per-profile budgets and rate limits
- ✅ **Bearer token authentication** — profile-based access control
- ✅ **8-stage filter pipeline** — content type → token measure → dedup → chunking → scoring → budget enforcement → progressive disclosure → delivery log
- ✅ **Embedding-based relevance scoring** — Ollama nomic-embed-text with TF-IDF/tags fallback
- ✅ **Tool index with semantic scoring** — up to 20 tools exposed per session, usage tracking
- ✅ **Progressive disclosure** — `scp_get_more` for paginating filtered content
- ✅ **Circuit breaker per backend** — threshold=5, probe=30s
- ✅ **Prometheus metrics** — `GET /metrics` with token savings tracking
- ✅ **Structured JSON logging** — correlation IDs for request tracing
- ✅ **Graceful shutdown** — request draining with configurable timeout
- ✅ **Hot reload** — SIGHUP or Admin API trigger
- ✅ **Admin API** — servers, sessions, tools, metrics, health endpoints

---

## Quick Start

### Build

```bash
cargo build --release
```

### Create Configuration

```bash
cat > scp.toml << 'EOF'
[hub]
listen_address = "127.0.0.1"
listen_port = 3100

[admin]
listen_address = "127.0.0.1"
port = 3101

[[servers]]
name = "my-server"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
sharing = "shared"
EOF
```

### Run

```bash
./target/release/scp-hub --config scp.toml
```

### Verify Health

```bash
curl http://localhost:3101/health
```

---

### Docker

A pre-built Docker image is published to GitHub Container Registry on every push to `master`:

```bash
docker pull ghcr.io/eliasstepanik/scp:latest
```

Example `docker-compose.yml`:

```yaml
services:
  scp-hub:
    image: ghcr.io/eliasstepanik/scp:latest
    ports:
      - "3100:3100"
      - "3101:3101"
    volumes:
      - ./config:/etc/scp:ro
    restart: unless-stopped
```

Mount your `scp.toml` at `/etc/scp/scp.toml`.

---

## Configuration Reference

SCP uses TOML for configuration. Here is a complete annotated example:

```toml
# Hub configuration
[hub]
listen_address = "127.0.0.1"
listen_port = 3100
session_timeout_secs = 3600
fanout_timeout_secs = 30
tool_cache_ttl_secs = 300
shutdown_timeout_secs = 30

# Admin API configuration
[admin]
listen_address = "127.0.0.1"
port = 3101

# Filter pipeline configuration
[filter]
enabled = true
short_circuit_below_tokens = 500
progressive_disclosure_enabled = true
relevance_engine = "embedding"  # tags | tfidf | embedding
intent_hint_enabled = true

# Embedding configuration (when relevance_engine = "embedding")
[filter.embedding]
model = "nomic-embed-text"
endpoint = "http://localhost:11434"
cache_embeddings = true
cache_path = "/tmp/scp_embeddings"

# Tool index configuration
[tool_index]
engine = "embedding"  # tags | tfidf | embedding
primary_weight = 0.6
usage_weight = 0.4
max_tools_exposed = 20

# Authentication (optional)
[hub.auth]
method = "bearer"  # none | bearer

[[hub.auth.profiles]]
name = "default"
token = "sk-scp-default-token"
token_budget_per_session = 64000
rate_limit_per_minute = 100

# Backend servers
[[servers]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"]
sharing = "shared"  # shared | pooled | dedicated
pool_size = 3

[[servers]]
name = "chromadb"
transport = "sse"
url = "http://localhost:8000/sse"
sharing = "shared"
```

### Configuration Sections

**`[hub]`** — Main hub settings
- `listen_address` — bind address (default: 127.0.0.1)
- `listen_port` — MCP port (default: 3100)
- `session_timeout_secs` — idle session timeout (default: 3600)
- `fanout_timeout_secs` — backend request timeout (default: 30)
- `tool_cache_ttl_secs` — tool index cache TTL (default: 300)
- `shutdown_timeout_secs` — graceful shutdown timeout (default: 30)

**`[admin]`** — Admin API settings
- `listen_address` — bind address (default: 127.0.0.1)
- `port` — admin API port (default: 3101)

**`[filter]`** — Filter pipeline settings
- `enabled` — enable filtering (default: true)
- `short_circuit_below_tokens` — skip filtering if response < N tokens (default: 500)
- `progressive_disclosure_enabled` — enable `scp_get_more` (default: true)
- `relevance_engine` — scoring method: tags, tfidf, or embedding (default: embedding)
- `intent_hint_enabled` — parse `_scp` hints in requests (default: true)

**`[filter.embedding]`** — Embedding configuration (when relevance_engine = "embedding")
- `model` — Ollama model name (default: nomic-embed-text)
- `endpoint` — Ollama API endpoint (default: http://localhost:11434)
- `cache_embeddings` — cache computed embeddings (default: true)
- `cache_path` — embedding cache directory (default: /tmp/scp_embeddings)

**`[tool_index]`** — Tool index settings
- `engine` — scoring engine: tags, tfidf, or embedding (default: embedding)
- `primary_weight` — weight for primary scoring method (default: 0.6)
- `usage_weight` — weight for usage frequency (default: 0.4)
- `max_tools_exposed` — max tools per session (default: 20)

**`[hub.auth]`** — Authentication (optional)
- `method` — auth method: none or bearer (default: none)
- `profiles` — array of auth profiles with token and budget

**`[[servers]]`** — Backend server configuration (repeatable)
- `name` — server identifier
- `transport` — stdio, sse, or streamable_http
- `command` — executable name (for stdio)
- `args` — command arguments (for stdio)
- `url` — server URL (for sse/streamable_http)
- `sharing` — shared, pooled, or dedicated (default: shared)
- `pool_size` — connection pool size (for pooled sharing, default: 3)

---

## Extension Tools

SCP provides four synthetic tools always available to clients:

| Tool | Description |
|---|---|
| `scp_get_more` | Retrieve the next batch of filtered content (pagination). Arguments: `request_id`, `offset`, `limit`. |
| `scp_info` | Get hub version, extensions, and connected server/tool counts. |
| `scp_budget` | Get current session token budget status (remaining, total, strategy). |
| `scp_budget_reset` | Reset the session token budget to initial value. |

---

## CLI Reference

SCP includes a CLI for common operations:

```bash
# Start the hub
scp start --config scp.toml

# Check hub status
scp status

# List servers
scp servers

# List active sessions
scp sessions

# List tools
scp tools

# Get metrics
scp metrics

# Check health
scp health

# Reload configuration
scp reload
```

---

## Performance

Benchmark results from v1.0 (measured on Intel i7-9700K):

| Operation | Time |
|---|---|
| Small pipeline (10 tools) | ~11.7 µs |
| Large pipeline (100 tools) | ~114.3 µs |
| Token counting (10KB) | ~22.9 µs |
| Relevance scoring (50 chunks) | ~6.2 µs |

---

## Phase History

SCP follows a 7-phase roadmap. All phases are complete:

| Phase | Version | Focus | Status |
|-------|---------|-------|--------|
| 0 | v0.1 | stdio passthrough, JSON-RPC plumbing | ✅ Complete |
| 1 | v0.2 | SSE, config, server pool, tool registry, admin API | ✅ Complete |
| 2 | v0.3 | Streamable HTTP, session store, auth | ✅ Complete |
| 3 | v0.4 | Scored tool index (TF-IDF, tags) | ✅ Complete |
| 4 | v0.5 | Full filter pipeline | ✅ Complete |
| 5 | v0.6 | Embedding scorer, progressive disclosure | ✅ Complete |
| 6 | v0.7 | Full admin API, Prometheus, OTLP tracing | ✅ Complete |
| 7 | v1.0 | Production hardening, docs, memory safety | ✅ Complete |

---

## Admin API

SCP provides an HTTP Admin API (port 3101) for runtime management:

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Hub health status |
| `GET` | `/metrics` | Prometheus-format metrics |
| `GET` | `/servers` | List all servers with health status |
| `POST` | `/servers` | Add a new server at runtime |
| `DELETE` | `/servers/{name}` | Remove a server |
| `PUT` | `/servers/{name}` | Update a server's config |
| `POST` | `/servers/{name}/disable` | Deactivate a server |
| `POST` | `/servers/{name}/enable` | Reactivate a disabled server |
| `GET` | `/sessions` | List active sessions |
| `GET` | `/sessions/{id}` | Session details |
| `DELETE` | `/sessions/{id}` | Force-close a session |
| `GET` | `/tools` | Global tool index |
| `GET` | `/tools?q={keyword}` | Search tools by keyword |
| `POST` | `/config/reload` | Trigger config hot-reload |

---

## Metrics

SCP exposes Prometheus metrics at `GET /metrics` (Admin API, port 3101).

**Primary metric:**
- `scp_tokens_saved_total` — total tokens filtered out across all sessions

**Full metric list:**
- `scp_sessions_active` — current active sessions
- `scp_servers_total` — total registered servers
- `scp_servers_healthy` — servers in healthy state
- `scp_servers_disabled` — servers in disabled state
- `scp_requests_total{server,tool}` — requests per server/tool
- `scp_request_duration_seconds{server}` — request latency
- `scp_tokens_received_total{server}` — raw tokens before filtering
- `scp_tokens_delivered_total{server}` — tokens after filtering
- `scp_tokens_saved_total{server}` — tokens filtered out
- `scp_filter_ratio{server}` — delivered/received ratio
- `scp_tool_index_size` — total tools in index
- `scp_tool_index_rebuild_total` — index rebuilds
- `scp_pool_connections_active{server}` — active connections per server
- `scp_pool_queue_depth{server}` — pending requests in pool queue

---

## Architecture

SCP is organized as a Rust Cargo workspace with 7 crates:

- **scp-core** — Protocol types, session management, budget tracking, configuration
- **scp-transport** — stdio, SSE, Streamable HTTP transports
- **scp-pool** — Server lifecycle management, sharing strategies (shared/pooled/dedicated)
- **scp-index** — Tool registry, relevance scoring (tags/TF-IDF/embedding)
- **scp-filter** — 8-stage filter pipeline implementation
- **scp-hub** — Main binary, request router, admin API server
- **scp-cli** — CLI commands for hub management

### Data Flow

1. **Client connects** → Listener accepts connection (stdio/SSE/HTTP)
2. **Session created** → SessionManager allocates budget, request ID map
3. **Request arrives** → Router dispatches to backend server
4. **Response received** → Filter pipeline processes (8 stages)
5. **Response sent** → Delivery logger records, budget updated
6. **Client disconnects** → Session cleaned up

---

## Token Reduction Features

SCP includes a suite of opt-in features (all disabled by default) that reduce the token footprint of `tools/list` responses, filter-pipeline output, and internal serialization overhead. Enable only what you need — each feature can be toggled independently in `scp.toml`.

**Quick reference:**

| ID | Name | Config key | Scope |
|----|------|-----------|-------|
| TR-1 | Tool Cache | `tool_cache_ttl_secs` | `[hub]` |
| TR-2 | Strip Input Schema | `strip_input_schema` | `[hub.defaults.exposure]` |
| TR-3 | scp_schema tool | auto-enabled with TR-2 | built-in |
| TR-4 | Max Description Chars | `max_description_chars` | `[hub.defaults.exposure]` |
| TR-5 | Chunk Usage Tracking | — | `scp_budget` response |
| TR-6 | Response Field Strip | `response_field_strip` | `[[servers]]` |
| TR-7 | tools/list Hash Cache | automatic | session-scoped |
| TR-8 | Schema Deduplication | `deduplicate_identical_schemas` | `[hub.defaults.exposure]` |
| TR-9 | Sentence-level Chunking | automatic | filter pipeline |
| TR-10 | Tool Catalog Injection | `inject_tool_catalog` | `[hub.defaults.exposure]` |

---

### TR-1: Tool Cache

`tools/list` responses from each backend are cached per-server with a configurable TTL. On a cache hit SCP returns the cached list immediately, skipping the full backend fanout. This is especially valuable with many slow backends or high-frequency reconnects.

Configured in `[hub]`:

```toml
[hub]
tool_cache_ttl_secs = 300   # default; set to 0 to disable caching
```

The cache is invalidated on config hot-reload or when a backend reconnects.

---

### TR-2: Strip Input Schema

When enabled, the full `inputSchema` JSON object for every tool is replaced with a minimal placeholder `{"type":"object","properties":{}}` in the `tools/list` wire response. The full schema is **not** discarded — it is kept in the internal tool registry and used for routing, validation, and the `scp_schema` companion tool (TR-3).

Typical saving: **1–5 KB per tool**, significant when a session lists dozens of tools.

Configured in `[hub.defaults.exposure]`:

```toml
[hub.defaults.exposure]
strip_input_schema = true
```

---

### TR-3: scp_schema Tool

A built-in extension tool that lets the model retrieve a tool's full `inputSchema` on demand. This is the companion to TR-2: strip the schemas from `tools/list` to save tokens on every list response, then call `scp_schema` only when the model is actually about to invoke a tool and needs the full parameter spec.

**Usage** — call with a single argument:

```json
{ "tool_name": "<qualified_tool_name>" }
```

`scp_schema` is automatically available whenever `strip_input_schema = true`. It also works independently when schemas are not stripped (useful for inspecting schemas without re-listing all tools).

---

### TR-4: Max Description Chars

Truncates tool descriptions in the `tools/list` wire response to at most N characters, appending `…` if the description was shortened. The full description is kept in the internal registry and is unaffected for routing or scoring purposes.

Configured in `[hub.defaults.exposure]`:

```toml
[hub.defaults.exposure]
max_description_chars = 150   # omit or set to null to disable
```

Set to a lower value (e.g. `80`) for aggressive savings, or omit the key entirely to keep full descriptions.

---

### TR-5: Chunk Usage Tracking

The `scp_budget` built-in tool now returns two additional counters alongside the token budget information:

- `chunks_stored` — total chunks written to the session chunk store since the last reset
- `chunks_fetched` — total chunks retrieved (via `scp_get_more` or filter delivery)

No configuration required. Example response:

```json
{
  "budget_total": 64000,
  "budget_remaining": 51200,
  "strategy": "top_k",
  "chunks_stored": 412,
  "chunks_fetched": 38
}
```

---

### TR-6: Response Field Strip

A per-server list of dot-separated JSON field paths to strip from backend responses **before** the filter pipeline runs. Useful for backends that embed large verbose metadata blocks (e.g. Kubernetes `managedFields`, Docker `Labels`) that are rarely needed by the model.

Paths are removed from every JSON response from that server. Nested paths are supported using dot notation.

Configured per server in `[[servers]]`:

```toml
[[servers]]
name = "kubernetes"
transport = "sse"
url = "http://kube-mcp:8080/sse"
response_field_strip = [
  "metadata.managedFields",
  "metadata.annotations",
  "status.conditions",
]
```

---

### TR-7: tools/list Hash Cache

A session-scoped hash cache for `tools/list` serialization. After the first `tools/list` request in a session, SCP records a hash of the tool registry state. If the registry has not changed by the time the next `tools/list` arrives in the **same session**, SCP returns the previously serialized response bytes directly — skipping JSON serialization entirely.

This feature is automatic and requires no configuration. The cache is invalidated whenever the tool registry changes (backend reconnect, hot-reload, tool discovery cycle).

---

### TR-8: Schema Deduplication

When multiple backends expose a tool with the same `original_name` **and** identical `inputSchema`, only one copy is included in `tools/list` (the copy from the highest-priority server wins). Duplicate entries are silently filtered from the wire response.

Duplicates remain fully callable via their qualified name (e.g. `servername__toolname`) — deduplication only affects what the model sees in the tool list.

Configured in `[hub.defaults.exposure]`:

```toml
[hub.defaults.exposure]
deduplicate_identical_schemas = true
```

---

### TR-9: Sentence-level Chunking

The filter pipeline's chunk splitter (stage 4) now performs **content-aware splitting**. When the splitter detects prose text — identified by the presence of `. `, `? `, or `! ` patterns — it splits on sentence boundaries instead of paragraph boundaries. This produces finer-grained chunks, improving relevance scoring accuracy and allowing the budget enforcer to deliver more precisely targeted content.

This behaviour is automatic and requires no configuration. Non-prose content (JSON, code, logs) continues to use the existing paragraph/line splitters.

---

### TR-10: Tool Catalog Injection

When enabled, SCP injects a compact Markdown-formatted tool catalog into the `instructions` field of the MCP `initialize` response (sent once per session, at connection time). The catalog lists every tool available in the session with a one-line description, allowing the model to orient itself without issuing a `tools/list` call that would consume token budget.

The injected catalog is intentionally compact: tool names plus truncated descriptions only. Full schemas and descriptions remain available via `tools/list` or `scp_schema`.

Configured in `[hub.defaults.exposure]`:

```toml
[hub.defaults.exposure]
inject_tool_catalog = true
```

---

### Combined Example

A production-hardened configuration combining all exposure-level features:

```toml
[hub]
tool_cache_ttl_secs = 300

[hub.defaults.exposure]
strip_input_schema           = true
max_description_chars        = 150
deduplicate_identical_schemas = true
inject_tool_catalog          = true

[[servers]]
name = "kubernetes"
transport = "sse"
url = "http://kube-mcp:8080/sse"
response_field_strip = [
  "metadata.managedFields",
  "metadata.annotations",
]

[[servers]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"]
```

With this configuration a typical session sees:

- `tools/list` payload reduced by ~60–80% (schema stripping + description truncation + deduplication)
- Kubernetes tool responses stripped of verbose metadata before filtering
- The model receives a tool catalog at session start with zero token-budget cost
- Repeated `tools/list` calls within a session served from the in-process hash cache

---

## Changelog

### v0.2.0 (2026-05-25)
- Fix: HTTP backend connections now use correct default timeouts (10s connect / 30s request) when `[servers.timeouts]` is absent from config
- Fix: SSE responses are now streamed line-by-line instead of buffering the full stream (prevents timeouts on long-lived SSE connections)
- Fix: `[servers.environment]` is now accepted as an alias for `[servers.env]` in server config
- Fix: Eliminated 50GB RAM spike caused by unbound chunk cache — sessions now cap cached content at 10MB
- Fix: Transient sessions for headerless clients are cleaned up immediately after request completes
- Feat: Periodic tool re-discovery every 60s heals startup races and backend restarts
- Feat: Full error chain logged on backend connection failures

---

## Contributing

SCP is open source and welcomes contributions. For architecture details and implementation guidance, see:

- **[AGENT.md](./AGENT.md)** — detailed implementation guide for developers
- **[plan.md](./plan.md)** — full specification and design decisions

Before starting work, please:
1. Read AGENT.md to understand the architecture
2. Check the [Issues](https://github.com/eliasstepanik/scp/issues) for ongoing work
3. Open an issue to discuss your contribution
4. Follow the development workflow in AGENT.md (cargo fmt, cargo clippy, tests)

---

## License

SCP is dual-licensed under MIT and Apache 2.0. You may use it under either license.

```
SPDX-License-Identifier: MIT OR Apache-2.0
```

See [LICENSE-MIT](./LICENSE-MIT) and [LICENSE-APACHE](./LICENSE-APACHE) for details.

---

## References

- **[Model Context Protocol](https://modelcontextprotocol.io/)** — official MCP specification
- **[AGENT.md](./AGENT.md)** — implementation guide
- **[plan.md](./plan.md)** — full specification

---

**Made with ❤️ by [Elias Stepanik](https://github.com/eliasstepanik)**
