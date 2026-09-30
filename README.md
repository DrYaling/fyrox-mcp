# MCP Bridge and Fyrox Editor Integration

## Scope / 范围

This standalone workspace contains two cooperating crates: `mcp-bridge` and `fyrox-mcp`. This document covers their responsibilities, capabilities, and request flow.

本独立 workspace 包含两个协作 crate：`mcp-bridge` 和 `fyrox-mcp`。本文档说明二者的职责、功能和请求流程。

## Components / 组件

### `mcp-bridge`

`mcp-bridge` is the executable bridge process. It exposes the public MCP tool catalog, validates JSON requests, and forwards approved commands to the running editor/game bridge. It does not own Fyrox scene state.

`mcp-bridge` 是可执行桥接进程，提供 MCP 工具目录、校验 JSON 请求，并把通过校验的命令转发给运行中的编辑器/游戏桥接层。它不拥有 Fyrox 场景状态。

### `fyrox-mcp`

`fyrox-mcp` is the Fyrox EditorPlugin. It owns the editor-thread command queue, executes scene/UI/asset operations on the editor thread, reads diagnostics and screenshots, and returns structured results to the bridge.

`fyrox-mcp` 是 Fyrox EditorPlugin。它维护编辑器线程命令队列，在编辑器线程执行场景/UI/资源操作，读取诊断信息和截图，并向桥接层返回结构化结果。

## Covered Capabilities / 功能范围

- Scene tree inspection, node search, transforms, enable state, camera control, and serialized scene management.
- Serialized UI scene creation, inspection, modification, loading, rendering, and screenshot capture.
- Asset and prefab search, inspection, duplication, move/rename, and lifecycle operations.
- Structured Lua script read/create/update/delete operations under the project script root.
- Batched command execution with validation, fail-fast control, and editor-main-thread ownership.
- JSON structured errors, resource-path checks, image encoding, and screenshot diagnostics.

- 场景树检查、节点搜索、变换、启用状态、摄像机控制和场景资源管理。
- 序列化 UI 场景的创建、检查、修改、加载、渲染和截图。
- 资源和 prefab 的搜索、检查、复制、移动/重命名及生命周期操作。
- 项目脚本根目录下 Lua 脚本的结构化读取、创建、更新和删除。
- 带参数校验、失败快速终止选项和编辑器主线程所有权的批量命令执行。
- 结构化 JSON 错误、资源路径检查、图片编码和截图诊断。

## Design / 设计

The system is split into transport, validation, execution, and resource layers:

```text
MCP request
    -> mcp-bridge catalog and parameter validation
    -> Fyrox MCP command envelope
    -> fyrox-mcp main-thread queue
    -> Fyrox scene/UI/resource APIs
    -> structured result or diagnostic error
```

The bridge process is intentionally stateless with respect to editor resources. The EditorPlugin is the authority for handles, scene ownership, UI trees, resource serialization, and screenshot capture. Commands are validated before execution, and mutations target existing serialized resources or generic editor capabilities rather than business-specific object names.

系统分为传输、校验、执行和资源四层：

```text
MCP 请求
    -> mcp-bridge 工具目录和参数校验
    -> Fyrox MCP 命令封装
    -> fyrox-mcp 编辑器主线程队列
    -> Fyrox 场景/UI/资源 API
    -> 结构化结果或诊断错误
```

桥接进程不持有编辑器资源状态。EditorPlugin 是句柄、场景所有权、UI 树、资源序列化和截图的权威。命令执行前经过校验；修改针对已有序列化资源或通用编辑器能力，不绑定具体业务对象名称。

## Repository Layout / 目录结构

```text
mcp/
  mcp-bridge/   # bridge executable and MCP catalog
  fyrox-mcp/    # Fyrox EditorPlugin and editor-side operations
```

```text
mcp/
  mcp-bridge/   # 桥接可执行程序和 MCP 工具目录
  fyrox-mcp/    # Fyrox EditorPlugin 和编辑器侧操作
```

## Verification / 验证

Run from this directory / 在本目录执行：

```powershell
rtk cargo check --manifest-path Cargo.toml
rtk cargo test --manifest-path Cargo.toml
```
