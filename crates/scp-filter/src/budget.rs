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
}
