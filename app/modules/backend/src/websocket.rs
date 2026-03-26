//! Local WebSocket server for bidirectional communication with the game mod.
//!
//! Daystrom binds a WebSocket server on `127.0.0.1:0` (OS-assigned port) and writes the
//! chosen port to a discovery file (`ws.port`) in the app data directory. The mod reads
//! this file on startup and connects as a client. Multiple mod instances (different
//! profiles) can connect simultaneously.
//!
//! Messages use a JSON envelope: `{"type": "...", "payload": {...}}`.

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

use tauri::Emitter;

use crate::settings;
use crate::use_log;

use_log!("WebSocket");

// ---- Message schema --------------------------------------------------------

/// JSON envelope for all WebSocket messages.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsMessage {
    /// Routing key (e.g. `settings.update`, `game.event`).
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Arbitrary JSON payload.
    pub payload: serde_json::Value,
}

// ---- Server state ----------------------------------------------------------

/// Broadcast channel for outgoing messages (Daystrom -> Mod).
///
/// Any part of the backend can call [`send`] to push a message to all connected clients.
static BROADCAST: std::sync::OnceLock<broadcast::Sender<String>> = std::sync::OnceLock::new();

/// Port file path, stored so we can clean it up on exit.
static PORT_FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Connected clients, keyed by peer address.
///
/// Used for logging and future per-client targeting.
type ClientMap = Arc<Mutex<HashMap<SocketAddr, String>>>;

// ---- Public API ------------------------------------------------------------

/// Send a message to all connected mod instances.
///
/// Silently ignored if the server has not been started yet or no clients are connected.
pub fn send(msg: &WsMessage) {
    if let Some(tx) = BROADCAST.get() {
        match serde_json::to_string(msg) {
            Ok(json) => {
                let _ = tx.send(json);
            }
            Err(e) => log_error!("Failed to serialise WsMessage: {e}"),
        }
    }
}

/// Start the WebSocket server in the Tauri async runtime.
///
/// Writes the port to `ws.port` in the app data directory, then accepts connections
/// indefinitely. Safe to call multiple times; further calls are no-ops.
pub fn start(app: tauri::AppHandle) {
    if BROADCAST.get().is_some() {
        log_debug!("WebSocket server already running");
        return;
    }

    let (tx, _) = broadcast::channel::<String>(64);
    let _ = BROADCAST.set(tx.clone());

    tauri::async_runtime::spawn(run_server(app, tx));
    tauri::async_runtime::spawn(forward_settings_events());
}

/// Remove the port discovery file.
///
/// Called during app shutdown so stale port files don't confuse the mod.
pub fn cleanup() {
    if let Some(path) = PORT_FILE.get() {
        if path.exists() {
            let _ = fs::remove_file(path);
            log_debug!("Removed port file {}", path.display());
        }
    }
}

// ---- Settings bridge -------------------------------------------------------

/// Forward settings change events to all connected mod instances.
///
/// Subscribes to [`settings::subscribe`] and translates each [`SettingsEvent`] into a WebSocket
/// `settings.update` message using the event's self-describing key/value pair.
/// Runs indefinitely as a background task. Does not need to be modified when new settings are added.
async fn forward_settings_events() {
    let mut rx = settings::subscribe();
    while let Ok(event) = rx.recv().await {
        let mut payload = serde_json::Map::new();
        payload.insert(event.key().to_string(), event.value());
        send(&WsMessage {
            msg_type: "settings.update".to_string(),
            payload: serde_json::Value::Object(payload),
        });
    }
}

// ---- Server implementation -------------------------------------------------

/// Resolve the app data directory (same as settings.rs).
fn data_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(env!("TAURI_IDENTIFIER")))
}

/// Write the port number to the discovery file.
fn write_port_file(port: u16) -> Result<PathBuf, String> {
    let dir = data_dir().ok_or_else(|| "Could not resolve app data directory".to_string())?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create data directory: {e}"))?;
    let path = dir.join("ws.port");
    fs::write(&path, port.to_string())
        .map_err(|e| format!("Failed to write port file: {e}"))?;
    Ok(path)
}

/// Main server loop: bind, write a port file, accept connections.
async fn run_server(app: tauri::AppHandle, tx: broadcast::Sender<String>) {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            log_error!("Failed to bind WebSocket server: {e}");
            return;
        }
    };

    let addr = match listener.local_addr() {
        Ok(a) => a,
        Err(e) => {
            log_error!("Failed to get local address: {e}");
            return;
        }
    };

    match write_port_file(addr.port()) {
        Ok(path) => {
            let _ = PORT_FILE.set(path);
            log_info!("Server listening on {addr}");
        }
        Err(e) => {
            log_error!("Port file: {e}");
            return;
        }
    }

    let clients: ClientMap = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                log_error!("Accept failed: {e}");
                continue;
            }
        };

        let tx = tx.clone();
        let app = app.clone();
        let clients = clients.clone();

        tauri::async_runtime::spawn(handle_client(stream, peer, tx, app, clients));
    }
}

/// Handle a single WebSocket client connection.
async fn handle_client(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    tx: broadcast::Sender<String>,
    app: tauri::AppHandle,
    clients: ClientMap,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            log_error!("Handshake failed ({peer}): {e}");
            return;
        }
    };

    log_info!("Client connected: {peer}");
    clients.lock().await.insert(peer, String::new());

    let (mut sink, mut source) = ws_stream.split();
    let mut rx = tx.subscribe();

    // Incoming: mod -> Daystrom
    let app_clone = app.clone();
    let recv_task = tauri::async_runtime::spawn(async move {
        while let Some(Ok(msg)) = source.next().await {
            if let Message::Text(text) = msg {
                handle_incoming(&app_clone, &text);
            }
        }
    });

    // Outgoing: Daystrom -> mod (via a broadcast channel)
    let send_task = tauri::async_runtime::spawn(async move {
        while let Ok(text) = rx.recv().await {
            if sink.send(Message::text(text)).await.is_err() {
                break;
            }
        }
    });

    let _ = tokio::join!(recv_task, send_task);

    clients.lock().await.remove(&peer);
    log_info!("Client disconnected: {peer}");
}

/// Process an incoming JSON message from the mod.
///
/// Some message types (e.g. `settings.request`) are handled directly; all others are emitted as
/// Tauri events, so other backend modules and the frontend can react.
fn handle_incoming(app: &tauri::AppHandle, text: &str) {
    let msg: WsMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            log_error!("Invalid message: {e}");
            return;
        }
    };

    log_debug!("Received: type={} payload={}", msg.msg_type, msg.payload);

    // Settings request: respond with current game settings as a full sync
    if msg.msg_type == "settings.request" {
        match serde_json::to_value(settings::get_game_settings()) {
            Ok(payload) => send(&WsMessage {
                msg_type: "settings.sync".to_string(),
                payload,
            }),
            Err(e) => log_error!("Failed to serialise settings: {e}"),
        }
        return;
    }

    // Everything else: emit as Tauri event
    let event_name = format!("ws:{}", msg.msg_type);
    let _ = app.emit(&event_name, msg.payload);
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- WsMessage serialisation --

    #[test]
    fn message_serialise_uses_type_field() {
        let msg = WsMessage {
            msg_type: "settings.update".to_string(),
            payload: serde_json::json!({"ui.scale": 0.8}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"settings.update""#));
        assert!(json.contains(r#""ui.scale":0.8"#));
    }

    #[test]
    fn message_deserialise_type_field() {
        let json = r#"{"type":"game.event","payload":{"event":"sync_complete"}}"#;
        let msg: WsMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "game.event");
        assert_eq!(msg.payload["event"], "sync_complete");
    }

    #[test]
    fn message_roundtrip() {
        let msg = WsMessage {
            msg_type: "test.ping".to_string(),
            payload: serde_json::json!({"value": 42, "nested": {"a": true}}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.msg_type, "test.ping");
        assert_eq!(parsed.payload["value"], 42);
        assert_eq!(parsed.payload["nested"]["a"], true);
    }

    #[test]
    fn message_missing_type_fails() {
        let json = r#"{"payload":{"value":1}}"#;
        assert!(serde_json::from_str::<WsMessage>(json).is_err());
    }

    // -- Port file --

    #[test]
    fn port_file_write_and_read() {
        let dir = std::env::temp_dir().join("daystrom-ws-test-port");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("ws.port");
        fs::write(&path, 54321_u16.to_string()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let port: u16 = content.parse().unwrap();
        assert_eq!(port, 54321);

        let _ = fs::remove_dir_all(&dir);
    }

    // -- Integration: server + client exchange --

    #[tokio::test]
    async fn client_connects_and_receives_broadcast() {
        // Start a minimal server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, _) = broadcast::channel::<String>(16);
        let tx_server = tx.clone();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            let (mut sink, _source) = ws.split();
            let mut rx = tx_server.subscribe();

            // Forward one broadcast message to the client
            if let Ok(text) = rx.recv().await {
                let _ = sink.send(Message::text(text)).await;
            }
        });

        // Connect client
        let url = format!("ws://{addr}");
        let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let (_sink, mut source) = ws.split();

        // Broadcast a message (simulates websocket::send)
        let msg = WsMessage {
            msg_type: "settings.update".to_string(),
            payload: serde_json::json!({"ui.scale": 0.8}),
        };
        tx.send(serde_json::to_string(&msg).unwrap()).unwrap();

        // Client should receive it
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            source.next(),
        )
        .await
        .expect("timeout waiting for message")
        .unwrap()
        .unwrap();

        if let Message::Text(text) = received {
            let parsed: WsMessage = serde_json::from_str(&text).unwrap();
            assert_eq!(parsed.msg_type, "settings.update");
            assert_eq!(parsed.payload["ui.scale"], 0.8);
        } else {
            panic!("Expected text message");
        }

        server.abort();
    }

    #[tokio::test]
    async fn client_sends_message_to_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Server: accept one message and return it via a oneshot channel
        let (result_tx, result_rx) = tokio::sync::oneshot::channel::<String>();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            let (_sink, mut source) = ws.split();

            if let Some(Ok(Message::Text(text))) = source.next().await {
                let _ = result_tx.send(text.to_string());
            }
        });

        // Client sends a message
        let url = format!("ws://{addr}");
        let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let (mut sink, _source) = ws.split();

        let msg = WsMessage {
            msg_type: "game.event".to_string(),
            payload: serde_json::json!({"resources": {"parsteel": 1000}}),
        };
        sink.send(Message::text(serde_json::to_string(&msg).unwrap()))
            .await
            .unwrap();

        // Verify the server received it
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            result_rx,
        )
        .await
        .expect("timeout")
        .unwrap();

        let parsed: WsMessage = serde_json::from_str(&received).unwrap();
        assert_eq!(parsed.msg_type, "game.event");
        assert_eq!(parsed.payload["resources"]["parsteel"], 1000);

        server.abort();
    }
}
