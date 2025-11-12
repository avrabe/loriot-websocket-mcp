use rmcp::{
    handler::server::router::tool::ToolRouter,
    model::*,
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError,
    ServiceExt,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::BufRead,
    path::Path,
    sync::Arc,
};
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct StoredReading {
    cmd: String,
    eui: String,
    data: String,
    ts: u64,
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_humidity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_battery_voltage: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_battery_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_activity: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_movement_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_lux: Option<u16>,
}

#[derive(Serialize, Clone, Debug)]
struct DeviceInfo {
    eui: String,
    name: String,
    total_readings: usize,
    avg_temperature: f32,
    avg_humidity: f32,
    latest_battery_voltage: f32,
    latest_battery_status: String,
    pir_events: usize,
    latest_timestamp: u64,
}

#[derive(Clone)]
pub struct SensorDataService {
    readings: Arc<RwLock<Vec<StoredReading>>>,
    device_names: Arc<HashMap<String, String>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl SensorDataService {
    fn new() -> Self {
        Self {
            readings: Arc::new(RwLock::new(Vec::new())),
            device_names: Arc::new(HashMap::new()),
            tool_router: Self::tool_router(),
        }
    }

    async fn load_data(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Load device names from devices.json if available
        let device_names: HashMap<String, String> = if let Ok(config_data) = std::fs::read_to_string("devices.json") {
            serde_json::from_str(&config_data).unwrap_or_default()
        } else {
            HashMap::new()
        };
        self.device_names = Arc::new(device_names);

        // Load sensor history
        let file_path = "sensor_history.jsonl";
        if !Path::new(file_path).exists() {
            return Ok(());
        }

        let file = std::fs::File::open(file_path)?;
        let reader = std::io::BufReader::new(file);
        let mut readings = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(reading) = serde_json::from_str::<StoredReading>(&line) {
                readings.push(reading);
            }
        }

        readings.sort_by_key(|r| r.ts);
        *self.readings.write().await = readings;
        Ok(())
    }

    #[tool(description = "Get the latest sensor readings from all devices. Optionally specify a limit (default: 10).")]
    async fn get_latest_readings(&self) -> Result<CallToolResult, McpError> {
        let limit = 10_usize;
        let readings = self.readings.read().await;

        let latest: Vec<_> = readings
            .iter()
            .rev()
            .take(limit)
            .collect();

        let json_output = serde_json::to_string_pretty(&latest)
            .map_err(|e| McpError::new(ErrorCode(-32603), e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json_output)]))
    }

    #[tool(description = "List all known sensor devices with their basic information")]
    async fn list_devices(&self) -> Result<CallToolResult, McpError> {
        let readings = self.readings.read().await;

        // Group by device EUI
        let mut devices_map: HashMap<String, Vec<&StoredReading>> = HashMap::new();
        for reading in readings.iter() {
            devices_map.entry(reading.eui.clone()).or_default().push(reading);
        }

        let devices: Vec<DeviceInfo> = devices_map
            .into_iter()
            .map(|(eui, device_readings)| {
                let name = self.device_names.get(&eui).cloned().unwrap_or(eui.clone());
                let total_readings = device_readings.len();

                let temps: Vec<f32> = device_readings.iter()
                    .filter_map(|r| r.parsed_temperature)
                    .collect();
                let hums: Vec<f32> = device_readings.iter()
                    .filter_map(|r| r.parsed_humidity)
                    .collect();

                let avg_temperature = if temps.is_empty() { 0.0 } else { temps.iter().sum::<f32>() / temps.len() as f32 };
                let avg_humidity = if hums.is_empty() { 0.0 } else { hums.iter().sum::<f32>() / hums.len() as f32 };

                let latest = device_readings.last().unwrap();
                let pir_events = device_readings.iter()
                    .filter(|r| r.parsed_activity.unwrap_or(false))
                    .count();

                DeviceInfo {
                    eui,
                    name,
                    total_readings,
                    avg_temperature,
                    avg_humidity,
                    latest_battery_voltage: latest.parsed_battery_voltage.unwrap_or(0.0),
                    latest_battery_status: latest.parsed_battery_status.clone().unwrap_or_default(),
                    pir_events,
                    latest_timestamp: latest.ts,
                }
            })
            .collect();

        let json_output = serde_json::to_string_pretty(&devices)
            .map_err(|e| McpError::new(ErrorCode(-32603), e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json_output)]))
    }

    #[tool(description = "Get PIR motion detection events (default limit: 20)")]
    async fn get_pir_events(&self) -> Result<CallToolResult, McpError> {
        let limit = 20_usize;
        let readings = self.readings.read().await;

        let pir_events: Vec<_> = readings
            .iter()
            .filter(|r| r.parsed_activity.unwrap_or(false))
            .rev()
            .take(limit)
            .collect();

        let json_output = serde_json::to_string_pretty(&pir_events)
            .map_err(|e| McpError::new(ErrorCode(-32603), e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json_output)]))
    }

    #[tool(description = "Get summary statistics across all devices")]
    async fn get_summary(&self) -> Result<CallToolResult, McpError> {
        let readings = self.readings.read().await;

        let total_readings = readings.len();
        let unique_devices: std::collections::HashSet<_> = readings.iter().map(|r| &r.eui).collect();
        let total_devices = unique_devices.len();

        let pir_events = readings.iter()
            .filter(|r| r.parsed_activity.unwrap_or(false))
            .count();

        let temps: Vec<f32> = readings.iter()
            .filter_map(|r| r.parsed_temperature)
            .collect();
        let avg_temp = if temps.is_empty() { 0.0 } else { temps.iter().sum::<f32>() / temps.len() as f32 };

        let hums: Vec<f32> = readings.iter()
            .filter_map(|r| r.parsed_humidity)
            .collect();
        let avg_hum = if hums.is_empty() { 0.0 } else { hums.iter().sum::<f32>() / hums.len() as f32 };

        let first_reading = readings.first();
        let latest_reading = readings.last();

        let summary = serde_json::json!({
            "total_readings": total_readings,
            "total_devices": total_devices,
            "total_pir_events": pir_events,
            "overall_stats": {
                "average_temperature": avg_temp,
                "average_humidity": avg_hum,
            },
            "time_range": {
                "first_reading": first_reading.map(|r|
                    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.ts as i64)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default()
                ),
                "latest_reading": latest_reading.map(|r|
                    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.ts as i64)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default()
                ),
            }
        });

        let json_output = serde_json::to_string_pretty(&summary)
            .map_err(|e| McpError::new(ErrorCode(-32603), e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json_output)]))
    }
}

#[tool_handler]
impl rmcp::ServerHandler for SensorDataService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("MCP server providing access to LoRaWAN sensor data collected from Loriot WebSocket API. Query temperature, humidity, battery status, and PIR motion detection data from Dragino LHT65S-PIR sensors.".to_string()),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut service = SensorDataService::new();

    // Load existing sensor data
    if let Err(e) = service.load_data().await {
        eprintln!("Warning: Failed to load sensor data: {}", e);
    } else {
        let readings_count = service.readings.read().await.len();
        eprintln!("Loaded {} sensor readings", readings_count);
    }

    eprintln!("LoRaWAN Sensor Data MCP Server starting...");
    let service = service.serve(stdio()).await?;
    eprintln!("Server ready");
    service.waiting().await?;
    Ok(())
}
