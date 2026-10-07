//! Persistent Unity-MCP framing client used by the stdio MCP server.
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpStream,
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

const MAX_FRAME: usize = 64 * 1024 * 1024;
const MAX_REQUEST_RETRIES: usize = 2;
static CLIENT: OnceLock<Mutex<Option<TcpStream>>> = OnceLock::new();
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub fn call(method: &str, arguments: Value) -> Result<Value, String> {
    if method == "wait_for_resource" {
        return wait_for_resource(&arguments);
    }
    if method == "preview_resource" {
        return preview_resource(&arguments);
    }
    // Loading a scene/UI is asynchronous inside Fyrox.  Offer an opt-in
    // synchronous envelope so callers do not have to guess how long to sleep
    // before querying the newly selected resource.
    if matches!(method, "manage_scene" | "manage_ui")
        && arguments
            .get("action")
            .and_then(Value::as_str)
            .is_some_and(|action| matches!(action, "load" | "load_scene"))
        && arguments
            .get("wait")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or("load wait requires path")?;
        let kind = if method == "manage_ui" { "ui" } else { "rgs" };
        let queued = call_editor(method, arguments.clone())?;
        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(15_000)
            .clamp(500, 120_000);
        wait_for_active(path, kind, timeout_ms)?;
        return Ok(json!({"queued":queued,"ready":true,"active_path":path,"active_kind":kind}));
    }
    call_editor(method, arguments)
}

fn wait_for_resource(arguments: &Value) -> Result<Value, String> {
    let path = arguments
        .get("path")
        .and_then(Value::as_str)
        .ok_or("wait_for_resource requires path")?;
    let path = path.replace('\\', "/");
    let resource = Path::new(&path);
    if resource.is_absolute()
        || !resource.starts_with("data")
        || resource
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("wait_for_resource path must be below data".into());
    }
    let kind = arguments
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_else(|| if path.ends_with(".ui") { "ui" } else { "rgs" });
    if !matches!(kind, "ui" | "rgs") {
        return Err("wait_for_resource kind must be ui or rgs".into());
    }
    let extension = resource.extension().and_then(|value| value.to_str());
    if extension != Some(kind) {
        return Err("wait_for_resource kind must match the .ui or .rgs path extension".into());
    }
    let timeout_ms = arguments
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(15_000)
        .clamp(500, 120_000);
    wait_for_active(&path, kind, timeout_ms)?;
    let current = call_editor("project_info", json!({}))?;
    Ok(json!({"ready":true,"path":path,"kind":kind,"timeout_ms":timeout_ms,"project":current}))
}

fn wait_for_active(path: &str, kind: &str, timeout_ms: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let current = call_editor("project_info", json!({}))?;
        if matches_active(&current, path, kind) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "resource did not become active within {timeout_ms}ms: {path}; editor={current}"
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn call_editor(method: &str, arguments: Value) -> Result<Value, String> {
    let lock = CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = lock
        .lock()
        .map_err(|_| "bridge client lock poisoned".to_owned())?;
    if guard.is_none() {
        *guard = Some(connect()?);
    }
    let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let payload = serde_json::to_vec(
        &json!({"jsonrpc":"2.0","id":request_id,"method":method,"params":arguments}),
    )
    .map_err(|e| e.to_string())?;
    for attempt in 0..MAX_REQUEST_RETRIES {
        let mut failed = false;
        if let Some(stream) = guard.as_mut() {
            if write_frame(stream, &payload).is_ok() {
                if let Ok(bytes) = read_frame(stream) {
                    let response: Value = serde_json::from_slice(&bytes)
                        .map_err(|e| format!("Invalid bridge response: {e}"))?;
                    if response.get("id").and_then(Value::as_u64) != Some(request_id) {
                        return Err(format!(
                            "bridge response id mismatch: expected {request_id}, got {}",
                            response.get("id").unwrap_or(&Value::Null)
                        ));
                    }
                    if let Some(error) = response.get("error") {
                        return Err(format!("editor bridge JSON-RPC error: {error}"));
                    }
                    let result = response.get("result").cloned().unwrap_or(Value::Null);
                    if let Some(error) = result.get("error").and_then(Value::as_str) {
                        return Err(error.to_owned());
                    }
                    return Ok(result);
                }
            }
            failed = true;
        }
        if failed {
            *guard = None;
        }
        if attempt + 1 < MAX_REQUEST_RETRIES {
            *guard = Some(connect()?);
        }
    }
    Err("FWOK bridge connection failed".to_owned())
}

fn preview_resource(arguments: &Value) -> Result<Value, String> {
    let raw = arguments
        .get("path")
        .and_then(Value::as_str)
        .ok_or("preview_resource requires path")?;
    let path = raw.replace('\\', "/");
    let resource = Path::new(&path);
    if resource.is_absolute()
        || !resource.starts_with("data")
        || resource
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("preview path must be below data".into());
    }
    let kind = match resource
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("ui") => "ui",
        Some("rgs") => "rgs",
        _ => return Err("preview path must end in .ui or .rgs".into()),
    };
    let requested_width = arguments.get("width").and_then(Value::as_u64);
    let requested_height = arguments.get("height").and_then(Value::as_u64);
    if requested_width.is_some() != requested_height.is_some() {
        return Err("preview dimensions require both width and height".into());
    }
    if requested_width.is_some_and(|value| !(64..=4096).contains(&value))
        || requested_height.is_some_and(|value| !(64..=4096).contains(&value))
    {
        return Err("preview dimensions must be 64..4096".into());
    }
    let file_name = arguments
        .get("file_name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "preview-{}.png",
                resource.file_stem().unwrap().to_string_lossy()
            )
        });
    let current = call_editor("project_info", json!({}))?;
    if let Some(root) = current["working_directory"].as_str() {
        let candidate = Path::new(root).join(resource);
        if !candidate.is_file() {
            return Err(format!("preview resource does not exist: {path}"));
        }
    }
    if !matches_active(&current, &path, kind) {
        let load = if kind == "ui" {
            call(
                "manage_ui",
                json!({"action":"load","path":path,"wait":true,"timeout_ms":15_000}),
            )?
        } else {
            call(
                "manage_scene",
                json!({"action":"load","path":path,"wait":true,"timeout_ms":15_000}),
            )?
        };
        if load.get("error").is_some() {
            return Err(format!("resource load rejected: {load}"));
        }
    }
    if kind == "ui" {
        let width = requested_width.unwrap_or(1200);
        let height = requested_height.unwrap_or(760);
        let mut layout_params = json!({"action":"validate_layout","width":width,"height":height});
        if let Some(expected) = arguments.get("expected_names") {
            layout_params["expected_names"] = expected.clone();
        }
        let layout = call_editor("manage_ui", layout_params)?;
        let image = call_editor(
            "manage_ui",
            json!({"action":"render_ui","width":width,"height":height,"file_name":file_name}),
        )?;
        Ok(
            json!({"success":layout["success"] == true && image["success"] == true,"path":path,"kind":kind,"layout":layout,"image":image}),
        )
    } else {
        let stats = call_editor("scene_stats", json!({"scene_path":path}))?;
        let cameras = call_editor("scene_cameras", json!({"scene_path":path}))?;
        let include_image = arguments
            .get("include_image")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut capture_params =
            json!({"file_name":file_name,"scene_path":path,"include_image":include_image});
        if let (Some(width), Some(height)) = (requested_width, requested_height) {
            capture_params["width"] = json!(width);
            capture_params["height"] = json!(height);
        }
        let camera =
            select_preview_camera(&cameras, arguments.get("camera").and_then(Value::as_str))?;
        capture_params["camera"] = json!(camera);
        let requested_image = call_editor("screenshot", capture_params.clone())?;
        let mut image = requested_image.clone();
        let mut fallback_camera = None;
        let node_count = stats["node_count"].as_u64().unwrap_or(0);
        let camera_count = cameras["cameras"].as_array().map_or(0, Vec::len);
        let mut issues = Vec::new();
        if node_count <= 1 {
            issues.push("scene_has_no_content");
        }
        if camera_count == 0 {
            issues.push("scene_has_no_camera");
        }
        if image["nonblank"] != true {
            // The serialized scene may contain a gameplay camera whose render
            // mask or transform is still being authored. Keep the validation
            // failure visible, but provide a useful EditorCamera preview so a
            // layout review can continue in the same MCP call.
            if camera != "EditorCamera" {
                let mut fallback_params = capture_params.clone();
                fallback_params["camera"] = json!("EditorCamera");
                let fallback_name = Path::new(&file_name)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|stem| format!("{stem}-editor-fallback.png"))
                    .unwrap_or_else(|| "editor-camera-fallback.png".to_owned());
                fallback_params["file_name"] = json!(fallback_name);
                let fallback = call_editor("screenshot", fallback_params)?;
                if fallback["nonblank"] == true {
                    image = fallback;
                    fallback_camera = Some("EditorCamera");
                    issues.push("requested_camera_blank_or_too_dark");
                    issues.push("editor_camera_fallback_used");
                } else {
                    issues.push("render_appears_blank_or_too_dark");
                }
            } else {
                issues.push("render_appears_blank_or_too_dark");
            }
        }
        Ok(
            json!({"success":issues.is_empty() && image["success"] == true,"path":path,"kind":kind,"selected_camera":camera,"fallback_camera":fallback_camera,"stats":stats,"cameras":cameras,"requested_image":requested_image,"image":image,"issues":issues}),
        )
    }
}

fn select_preview_camera<'a>(
    cameras: &'a Value,
    requested: Option<&'a str>,
) -> Result<&'a str, String> {
    let entries = cameras["cameras"]
        .as_array()
        .ok_or("scene camera list unavailable")?;
    let active = entries.iter().filter(|camera| camera["enabled"] == true);
    if let Some(name) = requested {
        return active
            .filter_map(|camera| camera["name"].as_str())
            .find(|candidate| *candidate == name)
            .ok_or_else(|| format!("preview camera is missing or disabled: {name}"));
    }
    active
        .filter_map(|camera| camera["name"].as_str())
        .find(|name| *name != "EditorCamera")
        .ok_or("scene has no enabled content camera; specify a camera or enable one".into())
}

fn matches_active(info: &Value, path: &str, kind: &str) -> bool {
    if info["active_kind"].as_str() != Some(kind) {
        return false;
    }
    let Some(active) = info["active_path"].as_str() else {
        return false;
    };
    let active = active.replace('\\', "/");
    active == path || active.ends_with(&format!("/{path}"))
}

fn connect() -> Result<TcpStream, String> {
    let mut stream =
        TcpStream::connect_timeout(&"127.0.0.1:6501".parse().unwrap(), Duration::from_secs(2))
            .map_err(|e| format!("FWOK bridge unavailable: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    let mut handshake = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream
            .read_exact(&mut byte)
            .map_err(|e| format!("bridge handshake failed: {e}"))?;
        handshake.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
        if handshake.len() > 64 {
            return Err("bridge handshake too long".into());
        }
    }
    if handshake != b"WELCOME UNITY-MCP 1 FRAMING=1\n" {
        return Err(format!(
            "unexpected bridge handshake: {}",
            String::from_utf8_lossy(&handshake)
        ));
    }
    Ok(stream)
}

fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> Result<(), ()> {
    if payload.is_empty() || payload.len() > MAX_FRAME {
        return Err(());
    }
    stream
        .write_all(&(payload.len() as u64).to_be_bytes())
        .map_err(|_| ())?;
    stream.write_all(payload).map_err(|_| ())
}
fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, ()> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).map_err(|_| ())?;
    let len = u64::from_be_bytes(header);
    if len == 0 || len > MAX_FRAME as u64 {
        return Err(());
    }
    let mut payload = vec![0; len as usize];
    stream.read_exact(&mut payload).map_err(|_| ())?;
    Ok(payload)
}

pub fn content(result: &Value) -> Value {
    if let (Some(data), Some(mime_type)) = (
        result.get("base64").and_then(Value::as_str),
        result.get("mime_type").and_then(Value::as_str),
    ) {
        if mime_type.starts_with("image/") && !data.is_empty() {
            let mut metadata = result.clone();
            if let Some(object) = metadata.as_object_mut() {
                object.remove("base64");
            }
            return json!([
                {"type":"image","data":data,"mimeType":mime_type},
                {"type":"text","text":serde_json::to_string_pretty(&metadata).unwrap_or_else(|_| metadata.to_string())}
            ]);
        }
    }
    if let (Some(data), Some(mime_type)) = (
        result.get("base64").and_then(Value::as_str),
        result.get("mime_type").and_then(Value::as_str),
    ) {
        if mime_type.starts_with("image/") && !data.is_empty() {
            return json!([{"type":"image","data":data,"mimeType":mime_type}]);
        }
    }
    json!([{"type":"text","text":serde_json::to_string_pretty(result).unwrap_or_else(|_|result.to_string())}])
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    #[test]
    fn preview_rejects_paths_outside_serialized_data() {
        assert!(super::preview_resource(&json!({"path":"../outside.ui"})).is_err());
        assert!(super::preview_resource(&json!({"path":"data/notes.txt"})).is_err());
    }

    #[test]
    fn active_resource_matches_relative_or_absolute_editor_path() {
        assert!(super::matches_active(
            &json!({"active_kind":"ui","active_path":"F:/project/data/realm.ui"}),
            "data/realm.ui",
            "ui"
        ));
        assert!(!super::matches_active(
            &json!({"active_kind":"rgs","active_path":"data/realm.rgs"}),
            "data/realm.ui",
            "ui"
        ));
    }
    #[test]
    fn screenshot_result_remains_path_metadata() {
        assert_eq!(
            super::content(&json!({"path":"screenshot/camera.png"}))[0]["type"],
            "text"
        );
    }

    #[test]
    fn preview_prefers_enabled_scene_camera() {
        let cameras = json!({"cameras":[{"name":"EditorCamera","enabled":true},{"name":"Disabled","enabled":false},{"name":"Playable","enabled":true}]});
        assert_eq!(
            super::select_preview_camera(&cameras, None).unwrap(),
            "Playable"
        );
        assert!(super::select_preview_camera(&cameras, Some("Disabled")).is_err());
    }

    #[test]
    fn multiview_result_remains_path_metadata() {
        let value = super::content(&json!({"captures":[
            {"image":{"path":"screenshot/front.png"}},
            {"image":{"path":"screenshot/left.png"}}
        ]}));
        assert_eq!(value.as_array().unwrap().len(), 1);
    }

    #[test]
    fn texture_result_exposes_mcp_image_content() {
        let value = super::content(&json!({
            "path":"textures/test.png",
            "mime_type":"image/png",
            "base64":"aGVsbG8="
        }));
        assert_eq!(value[0]["type"], "image");
        assert_eq!(value[0]["mimeType"], "image/png");
        assert_eq!(value[1]["type"], "text");
        assert!(!value[1]["text"].as_str().unwrap().contains("base64"));
    }
}
