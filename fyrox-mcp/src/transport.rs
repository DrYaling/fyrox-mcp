//! Persistent localhost transport host with Unity MCP-compatible handshake.
use crate::{dispatcher::PendingRequest, framing};
use fyrox::core::log::Log;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::Write,
    net::{TcpListener, TcpStream},
    sync::{mpsc, Arc, Mutex},
    thread,
};

const MAX_PENDING_REQUESTS: usize = 256;

pub fn start() -> Arc<Mutex<VecDeque<PendingRequest>>> {
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
    if stream.write_all(framing::HANDSHAKE).is_err() {
        return;
    }
    while let Ok(payload) = framing::read(&mut stream) {
        let Ok(request) = serde_json::from_slice::<Value>(&payload) else {
            break;
        };
        let (sender, receiver) = mpsc::channel();
        let queued = queue.lock().map(|mut value| {
            if value.len() >= MAX_PENDING_REQUESTS {
                false
            } else {
                value.push_back((request, sender));
                true
            }
        });
        if !matches!(queued, Ok(true)) {
            break;
        }
        let Ok(response) = receiver.recv() else { break };
        let Ok(bytes) = serde_json::to_vec(&response) else {
            break;
        };
        if framing::write(&mut stream, &bytes).is_err() {
            break;
        }
    }
}
