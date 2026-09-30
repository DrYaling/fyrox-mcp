//! Fyrox UI-scene MCP operations. Every mutation targets the editor's
//! serialized `UiScene.ui`; no runtime/game UI is created by this module.
use fyrox::{
    core::{algebra::Vector2, pool::Handle},
    graph::SceneGraph,
    graphics::framebuffer::ReadTarget,
    gui::{button::ButtonBuilder, text::TextBuilder, widget::WidgetBuilder, UiNode, UserInterface},
    renderer::ui_renderer::UiRenderInfo,
};
use fyroxed_base::{ui_scene::UiScene, Editor, Message};
use image::ImageEncoder;
use serde_json::{json, Value};
use std::{io::Cursor, path::PathBuf};

pub fn manage(editor: &mut Editor, params: &Value) -> Value {
    match params
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("ping")
    {
        "ping" => json!({"success":true,"backend":"fyrox_ui_scene","serialized_resource":true}),
        "create_scene" => create_scene(editor, params),
        "create" => create(editor, params),
        "get_visual_tree" | "read" => tree(editor),
        "modify_visual_element" | "update" => modify(editor, params),
        "save" => save(editor, params),
        "render_ui" | "screenshot" => render(editor),
        "load_and_capture" => json!({"error":"load_and_capture_async_workflow_required"}),
        _ => {
            json!({"error":"invalid_ui_action","valid":["ping","create_scene","create","get_visual_tree","modify_visual_element","save","render_ui","load_and_capture"]})
        }
    }
}

/* fn load_and_capture(editor: &mut Editor, params: &Value) -> Value {
    let Some(path) = params
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
    else {
        return json!({"error":"ui_path_required"});
    };
    if !path.starts_with("data") || path.extension().and_then(|x| x.to_str()) != Some("ui") {
        return json!({"error":"ui_path_must_be_data_ui_resource"});
    }
    match crate::manage::load_and_select(editor, &path) {
        Ok("ui_scene") => {
            let mut result = render(editor);
            result["selected_path"] = json!(path.to_string_lossy());
            result
        }
        Ok(_) => json!({"error":"loaded_resource_is_not_ui"}),
        Err(error) => json!({"error":"ui_load_and_select_failed","detail":error}),
    }
}
*/

fn current_ui(editor: &mut Editor) -> Option<&mut UiScene> {
    editor
        .scenes
        .current_scene_entry_mut()
        .controller
        .downcast_mut::<UiScene>()
}

fn create_scene(editor: &mut Editor, params: &Value) -> Value {
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("data/mcp_ui.ui");
    let path = PathBuf::from(path);
    if !path.starts_with("data") || path.extension().and_then(|x| x.to_str()) != Some("ui") {
        return json!({"error":"ui_path_must_be_data_ui_resource"});
    }
    let size = params
        .get("size")
        .and_then(Value::as_array)
        .filter(|x| x.len() == 2)
        .map(|x| {
            Vector2::new(
                x[0].as_f64().unwrap_or(800.0) as f32,
                x[1].as_f64().unwrap_or(600.0) as f32,
            )
        })
        .unwrap_or_else(|| Vector2::new(800.0, 600.0));
    editor.message_sender.send(Message::AddUiScene {
        ui: UserInterface::new(size),
        path: path.clone(),
    });
    json!({"success":true,"queued":true,"path":path.to_string_lossy()})
}

fn create(editor: &mut Editor, params: &Value) -> Value {
    let Some(ui) = current_ui(editor) else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("McpWidget");
    let kind = params.get("kind").and_then(Value::as_str).unwrap_or("text");
    let width = params.get("width").and_then(Value::as_f64).unwrap_or(240.0) as f32;
    let height = params.get("height").and_then(Value::as_f64).unwrap_or(48.0) as f32;
    let position = params
        .get("position")
        .and_then(Value::as_array)
        .filter(|v| v.len() == 2)
        .map(|v| {
            Vector2::new(
                v[0].as_f64().unwrap_or_default() as f32,
                v[1].as_f64().unwrap_or_default() as f32,
            )
        })
        .unwrap_or_default();
    let wb = WidgetBuilder::new()
        .with_name(name)
        .with_width(width)
        .with_height(height)
        .with_desired_position(position);
    let handle: Handle<UiNode> = match kind {
        "button" => ButtonBuilder::new(wb)
            .with_text(params.get("text").and_then(Value::as_str).unwrap_or(name))
            .build(&mut ui.ui.build_ctx())
            .to_base(),
        "text" => TextBuilder::new(wb)
            .with_text(params.get("text").and_then(Value::as_str).unwrap_or(name))
            .build(&mut ui.ui.build_ctx())
            .to_base(),
        _ => {
            return json!({"error":"unsupported_ui_kind","kind":kind,"supported":["text","button"]})
        }
    };
    let parent = params
        .get("parent")
        .and_then(Value::as_str)
        .and_then(|name| find(&ui.ui, name))
        .unwrap_or(ui.ui.root());
    ui.ui.link_nodes(handle, parent, false);
    ui.ui.update(ui.ui.screen_size(), 0.0, &Default::default());
    json!({"success":true,"name":name,"kind":kind,"handle":{"index":handle.index(),"generation":handle.generation()},"parent":ui.ui.node(parent).name()})
}

fn find(ui: &UserInterface, name: &str) -> Option<Handle<UiNode>> {
    ui.nodes()
        .pair_iter()
        .find(|(_, node)| node.name() == name)
        .map(|(h, _)| h)
}

fn tree(editor: &mut Editor) -> Value {
    let Some(ui_scene) = current_ui(editor) else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    let ui = &ui_scene.ui;
    let items = ui.nodes().pair_iter().map(|(h, node)| json!({
        "handle":{"index":h.index(),"generation":h.generation()}, "name":node.name(),
        "parent":{"index":node.parent().index(),"generation":node.parent().generation()},
        "children":node.children().iter().map(|c| json!({"index":c.index(),"generation":c.generation()})).collect::<Vec<_>>(),
        "position":[node.actual_local_position().x,node.actual_local_position().y],
        "size":[node.actual_local_size().x,node.actual_local_size().y], "visible":node.global_visibility
    })).collect::<Vec<_>>();
    json!({"success":true,"count":items.len(),"items":items,"root":{"index":ui.root().index(),"generation":ui.root().generation()}})
}

fn modify(editor: &mut Editor, params: &Value) -> Value {
    let Some(ui_scene) = current_ui(editor) else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(handle) = find(&ui_scene.ui, name) else {
        return json!({"error":"ui_node_not_found","name":name});
    };
    let ui = &mut ui_scene.ui;
    if let Some(v) = params.get("width").and_then(Value::as_f64) {
        ui.node_mut(handle).set_width(v as f32);
    }
    if let Some(v) = params.get("height").and_then(Value::as_f64) {
        ui.node_mut(handle).set_height(v as f32);
    }
    if let Some(v) = params.get("enabled").and_then(Value::as_bool) {
        ui.node_mut(handle).set_enabled(v);
    }
    ui.update(ui.screen_size(), 0.0, &Default::default());
    json!({"success":true,"name":name,"position":[ui.node(handle).actual_local_position().x,ui.node(handle).actual_local_position().y],"size":[ui.node(handle).actual_local_size().x,ui.node(handle).actual_local_size().y]})
}

fn save(editor: &mut Editor, params: &Value) -> Value {
    let fallback_path = editor.scenes.current_scene_entry_ref().path.clone();
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or(fallback_path);
    let Some(path) = path else {
        return json!({"error":"ui_scene_path_missing"});
    };
    if !path.starts_with("data") || path.extension().and_then(|x| x.to_str()) != Some("ui") {
        return json!({"error":"ui_path_must_be_data_ui_resource"});
    }
    let Some(ui_scene) = current_ui(editor) else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    match ui_scene.ui.clone().save(&path) {
        Ok(_) => json!({"success":true,"path":path.to_string_lossy()}),
        Err(e) => json!({"error":e.to_string()}),
    }
}

pub fn render(editor: &mut Editor) -> Value {
    // Split the editor fields so the serialized UI and renderer can be
    // borrowed simultaneously without raw pointers.
    let (scenes, engine) = (&mut editor.scenes, &mut editor.engine);
    let entry = scenes.current_scene_entry_mut();
    let scene_handle = entry.id;
    let Some(ui_scene) = entry.controller.downcast_mut::<UiScene>() else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    let target = ui_scene.render_target.clone();
    let draw_commands = ui_scene.ui.drawing_context.get_commands().len();
    let graphics = engine.graphics_context.as_initialized_mut();
    if let Err(e) = graphics.renderer.render_ui(UiRenderInfo {
        ui: &ui_scene.ui,
        render_target: Some(target.clone()),
        clear_color: fyrox::core::color::Color::DIM_GRAY,
        resource_manager: &engine.resource_manager,
    }) {
        return json!({"error":format!("ui_render_failed:{e}")});
    }
    let Some(fb) = graphics.renderer.ui_frame_buffers.get(&target.key()) else {
        return json!({"error":"ui_framebuffer_unavailable"});
    };
    let Some(pixels) = fb.read_pixels(ReadTarget::Color(0)) else {
        return json!({"error":"ui_gpu_readback_unavailable"});
    };
    let target_data = target.data_ref();
    let Some(data) = target_data.as_loaded_ref() else {
        return json!({"error":"ui_render_target_not_loaded"});
    };
    let Some(size) = data.kind().rectangle_size() else {
        return json!({"error":"ui_render_target_not_rectangle"});
    };
    let (w, h) = (size.x as usize, size.y as usize);
    let row_bytes = w * 4;
    let expected_bytes = row_bytes * h;
    if pixels.len() != expected_bytes {
        return json!({
            "error":"ui_gpu_readback_size_mismatch",
            "actual_bytes":pixels.len(),
            "expected_bytes":expected_bytes
        });
    }
    // GPU framebuffer rows are bottom-up; PNG consumers expect top-down.
    let mut top_down = vec![0; pixels.len()];
    for row in 0..h {
        let source = row * row_bytes;
        let target = (h - 1 - row) * row_bytes;
        top_down[target..target + row_bytes].copy_from_slice(&pixels[source..source + row_bytes]);
    }
    let mut png = Vec::new();
    if image::codecs::png::PngEncoder::new(Cursor::new(&mut png))
        .write_image(
            &top_down,
            w as u32,
            h as u32,
            image::ExtendedColorType::Rgba8,
        )
        .is_err()
    {
        return json!({"error":"png_encode_failed"});
    }
    let (full_path, relative_path) = match crate::capture::persist_png("ui-render.png", &png) {
        Ok(paths) => paths,
        Err(error) => return json!({"error":format!("ui_screenshot_save_failed:{error}")}),
    };
    json!({"success":true,"scene_id":format!("{scene_handle:?}"),"mime_type":"image/png","width":w,"height":h,"path":relative_path,"full_path":full_path,"draw_commands":draw_commands,"source":"fyrox_ui_scene_gpu_framebuffer"})
}
