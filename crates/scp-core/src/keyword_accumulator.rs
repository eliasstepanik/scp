use serde_json::Value;
use std::collections::HashMap;

/// Stop words to filter during tokenization
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "it", "in", "on", "at", "to", "do", "for", "of", "and", "or", "not",
    "be", "by", "as", "with", "this", "that", "from", "are", "was", "has", "have", "but", "if",
    "then", "than", "so", "use", "used",
];

/// Accumulates keywords from tool arguments with frequency tracking and decay support.
///
/// Extracts keywords from JSON arguments and plain text, maintains frequency counts,
/// and supports exponential decay for time-based relevance.
#[derive(Debug, Clone)]
pub struct KeywordAccumulator {
    /// Maps keyword -> frequency (f32 for decay support).
    freq: HashMap<String, f32>,
}

impl KeywordAccumulator {
    /// Create a new empty `KeywordAccumulator`.
    pub fn new() -> Self {
        Self {
            freq: HashMap::new(),
        }
    }

    /// Extract keywords from a JSON value.
    ///
    /// Recursively traverses the JSON structure, extracts all string values,
    /// tokenizes them, filters stop words and short tokens, and adds them to the frequency map.
    ///
    /// # Arguments
    ///
    /// * `args` - JSON value to extract keywords from.
    pub fn extract_from_args(&mut self, args: &Value) {
        self.extract_strings_recursive(args);
    }

    /// Recursively extract all string leaf values from JSON
    fn extract_strings_recursive(&mut self, value: &Value) {
        match value {
            Value::String(s) => {
                self.tokenize_and_add(s);
            }
            Value::Object(obj) => {
                for v in obj.values() {
                    self.extract_strings_recursive(v);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    self.extract_strings_recursive(v);
                }
            }
            _ => {} // Ignore null, bool, number
        }
    }

    /// Tokenize a string and add tokens to frequency map
    fn tokenize_and_add(&mut self, s: &str) {
        // Split on non-alphanumeric chars (whitespace, punctuation, _, -)
        let tokens: Vec<&str> = s
            .split(|c: char| !c.is_alphanumeric())
            .filter(|token| !token.is_empty())
            .collect();

        for token in tokens {
            let lowercased = token.to_lowercase();

            // Filter: tokens shorter than 3 chars
            if lowercased.len() < 3 {
                continue;
            }

            // Filter: stop words
            if STOP_WORDS.contains(&lowercased.as_str()) {
                continue;
            }

            // Add to frequency map
            *self.freq.entry(lowercased).or_insert(0.0) += 1.0;
        }
    }

    /// Add pre-computed keywords to the accumulator.
    ///
    /// Increments the frequency count of each keyword by 1.0, filtering stop words and short tokens.
    ///
    /// # Arguments
    ///
    /// * `words` - Slice of keywords to add.
    pub fn add_keywords(&mut self, words: &[String]) {
        for word in words {
            let lowercased = word.to_lowercase();

            // Filter: tokens shorter than 3 chars
            if lowercased.len() < 3 {
                continue;
            }

            // Filter: stop words
            if STOP_WORDS.contains(&lowercased.as_str()) {
                continue;
            }

            *self.freq.entry(lowercased).or_insert(0.0) += 1.0;
        }
    }

    /// Extract keywords from plain text.
    ///
    /// Tokenizes the text and adds the resulting keywords to the accumulator.
    ///
    /// # Arguments
    ///
    /// * `text` - Plain text to extract keywords from.
    pub fn extract_from_text(&mut self, text: &str) {
        let words = self.tokenize_text(text);
        self.add_keywords(&words);
    }

    /// Tokenize plain text into words, filtering stop words and short tokens
    fn tokenize_text(&self, text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() >= 3 && !self.is_stop_word(w))
            .collect()
    }

    /// Check if a word is a stop word
    fn is_stop_word(&self, word: &str) -> bool {
        STOP_WORDS.contains(&word)
    }

    /// Apply exponential decay to all keyword frequencies.
    ///
    /// Halves all frequency counts and removes entries that have decayed below 0.01.
    /// Typically called after each tool invocation to reduce the relevance of older keywords.
    pub fn decay(&mut self) {
        for freq in self.freq.values_mut() {
            *freq *= 0.5;
        }

        // Remove entries that have decayed below a threshold (e.g., 0.01)
        self.freq.retain(|_, &mut freq| freq >= 0.01);
    }

    /// Return top-k keywords by frequency, highest first
    pub fn top_k(&self, k: usize) -> Vec<String> {
        let mut entries: Vec<_> = self.freq.iter().collect();
        entries.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
            .into_iter()
            .take(k)
            .map(|(word, _)| word.clone())
            .collect()
    }
}

impl Default for KeywordAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_from_simple_string() {
        let mut acc = KeywordAccumulator::new();
        let args = serde_json::json!({
            "query": "hello world test"
        });
        acc.extract_from_args(&args);

        let top = acc.top_k(10);
        assert!(top.contains(&"hello".to_string()));
        assert!(top.contains(&"world".to_string()));
        assert!(top.contains(&"test".to_string()));
    }

    #[test]
    fn test_extract_from_nested_json() {
        let mut acc = KeywordAccumulator::new();
        let args = serde_json::json!({
            "user": {
                "name": "alice",
                "email": "alice@example.com"
            },
            "tags": ["python", "rust", "javascript"]
        });
        acc.extract_from_args(&args);

        let top = acc.top_k(10);
        assert!(top.contains(&"alice".to_string()));
        assert!(top.contains(&"python".to_string()));
        assert!(top.contains(&"rust".to_string()));
        assert!(top.contains(&"javascript".to_string()));
    }

    #[test]
    fn test_stop_words_filtered() {
        let mut acc = KeywordAccumulator::new();
        let args = serde_json::json!({
            "text": "the quick brown fox is a test"
        });
        acc.extract_from_args(&args);

        let top = acc.top_k(10);
        // "the", "is", "a" should be filtered
        assert!(!top.contains(&"the".to_string()));
        assert!(!top.contains(&"is".to_string()));
        assert!(!top.contains(&"a".to_string()));
        // But these should be present
        assert!(top.contains(&"quick".to_string()));
        assert!(top.contains(&"brown".to_string()));
        assert!(top.contains(&"fox".to_string()));
    }

    #[test]
    fn test_short_tokens_filtered() {
        let mut acc = KeywordAccumulator::new();
        let args = serde_json::json!({
            "text": "go to it or do"
        });
        acc.extract_from_args(&args);

        let top = acc.top_k(10);
        // All tokens are < 3 chars or stop words, so should be empty
        assert!(top.is_empty());
    }

    #[test]
    fn test_decay_halves_frequencies() {
        let mut acc = KeywordAccumulator::new();
        acc.freq.insert("test".to_string(), 8.0);
        acc.freq.insert("word".to_string(), 4.0);

        acc.decay();

        assert_eq!(acc.freq.get("test"), Some(&4.0));
        assert_eq!(acc.freq.get("word"), Some(&2.0));
    }

    #[test]
    fn test_decay_removes_small_frequencies() {
        let mut acc = KeywordAccumulator::new();
        acc.freq.insert("test".to_string(), 0.005);
        acc.freq.insert("word".to_string(), 1.0);

        acc.decay();

        // 0.005 * 0.5 = 0.0025, which is < 0.01, so should be removed
        assert!(!acc.freq.contains_key("test"));
        // 1.0 * 0.5 = 0.5, which is >= 0.01, so should remain
        assert_eq!(acc.freq.get("word"), Some(&0.5));
    }

    #[test]
    fn test_top_k_returns_highest_first() {
        let mut acc = KeywordAccumulator::new();
        acc.freq.insert("alpha".to_string(), 10.0);
        acc.freq.insert("beta".to_string(), 5.0);
        acc.freq.insert("gamma".to_string(), 15.0);
        acc.freq.insert("delta".to_string(), 3.0);

        let top = acc.top_k(2);
        assert_eq!(top, vec!["gamma".to_string(), "alpha".to_string()]);
    }

    #[test]
    fn test_top_k_respects_limit() {
        let mut acc = KeywordAccumulator::new();
        acc.freq.insert("one".to_string(), 1.0);
        acc.freq.insert("two".to_string(), 2.0);
        acc.freq.insert("three".to_string(), 3.0);

        let top = acc.top_k(2);
        assert_eq!(top.len(), 2);
    }

    #[test]
    fn test_add_keywords() {
        let mut acc = KeywordAccumulator::new();
        acc.add_keywords(&[
            "python".to_string(),
            "rust".to_string(),
            "javascript".to_string(),
        ]);

        let top = acc.top_k(10);
        assert_eq!(top.len(), 3);
        assert!(top.contains(&"python".to_string()));
        assert!(top.contains(&"rust".to_string()));
        assert!(top.contains(&"javascript".to_string()));
    }

    #[test]
    fn test_add_keywords_filters_stop_words() {
        let mut acc = KeywordAccumulator::new();
        acc.add_keywords(&["the".to_string(), "python".to_string(), "is".to_string()]);

        let top = acc.top_k(10);
        assert_eq!(top.len(), 1);
        assert!(top.contains(&"python".to_string()));
    }

    #[test]
    fn test_lowercase_normalization() {
        let mut acc = KeywordAccumulator::new();
        let args = serde_json::json!({
            "text": "Python RUST JavaScript"
        });
        acc.extract_from_args(&args);

        let top = acc.top_k(10);
        assert!(top.contains(&"python".to_string()));
        assert!(top.contains(&"rust".to_string()));
        assert!(top.contains(&"javascript".to_string()));
    }

    #[test]
    fn test_frequency_accumulation() {
        let mut acc = KeywordAccumulator::new();
        let args1 = serde_json::json!({ "text": "test test test" });
        let args2 = serde_json::json!({ "text": "test word" });

        acc.extract_from_args(&args1);
        acc.extract_from_args(&args2);

        // "test" should have frequency 4.0 (3 from first, 1 from second)
        assert_eq!(acc.freq.get("test"), Some(&4.0));
        assert_eq!(acc.freq.get("word"), Some(&1.0));
    }
}
