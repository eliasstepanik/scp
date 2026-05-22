# SCP — Selective Context Protocol

> Version: 0.3-draft
> Last updated: 2026-05-22


## 1. Vision

SCP is an MCP-compatible proxy hub that sits between LLM clients and MCP servers. It intercepts all MCP traffic and applies intelligent context selection before forwarding responses. Existing MCP servers work unmodified. Existing MCP clients connect to SCP as if it were a standard MCP server.

**Core principle:** Context is a budget, not a dump. Every token that reaches the model must earn its place through measured relevance.

**Non-goals (explicitly out of scope):**

- SCP is not a new protocol. It is a middleware layer that speaks MCP on both sides.
- SCP does not modify MCP server behavior. Servers are unaware of the proxy.
- SCP does not replace MCP. It augments it with a selection layer that MCP itself does not provide.
- SCP does not require client modifications for basic operation. Advanced features (intent hints, explicit budget requests) use optional MCP extensions that non-SCP-aware clients simply ignore.

**License:** Dual-licensed under MIT and Apache 2.0 (`MIT OR Apache-2.0`), following the Rust ecosystem convention. Contributors must agree to both licenses.


## 2. Problem Statement

MCP has no concept of relevance or cost. Every tool response is forwarded to the model in full, regardless of size, relevance, or how many other sources compete for context window space. At scale this creates compounding problems:

**Token waste.** A `filesystem/read_file` returning a 50KB log file consumes the entire context budget on content the model may need three lines from. Multiply by N concurrent tool calls and the context window is full of noise.

**Attention dilution.** Models perform worse when irrelevant content competes for attention. A relevant 200-token response buried in 8000 tokens of filler gets less model attention than the same 200 tokens delivered alone.

**Tool overload.** With 100+ servers exposing 500+ tools, the tool list itself becomes a context burden. Models struggle to select the right tool when presented with hundreds of options. Some clients (Claude, for example) have practical limits on how many tool definitions they handle well.

**No multiplexing.** Each client-server pair is a 1:1 connection. Ten clients using the same filesystem server means ten processes. No connection sharing, no pooling, no central coordination.

**No cross-source coordination.** When a model calls three tools in sequence, each response is independently sized. There is no mechanism to say "I already got 3000 tokens from tool A, so tool B should be more concise." Responses are blind to each other.


## 3. MCP Protocol Baseline

SCP must handle the full MCP protocol, not just tools. This section enumerates everything the proxy must understand.

### 3.1 Target Spec Version

MCP 2025-03-26 (current latest). This is the version that introduced Streamable HTTP transport and deprecated raw SSE.

### 3.2 Transports (Wire Level)

| Transport       | Direction    | Status in MCP spec | SCP support |
| --------------- | ------------ | ------------------ | ----------- |
| stdio           | Bidirectional via stdin/stdout | Stable | Required (Phase 0) |
| SSE (legacy)    | Server→Client events, Client→Server POST | Deprecated but widely used | Required (Phase 1) |
| Streamable HTTP | Bidirectional over HTTP with optional SSE upgrade | Current recommended | Required (Phase 2) |

SCP must support all three on **both sides** — as a server (accepting client connections) and as a client (connecting to backend MCP servers). A client may connect via stdio while the backend server uses Streamable HTTP. The proxy translates between transports transparently.

### 3.3 Lifecycle & Capability Negotiation

MCP connections begin with an `initialize` request from client to server, containing:
- Protocol version
- Client capabilities (roots, sampling)
- Client info (name, version)

The server responds with its capabilities (tools, resources, prompts, logging) and info. The client then sends `initialized` notification to confirm.

**SCP behavior:** The hub must perform capability negotiation on both sides independently.

- **Client-facing:** SCP advertises the *union* of all backend server capabilities, filtered by what the session is allowed to access. If any backend supports tools, SCP advertises tools. If any supports resources, SCP advertises resources.
- **Server-facing:** SCP connects to each backend and performs initialize as a normal client. It stores each server's declared capabilities and only routes requests for capabilities a server actually supports.
- **Version mismatch handling:** If a client requests a protocol version that a backend server doesn't support, SCP must either translate (if possible) or return an error for that specific backend's features.

### 3.4 MCP Primitives

The proxy must handle all six MCP primitives. Each has different implications for context selection:

**Tools** (`tools/list`, `tools/call`)
- Highest impact for SCP. Tool responses are the primary target for filtering.
- `tools/list` → SCP returns a filtered/scored subset from the merged global tool registry.
- `tools/call` → SCP forwards to the owning server, filters the response, delivers to client.
- Tool names may collide across servers (two servers both exposing `search`). SCP must namespace or disambiguate (see §6.4).

**Resources** (`resources/list`, `resources/read`, `resources/subscribe`)
- Resources are URI-addressed content (files, database rows, API data).
- `resources/list` → SCP merges resource lists from all backends, applies relevance filtering.
- `resources/read` → SCP forwards to the owning server, filters response content by budget.
- `resources/subscribe` → SCP forwards subscription, relays update notifications to the client.
- Resource templates (URI templates with parameters) must be forwarded unchanged.
- **URI namespacing:** Resource URIs can collide across servers (two servers could both expose `file:///etc/hosts`). SCP prefixes resource URIs with the server name when presenting to clients: `scp://chromadb/file:///etc/hosts` vs `scp://filesystem/file:///etc/hosts`. On `resources/read`, SCP strips the `scp://{server}/` prefix before forwarding to the backend. If a resource URI is unique across all servers, SCP exposes it unprefixed for convenience (configurable: `resource_namespacing = "always" | "on_collision" | "never"`, default `"on_collision"`).

**Prompts** (`prompts/list`, `prompts/get`)
- Prompts are parameterized message templates.
- Lower priority for filtering — prompts are usually small.
- SCP merges prompt lists and routes `prompts/get` to the owning server.
- **Prompt namespacing:** Same collision rules as tools. If two servers expose a prompt named `summarize`, SCP qualifies them as `chromadb.summarize` and `notion.summarize`. Config aliases apply. The unqualified name maps to the higher-priority server by default.

**Sampling** (`sampling/createMessage`)
- Server-to-client direction: a server asks the client's LLM to generate a completion.
- SCP must route sampling requests back to the originating client session, not to other clients.
- SCP does NOT filter sampling requests (the server is asking the model to think, not delivering context).
- If multiple clients could satisfy a sampling request, SCP routes to the session that triggered the original tool call.

**Roots** (`roots/list`)
- Client-to-server direction: client tells servers which filesystem roots it exposes.
- SCP must forward roots from each client session to backend servers.
- For `shared` and `pooled` servers, this is tricky: different clients may declare different roots. SCP must track which session's roots apply to which requests.

**Logging** (`logging/setLevel`, `notifications/message`)
- Server sends log messages to client.
- SCP forwards log notifications to the session that triggered the relevant request.
- Log messages do NOT count against token budgets (they're for human debugging, not model context).

### 3.5 Notifications

These are fire-and-forget messages (no response expected):

| Notification | Direction | SCP behavior |
|---|---|---|
| `notifications/initialized` | Client→Server | Forward to all backends on session start |
| `notifications/tools/list_changed` | Server→Client | Rebuild tool index for that server, then notify affected client sessions |
| `notifications/resources/list_changed` | Server→Client | Rebuild resource index, notify sessions |
| `notifications/resources/updated` | Server→Client | Forward to sessions subscribed to that resource |
| `notifications/prompts/list_changed` | Server→Client | Rebuild prompt index, notify sessions |
| `notifications/progress` | Server→Client | Forward to session that owns the in-flight request |
| `notifications/message` (logging) | Server→Client | Forward to owning session |
| `notifications/cancelled` | Either direction | See §3.6 |

When a backend emits `tools/list_changed`, SCP must re-fetch that server's tool list, update the global index, and emit `tools/list_changed` to all connected clients so they re-request the (now re-filtered) tool list.

### 3.6 Cancellation

MCP supports cancelling in-flight requests via `notifications/cancelled` with a `requestId` and optional `reason`.

- **Client cancels:** SCP receives cancellation, looks up which backend server is handling that request (via request ID mapping), and forwards cancellation to that server. SCP cleans up its own in-flight tracking.
- **Server cancels (sampling):** If a server cancels a sampling request it made to the client, SCP forwards the cancellation to the client session.
- **Request ID mapping:** SCP does NOT forward client request IDs directly to servers. It maps them: client sends request ID `42`, SCP generates internal ID `scp-a3f9`, sends that to the server. SCP maintains a bidirectional map per session. This is required because multiple clients might use the same request ID space.

### 3.7 Ping

MCP supports `ping` requests for keepalive. SCP responds to client pings directly (it IS the server from the client's perspective). SCP sends pings to backend servers independently for health checking.


## 4. Architecture

```
Clients (MCP)                    SCP Hub                         Backend MCP Servers
─────────────               ─────────────────                ──────────────────────

                         ┌────────────────────┐
Client A ──stdio──┐      │                    │      ┌──stdio──▶ filesystem [pool:3]
Client B ──stdio──┤      │  ┌──────────────┐  │      │            ├ worker 0
Client C ──HTTP───┼─────▶│  │ Listener     │  │      │            ├ worker 1
Client D ──HTTP───┤      │  │ (stdio+HTTP) │  │      │            └ worker 2
Client E ──SSE────┘      │  └──────┬───────┘  │      │
                         │         │          │      ├──HTTP───▶ notion (shared)
                         │  ┌──────▼───────┐  │      │
                         │  │ Session Mgr  │  │      ├──stdio──▶ mempalace [dedicated]
                         │  │ (per-client  │  │      │            ├ Client A instance
                         │  │  isolation)  │  │      │            ├ Client B instance
                         │  └──────┬───────┘  │      │            └ ...
                         │         │          │      │
                         │  ┌──────▼───────┐  │      ├──SSE────▶ legacy-server (shared)
                         │  │ Router       │  │      │
                         │  │ (dispatch +  │──┼──────┤
                         │  │  fan-out)    │  │      └──stdio──▶ chromadb (shared)
                         │  └──────┬───────┘  │
                         │         │          │
                         │  ┌──────▼───────┐  │
                         │  │ Filter       │  │
                         │  │ Pipeline     │  │
                         │  └──────┬───────┘  │
                         │         │          │
                         │  ┌──────▼───────┐  │
                         │  │ Tool Index   │  │
                         │  │ + Budget Mgr │  │
                         │  └──────────────┘  │
                         │                    │
                         │  ┌──────────────┐  │
                         │  │ Admin API    │  │
                         │  │ (:3101)      │  │
                         │  └──────────────┘  │
                         └────────────────────┘
```

SCP speaks MCP on both sides. Clients see it as a single MCP server. Backend servers see it as a normal MCP client. The intelligence is entirely in the middle layer.


## 5. Core Components

### 5.1 Session Manager

Every connected client gets an isolated session. A session is created on `initialize` and destroyed on disconnect or timeout.

**Session state:**

```rust
struct Session {
    id: SessionId,                          // UUID
    auth: Option<AuthIdentity>,             // bearer token or client cert identity
    created_at: Instant,
    last_activity: Instant,

    // MCP state
    client_capabilities: ClientCapabilities,  // from initialize request
    client_info: ClientInfo,                  // name, version
    roots: Vec<Root>,                         // filesystem roots declared by client

    // SCP state
    token_budget: TokenBudget,               // remaining budget for this session
    tool_scope: Option<Vec<ToolFilter>>,      // allowlist/denylist of tools
    delivery_log: DeliveryLog,               // hashes of content already sent
    request_map: BiMap<RequestId, InternalId>, // client↔internal request ID mapping
    active_requests: HashMap<InternalId, InFlightRequest>, // currently in-progress

    // Context tracking
    call_history: VecDeque<ToolCallRecord>,   // last N tool calls + summaries
    context_keywords: KeywordAccumulator,     // extracted keywords from tool args

    // Rate limiting
    rate_limiter: RateLimiter,               // token bucket per session
}
```

**Rate limiting:** Each session has a configurable rate limit to prevent a single client from overwhelming backend servers. Default: 100 requests/minute, burst of 20. When a client exceeds the rate limit, SCP returns a JSON-RPC error (`-32003: Rate limit exceeded, retry after {n}ms`). Rate limits are per session, not global — one busy client does not affect others. Configuration:

```toml
[hub.defaults]
rate_limit_per_minute = 100
rate_limit_burst = 20
```

Per-client overrides via auth token mapping:

```toml
[hub.auth.profiles.power_user]
token = "token-abc-123"
rate_limit_per_minute = 500
rate_limit_burst = 50
token_budget_per_request = 8000
```

**Memory limits:** `delivery_log` is capped at 10,000 hashes with LRU eviction. `call_history` is capped at 100 entries. `context_keywords` uses a fixed-size TF-IDF accumulator with decay. These defaults are configurable per session profile.

**Session persistence:** Sessions are ephemeral by default (in-memory, lost on hub restart). Optional persistence to SQLite for long-running clients — only session metadata is persisted, not the full delivery log (which rebuilds naturally).

### 5.2 Server Registry & Pool Manager

Central registry of all configured MCP servers.

**Server entry:**

```rust
struct ServerEntry {
    name: String,                    // unique identifier
    transport: TransportConfig,      // stdio command, SSE URL, or HTTP URL
    sharing: SharingStrategy,        // shared | pooled { size } | dedicated
    priority: Priority,              // high | medium | low (affects budget allocation)
    tags: Vec<String>,               // freeform labels for coarse tool matching
    enabled: bool,                   // false = disabled (registered but inactive, tools hidden)
    idle_timeout: Duration,          // time before cold-ing an idle connection
    request_timeout: Duration,       // max time for a single request
    max_retries: u32,                // retry count on transient failures
    health: HealthState,             // healthy | degraded | unhealthy | unknown | disabled
    capabilities: ServerCapabilities, // tools, resources, prompts — from initialize
    env: HashMap<String, String>,    // environment variables for stdio servers
}
```

**Sharing strategies in detail:**

`shared`: One connection instance. All requests from all sessions are serialized through it. The proxy uses MCP's JSON-RPC request IDs for correlation — it sends a request with a unique ID, and matches the response by that ID. This means the proxy can pipeline multiple requests on a single connection IF the server handles concurrent requests (most SSE/HTTP servers do). For stdio servers that process requests sequentially, a FIFO queue with per-request futures ensures correct correlation. A `tokio::Mutex` guards the write side; the read side demultiplexes by request ID.

`pooled { size: N }`: N identical instances of the same server. Dispatch strategy: least-outstanding-requests (not round-robin, because response times vary). If all instances are busy and a request arrives, it queues with a configurable max queue depth. If the queue overflows, the request fails with a backpressure error. Pool instances are started lazily — instance 0 starts on first request, instance 1 when instance 0 is busy, etc.

`dedicated`: One instance per client session. Created when the session first routes a request to this server. Destroyed when the session ends. This is the only strategy where the server sees a single logical client, which is required for servers that maintain internal state per connection (e.g., stateful REPL servers, servers that track conversation history).

**Lifecycle states:**

| State | Meaning | Transition triggers |
|-------|---------|-------------------|
| `cold` | Not connected, process not running | Initial state; after idle timeout; after max failures |
| `starting` | Connection/process being established | Request arrives for cold server |
| `warm` | Connected, idle, awaiting requests | After completing a request; after startup |
| `hot` | Actively processing one or more requests | Request dispatched |
| `draining` | No new requests; waiting for in-flight to finish | Shutdown signal; server removal; config reload; deactivation |
| `disabled` | Administratively deactivated. Tools hidden from index. Connections torn down. Server remains in registry for re-activation. | Explicit admin action (API/CLI/config). No automatic transitions into or out of this state. |
| `failed` | Consecutive failures exceeded threshold | N consecutive errors (configurable, default 5) |

**Health checking:** The hub pings each `warm` server every 30 seconds (configurable). Three consecutive missed pings → `degraded`. Five → `unhealthy` → connections are torn down and server goes `cold`. On next request, it tries to reconnect. After `max_retries` failed reconnects, server goes `failed` and is excluded from routing until manually reset or config reload.

**Dynamic server management (core requirement, not optional):**

Servers can be added, removed, deactivated, and reactivated at runtime without restarting the hub. This is a first-class capability, not an afterthought. All four operations are available from Phase 1 onwards via three interfaces: the admin HTTP API, the CLI, and TOML config hot-reload.

*Add a server at runtime:*
1. Client submits server definition (via API, CLI, or by editing config + triggering reload).
2. Hub validates the definition (transport reachable, no name collision).
3. Hub registers the server in state `cold`.
4. Hub performs MCP `initialize` handshake → server moves to `warm`.
5. Hub collects `tools/list` from the new server → merges into the global tool index.
6. Hub emits `notifications/tools/list_changed` to all connected client sessions so they re-fetch the (now updated) tool list.

*Remove a server at runtime:*
1. Hub moves the server to `draining` — no new requests are routed to it.
2. Hub waits for all in-flight requests to that server to complete (with a configurable timeout, default 10s; after timeout, in-flight requests are cancelled and error responses are returned to clients).
3. Hub tears down all connections (kills stdio processes, closes SSE/HTTP connections). For `dedicated` servers, all per-session instances are terminated.
4. Hub removes the server's tools from the global tool index.
5. Hub emits `notifications/tools/list_changed` to all connected clients.
6. Hub removes the server entry from the registry entirely. It is gone.

*Deactivate a server at runtime (disable without removing):*
1. Hub moves the server to `draining` → waits for in-flight → tears down connections → server enters `disabled`.
2. Hub removes the server's tools from the global tool index.
3. Hub emits `notifications/tools/list_changed` to all connected clients.
4. The server entry stays in the registry. It appears in `scp servers list` with state `disabled`. It can be reactivated later without re-entering the full server definition.

*Reactivate a disabled server:*
1. Hub moves the server from `disabled` to `cold`.
2. On next request (or immediately if `eager_connect = true`), hub performs `initialize` → collects tools → updates index → notifies clients.

*Config hot-reload:*
On `SIGHUP` signal or `POST /admin/config/reload`:
1. Hub reads and validates the new config file. If validation fails → log error, keep running with old config, return error on API response.
2. Hub diffs the new server list against the current registry:
   - Servers present in new config but not in registry → **add**.
   - Servers present in registry but not in new config → **remove** (with draining).
   - Servers present in both but with changed settings → **update** (drain old connection, reconnect with new settings).
   - Servers present in both with `enabled = false` in new config → **deactivate**.
   - Servers present in both with `enabled = true` that were previously disabled → **reactivate**.
3. Non-server config changes (budget defaults, auth tokens, tool index engine) are applied immediately.

*Consistency guarantee:* At no point during any of these operations will a client see a stale tool list. The sequence is always: update internal state → update tool index → notify clients. Notifications are sent after the index is consistent.

```rust
/// Runtime server management operations.
/// All operations are async-safe and can be called from any task.
pub trait ServerManager: Send + Sync {
    /// Register and connect a new server. Returns error if name conflicts.
    async fn add_server(&self, config: ServerConfig) -> Result<(), AddServerError>;

    /// Drain and permanently remove a server.
    async fn remove_server(&self, name: &str) -> Result<(), RemoveServerError>;

    /// Drain and disable a server (keeps registry entry for reactivation).
    async fn disable_server(&self, name: &str) -> Result<(), DisableServerError>;

    /// Reactivate a disabled server.
    async fn enable_server(&self, name: &str) -> Result<(), EnableServerError>;

    /// Update a server's config (drain old, reconnect with new).
    async fn update_server(&self, name: &str, config: ServerConfig) -> Result<(), UpdateServerError>;

    /// List all servers with current state.
    async fn list_servers(&self) -> Vec<ServerStatus>;

    /// Get detailed status for one server.
    async fn server_status(&self, name: &str) -> Option<ServerStatus>;
}
```

### 5.3 Tool Index

The tool index is the global registry of all tools across all backends. It is rebuilt whenever a server is added, removed, or emits `tools/list_changed`.

**Structure:**

```rust
struct ToolIndex {
    // Primary lookup
    tools: HashMap<QualifiedToolName, ToolEntry>,

    // Reverse mapping for routing
    server_tools: HashMap<ServerName, Vec<QualifiedToolName>>,

    // Scoring structures (populated lazily)
    tfidf_index: Option<TfIdfIndex>,        // built from tool descriptions
    embedding_index: Option<EmbeddingIndex>, // built from tool description embeddings
    usage_stats: UsageTracker,              // call frequency per tool per session profile
}

struct ToolEntry {
    original_name: String,        // tool name as the server knows it
    qualified_name: String,       // server_name.tool_name (for disambiguation)
    server: ServerName,           // which server owns this tool
    description: String,          // tool description from the server
    input_schema: Value,          // JSON Schema of tool parameters
    tags: Vec<String>,            // inherited from server + any tool-level overrides
    avg_response_tokens: f32,     // rolling average of response sizes (for budget prediction)
    call_count: u64,              // total calls across all sessions
}
```

**Tool name collision handling:** When two servers expose a tool with the same name (e.g., both expose `search`), SCP uses qualified names: `chromadb.search` and `notion.search`. The original unqualified name is mapped to the higher-priority server by default. Config can override this with explicit aliases:

```toml
[tool_aliases]
"search" = "chromadb.search"     # unqualified "search" → chromadb
"web_search" = "notion.search"   # explicit alias
```

If no alias is configured and priorities are equal, SCP appends a suffix: `search` (first registered), `search_2` (second). This is a last resort — config-based aliasing is preferred.

**Tool selection algorithm for `tools/list`:**

When a client calls `tools/list`, SCP does NOT return all 500+ tools. It scores and filters:

1. **Scope filter** — remove tools not in the session's allowlist (if configured).
2. **Health filter** — remove tools from `failed` or `unhealthy` servers.
3. **Tag pre-filter** — if session has context keywords, remove tools whose server tags have zero overlap (cheap, coarse).
4. **Relevance scoring** — score remaining tools by the active scoring engine:
   - `tags`: Jaccard similarity between server tags and context keywords. Score 0.0-1.0.
   - `tfidf`: TF-IDF cosine similarity between tool description and context keywords. Score 0.0-1.0.
   - `embedding`: Cosine similarity between tool description embedding and context embedding. Score 0.0-1.0.
   - `usage`: Bayesian score weighted by call frequency for similar past contexts. Score 0.0-1.0.
   - Final score = weighted combination (configurable weights, default: primary engine × 0.7 + usage × 0.3).
5. **Top-N selection** — return the top N tools (configurable, default 20), always including tools from `high` priority servers regardless of score.
6. **Mandatory tools** — some tools can be marked `always_include = true` in config (e.g., a core search tool). These bypass scoring.

**The intent problem:** The proxy does NOT have access to the user's conversation — it only sees JSON-RPC messages. "Context" for scoring comes from:
- Tool call arguments (the proxy sees what the client is searching for, reading, querying).
- Tool names called (filesystem calls suggest code-related work, notion calls suggest docs).
- Explicit intent hints (an optional SCP extension — see §9).
- Accumulated keywords from the above, decayed over time.

This is intentionally limited. SCP does not try to understand the conversation — it uses tool usage patterns as a proxy signal. This is a design choice, not a limitation to fix later.

### 5.4 Router

The router maps incoming MCP requests to the correct backend server and manages the request lifecycle.

**Routing table:**

| Request method | Routing logic |
|---|---|
| `tools/list` | Fan-out to all servers (or use cache), merge, filter through tool index |
| `tools/call` | Look up tool name in tool index → get server → dispatch to pool manager |
| `resources/list` | Fan-out to all servers that support resources, merge, apply URI namespacing |
| `resources/read` | Strip `scp://{server}/` prefix if present → look up URI → route to owning server |
| `resources/subscribe` | Route to owning server, track subscription in session |
| `prompts/list` | Fan-out to all servers that support prompts, merge, apply name qualification |
| `prompts/get` | Look up qualified prompt name → strip prefix → route to owning server |
| `sampling/createMessage` | Route back to the client session that owns the originating request |
| `roots/list` | Respond directly from session state (SCP is the server here) |
| `ping` | Respond directly (SCP handles its own keepalive) |
| `logging/setLevel` | Forward to all servers (or per-server if specified) |

**Name stripping on forwarding:**

SCP exposes qualified names to clients (`chromadb.search`, `notion.summarize`, `scp://filesystem/file:///data`) but backend servers know only their original unqualified names. SCP must strip qualifications before forwarding:

```
Client calls:        tools/call { name: "chromadb.search", arguments: {...} }
SCP forwards to chromadb: tools/call { name: "search", arguments: {...} }

Client calls:        resources/read { uri: "scp://filesystem/file:///data/log.txt" }
SCP forwards to filesystem: resources/read { uri: "file:///data/log.txt" }

Client calls:        prompts/get { name: "notion.summarize", arguments: {...} }
SCP forwards to notion: prompts/get { name: "summarize", arguments: {...} }
```

If the client sends an unqualified name that maps unambiguously to one server (no collision), SCP forwards it as-is. If the unqualified name is ambiguous, SCP uses the configured alias or priority-based default (see §5.3).

**Request ID mapping:**

Clients and servers have independent request ID spaces. SCP maintains a bidirectional map per session:

```
Client A sends: { id: 1, method: "tools/call", ... }
SCP generates:  { id: "scp-a3f9-001", method: "tools/call", ... }  → sends to server
Server responds: { id: "scp-a3f9-001", result: ... }
SCP maps back:   { id: 1, result: ... }  → sends to Client A
```

This prevents ID collisions when multiple clients use overlapping ID spaces, and allows SCP to track in-flight requests globally.

**Fan-out mechanics (for list operations):**

```rust
async fn fan_out_tools_list(&self, session: &Session) -> Vec<Tool> {
    let servers = self.registry.servers_with_capability("tools");
    let timeout = self.config.fanout_timeout; // default 5s

    let mut join_set = JoinSet::new();
    for server in servers {
        let pool = self.pool_manager.get(&server);
        join_set.spawn(async move {
            tokio::time::timeout(timeout, pool.send("tools/list", {})).await
        });
    }

    let mut all_tools = Vec::new();
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok(Ok(tools))) => all_tools.extend(tools),
            Ok(Ok(Err(e))) => tracing::warn!(server = %e.server, "tools/list failed"),
            Ok(Err(_)) => tracing::warn!("tools/list timed out for a server"),
            Err(e) => tracing::error!("join error: {e}"),
        }
    }

    all_tools
}
```

Slow or failed servers are logged but don't block the response. Their tools are simply absent from this listing cycle. This is intentional: a 5-second timeout on one server shouldn't freeze a client that has 99 healthy servers.

**Caching:** `tools/list` results are cached per server and invalidated on `tools/list_changed` notifications. Subsequent `tools/list` from clients use the cache, re-running only the scoring/filtering step. Cache TTL is configurable (default: 5 minutes) as a fallback if a server doesn't send change notifications.

### 5.5 Filter Pipeline

The filter pipeline processes tool/resource responses before they reach the client. Each stage is a trait implementation:

```rust
#[async_trait]
trait ContextFilter: Send + Sync {
    /// Filter a tool response. Returns the filtered content.
    /// May return None to drop the response entirely.
    async fn filter(
        &self,
        content: ToolResponseContent,
        context: &FilterContext,
    ) -> Result<Option<ToolResponseContent>, FilterError>;

    /// Name for logging/metrics
    fn name(&self) -> &str;
}

struct FilterContext<'a> {
    session: &'a Session,
    tool_name: &'a str,
    tool_args: &'a Value,
    budget_remaining: usize,    // tokens left for this request
    server_name: &'a str,
}
```

**Pipeline stages (in order):**

```
Raw MCP Response
       │
       ▼
┌──────────────────┐
│ 1. Content Type  │──▶ classify: text | structured_json | image | binary | mixed
│    Router        │    route non-text content to bypass (images pass through unfiltered)
└──────┬───────────┘
       ▼
┌──────────────────┐
│ 2. Token         │──▶ count tokens in text content
│    Measurement   │    if under budget → pass through unchanged (skip remaining stages)
└──────┬───────────┘
       ▼
┌──────────────────┐
│ 3. Dedup         │──▶ hash content chunks, check against session delivery log
│    Check         │    drop chunks already delivered in this session
└──────┬───────────┘
       ▼
┌──────────────────┐
│ 4. Chunk         │──▶ split large text into chunks (paragraph, section, or line-based)
│    Splitter      │    preserve structure (JSON arrays split by element, text by paragraph)
└──────┬───────────┘
       ▼
┌──────────────────┐
│ 5. Relevance     │──▶ score each chunk against session context
│    Scorer        │    engine: keyword overlap (v0) → TF-IDF (v1) → embedding (v2)
└──────┬───────────┘
       ▼
┌──────────────────┐
│ 6. Budget        │──▶ rank chunks by score, select top-k that fit in remaining budget
│    Enforcer      │    strategy: truncate (drop lowest) | summarize (LLM call) | hybrid
└──────┬───────────┘
       ▼
┌──────────────────┐
│ 7. Progressive   │──▶ if chunks were dropped, append metadata:
│    Disclosure    │    "[SCP: 15 of 42 results shown. Use scp_get_more for remaining.]"
│    Annotator     │    register a synthetic scp_get_more tool call target in the session
└──────┬───────────┘
       ▼
┌──────────────────┐
│ 8. Delivery      │──▶ record content hashes in session delivery log
│    Logger        │    update token budget consumed
└──────┬───────────┘
       ▼
Filtered MCP Response → Client
```

**Why this order matters:**

- Content type routing (stage 1) must be first — you can't TF-IDF score an image.
- Token measurement (stage 2) short-circuits small responses — most tool responses are small enough to pass through without filtering, so avoid the overhead.
- Dedup (stage 3) before scoring — no point scoring content that will be dropped anyway.
- Chunking (stage 4) before scoring — scoring operates on chunks, not the whole blob.
- Relevance (stage 5) before budget (stage 6) — budget enforcer needs scores to decide what to cut.
- Progressive disclosure (stage 7) after budget — it annotates what was cut.
- Delivery logging (stage 8) last — only record what was actually sent.

**Content type handling:**

| Content type | Filter behavior |
|---|---|
| `text` | Full pipeline (chunk, score, filter) |
| `structured_json` | If array: split by element, score each. If object: pass through (usually small) |
| `image` (base64) | Pass through unfiltered. Images are counted against budget by estimated token cost (base64 length / 3 * 0.75 for typical vision token costs), but not scored or truncated. |
| `binary` / `blob` | Pass through unfiltered, not counted against text token budget |
| `mixed` (text + images) | Text portions go through pipeline, images pass through |

**Token counting:**

The proxy does not know which model the client is using, so it cannot use model-specific tokenizers. Instead, SCP uses a **heuristic token estimator**: `token_count ≈ byte_length / 3.5` for English/code, `byte_length / 2.5` for non-Latin scripts. This is intentionally conservative (overestimates slightly) — it's better to deliver slightly under budget than over. The estimator is a trait, so it can be swapped for `tiktoken-rs` with a configured model if the client declares its model in initialization metadata (see §9, extensions).

**Summarization strategy (stage 6, optional):**

When configured, instead of dropping low-relevance chunks, the budget enforcer can summarize them via a local LLM:

```toml
[filter.budget]
strategy = "hybrid"           # "truncate" | "summarize" | "hybrid"
summarize_model = "qwen3:1.7b" # small, fast model for summaries
summarize_endpoint = "http://localhost:11434/api/generate"
summarize_max_latency_ms = 500  # if summarization takes longer, fall back to truncate
```

Hybrid strategy: chunks above 0.7 relevance score pass through verbatim. Chunks between 0.3-0.7 are summarized. Chunks below 0.3 are dropped. Thresholds are configurable.

**Summarization adds latency.** The hub must track p50/p95 summarization latency and fall back to truncation if the summarization endpoint is slow or down. Summarization is never on the critical path for v0 — it's an opt-in enhancement.

### 5.6 Budget Manager

Token budgets are allocated and tracked per session, per request, and globally.

**Budget hierarchy:**

```
Global budget (all sessions combined)
  └── Session budget (per-client)
        └── Request budget (per individual tool/resource call)
```

**Allocation algorithm for a single request:**

```
request_budget = min(
    session.remaining_budget,
    config.max_tokens_per_request,
    estimated_need(tool)
)

estimated_need(tool) = tool.avg_response_tokens * 1.2  // 20% headroom
```

If the session budget is nearly exhausted, the request budget is clamped to what remains. If the session budget IS exhausted, tool calls still go through but responses are aggressively summarized/truncated to a hard minimum (configurable, default 200 tokens — enough for an error message or a minimal result).

**Budget replenishment:** Session budgets replenish on a configurable schedule:
- `per_request`: full budget available for each tool call (simplest, no cross-request coordination).
- `per_turn`: budget resets when the client sends a new top-level request after receiving a response (heuristic: new request ID that isn't a continuation).
- `sliding_window`: budget covers the last N tool calls, oldest calls "expire" and free budget.
- `manual`: budget only resets when the client explicitly requests it (via SCP extension, see §9).

Default is `per_request` for simplicity. `sliding_window` is recommended for production use.

### 5.7 Admin API

A separate HTTP server (different port from the MCP listener) for management:

**Endpoints:**

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | Hub health status |
| `GET` | `/metrics` | Prometheus-format metrics |
| `GET` | `/servers` | List all registered servers with health status |
| `POST` | `/servers` | Add a new server at runtime (JSON body with ServerConfig fields) |
| `DELETE` | `/servers/{name}` | Remove a server (drain → disconnect → delete from registry) |
| `PUT` | `/servers/{name}` | Update a server's config (drain → reconnect with new settings) |
| `POST` | `/servers/{name}/disable` | Deactivate a server (drain → disconnect → keep in registry as disabled) |
| `POST` | `/servers/{name}/enable` | Reactivate a disabled server (cold → initialize → index rebuild) |
| `GET` | `/servers/{name}/tools` | List tools for a specific server |
| `GET` | `/sessions` | List active sessions |
| `GET` | `/sessions/{id}` | Session details (budget, active requests, etc.) |
| `DELETE` | `/sessions/{id}` | Force-close a session |
| `GET` | `/tools` | Global tool index (all tools, all servers) |
| `GET` | `/tools?q={keyword}` | Search tools by keyword |
| `POST` | `/config/reload` | Trigger config hot-reload (diff + apply) |

**Metrics (Prometheus format):**

```
scp_sessions_active                    # gauge: current active sessions
scp_servers_total                      # gauge: total registered servers
scp_servers_healthy                    # gauge: servers in healthy state
scp_servers_disabled                   # gauge: servers in disabled state
scp_requests_total{server,tool}        # counter: requests per server/tool
scp_request_duration_seconds{server}   # histogram: request latency
scp_tokens_received_total{server}      # counter: raw tokens before filtering
scp_tokens_delivered_total{server}     # counter: tokens after filtering
scp_tokens_saved_total{server}         # counter: tokens filtered out
scp_filter_ratio{server}               # gauge: delivered/received ratio
scp_tool_index_size                    # gauge: total tools in index
scp_tool_index_rebuild_total           # counter: index rebuilds
scp_pool_connections_active{server}    # gauge: active connections per server
scp_pool_queue_depth{server}           # gauge: pending requests in pool queue
```

`scp_tokens_saved_total` is the primary value metric — it shows exactly how much context budget SCP is saving.


## 6. Transport Details

### 6.1 stdio (Client-facing)

When SCP is used as a stdio server (e.g., configured in OpenCode or Claude Code's MCP settings), the host application spawns the SCP binary as a child process. SCP reads JSON-RPC from stdin, writes to stdout. stderr is used for logging only (never protocol messages).

One stdio client-facing connection = one session. There is no multiplexing on the client side over stdio.

### 6.2 stdio (Server-facing)

SCP spawns backend stdio servers as child processes. Each spawned process gets:
- Inherited environment variables from the server config's `env` map.
- A working directory (configurable, defaults to hub's working dir).
- stdout captured for JSON-RPC responses.
- stderr captured and forwarded to SCP's logging system (with server name prefix).
- A process group, so SCP can kill the process and all its children on cleanup.

**Process lifecycle concerns:**
- Zombie processes: SCP must `wait()` on all spawned children. Use `tokio::process::Child` which handles this.
- Crash recovery: if a stdio server's process exits unexpectedly, SCP marks it `failed`, cleans up in-flight requests with error responses, and will attempt restart on next request (up to `max_retries`).
- Signal handling: on SIGTERM/SIGINT, SCP sends SIGTERM to all child processes, waits up to 5 seconds, then SIGKILL.

### 6.3 SSE / Streamable HTTP (Client-facing)

SCP listens on a configurable HTTP endpoint. Both legacy SSE and Streamable HTTP are supported on the same port:

- Legacy SSE: client connects to `GET /sse` for the event stream, sends requests via `POST /messages`.
- Streamable HTTP: client sends requests via `POST /mcp` with optional `Accept: text/event-stream` for streaming responses.

Each HTTP connection (identified by session token in the `Authorization` header or a session cookie) maps to one session. Multiple HTTP requests from the same session share the same session state.

### 6.4 SSE / Streamable HTTP (Server-facing)

SCP connects to backend SSE/HTTP servers as a client. Connection management:
- Automatic reconnection with exponential backoff (1s, 2s, 4s, 8s, max 30s).
- SSE heartbeat monitoring: if no event (including comments/keepalives) for 60 seconds, reconnect.
- HTTP connection pooling via `hyper` connection pool (shared strategy) or dedicated connections (dedicated strategy).


## 7. Configuration

### 7.1 File Format

```toml
config_version = 1

# ─── Hub configuration ───

[hub]
listen_address = "127.0.0.1"
listen_port = 3100
transports = ["stdio", "sse", "streamable_http"]  # which client transports to accept
max_clients = 50
session_timeout_secs = 3600
shutdown_timeout_secs = 30

# Default budgets (overridable per session profile)
[hub.defaults]
token_budget_per_request = 4000
token_budget_per_session = 64000
budget_strategy = "per_request"     # per_request | per_turn | sliding_window | manual
max_tools_exposed = 20
fanout_timeout_ms = 5000
rate_limit_per_minute = 100
rate_limit_burst = 20

# Authentication
[hub.auth]
method = "bearer"                    # "none" | "bearer"
tokens_file = "./auth_tokens.json"   # hot-reloadable, format: { "token": "profile_name" }

# Session profiles (different clients can have different settings)
[hub.profiles.default]
token_budget_per_request = 4000
max_tools_exposed = 20
tool_scope = "all"                   # "all" | list of qualified tool names or glob patterns

[hub.profiles.opencode]
token_budget_per_request = 8000
max_tools_exposed = 40
tool_scope = "all"
rate_limit_per_minute = 500          # power user: higher limit
rate_limit_burst = 50

[hub.profiles.lightweight]
token_budget_per_request = 1000
max_tools_exposed = 10
tool_scope = ["filesystem.*", "chromadb.search"]
rate_limit_per_minute = 50           # restricted client

# ─── Tool Index ───

[tool_index]
engine = "tfidf"                     # "none" | "tags" | "tfidf" | "embedding"
max_tools_per_list = 20              # hard cap on tools/list responses
always_include = ["filesystem.read_file", "chromadb.search"]  # bypass scoring
rebuild_debounce_ms = 500            # debounce rapid list_changed notifications

[tool_index.embedding]
model = "nomic-embed-text"
endpoint = "http://localhost:11434/api/embed"
cache_embeddings = true              # persist embeddings to disk (avoid re-computing on restart)
cache_path = "./data/embeddings.bin"

[tool_index.scoring_weights]
primary = 0.7                        # weight for the main scoring engine
usage = 0.3                          # weight for usage frequency

# ─── Tool aliases (collision resolution) ───

[tool_aliases]
"search" = "chromadb.search"
"read" = "filesystem.read_file"

# ─── Filter Pipeline ───

[filter]
enabled = true                       # false = pure passthrough (for debugging)
short_circuit_below_tokens = 500     # skip filtering for small responses

[filter.chunking]
strategy = "paragraph"               # "paragraph" | "line" | "json_element" | "fixed_size"
fixed_size_tokens = 200              # only for fixed_size strategy
overlap_tokens = 20                  # overlap between fixed-size chunks

[filter.relevance]
engine = "tfidf"                     # "keyword" | "tfidf" | "embedding" (matches tool_index.engine)

[filter.budget]
strategy = "truncate"                # "truncate" | "summarize" | "hybrid"
min_tokens_per_response = 200        # never truncate below this

[filter.budget.summarize]
model = "qwen3:1.7b"
endpoint = "http://localhost:11434/api/generate"
max_latency_ms = 500
fallback = "truncate"

[filter.progressive_disclosure]
enabled = true
hint_text = "[SCP: {shown} of {total} results shown. Call scp_get_more(request_id=\"{id}\") for more.]"

# ─── Server definitions ───

[[servers]]
name = "filesystem"
transport = "stdio"
command = ["mcp-server-filesystem", "/home/elias", "/data"]
env = { "NODE_OPTIONS" = "--max-old-space-size=256" }
sharing = "pooled"
pool_size = 3
pool_max_queue = 10
priority = "medium"
tags = ["files", "code", "read", "write"]
idle_timeout_secs = 120
request_timeout_secs = 30
max_retries = 3
health_check_interval_secs = 30
# enabled = true  (default, can be omitted)

[[servers]]
name = "chromadb"
transport = "sse"
url = "http://localhost:8100/mcp"
sharing = "shared"
priority = "high"
tags = ["search", "memory", "embeddings", "semantic"]
request_timeout_secs = 10
max_retries = 2

[[servers]]
name = "notion"
transport = "streamable_http"
url = "https://mcp.notion.com/mcp"
sharing = "shared"
priority = "medium"
tags = ["docs", "notes", "wiki", "knowledge"]
request_timeout_secs = 15
headers = { "Authorization" = "Bearer ${NOTION_TOKEN}" }  # env var interpolation

[[servers]]
name = "mempalace"
transport = "stdio"
command = ["mempalace-mcp"]
sharing = "dedicated"
priority = "high"
tags = ["memory", "context", "personal"]
idle_timeout_secs = 600
request_timeout_secs = 10

[[servers]]
name = "experimental-rag"
transport = "stdio"
command = ["rag-mcp", "--db", "/data/rag.db"]
sharing = "shared"
priority = "low"
tags = ["search", "rag"]
enabled = false  # registered but disabled — activate later via API/CLI/config reload

# ─── Admin API ───

[admin]
enabled = true
listen_address = "127.0.0.1"
listen_port = 3101
auth_token = "${SCP_ADMIN_TOKEN}"    # env var interpolation

# ─── Logging ───

[logging]
level = "info"                       # trace | debug | info | warn | error
format = "json"                      # "json" | "pretty"
file = "./logs/scp.log"              # optional file output (in addition to stderr)

# ─── Telemetry ───

[telemetry]
enabled = false                      # enable OTLP trace export
exporter = "otlp"                    # "none" | "stdout" | "otlp"
otlp_endpoint = "http://localhost:4317"  # gRPC endpoint (Jaeger, Tempo, etc.)
service_name = "scp-hub"
sample_rate = 1.0                    # 1.0 = all, 0.1 = 10%

# ─── Namespacing ───

[namespacing]
resources = "on_collision"           # "always" | "on_collision" | "never"
prompts = "on_collision"             # "always" | "on_collision" | "never"
tools = "on_collision"               # "always" | "on_collision" | "never"
```

### 7.2 Hot Reload

On `SIGHUP` or `POST /config/reload`:

| Config section | Hot-reloadable? | Notes |
|---|---|---|
| `hub.listen_*` | No | Requires restart (socket is already bound) |
| `hub.max_clients` | Yes | New limit applies to new connections |
| `hub.auth` | Yes | Tokens file re-read, existing sessions unaffected |
| `hub.profiles` | Yes | Existing sessions keep old profile, new sessions get new |
| `hub.defaults` | Yes | Only affects new sessions |
| `tool_index.*` | Yes | Triggers full index rebuild |
| `tool_aliases` | Yes | Triggers tool name remapping |
| `filter.*` | Yes | Pipeline reconfigured, in-flight requests finish with old config |
| `servers` (add) | Yes | New server initialized and tools indexed |
| `servers` (remove) | Yes | Server drained and removed |
| `servers` (disable via `enabled = false`) | Yes | Server drained, tools removed from index, entry kept |
| `servers` (enable via `enabled = true`) | Yes | Server reconnected, tools re-indexed |
| `servers` (modify) | Partial | Transport/command changes require drain+restart. Timeouts/tags update in place. |
| `admin.*` | No | Requires restart |
| `logging.*` | Yes | Log level and format update immediately |
| `telemetry.*` | Partial | `sample_rate` updates in place. `exporter`/`otlp_endpoint` changes require restart |
| `namespacing.*` | Yes | Triggers tool/resource/prompt index rebuild + client notification |

**Concurrent reload protection:** Only one reload can execute at a time. A `tokio::Mutex` guards the reload path. If a second `SIGHUP` or `POST /config/reload` arrives while a reload is in progress, it is queued and executed after the current reload completes. The admin API returns `409 Conflict` if a reload is already running.

**Reload sequence:**
1. Acquire reload lock.
2. Read and parse new config file. If parse fails → release lock, return error, keep running with old config.
3. Validate new config (no duplicate server names, valid transports, all required fields). If validation fails → release lock, return error.
4. Diff new config against current runtime state.
5. Apply changes in order: remove servers → disable servers → update servers → add servers → enable servers → apply non-server changes.
6. Each server state transition completes fully (including drain) before the next begins.
7. After all server changes: rebuild tool/resource/prompt indexes.
8. Emit `notifications/tools/list_changed` to all connected clients.
9. Release reload lock.

### 7.3 Config Schema Versioning

The config file includes a `config_version` field at the top level:

```toml
config_version = 1
```

When the config schema changes in a breaking way, the version number is incremented. SCP validates `config_version` on startup and reload:

- **Missing `config_version`** — treated as version 1 (backward compatible with initial configs).
- **Known version** — parsed with the corresponding schema.
- **Unknown future version** — SCP refuses to start and prints a clear error: `"Config version 3 requires SCP v0.6+. You are running v0.4. Please upgrade."`.

Non-breaking additions (new optional fields with defaults) do NOT increment the version. Only removals, renames, or semantic changes to existing fields require a version bump. A `CHANGELOG.md` documents all config changes with migration instructions.

### 7.4 Environment Variable Interpolation

Any string value in the config can use `${ENV_VAR}` syntax. The hub resolves these at startup and on hot-reload. Missing env vars cause a startup error (fail-fast, don't silently use empty strings).

### 7.5 Config Validation

On startup and hot-reload, the hub validates:
- No duplicate server names.
- All tool aliases reference tools that exist (warning, not error — tool may appear later).
- Pool sizes are ≥1.
- Timeouts are positive.
- Scoring engine in filter matches tool_index engine (warning if mismatched).
- Auth tokens file exists and is valid JSON (if auth enabled).
- Endpoint URLs are parseable.
- stdio commands are resolvable in PATH.


## 8. Error Handling

### 8.1 Backend Server Errors

| Error type | SCP behavior |
|---|---|
| Server process crashes (stdio) | Return MCP error response to client. Mark server `failed`. Clean up in-flight requests. Log crash with stderr output. |
| Server connection refused (SSE/HTTP) | Retry with backoff. After `max_retries`, return MCP error. Mark server `failed`. |
| Server timeout (no response within `request_timeout`) | Return MCP error with timeout reason. Cancel request on server side if possible. |
| Server returns malformed JSON-RPC | Return MCP internal error to client. Log malformed response. Increment error counter. |
| Server returns MCP error response | Forward error to client unchanged (SCP is transparent for errors). |
| All pool instances busy + queue full | Return MCP error with "server overloaded" reason. |

Error responses use standard MCP error codes:
- `-32000`: Server error (crash, timeout)
- `-32001`: Server overloaded (pool exhausted)
- `-32002`: Server unhealthy (failed health check)
- `-32003`: Rate limit exceeded (per-session throttle)
- `-32603`: Internal error (SCP bug)

### 8.2 Client Errors

| Error type | SCP behavior |
|---|---|
| Client sends malformed JSON-RPC | Return JSON-RPC parse error (-32700) |
| Client sends unknown method | Return method not found (-32601) |
| Client requests tool that doesn't exist | Return invalid params (-32602) with "unknown tool" message |
| Client disconnects mid-request | Cancel in-flight backend requests. Clean up session. |
| Client auth fails | Return HTTP 401 (SSE/HTTP) or close stdio with error message |

### 8.3 SCP Internal Errors

| Error type | SCP behavior |
|---|---|
| Embedding endpoint down | Fall back to TF-IDF scoring. If TF-IDF not configured, fall back to tag scoring. If no scoring available, return all tools unfiltered (degrade gracefully, never block). |
| Summarization endpoint down | Fall back to truncation strategy. |
| Session store full (max_clients reached) | Reject new connections with "server busy" error. |
| Out of memory | SCP should have bounded memory usage (capped data structures). If OOM occurs anyway, it's a bug. |
| Config reload fails validation | Keep old config. Log error. Return error on admin API. |

**Graceful degradation principle:** SCP should always prefer delivering unfiltered content over delivering nothing. If any filtering stage fails, it is bypassed, not retried. The client gets a larger-than-ideal response, which is always better than an error.


## 9. SCP Extensions (Optional MCP Protocol Additions)

These are optional extensions that SCP-aware clients can use for better results. Non-SCP-aware clients simply never send these and everything works normally.

### 9.1 Intent Hints

A client can include an `_scp` field in tool call arguments to hint at intent:

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

The `_scp` field is stripped before forwarding to the backend server (which wouldn't understand it). SCP uses the intent for relevance scoring and the max_tokens as an explicit budget override.

### 9.2 Budget Control

A client can query and manage its budget:

```json
// Synthetic tool exposed by SCP
{ "method": "tools/call", "params": { "name": "scp_budget", "arguments": {} } }
// Returns: { "remaining": 3200, "total": 4000, "strategy": "per_request" }

// Reset budget
{ "method": "tools/call", "params": { "name": "scp_budget_reset", "arguments": {} } }
```

### 9.3 Progressive Disclosure Retrieval

When responses are truncated, SCP injects a hint. The client can retrieve more:

```json
{ "method": "tools/call", "params": { "name": "scp_get_more", "arguments": { "request_id": "scp-a3f9-001", "offset": 15, "limit": 15 } } }
```

This returns the next batch of chunks from the cached full response, filtered and budgeted.

### 9.4 Model Declaration

A client can declare its model for more accurate token counting:

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

### 9.5 Extension Discovery

SCP exposes its extensions via a synthetic tool:

```json
{ "method": "tools/call", "params": { "name": "scp_info", "arguments": {} } }
// Returns: { "version": "0.3.0", "extensions": ["intent_hints", "budget_control", "progressive_disclosure", "model_declaration"], "servers": 42, "tools": 187 }
```

All `scp_*` tools are clearly namespaced and documented. Non-SCP clients never call them, so they appear as extra tools in the list (scored low by the tool index if irrelevant to the context).


## 10. Security

### 10.1 Client Authentication

| Method | Transport | Details |
|---|---|---|
| None | stdio | Implicit trust (client spawned the hub or vice versa) |
| Bearer token | SSE / HTTP | `Authorization: Bearer <token>` header. Tokens map to session profiles. |
| mTLS | HTTPS | Client certificate validation. For production/team deployments. |

### 10.2 Backend Server Security

- stdio servers: implicitly trusted (SCP spawns them as child processes).
- SSE/HTTP servers: SCP forwards configured auth headers (from server config). SCP does NOT forward client auth to backend servers — SCP authenticates to backends independently.
- Env var interpolation for secrets (`${NOTION_TOKEN}`) — secrets never appear in config files.

### 10.3 Input Validation

- All incoming JSON-RPC is validated against the MCP schema before processing.
- Tool arguments are validated against the tool's declared JSON Schema before forwarding.
- Maximum request size: 10MB (configurable). Requests exceeding this are rejected.
- Maximum response size from backends: 50MB (configurable). Responses exceeding this are truncated with a warning.

### 10.4 Network Exposure

- By default, SCP binds to `127.0.0.1` (localhost only).
- For remote access (e.g., over Headscale), bind to `0.0.0.0` with auth enabled.
- Admin API always binds to localhost by default. Override requires explicit config.
- TLS termination: SCP itself does not handle TLS. Use a reverse proxy (caddy, nginx) or VPN (Headscale/Tailscale) for encryption in transit.

### 10.5 Isolation

- Sessions are fully isolated. One client cannot access another client's session state, delivery history, or budget.
- Backend server connections for `dedicated` strategy are session-scoped.
- Backend server connections for `shared` and `pooled` strategies are shared, but response routing ensures no cross-session data leakage (responses are routed by request ID, which is session-scoped).


## 11. Crate Structure

```
scp/
├── Cargo.toml                    # workspace root
├── Dockerfile
├── docker-compose.yml
├── config.example.toml           # documented example config
├── README.md
│
├── .github/
│   └── workflows/
│       ├── ci.yml                # check, clippy, test on push/PR
│       ├── docker.yml            # build + push Docker image to ghcr.io
│       └── release.yml           # cross-compile binaries, create GitHub Release on tag
│
├── scp-core/                     # shared types, traits, protocol
│   └── src/
│       ├── lib.rs
│       ├── protocol/
│       │   ├── mod.rs
│       │   ├── jsonrpc.rs        # JSON-RPC 2.0 types (Request, Response, Error, Notification)
│       │   ├── mcp.rs            # MCP method types (ToolsListRequest, ToolsCallRequest, etc.)
│       │   ├── capabilities.rs   # ClientCapabilities, ServerCapabilities
│       │   └── transport.rs      # Transport trait (send, receive, close)
│       ├── session.rs            # Session, SessionId, SessionConfig, SessionProfile
│       ├── budget.rs             # TokenBudget, BudgetStrategy, BudgetAllocator
│       ├── server.rs             # ServerEntry, SharingStrategy, HealthState, Priority
│       ├── filter.rs             # ContextFilter trait, FilterContext, FilterChain
│       ├── tool.rs               # ToolEntry, QualifiedToolName, ToolAlias
│       ├── error.rs              # ScpError enum (transport, routing, filter, config)
│       └── config.rs             # Config structs (deserialized from TOML)
│
├── scp-transport/                # transport implementations
│   └── src/
│       ├── lib.rs
│       ├── stdio.rs              # StdioTransport (spawn process, read/write JSON-RPC)
│       ├── sse.rs                # SseTransport (legacy SSE client + server)
│       ├── http.rs               # StreamableHttpTransport (Streamable HTTP client + server)
│       └── listener.rs           # ClientListener (accept stdio / SSE / HTTP connections)
│
├── scp-pool/                     # connection pooling and lifecycle
│   └── src/
│       ├── lib.rs
│       ├── manager.rs            # PoolManager (owns all server connections)
│       ├── shared.rs             # SharedPool (single connection, request serialization)
│       ├── pooled.rs             # WorkerPool (N instances, least-outstanding dispatch)
│       ├── dedicated.rs          # DedicatedPool (per-session instances)
│       ├── health.rs             # HealthChecker (periodic pings, failure tracking)
│       └── lifecycle.rs          # cold/starting/warm/hot/draining/failed transitions
│
├── scp-index/                    # tool/resource/prompt registry + scoring
│   └── src/
│       ├── lib.rs
│       ├── registry.rs           # ToolRegistry, ResourceRegistry, PromptRegistry
│       ├── scorer.rs             # ToolScorer trait + ScoringPipeline
│       ├── tags.rs               # TagScorer (Jaccard similarity)
│       ├── tfidf.rs              # TfIdfScorer (cosine similarity on descriptions)
│       ├── embedding.rs          # EmbeddingScorer (Ollama API, cosine similarity)
│       ├── usage.rs              # UsageTracker (call frequency, Bayesian scoring)
│       └── alias.rs              # ToolAliasResolver (collision handling, aliasing)
│
├── scp-filter/                   # response filtering pipeline
│   └── src/
│       ├── lib.rs
│       ├── pipeline.rs           # FilterPipeline (ordered chain, stage orchestration)
│       ├── content_type.rs       # ContentTypeRouter (text/json/image/binary classification)
│       ├── token_count.rs        # TokenEstimator trait + heuristic + tiktoken implementations
│       ├── chunker.rs            # ChunkSplitter (paragraph, line, json_element, fixed_size)
│       ├── dedup.rs              # DedupFilter (content hashing, delivery log check)
│       ├── relevance.rs          # RelevanceScorer (keyword, tfidf, embedding)
│       ├── budget.rs             # BudgetEnforcer (truncate, summarize, hybrid)
│       ├── summarize.rs          # Summarizer (Ollama API wrapper, latency tracking)
│       ├── progressive.rs        # ProgressiveDisclosure (annotation, scp_get_more cache)
│       └── delivery_log.rs       # DeliveryLog (bounded hash set with LRU eviction)
│
├── scp-hub/                      # main binary: orchestration
│   └── src/
│       ├── main.rs               # entry point, signal handling, graceful shutdown
│       ├── hub.rs                # Hub struct (owns session store, pool manager, router, index)
│       ├── session_store.rs      # SessionStore (create, get, expire, list)
│       ├── router.rs             # Router (request dispatch, fan-out, request ID mapping)
│       ├── server_manager.rs     # ServerManager impl (add/remove/disable/enable/update at runtime)
│       ├── admin.rs              # Admin HTTP API (axum router)
│       ├── metrics.rs            # Prometheus metrics registry
│       ├── tracing_setup.rs      # tracing-subscriber + OTLP exporter setup
│       └── reload.rs             # Config hot-reload logic (SIGHUP handler, diff + apply)
│
├── scp-cli/                      # CLI interface
│   └── src/
│       └── main.rs               # Subcommands: start, status, servers, sessions, tools, reload
│
└── tests/                        # integration tests
    ├── common/
    │   ├── mock_mcp_server.rs    # configurable mock MCP server (stdio + HTTP)
    │   └── test_client.rs        # test MCP client
    ├── passthrough_test.rs       # Phase 0: transparent proxy
    ├── multi_server_test.rs      # Phase 1: routing, fan-out
    ├── server_lifecycle_test.rs  # Phase 1: add/remove/disable/enable at runtime
    ├── multi_client_test.rs      # Phase 2: session isolation
    ├── tool_index_test.rs        # Phase 3: scoring, filtering
    ├── filter_pipeline_test.rs   # Phase 4: relevance filtering
    └── budget_test.rs            # budget allocation and enforcement
```


## 12. Implementation Roadmap

### Phase 0 — MCP Passthrough (v0.1.0)

**Goal:** A working MCP proxy that forwards everything unchanged over stdio. Prove the JSON-RPC plumbing works end-to-end.

**Deliverables:**

- [ ] Cargo workspace with all crates stubbed (lib.rs with `// TODO` for each).
- [ ] `scp-core/protocol`: JSON-RPC 2.0 request/response/notification types with serde.
- [ ] `scp-core/protocol`: MCP method types for `initialize`, `initialized`, `tools/list`, `tools/call`, `ping`.
- [ ] `scp-transport/stdio`: spawn a child process, send JSON-RPC over stdin, read from stdout, capture stderr for logging.
- [ ] `scp-hub`: single-client stdio listener → single-server stdio backend → pure passthrough. No sessions, no filtering, no routing logic. Just: read from client stdin, write to server stdin, read from server stdout, write to client stdout.
- [ ] `scp-hub`: proper `initialize` / `initialized` handshake on both sides. SCP performs `initialize` with the backend server and returns the server's capabilities to the client.
- [ ] `scp-hub`: request ID remapping (client IDs ≠ server IDs, even in passthrough).
- [ ] `tracing` setup with `tracing-subscriber` (JSON or pretty output). Log every request/response at `debug` level, every error at `error` level.
- [ ] Integration test: `tests/passthrough_test.rs` — mock MCP server (stdio) that returns canned responses. Test: client sends `tools/list`, gets the server's tools. Client sends `tools/call`, gets the response. Client sends `ping`, gets pong. Verify request ID remapping.
- [ ] End-to-end manual test: OpenCode → SCP → `mcp-server-filesystem` → file contents flow back.

**Exit criteria:** OpenCode can use SCP as a drop-in replacement for a direct MCP server connection with zero behavioral difference.

### Phase 1 — Multi-Server + Basic Filtering (v0.2.0)

**Goal:** Multiple backend servers, tool routing, token-based truncation, and runtime server management.

**Deliverables:**

- [ ] `scp-core/config`: TOML config parsing with serde. Validation on startup.
- [ ] `scp-core/server`: ServerEntry, SharingStrategy enum, HealthState enum (including `disabled` state).
- [ ] `scp-transport/sse`: SSE client (connect to backend SSE servers, handle events, send POSTs).
- [ ] `scp-pool/shared`: SharedPool — single connection per server, request serialization via tokio::Mutex, response demux by request ID.
- [ ] `scp-pool/manager`: PoolManager — owns one pool per server, routes requests by server name.
- [ ] `scp-pool/lifecycle`: cold/warm/hot/draining/disabled states. Lazy startup (cold → starting on first request). Idle timeout (warm → cold after inactivity).
- [ ] `scp-index/registry`: ToolRegistry — collect tools from all servers, store ToolEntry with qualified names. Rebuild on server add/remove/disable/enable.
- [ ] `scp-index/alias`: Tool name collision detection. Log warning. Apply configured aliases. Fall back to suffix numbering.
- [ ] `scp-hub/router`: Route `tools/call` by tool name → server lookup. Route `tools/list` with fan-out across all servers.
- [ ] `scp-filter/token_count`: Heuristic token estimator (bytes / 3.5).
- [ ] `scp-filter/budget`: Hard truncation at token budget. Truncate from the end. Append `[truncated by SCP: {original_tokens} → {delivered_tokens} tokens]`.
- [ ] `scp-filter/pipeline`: Two-stage pipeline: token_count → budget (truncate). Short-circuit for small responses.
- [ ] `scp-hub/server_manager`: Implement `ServerManager` trait — `add_server`, `remove_server`, `disable_server`, `enable_server` with full drain/reconnect lifecycle as specified in §5.2.
- [ ] `scp-hub/admin`: Admin HTTP API (server management subset): `GET/POST/DELETE /servers`, `POST /servers/{name}/disable`, `POST /servers/{name}/enable`, `POST /config/reload`.
- [ ] `scp-hub/reload`: Config hot-reload on `SIGHUP` signal — diff current vs. new config, add/remove/update/disable/enable servers accordingly. Validate before applying.
- [ ] `scp-hub`: Servers with `enabled = false` in config start in `disabled` state (registered but not connected, tools hidden).
- [ ] `scp-hub`: On any server add/remove/disable/enable, emit `notifications/tools/list_changed` to all connected clients.
- [ ] `scp-cli`: `scp start --config path/to/config.toml` and `scp status` (list servers, tool count, health).
- [ ] `scp-cli`: `scp servers add <name> --transport stdio --command "..." [--tags ...]` — add a server at runtime.
- [ ] `scp-cli`: `scp servers remove <name>` — drain and remove a server at runtime.
- [ ] `scp-cli`: `scp servers disable <name>` / `scp servers enable <name>` — toggle without removing.
- [ ] `scp-cli`: `scp servers list` — show all servers with state (cold/warm/hot/disabled/failed).
- [ ] Integration test: two mock servers with overlapping tool names. Verify routing and aliasing.
- [ ] Integration test: large response truncated to configured budget.
- [ ] Integration test: add a server at runtime via admin API → verify tools appear in client's next `tools/list`.
- [ ] Integration test: disable a server at runtime → verify tools disappear from client's `tools/list`. Re-enable → tools reappear.
- [ ] Integration test: remove a server with in-flight requests → verify draining completes before removal.

**Exit criteria:** SCP proxies between OpenCode and 3+ real MCP servers. Tool calls route correctly. Large responses are truncated. Servers can be added, removed, disabled, and re-enabled at runtime without restart or disruption to connected clients.

### Phase 2 — Multi-Client + Sessions (v0.3.0)

**Goal:** Multiple simultaneous clients with isolated sessions and per-session budgets.

**Deliverables:**

- [ ] `scp-transport/http`: Streamable HTTP server (accept client connections). Both legacy SSE and Streamable HTTP on the same listener.
- [ ] `scp-transport/listener`: ClientListener — accept stdio (single) and HTTP (multiple) clients.
- [ ] `scp-hub/session_store`: SessionStore — create sessions on `initialize`, lookup by ID, expire on timeout or disconnect.
- [ ] `scp-core/session`: Session struct with per-client budget, request ID map, client capabilities, roots.
- [ ] `scp-pool/pooled`: WorkerPool — N instances, least-outstanding-requests dispatch.
- [ ] `scp-pool/dedicated`: DedicatedPool — per-session instances, created on first route, destroyed on session end.
- [ ] `scp-hub`: bearer token auth for HTTP clients. Token → session profile mapping.
- [ ] `scp-hub/router`: Request ID mapping per session. Cancellation forwarding.
- [ ] `scp-hub/router`: Sampling request routing (server→client, routed to originating session).
- [ ] `scp-hub/router`: Roots handling (per-session roots forwarded to dedicated backends; for shared backends, roots from the requesting session are used).
- [ ] `scp-cli`: `scp sessions` (list active sessions with ID, profile, budget remaining, connected since).
- [ ] Integration test: two mock clients sending concurrent requests. Verify session isolation (budget, request IDs, responses never cross).
- [ ] Integration test: dedicated server — verify each client gets its own instance.

**Exit criteria:** Three clients (two HTTP, one stdio) connected simultaneously, using overlapping tools, with fully isolated sessions. No cross-session data leakage.

### Phase 3 — Tool Index + Smart Selection (v0.4.0)

**Goal:** Clients see a relevant subset of tools, not the full list.

**Deliverables:**

- [ ] `scp-index/tags`: TagScorer — Jaccard similarity between server tags and accumulated session keywords.
- [ ] `scp-index/tfidf`: TfIdfScorer — build TF-IDF vectors from tool descriptions at index build time. Score against session context keywords at query time.
- [ ] `scp-index/usage`: UsageTracker — count tool calls per session profile, use as scoring signal.
- [ ] `scp-index/scorer`: ScoringPipeline — weighted combination of scoring engines.
- [ ] `scp-core/session`: KeywordAccumulator — extract keywords from tool call arguments, decay old keywords over time.
- [ ] `scp-hub/router`: `tools/list` returns top-N scored tools per session. Always include `always_include` tools. Always include tools from `high` priority servers.
- [ ] `scp-hub/router`: `tools/list` cache with invalidation on `tools/list_changed` and TTL fallback.
- [ ] Handle `notifications/tools/list_changed` from backends — rebuild index, notify clients.
- [ ] `scp-cli`: `scp tools` (list all tools), `scp tools search <keyword>` (search by keyword).
- [ ] Integration test: 50 mock tools, verify that `tools/list` returns ≤20 based on session context.

**Exit criteria:** A client doing filesystem-related work sees filesystem tools ranked higher. A client doing search-related work sees search tools ranked higher. Tool list size is bounded.

### Phase 4 — Relevance Filtering (v0.5.0)

**Goal:** Large responses are filtered by relevance, not just truncated.

**Deliverables:**

- [ ] `scp-filter/chunker`: ChunkSplitter — paragraph-based for prose, line-based for logs/code, JSON element-based for structured data. Auto-detect strategy based on content analysis.
- [ ] `scp-filter/content_type`: ContentTypeRouter — classify response content and route non-text through bypass.
- [ ] `scp-filter/relevance`: RelevanceScorer — TF-IDF scoring of chunks against session context (tool args + keywords).
- [ ] `scp-filter/budget`: Smart budget enforcer — select top-k chunks by relevance score that fit budget.
- [ ] `scp-filter/dedup`: DedupFilter — SHA-256 hash chunks, check against delivery log, skip already-sent content.
- [ ] `scp-filter/delivery_log`: Bounded LRU hash set (cap 10,000 entries).
- [ ] `scp-filter/pipeline`: Full pipeline: content_type → token_count → dedup → chunk → relevance → budget → delivery_log.
- [ ] `scp-hub/metrics`: Add `scp_tokens_saved_total` metric.
- [ ] Integration test: 10KB log file response, filtered to relevant lines based on tool call arguments.
- [ ] Integration test: repeated tool calls, verify dedup drops already-sent content.

**Exit criteria:** Measurable token savings (visible in metrics). A `read_file` on a large log with a search intent returns only relevant lines.

### Phase 5 — Embeddings + Progressive Disclosure (v0.6.0)

**Goal:** Semantic scoring and lazy detail loading.

**Deliverables:**

- [ ] `scp-index/embedding`: EmbeddingScorer — call Ollama `/api/embed` for tool descriptions. Cache embeddings to disk. Cosine similarity scoring.
- [ ] `scp-filter/relevance`: Embedding-based chunk scoring (embed chunks at filter time, compare to context embedding).
- [ ] `scp-filter/progressive`: ProgressiveDisclosure — when chunks are dropped, cache the full response. Inject `scp_get_more` hint in response. Register cached response in session.
- [ ] `scp-hub/router`: Handle `scp_get_more` tool calls — retrieve cached response, return next batch through the filter pipeline.
- [ ] SCP extension tools: `scp_info`, `scp_budget`, `scp_budget_reset`, `scp_get_more` — implemented as synthetic tools in the hub.
- [ ] Intent hint support: parse `_scp` field from tool arguments, use `intent` for scoring, use `max_tokens` as budget override. Strip `_scp` before forwarding to backend.
- [ ] Graceful fallback: if Ollama embedding endpoint is down, fall back to TF-IDF. Log warning, increment `scp_embedding_fallback_total` metric.
- [ ] Integration test: semantic scoring produces different rankings than TF-IDF for ambiguous queries.

**Exit criteria:** Progressive disclosure works end-to-end. Client can retrieve more results. Embedding scoring is measurably better than TF-IDF for relevance (manual evaluation on 10 test cases).

### Phase 6 — Observability + Production Monitoring (v0.7.0)

**Goal:** Full metrics, dashboards, distributed tracing, and operational visibility.

**Deliverables:**

- [ ] `scp-hub/admin`: Remaining admin API endpoints (sessions, tools search, health — server management already in Phase 1).
- [ ] `scp-hub/metrics`: Prometheus metrics endpoint with all metrics from §5.7.
- [ ] `scp-hub/tracing`: OpenTelemetry (OTLP) trace export. Every client request gets a `trace_id` that propagates through routing, backend calls, and filter pipeline. Configurable exporter (stdout, OTLP/gRPC to Jaeger/Tempo, or none). Enables tracing a single request across all 100+ server hops.
- [ ] `scp-pool/health`: HealthChecker — periodic pings, failure counting, state transitions (healthy → degraded → unhealthy).
- [ ] `scp-cli`: Session management: `scp sessions list/kill`.
- [ ] `scp-cli`: `scp tools list/search`, `scp metrics`.
- [ ] Structured logging throughout (every request gets a trace_id, every backend call logged with server name and latency).
- [ ] Dashboard config: example Grafana dashboard JSON for the key SCP metrics.

**Tracing config:**

```toml
[telemetry]
enabled = true
exporter = "otlp"                      # "none" | "stdout" | "otlp"
otlp_endpoint = "http://localhost:4317" # gRPC endpoint (Jaeger, Tempo, etc.)
service_name = "scp-hub"
sample_rate = 1.0                       # 1.0 = trace everything, 0.1 = 10%
```

**Exit criteria:** Metrics show token savings, request latency, and server health. A single request can be traced end-to-end from client through routing, backend calls, and filter pipeline via Jaeger/Tempo. Can monitor and debug the hub without reading raw logs.

### Phase 7 — Hardening + v1.0 (v1.0.0)

**Goal:** Production stability.

**Deliverables:**

- [ ] Graceful shutdown: on SIGTERM, stop accepting new connections, drain active sessions (configurable timeout, default 30s), then exit.
- [ ] Connection retry with exponential backoff + jitter for all transports.
- [ ] Comprehensive error handling: every error path returns a meaningful MCP error response (no panics, no silent drops).
- [ ] `scp-transport/http`: Streamable HTTP transport for backends (client-side).
- [ ] Fuzzing: fuzz the JSON-RPC parser with `cargo-fuzz`.
- [ ] Load testing: verify hub handles 50 concurrent clients × 100 servers without degradation.
- [ ] Resource limits: verify bounded memory usage under sustained load (no unbounded growth).
- [ ] Full integration test suite with real MCP servers (filesystem, fetch, memory).
- [ ] Documentation: README with quickstart, architecture overview, config reference.
- [ ] `config.example.toml` with every option documented inline.
- [ ] `CHANGELOG.md`.

**Exit criteria:** Runs continuously for 72 hours under synthetic load without memory growth, panics, or leaked connections.


## 13. Deployment

### 13.1 Build & Install

```bash
# Build all crates
cargo build --release

# Binary locations
./target/release/scp-hub     # main hub binary
./target/release/scp-cli     # CLI tool (or: scp-hub includes CLI as subcommands)
```

Single static binary (no runtime dependencies except libc). Cross-compile for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`.

### 13.2 Running

```bash
# Foreground (development)
scp-hub start --config ./config.toml

# As systemd service (production)
sudo cp scp-hub.service /etc/systemd/system/
sudo systemctl enable --now scp-hub
```

Example systemd unit:

```ini
[Unit]
Description=SCP Hub - Selective Context Protocol
After=network.target ollama.service

[Service]
Type=simple
User=scp
ExecStart=/usr/local/bin/scp-hub start --config /etc/scp/config.toml
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5
Environment=SCP_ADMIN_TOKEN=changeme
Environment=NOTION_TOKEN=secret_xxx

[Install]
WantedBy=multi-user.target
```

**Graceful upgrade (zero-downtime restart):**

On `SIGTERM` (sent by `systemctl restart`), SCP:
1. Stops accepting new client connections.
2. Sends a close notification to all connected clients.
3. Waits for in-flight requests to complete (up to `shutdown_timeout`, default 30s).
4. After timeout, cancels remaining in-flight requests and returns error responses.
5. Tears down all backend connections.
6. Exits.

Clients must handle disconnection and reconnect. For MCP clients like OpenCode that use stdio transport (SCP is a child process), a restart means the client restarts SCP — this is inherent to stdio and unavoidable.

For SSE/HTTP clients connecting remotely, the reconnect is automatic (SSE has built-in reconnection). To minimize disruption during upgrades:

- Use `systemctl reload scp-hub` (sends `SIGHUP`) for config changes — no restart needed.
- For binary upgrades, `systemctl restart scp-hub` is the simplest approach. The downtime window is typically < 1 second. SSE clients auto-reconnect.
- For true zero-downtime binary upgrades, a load balancer (caddy, nginx) in front of two SCP instances with blue-green switching is possible but not built-in — out of scope for v1.0.

### 13.3 Docker

**Dockerfile** (multi-stage, multi-arch):

```dockerfile
# ── Build stage ──
FROM rust:1.82-alpine AS builder
RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static
WORKDIR /build
# Cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY scp-core/Cargo.toml scp-core/
COPY scp-transport/Cargo.toml scp-transport/
COPY scp-pool/Cargo.toml scp-pool/
COPY scp-index/Cargo.toml scp-index/
COPY scp-filter/Cargo.toml scp-filter/
COPY scp-hub/Cargo.toml scp-hub/
COPY scp-cli/Cargo.toml scp-cli/
RUN mkdir -p scp-core/src scp-transport/src scp-pool/src scp-index/src \
             scp-filter/src scp-hub/src scp-cli/src && \
    for d in scp-core scp-transport scp-pool scp-index scp-filter; do echo '' > $d/src/lib.rs; done && \
    echo 'fn main() {}' > scp-hub/src/main.rs && \
    echo 'fn main() {}' > scp-cli/src/main.rs && \
    cargo build --release 2>/dev/null || true
# Build real source
COPY . .
RUN touch scp-*/src/*.rs && cargo build --release

# ── Runtime stage ──
FROM alpine:3.21
RUN apk add --no-cache ca-certificates tini
COPY --from=builder /build/target/release/scp-hub /usr/local/bin/
COPY --from=builder /build/target/release/scp-cli /usr/local/bin/

# Non-root user
RUN addgroup -S scp && adduser -S scp -G scp
USER scp

EXPOSE 3100 3101
VOLUME ["/etc/scp", "/data/scp"]
HEALTHCHECK --interval=10s --timeout=3s --retries=3 \
    CMD ["scp-cli", "health", "--admin-url", "http://localhost:3101"]

ENTRYPOINT ["tini", "--", "scp-hub", "start"]
CMD ["--config", "/etc/scp/config.toml"]
```

**Docker Compose** (`docker-compose.yml`):

```yaml
services:
  scp-hub:
    image: ghcr.io/eliasstepanik/scp-hub:latest
    container_name: scp-hub
    restart: unless-stopped
    ports:
      - "3100:3100"   # MCP listener (SSE/Streamable HTTP)
      - "3101:3101"   # Admin API
    volumes:
      - ./config.toml:/etc/scp/config.toml:ro
      - scp-data:/data/scp          # tool index cache, embedding cache, session persistence
    environment:
      - SCP_ADMIN_TOKEN=${SCP_ADMIN_TOKEN}
      - NOTION_TOKEN=${NOTION_TOKEN}
      - SCP_LOG=info                 # tracing filter directive
    networks:
      - scp-net

    # If backend MCP servers run as separate containers, add them here:
    # depends_on:
    #   - chromadb

  # Example: ChromaDB as a backend MCP server in the same compose stack
  # chromadb:
  #   image: chromadb/chroma:latest
  #   container_name: chromadb
  #   ports:
  #     - "8100:8000"
  #   volumes:
  #     - chroma-data:/chroma/chroma
  #   networks:
  #     - scp-net

volumes:
  scp-data:
  # chroma-data:

networks:
  scp-net:
    driver: bridge
```

**Notes on stdio servers in Docker:** Stdio-based MCP servers need their binaries available inside the container. Two approaches:

1. **Bundled** — install the server binaries in the Docker image (add `COPY` or `RUN npm install -g` in the Dockerfile). Works for a known, fixed set of servers.
2. **Sidecar** — run stdio servers as separate containers and connect via SSE/HTTP instead. The SCP config points to `http://server-container:port/mcp` with `transport = "sse"` or `"streamable_http"`. Preferred for production — each server is independently deployable and restartable.

For local dev on cortex, the compose stack can mount the host's server binaries via volumes or use `network_mode: host` to reach locally running MCP servers directly.

### 13.4 GitHub Actions CI/CD

The repository uses GitHub Actions for automated builds, tests, and Docker image publishing to GitHub Container Registry (`ghcr.io`).

**Workflow: `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: "-D warnings"

jobs:
  check:
    name: Check & Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - uses: Swatinem/rust-cache@v2

      - name: Format check
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: Test
        run: cargo test --all --all-features
```

**Workflow: `.github/workflows/docker.yml`**

```yaml
name: Docker Build & Push

on:
  push:
    branches: [main]
    tags: ["v*"]
  pull_request:
    branches: [main]

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository_owner }}/scp-hub

jobs:
  docker:
    name: Build & Push Docker Image
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write

    steps:
      - uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Set up QEMU (multi-arch)
        uses: docker/setup-qemu-action@v3

      - name: Log in to GitHub Container Registry
        if: github.event_name != 'pull_request'
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract metadata (tags, labels)
        id: meta
        uses: docker/metadata-action@v5
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}
          tags: |
            # On push to main → :latest + :sha-xxxxxxx
            type=raw,value=latest,enable=${{ github.ref == 'refs/heads/main' }}
            type=sha,prefix=sha-,format=short
            # On tag push (v1.0.0) → :1.0.0 + :1.0 + :1 + :latest
            type=semver,pattern={{version}}
            type=semver,pattern={{major}}.{{minor}}
            type=semver,pattern={{major}},enable=${{ !startsWith(github.ref, 'refs/tags/v0.') }}

      - name: Build and push
        uses: docker/build-push-action@v6
        with:
          context: .
          platforms: linux/amd64,linux/arm64
          push: ${{ github.event_name != 'pull_request' }}
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
          cache-from: type=gha
          cache-to: type=gha,mode=max

      - name: Summary
        if: github.event_name != 'pull_request'
        run: |
          echo "### Docker Image Published 🐳" >> $GITHUB_STEP_SUMMARY
          echo "" >> $GITHUB_STEP_SUMMARY
          echo "**Tags:**" >> $GITHUB_STEP_SUMMARY
          echo '${{ steps.meta.outputs.tags }}' | tr ',' '\n' | sed 's/^/- /' >> $GITHUB_STEP_SUMMARY
```

**Workflow: `.github/workflows/release.yml`**

```yaml
name: Release

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

jobs:
  build-binaries:
    name: Build ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-musl
            os: ubuntu-latest
            artifact: scp-hub-linux-amd64
          - target: aarch64-unknown-linux-musl
            os: ubuntu-latest
            artifact: scp-hub-linux-arm64
          - target: x86_64-apple-darwin
            os: macos-13
            artifact: scp-hub-macos-amd64
          - target: aarch64-apple-darwin
            os: macos-14
            artifact: scp-hub-macos-arm64

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross
        run: cargo install cross --locked

      - name: Build
        run: cross build --release --target ${{ matrix.target }}

      - name: Package
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/scp-hub dist/${{ matrix.artifact }}
          cp target/${{ matrix.target }}/release/scp-cli dist/${{ matrix.artifact }}-cli
          chmod +x dist/*

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: dist/

  release:
    name: Create Release
    needs: build-binaries
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/download-artifact@v4
        with:
          path: artifacts

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          files: artifacts/**/*
```

**Tagging convention:**

| Tag | Docker tags produced | Binary release |
|---|---|---|
| Push to `main` | `latest`, `sha-abc1234` | No |
| `v0.2.0` | `0.2.0`, `0.2`, `latest` | Yes (linux-amd64, linux-arm64, macos-amd64, macos-arm64) |
| `v1.0.0` | `1.0.0`, `1.0`, `1`, `latest` | Yes |
| PR | Build only (no push) | No |

**Pulling the image:**

```bash
# Latest from main
docker pull ghcr.io/eliasstepanik/scp-hub:latest

# Specific version
docker pull ghcr.io/eliasstepanik/scp-hub:0.2.0

# Specific commit
docker pull ghcr.io/eliasstepanik/scp-hub:sha-abc1234
```

### 13.5 Client Configuration

To use SCP from OpenCode, configure it as an MCP server:

```json
{
    "mcpServers": {
        "scp": {
            "command": "scp-hub",
            "args": ["start", "--config", "/path/to/config.toml", "--transport", "stdio"]
        }
    }
}
```

Or via SSE/HTTP for remote access:

```json
{
    "mcpServers": {
        "scp": {
            "url": "http://cortex.sailehd.systems:3100/mcp"
        }
    }
}
```


## 14. Performance Targets

| Metric | Target | Notes |
|---|---|---|
| Passthrough latency (no filtering) | < 2ms added | JSON parse + ID remap + JSON serialize |
| Filter pipeline latency (TF-IDF) | < 10ms | For typical response sizes (< 5KB) |
| Filter pipeline latency (embedding) | < 100ms | Dominated by Ollama API call |
| Summarization latency | < 500ms | Small local model, short summaries |
| tools/list latency (cached) | < 5ms | Scoring + sorting |
| tools/list latency (fan-out, 100 servers) | < 6s | 5s timeout + 1s overhead |
| Memory per session | < 1MB | Capped data structures |
| Memory baseline (hub, no sessions) | < 50MB | Includes tool index, config, runtime |
| Max concurrent sessions | 50+ | Configurable, bounded |
| Max backend servers | 200+ | Lazy connections, bounded pools |


## 15. Open Design Questions

### Blocking (must decide before starting that phase)

1. **MCP protocol crate** — Use `rmcp` crate or hand-roll? Need to evaluate maturity, coverage of all MCP primitives, and transport support. If `rmcp` covers `initialize`, `tools/*`, `resources/*`, `prompts/*`, `sampling/*`, `roots/*`, notifications, and cancellation over stdio + SSE + Streamable HTTP: use it. If it's missing any of these: hand-roll the subset we need and contribute upstream later. **Decision required before Phase 0 starts.**

2. **Budget unit** — Tokens are model-specific (GPT-4 tokenizes differently from Claude). The current heuristic (`bytes / 3.5`) is a rough approximation. Options: (a) keep the heuristic — simple, good enough for budget enforcement; (b) let the client declare its model via the `_scp.model` extension (§9.4) and select the matching tokenizer; (c) budget in abstract "SCP units" decoupled from any tokenizer. Trade-off: (a) is imprecise but zero-config, (b) is accurate but adds complexity, (c) is clean but unintuitive to configure. **Decision point: Phase 1.**

3. **Sampling routing ambiguity** — If a `shared` server sends a `sampling/createMessage` request, which client should it go to? Currently: the client whose request triggered the server's processing. But with shared connections and serialized requests, the server doesn't know about sessions. Possible solutions: (a) track which session's request is currently being processed per server — if the server sends a sampling request during processing, it must be for the in-flight session; (b) require `dedicated` strategy for any server that uses the sampling capability (detected at `initialize`); (c) include the SCP internal request ID in a custom field, but this breaks MCP purity. **Decision point: Phase 2.**

### Non-blocking (decide when you get there)

4. **Naming** — "SCP" collides with the SCP Foundation (widely known fictional wiki). This will cause confusion in search results and discussions. Candidates: **SCMP** (Selective Context Management Protocol), **MCP Sieve**, **Sift** (simple, memorable), **Prism** (splits the full spectrum into what you need). **Decision can be deferred to v0.3** when the project goes public.

5. **Progressive disclosure UX** — The `scp_get_more` synthetic tool works but is clunky. The model has to know to call it. Alternative: return a structured `_scp_continuation` token in the response metadata that MCP-aware clients can auto-expand. But this requires client cooperation and breaks pure MCP compatibility. **Decision point: Phase 5.**

6. **Filter pipeline ordering** — Is the current order (token_count → dedup → relevance → budget → progressive) always optimal? Could there be cases where dedup should run after relevance scoring (e.g., two slightly different chunks that are both relevant)? Should the order be configurable per server? For now: fixed order, revisit if real-world usage reveals issues.

7. **MCP Elicitation** — The 2025-03-26 spec draft mentions an elicitation capability (server asks the user for input). If this ships, SCP needs to route elicitation requests to the correct client session, similar to sampling. Same routing ambiguity applies for `shared` servers. Monitor spec evolution.

### Deferred (post-v1.0)

8. **Multi-turn context persistence** — SCP currently builds "context" from tool call patterns within a session. But sessions are short-lived. Should SCP persist usage patterns and learned relevance across sessions (per client profile)? This would make the tool index improve over time. Trade-off: complexity, storage, privacy implications. Not needed for v1.0 — raw signal from tool call patterns within a session is sufficient to start.

9. **Plugin system for custom filter stages** — Users might want to write custom filter stages (e.g., a stage that redacts PII, or a stage that reformats JSON responses). Should SCP support dynamically loaded filter plugins (WASM? shared libraries? separate processes via stdio?)? Adds significant complexity. For v1.0, custom filtering is achieved by writing a new crate that implements the `ContextFilter` trait and recompiling. Post-v1.0, a WASM plugin system could make this more accessible.

10. **Server dependency ordering** — Some backends might depend on others (e.g., a RAG server that queries ChromaDB). Currently no concept of startup ordering or dependency declaration between servers. For v1.0, this is handled externally (Docker compose `depends_on`, systemd `After=`). A built-in dependency graph would add complexity with limited benefit — backends should be independently startable.

### Resolved (addressed in plan)

These were previously open but are now specified:

- **License** → MIT OR Apache-2.0 (§1).
- **Resource URI namespacing** → `scp://{server}/` prefix on collision, configurable strategy (§3.4).
- **Prompt namespacing** → same qualified-name rules as tools (§3.4).
- **Tool name stripping on forwarding** → explicit strip-and-forward behavior specified (§5.4).
- **Rate limiting** → per-session token bucket with configurable rates (§5.1).
- **Config schema versioning** → `config_version` field with migration logic (§7.3).
- **Concurrent reload protection** → mutex-guarded reload with 409 Conflict on overlap (§7.2).
- **macOS binaries** → added to release workflow (§13.4).
- **Graceful upgrade** → documented shutdown sequence and SSE reconnect behavior (§13.2).
- **Distributed tracing** → OTLP export with configurable exporter (Phase 6, §13.6 telemetry config).
