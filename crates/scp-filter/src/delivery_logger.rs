use crate::chunker::Chunk;
use crate::dedup::{DedupFilter, DeliveryLog};

/// Logs delivered chunks and updates token budgets.
pub struct DeliveryLogger;

impl DeliveryLogger {
    /// Record the hashes of all delivered chunks into the session delivery log.
    /// Called AFTER the filtered content has been assembled and is ready to send.
    pub fn record(chunks: &[Chunk], log: &mut DeliveryLog) {
        for chunk in chunks {
            let hash = DedupFilter::hash_text(&chunk.text);
            log.insert(hash);
        }
    }

    /// Update the session's token budget: subtract tokens_delivered from remaining.
    /// Returns the new remaining budget.
    pub fn update_budget(tokens_delivered: usize, budget_remaining: &mut usize) -> usize {
        *budget_remaining = budget_remaining.saturating_sub(tokens_delivered);
        *budget_remaining
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_chunks_adds_to_log() {
        let mut log = DeliveryLog::new(10);
        let chunks = vec![
            Chunk::new("chunk 1".to_string(), 0),
            Chunk::new("chunk 2".to_string(), 1),
            Chunk::new("chunk 3".to_string(), 2),
        ];

        DeliveryLogger::record(&chunks, &mut log);

        // Verify all 3 hashes are in the log
        for chunk in &chunks {
            let hash = DedupFilter::hash_text(&chunk.text);
            assert!(log.contains(&hash));
        }
    }

    #[test]
    fn test_record_empty_chunks() {
        let mut log = DeliveryLog::new(10);
        let chunks: Vec<Chunk> = vec![];

        // Should not panic
        DeliveryLogger::record(&chunks, &mut log);

        assert_eq!(log.len(), 0);
    }

    #[test]
    fn test_update_budget_decrements() {
        let mut budget = 1000;
        let result = DeliveryLogger::update_budget(300, &mut budget);

        assert_eq!(budget, 700);
        assert_eq!(result, 700);
    }

    #[test]
    fn test_update_budget_saturates_at_zero() {
        let mut budget = 100;
        let result = DeliveryLogger::update_budget(200, &mut budget);

        assert_eq!(budget, 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_record_then_dedup_filters() {
        let mut log = DeliveryLog::new(10);
        let text = "test content";
        let chunk = Chunk::new(text.to_string(), 0);

        // Record the chunk
        DeliveryLogger::record(&[chunk], &mut log);

        // Verify DedupFilter::is_duplicate returns true for same text
        assert!(DedupFilter::is_duplicate(text, &log));
    }
}
