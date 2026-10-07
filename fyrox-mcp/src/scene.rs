//! Editor 场景查询与变更：只访问 Editor 当前活动的已加载游戏场景。

use fyrox::{
    core::{
        algebra::{UnitQuaternion, Vector3},
        pool::Handle,
    },
    graph::SceneGraph,
    scene::{node::Node, Scene},
};
use fyroxed_base::{scene::GameScene, world::selection::GraphSelection, Editor};
use serde_json::{json, Value};

/// 取得当前活动的游戏场景句柄；参数为 Editor，返回不存在或 UI 场景时的 None。
pub(crate) fn active_handle(editor: &Editor) -> Option<Handle<Scene>> {
    game_handle(editor, None)
}

/// Resolve a loaded game scene by serialized path. This keeps UI and 3D scene
/// inspection independent when both are open in the editor.
pub(crate) fn game_handle(editor: &Editor, requested: Option<&str>) -> Option<Handle<Scene>> {
    let matches = |path: &Option<std::path::PathBuf>| {
        requested.map_or(true, |wanted| {
            let wanted = wanted.replace('\\', "/");
            path.as_ref()
                .map(|value| {
                    let value = value.to_string_lossy().replace('\\', "/");
                    value == wanted || value.ends_with(&format!("/{wanted}"))
                })
                .unwrap_or(false)
        })
    };
    editor
        .scenes
        .iter()
        .find(|entry| matches(&entry.path))
        .and_then(|entry| {
            entry
                .controller
                .downcast_ref::<GameScene>()
                .map(|scene| scene.scene)
        })
}

/// 导出当前活动游戏场景节点树；参数为 Editor，返回 JSON 场景树或错误对象。
pub fn tree(editor: &Editor, params: &Value) -> Value {
    let Some(handle) = game_handle(editor, params.get("scene_path").and_then(Value::as_str)) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let scene = &editor.engine.scenes[handle];
    json!({"scene_handle":format!("{:?}", handle),"node_count":scene.graph.pair_iter().count(),"root":export_node(&scene.graph, scene.graph.get_root())})
}

/// Return compact counts and depth information for the active serialized scene.
pub fn stats(editor: &Editor, params: &Value) -> Value {
    let Some(handle) = game_handle(editor, params.get("scene_path").and_then(Value::as_str)) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let graph = &editor.engine.scenes[handle].graph;
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut enabled = 0usize;
    let mut max_depth = 0usize;
    for (_, node) in graph.pair_iter() {
        *counts.entry(node_kind(node)).or_default() += 1;
        enabled += usize::from(node.is_globally_enabled());
        let mut depth = 0usize;
        let mut parent = node.parent();
        while parent.is_some() {
            depth += 1;
            parent = graph[parent].parent();
        }
        max_depth = max_depth.max(depth);
    }
    let node_count = graph.pair_iter().count();
    json!({"success":true,"scene_handle":format!("{:?}",handle),"node_count":node_count,"enabled_count":enabled,"disabled_count":node_count-enabled,"max_depth":max_depth,"types":counts})
}

/// Read the current Editor graph selection without changing it.
pub fn selection(editor: &Editor) -> Value {
    let entry = editor.scenes.current_scene_entry_ref();
    let Some(graph_selection) = entry.selection.as_ref::<GraphSelection>() else {
        return json!({"success":true,"kind":"none","count":0,"items":[]});
    };
    let Some(handle) = active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let graph = &editor.engine.scenes[handle].graph;
    let items = graph_selection.nodes().iter().filter_map(|handle| {
        let node = graph.try_get_node(*handle).ok()?;
        Some(json!({"name":node.name(),"type":node_kind(node),"enabled":node.is_globally_enabled(),"handle":{"index":handle.index(),"generation":handle.generation()}}))
    }).collect::<Vec<_>>();
    json!({"success":true,"kind":"graph","count":items.len(),"items":items})
}

/// 列出活动场景摄像机；参数为 Editor，返回摄像机数组或错误对象。
pub fn cameras(editor: &Editor, params: &Value) -> Value {
    let Some(handle) = game_handle(editor, params.get("scene_path").and_then(Value::as_str)) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let scene = &editor.engine.scenes[handle];
    let cameras = scene.graph.pair_iter().filter_map(|(handle, node)| {
        if !node.is_camera() { return None; }
        let camera = node.as_camera();
        Some(json!({"name":node.name(),"handle":{"index":handle.index(),"generation":handle.generation()},"enabled":node.is_globally_enabled() && camera.is_enabled(),"position":vec3(node.global_position()),"viewport":{"x":camera.viewport().x(),"y":camera.viewport().y(),"width":camera.viewport().w(),"height":camera.viewport().h()},"projection":format!("{:?}",camera.projection_value())}))
    }).collect::<Vec<_>>();
    json!({"cameras":cameras})
}

/// 按精确名称查找节点；参数为 Editor、name，返回节点元数据或 found=false。
pub fn find(editor: &Editor, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(handle) = active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let scene = &editor.engine.scenes[handle];
    let Some((handle, node)) = scene
        .graph
        .pair_iter()
        .find(|(_, node)| node.name() == name)
    else {
        return json!({"found":false,"name":name});
    };
    json!({
        "found": true,
        "name": node.name(),
        "handle": {"index":handle.index(),"generation":handle.generation()},
        "enabled": node.is_globally_enabled(),
        "local_position": vec3(**node.local_transform().position()),
        "global_position": vec3(node.global_position())
    })
}

/// 修改已有节点位置；参数为 Editor 和含 name/position 的 JSON，返回执行结果。
pub fn set_transform(editor: &mut Editor, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let position = optional_vec3(params, "position");
    let rotation = optional_vec3(params, "rotation");
    let scale = optional_vec3(params, "scale");
    if position.is_err() || rotation.is_err() || scale.is_err() {
        return json!({"error":"transform_fields_require_three_numbers"});
    }
    let (position, rotation, scale) = (position.unwrap(), rotation.unwrap(), scale.unwrap());
    if position.is_none() && rotation.is_none() && scale.is_none() {
        return json!({"error":"position_rotation_or_scale_required"});
    };
    let Some(handle) = active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let scene = &mut editor.engine.scenes[handle];
    let Some((_, node)) = scene
        .graph
        .pair_iter_mut()
        .find(|(_, node)| node.name() == name)
    else {
        return json!({"error":"node_not_found","name":name});
    };
    let transform = node.local_transform_mut();
    if let Some([x, y, z]) = position {
        transform.set_position(Vector3::new(x, y, z));
    }
    if let Some([x, y, z]) = rotation {
        transform.set_rotation(UnitQuaternion::from_euler_angles(
            x.to_radians(),
            y.to_radians(),
            z.to_radians(),
        ));
    }
    if let Some([x, y, z]) = scale {
        transform.set_scale(Vector3::new(x, y, z));
    }
    json!({"ok":true,"name":name,"position":position,"rotation":rotation,"scale":scale})
}

/// Parse an optional three-number JSON vector. Missing fields are valid; malformed fields fail.
fn optional_vec3(params: &Value, field: &str) -> Result<Option<[f32; 3]>, ()> {
    let Some(items) = params.get(field) else {
        return Ok(None);
    };
    let items = items.as_array().ok_or(())?;
    if items.len() != 3 {
        return Err(());
    }
    Ok(Some([
        items[0].as_f64().ok_or(())? as f32,
        items[1].as_f64().ok_or(())? as f32,
        items[2].as_f64().ok_or(())? as f32,
    ]))
}

/// 修改已有节点启用状态；参数为 Editor 和含 name/enabled 的 JSON，返回执行结果。
pub fn set_enabled(editor: &mut Editor, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let enabled = params
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let Some(handle) = active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let scene = &mut editor.engine.scenes[handle];
    let Some((_, node)) = scene
        .graph
        .pair_iter_mut()
        .find(|(_, node)| node.name() == name)
    else {
        return json!({"error":"node_not_found","name":name});
    };
    node.set_enabled(enabled);
    json!({"ok":true,"name":name,"enabled":enabled})
}

/// 递归导出节点公共信息；参数为图和节点句柄，返回可 JSON 序列化的节点对象。
fn export_node(graph: &fyrox::scene::graph::Graph, handle: Handle<Node>) -> Value {
    let Ok(node) = graph.try_get_node(handle) else {
        return Value::Null;
    };
    json!({"name":node.name(),"handle":{"index":handle.index(),"generation":handle.generation()},"type":node_kind(node),"enabled":node.is_globally_enabled(),"is_camera":node.is_camera(),"local_position":vec3(**node.local_transform().position()),"global_position":vec3(node.global_position()),"children":node.children().iter().map(|child|export_node(graph,*child)).collect::<Vec<_>>()})
}

/// 转换 Fyrox 三维向量；参数为向量，返回三元素 JSON 数组。
#[inline]
fn vec3(value: Vector3<f32>) -> Value {
    json!([value.x, value.y, value.z])
}

/// 返回稳定的节点类别标签；参数为节点，返回类别文本。
#[inline]
fn node_kind(node: &Node) -> &'static str {
    if node.is_camera() {
        "Camera"
    } else if node.is_mesh() {
        "Mesh"
    } else if node.is_sprite() {
        "Sprite"
    } else if node.is_pivot() {
        "Pivot"
    } else {
        "Node"
    }
}
