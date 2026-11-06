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
