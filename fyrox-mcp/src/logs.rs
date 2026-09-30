//! Editor 日志读取：仅读取项目 fyrox.log 的尾部行，供 MCP 调试诊断使用。

use serde_json::{json, Value};

/// 读取最近日志；参数为可选 lines，返回日志行数组或读取错误。
pub fn read_recent(params: &Value) -> Value {
    let lines = params
        .get("lines")
        .and_then(Value::as_u64)
        .unwrap_or(100)
        .clamp(1, 5000) as usize;
    match std::fs::read_to_string("fyrox.log") {
        Ok(text) => {
            json!({"lines":text.lines().rev().take(lines).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>() })
        }
        Err(error) => json!({"error":format!("log_read_failed:{error}")}),
    }
}
