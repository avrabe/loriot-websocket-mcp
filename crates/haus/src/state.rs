//! Shared application state

use haus_core::ingest::{loriot_items, Ingestor};
use haus_core::{AppConfig, ConnectionStatus, DiskReader, SensorReading, StorageManager};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Run blocking work (file IO, heavy analysis) off the async runtime.
/// A panic in `f` is propagated to the caller.
pub async fn run_blocking<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(v) => v,
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    }
}

/// Lock-free view of one source's connection, read by `/health`.
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

/// Connection state of every source (DD-011). A source that is not
/// configured is `None` and left out of `/health`.
#[derive(Debug, Default)]
pub struct Sources {
    pub loriot: UpstreamHealth,
    pub loxone: Option<UpstreamHealth>,
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
    /// Connection state per source (for `/health`)
    pub sources: Arc<Sources>,
    /// Writes readings into the time-series store
    pub ingest: Arc<Ingestor>,
}

impl AppState {
    pub fn new(
        storage: StorageManager,
        device_names: HashMap<String, String>,
        config: AppConfig,
        ingest: Arc<Ingestor>,
        sources: Sources,
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
            sources: Arc::new(sources),
            ingest,
        }
    }

    /// Update connection status and broadcast to subscribers
    pub async fn set_connection_status(&self, status: ConnectionStatus) {
        *self.connection_status.write().await = status.clone();
        let _ = self.status_tx.send(status);
    }

    /// Add a new reading and broadcast to subscribers. The in-memory update
    /// happens under the storage lock; the file append and the store write
    /// run afterwards on a blocking thread, with the lock released
    /// (REQ-STORE-001: no storage IO on the runtime).
    pub async fn add_reading(&self, reading: SensorReading) -> haus_core::Result<bool> {
        let (line, disk) = {
            let mut storage = self.storage.write().await;
            (storage.insert_reading(reading.clone())?, storage.disk())
        };
        let Some(line) = line else {
            return Ok(false);
        };
        let name = reading.name.clone().unwrap_or_else(|| reading.eui.clone());
        let items = loriot_items(&reading, &name);
        let ingest = self.ingest.clone();
        run_blocking(move || -> haus_core::Result<()> {
            disk.append_line(&line)?;
            ingest.ingest(&items)?;
            Ok(())
        })
        .await?;
        let _ = self.reading_tx.send(reading);
        Ok(true)
    }

    /// Handle to the history file, cloned out of the storage lock.
    pub async fn disk(&self) -> DiskReader {
        self.storage.read().await.disk()
    }

    /// Get device name from EUI
    pub async fn get_device_name(&self, eui: &str) -> Option<String> {
        self.device_names.read().await.get(eui).cloned()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use haus_core::store::{Source, Store};
    use haus_core::StorageConfig;

    /// A state on temporary files, for tests.
    pub fn test_state(name: &str, config: AppConfig) -> AppState {
        let base = std::env::temp_dir().join(format!(
            "haus-state-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let storage = StorageManager::new(StorageConfig {
            file_path: base.join("h.jsonl").to_string_lossy().into_owned(),
            ..Default::default()
        });
        let store = Arc::new(Store::open(base.join("s.redb")).unwrap());
        AppState::new(
            storage,
            HashMap::new(),
            config,
            Arc::new(Ingestor::new(store)),
            Sources::default(),
        )
    }

    fn reading(ts: u64) -> SensorReading {
        serde_json::from_str(&format!(
            r#"{{"cmd":"rx","eui":"E1","data":"00","ts":{ts},"name":null,"parsed_temperature":21.5}}"#
        ))
        .unwrap()
    }

    // rivet: verifies REQ-STORE-001
    #[tokio::test]
    async fn add_reading_persists_once_and_dedupes() {
        let state = test_state("add", AppConfig::default());
        let path = std::path::PathBuf::from(state.storage.read().await.disk().path());
        let mut rx = state.reading_tx.subscribe();

        assert!(state.add_reading(reading(1)).await.unwrap());
        assert!(!state.add_reading(reading(1)).await.unwrap());
        assert!(state.add_reading(reading(2)).await.unwrap());

        let lines = std::fs::read_to_string(&path).unwrap();
        assert_eq!(lines.lines().count(), 2);
        assert_eq!(state.storage.read().await.reading_count(), 2);
        assert_eq!(rx.recv().await.unwrap().ts, 1);
        assert_eq!(rx.recv().await.unwrap().ts, 2);

        // The store has the readings too, one series per decoded field.
        let store = state.ingest.store().clone();
        let id = store
            .series_id(&haus_core::store::SeriesKey::new(
                Source::Loriot,
                "E1",
                "temperature",
            ))
            .unwrap()
            .expect("temperature series");
        assert_eq!(store.range(id, 0, 10).unwrap(), vec![(1, 21.5), (2, 21.5)]);
    }
}
