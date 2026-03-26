//! WebSocket client for communication with the Daystrom app.
//!
//! Connects to the local WebSocket server that Daystrom runs.
//! Messages are sent via a channel from game hooks (sync context) to the async WebSocket task.
//! Reconnects automatically with exponential backoff.

use std::sync::OnceLock;
use std::time::Duration;

use futures_util::SinkExt;
use log::{debug, info, warn};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::TAURI_IDENTIFIER;

// ---- Public API ------------------------------------------------------------

/// Channel sender for outgoing messages, accessible from sync hook code.
static SENDER: OnceLock<mpsc::UnboundedSender<String>> = OnceLock::new();

/// Send a JSON message to Daystrom via WebSocket.
///
/// Safe to call from any thread. Messages are queued if the connection is not yet established.
/// Returns silently if the client has not been initialized or the channel is closed.
pub fn send(msg_type: &str, payload: serde_json::Value) {
    if let Some(tx) = SENDER.get() {
        let msg = serde_json::json!({
            "type": msg_type,
            "payload": payload,
        });
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = tx.send(json);
        }
    }
}

/// Start the WebSocket client in a background thread with its own Tokio runtime.
///
/// Must be called once during mod initialization.
/// The client will keep trying to connect to Daystrom's WebSocket server, reconnecting with backoff on failure.
pub fn init() {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let _ = SENDER.set(tx);

    std::thread::Builder::new()
        .name("ws-client".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime");
            rt.block_on(client_loop(rx));
        })
        .expect("failed to spawn WebSocket client thread");
}

// ---- Port discovery --------------------------------------------------------

/// Resolve the WebSocket port file path.
///
/// Uses the same data directory as Daystrom's WebSocket server.
fn port_file_path() -> Option<std::path::PathBuf> {
    Some(dirs::data_dir()?.join(TAURI_IDENTIFIER).join("ws.port"))
}

/// Read the port number from the discovery file written by Daystrom.
fn read_port() -> Option<u16> {
    let path = port_file_path()?;
    let content = std::fs::read_to_string(path).ok()?;
    content.trim().parse().ok()
}

// ---- Connection loop -------------------------------------------------------

/// Main client loop: connect, send messages, reconnect on failure.
///
/// Runs indefinitely.
/// When the connection drops or Daystrom is not running, waits with exponential backoff (1s to 30s) before retrying.
async fn client_loop(mut rx: mpsc::UnboundedReceiver<String>) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    loop {
        let port = match read_port() {
            Some(p) => p,
            None => {
                debug!(target: "WsClient", "No port file, retrying in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
                continue;
            }
        };

        let url = format!("ws://127.0.0.1:{port}");
        debug!(target: "WsClient", "Connecting to {url}...");

        match tokio_tungstenite::connect_async(&url).await {
            Ok((mut ws, _)) => {
                info!(target: "WsClient", "Connected to Daystrom");
                backoff = Duration::from_secs(1);

                while let Some(msg) = rx.recv().await {
                    if ws.send(Message::text(msg)).await.is_err() {
                        warn!(target: "WsClient", "Connection lost, reconnecting...");
                        break;
                    }
                }
            }
            Err(e) => {
                debug!(
                    target: "WsClient",
                    "Connection failed: {e}, retrying in {backoff:?}"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
}
