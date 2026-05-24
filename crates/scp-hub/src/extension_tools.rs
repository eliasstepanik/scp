use serde_json::{json, Value};

/// Returns the list of SCP extension tool definitions in MCP tools/list format.
///
/// These are built-in tools provided by the SCP hub:
/// - `scp_get_more`: Retrieve filtered content that was omitted from a previous response
/// - `scp_info`: Get information about the SCP hub
/// - `scp_budget`: Get the current token budget status
/// - `scp_budget_reset`: Reset the session token budget
#[allow(dead_code)]
pub fn scp_extension_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "scp_get_more",
            "description": "Retrieve the next batch of filtered content that was omitted from a previous tool response due to context budget limits. Use the request_id from the progressive disclosure hint.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request_id": {
                        "type": "string",
                        "description": "The request ID from the [SCP: ...] hint in the previous response"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Chunk offset to start from (default: 0)",
                        "default": 0
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of chunks to return (default: 15)",
                        "default": 15
                    }
                },
                "required": ["request_id"]
            }
        }),
        json!({
            "name": "scp_info",
            "description": "Get information about the SCP hub: version, enabled extensions, number of connected servers, and total available tools.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "scp_budget",
            "description": "Get the current token budget status for this session: remaining tokens, total budget, strategy, and profile.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "scp_budget_reset",
            "description": "Reset the session token budget to its configured maximum. Use when the budget is exhausted but more work is needed.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
    ]
}
