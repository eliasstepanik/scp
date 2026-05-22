# SCP — Selective Context Protocol

[![Build Status](https://github.com/eliasstepanik/scp/actions/workflows/ci.yml/badge.svg)](https://github.com/eliasstepanik/scp/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/eliasstepanik/scp#license)
[![Version](https://img.shields.io/badge/version-0.1.0--dev-orange.svg)](https://github.com/eliasstepanik/scp/releases)

**An MCP-compatible proxy hub that treats context as a budget, not a dump.**

SCP sits between your LLM client and MCP servers, intelligently filtering tool responses to stay within your context window. Clients connect to SCP as if it's a normal MCP server — zero changes required.

---

## What is SCP?

### The Problem

Modern LLM applications use the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) to access tools and data. But MCP has no concept of relevance or cost:

- **Token waste:** A `filesystem/read_file` returning a 50KB log file consumes your entire context budget on content you may need three lines from.
- **Attention dilution:** Models perform worse when irrelevant content competes for attention.
- **Tool overload:** With 100+ servers exposing 500+ tools, the tool list itself becomes a context burden.
- **No multiplexing:** Each client-server pair is a 1:1 connection. Ten clients using the same filesystem server means ten processes.
- **No cross-source coordination:** When a model calls three tools in sequence, each response is independently sized with no awareness of what came before.

### The Solution

SCP is a transparent proxy that:

1. **Sits in the middle** — clients connect to SCP, SCP connects to backend MCP servers
2. **Measures context** — every response is tokenized and budgeted
3. **Filters intelligently** — responses are scored by relevance and truncated/summarized to fit your budget
4. **Multiplexes servers** — one SCP instance connects to many backends, sharing connections efficiently
5. **Stays transparent** — clients need zero changes; non-SCP-aware clients work as-is

**Core principle:** Context is a budget, not a dump. Every token that reaches the model must earn its place through measured relevance.

---

## How It Works

```
LLM Client → SCP Hub (port 3100) → [Filter Pipeline] → Backend MCP Servers
                    ↕
             Admin API (port 3101)
```

### Key Mechanisms

**1. Token Budget**
Every session gets a token budget (default 64,000 tokens). Responses are filtered to stay within it. Budget can be allocated per-request, per-turn, or via a sliding window.

**2. Tool Index**
Instead of exposing all 500+ tools, SCP returns only the top-N most relevant tools (default 20). Relevance is scored using:
- Tag similarity (server tags vs. session context)
- TF-IDF (tool descriptions vs. session keywords)
- Embeddings (semantic similarity via Ollama)
- Usage frequency (Bayesian scoring)

**3. Filter Pipeline**
An 8-stage pipeline processes every response:
1. **Content Type Router** — classify text, JSON, images, binary
2. **Token Measurement** — count tokens; short-circuit if under budget
3. **Dedup Check** — drop content already delivered in this session
4. **Chunk Splitter** — split large text into paragraphs/lines/JSON elements
5. **Relevance Scorer** — score chunks against session context
6. **Budget Enforcer** — select top-k chunks that fit budget (truncate/summarize/hybrid)
7. **Progressive Disclosure** — append metadata if chunks were dropped
8. **Delivery Logger** — record what was sent to prevent re-delivery

**4. Multiplexing**
One SCP instance connects to multiple MCP servers simultaneously. Sharing strategies:
- **Shared:** one connection, serialized requests (low overhead)
- **Pooled:** N instances, least-outstanding-requests dispatch (balanced load)
- **Dedicated:** per-session instances (full isolation)

---

## Features

- ✅ **Transparent MCP proxy** — clients need zero changes
- ✅ **Token budget management** — global → session → request hierarchy
- ✅ **Intelligent tool filtering** — tag, TF-IDF, embedding scoring
- ✅ **Multi-server multiplexing** — fan-out with timeout handling
- ✅ **Three sharing strategies** — shared, pooled, dedicated
- ✅ **Progressive disclosure** — `scp_get_more` for truncated content
- ✅ **Optional summarization** — via local LLM (Ollama qwen3:1.7b)
- ✅ **Hot-reloadable config** — SIGHUP or Admin API
- ✅ **Prometheus metrics** — `scp_tokens_saved_total` is the primary value metric
- ✅ **Admin API** — runtime server management
- ✅ **SCP-aware extensions** — intent hints, budget control, model declaration

---

## Status

**Pre-release. In active development.**

SCP follows a 7-phase roadmap. Current status: **Phase 0 not yet started**.

| Phase | Version | Focus | Status |
|-------|---------|-------|--------|
| 0 | v0.1 | stdio passthrough, JSON-RPC plumbing | 🔴 Not started |
| 1 | v0.2 | SSE, config, server pool, tool registry, admin API | 🔴 Not started |
| 2 | v0.3 | Streamable HTTP, session store, auth | 🔴 Not started |
| 3 | v0.4 | Scored tool index (TF-IDF, tags) | 🔴 Not started |
| 4 | v0.5 | Full filter pipeline | 🔴 Not started |
| 5 | v0.6 | Embedding scorer, progressive disclosure | 🔴 Not started |
| 6 | v0.7 | Full admin API, Prometheus, OTLP tracing | 🔴 Not started |
| 7 | v1.0 | Production hardening, docs | 🔴 Not started |

---

## Quick Start (Placeholder)

Once Phase 0 is complete, usage will look like this:

```toml
# scp.toml
[hub]
listen_address = "127.0.0.1"
listen_port = 3100

[hub.defaults]
token_budget_per_request = 4000
token_budget_per_session = 64000
max_tools_exposed = 20

[[servers]]
name = "filesystem"
transport = "stdio"
command = ["mcp-server-filesystem", "/home/user"]
sharing = "pooled"
pool_size = 3

[[servers]]
name = "chromadb"
transport = "sse"
url = "http://localhost:8000/sse"
sharing = "shared"

[[servers]]
name = "notion"
transport = "streamable_http"
url = "https://mcp.notion.com/mcp"
sharing = "shared"
```

```bash
# Start the hub
scp start --config scp.toml

# In another terminal, check status
scp servers list
scp sessions list
scp tools search "filesystem"
```

---

## Configuration

SCP uses TOML for configuration. Key sections:

### Hub
```toml
[hub]
listen_address = "127.0.0.1"
listen_port = 3100
transports = ["stdio", "sse", "streamable_http"]
max_clients = 50
session_timeout_secs = 3600

[hub.defaults]
token_budget_per_request = 4000
token_budget_per_session = 64000
budget_strategy = "per_request"  # per_request | per_turn | sliding_window | manual
max_tools_exposed = 20
fanout_timeout_ms = 5000
rate_limit_per_minute = 100
rate_limit_burst = 20
```

### Tool Index
```toml
[tool_index]
engine = "tfidf"  # "none" | "tags" | "tfidf" | "embedding"
max_tools_per_list = 20
always_include = ["filesystem.read_file", "chromadb.search"]
```

### Filter Pipeline
```toml
[filter]
enabled = true
short_circuit_below_tokens = 500

[filter.chunking]
strategy = "paragraph"  # "paragraph" | "line" | "json_element" | "fixed_size"

[filter.budget]
strategy = "truncate"  # "truncate" | "summarize" | "hybrid"
min_tokens_per_response = 200
```

### Servers
```toml
[[servers]]
name = "filesystem"
transport = "stdio"
command = ["mcp-server-filesystem", "/home/user"]
sharing = "pooled"
pool_size = 3
priority = "medium"
tags = ["files", "code", "read", "write"]
idle_timeout_secs = 120
request_timeout_secs = 30
max_retries = 3
```

See `config.example.toml` in the repository for a complete example.

---

## Architecture

SCP is organized as a Rust Cargo workspace:

```
scp/
├── scp-core/       # Protocol types, session, budget, config
├── scp-transport/  # stdio, SSE, Streamable HTTP transports
├── scp-pool/       # Server lifecycle, sharing strategies
├── scp-index/      # Tool registry, relevance scoring
├── scp-filter/     # 8-stage filter pipeline
├── scp-hub/        # Main binary, router, admin API
├── scp-cli/        # CLI commands
└── tests/          # Integration tests
```

### Data Flow

1. **Client connects** → Listener accepts connection (stdio/SSE/HTTP)
2. **Session created** → SessionManager allocates budget, request ID map
3. **Request arrives** → Router dispatches to backend server
4. **Response received** → Filter pipeline processes (8 stages)
5. **Response sent** → Delivery logger records, budget updated
6. **Client disconnects** → Session cleaned up

### Key Components

- **Listener** — accepts client connections (stdio, SSE, Streamable HTTP)
- **Session Manager** — per-client isolation, budget tracking, request ID mapping
- **Router** — dispatches requests to backend servers, handles fan-out
- **Filter Pipeline** — 8-stage processing for intelligent context selection
- **Tool Index** — merged registry of all tools, scored by relevance
- **Pool Manager** — manages backend connections (shared, pooled, dedicated)
- **Admin API** — HTTP server for runtime management

---

## SCP Extensions (for SCP-Aware Clients)

SCP-aware clients can opt into advanced features:

### Intent Hints
```json
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
```

### Budget Control
```json
{ "method": "tools/call", "params": { "name": "scp_budget", "arguments": {} } }
// Returns: { "remaining": 3200, "total": 4000, "strategy": "per_request" }

{ "method": "tools/call", "params": { "name": "scp_budget_reset", "arguments": {} } }
```

### Progressive Disclosure
```json
{ "method": "tools/call", "params": { "name": "scp_get_more", "arguments": { "request_id": "scp-a3f9-001", "offset": 15, "limit": 15 } } }
```

### Model Declaration
```json
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
```

### Extension Discovery
```json
{ "method": "tools/call", "params": { "name": "scp_info", "arguments": {} } }
// Returns: { "version": "0.3.0", "extensions": ["intent_hints", "budget_control", "progressive_disclosure"], "servers": 42, "tools": 187 }
```

---

## Deployment

### Docker

```bash
docker run -p 3100:3100 -p 3101:3101 \
  -v ./scp.toml:/etc/scp/scp.toml \
  -v ./auth_tokens.json:/etc/scp/auth_tokens.json \
  ghcr.io/eliasstepanik/scp-hub:latest
```

### Binary

SCP is distributed as a single static binary, cross-compiled for:
- Linux (amd64, arm64)
- macOS (amd64, arm64)
- Windows (amd64)

Download from [Releases](https://github.com/eliasstepanik/scp/releases).

### Systemd Service

```ini
[Unit]
Description=SCP Hub
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/scp-hub start --config /etc/scp/scp.toml
Restart=on-failure
RestartSec=5s
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

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

## CLI

SCP includes a CLI for common operations:

```bash
# Start the hub
scp start --config scp.toml

# Check hub status
scp status

# List servers
scp servers list
scp servers status filesystem

# List active sessions
scp sessions list
scp sessions info <session-id>

# Search tools
scp tools list
scp tools search "filesystem"

# Reload config
scp config reload
```

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
- **[config.example.toml](./config.example.toml)** — documented example configuration

---

**Made with ❤️ by [Elias Stepanik](https://github.com/eliasstepanik)**
