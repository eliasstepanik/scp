use std::collections::HashMap;

/// Tracks per-profile tool call counts and computes Bayesian scores.
pub struct UsageTracker {
    /// profile -> (tool_qualified_name -> call_count)
    counts: HashMap<String, HashMap<String, u64>>,
}

impl UsageTracker {
    /// Create a new usage tracker
    pub fn new() -> Self {
        Self {
            counts: HashMap::new(),
        }
    }

    /// Record a tool call for a given profile.
    pub fn record_call(&mut self, profile: &str, qualified_name: &str) {
        self.counts
            .entry(profile.to_string())
            .or_default()
            .entry(qualified_name.to_string())
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }

    /// Compute Bayesian score with Laplace smoothing (α=1):
    ///   score = (count + 1) / (total_calls_for_profile + num_tools)
    /// where num_tools is the number of distinct tools ever seen for this profile.
    /// Returns 0.0 if profile has no history and there are zero known tools.
    /// For an unseen tool under a known profile, count = 0 (Laplace floor).
    pub fn score(&self, profile: &str, qualified_name: &str) -> f32 {
        match self.counts.get(profile) {
            Some(profile_counts) => {
                let num_tools = profile_counts.len() as f32;
                if num_tools == 0.0 {
                    return 0.0;
                }

                let count = profile_counts.get(qualified_name).copied().unwrap_or(0) as f32;
                let total_calls: u64 = profile_counts.values().sum();

                (count + 1.0) / (total_calls as f32 + num_tools)
            }
            None => {
                // Profile has no history. Return uniform prior if we know any tools,
                // otherwise 0.0
                0.0
            }
        }
    }

    /// Return the total call count for a tool under a profile.
    pub fn call_count(&self, profile: &str, qualified_name: &str) -> u64 {
        self.counts
            .get(profile)
            .and_then(|counts| counts.get(qualified_name))
            .copied()
            .unwrap_or(0)
    }
}

impl Default for UsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_call_increments_count() {
        let mut tracker = UsageTracker::new();
        tracker.record_call("profile1", "tool1");
        tracker.record_call("profile1", "tool1");
        tracker.record_call("profile1", "tool2");

        assert_eq!(tracker.call_count("profile1", "tool1"), 2);
        assert_eq!(tracker.call_count("profile1", "tool2"), 1);
    }

    #[test]
    fn test_frequently_called_tool_scores_higher() {
        let mut tracker = UsageTracker::new();

        // Record tool1 5 times, tool2 1 time
        for _ in 0..5 {
            tracker.record_call("profile1", "tool1");
        }
        tracker.record_call("profile1", "tool2");

        let score1 = tracker.score("profile1", "tool1");
        let score2 = tracker.score("profile1", "tool2");

        assert!(score1 > score2, "Frequently called tool should score higher");
    }

    #[test]
    fn test_unseen_tool_gets_nonzero_floor_score() {
        let mut tracker = UsageTracker::new();

        // Record some calls for tool1
        tracker.record_call("profile1", "tool1");
        tracker.record_call("profile1", "tool1");

        // tool2 has never been called
        let score_unseen = tracker.score("profile1", "tool2");

        assert!(score_unseen > 0.0, "Unseen tool should get non-zero floor score (Laplace smoothing)");
    }

    #[test]
    fn test_profile_with_no_history_returns_zero() {
        let tracker = UsageTracker::new();

        let score = tracker.score("unknown_profile", "tool1");

        assert_eq!(score, 0.0, "Profile with no history should return 0.0");
    }

    #[test]
    fn test_scores_sum_approximately_to_one() {
        let mut tracker = UsageTracker::new();

        // Record calls for 3 tools
        tracker.record_call("profile1", "tool1");
        tracker.record_call("profile1", "tool1");
        tracker.record_call("profile1", "tool2");
        tracker.record_call("profile1", "tool3");

        let score1 = tracker.score("profile1", "tool1");
        let score2 = tracker.score("profile1", "tool2");
        let score3 = tracker.score("profile1", "tool3");

        let sum = score1 + score2 + score3;

        // With Laplace smoothing, sum of known tools should be approximately 1.0
        // (count + 1) / (total + num_tools) for each tool
        // sum = (2+1 + 1+1 + 1+1) / (4 + 3) = 7 / 7 = 1.0
        assert!((sum - 1.0).abs() < 0.0001, "Scores should sum to approximately 1.0, got {}", sum);
    }

    #[test]
    fn test_multiple_profiles_independent() {
        let mut tracker = UsageTracker::new();

        tracker.record_call("profile1", "tool1");
        tracker.record_call("profile1", "tool1");

        tracker.record_call("profile2", "tool1");

        let count1 = tracker.call_count("profile1", "tool1");
        let count2 = tracker.call_count("profile2", "tool1");

        assert_eq!(count1, 2);
        assert_eq!(count2, 1);
    }

    #[test]
    fn test_call_count_returns_zero_for_unknown() {
        let tracker = UsageTracker::new();

        assert_eq!(tracker.call_count("unknown_profile", "unknown_tool"), 0);
    }
}
