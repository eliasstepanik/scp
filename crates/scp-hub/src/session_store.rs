use anyhow::Result;
use chrono::{DateTime, Utc};
use scp_core::id_map::IdMap;
use scp_core::keyword_accumulator::KeywordAccumulator;
use scp_core::mcp_types::{ClientCapabilities, Implementation};
use scp_filter::dedup::DeliveryLog;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info};
use uuid::Uuid;

/// Session ID type.
pub type SessionId = String;

/// Root resource definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    /// URI of the root resource.
    pub uri: String,
    /// Optional name of the root resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Tool call record for history tracking.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolCallRecord {
    /// Name of the tool called.
    pub tool_name: String,
    /// Name of the server that provided the tool.
    pub server_name: String,
    /// Timestamp of the call.
    pub timestamp: DateTime<Utc>,
    /// Token cost of the call.
    pub token_cost: usize,
}

/// In-flight request tracking.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct InFlightRequest {
    /// The RPC method name.
    pub method: String,
    /// The server handling the request.
    pub server_name: String,
    /// When the request started.
    pub started_at: DateTime<Utc>,
}

/// Session state.
#[allow(dead_code)]
pub struct Session {
    /// Unique session identifier.
    pub id: SessionId,
    /// Optional authentication token.
    pub auth_token: Option<String>,
    /// Profile name for this session.
    pub profile: String,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// Last activity timestamp.
    pub last_active: DateTime<Utc>,
    /// Client capabilities.
    pub client_capabilities: ClientCapabilities,
    /// Client implementation info.
    pub client_info: Option<Implementation>,
    /// Root resources.
    pub roots: Vec<Root>,
    /// Remaining token budget.
    pub token_budget_remaining: usize,
    /// Optional tool scope filter.
    pub tool_scope: Option<Vec<String>>,
    /// Request ID mapping.
    pub request_map: IdMap,
    /// History of tool calls.
    pub call_history: VecDeque<ToolCallRecord>,
    /// Currently in-flight requests.
    pub active_requests: HashMap<String, InFlightRequest>,
    /// Broadcast channel for outbound messages.
    pub outbound_tx: broadcast::Sender<Value>,
    /// Remaining rate limit tokens.
    pub rate_limit_remaining: u32,
    /// Last rate limit refill time.
    pub rate_limit_last_refill: Instant,
    /// Rate limit per minute.
    pub rate_limit_per_minute: u32,
    /// Keyword accumulator for relevance scoring.
    pub keyword_accumulator: KeywordAccumulator,
    /// Delivery log for deduplication.
    pub delivery_log: Arc<Mutex<DeliveryLog>>,
    /// Cache of chunked content.
    pub chunk_cache: HashMap<String, Vec<scp_filter::chunker::Chunk>>,
    /// Order of chunk cache entries for LRU eviction.
    pub chunk_cache_order: VecDeque<String>,
}

#[allow(dead_code)]
impl Session {
    // Methods are used by listener and router in P2.B and beyond
}

impl Session {
    #[allow(dead_code)]
    /// Create a new session
    pub fn new(
        auth_token: Option<String>,
        profile: String,
        budget: usize,
        rate_limit_per_minute: u32,
        outbound_tx: broadcast::Sender<Value>,
    ) -> Self {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        Self {
            id: id.clone(),
            auth_token,
            profile,
            created_at: now,
            last_active: now,
            client_capabilities: ClientCapabilities::default(),
            client_info: None,
            roots: Vec::new(),
            token_budget_remaining: budget,
            tool_scope: None,
            request_map: IdMap::new(id),
            call_history: VecDeque::with_capacity(100),
            active_requests: HashMap::new(),
            outbound_tx,
            rate_limit_remaining: rate_limit_per_minute,
            rate_limit_last_refill: Instant::now(),
            rate_limit_per_minute,
            keyword_accumulator: KeywordAccumulator::new(),
            delivery_log: Arc::new(Mutex::new(DeliveryLog::new(10_000))),
            chunk_cache: HashMap::new(),
            chunk_cache_order: VecDeque::new(),
        }
    }

    /// Update last active time
    #[allow(dead_code)]
    pub fn touch(&mut self) {
        self.last_active = Utc::now();
    }

    /// Check and apply rate limiting. Returns true if request is allowed, false if rate limited.
    #[allow(dead_code)]
    pub fn check_rate_limit(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.rate_limit_last_refill);

        // If 60 seconds have passed, refill the bucket
        if elapsed.as_secs() >= 60 {
            self.rate_limit_remaining = self.rate_limit_per_minute;
            self.rate_limit_last_refill = now;
        }

        // Check if we have tokens remaining
        if self.rate_limit_remaining > 0 {
            self.rate_limit_remaining -= 1;
            true
        } else {
            false
        }
    }

    /// Add a tool call to history
    #[allow(dead_code)]
    pub fn record_tool_call(&mut self, tool_name: String, server_name: String, token_cost: usize) {
        let record = ToolCallRecord {
            tool_name,
            server_name,
            timestamp: Utc::now(),
            token_cost,
        };

        if self.call_history.len() >= 100 {
            self.call_history.pop_front();
        }
        self.call_history.push_back(record);
    }

    /// Store chunks for a request with LRU eviction
    #[allow(dead_code)]
    pub fn store_chunks(&mut self, request_id: String, chunks: Vec<scp_filter::chunker::Chunk>) {
        const MAX_CACHE: usize = 50;
        if self.chunk_cache.len() >= MAX_CACHE {
            if let Some(oldest) = self.chunk_cache_order.pop_front() {
                self.chunk_cache.remove(&oldest);
            }
        }
        self.chunk_cache_order.push_back(request_id.clone());
        self.chunk_cache.insert(request_id, chunks);
    }

    /// Retrieve cached chunks for a request
    #[allow(dead_code)]
    pub fn get_chunks(&self, request_id: &str) -> Option<&Vec<scp_filter::chunker::Chunk>> {
        self.chunk_cache.get(request_id)
    }

    /// Extract keywords from tool call arguments and feed into the accumulator.
    ///
    /// Call this before making the backend request so terms are ready for relevance scoring.
    /// Also applies exponential decay so older terms lose relevance over time.
    pub fn feed_tool_args(&mut self, args: &serde_json::Value) {
        self.keyword_accumulator.decay();
        self.keyword_accumulator.extract_from_args(args);
    }

    /// Get top-k query terms for relevance scoring.
    pub fn current_query_terms(&self, k: usize) -> Vec<String> {
        self.keyword_accumulator.top_k(k)
    }
}

/// Session summary for listing.
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct SessionSummary {
    /// Session ID.
    pub id: SessionId,
    /// When the session was created.
    pub created_at: String,
    /// Last activity time.
    pub last_active: String,
    /// Number of tool calls made.
    pub tool_call_count: usize,
    /// Remaining token budget.
    pub budget_remaining: usize,
}

/// Session store — manages all active sessions
#[allow(dead_code)]
pub struct SessionStore {
    sessions: Arc<RwLock<HashMap<SessionId, Arc<Mutex<Session>>>>>,
    default_budget: usize,
    default_rate_limit: u32,
}

impl SessionStore {
    #[allow(dead_code)]
    /// Create a new session store
    pub fn new(default_budget: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_budget,
            default_rate_limit: 60,
        }
    }

    #[allow(dead_code)]
    /// Create a new session store with custom rate limit
    pub fn with_rate_limit(default_budget: usize, default_rate_limit: u32) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_budget,
            default_rate_limit,
        }
    }

    /// Create a new session
    #[allow(dead_code)]
    pub async fn create(
        &self,
        auth_token: Option<String>,
        profile: String,
        budget: usize,
        rate_limit_per_minute: u32,
    ) -> (SessionId, broadcast::Receiver<Value>) {
        let (tx, rx) = broadcast::channel(100);
        let session = Session::new(auth_token, profile, budget, rate_limit_per_minute, tx);
        let session_id = session.id.clone();

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), Arc::new(Mutex::new(session)));

        debug!("Created session: {}", session_id);
        (session_id, rx)
    }

    /// Create a new session with default profile
    #[allow(dead_code)]
    pub async fn create_with_defaults(
        &self,
        auth_token: Option<String>,
    ) -> (SessionId, broadcast::Receiver<Value>) {
        self.create(
            auth_token,
            "default".to_string(),
            self.default_budget,
            self.default_rate_limit,
        )
        .await
    }

    /// Get a session by ID
    #[allow(dead_code)]
    pub async fn get(&self, id: &SessionId) -> Option<Arc<Mutex<Session>>> {
        let sessions = self.sessions.read().await;
        sessions.get(id).cloned()
    }

    /// Remove a session by ID
    #[allow(dead_code)]
    pub async fn remove(&self, id: &SessionId) -> bool {
        let mut sessions = self.sessions.write().await;
        let removed = sessions.remove(id).is_some();
        if removed {
            info!("Removed session: {}", id);
        }
        removed
    }

    /// List all sessions
    #[allow(dead_code)]
    pub async fn list(&self) -> Vec<SessionSummary> {
        let sessions = self.sessions.read().await;
        let mut summaries = Vec::new();

        for session in sessions.values() {
            let s = session.lock().unwrap_or_else(|e| e.into_inner());
            summaries.push(SessionSummary {
                id: s.id.clone(),
                created_at: s.created_at.to_rfc3339(),
                last_active: s.last_active.to_rfc3339(),
                tool_call_count: s.call_history.len(),
                budget_remaining: s.token_budget_remaining,
            });
        }

        summaries
    }

    /// Initialize a session with client capabilities
    #[allow(dead_code)]
    pub async fn initialize_session(
        &self,
        id: &SessionId,
        capabilities: ClientCapabilities,
        client_info: Implementation,
        roots: Option<Vec<Root>>,
    ) -> Result<()> {
        let session = self
            .get(id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", id))?;

        let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
        s.client_capabilities = capabilities;
        s.client_info = Some(client_info);
        if let Some(roots) = roots {
            s.roots = roots;
        }
        s.touch();

        debug!("Initialized session: {}", id);
        Ok(())
    }

    /// Start a background cleanup task for expired sessions.
    ///
    /// Sessions whose `last_active` timestamp is older than `timeout_secs` seconds are removed.
    #[allow(dead_code)]
    pub fn start_cleanup_task(self: Arc<Self>, timeout_secs: u64) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                let sessions = self.sessions.read().await;

                let expired: Vec<SessionId> = sessions
                    .iter()
                    .filter_map(|(id, session_arc)| {
                        let s = session_arc.lock().unwrap_or_else(|e| e.into_inner());
                        let idle_secs = (Utc::now() - s.last_active).num_seconds();
                        if idle_secs >= timeout_secs as i64 {
                            Some(id.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                drop(sessions);

                for id in expired {
                    self.remove(&id).await;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_creation() {
        let store = SessionStore::new(1000);
        let (session_id, _rx) = store.create(None, "default".to_string(), 1000, 60).await;

        let session = store.get(&session_id).await;
        assert!(session.is_some());
    }

    #[tokio::test]
    async fn test_session_removal() {
        let store = SessionStore::new(1000);
        let (session_id, _rx) = store.create(None, "default".to_string(), 1000, 60).await;

        let removed = store.remove(&session_id).await;
        assert!(removed);

        let session = store.get(&session_id).await;
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn test_session_list() {
        let store = SessionStore::new(1000);
        let (_id1, _rx1) = store.create(None, "default".to_string(), 1000, 60).await;
        let (_id2, _rx2) = store.create(None, "default".to_string(), 1000, 60).await;

        let list = store.list().await;
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_session_timestamps_are_iso8601() {
        let store = SessionStore::new(1000);
        let (_id, _rx) = store.create(None, "default".to_string(), 1000, 60).await;

        let list = store.list().await;
        assert_eq!(list.len(), 1);

        let summary = &list[0];
        // RFC 3339 / ISO 8601 timestamps contain 'T' and end with 'Z' or an offset
        assert!(
            summary.created_at.contains('T'),
            "created_at is not ISO8601: {}",
            summary.created_at
        );
        assert!(
            summary.last_active.contains('T'),
            "last_active is not ISO8601: {}",
            summary.last_active
        );
        // Verify it parses back correctly
        assert!(
            DateTime::parse_from_rfc3339(&summary.created_at).is_ok(),
            "created_at failed rfc3339 parse: {}",
            summary.created_at
        );
        assert!(
            DateTime::parse_from_rfc3339(&summary.last_active).is_ok(),
            "last_active failed rfc3339 parse: {}",
            summary.last_active
        );
    }

    #[tokio::test]
    async fn test_session_initialize() {
        let store = SessionStore::new(1000);
        let (session_id, _rx) = store.create(None, "default".to_string(), 1000, 60).await;

        let caps = ClientCapabilities::default();
        let info = Implementation {
            name: "test-client".to_string(),
            version: "1.0".to_string(),
        };

        let result = store
            .initialize_session(&session_id, caps, info, None)
            .await;
        assert!(result.is_ok());

        let session = store.get(&session_id).await.unwrap();
        let s = session.lock().unwrap();
        assert_eq!(s.client_info.as_ref().unwrap().name, "test-client");
    }
}
