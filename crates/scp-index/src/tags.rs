use std::collections::HashSet;

/// Computes Jaccard similarity between tool tags and context keywords
pub struct TagScorer;

impl TagScorer {
    /// Compute Jaccard similarity: |intersection| / |union|
    ///
    /// # Arguments
    /// * `tool_tags` - Tags associated with a tool
    /// * `context_keywords` - Keywords from session context
    ///
    /// # Returns
    /// Jaccard similarity score in range [0.0, 1.0]:
    /// - Empty keywords → 0.5 (neutral; don't penalize when no context yet)
    /// - Empty tool tags → 0.0
    /// - Both empty → 0.5 (neutral)
    /// - Both non-empty → standard Jaccard: |A ∩ B| / |A ∪ B|
    pub fn score(tool_tags: &[String], context_keywords: &[String]) -> f32 {
        // Handle empty cases
        if context_keywords.is_empty() {
            return 0.5; // Neutral when no context
        }

        if tool_tags.is_empty() {
            return 0.0; // No tags means no match
        }

        // Convert to lowercase sets for case-insensitive comparison
        let tags_set: HashSet<String> = tool_tags
            .iter()
            .map(|t| t.to_lowercase())
            .collect();

        let keywords_set: HashSet<String> = context_keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();

        // Compute intersection and union
        let intersection = tags_set
            .intersection(&keywords_set)
            .count();
        let union = tags_set.union(&keywords_set).count();

        // Jaccard similarity
        if union == 0 {
            0.5 // Both empty (shouldn't reach here due to earlier checks)
        } else {
            intersection as f32 / union as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_vec(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_exact_match() {
        let tags = str_vec(&["search", "index"]);
        let keywords = str_vec(&["search", "index"]);
        assert_eq!(TagScorer::score(&tags, &keywords), 1.0);
    }

    #[test]
    fn test_no_overlap() {
        let tags = str_vec(&["search", "index"]);
        let keywords = str_vec(&["read", "write"]);
        assert_eq!(TagScorer::score(&tags, &keywords), 0.0);
    }

    #[test]
    fn test_partial_overlap() {
        // tags = ["a", "b"], keywords = ["b", "c"]
        // intersection = {"b"} = 1
        // union = {"a", "b", "c"} = 3
        // Jaccard = 1/3 ≈ 0.333...
        let tags = str_vec(&["a", "b"]);
        let keywords = str_vec(&["b", "c"]);
        let score = TagScorer::score(&tags, &keywords);
        assert!((score - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_empty_keywords() {
        let tags = str_vec(&["search", "index"]);
        let keywords = vec![];
        assert_eq!(TagScorer::score(&tags, &keywords), 0.5);
    }

    #[test]
    fn test_empty_tags() {
        let tags = vec![];
        let keywords = str_vec(&["search", "index"]);
        assert_eq!(TagScorer::score(&tags, &keywords), 0.0);
    }

    #[test]
    fn test_both_empty() {
        let tags = vec![];
        let keywords = vec![];
        assert_eq!(TagScorer::score(&tags, &keywords), 0.5);
    }

    #[test]
    fn test_case_insensitive() {
        let tags = str_vec(&["Search", "INDEX"]);
        let keywords = str_vec(&["search", "index"]);
        assert_eq!(TagScorer::score(&tags, &keywords), 1.0);
    }

    #[test]
    fn test_single_tag_match() {
        let tags = str_vec(&["search"]);
        let keywords = str_vec(&["search"]);
        assert_eq!(TagScorer::score(&tags, &keywords), 1.0);
    }

    #[test]
    fn test_single_tag_no_match() {
        let tags = str_vec(&["search"]);
        let keywords = str_vec(&["read"]);
        assert_eq!(TagScorer::score(&tags, &keywords), 0.0);
    }

    #[test]
    fn test_duplicate_tags() {
        // Duplicates should be treated as a single item in the set
        let tags = str_vec(&["search", "search", "index"]);
        let keywords = str_vec(&["search"]);
        // tags set = {"search", "index"}
        // keywords set = {"search"}
        // intersection = {"search"} = 1
        // union = {"search", "index"} = 2
        // Jaccard = 1/2 = 0.5
        let score = TagScorer::score(&tags, &keywords);
        assert!((score - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_multiple_partial_overlap() {
        // tags = ["a", "b", "c"], keywords = ["b", "c", "d"]
        // intersection = {"b", "c"} = 2
        // union = {"a", "b", "c", "d"} = 4
        // Jaccard = 2/4 = 0.5
        let tags = str_vec(&["a", "b", "c"]);
        let keywords = str_vec(&["b", "c", "d"]);
        let score = TagScorer::score(&tags, &keywords);
        assert!((score - 0.5).abs() < 0.001);
    }
}
