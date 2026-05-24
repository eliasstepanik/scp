/// Heuristic token counter
///
/// Uses a simple byte-based heuristic:
/// - For ASCII/Latin text: bytes / 3.5 (conservative estimate)
/// - For non-Latin text (>30% non-ASCII bytes): bytes / 2.5 (more tokens per byte)
pub fn count_tokens(text: &str) -> usize {
    let bytes = text.len();
    if bytes == 0 {
        return 0;
    }

    // Count non-ASCII bytes
    let non_ascii_bytes = text.bytes().filter(|&b| b > 127).count();

    // If >30% of bytes are non-ASCII, use different ratio
    if non_ascii_bytes as f64 / bytes as f64 > 0.3 {
        (bytes as f64 / 2.5).ceil() as usize
    } else {
        (bytes as f64 / 3.5).ceil() as usize
    }
}

/// Measure tokens in a JSON-RPC response
pub fn measure_response_tokens(response: &serde_json::Value) -> usize {
    let json_str = serde_json::to_string(response).unwrap_or_default();
    count_tokens(&json_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens_empty() {
        assert_eq!(count_tokens(""), 0);
    }

    #[test]
    fn test_count_tokens_ascii() {
        // "Hello, World!" = 13 bytes / 3.5 ≈ 4 tokens
        let tokens = count_tokens("Hello, World!");
        assert!((3..=5).contains(&tokens));
    }

    #[test]
    fn test_count_tokens_code() {
        // Code is typically ASCII
        let code = "fn main() { println!(\"Hello\"); }";
        let tokens = count_tokens(code);
        assert!(tokens > 0);
    }

    #[test]
    fn test_count_tokens_cjk() {
        // CJK characters are multi-byte
        let cjk = "你好世界";
        let tokens = count_tokens(cjk);
        // 12 bytes / 2.5 ≈ 5 tokens
        assert!((4..=6).contains(&tokens));
    }

    #[test]
    fn test_count_tokens_mixed() {
        // Mixed ASCII and non-ASCII
        let mixed = "Hello 世界";
        let tokens = count_tokens(mixed);
        assert!(tokens > 0);
    }

    #[test]
    fn test_measure_response_tokens() {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "message": "Hello, World!"
            }
        });

        let tokens = measure_response_tokens(&response);
        assert!(tokens > 0);
    }
}
