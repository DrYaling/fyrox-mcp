//! Editor 项目摘要：提供当前编辑器进程和活动场景的只读诊断信息。

use fyroxed_base::Editor;
use serde_json::{json, Value};

/// 返回项目和 Editor 运行摘要；参数为 Editor，返回 JSON 对象。
pub fn info(editor: &Editor) -> Value {
    json!({"project":"fwok","engine":"fyrox-editor","platform":"windows","bridge_protocol":"fwok-editor-bridge/2","framing":"uint64be","scene_loaded":editor.scenes.has_active_scene(),"working_directory":std::env::current_dir().ok().map(|path|path.to_string_lossy().replace('\\', "/"))})
}
