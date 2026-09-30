//! Main-thread queue model shared by transport and `McpEditorPlugin`.
use serde_json::Value;
use std::sync::mpsc;

/// A command plus its one-shot correlated response channel. The transport
/// thread never touches `Editor`, `SceneGraph`, resources, or the renderer.
pub type PendingRequest = (Value, mpsc::Sender<Value>);
