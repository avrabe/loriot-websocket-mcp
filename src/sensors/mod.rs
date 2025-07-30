use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SensorReading {
    // Loriot WebSocket format fields (raw data)
    pub cmd: String,
    pub eui: String,
    pub data: String,
    pub ts: u64,
    pub name: Option<String>,
    
    // Parsed fields (for faster loading)
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

pub mod dragino;

pub use dragino::decode_lht65s_pir;