use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};

/// SHA-256 hash of a content chunk (32 bytes)
pub type ChunkHash = [u8; 32];

/// LRU-bounded delivery log — tracks hashes of content already sent to a session.
/// Capped at `capacity` entries with LRU eviction.
pub struct DeliveryLog {
    inner: HashMap<ChunkHash, ()>,
    order: VecDeque<ChunkHash>,
    capacity: usize,
}

impl DeliveryLog {
    /// Create a new delivery log with the given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    /// Returns true if this hash was already delivered (duplicate).
    pub fn contains(&self, hash: &ChunkHash) -> bool {
        self.inner.contains_key(hash)
    }

    /// Record a hash as delivered. Evicts the oldest (LRU) entry if at capacity.
    pub fn insert(&mut self, hash: ChunkHash) {
        // If already present, don't insert again
        if self.inner.contains_key(&hash) {
            return;
        }

        // If at capacity, evict the oldest (front of queue)
        if self.inner.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.inner.remove(&oldest);
            }
        }

        // Insert the new hash
        self.inner.insert(hash, ());
        self.order.push_back(hash);
    }

    /// Return the number of entries in the log
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the log is empty
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// Dedup filter — checks text content against the session delivery log.
pub struct DedupFilter;

impl DedupFilter {
    /// Hash a text string using SHA-256.
    pub fn hash_text(text: &str) -> ChunkHash {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Check if text has already been delivered.
    /// Does NOT record — recording is done by DeliveryLogger (T7) after delivery.
    pub fn is_duplicate(text: &str, log: &DeliveryLog) -> bool {
        let hash = Self::hash_text(text);
        log.contains(&hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_text_deterministic() {
        let text = "hello world";
        let hash1 = DedupFilter::hash_text(text);
        let hash2 = DedupFilter::hash_text(text);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_text_different() {
        let text1 = "hello world";
        let text2 = "hello world!";
        let hash1 = DedupFilter::hash_text(text1);
        let hash2 = DedupFilter::hash_text(text2);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_delivery_log_insert_and_contains() {
        let mut log = DeliveryLog::new(10);
        let hash = DedupFilter::hash_text("test content");

        assert!(!log.contains(&hash));
        log.insert(hash);
        assert!(log.contains(&hash));
    }

    #[test]
    fn test_delivery_log_not_contains() {
        let log = DeliveryLog::new(10);
        let hash = DedupFilter::hash_text("test content");
        assert!(!log.contains(&hash));
    }

    #[test]
    fn test_delivery_log_lru_eviction() {
        let mut log = DeliveryLog::new(3);

        let hash1 = DedupFilter::hash_text("content 1");
        let hash2 = DedupFilter::hash_text("content 2");
        let hash3 = DedupFilter::hash_text("content 3");
        let hash4 = DedupFilter::hash_text("content 4");

        // Insert 3 items (at capacity)
        log.insert(hash1);
        log.insert(hash2);
        log.insert(hash3);
        assert_eq!(log.len(), 3);
        assert!(log.contains(&hash1));
        assert!(log.contains(&hash2));
        assert!(log.contains(&hash3));

        // Insert 4th item, should evict hash1 (oldest)
        log.insert(hash4);
        assert_eq!(log.len(), 3);
        assert!(!log.contains(&hash1));
        assert!(log.contains(&hash2));
        assert!(log.contains(&hash3));
        assert!(log.contains(&hash4));
    }

    #[test]
    fn test_dedup_filter_is_duplicate_true() {
        let mut log = DeliveryLog::new(10);
        let text = "test content";
        let hash = DedupFilter::hash_text(text);

        log.insert(hash);
        assert!(DedupFilter::is_duplicate(text, &log));
    }

    #[test]
    fn test_dedup_filter_is_duplicate_false() {
        let log = DeliveryLog::new(10);
        let text = "test content";
        assert!(!DedupFilter::is_duplicate(text, &log));
    }
}
