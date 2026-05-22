# SCP Phase 1 (v0.2) — Task Breakdown

> Generated: 2026-05-22  
> Status: All tasks = `planning`  
> Embedding store unavailable (budget exhausted) — this file is the authoritative task record.

---

## P1 — Phase 1 Root

**Title:** Phase 1 (v0.2) — Multi-Server + Basic Filtering  
**Importance:** critical  
**Exit criteria:** SCP proxies 3+ real servers. Tool routing works. Large responses truncated. Servers manageable at runtime.

---

## P1.A — TOML Config + Env Var Interpolation

**Importance:** critical  
**Crate:** `scp-core/src/config.rs`  
**Depends on:** nothing (foundation for all other groups)

### P1.A.1 — Config struct skeleton
- Define `Config`, `HubConfig`, `HubDefaults`, `ServerConfig`, `AdminConfig`, `FilterConfig` structs
- Derive `serde::Deserialize` on all
- Add `config_version: u32` field, validate == 1 on load
- **Acceptance:** `cargo test` passes, structs compile

### P1.A.2 — TOML file loading
- `Config::from_file(path: &Path) -> Result<Config>` using `toml` crate
- Return typed error on parse failure with file + line info
- **Acceptance:** loads the example config from the prompt without error

### P1.A.3 — Env var interpolation
- Pre-process TOML string before parsing: replace `${VAR_NAME}` with `std::env::var("VAR_NAME")`
- Fail-fast with clear error if variable is missing
- Support nested values (command args, URLs, tokens)
- **Acceptance:** unit test: set env var, load config, assert interpolated value

### P1.A.4 — Config validation
- Validate: `listen_port` in 1–65535, `admin.port` != `hub.listen_port`, server names unique, `sharing` is valid enum
- Return `Vec<ConfigError>` (collect all errors, don't stop at first)
- **Acceptance:** unit tests for each validation rule

### P1.A.5 — TransportConfig enum
- `TransportConfig::Stdio { command, args, env }` and `TransportConfig::Sse { url, headers }`
- Deserialize from `transport = "stdio"` / `transport = "sse"` TOML field
- **Acceptance:** both variants parse correctly from TOML

### P1.A.6 — SharingStrategy enum
- `SharingStrategy::Shared | Pooled { pool_size } | Dedicated`
- Deserialize from `sharing = "shared"` / `"pooled"` / `"dedicated"`
- **Acceptance:** all three variants parse

---

## P1.B — Config-Driven Server Startup

**Importance:** critical  
**Crate:** `scp-hub/src/main.rs`, `scp-hub/src/hub.rs`  
**Depends on:** P1.A

### P1.B.1 — CLI `scp start --config <path>`
- Parse `--config` flag (clap), default to `./config.toml`
- Load and validate config on startup, exit with error if invalid
- **Acceptance:** `scp start --config missing.toml` exits with non-zero + message

### P1.B.2 — Hub initialization from config
- `Hub::from_config(config: Config)` — replaces hardcoded single-server setup
- Iterate `config.servers`, create a `ServerEntry` per server
- **Acceptance:** hub starts with 0 servers (empty config) without panic

### P1.B.3 — Multi-server startup
- For each enabled server in config, call `PoolManager::add_server()`
- Log server name, transport, sharing strategy on startup
- **Acceptance:** integration test: config with 2 stdio servers, both start

---

## P1.C — SharedPool + PoolManager + Lifecycle

**Importance:** critical  
**Crate:** `scp-pool/`  
**Depends on:** P1.A, P1.B

### P1.C.1 — Lifecycle state machine
- `LifecycleState` enum: `Cold | Starting | Warm | Hot | Draining | Disabled | Failed`
- `ServerState` struct: holds `LifecycleState`, failure count, last_ping, last_error
- Transitions: `cold→starting→warm→hot`, `hot→warm`, `warm→draining→cold`, `*→failed`, `*→disabled`
- **Acceptance:** unit tests for all valid transitions; invalid transitions return `Err`

### P1.C.2 — SharedPool struct
- `SharedPool`: wraps a single `StdioTransport` (or SSE), serializes requests via `tokio::Mutex`
- `SharedPool::send(request) -> Result<Response>` — acquires lock, sends, awaits response, releases
- Request ID remapping: generate internal `scp-{uuid}-{seq}` ID, map back on response
- **Acceptance:** unit test: 3 concurrent callers, all get correct responses in order

### P1.C.3 — PoolManager
- `PoolManager`: `Arc<RwLock<HashMap<String, Arc<SharedPool>>>>` (server name → pool)
- `add_server(config: &ServerConfig) -> Result<()>` — creates pool, starts connection
- `remove_server(name: &str) -> Result<()>` — drains, removes
- `get_pool(name: &str) -> Option<Arc<SharedPool>>`
- **Acceptance:** unit test: add 2 servers, get both, remove one, get returns None

### P1.C.4 — Health checker
- Background tokio task: ping each server every 30s via `ping` MCP method
- Track consecutive failures: 3 → `degraded` log, 5 → `LifecycleState::Failed`
- On failure: log with server name + failure count
- **Acceptance:** unit test with mock server that stops responding; verify state → Failed after 5 pings

### P1.C.5 — SSE transport (server-facing)
- `SseTransport`: connects to `url` (SSE endpoint), sends JSON-RPC via HTTP POST, receives via SSE stream
- Implement same `Transport` trait as `StdioTransport`
- Handle reconnect on disconnect
- **Acceptance:** integration test: mock SSE server, send request, receive response

---

## P1.D — ToolRegistry + Collision/Alias

**Importance:** high  
**Crate:** `scp-index/src/registry.rs`  
**Depends on:** P1.C

### P1.D.1 — ToolRegistry struct
- `ToolRegistry`: `HashMap<String, ToolEntry>` where key = qualified name (`server.tool`)
- `ToolEntry`: `{ server_name, original_name, description, input_schema, tags }`
- **Acceptance:** struct compiles, basic insert/lookup works

### P1.D.2 — Register tools from server
- `register_server_tools(server_name: &str, tools: Vec<Tool>) -> Vec<CollisionEvent>`
- Qualified name = `{server_name}.{tool_name}`
- Detect collision: same unqualified name from 2+ servers → log warning, use qualified names for both
- **Acceptance:** unit test: 2 servers both have `search` tool → both registered as `server1.search`, `server2.search`

### P1.D.3 — Alias resolution
- `resolve(name: &str) -> Option<&ToolEntry>` — accepts both qualified and unqualified names
- Unqualified: returns entry only if exactly one server has that tool (no collision)
- Qualified: direct lookup
- **Acceptance:** unit test: unqualified with collision returns None; unqualified without collision returns entry

### P1.D.4 — Name stripping on forward
- `strip_prefix(qualified_name: &str) -> &str` — returns original tool name
- Used by Router before forwarding `tools/call` to backend
- **Acceptance:** `"chromadb.search"` → `"search"`, `"search"` → `"search"`

### P1.D.5 — Deregister server tools
- `deregister_server(server_name: &str)` — removes all tools for that server
- **Acceptance:** unit test: register, deregister, lookup returns None

---

## P1.E — Router (tools/list fan-out + tools/call routing)

**Importance:** critical  
**Crate:** `scp-hub/src/router.rs`  
**Depends on:** P1.C, P1.D

### P1.E.1 — Router struct
- `Router`: holds `Arc<PoolManager>`, `Arc<ToolRegistry>`
- `route(request: JsonRpcRequest) -> Result<JsonRpcResponse>`
- **Acceptance:** compiles, basic dispatch works

### P1.E.2 — tools/call routing
- Look up tool name in `ToolRegistry` (qualified or unqualified)
- Strip prefix, forward to owning server's pool
- Return MCP error `-32602` if tool not found
- **Acceptance:** unit test: call `"filesystem.read_file"` → forwarded as `"read_file"` to filesystem server

### P1.E.3 — tools/list fan-out
- Send `tools/list` to all healthy servers concurrently (tokio::join_all)
- Timeout per server: `fanout_timeout_secs` (default 5s)
- Merge results: qualify all tool names, deduplicate
- Slow/failed servers: log warning, exclude from result (don't block)
- **Acceptance:** integration test: 2 servers, one slow (6s), fan-out returns tools from fast server within 5s

### P1.E.4 — ping passthrough
- `ping` → respond directly (no backend needed)
- **Acceptance:** ping returns `{}` result immediately

### P1.E.5 — initialize passthrough
- Forward `initialize` to all servers, merge capabilities
- Return merged `InitializeResult` to client
- **Acceptance:** unit test: 2 servers with different capabilities → merged result

---

## P1.F — Token Counter + Budget Truncation

**Importance:** high  
**Crate:** `scp-filter/src/token_count.rs`, `scp-filter/src/budget.rs`  
**Depends on:** P1.A (config for budget values)

### P1.F.1 — TokenEstimator trait + heuristic impl
- `trait TokenEstimator { fn estimate(&self, text: &str) -> usize; }`
- `HeuristicEstimator`: count non-ASCII bytes; if > 20% → bytes/2.5, else → bytes/3.5
- **Acceptance:** unit tests: ASCII text, CJK text, mixed text — verify estimates within 10% of expected

### P1.F.2 — Response token measurement
- `measure_response(response: &JsonRpcResponse) -> usize` — serialize result to JSON string, estimate tokens
- **Acceptance:** unit test with known-size response

### P1.F.3 — Hard budget truncation
- `truncate_to_budget(text: &str, budget: usize, estimator: &dyn TokenEstimator) -> String`
- Binary search or linear scan to find cut point
- Append `"..."` after truncation
- Minimum: always deliver at least 200 tokens even if over budget
- **Acceptance:** unit test: 1000-token text, 300-token budget → truncated with `"..."`, estimate ≤ 300

### P1.F.4 — Budget config wiring
- Read `request_token_budget` and `session_token_budget` from `HubDefaults`
- Apply truncation in Router after receiving backend response
- **Acceptance:** integration test: server returns 2000-token response, budget=500 → response truncated

---

## P1.G — ServerManager (runtime add/remove/disable/enable)

**Importance:** high  
**Crate:** `scp-hub/src/server_manager.rs`  
**Depends on:** P1.C, P1.D, P1.E

### P1.G.1 — ServerManager trait
```rust
#[async_trait]
pub trait ServerManager: Send + Sync {
    async fn add_server(&self, config: ServerConfig) -> Result<()>;
    async fn remove_server(&self, name: &str) -> Result<()>;
    async fn disable_server(&self, name: &str) -> Result<()>;
    async fn enable_server(&self, name: &str) -> Result<()>;
    async fn list_servers(&self) -> Vec<ServerStatus>;
}
```
- **Acceptance:** trait compiles, mock impl works

### P1.G.2 — add_server implementation
- Validate config (name unique, transport valid)
- Add to PoolManager, register tools in ToolRegistry
- Set state to `Starting`, connect, transition to `Warm`
- **Acceptance:** integration test: add server at runtime, tools appear in tools/list

### P1.G.3 — remove_server implementation
- Drain in-flight requests (wait up to 10s)
- Deregister tools from ToolRegistry
- Remove from PoolManager
- **Acceptance:** integration test: remove server, tools disappear from tools/list

### P1.G.4 — disable_server / enable_server
- `disable`: set state to `Disabled`, hide tools from tools/list (keep connection)
- `enable`: set state to `Warm`, re-expose tools
- **Acceptance:** unit test: disable → tools/list excludes server; enable → tools/list includes server

### P1.G.5 — ServerStatus struct
- `ServerStatus { name, state: LifecycleState, tool_count, last_ping, error_count }`
- `list_servers()` returns Vec of these
- **Acceptance:** list_servers returns correct data after add/disable/enable

---

## P1.H — Admin API (axum, port 3101)

**Importance:** high  
**Crate:** `scp-hub/src/admin.rs`  
**Depends on:** P1.G

### P1.H.1 — Axum server setup
- Start axum HTTP server on `admin.port` (default 3101) in background tokio task
- Bind to `127.0.0.1` only
- **Acceptance:** server starts, `GET /health` returns 200

### P1.H.2 — GET /health
- Returns `{ "status": "ok", "servers": N, "healthy": M }`
- **Acceptance:** curl test

### P1.H.3 — GET /servers
- Returns JSON array of `ServerStatus`
- **Acceptance:** returns correct list after adding servers

### P1.H.4 — POST /servers
- Body: `ServerConfig` JSON
- Calls `ServerManager::add_server()`
- Returns 201 on success, 409 if name exists, 400 on validation error
- **Acceptance:** integration test: POST → server appears in GET /servers

### P1.H.5 — PUT /servers/{name}
- Update server config (only `enabled`, `tags`, `priority` mutable at runtime)
- Returns 200 on success, 404 if not found
- **Acceptance:** unit test

### P1.H.6 — DELETE /servers/{name}
- Calls `ServerManager::remove_server()`
- Returns 204 on success, 404 if not found
- **Acceptance:** integration test: DELETE → server gone from GET /servers

### P1.H.7 — POST /servers/{name}/disable
- Calls `ServerManager::disable_server()`
- Returns 200, 404
- **Acceptance:** unit test

### P1.H.8 — POST /servers/{name}/enable
- Calls `ServerManager::enable_server()`
- Returns 200, 404
- **Acceptance:** unit test

### P1.H.9 — POST /config/reload
- Trigger config hot-reload (see P1.I)
- Returns 200 on success, 409 if reload in progress, 422 on validation error
- **Acceptance:** integration test: modify config file, POST → new server appears

---

## P1.I — SIGHUP Hot-Reload

**Importance:** medium  
**Crate:** `scp-hub/src/reload.rs`  
**Depends on:** P1.A, P1.G, P1.H

### P1.I.1 — SIGHUP signal handler
- Register `tokio::signal::unix::signal(SignalKind::hangup())` handler
- On SIGHUP: trigger reload pipeline
- Windows: skip SIGHUP, only POST /config/reload works
- **Acceptance:** `kill -HUP <pid>` triggers reload on Linux/macOS

### P1.I.2 — Config diff engine
- `diff_configs(old: &Config, new: &Config) -> ConfigDiff`
- `ConfigDiff { added: Vec<ServerConfig>, removed: Vec<String>, updated: Vec<ServerConfig>, hub_changed: bool }`
- **Acceptance:** unit tests for add/remove/update detection

### P1.I.3 — Reload apply sequence
- Sequence: read → validate → diff → apply (remove → disable → update → add → enable) → rebuild indexes → log
- Use `tokio::Mutex` to prevent concurrent reloads (return 409 if locked)
- **Acceptance:** integration test: add server to config file, trigger reload, server appears

### P1.I.4 — Reload error handling
- If new config is invalid: log error, keep old config running
- If apply partially fails: log which servers failed, continue with rest
- **Acceptance:** unit test: invalid config → reload fails, old config still active

---

## P1.J — CLI (scp-cli crate)

**Importance:** medium  
**Crate:** `scp-cli/`  
**Depends on:** P1.H (calls Admin API)

### P1.J.1 — CLI crate setup
- New binary crate `scp-cli` in workspace
- `clap` for argument parsing
- **Acceptance:** `cargo build -p scp-cli` succeeds

### P1.J.2 — `scp start`
- `scp start [--config <path>] [--log-level <level>]`
- Starts the hub (calls into scp-hub library)
- **Acceptance:** `scp start --config config.toml` starts hub

### P1.J.3 — `scp status`
- Calls `GET /health` on admin API
- Prints: hub status, server count, healthy count
- **Acceptance:** output matches admin API response

### P1.J.4 — `scp servers list`
- Calls `GET /servers`
- Prints table: name | state | tools | last_ping | errors
- **Acceptance:** formatted table output

### P1.J.5 — `scp servers add <name> --transport stdio --command <cmd> [--args ...]`
- Calls `POST /servers`
- **Acceptance:** server appears in `scp servers list`

### P1.J.6 — `scp servers remove <name>`
- Calls `DELETE /servers/{name}`
- **Acceptance:** server gone from list

### P1.J.7 — `scp servers disable <name>` / `enable <name>`
- Calls `POST /servers/{name}/disable` / `enable`
- **Acceptance:** state changes in list

### P1.J.8 — `scp reload`
- Calls `POST /config/reload`
- Prints success or error message
- **Acceptance:** triggers reload

---

## P1.K — Integration Tests

**Importance:** high  
**Location:** `tests/`  
**Depends on:** all above groups

### P1.K.1 — Test infrastructure: mock MCP server
- `tests/common/mock_server.rs`: in-process mock MCP server
- Configurable: tool list, response delay, failure injection
- **Acceptance:** used by other tests without flakiness

### P1.K.2 — multi_server_test.rs
- Start hub with 2 mock servers
- Verify `tools/list` returns tools from both servers (qualified names)
- Verify `tools/call` routes to correct server
- Verify fan-out timeout: one slow server doesn't block
- **Acceptance:** all assertions pass, test < 10s

### P1.K.3 — server_lifecycle_test.rs
- Start hub, add server at runtime, verify tools appear
- Disable server, verify tools hidden
- Enable server, verify tools reappear
- Remove server, verify tools gone
- **Acceptance:** all state transitions verified

### P1.K.4 — config_reload_test.rs
- Start hub with config A (1 server)
- Modify config to add server B
- Trigger reload (POST /config/reload)
- Verify server B appears in tools/list
- **Acceptance:** reload works without restart

### P1.K.5 — budget_truncation_test.rs
- Configure budget = 200 tokens
- Mock server returns 2000-token response
- Verify response is truncated to ≤ 200 tokens + "..."
- **Acceptance:** truncation verified

### P1.K.6 — sse_transport_test.rs
- Start mock SSE server
- Connect SCP to it
- Send tools/list, verify response
- **Acceptance:** SSE transport works end-to-end

---

## Execution Order & Critical Path

### Dependency Graph
```
P1.A (Config) ──────────────────────────────────────────────────────┐
    └─► P1.B (Startup) ──────────────────────────────────────────────┤
            └─► P1.C (Pool/Lifecycle) ──────────────────────────────┤
                    ├─► P1.D (ToolRegistry) ──────────────────────┐  │
                    └─► P1.E (Router) ◄──────────────────────────┘  │
                            └─► P1.F (Token/Budget) ────────────────┤
                                    └─► P1.G (ServerManager) ───────┤
                                            └─► P1.H (Admin API) ───┤
                                                    └─► P1.I (Reload)│
                                                    └─► P1.J (CLI) ──┤
                                                                      │
P1.K (Tests) ◄────────────────────────────────────────────────────────┘
```

### Recommended Build Sequence

**Sprint 1 (foundation):** P1.A → P1.B → P1.C.1 + P1.C.2 + P1.C.3
**Sprint 2 (routing):** P1.C.4 + P1.C.5 → P1.D → P1.E
**Sprint 3 (filtering):** P1.F → P1.G
**Sprint 4 (management):** P1.H → P1.I + P1.J
**Sprint 5 (tests):** P1.K (all)

### Critical Path
P1.A.1 → P1.A.2 → P1.A.3 → P1.B.2 → P1.C.2 → P1.C.3 → P1.D.2 → P1.E.2 → P1.E.3 → P1.F.3 → P1.G.2 → P1.H.4 → P1.K.2

### Parallelism Opportunities
- P1.D and P1.C.4 + P1.C.5 can be built in parallel after P1.C.3
- P1.F can be built in parallel with P1.D
- P1.I and P1.J can be built in parallel after P1.H.1
- All P1.K tests can be written in parallel with their respective feature groups

---

## Task Count Summary

| Group | Tasks | Importance |
|-------|-------|------------|
| P1.A Config | 6 | critical |
| P1.B Startup | 3 | critical |
| P1.C Pool/Lifecycle | 5 | critical |
| P1.D ToolRegistry | 5 | high |
| P1.E Router | 5 | critical |
| P1.F Token/Budget | 4 | high |
| P1.G ServerManager | 5 | high |
| P1.H Admin API | 9 | high |
| P1.I Hot-Reload | 4 | medium |
| P1.J CLI | 8 | medium |
| P1.K Tests | 6 | high |
| **Total** | **60** | |

---

*Note: Task MCP embedding store was unavailable (budget exhausted). This file is the authoritative task record for Phase 1. The orchestrator should use task IDs from this file as logical references.*
