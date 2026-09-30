//! Validated command envelope received from the external MCP server.
use serde_json::Value;

/// Immutable editor command. Network input is normalized before it reaches
/// the main-thread registry, matching Unity MCP's `Command` boundary.
pub struct Command<'a> {
    pub id: Value,
    pub method: &'a str,
    pub params: Value,
}

impl<'a> Command<'a> {
    pub fn parse(value: &'a Value) -> Self {
        Self {
            id: value.get("id").cloned().unwrap_or(Value::Null),
            method: value.get("method").and_then(Value::as_str).unwrap_or(""),
            params: value
                .get("params")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        }
    }
}
