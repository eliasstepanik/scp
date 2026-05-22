use crate::protocol::RequestId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Internal request ID — format: "scp-{8 hex chars}-{counter}"
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternalId(pub String);

impl InternalId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InternalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Bidirectional request ID mapping
pub struct IdMap {
    client_to_internal: HashMap<RequestId, InternalId>,
    internal_to_client: HashMap<InternalId, RequestId>,
    counter: Arc<AtomicU64>,
    #[allow(dead_code)]
    session_id: String,
}

impl IdMap {
    pub fn new(session_id: String) -> Self {
        Self {
            client_to_internal: HashMap::new(),
            internal_to_client: HashMap::new(),
            counter: Arc::new(AtomicU64::new(0)),
            session_id,
        }
    }

    /// Generate a new internal ID and insert both directions
    pub fn generate(&mut self, client_id: RequestId) -> InternalId {
        let count = self.counter.fetch_add(1, Ordering::SeqCst);
        let random_hex = format!("{:08x}", count.wrapping_mul(0x9e3779b97f4a7c15));
        let internal_id = InternalId(format!("scp-{}-{:04}", &random_hex[..8], count));

        self.client_to_internal
            .insert(client_id.clone(), internal_id.clone());
        self.internal_to_client
            .insert(internal_id.clone(), client_id);

        internal_id
    }

    /// Look up client ID from internal ID
    pub fn get_client(&self, internal: &InternalId) -> Option<&RequestId> {
        self.internal_to_client.get(internal)
    }

    /// Look up internal ID from client ID
    pub fn get_internal(&self, client: &RequestId) -> Option<&InternalId> {
        self.client_to_internal.get(client)
    }

    /// Remove mapping by internal ID, returns the client ID
    pub fn remove(&mut self, internal: &InternalId) -> Option<RequestId> {
        if let Some(client_id) = self.internal_to_client.remove(internal) {
            self.client_to_internal.remove(&client_id);
            Some(client_id)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_lookup() {
        let mut id_map = IdMap::new("test-session".to_string());
        let client_id = RequestId::Number(1);
        let internal_id = id_map.generate(client_id.clone());

        assert_eq!(id_map.get_client(&internal_id), Some(&client_id));
        assert_eq!(id_map.get_internal(&client_id), Some(&internal_id));
    }

    #[test]
    fn test_multiple_ids() {
        let mut id_map = IdMap::new("test-session".to_string());
        let client_id_1 = RequestId::Number(1);
        let client_id_2 = RequestId::String("test".to_string());

        let internal_id_1 = id_map.generate(client_id_1.clone());
        let internal_id_2 = id_map.generate(client_id_2.clone());

        assert_ne!(internal_id_1, internal_id_2);
        assert_eq!(id_map.get_client(&internal_id_1), Some(&client_id_1));
        assert_eq!(id_map.get_client(&internal_id_2), Some(&client_id_2));
    }

    #[test]
    fn test_remove() {
        let mut id_map = IdMap::new("test-session".to_string());
        let client_id = RequestId::Number(1);
        let internal_id = id_map.generate(client_id.clone());

        let removed = id_map.remove(&internal_id);
        assert_eq!(removed, Some(client_id.clone()));
        assert_eq!(id_map.get_client(&internal_id), None);
        assert_eq!(id_map.get_internal(&client_id), None);
    }

    #[test]
    fn test_collision_free() {
        let mut id_map = IdMap::new("test-session".to_string());
        let mut internal_ids = Vec::new();

        for i in 0..100 {
            let client_id = RequestId::Number(i);
            let internal_id = id_map.generate(client_id);
            internal_ids.push(internal_id);
        }

        // Check all are unique
        let mut seen = std::collections::HashSet::new();
        for id in &internal_ids {
            assert!(seen.insert(id.clone()), "Duplicate ID found: {}", id);
        }
    }
}
