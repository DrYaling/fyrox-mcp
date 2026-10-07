//! MCP EditorPlugin：仅在编辑器进程中维护回环 TCP 请求队列并在编辑器线程执行。

mod assets;
mod capture;
mod command;
mod compatibility;
mod dispatcher;
mod framing;
mod launcher;
mod logs;
mod manage;
mod project;
mod registry;
mod response;
mod scene;
mod transport;
mod ui;

use dispatcher::PendingRequest;
use fyrox::engine::ApplicationLoopController;
use fyroxed_base::{plugin::EditorPlugin, Editor};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_REQUESTS_PER_UPDATE: usize = 16;
const MAX_UPDATE_BUDGET: Duration = Duration::from_millis(8);

/// 编辑器 MCP 插件；队列归插件所有，生命周期严格限于 Editor 进程。
#[derive(Default)]
pub struct McpEditorPlugin {
    /// 网络线程写入、EditorPlugin 的 on_update 消费的请求队列。
    queue: Option<Arc<Mutex<VecDeque<PendingRequest>>>>,
    /// Auto-started stdio MCP tool. It is owned by the plugin so Editor
    /// binaries do not need process discovery or shutdown code.
    mcp_process: Option<std::process::Child>,
}

impl EditorPlugin for McpEditorPlugin {
    /// 编辑器完全初始化后启动回环桥接；参数为 Editor，返回无。
    fn on_start(&mut self, editor: &mut Editor) {
        // The MCP transport is external to Winit, so an incoming TCP request does not create
        // a window event. Keep the editor update loop active while this editor-only plugin is
        // loaded; otherwise an unfocused Windows editor can listen on 6501 yet never drain the
        // main-thread queue. This setting belongs to FyroxEd and is absent from the game crate.
        editor.settings.general.keep_editor_active = true;
        // Probe/start the external stdio tool before binding the Editor transport. The
        // transport owns the same port, so probing after `transport::start` would always
        // see our own listener and incorrectly skip auto-start.
        self.mcp_process = launcher::start();
        self.queue = Some(transport::start());
    }

    /// 每帧在 Editor 主线程处理一小批请求；参数为 Editor 和循环控制器，返回无。
    fn on_update(&mut self, editor: &mut Editor, _loop_controller: ApplicationLoopController) {
        let Some(queue) = &self.queue else { return };
        let started = Instant::now();
        for _ in 0..MAX_REQUESTS_PER_UPDATE {
            let Some(pending) = queue.lock().ok().and_then(|mut queue| queue.pop_front()) else {
                break;
            };
            transport::set_queue_depth(queue);
            if pending.canceled.load(std::sync::atomic::Ordering::Acquire) {
                continue;
            }
            let request = pending.request;
            let reply = pending.reply;
            // 单个外部 MCP 命令不得终止整个编辑器。捕获边界仅保护插件调度层；
            // 具体模块仍负责把可预期错误转换为 JSON。
            let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                registry::execute(&request, editor)
            }))
            .unwrap_or_else(|panic| {
                let detail = panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("unknown panic payload");
                response::panic(
                    request
                        .get("id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    detail,
                )
            });
            let _ = reply.send(response);
            if started.elapsed() >= MAX_UPDATE_BUDGET {
                break;
            }
        }
    }

    /// MCP 连接需要在编辑器窗口未产生输入事件时继续消费主线程命令。
    /// 返回 `true` 使 Fyrox 保持稳定更新循环；插件不修改预览状态，也不回滚场景。
    fn is_in_preview_mode(&self, _editor: &Editor) -> bool {
        true
    }

    /// 本插件没有临时预览数据，因此离开预览模式时无需处理。
    fn on_leave_preview_mode(&mut self, _editor: &mut Editor) {}

    /// 编辑器退出前释放队列所有权；参数为 Editor，返回无。
    fn on_exit(&mut self, _editor: &mut Editor) {
        self.queue = None;
        if let Some(mut process) = self.mcp_process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }
    }
}
