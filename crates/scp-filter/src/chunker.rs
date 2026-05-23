use serde_json::Value;
use crate::token_count::count_tokens;

/// A single chunk of content with its source position and relevance score.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// The text content of this chunk.
    pub text: String,
    /// Position in original content (for ordering after score-sort).
    pub index: usize,
    /// Relevance score filled in by RelevanceScorer, default 0.0.
    pub score: f32,
}

impl Chunk {
    /// Create a new chunk with the given text and index.
    pub fn new(text: String, index: usize) -> Self {
        Self { text, index, score: 0.0 }
    }
}

/// Strategy for splitting content into chunks.
pub enum ChunkStrategy {
    /// Split on double newline (\n\n).
    Paragraph,
    /// Split on single newline (\n).
    Line,
    /// For JSON arrays: one element per chunk.
    JsonElement,
    /// Fixed token window with overlap.
    FixedSize {
        /// Token window size.
        tokens: usize,
        /// Overlap between windows.
        overlap: usize,
    },
}

/// Splits text content into chunks based on a selected strategy.
pub struct ChunkSplitter {
    /// The chunking strategy to use.
    pub strategy: ChunkStrategy,
}

impl ChunkSplitter {
    /// Create a new chunk splitter with the given strategy.
    pub fn new(strategy: ChunkStrategy) -> Self {
        Self { strategy }
    }

    /// Split text content into chunks based on the selected strategy.
    pub fn split(&self, text: &str) -> Vec<Chunk> {
        match &self.strategy {
            ChunkStrategy::Paragraph => self.split_paragraph(text),
            ChunkStrategy::Line => self.split_line(text),
            ChunkStrategy::JsonElement => {
                // JsonElement strategy requires JSON input, not plain text
                // For plain text, treat as single chunk
                if text.is_empty() {
                    vec![]
                } else {
                    vec![Chunk::new(text.to_string(), 0)]
                }
            }
            ChunkStrategy::FixedSize { tokens, overlap } => {
                self.split_fixed_size(text, *tokens, *overlap)
            }
        }
    }

    /// Split a JSON array into chunks (one element per chunk).
    /// Serializes each element to a JSON string.
    pub fn split_json_array(&self, array: &[Value]) -> Vec<Chunk> {
        array
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let text = value.to_string();
                Chunk::new(text, index)
            })
            .collect()
    }

    /// Choose the best strategy for a given text automatically.
    /// Heuristic: if text has \n\n → Paragraph; else if >5 lines → Line; else FixedSize(200, 20)
    pub fn auto(text: &str) -> Self {
        if text.contains("\n\n") {
            Self::new(ChunkStrategy::Paragraph)
        } else {
            let line_count = text.lines().count();
            if line_count > 5 {
                Self::new(ChunkStrategy::Line)
            } else {
                Self::new(ChunkStrategy::FixedSize {
                    tokens: 200,
                    overlap: 20,
                })
            }
        }
    }

    fn split_paragraph(&self, text: &str) -> Vec<Chunk> {
        text.split("\n\n")
            .filter_map(|chunk| {
                let trimmed = chunk.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .enumerate()
            .map(|(index, text)| Chunk::new(text, index))
            .collect()
    }

    fn split_line(&self, text: &str) -> Vec<Chunk> {
        text.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .enumerate()
            .map(|(index, text)| Chunk::new(text, index))
            .collect()
    }

    fn split_fixed_size(&self, text: &str, window_size: usize, overlap: usize) -> Vec<Chunk> {
        if text.is_empty() {
            return vec![];
        }

        // Split text into words
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return vec![];
        }

        let mut chunks = vec![];
        let mut chunk_index = 0;
        let mut word_index = 0;

        while word_index < words.len() {
            // Collect words for this window
            let mut window_words = vec![];
            let mut current_tokens = 0;

            // Add words until we reach the token limit
            while word_index < words.len() {
                let word = words[word_index];
                let word_tokens = count_tokens(word);

                if !window_words.is_empty() && current_tokens + word_tokens > window_size {
                    // Adding this word would exceed the limit
                    break;
                }

                window_words.push(word);
                current_tokens += word_tokens;
                word_index += 1;
            }

            // If we couldn't add even one word, add it anyway to avoid infinite loop
            if window_words.is_empty() && word_index < words.len() {
                window_words.push(words[word_index]);
                word_index += 1;
            }

            if !window_words.is_empty() {
                let chunk_text = window_words.join(" ");
                chunks.push(Chunk::new(chunk_text, chunk_index));
                chunk_index += 1;

                // Move back by overlap amount for next window
                if overlap > 0 && word_index < words.len() {
                    word_index = word_index.saturating_sub(overlap);
                }
            }
        }

        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paragraph_split() {
        let text = "foo\n\nbar\n\nbaz";
        let splitter = ChunkSplitter::new(ChunkStrategy::Paragraph);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "foo");
        assert_eq!(chunks[1].text, "bar");
        assert_eq!(chunks[2].text, "baz");
    }

    #[test]
    fn test_paragraph_empty_input() {
        let text = "";
        let splitter = ChunkSplitter::new(ChunkStrategy::Paragraph);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_paragraph_single_chunk() {
        let text = "hello world";
        let splitter = ChunkSplitter::new(ChunkStrategy::Paragraph);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "hello world");
    }

    #[test]
    fn test_line_split() {
        let text = "a\nb\nc";
        let splitter = ChunkSplitter::new(ChunkStrategy::Line);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "a");
        assert_eq!(chunks[1].text, "b");
        assert_eq!(chunks[2].text, "c");
    }

    #[test]
    fn test_line_empty_input() {
        let text = "";
        let splitter = ChunkSplitter::new(ChunkStrategy::Line);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_json_element_split() {
        let array = vec![
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
        ];
        let splitter = ChunkSplitter::new(ChunkStrategy::JsonElement);
        let chunks = splitter.split_json_array(&array);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "1");
        assert_eq!(chunks[1].text, "2");
        assert_eq!(chunks[2].text, "3");
    }

    #[test]
    fn test_json_element_single() {
        let array = vec![serde_json::json!({"key": "val"})];
        let splitter = ChunkSplitter::new(ChunkStrategy::JsonElement);
        let chunks = splitter.split_json_array(&array);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("key"));
    }

    #[test]
    fn test_fixed_size_split() {
        // Create a long text with many words
        let text = "word ".repeat(100); // 100 words
        let splitter = ChunkSplitter::new(ChunkStrategy::FixedSize {
            tokens: 10,
            overlap: 2,
        });
        let chunks = splitter.split(&text);
        // With 100 words and small token window, should produce multiple chunks
        assert!(chunks.len() > 1);
        // All chunks should have non-empty text
        for chunk in &chunks {
            assert!(!chunk.text.is_empty());
        }
    }

    #[test]
    fn test_fixed_size_single_chunk() {
        let text = "short text";
        let splitter = ChunkSplitter::new(ChunkStrategy::FixedSize {
            tokens: 200,
            overlap: 20,
        });
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "short text");
    }

    #[test]
    fn test_chunk_indices_sequential() {
        let text = "foo\n\nbar\n\nbaz";
        let splitter = ChunkSplitter::new(ChunkStrategy::Paragraph);
        let chunks = splitter.split(text);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
    }

    #[test]
    fn test_paragraph_strips_whitespace() {
        let text = "  foo  \n\n  bar  ";
        let splitter = ChunkSplitter::new(ChunkStrategy::Paragraph);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "foo");
        assert_eq!(chunks[1].text, "bar");
    }

    #[test]
    fn test_chunk_default_score() {
        let chunk = Chunk::new("test".to_string(), 0);
        assert_eq!(chunk.score, 0.0);
    }

    #[test]
    fn test_auto_strategy_paragraph() {
        let text = "paragraph 1\n\nparagraph 2";
        let splitter = ChunkSplitter::auto(text);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_auto_strategy_line() {
        let text = "line1\nline2\nline3\nline4\nline5\nline6";
        let splitter = ChunkSplitter::auto(text);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 6);
    }

    #[test]
    fn test_auto_strategy_fixed_size() {
        let text = "short";
        let splitter = ChunkSplitter::auto(text);
        let chunks = splitter.split(text);
        assert_eq!(chunks.len(), 1);
    }
}
