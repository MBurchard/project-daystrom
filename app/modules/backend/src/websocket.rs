//! Local WebSocket server for bidirectional communication with the game mod.
//!
//! Daystrom binds a WebSocket server on `127.0.0.1:0` (OS-assigned port) and writes the chosen port to a discovery
//! file (`ws.port`) in the app data directory.
//! The mod reads this file on startup and connects as a client.
//! Multiple mod instances (different profiles) can connect simultaneously.
//!
//! Messages use a JSON envelope: `{"type": "...", "payload": {...}}`.

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, broadcast};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

use tauri::Emitter;

use crate::settings;
use crate::use_log;

use_log!("WebSocket");

/// How often the backend repairs a missing port discovery file.
const PORT_FILE_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

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

/// Identity announced by a Daystrom-injected mod after each WebSocket connection.
#[derive(Debug, Deserialize, PartialEq)]
struct ClientHello {
    /// Operating-system process identifier of the game.
    pid: u32,
    /// Profile stem inherited through `DAYSTROM_PROFILE`.
    profile: String,
}

// ---- Server state ----------------------------------------------------------

/// Broadcast channel for outgoing messages (Daystrom -> Mod).
///
/// Any part of the backend can call [`send`] to push a message to all connected clients.
static BROADCAST: std::sync::OnceLock<broadcast::Sender<String>> = std::sync::OnceLock::new();

/// Monotonic identifier used to make reconnect clean-up connection-specific.
static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

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
            Err(e) => log_error!("Failed to serialize WsMessage: {e}"),
        }
    }
}

/// Start the WebSocket server in the Tauri async runtime.
///
/// Writes the port to `ws.port` in the app data directory, then accepts connections indefinitely.
/// Safe to call multiple times, further calls are no-ops once the server has bound successfully.
pub fn start(app: tauri::AppHandle) {
    if BROADCAST.get().is_some() {
        log_debug!("WebSocket server already running");
        return;
    }

    tauri::async_runtime::spawn(run_server(app));
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

/// Write the current WebSocket port to the shared discovery file.
///
/// The file deliberately remains after shutdown. A stopped backend simply refuses the connection,
/// while a replacement backend overwrites the file with its new port. Avoiding shutdown deletion
/// prevents an old process from removing a newer backend's discovery information.
fn write_port_file(port: u16) -> Result<PathBuf, String> {
    let dir = data_dir().ok_or_else(|| "Could not resolve app data directory".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create data directory: {e}"))?;
    let path = dir.join("ws.port");
    fs::write(&path, port.to_string()).map_err(|e| format!("Failed to write port file: {e}"))?;
    Ok(path)
}

/// Restore the discovery file when an older backend removed it during an overlapping restart.
fn restore_missing_port_file(path: &std::path::Path, port: u16) -> Result<bool, String> {
    match fs::metadata(path) {
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to inspect port file: {error}")),
    }
    fs::write(path, port.to_string()).map_err(|e| format!("Failed to write port file: {e}"))?;
    Ok(true)
}

/// Keep the discovery file available when an older backend exits during a development restart.
async fn maintain_port_file(path: PathBuf, port: u16) {
    loop {
        tokio::time::sleep(PORT_FILE_REFRESH_INTERVAL).await;
        match restore_missing_port_file(&path, port) {
            Ok(true) => log_debug!("Restored WebSocket port file {}", path.display()),
            Ok(false) => {}
            Err(error) => log_warn!("Failed to refresh WebSocket port file: {error}"),
        }
    }
}

/// Main server loop: bind, write a port file, accept connections.
///
/// Creates the broadcast channel and registers it in [`BROADCAST`] only after the listener has bound successfully.
/// This ensures that a transient bind failure does not permanently block retries via the `BROADCAST.get().is_some()`
/// guard in [`start`].
async fn run_server(app: tauri::AppHandle) {
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
            log_debug!("Wrote WebSocket port file {}", path.display());
            if tauri::is_dev() {
                tauri::async_runtime::spawn(maintain_port_file(path, addr.port()));
            }
            log_info!("Server listening on {addr}");
        }
        Err(e) => {
            log_error!("Port file: {e}");
            return;
        }
    }

    // Publish broadcast channel only after successful startup so the guard in start() stays open on failure, allowing
    // a retry.
    let (tx, _) = broadcast::channel::<String>(64);
    let _ = BROADCAST.set(tx.clone());

    tauri::async_runtime::spawn(forward_settings_events());

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
        tauri::async_runtime::spawn(handle_client(stream, peer, tx, app));
    }
}

/// Handle a single WebSocket client connection.
async fn handle_client(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    tx: broadcast::Sender<String>,
    app: tauri::AppHandle,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            log_error!("Handshake failed ({peer}): {e}");
            return;
        }
    };

    log_info!("Client connected: {peer}");
    let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);

    let (mut sink, mut source) = ws_stream.split();
    let mut rx = tx.subscribe();
    let client_pid = Arc::new(Mutex::new(None::<u32>));

    // Incoming: mod -> Daystrom
    let app_clone = app.clone();
    let recv_client_pid = client_pid.clone();
    let recv_task = async move {
        while let Some(Ok(msg)) = source.next().await {
            if let Message::Text(text) = msg
                && let Some(hello) = handle_incoming(&app_clone, &text)
            {
                let mut registered_pid = recv_client_pid.lock().await;
                if registered_pid.is_some_and(|pid| pid != hello.pid) {
                    log_warn!("Client {peer} attempted to change its game PID");
                    continue;
                }
                crate::process_origin::register_reconnected_launch(hello.pid, hello.profile.clone(), connection_id);
                crate::game_state::update(&app_clone, |state| {
                    state.game_started_by_us = true;
                });
                let running_profiles = crate::process_origin::running_profiles();
                crate::profile_state::update(&app_clone, |state| {
                    state.running_profiles = running_profiles;
                    state.external_game_running = false;
                    state.game_origin_pending = false;
                    state.mod_connection_missing = false;
                });
                *registered_pid = Some(hello.pid);
                log_info!("Restored Daystrom game tracking: PID {}, profile {}", hello.pid, hello.profile);
            }
        }
    };

    // Outgoing: Daystrom -> mod (via a broadcast channel)
    let send_task = async move {
        while let Ok(text) = rx.recv().await {
            if sink.send(Message::text(text)).await.is_err() {
                break;
            }
        }
    };

    tokio::pin!(recv_task, send_task);
    tokio::select! {
        () = &mut recv_task => {}
        () = &mut send_task => {}
    }

    if let Some(pid) = *client_pid.lock().await
        && crate::process_origin::unregister_reconnected_launch(pid, connection_id)
        && !crate::process_origin::has_tracked_game()
    {
        crate::process_origin::clear_game_started();
        crate::game_state::update(&app, |state| {
            state.game_started_by_us = false;
        });
    }
    log_info!("Client disconnected: {peer}");
}

/// Parse and validate a mod reconnect identity.
fn parse_client_hello(payload: serde_json::Value) -> Result<ClientHello, String> {
    let hello: ClientHello = serde_json::from_value(payload).map_err(|error| error.to_string())?;
    if hello.pid == 0 {
        return Err("game PID must not be zero".to_string());
    }
    if hello.profile.trim().is_empty() || hello.profile.len() > 255 {
        return Err("game profile must contain between 1 and 255 bytes".to_string());
    }
    Ok(hello)
}

/// Process an incoming JSON message from the mod.
///
/// Some message types (e.g. `settings.request`) are handled directly, all others are emitted as Tauri events,
/// so other backend modules and the frontend can react.
fn handle_incoming(app: &tauri::AppHandle, text: &str) -> Option<ClientHello> {
    let msg: WsMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            log_error!("Invalid message: {e}");
            return None;
        }
    };

    log_debug!("Received: type={} payload={}", msg.msg_type, msg.payload);

    if msg.msg_type == "client.hello" {
        return match parse_client_hello(msg.payload) {
            Ok(hello) => Some(hello),
            Err(error) => {
                log_warn!("Invalid client.hello message: {error}");
                None
            }
        };
    }

    // Settings request: respond with current game settings as a full sync.
    if msg.msg_type == "settings.request" {
        match serde_json::to_value(settings::get_game_settings()) {
            Ok(payload) => send(&WsMessage {
                msg_type: "settings.sync".to_string(),
                payload,
            }),
            Err(e) => log_error!("Failed to serialize settings: {e}"),
        }
        return None;
    }

    // Everything else: emit as Tauri event.
    let event_name = format!("ws:{}", msg.msg_type);
    let _ = app.emit(&event_name, msg.payload);
    None
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- WsMessage serialization --

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

    #[test]
    fn client_hello_accepts_game_identity() {
        let hello = parse_client_hello(serde_json::json!({
            "pid": 4242,
            "profile": "106_Nabor",
        }))
        .unwrap();

        assert_eq!(
            hello,
            ClientHello {
                pid: 4242,
                profile: "106_Nabor".to_string(),
            }
        );
    }

    #[test]
    fn client_hello_rejects_missing_profile() {
        assert!(parse_client_hello(serde_json::json!({"pid": 4242, "profile": ""})).is_err());
    }

    // -- Port file --

    #[test]
    fn running_backend_restores_removed_port_file() {
        let dir = std::env::temp_dir().join(format!("daystrom-ws-test-port-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("ws.port");
        fs::write(&path, "54321").unwrap();
        fs::remove_file(&path).unwrap();
        assert!(restore_missing_port_file(&path, 54322).unwrap());

        assert_eq!(fs::read_to_string(path).unwrap(), "54322");

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
        let received = tokio::time::timeout(Duration::from_secs(2), source.next())
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
        sink.send(Message::text(serde_json::to_string(&msg).unwrap())).await.unwrap();

        // Verify the server received it
        let received = tokio::time::timeout(Duration::from_secs(2), result_rx)
            .await
            .expect("timeout")
            .unwrap();

        let parsed: WsMessage = serde_json::from_str(&received).unwrap();
        assert_eq!(parsed.msg_type, "game.event");
        assert_eq!(parsed.payload["resources"]["parsteel"], 1000);

        server.abort();
    }
}
