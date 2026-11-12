//! WebSocket connection to Loriot for sensor data ingestion
//!
//! Liveness rules (see the 2026-10 hang): every await on the upstream is
//! bounded. The handshake has a timeout, every write has a timeout, and the
//! connection is declared dead when no frame (data or pong) has arrived for
//! `pong_timeout`, measured from the last received frame, so sending pings
//! cannot postpone it. On any end of the session the socket is dropped and
//! the client reconnects with capped exponential backoff.

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use haus_core::{
    decode_lht65s_pir, sensors::dragino::create_sensor_reading, ConnectionStatus, SensorReading,
};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::{interval_at, sleep, sleep_until, timeout, Instant, MissedTickBehavior};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::AppState;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

const LORIOT_WS_URL: &str = "wss://eu1.loriot.io/app";
/// After this many consecutive failed sessions the status turns to `Error`
/// (the client keeps retrying at the backoff cap).
const MAX_RETRIES: u32 = 10;
const PING_INTERVAL: Duration = Duration::from_secs(30);
/// Maximum silence (no frame at all, pongs included) before the connection is
/// considered dead. Must be longer than `PING_INTERVAL`.
const PONG_TIMEOUT: Duration = Duration::from_secs(75);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const BACKOFF_BASE: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

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
    /// Maximum time without any received frame before the session is dropped.
    pub pong_timeout: Duration,
    pub connect_timeout: Duration,
    pub write_timeout: Duration,
    pub backoff_base: Duration,
    pub backoff_max: Duration,
}

impl Default for LoriotClientConfig {
    fn default() -> Self {
        Self {
            url: LORIOT_WS_URL.to_string(),
            ping_interval: PING_INTERVAL,
            pong_timeout: PONG_TIMEOUT,
            connect_timeout: CONNECT_TIMEOUT,
            write_timeout: WRITE_TIMEOUT,
            backoff_base: BACKOFF_BASE,
            backoff_max: BACKOFF_MAX,
        }
    }
}

/// Delay before reconnect attempt `attempt` (1-based): base * 2^(attempt-1),
/// capped at `max`.
fn backoff_delay(cfg: &LoriotClientConfig, attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1).min(16);
    cfg.backoff_base
        .saturating_mul(1u32 << exp)
        .min(cfg.backoff_max)
}

/// Run the Loriot WebSocket connection with automatic reconnection
pub async fn run_loriot_connection(state: AppState) {
    run_loriot_connection_with(state, LoriotClientConfig::default()).await
}

/// Run the Loriot WebSocket connection against `cfg` with automatic reconnection.
/// Never returns.
pub async fn run_loriot_connection_with(state: AppState, cfg: LoriotClientConfig) {
    let mut attempt: u32 = 0;

    loop {
        state
            .set_connection_status(ConnectionStatus::Connecting)
            .await;

        let mut received_data = false;
        let result = run_session(&state, &cfg, &mut received_data).await;
        state.sources.loriot.set_connected(false);

        match result {
            Ok(()) => tracing::info!("Loriot connection closed by upstream"),
            Err(e) => tracing::warn!("Loriot connection lost: {}", e),
        }

        // A session that delivered data counts as healthy: start over at the
        // shortest delay. Otherwise back off further.
        attempt = if received_data {
            1
        } else {
            attempt.saturating_add(1)
        };

        let status = if attempt > MAX_RETRIES {
            ConnectionStatus::Error(format!(
                "{} consecutive failed attempts, still retrying",
                attempt - 1
            ))
        } else {
            ConnectionStatus::Reconnecting {
                attempt,
                max_attempts: MAX_RETRIES,
            }
        };
        state.set_connection_status(status).await;

        let delay = backoff_delay(&cfg, attempt);
        let jitter = Duration::from_millis(fastrand::u64(0..=(delay.as_millis() as u64 / 4)));
        sleep(delay + jitter).await;
    }
}

/// One connection session. Returns when the upstream closes, errors, or goes
/// silent; the stream (and its socket) is dropped on return.
async fn run_session(
    state: &AppState,
    cfg: &LoriotClientConfig,
    received_data: &mut bool,
) -> Result<(), BoxError> {
    let url = format!("{}?token={}", cfg.url, state.config.loriot_token);

    tracing::info!("Connecting to Loriot WebSocket...");

    let (ws_stream, _) = timeout(cfg.connect_timeout, connect_async(&url))
        .await
        .map_err(|_| format!("handshake timed out after {:?}", cfg.connect_timeout))??;
    let (mut write, mut read) = ws_stream.split();

    state
        .set_connection_status(ConnectionStatus::Connected)
        .await;
    state.sources.loriot.set_connected(true);
    tracing::info!("Connected to Loriot");

    // Request cached messages from the last 24 hours
    let now_ms = chrono::Utc::now().timestamp_millis();
    let cache_query = serde_json::json!({
        "cmd": "cq",
        "from": now_ms - (24 * 60 * 60 * 1000),
        "to": now_ms,
        "pagesize": 100
    });
    timeout(
        cfg.write_timeout,
        write.send(Message::Text(cache_query.to_string().into())),
    )
    .await
    .map_err(|_| "cache query write timed out")??;

    let mut ping = interval_at(Instant::now() + cfg.ping_interval, cfg.ping_interval);
    ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_rx = Instant::now();

    loop {
        tokio::select! {
            msg = read.next() => match msg {
                Some(Ok(message)) => {
                    last_rx = Instant::now();
                    match message {
                        Message::Text(text) => {
                            *received_data = true;
                            state.sources.loriot.record_message();
                            if let Err(e) = handle_text(state, text.as_str()).await {
                                tracing::warn!("Error handling message: {}", e);
                            }
                        }
                        Message::Close(frame) => {
                            tracing::info!("Upstream sent close: {:?}", frame);
                            return Ok(());
                        }
                        // Ping is answered by tungstenite; Pong/Binary only prove liveness.
                        _ => {}
                    }
                }
                Some(Err(e)) => return Err(e.into()),
                None => return Ok(()),
            },
            // The deadline is derived from the last received frame, so pings
            // sent meanwhile do not move it.
            _ = sleep_until(last_rx + cfg.pong_timeout) => {
                return Err(format!("no frame from upstream for {:?}", cfg.pong_timeout).into());
            }
            _ = ping.tick() => {
                timeout(cfg.write_timeout, write.send(Message::Ping(Bytes::new())))
                    .await
                    .map_err(|_| "ping write timed out")??;
            }
        }
    }
}

async fn handle_text(state: &AppState, text: &str) -> Result<(), BoxError> {
    let payload: LoriotPayload = serde_json::from_str(text)?;

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
    use haus_core::AppConfig;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    const DUMMY_TOKEN: &str = "dummy-test-token";

    fn test_state(name: &str) -> AppState {
        crate::state::tests::test_state(
            name,
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
        /// Send one message, then keep reading (so pings get pongs) but send
        /// no data: a healthy, quiet upstream that must NOT be dropped.
        QuietButAlive,
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
                let _ = ws.send(Message::Text(r#"{"cmd":"noop"}"#.into())).await;
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
                    Upstream::QuietButAlive => held.push(tokio::spawn(async move {
                        // Polling the stream makes tungstenite answer pings.
                        while let Some(Ok(_)) = ws.next().await {}
                        std::future::pending::<()>().await;
                    })),
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
            let mut iv = tokio::time::interval(Duration::from_millis(10));
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
        assert!(
            got >= expected,
            "runtime starved: {got} ticks, expected >= {expected}"
        );
    }

    fn fast() -> LoriotClientConfig {
        LoriotClientConfig {
            ping_interval: Duration::from_millis(100),
            pong_timeout: Duration::from_millis(300),
            ..Default::default()
        }
    }

    // rivet: verifies REQ-LORIOT-001
    #[tokio::test(flavor = "current_thread")]
    async fn reconnects_when_upstream_goes_silent() {
        let _serial = crate::state::tests::serial().await;
        assert_reconnects(Upstream::GoSilent, fast(), Duration::from_secs(5)).await;
    }

    // rivet: verifies REQ-LORIOT-001
    #[tokio::test(flavor = "current_thread")]
    async fn reconnects_when_upstream_drops_tcp() {
        let _serial = crate::state::tests::serial().await;
        assert_reconnects(Upstream::DropTcp, fast(), Duration::from_secs(6)).await;
    }

    // rivet: verifies REQ-LORIOT-001
    #[tokio::test(flavor = "current_thread")]
    async fn reconnects_when_handshake_never_completes() {
        let _serial = crate::state::tests::serial().await;
        // Uses the production connect timeout on purpose.
        assert_reconnects(Upstream::NoHandshake, fast(), Duration::from_secs(20)).await;
    }

    // rivet: verifies REQ-LORIOT-001
    #[tokio::test(flavor = "current_thread")]
    async fn keeps_a_quiet_upstream_that_answers_pings() {
        let _serial = crate::state::tests::serial().await;
        let (url, accepts) = spawn_upstream(Upstream::QuietButAlive).await;
        let cfg = LoriotClientConfig { url, ..fast() };
        let client = tokio::spawn(run_loriot_connection_with(test_state("quiet"), cfg));
        // ~7 idle timeouts' worth of silence without data.
        tokio::time::sleep(Duration::from_secs(2)).await;
        client.abort();
        assert_eq!(
            accepts.load(Ordering::SeqCst),
            1,
            "healthy upstream was dropped"
        );
    }

    // rivet: verifies REQ-LORIOT-001
    #[test]
    fn the_production_timings_are_the_specified_ones() {
        let c = LoriotClientConfig::default();
        assert_eq!(c.ping_interval, Duration::from_secs(30));
        assert_eq!(c.pong_timeout, Duration::from_secs(75));
        assert!(c.connect_timeout > Duration::ZERO && c.connect_timeout <= Duration::from_secs(30));
        assert!(c.write_timeout > Duration::ZERO && c.write_timeout <= Duration::from_secs(30));
        assert_eq!(backoff_delay(&c, 1), Duration::from_secs(1));
        assert_eq!(backoff_delay(&c, 7), Duration::from_secs(60));
        assert_eq!(backoff_delay(&c, 100), Duration::from_secs(60));
    }

    // rivet: verifies REQ-LORIOT-001
    #[test]
    fn backoff_is_exponential_and_capped() {
        let cfg = LoriotClientConfig::default();
        let secs: Vec<u64> = (1..=9).map(|a| backoff_delay(&cfg, a).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
        assert_eq!(backoff_delay(&cfg, u32::MAX), BACKOFF_MAX);
    }
}
