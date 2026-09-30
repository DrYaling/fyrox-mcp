//! Editor 资源读取：将访问范围限制为项目 data 目录，并编码可传输贴图。

use base64::Engine;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

/// 枚举 data 目录资源；参数为可选 root，返回受沙箱约束的资源元数据。
pub fn list(params: &Value) -> Value {
    let Some(root) = safe_path(params.get("root").and_then(Value::as_str).unwrap_or("")) else {
        return json!({"error":"invalid_asset_path"});
    };
    let mut resources = Vec::new();
    collect(Path::new("data"), &root, &mut resources);
    json!({"root":root.to_string_lossy().replace('\\', "/"),"resources":resources})
}

/// 读取 data 内贴图；参数为 path，返回 MIME、尺寸和 base64 或错误对象。
pub fn read_texture(params: &Value) -> Value {
    let Some(path) = safe_path(params.get("path").and_then(Value::as_str).unwrap_or("")) else {
        return json!({"error":"invalid_asset_path"});
    };
    let mime_type = match path
        .extension()
        .and_then(|item| item.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "tga" => "image/x-tga",
        _ => return json!({"error":"unsupported_texture_format"}),
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => return json!({"error":format!("texture_read_failed:{error}")}),
    };
    let dimensions = image::load_from_memory(&bytes)
        .ok()
        .map(|image| [image.width(), image.height()]);
    json!({"path":path.to_string_lossy().replace('\\', "/"),"mime_type":mime_type,"dimensions":dimensions,"base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
}

/// 规范化相对资源路径；参数为请求路径，返回 data 内路径或 None。
fn safe_path(requested: &str) -> Option<PathBuf> {
    let relative = Path::new(requested.trim_start_matches(['/', '\\']));
    if relative.is_absolute()
        || relative
            .components()
            .any(|item| matches!(item, Component::ParentDir | Component::Prefix(_)))
    {
        return None;
    }
    let data = PathBuf::from("data");
    Some(if relative.starts_with(&data) {
        relative.to_path_buf()
    } else {
        data.join(relative)
    })
}

/// 递归收集资源；参数为 data 根、当前目录、输出数组，返回无。
fn collect(root: &Path, current: &Path, out: &mut Vec<Value>) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(json!({"path":relative.to_string_lossy().replace('\\', "/"),"extension":path.extension().and_then(|item|item.to_str()).unwrap_or("")}));
        }
    }
}
