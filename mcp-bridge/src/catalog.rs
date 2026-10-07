//! MCP tool catalog and bridge method mapping.
use serde_json::{json, Value};

/// 返回全部 MCP 工具定义；无参数，返回符合 MCP `tools/list` 的 JSON 数组。
pub fn tools() -> Value {
    json!([
        {"name":"fwok_ping","description":"Check the local Fyrox EditorPlugin bridge and protocol version.","inputSchema":{"type":"object","additionalProperties":false}},
        {"name":"fwok_scene_tree","description":"Read a loaded Fyrox 3D scene node tree. scene_path may target a loaded .rgs while a UI scene is active.","inputSchema":{"type":"object","properties":{"scene_path":{"type":"string"}},"additionalProperties":false}},
        {"name":"fwok_scene_stats","description":"Read loaded-scene node counts; scene_path may target a loaded .rgs while a UI scene is active.","inputSchema":{"type":"object","properties":{"scene_path":{"type":"string"}},"additionalProperties":false}},
        {"name":"fwok_scene_selection","description":"Read the currently selected Fyrox scene nodes in the Editor.","inputSchema":{"type":"object","additionalProperties":false}},
        {"name":"fwok_cameras","description":"List cameras in a loaded .rgs scene.","inputSchema":{"type":"object","properties":{"scene_path":{"type":"string"}},"additionalProperties":false}},
        {"name":"fwok_project_info","description":"Read Windows runtime, project, scene and bridge metadata.","inputSchema":{"type":"object","additionalProperties":false}},
        {"name":"fwok_wait_for_resource","description":"Wait until a serialized .ui or .rgs resource is the active Editor resource, using bounded polling instead of client-side sleeps.","inputSchema":{"type":"object","required":["path"],"properties":{"path":{"type":"string"},"kind":{"type":"string","enum":["ui","rgs"]},"timeout_ms":{"type":"integer","minimum":500,"maximum":120000}},"additionalProperties":false}},
        {"name":"fwok_preview_resource","description":"Open a serialized data/*.ui or data/*.rgs resource, wait for Fyrox Editor selection, capture a GPU preview, and return UI layout or 3D scene validation.","inputSchema":{"type":"object","required":["path"],"properties":{"path":{"type":"string"},"width":{"type":"integer","minimum":64,"maximum":4096},"height":{"type":"integer","minimum":64,"maximum":4096},"file_name":{"type":"string"},"camera":{"type":"string"},"include_image":{"type":"boolean"},"expected_names":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}},
        {"name":"fwok_find_node","description":"Find an existing scene node by exact resource name.","inputSchema":{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}},
        {"name":"fwok_set_transform","description":"Set an existing scene node position, Euler rotation, and/or scale without creating resources.","inputSchema":{"type":"object","required":["name"],"anyOf":[{"required":["position"]},{"required":["rotation"]},{"required":["scale"]}],"properties":{"name":{"type":"string"},"position":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"rotation":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"scale":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3}}}},
        {"name":"fwok_set_enabled","description":"Enable or disable an existing scene node.","inputSchema":{"type":"object","required":["name","enabled"],"properties":{"name":{"type":"string"},"enabled":{"type":"boolean"}}}},
        {"name":"fwok_logs","description":"Read recent Fyrox log lines.","inputSchema":{"type":"object","properties":{"lines":{"type":"integer","minimum":1,"maximum":5000}}}},
        {"name":"fwok_screenshot","description":"Render a loaded Fyrox camera and save PNG metadata; set include_image=true for an MCP image block.","inputSchema":{"type":"object","properties":{"scene_path":{"type":"string"},"camera":{"type":"string","description":"Optional active camera name to validate before capture."},"file_name":{"type":"string"},"width":{"type":"integer","minimum":64,"maximum":4096},"height":{"type":"integer","minimum":64,"maximum":4096},"include_image":{"type":"boolean"}},"additionalProperties":false}},
        {"name":"fwok_list_resources","description":"List files below the project data directory.","inputSchema":{"type":"object","properties":{"root":{"type":"string"}}}},
        {"name":"fwok_read_texture","description":"Transfer a PNG, JPEG, BMP or TGA under data as MCP image content.","inputSchema":{"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}}
        ,{"name":"fwok_manage_scene","description":"Create, save, or load a serialized Fyrox scene. Set wait=true for a stable response after the Editor selects the resource.","inputSchema":{"type":"object","required":["action"],"properties":{"action":{"type":"string","enum":["create","save","load"]},"path":{"type":"string"},"wait":{"type":"boolean","description":"Wait until active_path matches path after an asynchronous load."},"timeout_ms":{"type":"integer","minimum":500,"maximum":120000}}}}
        ,{"name":"fwok_manage_nodes","description":"Batch-create primitive, camera, light, or pivot nodes through the Fyrox Editor command stack.","inputSchema":{"type":"object","required":["nodes"],"properties":{"nodes":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"object","required":["name","kind"],"properties":{"name":{"type":"string"},"kind":{"type":"string","enum":["cube","sphere","cylinder","camera","point_light","pivot"]},"position":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"rotation":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"scale":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"color":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":4,"maxItems":4},"vertical_size":{"type":"number"},"radius":{"type":"number"}}}}}}}
        ,{"name":"find_gameobjects","description":"Find Fyrox nodes by name, substring, or type.","inputSchema":{"type":"object","required":["searchTerm"],"properties":{"searchTerm":{"type":"string"},"searchMethod":{"type":"string","enum":["by_name","contains","by_type"]},"includeInactive":{"type":"boolean"}}}}
        ,{"name":"manage_gameobject","description":"Create, modify, delete, duplicate, reparent, or inspect Fyrox nodes.","inputSchema":{"type":"object","required":["action"],"properties":{"action":{"type":"string","enum":["create","modify","delete","duplicate","reparent","get_info"]},"target":{"type":"string"},"name":{"type":"string"},"kind":{"type":"string"},"parent":{"type":"string"},"position":{"type":"array"},"rotation":{"type":"array"},"scale":{"type":"array"},"enabled":{"type":"boolean"}}}}
        ,{"name":"manage_camera","description":"Create, aim, focus, temporarily configure, and capture Fyrox cameras. Focus accepts world XYZ or orbit yaw/pitch/distance. Capture projection, viewport, and enabled changes are restored after the screenshot.","inputSchema":{"type":"object","required":["action"],"properties":{"action":{"type":"string","enum":["ping","list_cameras","create_camera","set_target","focus_target","configure_for_capture","screenshot","screenshot_multiview"]},"scene_path":{"type":"string"},"camera":{"type":"string"},"name":{"type":"string"},"target_node":{"type":"string"},"file_name":{"type":"string"},"position":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"rotation":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"x":{"type":"number"},"y":{"type":"number"},"z":{"type":"number"},"target_position":{"type":"array","items":{"type":"number"}},"distance":{"type":"number","exclusiveMinimum":0},"yaw":{"type":"number"},"pitch":{"type":"number"},"fov_degrees":{"type":"number","exclusiveMinimum":0},"z_near":{"type":"number","exclusiveMinimum":0},"z_far":{"type":"number","exclusiveMinimum":0},"viewport":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4},"enabled":{"type":"boolean"},"radius":{"type":"number"},"views":{"type":"array","items":{"type":"string"}}}}}
        ,{"name":"manage_prefab","description":"Copy, rename, modify, move, delete, instantiate, or remove Fyrox .rgs prefab model resources. Rename/move preserves the registry UUID; duplicate creates a separate file and must receive a new UUID.","inputSchema":{"type":"object","required":["action"],"properties":{"action":{"type":"string","enum":["get_info","duplicate","rename","move","modify","delete","add_to_scene","remove_from_scene"]},"path":{"type":"string"},"content":{"type":"string"},"source":{"type":"string"},"destination":{"type":"string"},"target":{"type":"string"},"instance_name":{"type":"string"},"parent":{"type":"string"},"position":{"type":"array","items":{"type":"number"}}}}}
        ,{"name":"manage_ui","description":"Load, create, inspect, batch-style, save, and render serialized Fyrox UI prefabs. Set wait=true for a stable response after the Editor selects the resource. Mutations persist to .ui resources; runtime UI creation remains disabled.","inputSchema":{"type":"object","required":["action"],"properties":{"action":{"type":"string","enum":["ping","create_scene","load","load_scene","create","get_visual_tree","read","modify_visual_element","update","save","render_ui","screenshot","load_and_capture"]},"path":{"type":"string"},"wait":{"type":"boolean"},"timeout_ms":{"type":"integer","minimum":500,"maximum":120000},"file_name":{"type":"string"},"name":{"type":"string"},"kind":{"type":"string","enum":["text","button","panel","canvas"]},"text":{"type":"string"},"parent":{"type":"string"},"position":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"width":{"type":"number"},"height":{"type":"number"},"enabled":{"type":"boolean"},"visible":{"type":"boolean"},"font_size":{"type":"number"},"background":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":4,"maxItems":4},"foreground":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":4,"maxItems":4},"updates":{"type":"array","maxItems":256,"items":{"type":"object"}},"size":{"type":"array","items":{"type":"number"}}}}}
        ,{"name":"manage_asset","description":"Search, inspect, create folders, delete, duplicate, move, or rename data assets.","inputSchema":{"type":"object","required":["action"],"properties":{"action":{"type":"string","enum":["search","get_info","create_folder","delete","duplicate","move","rename"]},"path":{"type":"string"},"source":{"type":"string"},"destination":{"type":"string"}}}}
        ,{"name":"manage_script","description":"Read, create, update, apply structured text edits, or delete UTF-8 scripts under data/scripts.","inputSchema":{"type":"object","required":["action","path"],"properties":{"action":{"type":"string","enum":["read","create","update","apply_text_edits","delete"]},"path":{"type":"string"},"content":{"type":"string"},"edits":{"type":"array","items":{"type":"object"}}}}}
        ,{"name":"manage_editor","description":"Run Fyrox editor undo, redo, play/build, and stop/edit actions.","inputSchema":{"type":"object","required":["action"],"properties":{"action":{"type":"string","enum":["undo","redo","play","stop"]}}}}
        ,{"name":"read_console","description":"Read recent Fyrox editor log records.","inputSchema":{"type":"object","properties":{"lines":{"type":"integer","minimum":1,"maximum":5000}}}}
        ,{"name":"batch_execute","description":"Execute up to 100 editor commands on Fyrox's main thread. Commands are always serialized for scene safety; parallel=true is accepted as an explicit request and reported as parallelApplied=false.","inputSchema":{"type":"object","required":["commands"],"properties":{"commands":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","required":["tool"],"properties":{"tool":{"type":"string"},"params":{"type":"object"}}}},"failFast":{"type":"boolean"},"parallel":{"type":"boolean"}}}}
    ])
}

/// 将 MCP 工具名映射为游戏桥接方法；未知名称返回 `None`。
#[inline]
pub fn bridge_method(tool: &str) -> Option<&'static str> {
    match tool {
        "fwok_ping" => Some("ping"),
        "fwok_scene_tree" => Some("scene_tree"),
        "fwok_scene_stats" => Some("scene_stats"),
        "fwok_scene_selection" => Some("scene_selection"),
        "fwok_cameras" => Some("scene_cameras"),
        "fwok_project_info" => Some("project_info"),
        "fwok_wait_for_resource" => Some("wait_for_resource"),
        "fwok_preview_resource" => Some("preview_resource"),
        "fwok_find_node" => Some("scene_find"),
        "fwok_set_transform" => Some("scene_set_transform"),
        "fwok_set_enabled" => Some("scene_set_enabled"),
        "fwok_logs" => Some("logs"),
        "fwok_screenshot" => Some("screenshot"),
        "fwok_list_resources" => Some("resources_list"),
        "fwok_read_texture" => Some("texture_read"),
        "fwok_manage_scene" => Some("manage_scene"),
        "fwok_manage_nodes" => Some("manage_nodes"),
        "find_gameobjects" => Some("find_gameobjects"),
        "manage_gameobject" => Some("manage_gameobject"),
        "manage_camera" => Some("manage_camera"),
        "manage_prefab" => Some("manage_prefab"),
        "manage_ui" => Some("manage_ui"),
        "manage_asset" => Some("manage_asset"),
        "manage_script" => Some("manage_script"),
        "manage_editor" => Some("manage_editor"),
        "read_console" => Some("read_console"),
        "batch_execute" => Some("batch_execute"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn public_tools_have_bridge_methods() {
        for tool in super::tools().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            assert!(
                super::bridge_method(name).is_some(),
                "missing route for {name}"
            );
        }
    }
}
