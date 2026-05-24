use crate::chunker::Chunk;
use crate::token_count::count_tokens;
use scp_core::protocol::JsonRpcResponse;
use serde_json::Value;

/// Budget enforcer — truncates responses to fit within token budget
pub struct BudgetEnforcer;

impl BudgetEnforcer {
    /// Truncate a response to fit within budget
    pub fn enforce(response: &mut JsonRpcResponse, budget_tokens: usize) {
        if let Some(Value::Object(ref mut obj)) = response.result {
            if let Some(Value::String(ref mut text)) = obj.get_mut("content") {
                *text = Self::truncate_to_budget(text, budget_tokens);
            }
        }
    }

    /// Truncate a string to fit within token budget
    /// Appends "..." if truncated, ensures minimum 200 tokens delivered
    pub fn truncate_to_budget(text: &str, budget_tokens: usize) -> String {
        let current_tokens = count_tokens(text);

        // If already under budget, return as-is
        if current_tokens <= budget_tokens {
            return text.to_string();
        }

        // Minimum delivery: 200 tokens
        let min_tokens = 200;
        let target_budget = budget_tokens.max(min_tokens);

        // Binary search for the right cut point
        let mut low = 0;
        let mut high = text.len();
        let mut best_cut = 0;

        while low <= high {
            let mid = (low + high) / 2;
            let truncated = &text[..mid];
            let tokens = count_tokens(truncated);

            if tokens <= target_budget {
                best_cut = mid;
                low = mid + 1;
            } else {
                high = mid.saturating_sub(1);
            }
        }

        // Ensure we cut at a character boundary
        let mut cut_point = best_cut;
        while cut_point > 0 && !text.is_char_boundary(cut_point) {
            cut_point -= 1;
        }

        // Truncate and append ellipsis
        let mut result = text[..cut_point].to_string();
        result.push_str("...");
        result
    }

    /// Select the top-k chunks by score that fit within the token budget.
    ///
    /// Algorithm:
    /// 1. Sort chunks by score descending (stable sort to preserve relative order on ties)
    /// 2. Greedily add chunks until budget is exhausted
    /// 3. Re-sort selected chunks by original index (preserve document order)
    /// 4. Return (selected_chunks, total_chunks_count)
    ///    - total_chunks_count is the original count before selection (used by ProgressiveDisclosure)
    pub fn select_chunks(
        chunks: Vec<Chunk>,
        budget_tokens: usize,
        _min_tokens: usize,
    ) -> (Vec<Chunk>, usize) {
        let total = chunks.len();

        // If no chunks, return empty
        if total == 0 {
            return (vec![], 0);
        }

        // Sort by score descending (handle NaN by treating as 0)
        let mut sorted_chunks = chunks;
        sorted_chunks.sort_by(|a, b| {
            let score_a = if a.score.is_nan() { 0.0 } else { a.score };
            let score_b = if b.score.is_nan() { 0.0 } else { b.score };
            // Reverse order for descending sort
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Greedily select chunks by score
        let mut selected = vec![];
        let mut running_tokens = 0;

        for chunk in sorted_chunks.iter() {
            let chunk_tokens = count_tokens(&chunk.text);
            if running_tokens + chunk_tokens <= budget_tokens {
                selected.push(chunk.clone());
                running_tokens += chunk_tokens;
            }
        }

        // If no chunks fit and budget is tight, force-include the top-scoring chunk
        if selected.is_empty() && total > 0 {
            // The first chunk in sorted_chunks is the highest-scoring
            selected.push(sorted_chunks[0].clone());
        }

        // Re-sort selected chunks by original index to restore document order
        selected.sort_by_key(|chunk| chunk.index);

        (selected, total)
    }

    /// Reassemble selected chunks into a single string.
    /// Joins with "\n\n" separator.
    pub fn reassemble(chunks: &[Chunk]) -> String {
        chunks
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_under_budget() {
        let text = "Hello, World!";
        let result = BudgetEnforcer::truncate_to_budget(text, 100);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_over_budget() {
        let text = "a".repeat(1000);
        let result = BudgetEnforcer::truncate_to_budget(&text, 100);
        assert!(result.ends_with("..."));
        let tokens = count_tokens(&result);
        assert!(tokens <= 100 || tokens >= 200); // Either under budget or at minimum
    }

    #[test]
    fn test_truncate_minimum_delivery() {
        let text = "a".repeat(10000);
        let result = BudgetEnforcer::truncate_to_budget(&text, 50);
        // Should deliver at least 200 tokens
        let tokens = count_tokens(&result);
        assert!(tokens >= 200);
    }

    #[test]
    fn test_truncate_preserves_boundaries() {
        let text = "Hello, 世界! This is a test.";
        let result = BudgetEnforcer::truncate_to_budget(text, 5);
        // Should not panic and should be valid UTF-8
        assert!(result.is_ascii() || result.chars().all(|c| !c.is_control()));
    }

    #[test]
    fn test_enforce_response() {
        let mut response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(scp_core::protocol::RequestId::Number(1)),
            result: Some(serde_json::json!({
                "content": "a".repeat(1000)
            })),
            error: None,
        };

        BudgetEnforcer::enforce(&mut response, 100);

        if let Some(Value::Object(obj)) = &response.result {
            if let Some(Value::String(text)) = obj.get("content") {
                assert!(text.ends_with("..."));
            }
        }
    }

    #[test]
    fn test_select_chunks_respects_budget() {
        // Create 5 chunks with different scores
        let chunks = vec![
            Chunk {
                text: "chunk one".to_string(),
                index: 0,
                score: 0.5,
            },
            Chunk {
                text: "chunk two".to_string(),
                index: 1,
                score: 0.9, // highest
            },
            Chunk {
                text: "chunk three".to_string(),
                index: 2,
                score: 0.3,
            },
            Chunk {
                text: "chunk four".to_string(),
                index: 3,
                score: 0.7,
            },
            Chunk {
                text: "chunk five".to_string(),
                index: 4,
                score: 0.2,
            },
        ];

        // Budget should fit approximately 3 chunks (each ~3 tokens)
        let (selected, total) = BudgetEnforcer::select_chunks(chunks, 30, 0);

        assert_eq!(total, 5);
        // Should select highest-scoring chunks: 0.9, 0.7, 0.5
        assert!(selected.len() >= 2); // At least the top 2 should fit
                                      // Verify they're in document order
        for i in 1..selected.len() {
            assert!(selected[i].index > selected[i - 1].index);
        }
    }

    #[test]
    fn test_select_chunks_preserves_document_order() {
        let chunks = vec![
            Chunk {
                text: "first".to_string(),
                index: 0,
                score: 0.3,
            },
            Chunk {
                text: "second".to_string(),
                index: 1,
                score: 0.9,
            },
            Chunk {
                text: "third".to_string(),
                index: 2,
                score: 0.5,
            },
        ];

        let (selected, _) = BudgetEnforcer::select_chunks(chunks, 100, 0);

        // All should fit
        assert_eq!(selected.len(), 3);
        // Should be in document order (by index), not score order
        assert_eq!(selected[0].index, 0);
        assert_eq!(selected[1].index, 1);
        assert_eq!(selected[2].index, 2);
    }

    #[test]
    fn test_select_chunks_empty_budget_force_includes_top() {
        let chunks = vec![
            Chunk {
                text: "low score".to_string(),
                index: 0,
                score: 0.1,
            },
            Chunk {
                text: "high score".to_string(),
                index: 1,
                score: 0.9,
            },
        ];

        // Budget 0 should still return the top-scoring chunk
        let (selected, total) = BudgetEnforcer::select_chunks(chunks, 0, 0);

        assert_eq!(total, 2);
        assert_eq!(selected.len(), 1);
        // Should be the highest-scoring chunk
        assert_eq!(selected[0].score, 0.9);
    }

    #[test]
    fn test_select_chunks_all_fit() {
        let chunks = vec![
            Chunk {
                text: "a".to_string(),
                index: 0,
                score: 0.5,
            },
            Chunk {
                text: "b".to_string(),
                index: 1,
                score: 0.7,
            },
            Chunk {
                text: "c".to_string(),
                index: 2,
                score: 0.3,
            },
        ];

        // Large budget should fit all
        let (selected, total) = BudgetEnforcer::select_chunks(chunks, 1000, 0);

        assert_eq!(total, 3);
        assert_eq!(selected.len(), 3);
        // Verify document order
        assert_eq!(selected[0].index, 0);
        assert_eq!(selected[1].index, 1);
        assert_eq!(selected[2].index, 2);
    }

    #[test]
    fn test_reassemble_joins_with_separator() {
        let chunks = vec![
            Chunk {
                text: "first chunk".to_string(),
                index: 0,
                score: 0.5,
            },
            Chunk {
                text: "second chunk".to_string(),
                index: 1,
                score: 0.7,
            },
            Chunk {
                text: "third chunk".to_string(),
                index: 2,
                score: 0.3,
            },
        ];

        let result = BudgetEnforcer::reassemble(&chunks);

        assert_eq!(result, "first chunk\n\nsecond chunk\n\nthird chunk");
    }

    #[test]
    fn test_reassemble_empty() {
        let chunks: Vec<Chunk> = vec![];
        let result = BudgetEnforcer::reassemble(&chunks);
        assert_eq!(result, "");
    }

    #[test]
    fn test_reassemble_single_chunk() {
        let chunks = vec![Chunk {
            text: "only chunk".to_string(),
            index: 0,
            score: 0.5,
        }];

        let result = BudgetEnforcer::reassemble(&chunks);
        assert_eq!(result, "only chunk");
    }
}
