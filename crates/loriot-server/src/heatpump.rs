//! Heat Pump API handlers

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use loriot_core::heatpump::{HeatPumpAnalyzer, HeatPumpConfig, HeatPumpReport};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::weather::SharedWeatherState;
use crate::AppState;

/// Heat pump service state
pub struct HeatPumpState {
    /// Configuration
    pub config: HeatPumpConfig,
}

impl HeatPumpState {
    pub fn new() -> Self {
        Self {
            config: HeatPumpConfig::default(),
        }
    }

    #[allow(dead_code)] // Reserved for configuration via API
    pub fn with_config(config: HeatPumpConfig) -> Self {
        Self { config }
    }
}

impl Default for HeatPumpState {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for shared heat pump state
pub type SharedHeatPumpState = Arc<RwLock<HeatPumpState>>;

/// Combined state for heat pump endpoints
#[derive(Clone)]
pub struct CombinedHeatPumpState {
    pub app: AppState,
    pub weather: SharedWeatherState,
    pub heatpump: SharedHeatPumpState,
}

/// Query parameters for heat pump analysis
#[derive(Debug, Deserialize)]
pub struct HeatPumpQuery {
    /// Number of days to analyze
    pub days: Option<usize>,
}

/// Heat pump configuration request
#[derive(Debug, Deserialize)]
pub struct ConfigureRequest {
    /// Device EUIs for underground heat source sensors
    pub source_devices: Vec<String>,
    /// Device names mapping
    pub device_names: Option<HashMap<String, String>>,
    /// Heating target temperature
    pub heating_target: Option<f32>,
    /// Cooling target temperature
    pub cooling_target: Option<f32>,
}

/// Heat pump info response
#[derive(Debug, Serialize)]
pub struct HeatPumpInfo {
    pub config: HeatPumpConfigResponse,
    pub has_data: bool,
    pub source_device_count: usize,
}

#[derive(Debug, Serialize)]
pub struct HeatPumpConfigResponse {
    pub source_devices: Vec<String>,
    pub device_names: HashMap<String, String>,
    pub heating_target: f32,
    pub cooling_target: f32,
}

impl From<&HeatPumpConfig> for HeatPumpConfigResponse {
    fn from(config: &HeatPumpConfig) -> Self {
        Self {
            source_devices: config.source_devices.clone(),
            device_names: config.device_names.clone(),
            heating_target: config.heating_target,
            cooling_target: config.cooling_target,
        }
    }
}

/// GET /api/heatpump/info - Get heat pump configuration info
pub async fn get_heatpump_info(State(state): State<CombinedHeatPumpState>) -> Json<HeatPumpInfo> {
    let hp = state.heatpump.read().await;
    let storage = state.app.storage.read().await;

    let has_data = !hp.config.source_devices.is_empty() && storage.reading_count() > 0;

    Json(HeatPumpInfo {
        config: (&hp.config).into(),
        has_data,
        source_device_count: hp.config.source_devices.len(),
    })
}

/// POST /api/heatpump/configure - Configure heat pump analysis
pub async fn configure_heatpump(
    State(state): State<CombinedHeatPumpState>,
    Json(request): Json<ConfigureRequest>,
) -> Json<HeatPumpInfo> {
    let mut hp = state.heatpump.write().await;

    hp.config.source_devices = request.source_devices;
    if let Some(names) = request.device_names {
        hp.config.device_names = names;
    }
    if let Some(heating) = request.heating_target {
        hp.config.heating_target = heating;
    }
    if let Some(cooling) = request.cooling_target {
        hp.config.cooling_target = cooling;
    }

    let storage = state.app.storage.read().await;
    let has_data = !hp.config.source_devices.is_empty() && storage.reading_count() > 0;

    Json(HeatPumpInfo {
        config: (&hp.config).into(),
        has_data,
        source_device_count: hp.config.source_devices.len(),
    })
}

/// GET /api/heatpump/analyze - Run heat pump analysis
pub async fn analyze_heatpump(
    State(state): State<CombinedHeatPumpState>,
    Query(query): Query<HeatPumpQuery>,
) -> Result<Json<HeatPumpReport>, StatusCode> {
    let hp = state.heatpump.read().await;

    if hp.config.source_devices.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let storage = state.app.storage.read().await;
    let weather = state.weather.read().await;

    // Get sensor readings
    let days = query.days.unwrap_or(30);
    let cutoff_ms = loriot_core::current_timestamp_ms() - (days as u64 * 24 * 60 * 60 * 1000);

    let readings = storage
        .read_from_disk_filtered(Some(cutoff_ms), None)
        .unwrap_or_default();

    // Get weather data
    let weather_records = weather.storage.read_all_from_disk().unwrap_or_default();
    let weather_data: Vec<_> = weather_records.iter().map(|r| r.weather.clone()).collect();

    // Run analysis
    let analyzer = HeatPumpAnalyzer::new(hp.config.clone());
    let report = analyzer.analyze(&readings, &weather_data);

    Ok(Json(report))
}

/// GET /api/heatpump/devices - Get available devices for configuration
pub async fn get_available_devices(
    State(state): State<CombinedHeatPumpState>,
) -> Json<Vec<DeviceOption>> {
    let storage = state.app.storage.read().await;
    let device_names = state.app.device_names.read().await;

    let stats = storage.get_device_statistics(None);

    let devices: Vec<DeviceOption> = stats
        .values()
        .map(|stat| {
            let name = stat
                .device_name
                .clone()
                .or_else(|| device_names.get(&stat.device_id).cloned())
                .unwrap_or_else(|| stat.device_id.clone());

            DeviceOption {
                eui: stat.device_id.clone(),
                name,
                reading_count: stat.total_readings,
                avg_temperature: stat.avg_temperature(),
            }
        })
        .collect();

    Json(devices)
}

#[derive(Debug, Serialize)]
pub struct DeviceOption {
    pub eui: String,
    pub name: String,
    pub reading_count: usize,
    pub avg_temperature: Option<f32>,
}
