//! Main-thread queue model shared by transport and `McpEditorPlugin`.
use serde_json::Value;
use std::sync::{atomic::AtomicBool, mpsc, Arc};

/// A command plus its one-shot correlated response channel. The transport
/// thread never touches `Editor`, `SceneGraph`, resources, or the renderer.
pub struct PendingRequest {
    pub request: Value,
    pub reply: mpsc::Sender<Value>,
    /// Set by the transport when the client deadline expires. The Editor
    /// thread checks it before touching scene/resource state.
    pub canceled: Arc<AtomicBool>,
}
