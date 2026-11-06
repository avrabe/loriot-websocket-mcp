//! Weather Forecast and Room Temperature Prediction API
//!
//! Provides endpoints for fetching weather forecasts and predicting
//! indoor room temperatures based on thermal analysis.

use axum::{
    extract::{Query, State},
    Json,
};
use loriot_core::thermal::ThermalAnalyzer;
use loriot_core::weather::forecast::{
    DailyForecast, ForecastFetcher, RoomForecast, RoomPredictor, ThermalModel, WeatherForecast,
};
use loriot_core::weather::Coordinates;
use serde::{Deserialize, Serialize};

use crate::weather::SharedWeatherState;
use crate::AppState;

/// Combined state for forecast endpoints
#[derive(Clone)]
pub struct ForecastState {
    pub app: AppState,
    pub weather: SharedWeatherState,
}

/// Query parameters for forecast requests
#[derive(Debug, Deserialize)]
pub struct ForecastQuery {
    /// Number of forecast days (1-16, default: 7)
    pub days: Option<u8>,
}

/// Query parameters for room prediction
#[derive(Debug, Deserialize)]
pub struct RoomPredictionQuery {
    /// Number of forecast days (1-16, default: 7)
    pub days: Option<u8>,
    /// Specific device EUI (optional, predicts all if not specified)
    pub device: Option<String>,
    /// Number of historical days to use for model (default: 90)
    pub history_days: Option<usize>,
}

/// Full forecast response with room predictions
#[derive(Debug, Serialize)]
pub struct ForecastResponse {
    /// Weather forecast data
    pub weather: WeatherForecastSummary,
    /// Room temperature predictions
    pub rooms: Vec<RoomForecast>,
    /// Timestamp when forecast was generated
    pub generated_at: u64,
}

/// Summary of weather forecast for API response
#[derive(Debug, Serialize)]
pub struct WeatherForecastSummary {
    /// Location coordinates used
    pub coordinates: Coordinates,
    /// Timezone
    pub timezone: String,
    /// Daily forecasts
    pub daily: Vec<DailyForecastSummary>,
}

/// Simplified daily forecast for API
#[derive(Debug, Serialize)]
pub struct DailyForecastSummary {
    pub date: String,
    pub temp_min: f32,
    pub temp_max: f32,
    pub temp_mean: f32,
    pub precipitation: f32,
    pub humidity: f32,
}

impl From<&DailyForecast> for DailyForecastSummary {
    fn from(f: &DailyForecast) -> Self {
        Self {
            date: f.date.clone(),
            temp_min: f.temp_min,
            temp_max: f.temp_max,
            temp_mean: f.temp_mean,
            precipitation: f.precipitation,
            humidity: f.humidity,
        }
    }
}

/// GET /api/forecast - Get weather forecast only
pub async fn get_weather_forecast(
    State(state): State<ForecastState>,
    Query(query): Query<ForecastQuery>,
) -> Json<WeatherForecastResult> {
    let days = query.days.unwrap_or(7).clamp(1, 16);

    // Get coordinates from weather state
    let weather = state.weather.read().await;
    let coords = weather
        .location
        .unwrap_or_else(|| Coordinates::new(51.4818, 7.2162)); // Default to Bochum
    drop(weather);

    let fetcher = ForecastFetcher::new(days);
    match fetcher.fetch(&coords).await {
        Ok(forecast) => Json(WeatherForecastResult::Success(WeatherForecastSummary {
            coordinates: forecast.coordinates,
            timezone: forecast.timezone,
            daily: forecast.daily.iter().map(|d| d.into()).collect(),
        })),
        Err(e) => Json(WeatherForecastResult::Error(e.to_string())),
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "status")]
pub enum WeatherForecastResult {
    #[serde(rename = "success")]
    Success(WeatherForecastSummary),
    #[serde(rename = "error")]
    Error(String),
}

/// GET /api/forecast/rooms - Get room temperature predictions
pub async fn get_room_predictions(
    State(state): State<ForecastState>,
    Query(query): Query<RoomPredictionQuery>,
) -> Json<ForecastResponse> {
    let days = query.days.unwrap_or(7).clamp(1, 16);
    let history_days = query.history_days.unwrap_or(90);
    let cutoff_ms =
        loriot_core::current_timestamp_ms() - (history_days as u64 * 24 * 60 * 60 * 1000);

    // Get coordinates from weather state
    let weather_lock = state.weather.read().await;
    let coords = weather_lock
        .location
        .unwrap_or_else(|| Coordinates::new(51.4818, 7.2162));
    let weather_records = weather_lock
        .storage
        .read_all_from_disk()
        .unwrap_or_default();
    let weather_data: Vec<_> = weather_records.iter().map(|r| r.weather.clone()).collect();
    drop(weather_lock);

    // Get sensor readings for thermal analysis
    let storage = state.app.storage.read().await;
    let readings = storage
        .read_from_disk_filtered(Some(cutoff_ms), query.device.as_deref())
        .unwrap_or_default();
    drop(storage);

    // Run thermal analysis to get device characteristics
    let analyzer = ThermalAnalyzer::new();
    let thermal_report = analyzer.analyze(&readings, &weather_data);

    // Fetch weather forecast
    let fetcher = ForecastFetcher::new(days);
    let forecast = fetcher
        .fetch(&coords)
        .await
        .unwrap_or_else(|_| WeatherForecast {
            coordinates: coords,
            timezone: "Europe/Berlin".to_string(),
            generation_time_ms: 0.0,
            daily: vec![],
            hourly: vec![],
        });

    // Generate room predictions for each analyzed device
    let rooms: Vec<RoomForecast> = thermal_report
        .device_analysis
        .iter()
        .filter(|d| {
            // Filter by device if specified
            query
                .device
                .as_ref()
                .map(|dev| d.device_id == *dev)
                .unwrap_or(true)
        })
        .map(|device| {
            // Build thermal model from analysis
            let model = RoomPredictor::build_model(
                device.damping_factor,
                device.outdoor_correlation,
                device.avg_temp_difference,
                device.daily_profiles.len(),
            );

            // Generate predictions
            RoomPredictor::predict(
                &forecast,
                &device.device_id,
                device.device_name.clone(),
                &model,
            )
        })
        .collect();

    Json(ForecastResponse {
        weather: WeatherForecastSummary {
            coordinates: forecast.coordinates,
            timezone: forecast.timezone,
            daily: forecast.daily.iter().map(|d| d.into()).collect(),
        },
        rooms,
        generated_at: loriot_core::current_timestamp_ms(),
    })
}

/// GET /api/forecast/models - Get thermal models for all devices
pub async fn get_thermal_models(
    State(state): State<ForecastState>,
    Query(query): Query<RoomPredictionQuery>,
) -> Json<Vec<DeviceThermalModel>> {
    let history_days = query.history_days.unwrap_or(90);
    let cutoff_ms =
        loriot_core::current_timestamp_ms() - (history_days as u64 * 24 * 60 * 60 * 1000);

    // Get weather data
    let weather_lock = state.weather.read().await;
    let weather_records = weather_lock
        .storage
        .read_all_from_disk()
        .unwrap_or_default();
    let weather_data: Vec<_> = weather_records.iter().map(|r| r.weather.clone()).collect();
    drop(weather_lock);

    // Get sensor readings
    let storage = state.app.storage.read().await;
    let readings = storage
        .read_from_disk_filtered(Some(cutoff_ms), query.device.as_deref())
        .unwrap_or_default();
    drop(storage);

    // Run thermal analysis
    let analyzer = ThermalAnalyzer::new();
    let thermal_report = analyzer.analyze(&readings, &weather_data);

    // Return models for each device
    thermal_report
        .device_analysis
        .iter()
        .map(|d| {
            let model = RoomPredictor::build_model(
                d.damping_factor,
                d.outdoor_correlation,
                d.avg_temp_difference,
                d.daily_profiles.len(),
            );
            DeviceThermalModel {
                device_id: d.device_id.clone(),
                device_name: d.device_name.clone(),
                model,
                avg_indoor_temp: d.avg_indoor_temp,
                indoor_temp_range: d.indoor_temp_range,
            }
        })
        .collect::<Vec<_>>()
        .into()
}

/// Device thermal model for API response
#[derive(Debug, Serialize)]
pub struct DeviceThermalModel {
    pub device_id: String,
    pub device_name: Option<String>,
    pub model: ThermalModel,
    pub avg_indoor_temp: f32,
    pub indoor_temp_range: (f32, f32),
}
