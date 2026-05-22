use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt;

/// JSON-RPC 2.0 Request ID — can be string, number, or null
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i64),
    Null,
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestId::String(s) => write!(f, "{}", s),
            RequestId::Number(n) => write!(f, "{}", n),
            RequestId::Null => write!(f, "null"),
        }
    }
}

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<RequestId>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: RequestId, method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method,
            params,
        }
    }
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    // SCP error codes
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    pub const BACKEND_ERROR: i32 = -32000;
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<RequestId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: RequestId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 2.0 Notification (no id field)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
        }
    }
}

/// Incoming message — auto-detects Request, Response, or Notification
#[derive(Debug, Clone, Serialize)]
pub enum IncomingMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

impl<'de> Deserialize<'de> for IncomingMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let obj = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("Expected object"))?;

        // Check if it has an id field
        let has_id = obj.contains_key("id");
        let has_method = obj.contains_key("method");
        let has_result = obj.contains_key("result");
        let has_error = obj.contains_key("error");

        // Determine the message type based on fields
        if has_method && has_id {
            // Request: has both method and id
            serde_json::from_value::<JsonRpcRequest>(value)
                .map(IncomingMessage::Request)
                .map_err(serde::de::Error::custom)
        } else if has_id && (has_result || has_error) {
            // Response: has id and (result or error)
            serde_json::from_value::<JsonRpcResponse>(value)
                .map(IncomingMessage::Response)
                .map_err(serde::de::Error::custom)
        } else if has_method && !has_id {
            // Notification: has method but no id
            serde_json::from_value::<JsonRpcNotification>(value)
                .map(IncomingMessage::Notification)
                .map_err(serde::de::Error::custom)
        } else {
            Err(serde::de::Error::custom(
                "Invalid JSON-RPC message: must be Request, Response, or Notification",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id_string() {
        let id = RequestId::String("test".to_string());
        assert_eq!(id.to_string(), "test");
    }

    #[test]
    fn test_request_id_number() {
        let id = RequestId::Number(42);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn test_request_id_null() {
        let id = RequestId::Null;
        assert_eq!(id.to_string(), "null");
    }

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new(
            RequestId::Number(1),
            "test_method".to_string(),
            Some(serde_json::json!({"key": "value"})),
        );
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"test_method\""));
    }

    #[test]
    fn test_response_success() {
        let resp =
            JsonRpcResponse::success(RequestId::Number(1), serde_json::json!({"result": "ok"}));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_response_error() {
        let err = JsonRpcError::new(JsonRpcError::INTERNAL_ERROR, "Something went wrong");
        let resp = JsonRpcResponse::error(RequestId::Number(1), err);
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_notification_serialization() {
        let notif = JsonRpcNotification::new(
            "test_notification".to_string(),
            Some(serde_json::json!({"data": "test"})),
        );
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(!json.contains("\"id\""));
        assert!(json.contains("\"method\":\"test_notification\""));
    }

    #[test]
    fn test_incoming_message_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"key":"value"}}"#;
        let msg: IncomingMessage = serde_json::from_str(json).unwrap();
        match msg {
            IncomingMessage::Request(req) => {
                assert_eq!(req.method, "test");
                assert!(req.id.is_some());
            }
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_incoming_message_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let msg: IncomingMessage = serde_json::from_str(json).unwrap();
        match msg {
            IncomingMessage::Response(resp) => {
                assert!(resp.result.is_some());
                assert!(resp.error.is_none());
            }
            _ => panic!("Expected Response"),
        }
    }

    #[test]
    fn test_incoming_message_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"notification","params":{"data":"test"}}"#;
        let msg: IncomingMessage = serde_json::from_str(json).unwrap();
        match msg {
            IncomingMessage::Notification(notif) => {
                assert_eq!(notif.method, "notification");
            }
            _ => panic!("Expected Notification"),
        }
    }
}
