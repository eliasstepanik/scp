use crate::chunker::Chunk;

/// Progressive disclosure annotator for filtered content.
///
/// When chunks are dropped during filtering, this annotator appends a hint
/// to the content to inform the user that some content was filtered.
pub struct ProgressiveDisclosureAnnotator {
    /// Whether progressive disclosure annotation is enabled.
    pub enabled: bool,
}

impl ProgressiveDisclosureAnnotator {
    /// Create a new annotator with the given enabled state.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Annotate content with a disclosure hint if chunks were dropped.
    ///
    /// If chunks were dropped, append a disclosure hint.
    /// Returns the (possibly annotated) content string and the dropped chunks.
    ///
    /// - If no chunks were dropped (dropped_chunks is empty) → return (content, empty vec)
    /// - If !self.enabled → return (content, empty vec)
    /// - Otherwise append hint using template substitution and return (annotated_content, dropped_chunks)
    pub fn annotate(
        &self,
        content: String,
        shown: usize,
        dropped_chunks: Vec<Chunk>,
        request_id: &str,
        hint_text: &str,
    ) -> (String, Vec<Chunk>) {
        // If not enabled, return unchanged with no dropped chunks
        if !self.enabled {
            return (content, vec![]);
        }

        // If no chunks were dropped, return unchanged
        if dropped_chunks.is_empty() {
            return (content, vec![]);
        }

        // Generate hint using template substitution
        let total = shown + dropped_chunks.len();
        let hint = hint_text
            .replace("{shown}", &shown.to_string())
            .replace("{total}", &total.to_string())
            .replace("{id}", request_id);

        let annotated = content + "\n\n" + &hint;
        (annotated, dropped_chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annotate_when_no_chunks_dropped() {
        let annotator = ProgressiveDisclosureAnnotator::new(true);
        let content = "This is the content".to_string();
        let hint_text =
            "[SCP: {shown} of {total} chunks shown. Some content was filtered for relevance.]";
        let (result_content, dropped) =
            annotator.annotate(content.clone(), 5, vec![], "req-1", hint_text);
        assert_eq!(result_content, content);
        assert!(dropped.is_empty());
    }

    #[test]
    fn test_annotate_when_some_filtered() {
        let annotator = ProgressiveDisclosureAnnotator::new(true);
        let content = "This is the content".to_string();
        let hint_text =
            "[SCP: {shown} of {total} chunks shown. Some content was filtered for relevance.]";
        let dropped_chunks = vec![
            Chunk::new("dropped1".to_string(), 5),
            Chunk::new("dropped2".to_string(), 6),
        ];
        let (result_content, returned_dropped) = annotator.annotate(
            content.clone(),
            3,
            dropped_chunks.clone(),
            "req-1",
            hint_text,
        );
        assert!(result_content.contains("[SCP:"));
        assert!(result_content.contains("3 of 5 chunks shown"));
        assert!(result_content.contains("Some content was filtered for relevance."));
        assert_eq!(returned_dropped.len(), 2);
        assert_eq!(returned_dropped, dropped_chunks);
    }

    #[test]
    fn test_annotate_when_disabled() {
        let annotator = ProgressiveDisclosureAnnotator::new(false);
        let content = "This is the content".to_string();
        let hint_text =
            "[SCP: {shown} of {total} chunks shown. Some content was filtered for relevance.]";
        let dropped_chunks = vec![
            Chunk::new("dropped1".to_string(), 5),
            Chunk::new("dropped2".to_string(), 6),
        ];
        let (result_content, returned_dropped) =
            annotator.annotate(content.clone(), 3, dropped_chunks, "req-1", hint_text);
        assert_eq!(result_content, content);
        assert!(returned_dropped.is_empty());
    }

    #[test]
    fn test_annotate_hint_contains_numbers() {
        let annotator = ProgressiveDisclosureAnnotator::new(true);
        let content = "This is the content".to_string();
        let hint_text =
            "[SCP: {shown} of {total} chunks shown. Some content was filtered for relevance.]";
        let dropped_chunks = vec![
            Chunk::new("dropped1".to_string(), 7),
            Chunk::new("dropped2".to_string(), 8),
            Chunk::new("dropped3".to_string(), 9),
            Chunk::new("dropped4".to_string(), 10),
            Chunk::new("dropped5".to_string(), 11),
        ];
        let (result_content, _) =
            annotator.annotate(content, 2, dropped_chunks, "req-1", hint_text);
        assert!(result_content.contains("2"));
        assert!(result_content.contains("7"));
    }

    #[test]
    fn test_annotate_empty_content() {
        let annotator = ProgressiveDisclosureAnnotator::new(true);
        let content = String::new();
        let hint_text =
            "[SCP: {shown} of {total} chunks shown. Some content was filtered for relevance.]";
        let dropped_chunks = vec![Chunk::new("dropped1".to_string(), 1)];
        let (result_content, _) =
            annotator.annotate(content, 1, dropped_chunks, "req-1", hint_text);
        assert!(result_content.contains("[SCP:"));
        assert!(result_content.contains("1 of 2 chunks shown"));
    }

    #[test]
    fn test_annotate_zero_shown() {
        let annotator = ProgressiveDisclosureAnnotator::new(true);
        let content = "This is the content".to_string();
        let hint_text =
            "[SCP: {shown} of {total} chunks shown. Some content was filtered for relevance.]";
        let dropped_chunks = vec![
            Chunk::new("dropped1".to_string(), 0),
            Chunk::new("dropped2".to_string(), 1),
            Chunk::new("dropped3".to_string(), 2),
            Chunk::new("dropped4".to_string(), 3),
            Chunk::new("dropped5".to_string(), 4),
        ];
        let (result_content, _) =
            annotator.annotate(content, 0, dropped_chunks, "req-1", hint_text);
        assert!(result_content.contains("[SCP:"));
        assert!(result_content.contains("0 of 5 chunks shown"));
    }

    #[test]
    fn test_annotate_with_request_id_substitution() {
        let annotator = ProgressiveDisclosureAnnotator::new(true);
        let content = "This is the content".to_string();
        let hint_text = "[SCP: {shown} of {total} chunks shown. Request ID: {id}]";
        let dropped_chunks = vec![
            Chunk::new("dropped1".to_string(), 2),
            Chunk::new("dropped2".to_string(), 3),
            Chunk::new("dropped3".to_string(), 4),
        ];
        let (result_content, _) =
            annotator.annotate(content, 2, dropped_chunks, "my-request-123", hint_text);
        assert!(result_content.contains("my-request-123"));
        assert!(result_content.contains("2 of 5 chunks shown"));
    }
}
