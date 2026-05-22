# Phase 0 (v0.1) Task Breakdown — SCP MCP Passthrough

> Generated: 2026-05-22
> NOTE: task-local MCP embedding API is over monthly budget. Tasks stored here as markdown fallback.
> Exit criteria: OpenCode can use SCP as a drop-in replacement for a direct MCP server connection via stdio.

---

## P0 [CRITICAL] Phase 0 (v0.1) MCP Passthrough

### P0.1 [HIGH] Cargo Workspace Scaffold
Acceptance: `cargo build` succeeds, all 8 crates present, workspace Cargo.toml lists all members.

- **P0.1.1** [HIGH] Create root Cargo.toml workspace (resolver=2, [workspace.dependencies])
- **P0.1.2** [HIGH] Scaffold scp-core crate (lib.rs, declare modules: protocol, error)
- **P0.1.3** [HIGH] Scaffold scp-transport crate (lib.rs, declare module: stdio)
- **P0.1.4** [MEDIUM] Scaffold scp-pool crate (stub lib.rs only)
- **P0.1.5** [MEDIUM] Scaffold scp-index crate (stub lib.rs only)
- **P0.1.6** [MEDIUM] Scaffold scp-filter crate (stub lib.rs only)
- **P0.1.7** [HIGH] Scaffold scp-hub binary crate (main.rs prints version, exits cleanly)
- **P0.1.8** [LOW] Scaffold scp-cli binary crate (stub main.rs, prints help)
- **P0.1.9** [HIGH] Scaffold tests integration crate (common/mod.rs, passthrough_test.rs placeholder)

### P0.2 [CRITICAL] JSON-RPC 2.0 Types — scp-core/src/protocol.rs
Acceptance: All types serialize/deserialize correctly, unit tests pass.

- **P0.2.1** [HIGH] Define RequestId enum: String(String), Number(i64), Null — serde untagged, impl Display
- **P0.2.2** [CRITICAL] Define JsonRpcRequest struct: jsonrpc, id: Option<RequestId>, method, params: Option<Value>
- **P0.2.3** [CRITICAL] Define JsonRpcResponse struct: jsonrpc, id, result: Option<Value>, error: Option<JsonRpcError>; constructors success()/error()
- **P0.2.4** [HIGH] Define JsonRpcNotification struct: jsonrpc, method, params: Option<Value> (no id)
- **P0.2.5** [HIGH] Define JsonRpcError struct: code: i32, message, data: Option<Value>; SCP codes: -32000/-32001/-32002/-32003
- **P0.2.6** [HIGH] Define IncomingMessage enum: Request/Response/Notification; auto-detect via serde; unit tests for each variant

### P0.3 [HIGH] MCP Method Types — scp-core/src/mcp_types.rs
Acceptance: All structs derive Serialize, Deserialize, Debug, Clone.

- **P0.3.1** [HIGH] Initialize types: InitializeParams, InitializeResult, ClientCapabilities, ServerCapabilities, ClientInfo, ServerInfo
- **P0.3.2** [HIGH] Tool types: Tool, ListToolsResult, CallToolParams, CallToolResult, ToolContent (text/image/resource variants)
- **P0.3.3** [MEDIUM] Ping types: PingParams (empty), PingResult (empty)
- **P0.3.4** [MEDIUM] Notification types: InitializedNotification, ToolsListChangedNotification, CancelledNotification

### P0.4 [HIGH] Tracing Setup — scp-hub/src/tracing_setup.rs
Acceptance: JSON and pretty formats work; RUST_LOG respected; called from main() before any other work.

- **P0.4.1** [HIGH] Add tracing + tracing-subscriber (env-filter, json, fmt features) to scp-hub Cargo.toml
- **P0.4.2** [HIGH] Implement TracingFormat enum (Json, Pretty) + init_tracing(format, level) function

### P0.5 [CRITICAL] stdio Transport Server-Facing — scp-transport/src/stdio_server.rs
Acceptance: Spawn child, write/read newline-delimited JSON-RPC, handle child exit gracefully.

- **P0.5.1** [CRITICAL] StdioServerTransport struct + spawn(command: &[String], env: &HashMap<String,String>) -> Result<Self>
- **P0.5.2** [HIGH] stdin writer task: tokio task, receives from channel, writes line+\n to child stdin, flushes
- **P0.5.3** [HIGH] stdout reader task: tokio task, reads lines from child stdout via BufReader, sends to channel
- **P0.5.4** [MEDIUM] stderr logger task: reads child stderr, logs via tracing::debug!(server=%name, "server stderr")
- **P0.5.5** [CRITICAL] send(&self, msg) -> Result<()> and receive(&mut self) -> Result<IncomingMessage>; TransportError enum

### P0.6 [CRITICAL] stdio Transport Client-Facing — scp-transport/src/stdio_client.rs
Acceptance: Read from own stdin, write to own stdout; stderr never used for protocol.

- **P0.6.1** [CRITICAL] StdioClientTransport struct wrapping tokio::io::stdin() + tokio::io::stdout()
- **P0.6.2** [CRITICAL] receive() -> Result<IncomingMessage>: BufReader<Stdin>, read line, deserialize
- **P0.6.3** [CRITICAL] send(msg) -> Result<()>: serialize to JSON, write line+\n, flush stdout

### P0.7 [HIGH] Request ID Remapping — scp-core/src/id_map.rs
Acceptance: Bidirectional lookup works; unit tests for insert/lookup/remove.

- **P0.7.1** [HIGH] IdMap struct: client_to_internal: HashMap<RequestId, InternalId>, internal_to_client: HashMap<InternalId, RequestId>; InternalId newtype over String
- **P0.7.2** [HIGH] generate_internal_id() -> InternalId: format "scp-{8 random hex chars}-{AtomicU64 counter}"
- **P0.7.3** [HIGH] IdMap methods: insert(client_id, internal_id), get_internal(client_id), get_client(internal_id), remove_by_internal(internal_id) + unit tests

### P0.8 [CRITICAL] Initialize Handshake Both Sides — scp-hub/src/hub.rs
Acceptance: Full MCP initialize lifecycle completed on both sides before proxy loop starts.

- **P0.8.1** [CRITICAL] initialize_backend(transport): send initialize request (protocolVersion "2025-03-26", clientInfo scp/0.1.0), receive InitializeResult, send notifications/initialized
- **P0.8.2** [CRITICAL] handle_client_initialize(client_transport, backend_caps): receive initialize from client, respond with backend capabilities, wait for notifications/initialized from client
- **P0.8.3** [HIGH] Capability passthrough: advertise backend caps verbatim; serverInfo = {name:"scp", version:"0.1.0"}

### P0.9 [CRITICAL] Passthrough Proxy Loop — scp-hub/src/hub.rs
Acceptance: All request types forwarded correctly; ID remapping applied; clean exit on client disconnect.

- **P0.9.1** [CRITICAL] run_proxy(client, backend, id_map): main async loop — receive from client, dispatch, receive from backend, send to client
- **P0.9.2** [CRITICAL] Request forwarding: generate internal ID, insert into id_map, rewrite request id, send to backend
- **P0.9.3** [CRITICAL] Response forwarding: lookup internal ID in id_map, rewrite with client ID, remove from id_map, send to client
- **P0.9.4** [HIGH] Notification forwarding: client->backend (no ID change) and backend->client (no ID change)
- **P0.9.5** [MEDIUM] Ping handling: respond directly to client with empty result, do NOT forward to backend
- **P0.9.6** [HIGH] Error handling: backend TransportError -> JSON-RPC error -32000 to client; client EOF -> clean loop exit

### P0.10 [HIGH] scp-hub main() Wiring
Acceptance: `scp-hub --server <cmd>` works end-to-end; exit code 0 on clean shutdown.

- **P0.10.1** [HIGH] CLI arg parsing with clap: --server <cmd+args>, --log-format <json|pretty>, --log-level <trace|debug|info|warn|error>
- **P0.10.2** [CRITICAL] Startup sequence: init_tracing() -> StdioServerTransport::spawn() -> initialize_backend() -> StdioClientTransport::new() -> handle_client_initialize() -> run_proxy() -> exit

### P0.11 [CRITICAL] Passthrough Integration Test — tests/passthrough_test.rs
Acceptance: `cargo test` passes; all 5 test cases green.

- **P0.11.1** [HIGH] Mock MCP server (tests/common/mock_server.rs): in-process or binary that handles initialize/tools_list/tools_call/ping with fixed fixtures
- **P0.11.2** [HIGH] spawn_scp_with_mock() helper: spawns mock + scp-hub, returns StdioClientTransport to scp-hub
- **P0.11.3** [CRITICAL] Test: initialize handshake round-trip (send initialize, verify protocolVersion + capabilities, send initialized)
- **P0.11.4** [CRITICAL] Test: tools/list passthrough (verify 2 tools returned, response ID matches request ID)
- **P0.11.5** [CRITICAL] Test: tools/call passthrough (verify fixture content returned, response ID matches)
- **P0.11.6** [MEDIUM] Test: ping handled by SCP directly (mock server must NOT receive ping)
- **P0.11.7** [HIGH] Test: request ID remapping (send requests with IDs 1 and 2, verify responses have IDs 1 and 2)

---

## Implementation Order

| Step | Tasks | Can Parallelize |
|------|-------|-----------------|
| 1 | P0.1 (workspace scaffold) | No — prerequisite for everything |
| 2 | P0.2 (JSON-RPC types) | No — prerequisite for all protocol work |
| 3 | P0.3 (MCP types), P0.4 (tracing) | Yes — parallel with each other |
| 4 | P0.5 (stdio server), P0.6 (stdio client), P0.7 (ID map) | Yes — all parallel |
| 5 | P0.8 (initialize handshake) | No — needs P0.2-P0.6 |
| 6 | P0.9 (proxy loop) | No — needs P0.7-P0.8 |
| 7 | P0.10 (main wiring) | No — needs P0.9 |
| 8 | P0.11 (integration test) | No — needs P0.10; P0.11.1 can start after P0.2 |

**Critical path:** P0.1 → P0.2 → {P0.3, P0.5, P0.6, P0.7} → P0.8 → P0.9 → P0.10 → P0.11

**Total subtasks:** 42 atomic tasks across 11 deliverable groups.
