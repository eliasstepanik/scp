# SCP Phase 2 (v0.3) — Task Breakdown

> Generated: 2026-05-22
> Status: All tasks = `planning`
> Embedding store unavailable (budget exhausted) — this file is the authoritative task record.

---

## P2 — Phase 2 Root

**Title:** Phase 2 (v0.3) — Streamable HTTP, Sessions, Pooled/Dedicated Pools
**Importance:** critical
**Exit criteria:** SCP accepts HTTP clients on port 3100 (Streamable HTTP + bearer auth), manages per-session state, supports pooled and dedicated backend pool strategies, propagates cancellation, handles sampling callbacks and roots, and all Phase 0 passthrough tests pass over HTTP.

---

## P2.A — Streamable HTTP Server-Facing Transport

**Importance:** critical
**Crate:** `crates/scp-transport/src/http_server.rs`
**Depends on:** nothing (new module, parallel with P2.B)

Implement the MCP 2025-03-26 Streamable HTTP transport for connecting SCP **to** a backend MCP server that speaks Streamable HTTP. This is the *outbound* (server-facing) side.

### P2.A.1 — Add HTTP dependencies to scp-transport

- Add to `crates/scp-transport/Cargo.toml`:
  - `reqwest = { workspace = true }` (already in workspace)
  - `tokio-stream = "0.1"` (for SSE stream parsing)
  - `eventsource-stream = "0.2"` (SSE client parsing)
  - `futures = "0.3"`
- Add `pub mod http_server;` to `crates/scp-transport/src/lib.rs`
- **Acceptance:** `cargo build -p scp-transport` succeeds with new deps

### P2.A.2 — HttpServerTransport struct

- `HttpServerTransport { url: String, session_id: Option<String>, client: reqwest::Client, headers: HashMap<String, String> }`
- Constructor: `HttpServerTransport::new(url: String, headers: HashMap<String, String>) -> Self`
- Implement the existing `Transport` trait (same interface as `StdioServerTransport`)
- **Acceptance:** struct compiles, trait impl skeleton compiles

### P2.A.3 — POST /mcp request sending

- `send(message: &Value) -> Result<()>`: POST the JSON-RPC message to `{url}/mcp`
- Include `Content-Type: application/json` and `Accept: application/json, text/event-stream`
- If `session_id` is set, include `Mcp-Session-Id: {id}` header
- On 4xx/5xx: return typed error with status code
- **Acceptance:** unit test with `mockito` or `wiremock`: POST sends correct headers and body

### P2.A.4 — SSE stream reception

- `receive() -> Result<Option<IncomingMessage>>`: open GET `{url}/mcp` SSE stream, parse events
- Parse `data:` lines as JSON-RPC messages (responses and notifications)
- Extract `Mcp-Session-Id` from response headers on first connection, store in `self.session_id`
- Handle SSE `event: message` and bare `data:` lines
- **Acceptance:** unit test: mock SSE server sends a response event, `receive()` returns it parsed

### P2.A.5 — DELETE /mcp session teardown

- `close() -> Result<()>`: send DELETE `{url}/mcp` with `Mcp-Session-Id` header
- Log session closure
- **Acceptance:** unit test: DELETE is sent with correct session ID header

### P2.A.6 — Reconnect on disconnect

- If SSE stream drops (EOF or error), attempt reconnect with exponential backoff: 1s, 2s, 4s, 8s, max 30s
- After 5 consecutive failures, return error (caller marks server as Failed)
- **Acceptance:** unit test: mock server closes SSE stream, transport reconnects

---

## P2.B — Streamable HTTP Client-Facing Listener

**Importance:** critical
**Crates:** `crates/scp-transport/src/http_client.rs`, `crates/scp-hub/src/listener.rs`
**Depends on:** P2.C (SessionStore must exist before listener can assign sessions)

Implement the axum HTTP server that accepts MCP clients connecting via Streamable HTTP. This is the *inbound* (client-facing) side.

### P2.B.1 — Add axum SSE dependency

- Add to `crates/scp-transport/Cargo.toml`:
  - `axum = { workspace = true }` (already in workspace)
  - `axum-extra = { version = "0.9", features = ["typed-header"] }`
  - `headers = "0.4"`
- Add `pub mod http_client;` to `crates/scp-transport/src/lib.rs`
- **Acceptance:** `cargo build -p scp-transport` succeeds

### P2.B.2 — ClientListener struct

- `crates/scp-hub/src/listener.rs`
- `ClientListener { addr: SocketAddr, session_store: Arc<SessionStore>, router: Arc<Router>, auth_config: Option<AuthConfig> }`
- `ClientListener::new(config: &HubConfig, session_store: Arc<SessionStore>, router: Arc<Router>) -> Self`
- `ClientListener::run(self) -> Result<()>`: starts axum server, blocks until shutdown
- **Acceptance:** struct compiles, `run()` binds to port without panic

### P2.B.3 — POST /mcp endpoint

- Accept JSON-RPC message from client body
- Extract `Mcp-Session-Id` header (if present) to look up existing session; if absent, create new session
- Validate bearer token (if auth configured) — return 401 if invalid
- Dispatch message to `Router::route(session_id, message)`
- Return response as JSON body (for non-streaming) or 202 Accepted (for notifications)
- **Acceptance:** integration test: POST initialize returns InitializeResult JSON

### P2.B.4 — GET /mcp SSE stream endpoint

- Open SSE stream for server-to-client push
- Associate stream with session (via `Mcp-Session-Id` header or new session)
- Return `Content-Type: text/event-stream`, `Mcp-Session-Id: {id}` response header
- Keep stream open; push messages from session outbound channel as SSE `data:` events
- On session close or client disconnect: clean up
- **Acceptance:** integration test: GET /mcp returns SSE stream, hub can push a notification to it

### P2.B.5 — DELETE /mcp endpoint

- Extract `Mcp-Session-Id` header
- Call `SessionStore::remove(session_id)` — tears down session
- Return 200 OK
- **Acceptance:** integration test: DELETE closes session, subsequent POST returns 404

### P2.B.6 — Bearer token authentication middleware

- `AuthMiddleware`: axum middleware layer
- If `hub.auth.method = "bearer"`: require `Authorization: Bearer {token}` on all endpoints
- Validate token against configured token list (from `hub.auth` config)
- Return 401 with JSON error body on missing/invalid token
- Bypass auth for `GET /health` (admin endpoint, different port)
- **Acceptance:** unit test: missing token returns 401; valid token returns 200; invalid token returns 401

### P2.B.7 — Wire listener into hub main

- `crates/scp-hub/src/main.rs`: after loading config, construct `ClientListener` and spawn it
- Listener runs concurrently with admin API and health checker
- Graceful shutdown: on SIGTERM/SIGINT, stop accepting new connections, wait for in-flight
- **Acceptance:** `scp start --config scp.toml` starts HTTP listener on configured port

---

## P2.C — SessionStore

**Importance:** critical
**Crate:** `crates/scp-hub/src/session_store.rs`
**Depends on:** nothing (pure data structure, no transport deps)

### P2.C.1 — Session struct

Define `Session` with fields:
- `id: SessionId` (UUID v4 string)
- `auth_token: Option<String>`
- `created_at: Instant`, `last_active: Instant`
- `client_capabilities: ClientCapabilities`, `client_info: Implementation`
- `roots: Vec<Root>`
- `token_budget_remaining: usize`
- `tool_scope: Option<Vec<String>>`
- `request_map: IdMap` (per-session, from scp_core::id_map)
- `call_history: VecDeque<ToolCallRecord>` (cap 100)
- `outbound_tx: tokio::sync::mpsc::Sender<Value>`

Also define `ToolCallRecord { tool_name: String, server_name: String, timestamp: Instant, token_cost: usize }`
- **Acceptance:** struct compiles with all fields

### P2.C.2 — SessionStore struct

- `SessionStore { sessions: Arc<RwLock<HashMap<SessionId, Arc<Mutex<Session>>>>> }`
- `create(auth_token: Option<String>, budget: usize) -> (SessionId, Receiver<Value>)`
- `get(id: &SessionId) -> Option<Arc<Mutex<Session>>>`
- `remove(id: &SessionId) -> bool`
- `list() -> Vec<SessionSummary>`
- `SessionSummary { id, created_at, last_active, tool_call_count, budget_remaining }`
- **Acceptance:** unit test: create, get, remove, list all work correctly

### P2.C.3 — Session timeout cleanup

- Background tokio task: every 60 seconds, scan sessions for idle > `session_timeout_secs`
- Remove expired sessions, log with session ID
- `SessionStore::start_cleanup_task(timeout_secs: u64) -> JoinHandle<()>`
- **Acceptance:** unit test: create session, advance mock time past timeout, verify cleanup removes it

### P2.C.4 — Per-session IdMap

- Each `Session` has its own `IdMap` (from `scp_core::id_map`)
- Remove the global `IdMap` from `hub.rs` (it was a single global — now per-session)
- Router must accept `session_id` and look up the session IdMap for remapping
- **Acceptance:** unit test: two sessions with same client request ID (both send id=1) — no collision

### P2.C.5 — Session initialization from MCP initialize

- `SessionStore::initialize_session(id: &SessionId, params: InitializeParams) -> Result<()>`
- Stores `client_capabilities`, `client_info`, `roots` (if provided in params)
- Sets `token_budget_remaining` from config defaults
- **Acceptance:** unit test: after initialize_session, session has correct capabilities stored

---

## P2.D — Pooled + Dedicated Pool Strategies

**Importance:** high
**Crate:** `crates/scp-pool/src/pooled.rs`, `crates/scp-pool/src/dedicated.rs`
**Depends on:** P2.A (HttpServerTransport), P2.C (SessionStore for dedicated)

### P2.D.1 — Pool trait abstraction

- `crates/scp-pool/src/lib.rs`: define `Pool` trait with `send()`, `send_notification()`, `strategy()`
- `PoolStrategy` enum: `Shared | Pooled | Dedicated`
- Update `PoolManager::get_pool()` to return `Arc<dyn Pool>`
- **Acceptance:** trait compiles, existing `SharedPool` implements it

### P2.D.2 — PooledPool struct

- `crates/scp-pool/src/pooled.rs`
- `PooledPool { workers: Vec<Arc<PoolWorker>>, config: ServerConfig }`
- `PoolWorker { transport: Arc<Mutex<dyn Transport>>, in_flight: AtomicUsize }`
- Lazy startup: worker 0 starts on first request; worker N starts when all N-1 are busy
- Dispatch: least-outstanding-requests (pick worker with lowest `in_flight` count)
- Max queue depth: configurable (default 10); overflow returns `PoolError::Backpressure`
- **Acceptance:** unit test: 3 workers, 6 concurrent requests distributed across workers

### P2.D.3 — DedicatedPool struct

- `crates/scp-pool/src/dedicated.rs`
- `DedicatedPool { sessions: Arc<RwLock<HashMap<SessionId, Arc<Mutex<dyn Transport>>>>>, config: ServerConfig }`
- `get_or_create(session_id: &SessionId) -> Arc<Mutex<dyn Transport>>`: spawn new backend if not exists
- `remove_session(session_id: &SessionId)`: tear down backend connection for that session
- **Acceptance:** unit test: two sessions get independent backend connections; removing one does not affect the other

### P2.D.4 — Wire into PoolManager

- `PoolManager::add_server()`: check `config.sharing`:
  - `"shared"` creates `SharedPool` (existing)
  - `"pooled"` creates `PooledPool` with `pool_size` workers
  - `"dedicated"` creates `DedicatedPool`
- Store as `Arc<dyn Pool>` in `ServerEntry`
- **Acceptance:** integration test: config with all three strategies, all start without error

### P2.D.5 — DedicatedPool session teardown on session end

- `SessionStore::remove()` must notify `DedicatedPool` to tear down the session backend connection
- Implement via a `SessionCleanupHook` trait or direct `PoolManager` reference in `SessionStore`
- **Acceptance:** integration test: session ends, dedicated backend process is killed

---

## P2.E — Per-Session Request ID Mapping

**Importance:** high
**Crate:** `crates/scp-hub/src/router.rs`, `crates/scp-hub/src/session_store.rs`
**Depends on:** P2.C

### P2.E.1 — Router accepts session_id parameter

- `Router::route(session_id: &SessionId, request: JsonRpcRequest) -> Result<JsonRpcResponse>`
- Look up session from `SessionStore`, acquire lock, use session IdMap for remapping
- **Acceptance:** compiles; existing routing logic unchanged except IdMap source

### P2.E.2 — ID remapping uses session IdMap

- Before forwarding to backend: `session.request_map.generate(client_id)` returns internal ID
- After receiving backend response: `session.request_map.remove(internal_id)` returns client ID
- **Acceptance:** unit test: two sessions both send request id=42, no cross-session collision

### P2.E.3 — In-flight request tracking per session

- `Session.active_requests: HashMap<InternalId, InFlightRequest>`
- `InFlightRequest { method: String, server_name: String, started_at: Instant }`
- Insert on dispatch, remove on response
- **Acceptance:** unit test: dispatch request, active_requests has entry; receive response, entry removed

---

## P2.F — Cancellation Propagation

**Importance:** high
**Crate:** `crates/scp-hub/src/hub.rs`, `crates/scp-hub/src/router.rs`
**Depends on:** P2.C, P2.E

### P2.F.1 — Handle notifications/cancelled from client

- In the message dispatch loop: detect `notifications/cancelled` method
- Extract `requestId` from params
- Look up session IdMap to find internal ID
- Forward `notifications/cancelled` with internal ID to the backend server that owns the request
- Remove from `session.active_requests`
- **Acceptance:** unit test: client sends cancel for id=5, backend receives cancel with internal ID

### P2.F.2 — Handle notifications/cancelled from backend

- Backend sends `notifications/cancelled` with internal ID
- Look up session IdMap to find client ID
- Forward `notifications/cancelled` with client ID to client SSE stream
- **Acceptance:** unit test: backend sends cancel, client receives cancel with original client ID

### P2.F.3 — Cancellation cleanup

- On cancellation (either direction): remove from `session.active_requests`
- If backend connection is stdio: send SIGTERM to the backend process (optional, best-effort)
- **Acceptance:** unit test: after cancel, active_requests is empty for that request

---

## P2.G — Sampling Routing

**Importance:** medium
**Crate:** `crates/scp-hub/src/router.rs`
**Depends on:** P2.C, P2.B (SSE outbound channel)

### P2.G.1 — Detect sampling/createMessage from backend

- In the backend message receive loop: detect `sampling/createMessage` request
- This is a server-to-client request (backend asks client LLM to generate)
- **Acceptance:** unit test: backend sends sampling request, router identifies it as sampling

### P2.G.2 — Route sampling request to originating session

- Maintain `backend_session_to_client_session: HashMap<BackendSessionKey, SessionId>` in Router
- When backend sends `sampling/createMessage`, look up which client session triggered the current request
- Forward the sampling request to that session outbound SSE channel
- **Acceptance:** unit test: backend sampling request appears in correct client SSE stream

### P2.G.3 — Route sampling response back to backend

- Client responds to sampling request via POST /mcp with a response
- Router detects it is a response to a sampling request (by ID)
- Forward response to the backend that made the sampling request
- **Acceptance:** unit test: client sampling response forwarded to correct backend

---

## P2.H — Roots Handling

**Importance:** medium
**Crate:** `crates/scp-hub/src/router.rs`, `crates/scp-hub/src/session_store.rs`
**Depends on:** P2.C

### P2.H.1 — Store roots from client initialize

- During `initialize` handling: extract `roots` from `ClientCapabilities` if present
- Store in `Session.roots`
- **Acceptance:** unit test: initialize with roots, session.roots populated

### P2.H.2 — Handle roots/list request from backend

- Backend sends `roots/list` request to SCP (SCP is the client, so it must respond)
- Router intercepts this: respond directly with `session.roots` (no forwarding to client needed)
- **Acceptance:** unit test: backend sends roots/list, SCP responds with session roots

### P2.H.3 — Handle roots/listChanged notification from client

- Client sends `notifications/roots/list_changed`
- Router triggers re-fetch: send `roots/list` request to client, update `session.roots`
- **Acceptance:** unit test: client sends roots/listChanged, session.roots updated

---

## P2.I — Sessions CLI

**Importance:** low
**Crate:** `crates/scp-cli/src/main.rs`
**Depends on:** P2.C (SessionStore), P2.B (Admin API sessions endpoint)

### P2.I.1 — Admin API: GET /sessions endpoint

- `crates/scp-hub/src/admin.rs`: add `GET /sessions` handler
- Returns JSON array of `SessionSummary { id, created_at, last_active, tool_call_count, budget_remaining }`
- **Acceptance:** curl `GET /sessions` returns correct JSON

### P2.I.2 — Admin API: GET /sessions/{id} endpoint

- Returns full session details (capabilities, roots count, active request count, budget)
- Returns 404 if session not found
- **Acceptance:** curl `GET /sessions/{id}` returns session details

### P2.I.3 — Admin API: DELETE /sessions/{id} endpoint

- Force-close a session: remove from SessionStore, close SSE stream
- Returns 204 on success, 404 if not found
- **Acceptance:** DELETE closes session, subsequent GET returns 404

### P2.I.4 — CLI: scp sessions list

- `crates/scp-cli/src/main.rs`: add `sessions list` subcommand
- Calls `GET /sessions` on admin API
- Prints table: id | created_at | last_active | tool_calls | budget_remaining
- **Acceptance:** formatted table output matches admin API data

### P2.I.5 — CLI: scp sessions close <id>

- Calls `DELETE /sessions/{id}` on admin API
- Prints success or session not found
- **Acceptance:** session closed, no longer in `scp sessions list`

---

## P2.J — Re-wire Phase 0 Passthrough Tests

**Importance:** high
**Location:** `tests/tests/passthrough.rs`
**Depends on:** P2.B (HTTP listener), P2.C (SessionStore)

The 5 passthrough tests are currently `#[ignore]` because the CLI interface changed in Phase 1. Phase 2 re-wires them to use the HTTP transport.

### P2.J.1 — Test config file for passthrough tests

- Create `tests/fixtures/passthrough-test.toml`: minimal config pointing to `mock-mcp-server`
- Config: 1 server (stdio, mock-mcp-server binary), listen_port=3200 (test port), admin.port=3201
- No auth (for simplicity in passthrough tests)
- **Acceptance:** config file parses without error

### P2.J.2 — HTTP test helper functions

- Replace `spawn_scp_with_mock()` with `spawn_scp_http(config_path) -> ScpHttpClient`
- `ScpHttpClient { base_url: String, session_id: Option<String>, client: reqwest::Client }`
- `ScpHttpClient::initialize() -> InitializeResult` — POST initialize, store session_id from response header
- `ScpHttpClient::send_request(method, params) -> Value` — POST JSON-RPC, return result
- `ScpHttpClient::send_notification(method, params)` — POST notification, expect 202
- **Acceptance:** helper compiles and can connect to a running SCP instance

### P2.J.3 — Re-enable test_initialize_handshake

- Remove `#[ignore]`
- Use `ScpHttpClient::initialize()` instead of stdio
- Assert: response has `server_info.name == "scp"`, session_id header is set
- **Acceptance:** test passes without `#[ignore]`

### P2.J.4 — Re-enable test_tools_list_passthrough

- Remove `#[ignore]`
- Use HTTP client: initialize, then POST tools/list
- Assert: response has `tools` array
- **Acceptance:** test passes

### P2.J.5 — Re-enable test_tools_call_passthrough

- Remove `#[ignore]`
- Use HTTP client: initialize, then POST tools/call with `echo` tool
- Assert: response has `content` array
- **Acceptance:** test passes

### P2.J.6 — Re-enable test_ping_handled_by_scp

- Remove `#[ignore]`
- Use HTTP client: initialize, then POST ping
- Assert: response is `{}`
- **Acceptance:** test passes

### P2.J.7 — Re-enable test_id_remapping

- Remove `#[ignore]`
- Use HTTP client: initialize, send request id=100, then id=200
- Assert: responses have matching IDs (100 and 200 respectively)
- **Acceptance:** test passes

---

## P2.K — Integration Tests

**Importance:** high
**Location:** `tests/tests/phase2.rs`
**Depends on:** all P2 groups

### P2.K.1 — Test infrastructure: HTTP mock MCP server

- `tests/common/mock_http_server.rs`: in-process axum server that speaks Streamable HTTP MCP
- Configurable: tool list, response delay, failure injection, sampling request injection
- **Acceptance:** used by other tests without flakiness

### P2.K.2 — multi_client_concurrent_sessions_test

- Start SCP hub with HTTP listener
- Connect 3 clients simultaneously (3 HTTP sessions)
- Each client calls tools/list and tools/call concurrently
- Assert: each client gets correct responses with correct IDs (no cross-session contamination)
- **Acceptance:** all assertions pass, test completes in under 15s

### P2.K.3 — bearer_auth_test

- Start SCP hub with bearer auth configured (token = "test-token-123")
- Test 1: POST without Authorization header returns 401
- Test 2: POST with `Authorization: Bearer wrong-token` returns 401
- Test 3: POST with `Authorization: Bearer test-token-123` returns 200
- **Acceptance:** all three assertions pass

### P2.K.4 — session_timeout_test

- Configure session_timeout_secs = 2 (very short for test)
- Create session, wait 3 seconds
- Attempt to use session returns 404 (session expired)
- **Acceptance:** session expires and is cleaned up

### P2.K.5 — cancellation_propagation_test

- Start SCP with a slow mock backend (responds after 5s)
- Client sends tools/call, immediately sends notifications/cancelled
- Assert: backend receives cancellation notification
- Assert: client receives error response (not the 5s response)
- **Acceptance:** cancellation propagates within 500ms

### P2.K.6 — roots_handling_test

- Client initializes with roots: `[{ uri: "file:///home/user" }]`
- Backend sends roots/list request
- Assert: SCP responds with the client roots (not forwarding to client)
- **Acceptance:** roots round-trip works correctly

### P2.K.7 — dedicated_pool_isolation_test

- Configure a server with `sharing = "dedicated"`
- Two clients connect and call tools
- Assert: each client gets a separate backend process (verify via PID or unique state)
- Assert: when client A disconnects, client A backend process is killed
- **Acceptance:** dedicated pool isolation verified

### P2.K.8 — pooled_pool_distribution_test

- Configure a server with `sharing = "pooled"`, `pool_size = 2`
- Send 4 concurrent requests
- Assert: requests are distributed across 2 workers (not all to one)
- Assert: all 4 requests complete successfully
- **Acceptance:** load distribution verified

---

## Dependency Graph

```
P2.C (SessionStore) ──────────────────────────────────────────────────────┐
    │                                                                       │
    ├─► P2.B (HTTP Client Listener) ──────────────────────────────────────┤
    │       └─► P2.B.7 (Wire into main) ──────────────────────────────────┤
    │                                                                       │
    ├─► P2.E (Per-session ID mapping) ────────────────────────────────────┤
    │       └─► P2.F (Cancellation) ──────────────────────────────────────┤
    │                                                                       │
    ├─► P2.G (Sampling routing) ──────────────────────────────────────────┤
    ├─► P2.H (Roots handling) ────────────────────────────────────────────┤
    └─► P2.I (Sessions CLI) ──────────────────────────────────────────────┤
                                                                            │
P2.A (HTTP Server Transport) ─────────────────────────────────────────────┤
    └─► P2.D (Pooled + Dedicated) ────────────────────────────────────────┤
                                                                            │
P2.J (Passthrough tests re-wire) ◄────────────────────────────────────────┤
P2.K (Integration tests) ◄────────────────────────────────────────────────┘
```

---

## Recommended Build Sequence

### Sprint 1 — Foundation (parallel tracks)

**Track A (transport):** P2.A.1 → P2.A.2 → P2.A.3 → P2.A.4 → P2.A.5 → P2.A.6
**Track B (session):** P2.C.1 → P2.C.2 → P2.C.3 → P2.C.4 → P2.C.5

These two tracks have no dependencies on each other and can be built in parallel.

### Sprint 2 — Listener + Pool Strategies

**Sequential after Sprint 1:**
P2.B.1 → P2.B.2 → P2.B.3 → P2.B.4 → P2.B.5 → P2.B.6 → P2.B.7
P2.D.1 → P2.D.2 → P2.D.3 → P2.D.4 → P2.D.5

P2.B and P2.D can be built in parallel (B needs P2.C; D needs P2.A and P2.C).

### Sprint 3 — Protocol Features

**After Sprint 2:**
P2.E.1 → P2.E.2 → P2.E.3
P2.F.1 → P2.F.2 → P2.F.3 (after P2.E)
P2.G.1 → P2.G.2 → P2.G.3 (parallel with P2.F)
P2.H.1 → P2.H.2 → P2.H.3 (parallel with P2.F and P2.G)

### Sprint 4 — CLI + Test Re-wire

**After Sprint 3:**
P2.I.1 → P2.I.2 → P2.I.3 → P2.I.4 → P2.I.5
P2.J.1 → P2.J.2 → P2.J.3 → P2.J.4 → P2.J.5 → P2.J.6 → P2.J.7

P2.I and P2.J can be built in parallel.

### Sprint 5 — Integration Tests

**After Sprint 4:**
P2.K.1 → P2.K.2 through P2.K.8 (P2.K.2–P2.K.8 can be written in parallel after P2.K.1)

---

## Critical Path

```
P2.C.1 → P2.C.2 → P2.C.4 → P2.E.1 → P2.E.2 → P2.F.1 → P2.F.2
    ↓
P2.B.2 → P2.B.3 → P2.B.4 → P2.B.7
    ↓
P2.J.2 → P2.J.3 → P2.K.2
```

The critical path runs through SessionStore → Listener → Passthrough tests → Integration tests.

---

## Parallelism Opportunities

| Sprint | Parallel work |
|--------|--------------|
| Sprint 1 | P2.A (transport) parallel with P2.C (session store) |
| Sprint 2 | P2.B (listener) parallel with P2.D (pool strategies) |
| Sprint 3 | P2.F (cancel) parallel with P2.G (sampling) parallel with P2.H (roots) |
| Sprint 4 | P2.I (CLI) parallel with P2.J (test re-wire) |
| Sprint 5 | P2.K.2 through P2.K.8 all parallel after P2.K.1 |

---

## First Tasks for @build

The following tasks have no dependencies and can be started immediately in parallel:

1. **P2.A.1** — Add HTTP deps to scp-transport (Cargo.toml edit + lib.rs mod declaration)
2. **P2.C.1** — Session struct definition (session_store.rs new file)

After those complete:
3. **P2.A.2** — HttpServerTransport struct
4. **P2.C.2** — SessionStore struct

---

## Cargo.toml Changes Required

### crates/scp-transport/Cargo.toml
Add:
```toml
axum = { workspace = true }
reqwest = { workspace = true }
tokio-stream = "0.1"
eventsource-stream = "0.2"
futures = "0.3"
axum-extra = { version = "0.9", features = ["typed-header"] }
headers = "0.4"
```

### Cargo.toml (workspace)
Add to [workspace.dependencies]:
```toml
tokio-stream = "0.1"
eventsource-stream = "0.2"
futures = "0.3"
axum-extra = { version = "0.9", features = ["typed-header"] }
headers = "0.4"
```

### crates/scp-hub/Cargo.toml
Add:
```toml
futures = { workspace = true }
```

---

## Task Count Summary

| Group | Tasks | Importance |
|-------|-------|------------|
| P2.A HTTP Server Transport | 6 | critical |
| P2.B HTTP Client Listener | 7 | critical |
| P2.C SessionStore | 5 | critical |
| P2.D Pooled + Dedicated | 5 | high |
| P2.E Per-session ID mapping | 3 | high |
| P2.F Cancellation | 3 | high |
| P2.G Sampling routing | 3 | medium |
| P2.H Roots handling | 3 | medium |
| P2.I Sessions CLI | 5 | low |
| P2.J Passthrough test re-wire | 7 | high |
| P2.K Integration tests | 8 | high |
| **Total** | **55** | |

---

*Note: Task MCP embedding store was unavailable (budget exhausted). This file is the authoritative task record for Phase 2. The orchestrator should use task IDs from this file as logical references.*
