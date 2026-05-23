use crate::chunker::Chunk;

/// Scores chunks based on keyword overlap with query terms.
/// Uses simple substring matching for relevance scoring.
pub struct RelevanceScorer;

impl RelevanceScorer {
    /// Score each chunk against the session's context keywords using keyword overlap scoring.
    /// Mutates chunk.score in place.
    ///
    /// - query_terms: top-k keywords from session.keyword_accumulator.top_k(20)
    /// - If query_terms is empty, all chunks get score 0.5 (neutral — don't filter anything)
    pub fn score_chunks(chunks: &mut [Chunk], query_terms: &[String]) {
        for chunk in chunks.iter_mut() {
            chunk.score = Self::score_text(&chunk.text, query_terms);
        }
    }

    /// Score a single chunk text against query terms.
    /// Returns f32 in [0.0, 1.0].
    /// Algorithm: keyword overlap — for each query term, check if it appears in the lowercased chunk text.
    ///   score = (number of query terms found in chunk) / (total query terms)
    /// If query_terms is empty, return 0.5.
    pub fn score_text(text: &str, query_terms: &[String]) -> f32 {
        if query_terms.is_empty() {
            return 0.5;
        }

        let lowercased_text = text.to_lowercase();
        let matched_count = query_terms
            .iter()
            .filter(|term| lowercased_text.contains(term.as_str()))
            .count();

        let score = matched_count as f32 / query_terms.len() as f32;
        score.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_query_returns_neutral() {
        let text = "some random text";
        let query_terms: Vec<String> = vec![];
        let score = RelevanceScorer::score_text(text, &query_terms);
        assert_eq!(score, 0.5);
    }

    #[test]
    fn test_matching_chunk_scores_higher() {
        let mut chunks = vec![
            Chunk::new("this is about database".to_string(), 0),
            Chunk::new("random unrelated content".to_string(), 1),
            Chunk::new("database optimization tips".to_string(), 2),
        ];

        let query_terms = vec!["database".to_string()];
        RelevanceScorer::score_chunks(&mut chunks, &query_terms);

        // Chunks containing "database" should score 1.0
        assert_eq!(chunks[0].score, 1.0);
        assert_eq!(chunks[2].score, 1.0);
        // Chunk without "database" should score 0.0
        assert_eq!(chunks[1].score, 0.0);

        // Verify that chunks with the term score higher than those without
        assert!(chunks[0].score > chunks[1].score);
        assert!(chunks[2].score > chunks[1].score);
    }

    #[test]
    fn test_no_match_scores_zero() {
        let mut chunks = vec![
            Chunk::new("apple orange banana".to_string(), 0),
            Chunk::new("red green blue".to_string(), 1),
            Chunk::new("cat dog bird".to_string(), 2),
        ];

        let query_terms = vec!["database".to_string(), "server".to_string()];
        RelevanceScorer::score_chunks(&mut chunks, &query_terms);

        // None of the chunks contain the query terms
        assert_eq!(chunks[0].score, 0.0);
        assert_eq!(chunks[1].score, 0.0);
        assert_eq!(chunks[2].score, 0.0);
    }

    #[test]
    fn test_score_text_partial_match() {
        let text = "database and server configuration";
        let query_terms = vec![
            "database".to_string(),
            "server".to_string(),
            "network".to_string(),
            "storage".to_string(),
        ];
        let score = RelevanceScorer::score_text(text, &query_terms);
        // 2 out of 4 terms match
        assert_eq!(score, 0.5);
    }

    #[test]
    fn test_score_text_full_match() {
        let text = "database server network storage";
        let query_terms = vec![
            "database".to_string(),
            "server".to_string(),
            "network".to_string(),
            "storage".to_string(),
        ];
        let score = RelevanceScorer::score_text(text, &query_terms);
        // All 4 terms match
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_score_text_empty_query() {
        let text = "some text content";
        let query_terms: Vec<String> = vec![];
        let score = RelevanceScorer::score_text(text, &query_terms);
        assert_eq!(score, 0.5);
    }

    #[test]
    fn test_case_insensitive() {
        let text = "ERROR occurred in system";
        let query_terms = vec!["error".to_string()];
        let score = RelevanceScorer::score_text(text, &query_terms);
        // Should match despite case difference
        assert_eq!(score, 1.0);
    }
}
