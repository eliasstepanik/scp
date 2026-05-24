use serde_json::Value;

/// Classification of tool response content
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    /// Plain text content
    Text,
    /// JSON object or array (structured data)
    StructuredJson,
    /// Base64-encoded image or data URI
    Image,
    /// Raw binary blob
    Binary,
    /// Text + images interleaved (MCP mixed content arrays)
    Mixed,
}

/// Router for classifying tool response content into content types
pub struct ContentTypeRouter;

impl ContentTypeRouter {
    /// Classify a tool response content value into a ContentType.
    /// Input: the `content` field from a tools/call response (serde_json::Value).
    pub fn classify(content: &Value) -> ContentType {
        match content {
            Value::Array(arr) => Self::classify_array(arr),
            Value::String(s) => Self::classify_string(s),
            Value::Object(_) => ContentType::StructuredJson,
            Value::Null | Value::Bool(_) | Value::Number(_) => ContentType::Text,
        }
    }

    /// Classify an array value
    fn classify_array(arr: &[Value]) -> ContentType {
        if arr.is_empty() {
            return ContentType::Text;
        }

        // Check if this is an MCP content array (all elements have "type" field)
        let all_have_type = arr.iter().all(|v| {
            if let Value::Object(obj) = v {
                obj.contains_key("type")
            } else {
                false
            }
        });

        if all_have_type {
            return Self::classify_mcp_content_array(arr);
        }

        // Check if it's a mixed array (strings and objects)
        let has_strings = arr.iter().any(|v| matches!(v, Value::String(_)));
        let has_objects = arr.iter().any(|v| matches!(v, Value::Object(_)));

        if has_strings && has_objects {
            return ContentType::Mixed;
        }

        // All objects or all other types → StructuredJson
        if arr.iter().all(|v| matches!(v, Value::Object(_))) {
            return ContentType::StructuredJson;
        }

        // Default for arrays
        ContentType::StructuredJson
    }

    /// Classify an MCP content array (all elements have "type" field)
    fn classify_mcp_content_array(arr: &[Value]) -> ContentType {
        let mut has_text = false;
        let mut has_image = false;
        let mut has_resource = false;

        for item in arr {
            if let Value::Object(obj) = item {
                if let Some(Value::String(type_str)) = obj.get("type") {
                    match type_str.as_str() {
                        "text" => has_text = true,
                        "image" => has_image = true,
                        "resource" => has_resource = true,
                        _ => {}
                    }
                }
            }
        }

        // If any resource type, treat as StructuredJson
        if has_resource {
            return ContentType::StructuredJson;
        }

        // If both text and image, it's mixed
        if has_text && has_image {
            return ContentType::Mixed;
        }

        // If only text
        if has_text {
            return ContentType::Text;
        }

        // If only image
        if has_image {
            return ContentType::Image;
        }

        // Default
        ContentType::Text
    }

    /// Classify a string value
    fn classify_string(s: &str) -> ContentType {
        // Check for data URI format
        if s.starts_with("data:image/") {
            return ContentType::Image;
        }

        // Check if it's base64 and long enough to be binary
        if Self::is_base64(s) && s.len() > 1024 {
            return ContentType::Image;
        }

        ContentType::Text
    }

    /// Check if a string is pure base64 (only [A-Za-z0-9+/=] chars)
    fn is_base64(s: &str) -> bool {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_string() {
        let content = Value::String("Hello, World!".to_string());
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Text);
    }

    #[test]
    fn test_json_object() {
        let content = serde_json::json!({"key": "value"});
        assert_eq!(
            ContentTypeRouter::classify(&content),
            ContentType::StructuredJson
        );
    }

    #[test]
    fn test_json_array() {
        let content = serde_json::json!([1, 2, 3]);
        assert_eq!(
            ContentTypeRouter::classify(&content),
            ContentType::StructuredJson
        );
    }

    #[test]
    fn test_data_uri_image() {
        let content = Value::String("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".to_string());
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Image);
    }

    #[test]
    fn test_mcp_content_array_text() {
        let content = serde_json::json!([
            {"type": "text", "text": "hello"}
        ]);
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Text);
    }

    #[test]
    fn test_mcp_content_array_image() {
        let content = serde_json::json!([
            {"type": "image", "data": "base64encodeddata"}
        ]);
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Image);
    }

    #[test]
    fn test_mcp_content_array_mixed() {
        let content = serde_json::json!([
            {"type": "text", "text": "a"},
            {"type": "image", "data": "base64encodeddata"}
        ]);
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Mixed);
    }

    #[test]
    fn test_null_value() {
        let content = Value::Null;
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Text);
    }

    #[test]
    fn test_number_value() {
        let content = Value::Number(42.into());
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Text);
    }

    #[test]
    fn test_base64_long_string() {
        let base64_str = "A".repeat(2000); // Pure base64 chars, > 1024 bytes
        let content = Value::String(base64_str.to_string());
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Image);
    }

    #[test]
    fn test_base64_short_string() {
        let base64_str = "AAAA"; // Pure base64 chars, but < 1024 bytes
        let content = Value::String(base64_str.to_string());
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Text);
    }

    #[test]
    fn test_mixed_array_strings_and_objects() {
        let content = serde_json::json!([
            "text string",
            {"key": "value"}
        ]);
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Mixed);
    }

    #[test]
    fn test_mcp_content_array_with_resource() {
        let content = serde_json::json!([
            {"type": "text", "text": "hello"},
            {"type": "resource", "uri": "file:///path"}
        ]);
        assert_eq!(
            ContentTypeRouter::classify(&content),
            ContentType::StructuredJson
        );
    }

    #[test]
    fn test_empty_array() {
        let content = serde_json::json!([]);
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Text);
    }

    #[test]
    fn test_bool_value() {
        let content = Value::Bool(true);
        assert_eq!(ContentTypeRouter::classify(&content), ContentType::Text);
    }
}
