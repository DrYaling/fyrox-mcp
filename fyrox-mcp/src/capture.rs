//! Editor 场景相机截图：渲染当前活动场景并将 LDR framebuffer 编码为 PNG。

use base64::Engine;
use fyrox::{
    core::pool::Handle,
    engine::GraphicsContext,
    graph::SceneGraph,
    graphics::{framebuffer::ReadTarget, gpu_texture::GpuTextureKind},
    resource::texture::TextureResourceExtension,
    scene::collider::BitMask,
};
use image::ImageEncoder;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

/// 捕获当前 Editor 活动场景；PNG 写入 `screenshot/`，响应返回路径元数据。
pub fn capture(editor: &mut fyroxed_base::Editor, params: &Value) -> Value {
    let Some(scene_handle) =
        super::scene::game_handle(editor, params.get("scene_path").and_then(Value::as_str))
    else {
        return json!({"error":"active_game_scene_not_found"});
    };
    let camera = params.get("camera").and_then(Value::as_str);
    let elapsed = editor.engine.elapsed_time();
    if !matches!(
        editor.engine.graphics_context,
        GraphicsContext::Initialized(_)
    ) {
        return json!({"error":"graphics_context_uninitialized"});
    }
    let editor_viewport = {
        let ui = editor.engine.user_interfaces.first();
        let size = editor.scene_viewer.frame_bounds(ui).size;
        if size.x >= 64.0 && size.y >= 64.0 {
            Some((size.x.round() as u32, size.y.round() as u32))
        } else {
            None
        }
    };
    let engine = &mut editor.engine;
    let previous_render_target = engine.scenes[scene_handle]
        .rendering_options
        .render_target
        .clone();
    let explicit_width = params
        .get("width")
        .and_then(Value::as_u64)
        .filter(|value| (64..=4096).contains(value));
    let explicit_height = params
        .get("height")
        .and_then(Value::as_u64)
        .filter(|value| (64..=4096).contains(value));
    if explicit_width.is_some() != explicit_height.is_some() {
        return json!({"error":"capture_dimensions_require_width_and_height"});
    }
    let (requested_width, requested_height, target_size_source) =
        if let (Some(width), Some(height)) = (explicit_width, explicit_height) {
            (width as u32, height as u32, "explicit")
        } else if let Some((width, height)) = editor_viewport {
            (width, height, "editor_viewport")
        } else {
            let existing = engine.scenes[scene_handle]
                .rendering_options
                .render_target
                .as_ref()
                .and_then(|target| {
                    target
                        .data_ref()
                        .as_loaded_ref()
                        .and_then(|data| data.kind().rectangle_size())
                })
                .map(|size| (size.x as u32, size.y as u32));
            existing
                .map(|(width, height)| (width, height, "scene_render_target"))
                .unwrap_or((945, 649, "fallback"))
        };
    let resize_target = explicit_width.is_some() || previous_render_target.is_none();
    // Editor-loaded game scenes start with a 0x0 render target until the scene
    // viewer performs its next layout pass. Allocate the capture target here;
    // this does not mutate the game runtime or the Windows swapchain.
    if resize_target {
        engine.scenes[scene_handle].rendering_options.render_target = Some(
            fyrox::resource::texture::TextureResource::new_render_target(
                requested_width,
                requested_height,
            ),
        );
    }
    let active_cameras = engine.scenes[scene_handle]
        .graph
        .pair_iter()
        .filter(|(_, node)| {
            node.is_camera() && node.is_globally_enabled() && node.as_camera().is_enabled()
        })
        .map(|(_, node)| node.name().to_owned())
        .collect::<Vec<_>>();
    if active_cameras.is_empty() {
        engine.scenes[scene_handle].rendering_options.render_target = previous_render_target;
        return json!({"error":"no_active_camera"});
    }
    if camera.is_some_and(|name| !active_cameras.iter().any(|item| item == name)) {
        engine.scenes[scene_handle].rendering_options.render_target = previous_render_target;
        return json!({"error":"camera_not_found_or_disabled","camera":camera});
    }
    // Fyrox 会按顺序渲染所有启用 Camera；指定名称时临时屏蔽其它 Camera，
    // 避免 EditorCamera 的最后一次绘制覆盖调用方要求的游戏摄像机画面。
    let camera_states = if let Some(requested) = camera {
        let scene = &mut engine.scenes[scene_handle];
        scene
            .graph
            .pair_iter_mut()
            .filter_map(|(handle, node)| {
                node.is_camera().then(|| {
                    let previous = node.as_camera().is_enabled();
                    let is_requested = node.name() == requested;
                    node.as_camera_mut().set_enabled(is_requested);
                    (handle, previous)
                })
            })
            .collect::<Vec<(Handle<_>, bool)>>()
    } else {
        Vec::new()
    };
    // Authored cameras sometimes carry a debug-only render mask while the
    // meshes use the default mask. A capture is an inspection operation, so
    // temporarily make every node visible to the selected camera and restore
    // the serialized values through the same cleanup path.
    let render_masks = if camera.is_some() {
        let scene = &mut engine.scenes[scene_handle];
        scene
            .graph
            .pair_iter_mut()
            .map(|(handle, node)| {
                let previous = *node.render_mask;
                node.render_mask.set_value_and_mark_modified(BitMask::all());
                (handle, previous)
            })
            .collect::<Vec<(Handle<_>, BitMask)>>()
    } else {
        Vec::new()
    };
    let cleanup =
        |engine: &mut fyrox::engine::Engine,
         states: &[(Handle<fyrox::scene::node::Node>, bool)],
         masks: &[(Handle<fyrox::scene::node::Node>, BitMask)],
         previous_render_target: Option<fyrox::resource::texture::TextureResource>| {
            for (handle, enabled) in states {
                if let Ok(node) = engine.scenes[scene_handle].graph.try_get_mut(*handle) {
                    node.as_camera_mut().set_enabled(*enabled);
                }
            }
            for (handle, mask) in masks {
                if let Ok(node) = engine.scenes[scene_handle].graph.try_get_mut(*handle) {
                    node.render_mask.set_value_and_mark_modified(*mask);
                }
            }
            engine.scenes[scene_handle].rendering_options.render_target = previous_render_target;
        };
    let rendered = {
        let scene = &engine.scenes[scene_handle];
        let graphics = engine.graphics_context.as_initialized_mut();
        match graphics.renderer.render_scene(
            scene_handle,
            scene,
            elapsed,
            0.0,
            &engine.resource_manager,
        ) {
            Ok(data) => data,
            Err(error) => {
                cleanup(
                    engine,
                    &camera_states,
                    &render_masks,
                    previous_render_target,
                );
                return json!({"error":format!("camera_render_failed:{error}")});
            }
        }
    };
    let pixels = match rendered
        .scene_data
        .ldr_scene_framebuffer
        .read_pixels(ReadTarget::Color(0))
    {
        Some(pixels) => pixels,
        None => {
            cleanup(
                engine,
                &camera_states,
                &render_masks,
                previous_render_target,
            );
            return json!({"error":"gpu_readback_unavailable"});
        }
    };
    let (width, height) = match rendered.scene_data.ldr_scene_frame_texture().kind() {
        GpuTextureKind::Rectangle { width, height } => (width as u32, height as u32),
        _ => {
            cleanup(
                engine,
                &camera_states,
                &render_masks,
                previous_render_target,
            );
            return json!({"error":"camera_target_not_2d"});
        }
    };
    if width < 64 || height < 64 {
        cleanup(
            engine,
            &camera_states,
            &render_masks,
            previous_render_target,
        );
        return json!({"error":"capture_framebuffer_too_small","width":width,"height":height,"detail":"Windows Editor must have a renderable viewport; minimized/headless 1x1 framebuffer is not valid screenshot evidence"});
    }
    let row_bytes = width as usize * 4;
    let expected_pixel_bytes = row_bytes * height as usize;
    // OpenGL 读回失败时驱动可能返回长度不完整的缓冲区。先验证边界，避免 MCP
    // 请求将 Editor 主线程带入切片 panic；调用方会得到可诊断的 JSON 错误。
    if pixels.len() != expected_pixel_bytes {
        cleanup(
            engine,
            &camera_states,
            &render_masks,
            previous_render_target,
        );
        return json!({
            "error":"gpu_readback_size_mismatch",
            "actual_bytes":pixels.len(),
            "expected_bytes":expected_pixel_bytes
        });
    }
    let mut top_down = vec![0; pixels.len()];
    for row in 0..height as usize {
        let source = row * row_bytes;
        let target = (height as usize - 1 - row) * row_bytes;
        top_down[target..target + row_bytes].copy_from_slice(&pixels[source..source + row_bytes]);
    }
    let mut sampled_colors = HashSet::new();
    let mut min_rgb = [u8::MAX; 3];
    let mut max_rgb = [u8::MIN; 3];
    let mut sampled_pixels = 0usize;
    let mut visible_pixels = 0usize;
    for pixel in top_down.chunks_exact(4).step_by(32) {
        sampled_pixels += 1;
        if pixel[..3].iter().copied().max().unwrap_or(0) > 48 {
            visible_pixels += 1;
        }
        for channel in 0..3 {
            min_rgb[channel] = min_rgb[channel].min(pixel[channel]);
            max_rgb[channel] = max_rgb[channel].max(pixel[channel]);
        }
        sampled_colors.insert([pixel[0], pixel[1], pixel[2]]);
    }
    let pixel_range = [
        max_rgb[0].saturating_sub(min_rgb[0]),
        max_rgb[1].saturating_sub(min_rgb[1]),
        max_rgb[2].saturating_sub(min_rgb[2]),
    ];
    let visible_ratio = visible_pixels as f64 / sampled_pixels.max(1) as f64;
    // Thin editor gizmos can span the RGB range while the scene is otherwise black.
    let nonblank = sampled_colors.len() > 2
        && pixel_range.iter().any(|range| *range >= 8)
        && visible_ratio >= 0.05;
    let mut png = Vec::new();
    if image::codecs::png::PngEncoder::new(Cursor::new(&mut png))
        .write_image(&top_down, width, height, image::ExtendedColorType::Rgba8)
        .is_err()
    {
        cleanup(
            engine,
            &camera_states,
            &render_masks,
            previous_render_target,
        );
        return json!({"error":"png_encode_failed"});
    }
    cleanup(
        engine,
        &camera_states,
        &render_masks,
        previous_render_target,
    );
    let file_name = params
        .get("file_name")
        .or_else(|| params.get("fileName"))
        .and_then(Value::as_str)
        .unwrap_or("screenshot.png");
    let (full_path, relative_path) = match persist_png(file_name, &png) {
        Ok(paths) => paths,
        Err(error) => return json!({"error":format!("screenshot_save_failed:{error}")}),
    };
    let include_image = params
        .get("include_image")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let encoded = include_image.then(|| base64::engine::general_purpose::STANDARD.encode(&png));
    json!({"success":true,"mime_type":"image/png","width":width,"height":height,"path":relative_path,"full_path":full_path,"source":"fyrox_editor_active_scene_ldr_framebuffer","target_size_source":target_size_source,"editor_viewport_size":editor_viewport.map(|(w,h)| json!([w,h])).unwrap_or(Value::Null),"matches_editor_viewport":editor_viewport.is_some_and(|(w,h)| w == width && h == height),"requested_camera":camera,"nonblank":nonblank,"visible_pixel_ratio":visible_ratio,"sampled_colors":sampled_colors.len(),"pixel_range":pixel_range,"base64":encoded})
}

pub fn persist_png(file_name: &str, png: &[u8]) -> Result<(String, String), String> {
    let clean = Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("screenshot.png");
    let clean = if clean.to_ascii_lowercase().ends_with(".png") {
        clean.to_owned()
    } else {
        format!("{clean}.png")
    };
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let dir = root.join("screenshot");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path: PathBuf = dir.join(clean);
    fs::write(&path, png).map_err(|e| e.to_string())?;
    let full_path = path.to_string_lossy().replace('\\', "/");
    Ok((
        full_path,
        format!("screenshot/{}", path.file_name().unwrap().to_string_lossy()),
    ))
}
