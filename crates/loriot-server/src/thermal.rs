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

use crate::state::run_blocking;
use crate::weather::{load_daily_weather, weather_history_path, SharedWeatherState};
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

/// Read sensor and weather history and run the thermal analysis, off the
/// runtime and without holding any lock during file IO.
async fn thermal_report(
    state: &ThermalState,
    cutoff_ms: u64,
    device: Option<String>,
) -> ThermalAnalysisReport {
    let disk = state.app.disk().await;
    let weather_path = weather_history_path(&state.weather).await;
    run_blocking(move || {
        let readings = disk
            .read_filtered(Some(cutoff_ms), device.as_deref())
            .unwrap_or_default();
        let weather_data = load_daily_weather(&weather_path);
        ThermalAnalyzer::new().analyze(&readings, &weather_data)
    })
    .await
}

/// GET /api/thermal/analyze - Run thermal correlation analysis
pub async fn analyze_thermal(
    State(state): State<ThermalState>,
    Query(query): Query<ThermalQuery>,
) -> Json<ThermalAnalysisReport> {
    let days = query.days.unwrap_or(30);
    let cutoff_ms = loriot_core::current_timestamp_ms() - (days as u64 * 24 * 60 * 60 * 1000);

    let report = thermal_report(&state, cutoff_ms, query.device.clone()).await;

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

    let report = thermal_report(&state, cutoff_ms, query.device.clone()).await;

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
