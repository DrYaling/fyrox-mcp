//! Persistent Unity-MCP framing client used by the stdio MCP server.
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Mutex, OnceLock},
    time::Duration,
};

const MAX_FRAME: usize = 64 * 1024 * 1024;
static CLIENT: OnceLock<Mutex<Option<TcpStream>>> = OnceLock::new();

pub fn call(method: &str, arguments: Value) -> Result<Value, String> {
    let lock = CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = lock
        .lock()
        .map_err(|_| "bridge client lock poisoned".to_owned())?;
    if guard.is_none() {
        *guard = Some(connect()?);
    }
    let payload =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":arguments}))
            .map_err(|e| e.to_string())?;
    for attempt in 0..2 {
        let stream = guard.as_mut().expect("connected");
        if write_frame(stream, &payload).is_ok() {
            if let Ok(bytes) = read_frame(stream) {
                let response: Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Invalid bridge response: {e}"))?;
                let result = response.get("result").cloned().unwrap_or(Value::Null);
                if let Some(error) = result.get("error").and_then(Value::as_str) {
                    return Err(error.to_owned());
                }
                return Ok(result);
            }
        }
        if attempt == 0 {
            *guard = Some(connect()?);
        }
    }
    Err("FWOK bridge connection failed".to_owned())
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
    json!([{"type":"text","text":serde_json::to_string_pretty(result).unwrap_or_else(|_|result.to_string())}])
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    #[test]
    fn screenshot_result_remains_path_metadata() {
        assert_eq!(
            super::content(&json!({"path":"screenshot/camera.png"}))[0]["type"],
            "text"
        );
    }

    #[test]
    fn multiview_result_remains_path_metadata() {
        let value = super::content(&json!({"captures":[
            {"image":{"path":"screenshot/front.png"}},
            {"image":{"path":"screenshot/left.png"}}
        ]}));
        assert_eq!(value.as_array().unwrap().len(), 1);
    }
}
