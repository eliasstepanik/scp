use std::collections::HashMap;

/// Stop words to filter during tokenization
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "it", "in", "on", "at", "to", "do", "for", "of", "and", "or", "not",
    "be", "by", "as", "with", "this", "that", "from", "are", "was", "has", "have", "but", "if",
    "then", "than", "so", "use", "used",
];

/// TF-IDF index for scoring tools based on descriptions
#[derive(Default)]
pub struct TfIdfIndex {
    /// IDF scores per term across the corpus
    idf: HashMap<String, f32>,
    /// Per-tool TF-IDF vectors: qualified_name -> {term -> tfidf_weight}
    vectors: HashMap<String, HashMap<String, f32>>,
}

impl TfIdfIndex {
    /// Build a TF-IDF index from a list of (qualified_name, description) pairs.
    /// Call this after fetching tools from backends.
    pub fn build(tools: &[(String, Option<String>)]) -> Self {
        if tools.is_empty() {
            return Self {
                idf: HashMap::new(),
                vectors: HashMap::new(),
            };
        }

        // Step 1: Tokenize all descriptions and collect term frequencies per document
        let mut doc_terms: HashMap<String, HashMap<String, u32>> = HashMap::new();
        let mut doc_term_counts: HashMap<String, u32> = HashMap::new();
        let mut term_doc_freq: HashMap<String, u32> = HashMap::new();

        for (qualified_name, description) in tools {
            let tokens = tokenize(description.as_deref().unwrap_or(""));

            if tokens.is_empty() {
                doc_terms.insert(qualified_name.clone(), HashMap::new());
                doc_term_counts.insert(qualified_name.clone(), 0);
                continue;
            }

            let mut term_counts: HashMap<String, u32> = HashMap::new();
            for token in &tokens {
                *term_counts.entry(token.clone()).or_insert(0) += 1;
                *term_doc_freq.entry(token.clone()).or_insert(0) += 1;
            }

            doc_term_counts.insert(qualified_name.clone(), tokens.len() as u32);
            doc_terms.insert(qualified_name.clone(), term_counts);
        }

        // Step 2: Compute IDF scores
        let num_docs = tools.len() as f32;
        let mut idf: HashMap<String, f32> = HashMap::new();

        for (term, doc_freq) in term_doc_freq {
            let idf_score = (num_docs / doc_freq as f32).ln();
            idf.insert(term, idf_score.max(0.0));
        }

        // Step 3: Compute TF-IDF vectors and L2-normalize
        let mut vectors: HashMap<String, HashMap<String, f32>> = HashMap::new();

        for (qualified_name, term_counts) in doc_terms {
            let total_terms = doc_term_counts[&qualified_name] as f32;

            let mut vector: HashMap<String, f32> = HashMap::new();
            let mut magnitude_sq = 0.0f32;

            if total_terms > 0.0 {
                for (term, count) in term_counts {
                    let tf = count as f32 / total_terms;
                    let idf_score = idf.get(&term).copied().unwrap_or(0.0);
                    let tfidf = tf * idf_score;

                    magnitude_sq += tfidf * tfidf;
                    vector.insert(term, tfidf);
                }
            }

            // L2-normalize
            if magnitude_sq > 0.0 {
                let magnitude = magnitude_sq.sqrt();
                for weight in vector.values_mut() {
                    *weight /= magnitude;
                }
            }

            vectors.insert(qualified_name.clone(), vector);
        }

        Self { idf, vectors }
    }

    /// Score a tool against query terms using cosine similarity.
    /// - Empty query → 0.0
    /// - Tool not in index or no description → 0.0
    /// - Returns f32 in [0.0, 1.0]
    pub fn score(&self, qualified_name: &str, query_terms: &[String]) -> f32 {
        if query_terms.is_empty() {
            return 0.0;
        }

        let tool_vector = match self.vectors.get(qualified_name) {
            Some(v) => v,
            None => return 0.0,
        };

        // Build query vector using IDF scores
        let mut query_vector: HashMap<String, f32> = HashMap::new();
        let mut magnitude_sq = 0.0f32;

        for term in query_terms {
            let idf_score = self.idf.get(term).copied().unwrap_or(0.0);
            if idf_score > 0.0 {
                magnitude_sq += idf_score * idf_score;
                query_vector.insert(term.clone(), idf_score);
            }
        }

        if magnitude_sq == 0.0 {
            return 0.0;
        }

        // L2-normalize query vector
        let query_magnitude = magnitude_sq.sqrt();
        for weight in query_vector.values_mut() {
            *weight /= query_magnitude;
        }

        // Compute cosine similarity (dot product of normalized vectors)
        let mut dot_product = 0.0f32;
        for (term, query_weight) in &query_vector {
            if let Some(tool_weight) = tool_vector.get(term) {
                dot_product += query_weight * tool_weight;
            }
        }

        dot_product.clamp(0.0, 1.0)
    }
}



/// Tokenize a description: lowercase, split on non-alnum, filter < 3 chars and stop words
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.len() >= 3 && !STOP_WORDS.contains(token))
        .map(|token| token.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_with_two_tools_different_descriptions() {
        let tools = vec![
            (
                "server1.read_file".to_string(),
                Some("filesystem read file operations".to_string()),
            ),
            (
                "server2.web_search".to_string(),
                Some("web search query results".to_string()),
            ),
        ];

        let index = TfIdfIndex::build(&tools);

        // Query "file read" should score tool A higher than tool B
        let query = vec!["file".to_string(), "read".to_string()];
        let score_a = index.score("server1.read_file", &query);
        let score_b = index.score("server2.web_search", &query);

        assert!(score_a > score_b, "Tool A should score higher for 'file read' query");
        assert!(score_a > 0.0, "Tool A should have non-zero score");
        assert!(score_b >= 0.0, "Tool B should have non-negative score");
    }

    #[test]
    fn test_build_with_empty_descriptions() {
        let tools = vec![
            ("server1.tool1".to_string(), None),
            ("server1.tool2".to_string(), Some(String::new())),
        ];

        let index = TfIdfIndex::build(&tools);

        let query = vec!["test".to_string()];
        assert_eq!(index.score("server1.tool1", &query), 0.0);
        assert_eq!(index.score("server1.tool2", &query), 0.0);
    }

    #[test]
    fn test_score_with_empty_query() {
        let tools = vec![(
            "server1.tool1".to_string(),
            Some("some description".to_string()),
        )];

        let index = TfIdfIndex::build(&tools);

        assert_eq!(index.score("server1.tool1", &[]), 0.0);
    }

    #[test]
    fn test_score_for_unknown_tool() {
        let tools = vec![(
            "server1.tool1".to_string(),
            Some("some description".to_string()),
        )];

        let index = TfIdfIndex::build(&tools);

        let query = vec!["test".to_string()];
        assert_eq!(index.score("unknown.tool", &query), 0.0);
    }

    #[test]
    fn test_score_bounded_in_range() {
        let tools = vec![
            (
                "server1.tool1".to_string(),
                Some("filesystem read file operations".to_string()),
            ),
            (
                "server1.tool2".to_string(),
                Some("web search query results".to_string()),
            ),
            (
                "server1.tool3".to_string(),
                Some("database write operations".to_string()),
            ),
        ];

        let index = TfIdfIndex::build(&tools);

        let query = vec!["file".to_string(), "read".to_string()];
        for tool in &["server1.tool1", "server1.tool2", "server1.tool3"] {
            let score = index.score(tool, &query);
            assert!(
                score >= 0.0 && score <= 1.0,
                "Score for {} should be in [0.0, 1.0], got {}",
                tool,
                score
            );
        }
    }

    #[test]
    fn test_tokenization_filters_stop_words() {
        let tokens = tokenize("the quick brown fox jumps over the lazy dog");
        // "the" is a stop word; "quick", "brown", "fox", "jumps", "over", "lazy" should remain
        assert!(!tokens.contains(&"the".to_string()));
        assert!(tokens.contains(&"quick".to_string()));
        assert!(tokens.contains(&"brown".to_string()));
        assert!(tokens.contains(&"fox".to_string()));
        assert!(tokens.contains(&"jumps".to_string()));
        assert!(tokens.contains(&"over".to_string()));
        assert!(tokens.contains(&"lazy".to_string()));
    }

    #[test]
    fn test_tokenization_filters_short_tokens() {
        let tokens = tokenize("a ab abc abcd");
        // "a" and "ab" are < 3 chars, "abc" and "abcd" should remain
        assert!(!tokens.contains(&"a".to_string()));
        assert!(!tokens.contains(&"ab".to_string()));
        assert!(tokens.contains(&"abc".to_string()));
        assert!(tokens.contains(&"abcd".to_string()));
    }

    #[test]
    fn test_tokenization_lowercase() {
        let tokens = tokenize("FileSystem ReadFile");
        assert!(tokens.contains(&"filesystem".to_string()));
        assert!(tokens.contains(&"readfile".to_string()));
    }

    #[test]
    fn test_empty_tools_list() {
        let index = TfIdfIndex::build(&[]);
        assert_eq!(index.score("any.tool", &["query".to_string()]), 0.0);
    }

    #[test]
    fn test_query_with_unknown_terms() {
        let tools = vec![(
            "server1.tool1".to_string(),
            Some("filesystem read file operations".to_string()),
        )];

        let index = TfIdfIndex::build(&tools);

        // Query with terms not in any description
        let query = vec!["xyzabc".to_string(), "qwerty".to_string()];
        assert_eq!(index.score("server1.tool1", &query), 0.0);
    }

    #[test]
    fn test_identical_descriptions_equal_scores() {
        let tools = vec![
            (
                "server1.tool1".to_string(),
                Some("filesystem read file operations".to_string()),
            ),
            (
                "server1.tool2".to_string(),
                Some("filesystem read file operations".to_string()),
            ),
        ];

        let index = TfIdfIndex::build(&tools);

        let query = vec!["file".to_string(), "read".to_string()];
        let score1 = index.score("server1.tool1", &query);
        let score2 = index.score("server1.tool2", &query);

        assert!((score1 - score2).abs() < 0.0001, "Identical descriptions should have equal scores");
    }
}
