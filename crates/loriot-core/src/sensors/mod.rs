//! Sensor data types and decoding for LoRaWAN devices

use serde::{Deserialize, Serialize};

pub mod dragino;

pub use dragino::{create_sensor_reading, decode_lht65s_pir};

/// Represents a single sensor reading from a LoRaWAN device
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SensorReading {
    /// Message command type from Loriot (e.g., "rx", "gw", "cq")
    pub cmd: String,
    /// Device EUI (unique identifier)
    pub eui: String,
    /// Hex-encoded raw payload data
    pub data: String,
    /// Timestamp in milliseconds since UNIX epoch
    pub ts: u64,
    /// Optional device name from Loriot or local mapping
    pub name: Option<String>,

    // Parsed fields (cached for performance)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_humidity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_battery_voltage: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_battery_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_activity: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_movement_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_lux: Option<u16>,
}

impl SensorReading {
    /// Get the display name (device name or EUI if no name set)
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.eui)
    }

    /// Check if this reading has valid parsed environmental data
    pub fn has_environmental_data(&self) -> bool {
        self.parsed_temperature.is_some() && self.parsed_humidity.is_some()
    }

    /// Get temperature in the preferred format
    pub fn temperature_display(&self) -> String {
        self.parsed_temperature
            .map(|t| format!("{:.1}°C", t))
            .unwrap_or_else(|| "N/A".to_string())
    }

    /// Get humidity in the preferred format
    pub fn humidity_display(&self) -> String {
        self.parsed_humidity
            .map(|h| format!("{:.1}%", h))
            .unwrap_or_else(|| "N/A".to_string())
    }

    /// Get battery status display
    pub fn battery_display(&self) -> String {
        match (&self.parsed_battery_status, self.parsed_battery_voltage) {
            (Some(status), Some(voltage)) => format!("{} ({:.2}V)", status, voltage),
            (Some(status), None) => status.clone(),
            (None, Some(voltage)) => format!("{:.2}V", voltage),
            (None, None) => "N/A".to_string(),
        }
    }
}
