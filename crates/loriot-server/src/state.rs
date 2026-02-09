//! Shared application state

use loriot_core::{AppConfig, ConnectionStatus, SensorReading, StorageManager};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

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
