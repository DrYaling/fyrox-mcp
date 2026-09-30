#![recursion_limit = "256"]
//! MCP bridge executable entry point. This file only wires modules and runs the request loop.
mod bridge;
mod catalog;
mod rpc;

/// 启动 MCP 标准输入输出循环；无参数且不返回业务结果。
fn main() {
    rpc::run();
}
