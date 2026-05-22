use std::time::Instant;

/// Server lifecycle state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    /// Not connected, process not running
    Cold,
    /// Connection/process being established
    Starting,
    /// Connected, idle, awaiting requests
    Warm,
    /// Actively processing requests
    Hot,
    /// No new requests; waiting for in-flight
    Draining,
    /// Administratively deactivated, tools hidden
    Disabled,
    /// Consecutive failures exceeded threshold
    Failed,
}

impl ServerState {
    /// Check if server is healthy (can accept requests)
    pub fn is_healthy(&self) -> bool {
        matches!(self, ServerState::Warm | ServerState::Hot)
    }

    /// Check if server is available (not disabled or failed)
    pub fn is_available(&self) -> bool {
        !matches!(self, ServerState::Disabled | ServerState::Failed)
    }

    /// Check if server can accept new requests
    pub fn can_accept_requests(&self) -> bool {
        matches!(self, ServerState::Warm | ServerState::Hot)
    }
}

impl std::fmt::Display for ServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerState::Cold => write!(f, "cold"),
            ServerState::Starting => write!(f, "starting"),
            ServerState::Warm => write!(f, "warm"),
            ServerState::Hot => write!(f, "hot"),
            ServerState::Draining => write!(f, "draining"),
            ServerState::Disabled => write!(f, "disabled"),
            ServerState::Failed => write!(f, "failed"),
        }
    }
}

/// Server lifecycle information
#[derive(Debug, Clone)]
pub struct LifecycleInfo {
    pub state: ServerState,
    pub failure_count: u32,
    pub last_ping: Option<Instant>,
    pub last_error: Option<String>,
}

impl LifecycleInfo {
    /// Create a new lifecycle info in Cold state
    pub fn new() -> Self {
        Self {
            state: ServerState::Cold,
            failure_count: 0,
            last_ping: None,
            last_error: None,
        }
    }

    /// Transition to a new state
    pub fn transition_to(&mut self, new_state: ServerState) {
        self.state = new_state;
        if new_state == ServerState::Warm || new_state == ServerState::Hot {
            self.failure_count = 0;
            self.last_error = None;
        }
    }

    /// Record a failure
    pub fn record_failure(&mut self, error: String) {
        self.failure_count += 1;
        self.last_error = Some(error);
        self.last_ping = Some(Instant::now());
    }

    /// Record a successful ping
    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.last_error = None;
        self.last_ping = Some(Instant::now());
    }
}

impl Default for LifecycleInfo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_state_is_healthy() {
        assert!(ServerState::Warm.is_healthy());
        assert!(ServerState::Hot.is_healthy());
        assert!(!ServerState::Cold.is_healthy());
        assert!(!ServerState::Failed.is_healthy());
    }

    #[test]
    fn test_server_state_is_available() {
        assert!(ServerState::Warm.is_available());
        assert!(ServerState::Hot.is_available());
        assert!(ServerState::Cold.is_available());
        assert!(!ServerState::Disabled.is_available());
        assert!(!ServerState::Failed.is_available());
    }

    #[test]
    fn test_lifecycle_info_transition() {
        let mut info = LifecycleInfo::new();
        assert_eq!(info.state, ServerState::Cold);

        info.transition_to(ServerState::Starting);
        assert_eq!(info.state, ServerState::Starting);

        info.transition_to(ServerState::Warm);
        assert_eq!(info.state, ServerState::Warm);
        assert_eq!(info.failure_count, 0);
    }

    #[test]
    fn test_lifecycle_info_record_failure() {
        let mut info = LifecycleInfo::new();
        info.record_failure("Connection refused".to_string());
        assert_eq!(info.failure_count, 1);
        assert!(info.last_error.is_some());
        assert!(info.last_ping.is_some());
    }
}
