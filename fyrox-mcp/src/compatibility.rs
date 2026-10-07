//! Concrete Unity-MCP-compatible editor operations backed by Fyrox APIs.
use crate::{assets, capture, logs, manage, scene};
use fyrox::{
    core::{
        algebra::{UnitQuaternion, Vector3},
        math::{Matrix4Ext, Rect},
        pool::Handle,
        SafeLock,
    },
    graph::SceneGraph,
    resource::model::{Model, ModelResourceExtension},
    scene::node::Node,
};
use fyroxed_base::{
    scene::commands::graph::{DeleteSubGraphCommand, LinkNodesCommand},
    Editor, Message,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Find serialized scene nodes by exact name, substring, or type.
pub fn find_gameobjects(editor: &Editor, params: &Value) -> Value {
    let query = params
        .get("searchTerm")
        .or_else(|| params.get("target"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let method = params
        .get("searchMethod")
        .and_then(Value::as_str)
        .unwrap_or("by_name");
    let inactive = params
        .get("includeInactive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let Some(sh) = scene::active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let graph = &editor.engine.scenes[sh].graph;
    let items = graph.pair_iter().filter(|(_, n)| (inactive || n.is_globally_enabled()) && match method { "by_name"|"name" => n.name() == query, "contains" => n.name().to_lowercase().contains(&query.to_lowercase()), "by_type"|"type" => node_type(n).eq_ignore_ascii_case(query), _ => false }).map(|(h,n)| json!({"name":n.name(),"type":node_type(n),"enabled":n.is_globally_enabled(),"handle":{"index":h.index(),"generation":h.generation()},"position":[n.global_position().x,n.global_position().y,n.global_position().z]})).collect::<Vec<_>>();
    json!({"success":true,"searchMethod":method,"searchTerm":query,"totalCount":items.len(),"items":items})
}

/// CRUD subset of Unity manage_gameobject using Fyrox's undoable command stack.
pub fn manage_gameobject(editor: &mut Editor, params: &Value) -> Value {
    let action = params.get("action").and_then(Value::as_str).unwrap_or("");
    let name = params
        .get("target")
        .or_else(|| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    match action {
        "create" => {
            let mut node = params.clone();
            if node.get("kind").is_none() {
                if let Some(primitive_type) = node.get("primitive_type").cloned() {
                    node["kind"] = primitive_type;
                }
            }
            if node.get("name").is_none() && !name.is_empty() {
                node["name"] = json!(name);
            }
            manage::create_nodes(editor, &json!({"nodes":[node]}))
        }
        "modify" => {
            let mut p = params.clone();
            p["name"] = json!(name);
            let t = ["position", "rotation", "scale"]
                .iter()
                .any(|k| params.get(*k).is_some())
                .then(|| scene::set_transform(editor, &p));
            let e = params
                .get("enabled")
                .map(|v| scene::set_enabled(editor, &json!({"name":name,"enabled":v})));
            if t.is_none() && e.is_none() {
                json!({"error":"modify_requires_transform_or_enabled"})
            } else {
                let transform_ok = t
                    .as_ref()
                    .map(|value| value.get("error").is_none())
                    .unwrap_or(true);
                let enabled_ok = e
                    .as_ref()
                    .map(|value| value.get("error").is_none())
                    .unwrap_or(true);
                json!({"success":transform_ok && enabled_ok,"target":name,"transform":t,"enabled":e})
            }
        }
        "get" | "get_info" => scene::find(editor, &json!({"name":name})),
        "delete" => with_node(editor, &name, |ed, h| {
            ed.message_sender.do_command(DeleteSubGraphCommand::new(h));
            json!({"success":true,"action":"delete","target":name,"queued":true})
        }),
        "duplicate" => with_node(editor, &name, |ed, h| {
            let sh = scene::active_handle(ed).unwrap();
            let graph = &ed.engine.scenes[sh].graph;
            let parent = graph[h].parent();
            let mut n = graph.copy_single_node(h);
            let default_name = format!("{name}_Copy");
            let new_name = params
                .get("new_name")
                .and_then(Value::as_str)
                .unwrap_or(&default_name);
            n.set_name(new_name);
            // See `manage::create_nodes`: normal graph linking retains the clone's
            // serialized local transform, unlike FyroxEd's keep-global command.
            let graph = &mut ed.engine.scenes[sh].graph;
            let new_handle = graph.add_node(n);
            graph.link_nodes(new_handle, parent);
            json!({"success":true,"action":"duplicate","target":name,"new_name":new_name,"queued":false,"undoable":false})
        }),
        "reparent" => {
            let parent = params.get("parent").and_then(Value::as_str).unwrap_or("");
            let Some(sh) = scene::active_handle(editor) else {
                return json!({"error":"active_game_scene_not_found"});
            };
            let g = &editor.engine.scenes[sh].graph;
            let (Some(ch), Some(ph)) = (find_handle(g, &name), find_handle(g, parent)) else {
                return json!({"error":"node_or_parent_not_found"});
            };
            editor
                .message_sender
                .do_command(LinkNodesCommand::new(ch, ph));
            json!({"success":true,"action":"reparent","target":name,"parent":parent,"queued":true})
        }
        _ => {
            json!({"error":"invalid_gameobject_action","valid":["create","modify","delete","duplicate","reparent","get_info"]})
        }
    }
}

/// Fyrox camera operations corresponding to Unity's basic camera actions.
pub fn manage_camera(editor: &mut Editor, params: &Value) -> Value {
    match params.get("action").and_then(Value::as_str).unwrap_or("") {
        "ping" => json!({"success":true,"backend":"fyrox_camera","cinemachine":false}),
        "list_cameras" => scene::cameras(editor, params),
        "create_camera" => manage::create_nodes(
            editor,
            &json!({"nodes":[{"name":params.get("name").and_then(Value::as_str).unwrap_or("Camera"),"kind":"camera","position":params.get("position").cloned().unwrap_or(json!([0,0,0])),"rotation":params.get("rotation").cloned().unwrap_or(json!([0,0,0]))}]}),
        ),
        "screenshot" => capture::capture(editor, params),
        "set_target" => set_camera_target(editor, params),
        "focus_target" => focus_camera_target(editor, params),
        "configure_for_capture" => configure_for_capture(editor, params),
        "screenshot_multiview" => screenshot_multiview(editor, params),
        _ => {
            json!({"error":"invalid_camera_action","valid":["ping","list_cameras","create_camera","set_target","focus_target","configure_for_capture","screenshot","screenshot_multiview"]})
        }
    }
}

/// File and instance operations for Fyrox `.rgs` model resources.
/// Rename/copy preserve the serialized bytes, so the resource UUID in the file is unchanged.
pub fn manage_prefab(editor: &mut Editor, params: &Value) -> Value {
    match params.get("action").and_then(Value::as_str).unwrap_or("") {
        "get_info" => {
            let Some(path) = prefab_path(params.get("path").and_then(Value::as_str)) else {
                return json!({"error":"invalid_prefab_path"});
            };
            match fs::metadata(&path) {
                Ok(m) => {
                    json!({"success":true,"path":norm(&path),"bytes":m.len(),"is_file":m.is_file()})
                }
                Err(e) => json!({"error":e.to_string()}),
            }
        }
        "duplicate" => prefab_copy(editor, params, false),
        "rename" | "move" => move_prefab(editor, params),
        "modify" => modify_prefab_file(editor, params),
        "delete" => {
            let Some(path) = prefab_path(
                params
                    .get("path")
                    .or_else(|| params.get("target"))
                    .and_then(Value::as_str),
            ) else {
                return json!({"error":"invalid_prefab_path"});
            };
            delete_prefab(editor, &path)
        }
        "add_to_scene" => add_prefab_to_scene(editor, params),
        "remove_from_scene" => remove_prefab_from_scene(editor, params),
        _ => {
            json!({"error":"invalid_prefab_action","valid":["get_info","duplicate","rename","move","modify","delete","add_to_scene","remove_from_scene"]})
        }
    }
}

fn modify_prefab_file(editor: &mut Editor, params: &Value) -> Value {
    let Some(path) = prefab_path(params.get("path").and_then(Value::as_str)) else {
        return json!({"error":"invalid_prefab_path"});
    };
    let Some(content) = params.get("content").and_then(Value::as_str) else {
        return json!({"error":"content_required"});
    };
    if !content.starts_with("FTAX:") {
        return json!({"error":"prefab_serialization_header_required"});
    }
    let metadata = PathBuf::from(format!("{}.meta", path.to_string_lossy()));
    if !metadata.exists() {
        return json!({"error":"prefab_metadata_missing","detail":"Refusing raw replacement because resource identity cannot be verified."});
    }
    let tmp = path.with_extension("rgs.mcp-tmp");
    if let Err(e) = fs::write(&tmp, content.as_bytes()) {
        return json!({"error":e.to_string()});
    }
    match fs::rename(&tmp, &path) {
        Ok(()) => {
            let reloaded = {
                let mut state = editor.engine.resource_manager.state();
                state.try_reload_resource_from_path(&path)
            };
            json!({"success":true,"action":"modify","path":norm(&path),"bytes":content.len(),"resource_id_preserved":true,"resource_reloaded":reloaded})
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            json!({"error":e.to_string()})
        }
    }
}

fn delete_prefab(editor: &mut Editor, path: &Path) -> Value {
    let state = editor.engine.resource_manager.state();
    if state
        .resource_registry
        .safe_lock()
        .path_to_uuid(path)
        .is_none()
    {
        return json!({"error":"prefab_not_registered","path":norm(path)});
    }
    if let Err(error) = state.resource_io.delete_file_sync(path) {
        return json!({"error":format!("prefab_delete_failed:{error}")});
    }
    let cleanup = match state
        .resource_registry
        .safe_lock()
        .modify()
        .remove_metadata(path)
    {
        Ok(()) => {
            json!({"success":true,"action":"delete","deleted":norm(path),"registry_updated":true})
        }
        Err(error) => {
            json!({"error":format!("prefab_registry_cleanup_failed:{error}"),"deleted":true})
        }
    };
    cleanup
}

fn move_prefab(editor: &mut Editor, params: &Value) -> Value {
    let Some(source) = prefab_path(
        params
            .get("source")
            .or_else(|| params.get("path"))
            .and_then(Value::as_str),
    ) else {
        return json!({"error":"invalid_prefab_source"});
    };
    let Some(destination) = prefab_path(
        params
            .get("destination")
            .or_else(|| params.get("new_path"))
            .and_then(Value::as_str),
    ) else {
        return json!({"error":"invalid_prefab_destination"});
    };
    match fyrox::core::futures::executor::block_on(
        editor
            .engine
            .resource_manager
            .move_resource_by_path(&source, &destination, false),
    ) {
        Ok(()) => {
            json!({"success":true,"source":norm(&source),"destination":norm(&destination),"resource_id_preserved":true})
        }
        Err(e) => json!({"error":format!("prefab_move_failed:{e}")}),
    }
}

fn prefab_copy(editor: &mut Editor, params: &Value, moving: bool) -> Value {
    let Some(source) = prefab_path(
        params
            .get("source")
            .or_else(|| params.get("path"))
            .and_then(Value::as_str),
    ) else {
        return json!({"error":"invalid_prefab_source"});
    };
    let Some(destination) = prefab_path(
        params
            .get("destination")
            .or_else(|| params.get("new_path"))
            .and_then(Value::as_str),
    ) else {
        return json!({"error":"invalid_prefab_destination"});
    };
    if let Some(parent) = destination.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return json!({"error":e.to_string()});
        }
    }
    let result = if moving {
        fs::rename(&source, &destination).map(|_| ())
    } else {
        fs::copy(&source, &destination).map(|_| ())
    };
    result.map(|_| {
        if !moving {
            editor.engine.resource_manager.update_or_load_registry();
        }
        json!({"success":true,"source":norm(&source),"destination":norm(&destination),"resource_bytes_preserved":true,"resource_id_preserved":false,"detail":"A duplicated resource must receive a new metadata UUID; registry refresh will assign it on next scan."})
    }).unwrap_or_else(|e| json!({"error":e.to_string()}))
}

fn add_prefab_to_scene(editor: &mut Editor, params: &Value) -> Value {
    let Some(path) = prefab_path(params.get("path").and_then(Value::as_str)) else {
        return json!({"error":"invalid_prefab_path"});
    };
    let Some(sh) = scene::active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let Some(model) = editor
        .engine
        .resource_manager
        .try_request::<Model>(&path)
        .and_then(|r| fyrox::core::futures::executor::block_on(r).ok())
    else {
        return json!({"error":"prefab_load_failed","path":norm(&path)});
    };
    let scene = &mut editor.engine.scenes[sh];
    let root = model.instantiate(scene);
    let parent_name = params.get("parent").and_then(Value::as_str).unwrap_or("");
    let parent = if parent_name.is_empty() {
        scene.graph.get_root()
    } else {
        scene
            .graph
            .pair_iter()
            .find(|(_, n)| n.name() == parent_name)
            .map(|(h, _)| h)
            .unwrap_or(scene.graph.get_root())
    };
    scene.graph.link_nodes(root, parent);
    if let Some(position) = json_vec3(params.get("position")) {
        scene.graph[root]
            .local_transform_mut()
            .set_position(position);
    }
    scene.graph.update_hierarchical_data();
    json!({"success":true,"action":"add_to_scene","path":norm(&path),"instance_name":scene.graph[root].name(),"handle":{"index":root.index(),"generation":root.generation()},"resource_instance_root":true})
}

fn remove_prefab_from_scene(editor: &mut Editor, params: &Value) -> Value {
    let name = params
        .get("instance_name")
        .or_else(|| params.get("target"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(sh) = scene::active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let graph = &editor.engine.scenes[sh].graph;
    let Some(handle) = graph
        .pair_iter()
        .find(|(_, n)| n.name() == name && n.is_resource_instance_root())
        .map(|(h, _)| h)
    else {
        return json!({"error":"prefab_instance_not_found","target":name});
    };
    editor
        .message_sender
        .do_command(DeleteSubGraphCommand::new(handle));
    json!({"success":true,"action":"remove_from_scene","instance_name":name,"queued":true})
}

fn prefab_path(raw: Option<&str>) -> Option<PathBuf> {
    let path = PathBuf::from(raw?.trim_start_matches(['/', '\\']));
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        || !path.starts_with("data")
        || path.extension().and_then(|e| e.to_str()) != Some("rgs")
    {
        None
    } else {
        Some(path)
    }
}

/// Position and aim a camera around an existing node. Angles are degrees.
fn focus_camera_target(editor: &mut Editor, params: &Value) -> Value {
    let camera_name = params.get("camera").and_then(Value::as_str).unwrap_or("");
    let target_name = params
        .get("target_node")
        .or_else(|| params.get("target"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let distance = params
        .get("distance")
        .and_then(Value::as_f64)
        .unwrap_or(8.0) as f32;
    let yaw = params.get("yaw").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let pitch = params.get("pitch").and_then(Value::as_f64).unwrap_or(15.0) as f32;
    if !distance.is_finite() || distance <= 0.0 {
        return json!({"error":"distance_must_be_positive"});
    }
    let Some(sh) = scene::active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let graph = &mut editor.engine.scenes[sh].graph;
    let Some(target) = graph
        .pair_iter()
        .find(|(_, n)| n.name() == target_name)
        .map(|(h, _)| h)
    else {
        return json!({"error":"target_node_not_found","target":target_name});
    };
    let Some(camera) = graph
        .pair_iter()
        .find(|(_, n)| n.is_camera() && (camera_name.is_empty() || n.name() == camera_name))
        .map(|(h, _)| h)
    else {
        return json!({"error":"camera_not_found","camera":camera_name});
    };
    graph.update_hierarchical_data();
    let center = graph[target].global_position();
    let yaw = yaw.to_radians();
    let pitch = pitch.to_radians().clamp(-1.55, 1.55);
    let offset = Vector3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    ) * distance;
    let position = json_vec3(params.get("position"))
        .or_else(|| {
            let xyz = [params.get("x"), params.get("y"), params.get("z")];
            xyz.iter().all(Option::is_some).then(|| {
                Vector3::new(
                    xyz[0].and_then(Value::as_f64).unwrap_or_default() as f32,
                    xyz[1].and_then(Value::as_f64).unwrap_or_default() as f32,
                    xyz[2].and_then(Value::as_f64).unwrap_or_default() as f32,
                )
            })
        })
        .unwrap_or(center + offset);
    let rotation = json_vec3(params.get("rotation"))
        .map(|euler| {
            UnitQuaternion::from_euler_angles(
                euler.x.to_radians(),
                euler.y.to_radians(),
                euler.z.to_radians(),
            )
        })
        .unwrap_or_else(|| UnitQuaternion::face_towards(&(position - center), &Vector3::y()));
    graph.set_global_position(camera, position);
    graph.set_global_rotation(camera, rotation);
    graph.update_hierarchical_data();
    graph.update_hierarchical_data();
    let actual = graph[camera].global_position();
    json!({"success":true,"camera":graph[camera].name(),"target_node":graph[target].name(),"position":[actual.x,actual.y,actual.z],"distance":distance,"yaw":params.get("yaw").and_then(Value::as_f64).unwrap_or(0.0),"pitch":params.get("pitch").and_then(Value::as_f64).unwrap_or(15.0)})
}

/// Temporarily apply camera projection/viewport overrides, capture, then restore all camera state.
fn configure_for_capture(editor: &mut Editor, params: &Value) -> Value {
    let camera_name = params.get("camera").and_then(Value::as_str).unwrap_or("");
    let Some(sh) = scene::active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let graph = &mut editor.engine.scenes[sh].graph;
    let Some(handle) = graph
        .pair_iter()
        .find(|(_, n)| n.is_camera() && (camera_name.is_empty() || n.name() == camera_name))
        .map(|(h, _)| h)
    else {
        return json!({"error":"camera_not_found","camera":camera_name});
    };
    let old_projection = graph[handle].as_camera().projection_value();
    let old_viewport = graph[handle].as_camera().viewport();
    let old_enabled = graph[handle].as_camera().is_enabled();
    if let Some(fov) = params.get("fov_degrees").and_then(Value::as_f64) {
        if let fyrox::scene::camera::Projection::Perspective(mut p) = old_projection.clone() {
            p.fov = (fov as f32).to_radians();
            graph[handle]
                .as_camera_mut()
                .set_projection(fyrox::scene::camera::Projection::Perspective(p));
        }
    }
    if let Some(near) = params.get("z_near").and_then(Value::as_f64) {
        graph[handle]
            .as_camera_mut()
            .projection_mut()
            .set_z_near(near as f32);
    }
    if let Some(far) = params.get("z_far").and_then(Value::as_f64) {
        graph[handle]
            .as_camera_mut()
            .projection_mut()
            .set_z_far(far as f32);
    }
    if let Some(enabled) = params.get("enabled").and_then(Value::as_bool) {
        graph[handle].as_camera_mut().set_enabled(enabled);
    }
    if let Some(viewport) = params.get("viewport").and_then(Value::as_array) {
        if viewport.len() != 4 || viewport.iter().any(|value| value.as_f64().is_none()) {
            graph[handle].as_camera_mut().set_projection(old_projection);
            graph[handle].as_camera_mut().set_viewport(old_viewport);
            graph[handle].as_camera_mut().set_enabled(old_enabled);
            return json!({"error":"viewport_must_be_four_numbers"});
        }
        graph[handle].as_camera_mut().set_viewport(Rect::new(
            viewport[0].as_f64().unwrap() as f32,
            viewport[1].as_f64().unwrap() as f32,
            viewport[2].as_f64().unwrap() as f32,
            viewport[3].as_f64().unwrap() as f32,
        ));
    }
    let mut shot_params = params.clone();
    shot_params["camera"] = json!(graph[handle].name());
    shot_params["file_name"] = params
        .get("file_name")
        .cloned()
        .unwrap_or(json!("camera-configured.png"));
    let result = capture::capture(editor, &shot_params);
    let graph = &mut editor.engine.scenes[sh].graph;
    graph[handle].as_camera_mut().set_projection(old_projection);
    graph[handle].as_camera_mut().set_viewport(old_viewport);
    graph[handle].as_camera_mut().set_enabled(old_enabled);
    json!({"success":result.get("error").is_none(),"restored":true,"camera":graph[handle].name(),"image":result})
}

/// Aim a serialized scene camera at a world-space point. Fyrox cameras look
/// down their local -Z axis, hence `face_towards` receives the reverse view
/// vector. The resulting transform is immediately read back in the response.
fn set_camera_target(editor: &mut Editor, params: &Value) -> Value {
    let camera_name = params.get("camera").and_then(Value::as_str).unwrap_or("");
    let Some(target) = json_vec3(
        params
            .get("target_position")
            .or_else(|| params.get("targetPosition")),
    ) else {
        return json!({"error":"target_position_vec3_required"});
    };
    let Some(scene_handle) = scene::active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let graph = &mut editor.engine.scenes[scene_handle].graph;
    let Some(handle) = graph
        .pair_iter()
        .find(|(_, node)| {
            node.is_camera() && (camera_name.is_empty() || node.name() == camera_name)
        })
        .map(|(handle, _)| handle)
    else {
        return json!({"error":"camera_not_found","camera":camera_name});
    };
    graph.update_hierarchical_data();
    let position = graph[handle].global_position();
    let direction = target - position;
    if direction.norm_squared() <= f32::EPSILON {
        return json!({"error":"camera_and_target_positions_are_equal"});
    }
    let rotation = UnitQuaternion::face_towards(&-direction, &Vector3::y());
    graph.set_global_rotation(handle, rotation);
    graph.update_hierarchical_data();
    let rotation = UnitQuaternion::from_matrix_eps(
        &graph[handle].global_transform_without_scaling().basis(),
        10.0 * f32::EPSILON,
        16,
        UnitQuaternion::identity(),
    );
    let (roll, pitch, yaw) = rotation.euler_angles();
    json!({
        "success":true,
        "camera":graph[handle].name(),
        "position":[position.x,position.y,position.z],
        "target_position":[target.x,target.y,target.z],
        "rotation_radians":[roll,pitch,yaw]
    })
}

/// Capture genuinely different camera poses and restore the original local
/// transform afterwards. Each entry contains its own PNG payload so the MCP
/// client can expose every view as image content and independently hash it.
fn screenshot_multiview(editor: &mut Editor, params: &Value) -> Value {
    let camera_name = params.get("camera").and_then(Value::as_str).unwrap_or("");
    let target = json_vec3(
        params
            .get("target_position")
            .or_else(|| params.get("targetPosition")),
    )
    .unwrap_or_else(Vector3::zeros);
    let radius = params.get("radius").and_then(Value::as_f64).unwrap_or(8.0) as f32;
    if !radius.is_finite() || radius <= 0.0 {
        return json!({"error":"radius_must_be_positive"});
    }
    let Some(scene_handle) = scene::active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let Some(handle) = editor.engine.scenes[scene_handle]
        .graph
        .pair_iter()
        .find(|(_, node)| {
            node.is_camera() && (camera_name.is_empty() || node.name() == camera_name)
        })
        .map(|(handle, _)| handle)
    else {
        return json!({"error":"camera_not_found","camera":camera_name});
    };
    let resolved_name = editor.engine.scenes[scene_handle].graph[handle]
        .name()
        .to_owned();
    editor.engine.scenes[scene_handle]
        .graph
        .update_hierarchical_data();
    let old_position = editor.engine.scenes[scene_handle].graph[handle].global_position();
    let old_rotation = UnitQuaternion::from_matrix_eps(
        &editor.engine.scenes[scene_handle].graph[handle]
            .global_transform_without_scaling()
            .basis(),
        10.0 * f32::EPSILON,
        16,
        UnitQuaternion::identity(),
    );
    let requested = params
        .get("views")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["front", "back", "left", "right", "top"]);
    let mut captures = Vec::new();
    for view in requested {
        let offset = match view {
            "front" => Vector3::new(0.0, 0.0, radius),
            "back" => Vector3::new(0.0, 0.0, -radius),
            "left" => Vector3::new(-radius, 0.0, 0.0),
            "right" => Vector3::new(radius, 0.0, 0.0),
            "top" => Vector3::new(0.001, radius, 0.001),
            _ => {
                captures.push(json!({"view":view,"error":"unsupported_view"}));
                continue;
            }
        };
        let position = target + offset;
        let rotation = UnitQuaternion::face_towards(&(position - target), &Vector3::y());
        editor.engine.scenes[scene_handle]
            .graph
            .set_global_position(handle, position);
        editor.engine.scenes[scene_handle]
            .graph
            .set_global_rotation(handle, rotation);
        // `render_scene` consumes cached global transforms.  MCP edits happen
        // outside the normal editor update tick, so refresh the graph before
        // each readback; otherwise every requested view renders the old pose.
        editor.engine.scenes[scene_handle]
            .graph
            .update_hierarchical_data();
        let mut shot_params = params.clone();
        shot_params["camera"] = json!(resolved_name);
        shot_params["file_name"] = json!(format!("camera-{view}.png"));
        let shot = capture::capture(editor, &shot_params);
        captures.push(json!({"view":view,"position":[position.x,position.y,position.z],"target_position":[target.x,target.y,target.z],"image":shot}));
    }
    editor.engine.scenes[scene_handle]
        .graph
        .set_global_position(handle, old_position);
    editor.engine.scenes[scene_handle]
        .graph
        .set_global_rotation(handle, old_rotation);
    editor.engine.scenes[scene_handle]
        .graph
        .update_hierarchical_data();
    json!({"success":captures.iter().all(|item| item.get("error").is_none() && item.pointer("/image/error").is_none()),"camera":resolved_name,"restored":true,"captures":captures})
}

fn json_vec3(value: Option<&Value>) -> Option<Vector3<f32>> {
    let a = value?.as_array()?;
    if a.len() != 3 {
        return None;
    }
    Some(Vector3::new(
        a[0].as_f64()? as f32,
        a[1].as_f64()? as f32,
        a[2].as_f64()? as f32,
    ))
}

/// Real project-data asset operations; paths are sandboxed below `data`.
pub fn manage_asset(params: &Value) -> Value {
    match params.get("action").and_then(Value::as_str).unwrap_or("") {
        "search" => assets::list(&json!({"root":params.get("root").cloned().unwrap_or(json!(""))})),
        "get_info" => file_info(params),
        "create_folder" => mutate(params, |p| {
            fs::create_dir_all(p).map(|_| json!({"success":true,"path":norm(p)}))
        }),
        "delete" => mutate(params, |p| {
            if p.is_dir() {
                fs::remove_dir_all(p)
            } else {
                fs::remove_file(p)
            }
            .map(|_| json!({"success":true,"deleted":norm(p)}))
        }),
        "duplicate" => copy_move(params, false),
        "move" | "rename" => copy_move(params, true),
        _ => {
            json!({"error":"invalid_asset_action","valid":["search","get_info","create_folder","delete","duplicate","move","rename"]})
        }
    }
}

/// UTF-8 script CRUD under the canonical data/scripts resource root.
pub fn manage_script(params: &Value) -> Value {
    let action = params.get("action").and_then(Value::as_str).unwrap_or("");
    let Some(path) = script_path(params) else {
        return json!({"error":"invalid_script_path"});
    };
    match action {
        "read" => fs::read_to_string(&path)
            .map(|c| json!({"success":true,"path":norm(&path),"content":c}))
            .unwrap_or_else(|e| json!({"error":e.to_string()})),
        "create" | "update" => {
            let Some(c) = params.get("content").and_then(Value::as_str) else {
                return json!({"error":"content_required"});
            };
            if let Some(p) = path.parent() {
                let _ = fs::create_dir_all(p);
            };
            fs::write(&path, c)
                .map(|_| json!({"success":true,"action":action,"path":norm(&path),"bytes":c.len()}))
                .unwrap_or_else(|e| json!({"error":e.to_string()}))
        }
        "apply_text_edits" => apply_script_edits(&path, params),
        "delete" => fs::remove_file(&path)
            .map(|_| json!({"success":true,"deleted":norm(&path)}))
            .unwrap_or_else(|e| json!({"error":e.to_string()})),
        _ => {
            json!({"error":"invalid_script_action","valid":["read","create","update","apply_text_edits","delete"]})
        }
    }
}

/// Editor lifecycle actions that have direct FyroxEd message equivalents.
pub fn manage_editor(editor: &mut Editor, params: &Value) -> Value {
    match params.get("action").and_then(Value::as_str).unwrap_or("") {
        "undo" => {
            editor.message_sender.send(Message::UndoCurrentSceneCommand);
            json!({"success":true,"queued":true})
        }
        "redo" => {
            editor.message_sender.send(Message::RedoCurrentSceneCommand);
            json!({"success":true,"queued":true})
        }
        "play" => {
            editor.message_sender.send(Message::SwitchToBuildMode {
                play_after_build: true,
            });
            json!({"success":true,"queued":true})
        }
        "stop" => {
            editor.message_sender.send(Message::SwitchToEditMode);
            json!({"success":true,"queued":true})
        }
        _ => json!({"error":"invalid_editor_action","valid":["undo","redo","play","stop"]}),
    }
}
pub fn read_console(params: &Value) -> Value {
    logs::read_recent(params)
}
fn node_type(n: &Node) -> &'static str {
    if n.is_camera() {
        "Camera"
    } else if n.is_mesh() {
        "Mesh"
    } else if n.is_sprite() {
        "Sprite"
    } else if n.is_pivot() {
        "Pivot"
    } else {
        "Node"
    }
}
fn find_handle(g: &fyrox::scene::graph::Graph, name: &str) -> Option<Handle<Node>> {
    g.pair_iter()
        .find(|(_, n)| n.name() == name)
        .map(|(h, _)| h)
}
fn with_node<F: FnOnce(&mut Editor, Handle<Node>) -> Value>(
    e: &mut Editor,
    name: &str,
    f: F,
) -> Value {
    let Some(sh) = scene::active_handle(e) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let Some(h) = find_handle(&e.engine.scenes[sh].graph, name) else {
        return json!({"error":"node_not_found","name":name});
    };
    f(e, h)
}
fn safe(raw: &str) -> Option<PathBuf> {
    let r = Path::new(raw.trim_start_matches(['\\', '/']));
    if r.as_os_str().is_empty()
        || r.is_absolute()
        || r.components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        None
    } else if r.starts_with("data") {
        Some(r.to_path_buf())
    } else {
        Some(PathBuf::from("data").join(r))
    }
}
fn mutate<F: FnOnce(&Path) -> std::io::Result<Value>>(p: &Value, f: F) -> Value {
    let Some(path) = p
        .get("path")
        .or_else(|| p.get("target"))
        .and_then(Value::as_str)
        .and_then(safe)
    else {
        return json!({"error":"invalid_asset_path"});
    };
    f(&path).unwrap_or_else(|e| json!({"error":e.to_string()}))
}
fn file_info(p: &Value) -> Value {
    mutate(p, |path| {
        fs::metadata(path).map(|m|json!({"success":true,"path":norm(path),"is_file":m.is_file(),"is_dir":m.is_dir(),"bytes":m.len()}))
    })
}
fn copy_move(p: &Value, moving: bool) -> Value {
    let (Some(s), Some(d)) = (
        p.get("source")
            .or_else(|| p.get("path"))
            .and_then(Value::as_str)
            .and_then(safe),
        p.get("destination")
            .or_else(|| p.get("new_path"))
            .and_then(Value::as_str)
            .and_then(safe),
    ) else {
        return json!({"error":"invalid_asset_path"});
    };
    if let Some(parent) = d.parent() {
        let _ = fs::create_dir_all(parent);
    };
    let r = if moving {
        fs::rename(&s, &d).map(|_| ())
    } else if s.is_dir() {
        copy_dir_recursive(&s, &d)
    } else {
        fs::copy(&s, &d).map(|_| ())
    };
    r.map(|_| json!({"success":true,"source":norm(&s),"destination":norm(&d)}))
        .unwrap_or_else(|e| json!({"error":e.to_string()}))
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(from, to)?;
        }
    }
    Ok(())
}

/// Apply Unity-MCP-compatible 1-based line/column edits to a UTF-8 script.
/// Edits are validated against the original text and applied from the end.
fn apply_script_edits(path: &Path, params: &Value) -> Value {
    let Some(edits) = params.get("edits").and_then(Value::as_array) else {
        return json!({"error":"edits_array_required"});
    };
    let Ok(mut content) = fs::read_to_string(path) else {
        return json!({"error":"script_read_failed","path":norm(path)});
    };
    let mut ranges = Vec::with_capacity(edits.len());
    for edit in edits {
        let Some(start) = edit.get("start") else {
            return json!({"error":"edit_start_required"});
        };
        let Some(end) = edit.get("end") else {
            return json!({"error":"edit_end_required"});
        };
        let Some(a) = line_column_offset(&content, start) else {
            return json!({"error":"edit_start_out_of_range"});
        };
        let Some(b) = line_column_offset(&content, end) else {
            return json!({"error":"edit_end_out_of_range"});
        };
        if a > b {
            return json!({"error":"edit_range_reversed"});
        }
        let text = edit
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        ranges.push((a, b, text));
    }
    ranges.sort_by(|left, right| right.0.cmp(&left.0));
    for pair in ranges.windows(2) {
        if pair[0].0 < pair[1].1 {
            return json!({"error":"edit_ranges_overlap"});
        }
    }
    for (start, end, text) in ranges {
        content.replace_range(start..end, &text);
    }
    if let Err(error) = fs::write(path, content.as_bytes()) {
        return json!({"error":format!("script_write_failed:{error}")});
    }
    json!({"success":true,"action":"apply_text_edits","path":norm(path),"bytes":content.len(),"edit_count":edits.len()})
}

fn line_column_offset(content: &str, point: &Value) -> Option<usize> {
    let line = point.get("line").and_then(Value::as_u64)?.checked_sub(1)? as usize;
    let column = point
        .get("column")
        .and_then(Value::as_u64)?
        .checked_sub(1)? as usize;
    let mut offset = 0usize;
    for (index, segment) in content.split_inclusive('\n').enumerate() {
        if index == line {
            return segment
                .char_indices()
                .nth(column)
                .map(|(i, _)| offset + i)
                .or_else(|| (column == segment.chars().count()).then_some(offset + segment.len()));
        }
        offset += segment.len();
    }
    (line == content.split('\n').count().saturating_sub(1) && column == content.chars().count())
        .then_some(content.len())
}
fn script_path(p: &Value) -> Option<PathBuf> {
    let r = Path::new(p.get("path")?.as_str()?.trim_start_matches(['\\', '/']));
    if r.as_os_str().is_empty()
        || r.is_absolute()
        || r.components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        None
    } else if r.starts_with("data/scripts") {
        Some(r.to_path_buf())
    } else {
        Some(PathBuf::from("data/scripts").join(r))
    }
}
fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}
