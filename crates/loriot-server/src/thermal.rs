//! Thermal Analysis API handlers
//!
//! Provides endpoints for correlating indoor sensor data with outdoor weather
//! to analyze building thermal performance and insulation quality.

use axum::{
    extract::{Query, State},
    Json,
};
use loriot_core::thermal::{ThermalAnalysisReport, ThermalAnalyzer};
use serde::Deserialize;

use crate::weather::SharedWeatherState;
use crate::AppState;

/// Combined state for thermal analysis endpoints
#[derive(Clone)]
pub struct ThermalState {
    pub app: AppState,
    pub weather: SharedWeatherState,
}

/// Query parameters for thermal analysis
#[derive(Debug, Deserialize)]
pub struct ThermalQuery {
    /// Number of days to analyze (default: 30)
    pub days: Option<usize>,
    /// Specific device EUI to analyze (optional, analyzes all if not specified)
    pub device: Option<String>,
}

/// GET /api/thermal/analyze - Run thermal correlation analysis
pub async fn analyze_thermal(
    State(state): State<ThermalState>,
    Query(query): Query<ThermalQuery>,
) -> Json<ThermalAnalysisReport> {
    let days = query.days.unwrap_or(30);
    let cutoff_ms = loriot_core::current_timestamp_ms() - (days as u64 * 24 * 60 * 60 * 1000);

    // Get sensor readings
    let storage = state.app.storage.read().await;
    let readings = storage
        .read_from_disk_filtered(Some(cutoff_ms), query.device.as_deref())
        .unwrap_or_default();
    drop(storage);

    // Get weather data
    let weather = state.weather.read().await;
    let weather_records = weather.storage.read_all_from_disk().unwrap_or_default();
    let weather_data: Vec<_> = weather_records.iter().map(|r| r.weather.clone()).collect();
    drop(weather);

    // Run thermal analysis
    let analyzer = ThermalAnalyzer::new();
    let report = analyzer.analyze(&readings, &weather_data);

    Json(report)
}

/// Response for thermal summary
#[derive(Debug, serde::Serialize)]
pub struct ThermalSummary {
    /// Average indoor temperature
    pub avg_indoor_temp: f32,
    /// Average outdoor temperature
    pub avg_outdoor_temp: Option<f32>,
    /// Temperature difference
    pub temp_difference: Option<f32>,
    /// Insulation rating
    pub insulation_rating: String,
    /// Insulation rating description
    pub insulation_description: String,
    /// Heating degree days
    pub heating_degree_days: f64,
    /// Number of devices analyzed
    pub devices_analyzed: usize,
    /// Data quality rating
    pub data_quality: String,
}

/// GET /api/thermal/summary - Get quick thermal summary
pub async fn get_thermal_summary(
    State(state): State<ThermalState>,
    Query(query): Query<ThermalQuery>,
) -> Json<ThermalSummary> {
    let days = query.days.unwrap_or(30);
    let cutoff_ms = loriot_core::current_timestamp_ms() - (days as u64 * 24 * 60 * 60 * 1000);

    // Get sensor readings
    let storage = state.app.storage.read().await;
    let readings = storage
        .read_from_disk_filtered(Some(cutoff_ms), query.device.as_deref())
        .unwrap_or_default();
    drop(storage);

    // Get weather data
    let weather = state.weather.read().await;
    let weather_records = weather.storage.read_all_from_disk().unwrap_or_default();
    let weather_data: Vec<_> = weather_records.iter().map(|r| r.weather.clone()).collect();
    drop(weather);

    // Run thermal analysis
    let analyzer = ThermalAnalyzer::new();
    let report = analyzer.analyze(&readings, &weather_data);

    Json(ThermalSummary {
        avg_indoor_temp: report.building_metrics.avg_indoor_temp,
        avg_outdoor_temp: report.building_metrics.avg_outdoor_temp,
        temp_difference: report.building_metrics.avg_temp_difference,
        insulation_rating: format!("{:?}", report.building_metrics.insulation_rating),
        insulation_description: report
            .building_metrics
            .insulation_rating
            .description()
            .to_string(),
        heating_degree_days: report.heating_degree_days,
        devices_analyzed: report.device_analysis.len(),
        data_quality: format!("{:?}", report.data_quality.quality_rating),
    })
}
