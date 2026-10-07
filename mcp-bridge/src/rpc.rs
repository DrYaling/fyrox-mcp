//! MCP JSON-RPC framing and request dispatch over stdin/stdout.
use crate::{bridge, catalog};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

/// 运行逐行 JSON 的 MCP stdio 循环；通知不会产生 stdout 响应。
pub fn run() {
    let mut stdout = io::BufWriter::new(io::stdout());
    for line in io::stdin().lock().lines().flatten() {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let response = dispatch(&request);
        if !response.is_null() {
            let _ = writeln!(stdout, "{response}");
            let _ = stdout.flush();
        }
    }
}

/// 分派 MCP JSON-RPC 请求；返回 `Null` 表示通知无需响应。
pub fn dispatch(request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match request.get("method").and_then(Value::as_str).unwrap_or("") {
        "initialize" => {
            json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"mcp-bridge","version":"0.2.0"},"capabilities":{"tools":{"listChanged":false}}}})
        }
        "notifications/initialized" | "notifications/cancelled" => Value::Null,
        "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":{"tools":catalog::tools()}}),
        "tools/call" => call_tool(request, id),
        _ => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}),
    }
}

/// 执行 MCP 工具调用并包装 `content` 与 `structuredContent`。
fn call_tool(request: &Value, id: Value) -> Value {
    let tool = request
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(method) = catalog::bridge_method(tool) else {
        return rpc_error(id, -32602, "Unknown tool");
    };
    let arguments = request
        .pointer("/params/arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match bridge::call(method, arguments) {
        Ok(result) => {
            json!({"jsonrpc":"2.0","id":id,"result":{"content":bridge::content(&result),"structuredContent":result}})
        }
        Err(error) => {
            let detail = error.clone();
            json!({
                "jsonrpc":"2.0",
                "id":id,
                "result":{
                    "isError":true,
                    "content":[{"type":"text","text":detail}],
                    "structuredContent":{"success":false,"error":error,"retryable":false}
                }
            })
        }
    }
}

/// 构造 JSON-RPC 协议错误；`code` 为标准错误码，`message` 为可读说明。
#[inline]
fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn initialized_notification_has_no_response() {
        assert!(super::dispatch(&json!({"method":"notifications/initialized"})).is_null());
    }

    #[test]
    fn initialize_reports_protocol_and_server() {
        let response = super::dispatch(&json!({
            "jsonrpc":"2.0",
            "id":7,
            "method":"initialize",
            "params":{}
        }));
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(response["result"]["serverInfo"]["name"], "mcp-bridge");
    }

    #[test]
    fn unknown_method_is_json_rpc_error() {
        let response = super::dispatch(&json!({
            "jsonrpc":"2.0",
            "id":8,
            "method":"unknown"
        }));
        assert_eq!(response["error"]["code"], -32601);
        assert_eq!(response["id"], 8);
    }

    #[test]
    fn unknown_tool_is_protocol_error() {
        let response = super::dispatch(&json!({
            "jsonrpc":"2.0",
            "id":9,
            "method":"tools/call",
            "params":{"name":"does_not_exist","arguments":{}}
        }));
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["id"], 9);
    }
}
