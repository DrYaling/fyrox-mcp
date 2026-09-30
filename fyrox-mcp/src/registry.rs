//! Editor command registry, analogous to Unity MCP's `CommandRegistry`.
use crate::{
    assets, capture, command::Command, compatibility, logs, manage, project, response, scene, ui,
};
use fyroxed_base::Editor;
use serde_json::{json, Value};

pub fn execute(request: &Value, editor: &mut Editor) -> Value {
    let command = Command::parse(request);
    let data = match command.method {
        "ping" => {
            json!({"ok":true,"engine":"fyrox-editor","platform":"windows","protocol":"fwok-editor-bridge/2","framing":"uint64be"})
        }
        "project_info" => project::info(editor),
        "scene_tree" => scene::tree(editor),
        "scene_stats" => scene::stats(editor),
        "scene_selection" => scene::selection(editor),
        "scene_cameras" => scene::cameras(editor),
        "scene_find" => scene::find(editor, &command.params),
        "scene_set_transform" => scene::set_transform(editor, &command.params),
        "scene_set_enabled" => scene::set_enabled(editor, &command.params),
        "logs" => logs::read_recent(&command.params),
        "resources_list" => assets::list(&command.params),
        "texture_read" => assets::read_texture(&command.params),
        "screenshot" => capture::capture(editor, &command.params),
        "manage_scene" => manage::manage_scene(editor, &command.params),
        "manage_nodes" => manage::create_nodes(editor, &command.params),
        "find_gameobjects" => compatibility::find_gameobjects(editor, &command.params),
        "manage_gameobject" => compatibility::manage_gameobject(editor, &command.params),
        "manage_camera" => compatibility::manage_camera(editor, &command.params),
        "manage_prefab" => compatibility::manage_prefab(editor, &command.params),
        "manage_ui" => ui::manage(editor, &command.params),
        "manage_asset" => compatibility::manage_asset(&command.params),
        "manage_script" => compatibility::manage_script(&command.params),
        "manage_editor" => compatibility::manage_editor(editor, &command.params),
        "read_console" => compatibility::read_console(&command.params),
        "batch_execute" => batch_execute(editor, &command.params),
        _ => json!({"error":format!("unknown_editor_method:{}", command.method)}),
    };
    response::success(command.id, data)
}

/// Execute a bounded sequence on the same editor main thread. Parallel requests
/// are deliberately serialized because Fyrox's scene graph is not thread-safe.
fn batch_execute(editor: &mut Editor, params: &Value) -> Value {
    let Some(commands) = params.get("commands").and_then(Value::as_array) else {
        return json!({"error":"commands_array_required"});
    };
    if commands.is_empty() || commands.len() > 100 {
        return json!({"error":"commands_count_out_of_range","maximum":100});
    }
    let fail_fast = params
        .get("failFast")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut results = Vec::with_capacity(commands.len());
    let mut failures = 0usize;
    for entry in commands {
        let Some(object) = entry.as_object() else {
            failures += 1;
            results.push(json!({"callSucceeded":false,"error":"command_must_be_object"}));
            if fail_fast {
                break;
            }
            continue;
        };
        let tool = object.get("tool").and_then(Value::as_str).unwrap_or("");
        if tool.is_empty() || tool == "batch_execute" {
            failures += 1;
            results.push(
                json!({"tool":tool,"callSucceeded":false,"error":"empty_or_nested_batch_tool"}),
            );
            if fail_fast {
                break;
            }
            continue;
        }
        let method = public_tool_method(tool);
        let request = json!({"id": results.len(), "method": method, "params": object.get("params").cloned().unwrap_or_else(|| json!({}))});
        let response = execute(&request, editor);
        let result = response.get("result").cloned().unwrap_or(response);
        let success = result.get("error").is_none()
            && result
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(true)
            && result.get("ok").and_then(Value::as_bool).unwrap_or(true);
        if !success {
            failures += 1;
        }
        results.push(json!({"tool":tool,"callSucceeded":success,"result":result}));
        if fail_fast && !success {
            break;
        }
    }
    json!({"success":failures == 0,"results":results,"callSuccessCount":results.len()-failures,"callFailureCount":failures,"parallelApplied":false})
}

/// Batch entries use public MCP names, while the editor registry uses concise
/// bridge method names. Keep this conversion in the editor process so every
/// sub-call still crosses the same main-thread dispatch boundary.
fn public_tool_method(tool: &str) -> &str {
    match tool {
        "fwok_ping" => "ping",
        "fwok_scene_tree" => "scene_tree",
        "fwok_scene_stats" => "scene_stats",
        "fwok_scene_selection" => "scene_selection",
        "fwok_cameras" => "scene_cameras",
        "fwok_project_info" => "project_info",
        "fwok_find_node" => "scene_find",
        "fwok_set_transform" => "scene_set_transform",
        "fwok_set_enabled" => "scene_set_enabled",
        "fwok_logs" => "logs",
        "fwok_screenshot" => "screenshot",
        "fwok_list_resources" => "resources_list",
        "fwok_read_texture" => "texture_read",
        "fwok_manage_scene" => "manage_scene",
        "fwok_manage_nodes" => "manage_nodes",
        "find_gameobjects" => "find_gameobjects",
        "manage_gameobject" => "manage_gameobject",
        "manage_camera" => "manage_camera",
        "manage_prefab" => "manage_prefab",
        "manage_ui" => "manage_ui",
        "manage_asset" => "manage_asset",
        "manage_script" => "manage_script",
        "manage_editor" => "manage_editor",
        "read_console" => "read_console",
        other => other,
    }
}
