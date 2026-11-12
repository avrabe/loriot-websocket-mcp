//! Core types and logic for haus
//!
//! This crate provides shared types and functionality for:
//! - Sensor data structures and decoding (Dragino LHT65S-PIR)
//! - Storage management with JSONL persistence
//! - Environmental analytics and data quality assessment

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Current time in milliseconds since the Unix epoch.
pub fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub mod analytics;
pub mod chart;
pub mod energy;
pub mod heatpump;
pub mod import;
pub mod ingest;
pub mod loxone;
pub mod sensors;
pub mod storage;
pub mod store;
pub mod thermal;
pub mod weather;

// Re-export commonly used types
pub use analytics::{
    format_duration, format_timestamp, AnalyticsEngine, AnalyticsReport, DataGap, DataGapSeverity,
};
pub use sensors::{decode_lht65s_pir, SensorReading};
pub use storage::{DeviceStats, DiskReader, StorageConfig, StorageManager};

/// Common error types for the application
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Sensor decode error: {0}")]
    SensorDecode(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("{0}")]
    Store(#[from] store::StoreError),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Weather data error: {0}")]
    Weather(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

/// Time window for filtering sensor data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum TimeWindow {
    #[default]
    All,
    LastHour,
    Last6Hours,
    LastDay,
    LastWeek,
    LastMonth,
}

impl TimeWindow {
    pub fn display_name(&self) -> &str {
        match self {
            TimeWindow::All => "All Time",
            TimeWindow::LastHour => "Last Hour",
            TimeWindow::Last6Hours => "Last 6 Hours",
            TimeWindow::LastDay => "Last Day",
            TimeWindow::LastWeek => "Last Week",
            TimeWindow::LastMonth => "Last Month",
        }
    }

    pub fn get_cutoff_timestamp(&self) -> Option<u64> {
        let now = current_timestamp_ms();

        match self {
            TimeWindow::All => None,
            TimeWindow::LastHour => Some(now - (60 * 60 * 1000)),
            TimeWindow::Last6Hours => Some(now - (6 * 60 * 60 * 1000)),
            TimeWindow::LastDay => Some(now - (24 * 60 * 60 * 1000)),
            TimeWindow::LastWeek => Some(now - (7 * 24 * 60 * 60 * 1000)),
            TimeWindow::LastMonth => Some(now - (30 * 24 * 60 * 60 * 1000)),
        }
    }

    /// Get all variants for UI display
    pub fn variants() -> &'static [TimeWindow] {
        &[
            TimeWindow::LastHour,
            TimeWindow::Last6Hours,
            TimeWindow::LastDay,
            TimeWindow::LastWeek,
            TimeWindow::LastMonth,
            TimeWindow::All,
        ]
    }
}

/// Application configuration
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub loriot_token: String,
    pub device_names_file: Option<String>,
    pub storage_file: Option<String>,
    pub web_port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            loriot_token: String::new(),
            device_names_file: Some("devices.json".to_string()),
            storage_file: Some("sensor_history.jsonl".to_string()),
            web_port: 3000,
        }
    }
}

/// Load device name mappings from a JSON file
pub fn load_device_names(file_path: &str) -> HashMap<String, String> {
    std::fs::read_to_string(file_path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

/// Connection status for WebSocket
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ConnectionStatus {
    Connected,
    Connecting,
    #[default]
    Disconnected,
    Reconnecting {
        attempt: u32,
        max_attempts: u32,
    },
    Error(String),
}
