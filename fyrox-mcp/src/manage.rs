//! Editor 场景管理模块：通过 Fyrox Editor 的消息和命令系统创建、保存场景及批量添加通用节点。

use fyrox::{
    core::{
        algebra::{Matrix4, UnitQuaternion, Vector3},
        color::Color,
    },
    graph::SceneGraph,
    material::{Material, MaterialResource, MaterialResourceExtension},
    scene::{
        base::BaseBuilder,
        camera::{CameraBuilder, OrthographicProjection, Projection},
        light::{point::PointLightBuilder, BaseLightBuilder},
        mesh::{
            surface::{SurfaceBuilder, SurfaceData, SurfaceResource},
            MeshBuilder,
        },
        node::Node,
        pivot::PivotBuilder,
        transform::TransformBuilder,
    },
};
use fyroxed_base::{Editor, Message};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 处理 Unity MCP 风格的场景动作；`params` 包含 action/path，返回排队结果或参数错误。
pub fn manage_scene(editor: &mut Editor, params: &Value) -> Value {
    match params.get("action").and_then(Value::as_str).unwrap_or("") {
        "create" => {
            editor.message_sender.send(Message::NewScene);
            json!({"ok":true,"action":"create","queued":true})
        }
        "save" => save_scene(editor, params),
        "load" => {
            let Some(path) = safe_scene_path(params) else {
                return json!({"error":"path_must_be_rgs_below_data"});
            };
            editor.message_sender.send(Message::LoadScene(path.clone()));
            json!({"ok":true,"action":"load","queued":true,"path":normalized(&path)})
        }
        "load_and_capture" => json!({"error":"load_and_capture_async_workflow_required"}),
        action => {
            json!({"error":"invalid_scene_action","action":action,"valid":["create","save","load","load_and_capture"]})
        }
    }
}

/*
pub fn load_and_select(editor: &mut Editor, path: &Path) -> Result<&'static str, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let resource_manager = editor.engine.resource_manager.clone();
    let sender = editor.message_sender.clone();
    match extension {
        "rgs" => {
            let serialization = editor.engine.serialization_context.clone();
            let constructors = editor.engine.dyn_type_constructors.clone();
            let (loader, _) = block_on(SceneLoader::from_file(
                path,
                &FsResourceIo,
                serialization,
                constructors,
                resource_manager,
            ))
            .map_err(|error| error.to_string())?;
            let scene = block_on(loader.finish());
            let entry = EditorSceneEntry::new_game_scene(
                scene,
                Some(path.to_owned()),
                &mut editor.engine,
                &mut editor.settings,
                sender,
                &editor.scene_viewer,
                editor.highlighter.clone(),
            );
            editor.scenes.add_and_select(entry);
            Ok("game_scene")
        }
        "ui" => {
            let widgets = editor.engine.widget_constructors.clone();
            let constructors = editor.engine.dyn_type_constructors.clone();
            let ui = block_on(fyrox::gui::UserInterface::load_from_file_ex(
                path,
                widgets,
                constructors,
                resource_manager,
                &FsResourceIo,
            ))
            .map_err(|error| error.to_string())?
            .0;
            let entry = EditorSceneEntry::new_ui_scene(
                ui,
                Some(path.to_owned()),
                sender,
                &editor.scene_viewer,
                &mut editor.engine,
                &editor.settings,
            );
            editor.scenes.add_and_select(entry);
            Ok("ui_scene")
        }
        _ => Err("path_must_be_rgs_or_ui_below_data".to_owned()),
    }
}

*/

/// 批量创建通用 Fyrox 节点；`params.nodes` 是节点描述数组，返回已进入 Editor 命令栈的数量。
pub fn create_nodes(editor: &mut Editor, params: &Value) -> Value {
    let Some(specs) = params.get("nodes").and_then(Value::as_array) else {
        return json!({"error":"nodes_array_required"});
    };
    if specs.is_empty() || specs.len() > 256 {
        return json!({"error":"nodes_count_out_of_range","minimum":1,"maximum":256});
    }

    let Some(scene_handle) = crate::scene::active_handle(editor) else {
        return json!({"error":"active_game_scene_not_found"});
    };
    // Add to the actual graph and link normally. FyroxEd's AddNodeCommand links
    // with `KeepGlobalTransform`, which replaces the supplied local transform for
    // a detached node; that made a successful create response lose its position.
    let graph = &mut editor.engine.scenes[scene_handle].graph;
    let parent = graph.get_root();
    let mut names = Vec::with_capacity(specs.len());
    for spec in specs {
        match build_node(spec) {
            Ok(node) => {
                names.push(node.name().to_owned());
                let handle = graph.add_node(node);
                graph.link_nodes(handle, parent);
            }
            Err(error) => return json!({"error":error,"created_before_error":names}),
        }
    }
    json!({"ok":true,"queued":false,"undoable":false,"count":names.len(),"names":names})
}

/// 保存当前活动场景；`editor` 提供活动场景 ID，`params.path` 限定在 data 下的 rgs 文件。
fn save_scene(editor: &mut Editor, params: &Value) -> Value {
    let Some(path) = safe_scene_path(params) else {
        return json!({"error":"path_must_be_rgs_below_data"});
    };
    let id = editor.scenes.current_scene_entry_ref().id;
    editor.message_sender.send(Message::SaveScene {
        id,
        path: path.clone(),
    });
    json!({"ok":true,"action":"save","queued":true,"path":normalized(&path)})
}

/// 从 JSON 描述构造尚未加入 Graph 的通用节点；返回节点或明确的字段错误。
fn build_node(spec: &Value) -> Result<Node, &'static str> {
    let name = spec
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or("node_name_required")?;
    let kind = spec.get("kind").and_then(Value::as_str).unwrap_or("cube");
    let position = vector(spec, "position", [0.0, 0.0, 0.0])?;
    let rotation = vector(spec, "rotation", [0.0, 0.0, 0.0])?;
    let scale = vector(spec, "scale", [1.0, 1.0, 1.0])?;
    let transform = TransformBuilder::new()
        .with_local_position(Vector3::from(position))
        .with_local_rotation(UnitQuaternion::from_euler_angles(
            rotation[0].to_radians(),
            rotation[1].to_radians(),
            rotation[2].to_radians(),
        ))
        .with_local_scale(Vector3::from(scale))
        .build();
    let base = BaseBuilder::new()
        .with_name(name)
        .with_local_transform(transform);

    match kind {
        "pivot" => Ok(PivotBuilder::new(base).build_node()),
        // A generic point light keeps editor-authored primitive scenes visible in camera
        // captures. It is not gameplay-specific and remains serialized in the `.rgs` asset.
        "point_light" => Ok(PointLightBuilder::new(
            BaseLightBuilder::new(base).with_scatter_enabled(false),
        )
        .with_radius(spec.get("radius").and_then(Value::as_f64).unwrap_or(30.0) as f32)
        .build_node()),
        "camera" => Ok(CameraBuilder::new(base)
            .with_projection(Projection::Orthographic(OrthographicProjection {
                z_near: -100.0,
                z_far: 100.0,
                vertical_size: spec
                    .get("vertical_size")
                    .and_then(Value::as_f64)
                    .unwrap_or(22.0) as f32,
            }))
            .build_node()),
        "cube" | "sphere" | "cylinder" => {
            let surface = match kind {
                "sphere" => SurfaceData::make_sphere(16, 16, 0.5, &Matrix4::identity()),
                "cylinder" => SurfaceData::make_cylinder(24, 0.5, 1.0, true, &Matrix4::identity()),
                _ => SurfaceData::make_cube(Matrix4::identity()),
            };
            Ok(MeshBuilder::new(base)
                .with_surfaces(vec![SurfaceBuilder::new(SurfaceResource::new_embedded(
                    surface,
                ))
                .with_material(colored_material(spec)?)
                .build()])
                .build_node())
        }
        _ => Err("unsupported_node_kind"),
    }
}

/// 构造标准材质；`spec.color` 为 0..255 RGBA 数组，返回可共享的 Fyrox 材质资源。
fn colored_material(spec: &Value) -> Result<MaterialResource, &'static str> {
    let color = byte_color(spec.get("color"))?;
    let mut material = Material::standard();
    material.set_property(
        "diffuseColor",
        Color::from_rgba(color[0], color[1], color[2], color[3]),
    );
    Ok(MaterialResource::new(material))
}

/// 读取三维向量字段；缺失时使用 `fallback`，字段存在但格式错误时返回错误。
fn vector(spec: &Value, field: &str, fallback: [f32; 3]) -> Result<[f32; 3], &'static str> {
    let Some(value) = spec.get(field) else {
        return Ok(fallback);
    };
    let values = value.as_array().ok_or("vector_must_be_three_numbers")?;
    if values.len() != 3 {
        return Err("vector_must_be_three_numbers");
    }
    Ok([
        values[0].as_f64().ok_or("vector_must_be_three_numbers")? as f32,
        values[1].as_f64().ok_or("vector_must_be_three_numbers")? as f32,
        values[2].as_f64().ok_or("vector_must_be_three_numbers")? as f32,
    ])
}

/// 读取 RGBA 字节颜色；缺失时返回白色，非法值返回参数错误。
fn byte_color(value: Option<&Value>) -> Result<[u8; 4], &'static str> {
    let Some(values) = value.and_then(Value::as_array) else {
        return Ok([255, 255, 255, 255]);
    };
    if values.len() != 4 {
        return Err("color_must_be_four_bytes");
    }
    let mut color = [0u8; 4];
    for (target, value) in color.iter_mut().zip(values) {
        let number = value.as_u64().ok_or("color_must_be_four_bytes")?;
        *target = u8::try_from(number).map_err(|_| "color_must_be_four_bytes")?;
    }
    Ok(color)
}

/// 验证场景路径仅位于 data 目录且扩展名为 rgs；返回规范的相对路径。
fn safe_scene_path(params: &Value) -> Option<PathBuf> {
    let path = PathBuf::from(params.get("path")?.as_str()?);
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        || path.extension().and_then(|ext| ext.to_str()) != Some("rgs")
        || !path.starts_with("data")
    {
        return None;
    }
    Some(path)
}

/// 将 Windows 路径转换为 JSON 使用的斜杠形式；`path` 为项目相对路径。
#[inline]
fn normalized(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
