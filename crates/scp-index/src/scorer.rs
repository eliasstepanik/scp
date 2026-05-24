use crate::tags::TagScorer;
use crate::tfidf::TfIdfIndex;
use crate::usage::UsageTracker;
use crate::ToolEntry;
use std::collections::HashMap;

/// A tool with its computed score
#[derive(Debug, Clone)]
pub struct ScoredTool {
    /// Qualified name of the tool (server.tool_name)
    pub qualified_name: String,
    /// Computed relevance score
    pub score: f32,
}

/// Parameters for scoring and ranking tools
#[derive(Copy, Clone)]
pub struct ScoringParams<'a> {
    /// Slice of (qualified_name, ToolEntry) tuples to score
    pub tools: &'a [(&'a str, &'a ToolEntry)],
    /// Keywords from session context for matching
    pub keywords: &'a [String],
    /// Profile identifier for usage tracking
    pub profile: &'a str,
    /// Usage tracker for computing usage scores
    pub usage: &'a UsageTracker,
    /// TF-IDF index for computing text relevance scores
    pub tfidf: &'a TfIdfIndex,
    /// Tools that must appear in results regardless of score
    pub always_include: &'a [String],
    /// Optional precomputed embedding scores (qualified_name -> score)
    pub precomputed_scores: Option<&'a HashMap<String, f32>>,
}

/// Combines multiple scoring engines (tags, tfidf) with usage tracking
pub struct ScoringPipeline {
    /// Scoring engine type: "tags", "tfidf", or "embedding"
    pub engine: String,
    primary_weight: f32, // default 0.7
    usage_weight: f32,   // default 0.3
}

impl ScoringPipeline {
    /// Create a new scoring pipeline with specified engine and weights
    pub fn new(engine: &str, primary_weight: f32, usage_weight: f32) -> Self {
        Self {
            engine: engine.to_string(),
            primary_weight,
            usage_weight,
        }
    }

    /// Score and rank all provided tools.
    ///
    /// # Arguments
    /// * `params` - ScoringParams containing all scoring parameters
    ///
    /// # Returns
    /// Sorted `Vec<ScoredTool>` (highest score first)
    pub fn score_and_rank(&self, params: ScoringParams) -> Vec<ScoredTool> {
        let mut scored = Vec::new();

        for (qualified_name, entry) in params.tools {
            // Compute primary score based on engine
            let primary_score = if self.engine == "embedding" {
                // Use precomputed embedding scores if available
                params
                    .precomputed_scores
                    .and_then(|scores| scores.get(*qualified_name).copied())
                    .unwrap_or(0.0)
            } else if self.engine == "tags" {
                TagScorer::score(&entry.tags, params.keywords)
            } else {
                // Default to tfidf
                params.tfidf.score(qualified_name, params.keywords)
            };

            // Compute usage score
            let usage_score = params.usage.score(params.profile, qualified_name);

            // Combine scores
            let mut final_score =
                self.primary_weight * primary_score + self.usage_weight * usage_score;

            // Override score for always_include tools
            if params.always_include.contains(&qualified_name.to_string()) {
                final_score = 2.0;
            }

            scored.push(ScoredTool {
                qualified_name: qualified_name.to_string(),
                score: final_score,
            });
        }

        // Sort by score descending
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tool(name: &str, tags: Vec<String>, description: Option<String>) -> ToolEntry {
        ToolEntry {
            original_name: name.to_string(),
            qualified_name: format!("server.{}", name),
            server_name: "server".to_string(),
            description,
            input_schema: serde_json::json!({}),
            tags,
            avg_response_tokens: 100.0,
            call_count: 0,
        }
    }

    #[test]
    fn test_score_and_rank_with_tfidf_engine() {
        let pipeline = ScoringPipeline::new("tfidf", 0.7, 0.3);
        let usage = UsageTracker::new();

        // Build TF-IDF index with two tools
        let tools_for_index = vec![
            (
                "server.read_file".to_string(),
                Some("filesystem read file operations".to_string()),
            ),
            (
                "server.web_search".to_string(),
                Some("web search query results".to_string()),
            ),
        ];
        let tfidf = TfIdfIndex::build(&tools_for_index);

        // Create tool entries
        let tool1 = create_test_tool(
            "read_file",
            vec![],
            Some("filesystem read file operations".to_string()),
        );
        let tool2 = create_test_tool(
            "web_search",
            vec![],
            Some("web search query results".to_string()),
        );

        let tools = vec![("server.read_file", &tool1), ("server.web_search", &tool2)];

        let keywords = vec!["file".to_string(), "read".to_string()];
        let params = ScoringParams {
            tools: &tools,
            keywords: &keywords,
            profile: "profile1",
            usage: &usage,
            tfidf: &tfidf,
            always_include: &[],
            precomputed_scores: None,
        };
        let scored = pipeline.score_and_rank(params);

        // Tool with matching description should rank higher
        assert_eq!(scored.len(), 2);
        assert_eq!(scored[0].qualified_name, "server.read_file");
        assert!(scored[0].score > scored[1].score);
    }

    #[test]
    fn test_score_and_rank_with_tags_engine() {
        let pipeline = ScoringPipeline::new("tags", 0.7, 0.3);
        let usage = UsageTracker::new();
        let tfidf = TfIdfIndex::build(&[]);

        // Create tools with tags
        let tool1 = create_test_tool(
            "search",
            vec!["search".to_string(), "query".to_string()],
            None,
        );
        let tool2 = create_test_tool("read", vec!["file".to_string(), "io".to_string()], None);

        let tools = vec![("server.search", &tool1), ("server.read", &tool2)];

        let keywords = vec!["search".to_string()];
        let params = ScoringParams {
            tools: &tools,
            keywords: &keywords,
            profile: "profile1",
            usage: &usage,
            tfidf: &tfidf,
            always_include: &[],
            precomputed_scores: None,
        };
        let scored = pipeline.score_and_rank(params);

        // Tool with matching tags should rank higher
        assert_eq!(scored.len(), 2);
        assert_eq!(scored[0].qualified_name, "server.search");
        assert!(scored[0].score > scored[1].score);
    }

    #[test]
    fn test_always_include_tools_float_to_top() {
        let pipeline = ScoringPipeline::new("tfidf", 0.7, 0.3);
        let usage = UsageTracker::new();
        let tfidf = TfIdfIndex::build(&[]);

        let tool1 = create_test_tool("tool1", vec![], None);
        let tool2 = create_test_tool("tool2", vec![], None);
        let tool3 = create_test_tool("tool3", vec![], None);

        let tools = vec![
            ("server.tool1", &tool1),
            ("server.tool2", &tool2),
            ("server.tool3", &tool3),
        ];

        let keywords = vec![];
        let always_include = vec!["server.tool3".to_string()];
        let params = ScoringParams {
            tools: &tools,
            keywords: &keywords,
            profile: "profile1",
            usage: &usage,
            tfidf: &tfidf,
            always_include: &always_include,
            precomputed_scores: None,
        };
        let scored = pipeline.score_and_rank(params);

        // tool3 should be first with score 2.0
        assert_eq!(scored[0].qualified_name, "server.tool3");
        assert_eq!(scored[0].score, 2.0);
    }

    #[test]
    fn test_weight_combination_affects_score() {
        let pipeline_high_primary = ScoringPipeline::new("tfidf", 0.9, 0.1);
        let pipeline_high_usage = ScoringPipeline::new("tfidf", 0.1, 0.9);

        let mut usage = UsageTracker::new();
        // Record many calls for tool1
        for _ in 0..10 {
            usage.record_call("profile1", "server.tool1");
        }
        // Record few calls for tool2
        usage.record_call("profile1", "server.tool2");

        let tfidf = TfIdfIndex::build(&[
            (
                "server.tool1".to_string(),
                Some("test description".to_string()),
            ),
            (
                "server.tool2".to_string(),
                Some("test description".to_string()),
            ),
        ]);

        let tool1 = create_test_tool("tool1", vec![], Some("test description".to_string()));
        let tool2 = create_test_tool("tool2", vec![], Some("test description".to_string()));

        let tools = vec![("server.tool1", &tool1), ("server.tool2", &tool2)];

        let keywords = vec!["test".to_string()];

        let params = ScoringParams {
            tools: &tools,
            keywords: &keywords,
            profile: "profile1",
            usage: &usage,
            tfidf: &tfidf,
            always_include: &[],
            precomputed_scores: None,
        };
        let scored_high_primary = pipeline_high_primary.score_and_rank(params);
        let scored_high_usage = pipeline_high_usage.score_and_rank(params);

        // Both should rank tool1 first (higher usage), but with different scores
        assert_eq!(scored_high_primary[0].qualified_name, "server.tool1");
        assert_eq!(scored_high_usage[0].qualified_name, "server.tool1");
        // The scores should be different because the weights are different
        // and tool1 has higher usage than tool2
        assert_ne!(scored_high_primary[0].score, scored_high_usage[0].score);
    }

    #[test]
    fn test_empty_keywords_with_usage_tracking() {
        let pipeline = ScoringPipeline::new("tfidf", 0.7, 0.3);

        let mut usage = UsageTracker::new();
        usage.record_call("profile1", "server.tool1");
        usage.record_call("profile1", "server.tool1");
        usage.record_call("profile1", "server.tool2");

        let tfidf = TfIdfIndex::build(&[]);

        let tool1 = create_test_tool("tool1", vec![], None);
        let tool2 = create_test_tool("tool2", vec![], None);

        let tools = vec![("server.tool1", &tool1), ("server.tool2", &tool2)];

        let keywords = vec![];
        let params = ScoringParams {
            tools: &tools,
            keywords: &keywords,
            profile: "profile1",
            usage: &usage,
            tfidf: &tfidf,
            always_include: &[],
            precomputed_scores: None,
        };
        let scored = pipeline.score_and_rank(params);

        // With no keywords, tfidf score is 0, so usage score dominates
        // tool1 has higher usage, so it should rank first
        assert_eq!(scored[0].qualified_name, "server.tool1");
        assert!(scored[0].score > scored[1].score);
    }

    #[test]
    fn test_multiple_always_include_tools() {
        let pipeline = ScoringPipeline::new("tfidf", 0.7, 0.3);
        let usage = UsageTracker::new();
        let tfidf = TfIdfIndex::build(&[]);

        let tool1 = create_test_tool("tool1", vec![], None);
        let tool2 = create_test_tool("tool2", vec![], None);
        let tool3 = create_test_tool("tool3", vec![], None);

        let tools = vec![
            ("server.tool1", &tool1),
            ("server.tool2", &tool2),
            ("server.tool3", &tool3),
        ];

        let keywords = vec![];
        let always_include = vec!["server.tool1".to_string(), "server.tool3".to_string()];
        let params = ScoringParams {
            tools: &tools,
            keywords: &keywords,
            profile: "profile1",
            usage: &usage,
            tfidf: &tfidf,
            always_include: &always_include,
            precomputed_scores: None,
        };
        let scored = pipeline.score_and_rank(params);

        // Both tool1 and tool3 should have score 2.0
        let always_included_scores: Vec<_> = scored
            .iter()
            .filter(|s| always_include.contains(&s.qualified_name))
            .collect();

        assert_eq!(always_included_scores.len(), 2);
        for scored_tool in always_included_scores {
            assert_eq!(scored_tool.score, 2.0);
        }
    }

    #[test]
    fn test_sorted_by_score_descending() {
        let pipeline = ScoringPipeline::new("tfidf", 0.7, 0.3);

        let mut usage = UsageTracker::new();
        usage.record_call("profile1", "server.tool1");
        usage.record_call("profile1", "server.tool1");
        usage.record_call("profile1", "server.tool1");
        usage.record_call("profile1", "server.tool2");
        usage.record_call("profile1", "server.tool2");

        let tfidf = TfIdfIndex::build(&[]);

        let tool1 = create_test_tool("tool1", vec![], None);
        let tool2 = create_test_tool("tool2", vec![], None);
        let tool3 = create_test_tool("tool3", vec![], None);

        let tools = vec![
            ("server.tool1", &tool1),
            ("server.tool2", &tool2),
            ("server.tool3", &tool3),
        ];

        let keywords = vec![];
        let params = ScoringParams {
            tools: &tools,
            keywords: &keywords,
            profile: "profile1",
            usage: &usage,
            tfidf: &tfidf,
            always_include: &[],
            precomputed_scores: None,
        };
        let scored = pipeline.score_and_rank(params);

        // Verify descending order
        for i in 0..scored.len() - 1 {
            assert!(scored[i].score >= scored[i + 1].score);
        }
    }
}
