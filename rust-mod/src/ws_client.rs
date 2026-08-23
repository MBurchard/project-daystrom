//! WebSocket client for communication with the Daystrom app.
//!
//! Connects to the local WebSocket server that Daystrom runs.
//! Messages are sent via a channel from game hooks (sync context) to the async WebSocket task.
//! Incoming messages (e.g. settings) are dispatched to the appropriate module.
//! Reconnects automatically with exponential backoff.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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

/// Whether the game has completed at least one `ScreenManager` UI frame.
static UI_READY: AtomicBool = AtomicBool::new(false);

/// Unknown, unavailable, or available state of the game UI readiness hook.
static UI_READY_CAPABILITY: AtomicU8 = AtomicU8::new(0);

/// Serialize one protocol message for the WebSocket transport.
///
/// Returns `None` only when the JSON payload cannot be serialized.
fn serialize_message(msg_type: &str, payload: serde_json::Value) -> Option<String> {
    serde_json::to_string(&serde_json::json!({
        "type": msg_type,
        "payload": payload,
    }))
    .ok()
}

/// Build the identity payload used to restore Daystrom launch tracking after reconnecting.
fn client_hello_payload(profile: &str) -> serde_json::Value {
    serde_json::json!({
        "pid": std::process::id(),
        "profile": profile,
    })
}

/// Serialize the persistent game UI readiness announcement.
fn client_ready_message() -> Option<String> {
    serialize_message("client.ready", serde_json::json!({}))
}

/// Serialize the persistent game UI readiness capability announcement.
fn client_capabilities_message(supported: bool) -> Option<String> {
    serialize_message("client.capabilities", serde_json::json!({"uiReady": supported}))
}

/// Send a JSON message to Daystrom via WebSocket.
///
/// Safe to call from any thread. Messages are queued if the connection is not yet established.
/// Returns silently if the client has not been initialized or the channel is closed.
pub fn send(msg_type: &str, payload: serde_json::Value) {
    if let Some(tx) = SENDER.get()
        && let Some(json) = serialize_message(msg_type, payload)
    {
        let _ = tx.send(json);
    }
}

/// Persist and announce that the game completed its first UI frame.
pub fn mark_ui_ready() {
    if !UI_READY.swap(true, Ordering::SeqCst)
        && let Some(tx) = SENDER.get()
        && let Some(ready) = client_ready_message()
    {
        let _ = tx.send(ready);
    }
}

/// Persist and announce whether this game build supports UI readiness observation.
pub fn set_ui_ready_supported(supported: bool) {
    let capability = if supported { 2 } else { 1 };
    UI_READY_CAPABILITY.store(capability, Ordering::SeqCst);
    if let Some(tx) = SENDER.get()
        && let Some(message) = client_capabilities_message(supported)
    {
        let _ = tx.send(message);
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
    read_port_from(&path)
}

/// Read a WebSocket port from a discovery file.
fn read_port_from(path: &std::path::Path) -> Option<u16> {
    let content = std::fs::read_to_string(path).ok()?;
    content.trim().parse().ok()
}

// ---- Connection loop -------------------------------------------------------

/// Main client loop: connect, send/receive messages, reconnect on failure.
///
/// Runs indefinitely.
/// On connection, requests the current settings from Daystrom.
/// When the connection drops or Daystrom is not running, waits with exponential backoff (1s to 5s) before retrying.
async fn client_loop(mut rx: mpsc::UnboundedReceiver<String>) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(5);

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

                // Restore process identity, request settings and re-announce the observed player state.
                let mut initial_messages = Vec::with_capacity(6);
                let profile = std::env::var(crate::profile_protocol::PROFILE_ENV_VAR).unwrap_or_default();
                if !profile.is_empty()
                    && let Some(hello) = serialize_message("client.hello", client_hello_payload(&profile))
                {
                    initial_messages.push(hello);
                }
                let capability = UI_READY_CAPABILITY.load(Ordering::SeqCst);
                if capability != 0
                    && let Some(message) = client_capabilities_message(capability == 2)
                {
                    initial_messages.push(message);
                }
                if UI_READY.load(Ordering::SeqCst)
                    && let Some(ready) = client_ready_message()
                {
                    initial_messages.push(ready);
                }
                if let Some(request) = serialize_message("settings.request", serde_json::json!({})) {
                    initial_messages.push(request);
                }
                initial_messages.extend(
                    crate::game_state::snapshot_updates()
                        .into_iter()
                        .filter_map(|payload| serialize_message("player.update", payload)),
                );

                let mut initial_sync_failed = false;
                for message in initial_messages {
                    if sink.send(Message::text(message)).await.is_err() {
                        initial_sync_failed = true;
                        break;
                    }
                }
                if initial_sync_failed {
                    warn!(target: "WsClient", "Connection lost during initial synchronization");
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test-specific temporary directory for port discovery tests.
    ///
    /// `name` distinguishes tests that the Rust runner executes concurrently.
    fn test_directory(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("daystrom-mod-ws-test-{}-{name}", std::process::id()))
    }

    /// Read a numeric port from the discovery file.
    #[test]
    fn reads_numeric_port_file() {
        let dir = test_directory("numeric");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pointer = dir.join("ws.port");
        std::fs::write(&pointer, "54321").unwrap();

        assert_eq!(read_port_from(&pointer), Some(54321));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Reject a non-numeric discovery port.
    #[test]
    fn rejects_non_numeric_port_file() {
        let dir = test_directory("non-numeric");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pointer = dir.join("ws.port");
        std::fs::write(&pointer, "not-a-port").unwrap();

        assert_eq!(read_port_from(&pointer), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Include the current game PID and inherited profile in the reconnect identity.
    #[test]
    fn client_hello_identifies_daystrom_launch() {
        assert_eq!(
            client_hello_payload("106_Nabor"),
            serde_json::json!({
                "pid": std::process::id(),
                "profile": "106_Nabor",
            })
        );
    }

    #[test]
    fn client_ready_uses_a_connection_lifecycle_message() {
        let message: serde_json::Value = serde_json::from_str(&client_ready_message().unwrap()).unwrap();

        assert_eq!(message["type"], "client.ready");
        assert_eq!(message["payload"], serde_json::json!({}));
    }

    #[test]
    fn client_capabilities_reports_ui_readiness_support() {
        let message: serde_json::Value = serde_json::from_str(&client_capabilities_message(false).unwrap()).unwrap();

        assert_eq!(message["type"], "client.capabilities");
        assert_eq!(message["payload"], serde_json::json!({"uiReady": false}));
    }
}
