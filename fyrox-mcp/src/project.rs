//! Editor 项目摘要：提供当前编辑器进程和活动场景的只读诊断信息。

use fyroxed_base::Editor;
use fyroxed_base::Mode;
use fyroxed_base::{scene::GameScene, ui_scene::UiScene};
use serde_json::{json, Value};

/// 返回项目和 Editor 运行摘要；参数为 Editor，返回 JSON 对象。
pub fn info(editor: &Editor) -> Value {
    let (active_path, active_kind) = if editor.scenes.has_active_scene() {
        let entry = editor.scenes.current_scene_entry_ref();
        let kind = if entry.controller.downcast_ref::<UiScene>().is_some() {
            "ui"
        } else if entry.controller.downcast_ref::<GameScene>().is_some() {
            "rgs"
        } else {
            "unknown"
        };
        (
            entry
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().replace('\\', "/")),
            Some(kind),
        )
    } else {
        (None, None)
    };
    let mode = match editor.mode {
        Mode::Edit => "edit",
        Mode::Build { .. } => "build",
        Mode::Play { .. } => "play",
    };
    let loaded_scenes = editor.scenes.iter().map(|entry| {
        let kind = if entry.controller.downcast_ref::<UiScene>().is_some() { "ui" }
            else if entry.controller.downcast_ref::<GameScene>().is_some() { "rgs" }
            else { "unknown" };
        let scene_handle = entry.controller.downcast_ref::<GameScene>()
            .map(|scene| format!("{:?}", scene.scene));
        json!({"id":entry.id.to_string(),"path":entry.path.as_ref().map(|path| path.to_string_lossy().replace('\\', "/")),"kind":kind,"scene_handle":scene_handle})
    }).collect::<Vec<_>>();
    let active_scene_handle = editor
        .scenes
        .current_scene_entry_ref()
        .controller
        .downcast_ref::<GameScene>()
        .map(|scene| format!("{:?}", scene.scene));
    let ui_scene_id = editor
        .scenes
        .current_scene_entry_ref()
        .controller
        .downcast_ref::<UiScene>()
        .map(|_| editor.scenes.current_scene_entry_ref().id.to_string());
    json!({"project":"fwok","engine":"fyrox-editor","platform":"windows","bridge_protocol":"fwok-editor-bridge/2","framing":"uint64be","scene_loaded":editor.scenes.has_active_scene(),"active_path":active_path,"active_kind":active_kind,"scene_handle":active_scene_handle,"ui_scene_id":ui_scene_id,"loaded_scenes":loaded_scenes,"editor_mode":mode,"queue_hint":"commands are processed in bounded batches per frame","working_directory":std::env::current_dir().ok().map(|path|path.to_string_lossy().replace('\\', "/"))})
}
