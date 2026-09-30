//! Correlated JSON response helpers.
use serde_json::{json, Value};

pub fn success(id: Value, data: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":data})
}

pub fn panic(id: Value, detail: &str) -> Value {
    success(
        id,
        json!({"error":"editor_mcp_dispatch_panicked","detail":detail}),
    )
}
