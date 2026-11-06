//! WebSocket connection to Loriot for sensor data ingestion

use futures::{SinkExt, StreamExt};
use bytes::Bytes;
use loriot_core::{
    sensors::dragino::create_sensor_reading, decode_lht65s_pir, ConnectionStatus, SensorReading,
};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::{interval, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::AppState;

const LORIOT_WS_URL: &str = "wss://eu1.loriot.io/app";
const MAX_RETRIES: u32 = 10;
const PING_INTERVAL: Duration = Duration::from_secs(30);
const PONG_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
struct LoriotPayload {
    cmd: String,
    #[serde(rename = "EUI")]
    eui: Option<String>,
    data: Option<String>,
    ts: Option<u64>,
    #[serde(rename = "cache")]
    cached_messages: Option<Vec<CachedMessage>>,
}

#[derive(Debug, Deserialize)]
struct CachedMessage {
    cmd: String,
    #[serde(rename = "EUI")]
    eui: String,
    data: String,
    ts: u64,
}

/// Connection parameters for the Loriot client (a seam so tests can point the
/// client at a local server with short timings).
#[derive(Debug, Clone)]
pub struct LoriotClientConfig {
    pub url: String,
    pub ping_interval: Duration,
    pub pong_timeout: Duration,
}

impl Default for LoriotClientConfig {
    fn default() -> Self {
        Self {
            url: LORIOT_WS_URL.to_string(),
            ping_interval: PING_INTERVAL,
            pong_timeout: PONG_TIMEOUT,
        }
    }
}

/// Run the Loriot WebSocket connection with automatic reconnection
pub async fn run_loriot_connection(state: AppState) {
    run_loriot_connection_with(state, LoriotClientConfig::default()).await
}

/// Run the Loriot WebSocket connection against `cfg` with automatic reconnection
pub async fn run_loriot_connection_with(state: AppState, cfg: LoriotClientConfig) {
    let mut retry_count = 0;

    loop {
        state
            .set_connection_status(ConnectionStatus::Connecting)
            .await;

        match connect_to_loriot(&state, &cfg).await {
            Ok(()) => {
                // Connection closed normally
                retry_count = 0;
            }
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                retry_count += 1;

                if retry_count >= MAX_RETRIES {
                    state
                        .set_connection_status(ConnectionStatus::Error(format!(
                            "Max retries ({}) exceeded",
                            MAX_RETRIES
                        )))
                        .await;
                    retry_count = 0;
                }
            }
        }

        state
            .set_connection_status(ConnectionStatus::Reconnecting {
                attempt: retry_count,
                max_attempts: MAX_RETRIES,
            })
            .await;

        // Exponential backoff with jitter
        let delay = Duration::from_secs(2u64.pow(retry_count.min(6)));
        let jitter = Duration::from_millis(fastrand::u64(0..1000));
        tokio::time::sleep(delay + jitter).await;
    }
}

async fn connect_to_loriot(
    state: &AppState,
    cfg: &LoriotClientConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}?token={}", cfg.url, state.config.loriot_token);

    tracing::info!("Connecting to Loriot WebSocket...");

    let (ws_stream, _) = connect_async(&url).await?;
    let (mut write, mut read) = ws_stream.split();

    state
        .set_connection_status(ConnectionStatus::Connected)
        .await;
    tracing::info!("Connected to Loriot");

    // Request cached messages from the last 24 hours
    let cache_query = serde_json::json!({
        "cmd": "cq",
        "from": chrono::Utc::now().timestamp_millis() - (24 * 60 * 60 * 1000),
        "to": chrono::Utc::now().timestamp_millis(),
        "pagesize": 100
    });
    write
        .send(Message::Text(cache_query.to_string().into()))
        .await?;

    let mut ping_interval = interval(cfg.ping_interval);

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                write.send(Message::Ping(Bytes::new())).await?;
            }
            msg = timeout(cfg.pong_timeout, read.next()) => {
                match msg {
                    Ok(Some(Ok(message))) => {
                        if let Err(e) = handle_message(state, message).await {
                            tracing::warn!("Error handling message: {}", e);
                        }
                    }
                    Ok(Some(Err(e))) => {
                        return Err(e.into());
                    }
                    Ok(None) => {
                        tracing::info!("WebSocket connection closed");
                        return Ok(());
                    }
                    Err(_) => {
                        return Err("Ping timeout".into());
                    }
                }
            }
        }
    }
}

async fn handle_message(
    state: &AppState,
    message: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = match message {
        Message::Text(t) => t.to_string(),
        Message::Pong(_) => return Ok(()),
        Message::Ping(_) => return Ok(()),
        Message::Close(_) => return Err("Connection closed".into()),
        _ => return Ok(()),
    };

    let payload: LoriotPayload = serde_json::from_str(&text)?;

    match payload.cmd.as_str() {
        "rx" | "gw" => {
            // Real-time uplink message
            if let (Some(eui), Some(data), Some(ts)) = (payload.eui, payload.data, payload.ts) {
                if let Some(reading) = create_reading(&eui, &data, ts, &payload.cmd, state).await {
                    let is_new = state.add_reading(reading).await?;
                    if is_new {
                        tracing::debug!("New reading from {}", eui);
                    }
                }
            }
        }
        "cq" => {
            // Cached messages response
            if let Some(messages) = payload.cached_messages {
                tracing::info!("Processing {} cached messages", messages.len());
                for msg in messages {
                    if let Some(reading) =
                        create_reading(&msg.eui, &msg.data, msg.ts, &msg.cmd, state).await
                    {
                        let _ = state.add_reading(reading).await;
                    }
                }
            }
        }
        _ => {
            tracing::trace!("Ignoring message type: {}", payload.cmd);
        }
    }

    Ok(())
}

async fn create_reading(
    eui: &str,
    data: &str,
    ts: u64,
    cmd: &str,
    state: &AppState,
) -> Option<SensorReading> {
    let parsed = decode_lht65s_pir(data)?;
    let device_name = state.get_device_name(eui).await;

    Some(create_sensor_reading(
        cmd.to_string(),
        eui.to_string(),
        data.to_string(),
        ts,
        device_name,
        parsed,
    ))
}

#[cfg(test)]
mod tests {
    //! Regression tests for the 2026-10 hang: the client must notice a dead or
    //! silent upstream within a bounded time, reconnect, and never starve the
    //! runtime. All tests use a local plain-`ws://` server and a dummy token.
    use super::*;
    use loriot_core::{AppConfig, StorageConfig, StorageManager};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    const DUMMY_TOKEN: &str = "dummy-test-token";

    fn test_state(name: &str) -> AppState {
        let path = std::env::temp_dir().join(format!(
            "loriot-test-{}-{}.jsonl",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let storage = StorageManager::new(StorageConfig {
            file_path: path.to_string_lossy().into_owned(),
            ..Default::default()
        });
        AppState::new(
            storage,
            HashMap::new(),
            AppConfig {
                loriot_token: DUMMY_TOKEN.to_string(),
                device_names_file: None,
                ..Default::default()
            },
        )
    }

    #[derive(Clone, Copy)]
    enum Upstream {
        /// Complete the handshake, send one message, then never poll the
        /// socket again: no data, no pong, no close. The kernel still ACKs, so
        /// the client's writes succeed (a half-open / wedged peer).
        GoSilent,
        /// Send one message, then reset the TCP connection without a close frame.
        DropTcp,
        /// Accept TCP but never answer the websocket handshake.
        NoHandshake,
    }

    /// Start a local upstream; returns its URL and a counter of TCP accepts.
    async fn spawn_upstream(kind: Upstream) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepts = Arc::new(AtomicUsize::new(0));
        let counter = accepts.clone();
        tokio::spawn(async move {
            let mut held = Vec::new();
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    continue;
                };
                counter.fetch_add(1, Ordering::SeqCst);
                if let Upstream::NoHandshake = kind {
                    held.push(tokio::spawn(async move {
                        let _tcp = tcp;
                        std::future::pending::<()>().await;
                    }));
                    continue;
                }
                let Ok(mut ws) = accept_async(tcp).await else {
                    continue;
                };
                let _ = ws
                    .send(Message::Text(r#"{"cmd":"noop"}"#.into()))
                    .await;
                match kind {
                    Upstream::GoSilent => held.push(tokio::spawn(async move {
                        let _ws = ws;
                        std::future::pending::<()>().await;
                    })),
                    Upstream::DropTcp => {
                        let tcp = ws.into_inner();
                        let _ = tcp.set_linger(Some(Duration::ZERO));
                        drop(tcp);
                    }
                    Upstream::NoHandshake => unreachable!(),
                }
            }
        });
        (format!("ws://{addr}/app"), accepts)
    }

    /// Run the client against `kind` and wait until it connects a second time.
    /// Also runs a 10 ms ticker on the same (single-threaded) runtime so a
    /// client that busy-loops or blocks would show up as missing ticks.
    async fn assert_reconnects(kind: Upstream, cfg: LoriotClientConfig, within: Duration) {
        let (url, accepts) = spawn_upstream(kind).await;
        let cfg = LoriotClientConfig { url, ..cfg };

        let ticks = Arc::new(AtomicUsize::new(0));
        let t = ticks.clone();
        let ticker = tokio::spawn(async move {
            let mut iv = interval(Duration::from_millis(10));
            loop {
                iv.tick().await;
                t.fetch_add(1, Ordering::SeqCst);
            }
        });

        let client = tokio::spawn(run_loriot_connection_with(test_state("reconnect"), cfg));
        let start = Instant::now();
        while accepts.load(Ordering::SeqCst) < 2 && start.elapsed() < within {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let elapsed = start.elapsed();
        client.abort();
        ticker.abort();

        let n = accepts.load(Ordering::SeqCst);
        assert!(
            n >= 2,
            "client did not reconnect within {within:?} (accepts = {n}); a dead upstream is never detected"
        );
        // The runtime must stay responsive: expect at least a quarter of the
        // ideal 10 ms ticks while we waited.
        let expected = (elapsed.as_millis() / 10 / 4) as usize;
        let got = ticks.load(Ordering::SeqCst);
        assert!(got >= expected, "runtime starved: {got} ticks, expected >= {expected}");
    }

    fn fast() -> LoriotClientConfig {
        LoriotClientConfig {
            ping_interval: Duration::from_millis(100),
            pong_timeout: Duration::from_millis(300),
            ..Default::default()
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reconnects_when_upstream_goes_silent() {
        assert_reconnects(Upstream::GoSilent, fast(), Duration::from_secs(5)).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reconnects_when_upstream_drops_tcp() {
        assert_reconnects(Upstream::DropTcp, fast(), Duration::from_secs(6)).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reconnects_when_handshake_never_completes() {
        // Uses the production connect timeout on purpose.
        assert_reconnects(Upstream::NoHandshake, fast(), Duration::from_secs(20)).await;
    }
}
