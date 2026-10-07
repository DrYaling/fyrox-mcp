//! Fyrox UI-scene MCP operations. Every mutation targets the editor's
//! serialized `UiScene.ui`; no runtime/game UI is created by this module.
use fyrox::{
    core::{algebra::Vector2, pool::Handle},
    graph::SceneGraph,
    graphics::framebuffer::ReadTarget,
    gui::{
        border::BorderBuilder,
        brush::Brush,
        button::{Button, ButtonBuilder},
        canvas::CanvasBuilder,
        decorator::Decorator,
        text::{Text, TextBuilder, TextMessage},
        widget::WidgetBuilder,
        UiNode, UserInterface,
    },
    renderer::ui_renderer::UiRenderInfo,
    resource::texture::{TextureResource, TextureResourceExtension},
};
use fyroxed_base::{ui_scene::UiScene, Editor, Message};
use image::ImageEncoder;
use serde_json::{json, Value};
use std::{collections::HashMap, io::Cursor, path::PathBuf};

pub fn manage(editor: &mut Editor, params: &Value) -> Value {
    match params
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("ping")
    {
        "ping" => json!({"success":true,"backend":"fyrox_ui_scene","serialized_resource":true}),
        "create_scene" => create_scene(editor, params),
        "load" | "load_scene" => load_scene(editor, params),
        "create" => create(editor, params),
        "get_visual_tree" | "read" => tree(editor),
        "validate_layout" => validate_layout(editor, params),
        "modify_visual_element" | "update" => modify(editor, params),
        "save" => save(editor, params),
        "render_ui" | "screenshot" => render(editor, params),
        "load_and_capture" => load_and_capture(editor, params),
        _ => {
            json!({"error":"invalid_ui_action","valid":["ping","create_scene","load","create","get_visual_tree","validate_layout","modify_visual_element","save","render_ui","load_and_capture"]})
        }
    }
}

fn ui_path(params: &Value) -> Option<PathBuf> {
    let path = PathBuf::from(params.get("path")?.as_str()?);
    if path.is_absolute()
        || !path.starts_with("data")
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        || path.extension().and_then(|x| x.to_str()) != Some("ui")
    {
        return None;
    }
    Some(path)
}

/// Queue the editor's normal scene loader. Loading is asynchronous; callers
/// should wait one editor tick, then call get_visual_tree/render_ui.
fn load_scene(editor: &mut Editor, params: &Value) -> Value {
    let Some(path) = ui_path(params) else {
        return json!({"error":"ui_path_must_be_data_ui_resource"});
    };
    editor.message_sender.send(Message::LoadScene(path.clone()));
    json!({"success":true,"queued":true,"action":"load","path":path.to_string_lossy()})
}

fn load_and_capture(editor: &mut Editor, params: &Value) -> Value {
    let Some(path) = ui_path(params) else {
        return json!({"error":"ui_path_must_be_data_ui_resource"});
    };
    editor.message_sender.send(Message::LoadScene(path.clone()));
    json!({"success":true,"queued":true,"action":"load_and_capture","path":path.to_string_lossy(),"next":"call manage_ui.render_ui after the load is applied"})
}

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
    if !safe_ui_path(&path) {
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

fn safe_ui_path(path: &std::path::Path) -> bool {
    !path.is_absolute()
        && path.starts_with("data")
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        && path.extension().and_then(|x| x.to_str()) == Some("ui")
}

fn create(editor: &mut Editor, params: &Value) -> Value {
    let Some(ui) = current_ui(editor) else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("McpWidget");
    if find(&ui.ui, name).is_some() {
        return json!({"error":"ui_node_already_exists","name":name});
    }
    let parent = match params.get("parent").and_then(Value::as_str) {
        Some(parent) => match find(&ui.ui, parent) {
            Some(handle) => handle,
            None => return json!({"error":"ui_parent_not_found","parent":parent}),
        },
        None => ui.ui.root(),
    };
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
    let wb = apply_brushes(wb, params);
    let handle: Handle<UiNode> = match kind {
        "button" => ButtonBuilder::new(wb)
            .with_text(params.get("text").and_then(Value::as_str).unwrap_or(name))
            .build(&mut ui.ui.build_ctx())
            .to_base(),
        "text" => TextBuilder::new(wb)
            .with_text(params.get("text").and_then(Value::as_str).unwrap_or(name))
            .with_font_size(
                (params
                    .get("font_size")
                    .and_then(Value::as_f64)
                    .unwrap_or(18.0) as f32)
                    .into(),
            )
            .build(&mut ui.ui.build_ctx())
            .to_base(),
        "panel" => BorderBuilder::new(wb)
            .build(&mut ui.ui.build_ctx())
            .to_base(),
        "canvas" => CanvasBuilder::new(wb)
            .build(&mut ui.ui.build_ctx())
            .to_base(),
        _ => {
            return json!({"error":"unsupported_ui_kind","kind":kind,"supported":["text","button","panel","canvas"]})
        }
    };
    ui.ui.link_nodes(handle, parent, false);
    if kind == "button" {
        let style = color(params.get("background"));
        let text_color = color(params.get("foreground"));
        let parts = ui
            .ui
            .node(handle)
            .cast::<Button>()
            .map(|button| (*button.decorator, *button.content));
        if let Some((decorator, content)) = parts {
            if let Some(value) = style {
                let brush: Brush = Brush::Solid(value);
                if let Some(back) = ui.ui.node_mut(decorator).cast_mut::<Decorator>() {
                    let property: fyrox::gui::style::StyledProperty<Brush> = brush.clone().into();
                    back.normal_brush
                        .set_value_and_mark_modified(property.clone());
                    back.hover_brush
                        .set_value_and_mark_modified(property.clone());
                    back.pressed_brush
                        .set_value_and_mark_modified(property.clone());
                    back.set_background(brush.into());
                }
            }
            if let Some(value) = text_color {
                ui.ui
                    .node_mut(content)
                    .set_foreground(Brush::Solid(value).into());
            }
        }
    }
    ui.ui.update(ui.ui.screen_size(), 0.0, &Default::default());
    json!({"success":true,"name":name,"kind":kind,"handle":{"index":handle.index(),"generation":handle.generation()},"parent":ui.ui.node(parent).name()})
}

fn color(value: Option<&Value>) -> Option<fyrox::core::color::Color> {
    let values = value?.as_array()?;
    if values.len() != 4 {
        return None;
    }
    Some(fyrox::core::color::Color::from_rgba(
        values[0].as_u64()? as u8,
        values[1].as_u64()? as u8,
        values[2].as_u64()? as u8,
        values[3].as_u64()? as u8,
    ))
}

fn apply_brushes(mut widget: WidgetBuilder, params: &Value) -> WidgetBuilder {
    if let Some(value) = color(params.get("background")) {
        widget = widget.with_background(Brush::Solid(value).into());
    }
    if let Some(value) = color(params.get("foreground")) {
        widget = widget.with_foreground(Brush::Solid(value).into());
    }
    widget
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
    let items = ui.nodes().pair_iter().map(|(h, node)| {
        let kind = if node.cast::<Text>().is_some() {
            "Text"
        } else if node.cast::<Button>().is_some() {
            "Button"
        } else if node.is_or_has_field::<fyrox::gui::image::Image>() {
            "Image"
        } else {
            "Widget"
        };
        let text = node.cast::<Text>().map(|value| value.text());
        json!({
        "handle":{"index":h.index(),"generation":h.generation()}, "name":node.name(),
        "kind":kind, "text":text,
        "parent":{"index":node.parent().index(),"generation":node.parent().generation()},
        "children":node.children().iter().map(|c| json!({"index":c.index(),"generation":c.generation()})).collect::<Vec<_>>(),
        "position":[node.actual_local_position().x,node.actual_local_position().y],
        "size":[node.actual_local_size().x,node.actual_local_size().y], "visible":node.global_visibility
    })}).collect::<Vec<_>>();
    json!({"success":true,"count":items.len(),"items":items,"root":{"index":ui.root().index(),"generation":ui.root().generation()}})
}

fn validate_layout(editor: &mut Editor, params: &Value) -> Value {
    let Some(ui_scene) = current_ui(editor) else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    let ui = &mut ui_scene.ui;
    let old_size = ui.screen_size();
    let root = ui.root();
    let (old_width, old_height) = (ui.node(root).width(), ui.node(root).height());
    let width = params
        .get("width")
        .and_then(Value::as_u64)
        .unwrap_or(old_size.x as u64);
    let height = params
        .get("height")
        .and_then(Value::as_u64)
        .unwrap_or(old_size.y as u64);
    if !(64..=4096).contains(&width) || !(64..=4096).contains(&height) {
        return json!({"error":"ui_validation_dimensions_out_of_range"});
    }
    ui.set_screen_size(Vector2::new(width as f32, height as f32));
    ui.node_mut(root)
        .set_width(width as f32)
        .set_height(height as f32);
    ui.update(ui.screen_size(), 0.0, &Default::default());

    let mut names = HashMap::<String, usize>::new();
    let mut buttons = Vec::new();
    let mut issues = Vec::new();
    let mut visible_nodes = 0usize;
    for (handle, node) in ui.nodes().pair_iter() {
        let name = node.name();
        if name.is_empty() {
            continue;
        }
        *names.entry(name.to_owned()).or_default() += 1;
        if !node.global_visibility {
            continue;
        }
        visible_nodes += 1;
        let bounds = node.screen_bounds();
        if node.cast::<Button>().is_some() {
            let x = bounds.position.x;
            let y = bounds.position.y;
            let w = bounds.size.x;
            let h = bounds.size.y;
            if w < 1.0 || h < 1.0 {
                issues.push(json!({"kind":"zero_size_button","name":name}));
            } else if x < -0.5
                || y < -0.5
                || x + w > width as f32 + 0.5
                || y + h > height as f32 + 0.5
            {
                issues.push(json!({"kind":"button_outside_canvas","name":name,"bounds":[x,y,w,h]}));
            }
            buttons.push((name.to_owned(), node.parent(), x, y, w, h));
        }
        let _ = handle;
    }
    for (name, count) in &names {
        if *count > 1 {
            issues.push(json!({"kind":"duplicate_name","name":name,"count":count}));
        }
    }
    if let Some(expected) = params.get("expected_names").and_then(Value::as_array) {
        for name in expected.iter().filter_map(Value::as_str) {
            if !names.contains_key(name) {
                issues.push(json!({"kind":"missing_required_node","name":name}));
            }
        }
    }
    for (index, first) in buttons.iter().enumerate() {
        for second in buttons.iter().skip(index + 1) {
            if first.1 != second.1 {
                continue;
            }
            let overlap_x = (first.2 + first.4).min(second.2 + second.4) - first.2.max(second.2);
            let overlap_y = (first.3 + first.5).min(second.3 + second.5) - first.3.max(second.3);
            let smaller = (first.4 * first.5).min(second.4 * second.5);
            if overlap_x > 0.0
                && overlap_y > 0.0
                && smaller > 0.0
                && overlap_x * overlap_y / smaller > 0.2
            {
                issues
                    .push(json!({"kind":"overlapping_buttons","first":first.0,"second":second.0}));
            }
        }
    }
    ui.set_screen_size(old_size);
    ui.node_mut(root)
        .set_width(old_width)
        .set_height(old_height);
    ui.update(ui.screen_size(), 0.0, &Default::default());
    json!({"success":issues.is_empty(),"width":width,"height":height,"named_nodes":names.len(),"visible_nodes":visible_nodes,"buttons":buttons.len(),"issues":issues})
}

fn modify(editor: &mut Editor, params: &Value) -> Value {
    let Some(ui_scene) = current_ui(editor) else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    if let Some(updates) = params.get("updates").and_then(Value::as_array) {
        let mut applied = 0usize;
        let mut errors = Vec::new();
        for update in updates {
            match modify_one(&mut ui_scene.ui, update) {
                Ok(()) => applied += 1,
                Err(error) => errors
                    .push(json!({"name":update.get("name").and_then(Value::as_str),"error":error})),
            }
        }
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
        return json!({"success":errors.is_empty(),"updated":applied,"requested":updates.len(),"errors":errors});
    }
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(handle) = find(&ui_scene.ui, name) else {
        return json!({"error":"ui_node_not_found","name":name});
    };
    let ui = &mut ui_scene.ui;
    if let Err(error) = modify_one(ui, params) {
        return json!({"error":error,"name":name});
    }
    ui.update(ui.screen_size(), 0.0, &Default::default());
    json!({"success":true,"name":name,"position":[ui.node(handle).actual_local_position().x,ui.node(handle).actual_local_position().y],"size":[ui.node(handle).actual_local_size().x,ui.node(handle).actual_local_size().y]})
}

fn modify_one(ui: &mut UserInterface, params: &Value) -> Result<(), String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(handle) = find(ui, name) else {
        return Err("ui_node_not_found".into());
    };
    if let Some(v) = params.get("width").and_then(Value::as_f64) {
        ui.node_mut(handle).set_width(v as f32);
    }
    if let Some(v) = params.get("height").and_then(Value::as_f64) {
        ui.node_mut(handle).set_height(v as f32);
    }
    if let Some(v) = params.get("enabled").and_then(Value::as_bool) {
        ui.node_mut(handle).set_enabled(v);
    }
    if let Some(v) = params.get("visible").and_then(Value::as_bool) {
        ui.node_mut(handle).set_visibility(v);
    }
    if let Some(values) = params
        .get("position")
        .and_then(Value::as_array)
        .filter(|v| v.len() == 2)
    {
        ui.node_mut(handle).set_desired_local_position(Vector2::new(
            values[0].as_f64().unwrap_or_default() as f32,
            values[1].as_f64().unwrap_or_default() as f32,
        ));
    }
    let button_children = ui
        .node(handle)
        .cast::<Button>()
        .map(|button| (*button.decorator, *button.content));
    if let Some(value) = color(params.get("background")) {
        let brush: Brush = Brush::Solid(value);
        ui.node_mut(handle).set_background(brush.clone().into());
        if let Some((decorator, _)) = button_children {
            if let Some(back) = ui.node_mut(decorator).cast_mut::<Decorator>() {
                back.normal_brush
                    .set_value_and_mark_modified(brush.clone().into());
                back.hover_brush
                    .set_value_and_mark_modified(brush.clone().into());
                back.pressed_brush
                    .set_value_and_mark_modified(brush.clone().into());
                back.set_background(brush.into());
            }
        }
    }
    if let Some(value) = color(params.get("foreground")) {
        let brush: Brush = Brush::Solid(value);
        ui.node_mut(handle).set_foreground(brush.clone().into());
        if let Some((_, content)) = button_children {
            ui.node_mut(content).set_foreground(brush.into());
        }
    }
    if let Some(value) = params.get("font_size").and_then(Value::as_f64) {
        let content = button_children
            .map(|(_, content)| content)
            .unwrap_or(handle);
        ui.send(content, TextMessage::FontSize((value as f32).into()));
    }
    if let Some(value) = params.get("text").and_then(Value::as_str) {
        let content = button_children
            .map(|(_, content)| content)
            .unwrap_or(handle);
        if let Some(text) = ui.node_mut(content).cast_mut::<Text>() {
            text.set_bbcode(value.to_owned());
        }
    }
    Ok(())
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

pub fn render(editor: &mut Editor, params: &Value) -> Value {
    // Split the editor fields so the serialized UI and renderer can be
    // borrowed simultaneously without raw pointers.
    let (scenes, engine) = (&mut editor.scenes, &mut editor.engine);
    let entry = scenes.current_scene_entry_mut();
    let scene_handle = entry.id;
    let Some(ui_scene) = entry.controller.downcast_mut::<UiScene>() else {
        return json!({"error":"active_ui_scene_not_found"});
    };
    let original_target = ui_scene.render_target.clone();
    let original_screen_size = ui_scene.ui.screen_size();
    let requested_width = params.get("width").and_then(Value::as_u64);
    let requested_height = params.get("height").and_then(Value::as_u64);
    if requested_width.is_some() != requested_height.is_some() {
        return json!({"error":"ui_capture_dimensions_require_width_and_height"});
    }
    let requested_size = requested_width
        .zip(requested_height)
        .map(|(width, height)| (width as u32, height as u32));
    if let Some((width, height)) = requested_size {
        if !(64..=4096).contains(&width) || !(64..=4096).contains(&height) {
            return json!({"error":"ui_capture_dimensions_out_of_range","minimum":64,"maximum":4096});
        }
        // The editor's UI scene target follows the dock viewport (often
        // 945x653). Temporarily replace it and the UI screen size so portrait
        // resources can be rendered at their reference resolution.
        ui_scene.render_target = TextureResource::new_render_target(width, height);
        ui_scene
            .ui
            .set_screen_size(Vector2::new(width as f32, height as f32));
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
    }
    let target = ui_scene.render_target.clone();
    // The editor UI controller replaces the root's canvas size with its dock
    // viewport. For a reference-resolution capture, force the root widget to
    // occupy the requested canvas before measuring and drawing.
    if let Some((width, height)) = requested_size {
        let root = ui_scene.ui.root();
        ui_scene.ui.node_mut(root).set_width(width as f32);
        ui_scene.ui.node_mut(root).set_height(height as f32);
    }
    ui_scene
        .ui
        .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
    let draw_commands = ui_scene.ui.drawing_context.get_commands().len();
    // A headless/editor instance may still build UI draw commands while it has
    // no GPU context. Do not call `as_initialized_mut` in that state: it
    // panics and turns an actionable diagnostic into an editor plugin failure.
    if !matches!(
        engine.graphics_context,
        fyrox::engine::GraphicsContext::Initialized(_)
    ) {
        ui_scene.render_target = original_target;
        ui_scene.ui.set_screen_size(original_screen_size);
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
        return json!({
            "error":"graphics_context_uninitialized",
            "draw_commands":draw_commands,
            "detail":"UI layout is available, but screenshot rendering requires a non-headless editor with an initialized GPU context"
        });
    }
    let graphics = engine.graphics_context.as_initialized_mut();
    if let Err(e) = graphics.renderer.render_ui(UiRenderInfo {
        ui: &ui_scene.ui,
        render_target: Some(target.clone()),
        clear_color: fyrox::core::color::Color::DIM_GRAY,
        resource_manager: &engine.resource_manager,
    }) {
        ui_scene.render_target = original_target;
        ui_scene.ui.set_screen_size(original_screen_size);
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
        return json!({"error":format!("ui_render_failed:{e}")});
    }
    let Some(fb) = graphics.renderer.ui_frame_buffers.get(&target.key()) else {
        ui_scene.render_target = original_target;
        ui_scene.ui.set_screen_size(original_screen_size);
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
        return json!({"error":"ui_framebuffer_unavailable"});
    };
    let Some(pixels) = fb.read_pixels(ReadTarget::Color(0)) else {
        ui_scene.render_target = original_target;
        ui_scene.ui.set_screen_size(original_screen_size);
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
        return json!({"error":"ui_gpu_readback_unavailable"});
    };
    let target_data = target.data_ref();
    let Some(data) = target_data.as_loaded_ref() else {
        ui_scene.render_target = original_target;
        ui_scene.ui.set_screen_size(original_screen_size);
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
        return json!({"error":"ui_render_target_not_loaded"});
    };
    let Some(size) = data.kind().rectangle_size() else {
        ui_scene.render_target = original_target;
        ui_scene.ui.set_screen_size(original_screen_size);
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
        return json!({"error":"ui_render_target_not_rectangle"});
    };
    let (w, h) = (size.x as usize, size.y as usize);
    let row_bytes = w * 4;
    let expected_bytes = row_bytes * h;
    if pixels.len() != expected_bytes {
        ui_scene.render_target = original_target;
        ui_scene.ui.set_screen_size(original_screen_size);
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
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
        ui_scene.render_target = original_target;
        ui_scene.ui.set_screen_size(original_screen_size);
        ui_scene
            .ui
            .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
        return json!({"error":"png_encode_failed"});
    }
    ui_scene.render_target = original_target;
    ui_scene.ui.set_screen_size(original_screen_size);
    ui_scene
        .ui
        .update(ui_scene.ui.screen_size(), 0.0, &Default::default());
    let file_name = params
        .get("file_name")
        .or_else(|| params.get("fileName"))
        .and_then(Value::as_str)
        .unwrap_or("ui-render.png");
    let (full_path, relative_path) = match crate::capture::persist_png(file_name, &png) {
        Ok(paths) => paths,
        Err(error) => return json!({"error":format!("ui_screenshot_save_failed:{error}")}),
    };
    json!({"success":true,"scene_id":format!("{scene_handle:?}"),"mime_type":"image/png","width":w,"height":h,"path":relative_path,"full_path":full_path,"draw_commands":draw_commands,"source":"fyrox_ui_scene_gpu_framebuffer"})
}
