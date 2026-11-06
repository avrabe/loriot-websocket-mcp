//! Shared application state

use loriot_core::{AppConfig, ConnectionStatus, SensorReading, StorageManager};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Lock-free view of the upstream (Loriot) connection, read by `/health`.
/// Atomics only, so the health check can never wait on a contended lock.
#[derive(Debug, Default)]
pub struct UpstreamHealth {
    connected: AtomicBool,
    /// Unix time in ms of the last data message, 0 = none yet.
    last_message_ms: AtomicI64,
}

impl UpstreamHealth {
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    pub fn record_message(&self) {
        self.last_message_ms
            .store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
    }

    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn last_message(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match self.last_message_ms.load(Ordering::Relaxed) {
            0 => None,
            ms => chrono::DateTime::from_timestamp_millis(ms),
        }
    }
}

/// Shared application state accessible by all handlers
#[derive(Clone)]
pub struct AppState {
    /// Storage manager for sensor readings
    pub storage: Arc<RwLock<StorageManager>>,
    /// Device name mappings (EUI -> friendly name)
    pub device_names: Arc<RwLock<HashMap<String, String>>>,
    /// Current connection status
    pub connection_status: Arc<RwLock<ConnectionStatus>>,
    /// Broadcast channel for new readings (SSE)
    pub reading_tx: broadcast::Sender<SensorReading>,
    /// Broadcast channel for status updates (SSE)
    pub status_tx: broadcast::Sender<ConnectionStatus>,
    /// Application configuration
    pub config: AppConfig,
    /// Upstream connection health (for `/health`)
    pub upstream: Arc<UpstreamHealth>,
}

impl AppState {
    pub fn new(
        storage: StorageManager,
        device_names: HashMap<String, String>,
        config: AppConfig,
    ) -> Self {
        let (reading_tx, _) = broadcast::channel(100);
        let (status_tx, _) = broadcast::channel(16);

        Self {
            storage: Arc::new(RwLock::new(storage)),
            device_names: Arc::new(RwLock::new(device_names)),
            connection_status: Arc::new(RwLock::new(ConnectionStatus::Disconnected)),
            reading_tx,
            status_tx,
            config,
            upstream: Arc::new(UpstreamHealth::default()),
        }
    }

    /// Update connection status and broadcast to subscribers
    pub async fn set_connection_status(&self, status: ConnectionStatus) {
        *self.connection_status.write().await = status.clone();
        let _ = self.status_tx.send(status);
    }

    /// Add a new reading and broadcast to subscribers
    pub async fn add_reading(&self, reading: SensorReading) -> loriot_core::Result<bool> {
        let is_new = self.storage.write().await.store_reading(reading.clone())?;
        if is_new {
            let _ = self.reading_tx.send(reading);
        }
        Ok(is_new)
    }

    /// Get device name from EUI
    pub async fn get_device_name(&self, eui: &str) -> Option<String> {
        self.device_names.read().await.get(eui).cloned()
    }
}
