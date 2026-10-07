//! Persistent localhost transport host with Unity MCP-compatible handshake.
use crate::{dispatcher::PendingRequest, framing};
use fyrox::core::log::Log;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::Write,
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

const MAX_PENDING_REQUESTS: usize = 256;
static QUEUE_DEPTH: AtomicUsize = AtomicUsize::new(0);
static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static REJECTED: AtomicU64 = AtomicU64::new(0);
static STARTED_AT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn diagnostics() -> serde_json::Value {
    serde_json::json!({
        "queue_depth": QUEUE_DEPTH.load(Ordering::Relaxed),
        "queue_max": MAX_PENDING_REQUESTS,
        "accepted": ACCEPTED.load(Ordering::Relaxed),
        "rejected": REJECTED.load(Ordering::Relaxed),
        "uptime_ms": STARTED_AT.get().map(|at| at.elapsed().as_millis() as u64).unwrap_or(0)
    })
}

pub fn start() -> Arc<Mutex<VecDeque<PendingRequest>>> {
    let _ = STARTED_AT.set(std::time::Instant::now());
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let listener_queue = queue.clone();
    thread::spawn(move || {
        let listener = match TcpListener::bind("127.0.0.1:6501") {
            Ok(value) => value,
            Err(error) => {
                Log::err(format!("[MCP] bridge bind failed: {error}"));
                return;
            }
        };
        Log::info("[MCP] framed bridge listening on 127.0.0.1:6501".to_owned());
        for stream in listener.incoming().flatten() {
            let queue = listener_queue.clone();
            thread::spawn(move || serve(stream, queue));
        }
    });
    queue
}

fn serve(mut stream: TcpStream, queue: Arc<Mutex<VecDeque<PendingRequest>>>) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    if stream.write_all(framing::HANDSHAKE).is_err() {
        return;
    }
    while let Ok(payload) = framing::read(&mut stream) {
        let Ok(request) = serde_json::from_slice::<Value>(&payload) else {
            break;
        };
        let request_id = request.get("id").cloned().unwrap_or(Value::Null);
        let (sender, receiver) = mpsc::channel();
        let canceled = Arc::new(AtomicBool::new(false));
        let queued = queue.lock().map(|mut value| {
            if value.len() >= MAX_PENDING_REQUESTS {
                REJECTED.fetch_add(1, Ordering::Relaxed);
                false
            } else {
                value.push_back(PendingRequest {
                    request,
                    reply: sender,
                    canceled: canceled.clone(),
                });
                QUEUE_DEPTH.store(value.len(), Ordering::Relaxed);
                ACCEPTED.fetch_add(1, Ordering::Relaxed);
                true
            }
        });
        if !matches!(queued, Ok(true)) {
            let response = crate::response::success(
                request_id.clone(),
                serde_json::json!({
                    "error":"editor_mcp_queue_full",
                    "retryable":true,
                    "max_pending":MAX_PENDING_REQUESTS
                }),
            );
            if let Ok(bytes) = serde_json::to_vec(&response) {
                let _ = framing::write(&mut stream, &bytes);
            }
            // Keep the session alive.  A temporary burst should be reported as
            // retryable without forcing the MCP client to recreate its socket.
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        let Ok(response) = receiver.recv_timeout(Duration::from_secs(120)) else {
            canceled.store(true, Ordering::Release);
            let response = crate::response::success(
                request_id,
                serde_json::json!({"error":"editor_mcp_request_timeout","retryable":true}),
            );
            if let Ok(bytes) = serde_json::to_vec(&response) {
                let _ = framing::write(&mut stream, &bytes);
            }
            continue;
        };
        let Ok(bytes) = serde_json::to_vec(&response) else {
            break;
        };
        if framing::write(&mut stream, &bytes).is_err() {
            break;
        }
    }
}

pub fn set_queue_depth(queue: &Arc<Mutex<VecDeque<PendingRequest>>>) {
    if let Ok(value) = queue.lock() {
        QUEUE_DEPTH.store(value.len(), Ordering::Relaxed);
    }
}
