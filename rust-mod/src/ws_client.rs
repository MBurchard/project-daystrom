//! WebSocket client for communication with the Daystrom app.
//!
//! Connects to the local WebSocket server that Daystrom runs.
//! Messages are sent via a channel from game hooks (sync context) to the async WebSocket task.
//! Incoming messages (e.g. settings) are dispatched to the appropriate module.
//! Reconnects automatically with exponential backoff.

use std::sync::OnceLock;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::TAURI_IDENTIFIER;

// ---- Message schema --------------------------------------------------------

/// JSON envelope for incoming WebSocket messages.
#[derive(Debug, Deserialize)]
struct WsMessage {
    /// Routing key (e.g. `settings.sync`, `settings.update`).
    #[serde(rename = "type")]
    msg_type: String,
    /// Arbitrary JSON payload.
    payload: serde_json::Value,
}

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

/// Main client loop: connect, send/receive messages, reconnect on failure.
///
/// Runs indefinitely.
/// On connection, requests the current settings from Daystrom.
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
            Ok((ws, _)) => {
                info!(target: "WsClient", "Connected to Daystrom");
                backoff = Duration::from_secs(1);

                let (mut sink, mut source) = ws.split();

                // Request current settings on connection
                let request = serde_json::json!({
                    "type": "settings.request",
                    "payload": {},
                });
                if let Ok(json) = serde_json::to_string(&request)
                    && sink.send(Message::text(json)).await.is_err()
                {
                    warn!(target: "WsClient", "Connection lost during settings request");
                    continue;
                }

                // Bidirectional message loop
                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            match msg {
                                Some(text) => {
                                    if sink.send(Message::text(text)).await.is_err() {
                                        warn!(target: "WsClient",
                                            "Connection lost, reconnecting...");
                                        break;
                                    }
                                }
                                None => return, // channel closed, mod shutting down
                            }
                        }
                        msg = source.next() => {
                            match msg {
                                Some(Ok(Message::Text(text))) => {
                                    handle_incoming(&text);
                                }
                                Some(Ok(_)) => {} // binary, ping, pong
                                Some(Err(e)) => {
                                    warn!(target: "WsClient",
                                        "Receive error: {e}, reconnecting...");
                                    break;
                                }
                                None => {
                                    info!(target: "WsClient",
                                        "Connection closed, reconnecting...");
                                    break;
                                }
                            }
                        }
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

// ---- Incoming message dispatch ---------------------------------------------

/// Process an incoming JSON message from Daystrom.
///
/// Parses the envelope and dispatches by message type.
fn handle_incoming(text: &str) {
    let msg: WsMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!(target: "WsClient", "Invalid message: {e}");
            return;
        }
    };

    match msg.msg_type.as_str() {
        "settings.sync" => match serde_json::from_value(msg.payload) {
            Ok(settings) => crate::settings::apply_sync(settings),
            Err(e) => warn!(target: "WsClient", "Invalid settings.sync payload: {e}"),
        },
        "settings.update" => {
            if let Some(obj) = msg.payload.as_object() {
                for (key, value) in obj {
                    crate::settings::apply_update(key, value);
                }
            }
        }
        other => {
            debug!(target: "WsClient", "Unknown message type: {other}");
        }
    }
}
