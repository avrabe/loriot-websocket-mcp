use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Declare modules
pub mod sensors;
pub mod storage;

// Re-export common types for all modules
pub use sensors::*;
pub use storage::*;

// Operation modes for the CLI
#[derive(Debug, Clone, PartialEq)]
pub enum OperationMode {
    Tui,
    McpStdin,
    McpSse,
    McpHttp,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub mode: OperationMode,
    pub disable_tui: bool,
    pub loriot_token: String,
    pub device_names_file: Option<String>,
    pub storage_file: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            mode: OperationMode::Tui,
            disable_tui: false,
            loriot_token: String::new(),
            device_names_file: Some("devices.json".to_string()),
            storage_file: Some("sensor_history.jsonl".to_string()),
        }
    }
}

// Common error types
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
    
    #[error("MCP error: {0}")]
    Mcp(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

// Common time window enum
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TimeWindow {
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            
        match self {
            TimeWindow::All => None,
            TimeWindow::LastHour => Some(now - (60 * 60 * 1000)),
            TimeWindow::Last6Hours => Some(now - (6 * 60 * 60 * 1000)),
            TimeWindow::LastDay => Some(now - (24 * 60 * 60 * 1000)),
            TimeWindow::LastWeek => Some(now - (7 * 24 * 60 * 60 * 1000)),
            TimeWindow::LastMonth => Some(now - (30 * 24 * 60 * 60 * 1000)),
        }
    }
}

// Shared device utilities
pub fn load_device_names(file_path: &str) -> HashMap<String, String> {
    std::fs::read_to_string(file_path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}