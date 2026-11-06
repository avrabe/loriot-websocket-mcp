//! API client for fetching sensor data from the server

use loriot_core::{AnalyticsReport, DeviceStats, SensorReading, TimeWindow};
use serde::{Deserialize, Serialize};

/// Base URL for API requests (relative to current host)
const API_BASE: &str = "/api";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DataRange {
    pub earliest_ts: Option<u64>,
    pub latest_ts: Option<u64>,
    pub total_readings: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedReadings {
    pub readings: Vec<SensorReading>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

/// Query parameters for readings endpoint
#[derive(Debug, Serialize)]
pub struct ReadingsQuery {
    pub time_window: Option<String>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    /// Start timestamp in milliseconds (overrides time_window if set)
    pub start_ts: Option<u64>,
    /// End timestamp in milliseconds (overrides time_window if set)
    pub end_ts: Option<u64>,
}

impl Default for ReadingsQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadingsQuery {
    pub fn new() -> Self {
        Self {
            time_window: None,
            page: None,
            page_size: None,
            start_ts: None,
            end_ts: None,
        }
    }

    pub fn with_time_window(mut self, window: &TimeWindow) -> Self {
        self.time_window = Some(time_window_to_string(window));
        self
    }

    /// Set explicit time range (overrides time_window)
    pub fn with_time_range(mut self, start: u64, end: u64) -> Self {
        self.start_ts = Some(start);
        self.end_ts = Some(end);
        self
    }

    pub fn with_page(mut self, page: usize) -> Self {
        self.page = Some(page);
        self
    }

    pub fn with_page_size(mut self, size: usize) -> Self {
        self.page_size = Some(size);
        self
    }
}

fn time_window_to_string(window: &TimeWindow) -> String {
    match window {
        TimeWindow::LastHour => "hour".to_string(),
        TimeWindow::Last6Hours => "6hours".to_string(),
        TimeWindow::LastDay => "day".to_string(),
        TimeWindow::LastWeek => "week".to_string(),
        TimeWindow::LastMonth => "month".to_string(),
        TimeWindow::All => "all".to_string(),
    }
}

/// Fetch the overall data range (earliest and latest timestamps)
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_data_range() -> Result<DataRange, String> {
    use gloo_net::http::Request;

    let url = format!("{}/data-range", API_BASE);
    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch paginated readings from the API
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_readings(query: ReadingsQuery) -> Result<PaginatedReadings, String> {
    use gloo_net::http::Request;

    let mut url = format!("{}/readings", API_BASE);

    let mut params = vec![];
    // Use explicit time range if set, otherwise fall back to time_window
    if let (Some(start), Some(end)) = (query.start_ts, query.end_ts) {
        params.push(format!("start_ts={}", start));
        params.push(format!("end_ts={}", end));
    } else if let Some(tw) = &query.time_window {
        params.push(format!("time_window={}", tw));
    }
    if let Some(page) = query.page {
        // API uses 0-based pages, UI uses 1-based
        params.push(format!("page={}", page.saturating_sub(1)));
    }
    if let Some(size) = query.page_size {
        params.push(format!("page_size={}", size));
    }

    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch all devices with their stats
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_devices(time_window: Option<&TimeWindow>) -> Result<Vec<DeviceStats>, String> {
    use gloo_net::http::Request;

    let mut url = format!("{}/devices", API_BASE);
    if let Some(tw) = time_window {
        url.push_str(&format!("?time_window={}", time_window_to_string(tw)));
    }

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch readings for a specific device
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_device_readings(
    eui: &str,
    query: ReadingsQuery,
) -> Result<Vec<SensorReading>, String> {
    use gloo_net::http::Request;

    let mut url = format!("{}/readings/{}", API_BASE, eui);

    let mut params = vec![];
    // Use explicit time range if set, otherwise fall back to time_window
    if let (Some(start), Some(end)) = (query.start_ts, query.end_ts) {
        params.push(format!("start_ts={}", start));
        params.push(format!("end_ts={}", end));
    } else if let Some(tw) = &query.time_window {
        params.push(format!("time_window={}", tw));
    }

    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Query parameters for analytics endpoint
#[derive(Debug, Serialize)]
pub struct AnalyticsQuery {
    pub time_window: Option<String>,
    /// Start timestamp in milliseconds (overrides time_window if set)
    pub start_ts: Option<u64>,
    /// End timestamp in milliseconds (overrides time_window if set)
    pub end_ts: Option<u64>,
}

impl Default for AnalyticsQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyticsQuery {
    pub fn new() -> Self {
        Self {
            time_window: None,
            start_ts: None,
            end_ts: None,
        }
    }

    pub fn with_time_window(mut self, window: &TimeWindow) -> Self {
        self.time_window = Some(time_window_to_string(window));
        self
    }

    /// Set explicit time range (overrides time_window)
    pub fn with_time_range(mut self, start: u64, end: u64) -> Self {
        self.start_ts = Some(start);
        self.end_ts = Some(end);
        self
    }
}

/// Fetch analytics report
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_analytics(
    device_eui: Option<&str>,
    query: AnalyticsQuery,
) -> Result<AnalyticsReport, String> {
    use gloo_net::http::Request;

    let base = match device_eui {
        Some(eui) => format!("{}/analytics/{}", API_BASE, eui),
        None => format!("{}/analytics", API_BASE),
    };

    let mut params = vec![];
    // Use explicit time range if set, otherwise fall back to time_window
    if let (Some(start), Some(end)) = (query.start_ts, query.end_ts) {
        params.push(format!("start_ts={}", start));
        params.push(format!("end_ts={}", end));
    } else if let Some(tw) = &query.time_window {
        params.push(format!("time_window={}", tw));
    }

    let url = if params.is_empty() {
        base
    } else {
        format!("{}?{}", base, params.join("&"))
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

// Server-side stubs (return empty data)
#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_data_range() -> Result<DataRange, String> {
    Ok(DataRange {
        earliest_ts: None,
        latest_ts: None,
        total_readings: 0,
    })
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_readings(_query: ReadingsQuery) -> Result<PaginatedReadings, String> {
    Ok(PaginatedReadings {
        readings: vec![],
        total: 0,
        page: 0,
        page_size: 50,
    })
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_devices(_time_window: Option<&TimeWindow>) -> Result<Vec<DeviceStats>, String> {
    Ok(vec![])
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_device_readings(
    _eui: &str,
    _query: ReadingsQuery,
) -> Result<Vec<SensorReading>, String> {
    Ok(vec![])
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_analytics(
    _device_eui: Option<&str>,
    _query: AnalyticsQuery,
) -> Result<AnalyticsReport, String> {
    Err("Server-side rendering".to_string())
}

// ============================================================================
// Weather API Types and Functions
// ============================================================================

/// Weather info response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherInfo {
    pub location: Option<CoordinatesResponse>,
    pub station: Option<StationResponse>,
    pub station_distance_km: Option<f64>,
    pub data_range: WeatherDataRange,
    pub current: Option<DailyWeather>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatesResponse {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationResponse {
    pub id: String,
    pub name: String,
    pub state: Option<String>,
    pub coordinates: CoordinatesResponse,
    pub elevation: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherDataRange {
    pub earliest_date: Option<String>,
    pub latest_date: Option<String>,
    pub total_records: usize,
    pub cached_records: usize,
}

/// Daily weather data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyWeather {
    pub date: String,
    pub station_id: String,
    pub station_name: Option<String>,
    pub temp_mean: Option<f32>,
    pub temp_max: Option<f32>,
    pub temp_min: Option<f32>,
    pub humidity: Option<f32>,
    pub precipitation: Option<f32>,
    pub sunshine_hours: Option<f32>,
}

/// Outdoor weather summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdoorSummary {
    pub date: String,
    pub station_name: String,
    pub station_distance_km: f64,
    pub temp_mean: Option<f32>,
    pub temp_max: Option<f32>,
    pub temp_min: Option<f32>,
    pub humidity: Option<f32>,
    pub precipitation: Option<f32>,
}

/// Weather statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherStatistics {
    pub days: usize,
    pub temperature: TempStatistics,
    pub humidity: Option<HumidityStatistics>,
    pub precipitation_total: Option<f32>,
    pub sunshine_total: Option<f32>,
    pub heating_degree_days: f32,
    pub cooling_degree_days: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempStatistics {
    pub mean: f32,
    pub min: f32,
    pub max: f32,
    pub std_dev: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumidityStatistics {
    pub mean: f32,
    pub min: f32,
    pub max: f32,
}

/// Geocoding response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeocodingResponse {
    pub results: Vec<GeocodingResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeocodingResult {
    pub display_name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Stations response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationsResponse {
    pub stations: Vec<StationWithDistance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationWithDistance {
    pub station: StationResponse,
    pub distance_km: Option<f64>,
}

/// Fetch weather info
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_weather_info() -> Result<WeatherInfo, String> {
    use gloo_net::http::Request;

    let url = format!("{}/weather/info", API_BASE);
    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch current weather
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_current_weather() -> Result<OutdoorSummary, String> {
    use gloo_net::http::Request;

    let url = format!("{}/weather/current", API_BASE);
    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch weather history
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_weather_history(days: Option<usize>) -> Result<Vec<OutdoorSummary>, String> {
    use gloo_net::http::Request;

    let url = match days {
        Some(d) => format!("{}/weather/history?days={}", API_BASE, d),
        None => format!("{}/weather/history", API_BASE),
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch weather statistics
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_weather_statistics() -> Result<WeatherStatistics, String> {
    use gloo_net::http::Request;

    let url = format!("{}/weather/statistics", API_BASE);
    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Geocode an address
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn geocode_address(address: &str) -> Result<GeocodingResponse, String> {
    use gloo_net::http::Request;

    let encoded = js_sys::encode_uri_component(address);
    let url = format!("{}/weather/geocode?address={}", API_BASE, encoded);

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch available stations
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_stations(
    lat: Option<f64>,
    lon: Option<f64>,
) -> Result<StationsResponse, String> {
    use gloo_net::http::Request;

    let url = match (lat, lon) {
        (Some(lat), Some(lon)) => format!("{}/weather/stations?lat={}&lon={}", API_BASE, lat, lon),
        _ => format!("{}/weather/stations", API_BASE),
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Set current location
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn set_weather_location(latitude: f64, longitude: f64) -> Result<WeatherInfo, String> {
    use gloo_net::http::Request;

    let url = format!("{}/weather/location", API_BASE);
    let body = serde_json::json!({
        "latitude": latitude,
        "longitude": longitude
    });

    let response = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Trigger weather data fetch from DWD
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_dwd_data() -> Result<Vec<DailyWeather>, String> {
    use gloo_net::http::Request;

    let url = format!("{}/weather/fetch", API_BASE);

    let response = Request::post(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

// Server-side stubs for weather API
#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_weather_info() -> Result<WeatherInfo, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_current_weather() -> Result<OutdoorSummary, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_weather_history(_days: Option<usize>) -> Result<Vec<OutdoorSummary>, String> {
    Ok(vec![])
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_weather_statistics() -> Result<WeatherStatistics, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn geocode_address(_address: &str) -> Result<GeocodingResponse, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_stations(
    _lat: Option<f64>,
    _lon: Option<f64>,
) -> Result<StationsResponse, String> {
    Ok(StationsResponse { stations: vec![] })
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn set_weather_location(_latitude: f64, _longitude: f64) -> Result<WeatherInfo, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_dwd_data() -> Result<Vec<DailyWeather>, String> {
    Err("Server-side rendering".to_string())
}

// ============================================================================
// Heat Pump API Types and Functions
// ============================================================================

/// Heat pump info response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatPumpInfo {
    pub config: HeatPumpConfigResponse,
    pub has_data: bool,
    pub source_device_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatPumpConfigResponse {
    pub source_devices: Vec<String>,
    pub device_names: std::collections::HashMap<String, String>,
    pub heating_target: f32,
    pub cooling_target: f32,
}

/// Device option for configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceOption {
    pub eui: String,
    pub name: String,
    pub reading_count: usize,
    pub avg_temperature: Option<f32>,
}

/// Heat pump report (matches loriot_core::heatpump::HeatPumpReport)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatPumpReport {
    pub period: AnalysisPeriod,
    pub daily: Vec<DailyHeatPumpAnalysis>,
    pub overall: OverallStats,
    pub device_stats: std::collections::HashMap<String, DeviceSummary>,
    pub seasonal: SeasonalStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisPeriod {
    pub start_date: String,
    pub end_date: String,
    pub days: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyHeatPumpAnalysis {
    pub date: String,
    pub season: String,
    pub outdoor_temp: Option<f32>,
    pub source_temp: Option<f32>,
    pub temp_advantage: Option<f32>,
    pub heating_cop_estimate: Option<f32>,
    pub cooling_cop_estimate: Option<f32>,
    pub heating_degree_days: f32,
    pub cooling_degree_days: f32,
    pub source_readings: usize,
    pub device_temps: std::collections::HashMap<String, SourceDeviceStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDeviceStats {
    pub name: String,
    pub avg_temp: f32,
    pub min_temp: f32,
    pub max_temp: f32,
    pub avg_humidity: Option<f32>,
    pub readings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverallStats {
    pub avg_source_temp: f32,
    pub avg_outdoor_temp: Option<f32>,
    pub avg_temp_advantage: Option<f32>,
    pub avg_heating_cop: Option<f32>,
    pub avg_cooling_cop: Option<f32>,
    pub total_heating_degree_days: f32,
    pub total_cooling_degree_days: f32,
    pub energy_savings_estimate: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub name: String,
    pub avg_temp: f32,
    pub min_temp: f32,
    pub max_temp: f32,
    pub avg_humidity: Option<f32>,
    pub total_readings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonalStats {
    pub winter: Option<SeasonSummary>,
    pub spring: Option<SeasonSummary>,
    pub summer: Option<SeasonSummary>,
    pub autumn: Option<SeasonSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonSummary {
    pub days: usize,
    pub avg_source_temp: f32,
    pub avg_outdoor_temp: Option<f32>,
    pub avg_temp_advantage: Option<f32>,
    pub avg_cop: Option<f32>,
}

/// Configure heat pump request
#[derive(Debug, Clone, Serialize)]
pub struct ConfigureHeatPumpRequest {
    pub source_devices: Vec<String>,
    pub device_names: Option<std::collections::HashMap<String, String>>,
    pub heating_target: Option<f32>,
    pub cooling_target: Option<f32>,
}

/// Fetch heat pump info
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_heatpump_info() -> Result<HeatPumpInfo, String> {
    use gloo_net::http::Request;

    let url = format!("{}/heatpump/info", API_BASE);
    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch available devices for heat pump configuration
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_heatpump_devices() -> Result<Vec<DeviceOption>, String> {
    use gloo_net::http::Request;

    let url = format!("{}/heatpump/devices", API_BASE);
    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Configure heat pump
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn configure_heatpump(config: ConfigureHeatPumpRequest) -> Result<HeatPumpInfo, String> {
    use gloo_net::http::Request;

    let url = format!("{}/heatpump/configure", API_BASE);
    let body = serde_json::to_string(&config).map_err(|e| e.to_string())?;

    let response = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Analyze heat pump efficiency
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn analyze_heatpump(days: Option<usize>) -> Result<HeatPumpReport, String> {
    use gloo_net::http::Request;

    let url = match days {
        Some(d) => format!("{}/heatpump/analyze?days={}", API_BASE, d),
        None => format!("{}/heatpump/analyze", API_BASE),
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

// Server-side stubs for heat pump API
#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_heatpump_info() -> Result<HeatPumpInfo, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_heatpump_devices() -> Result<Vec<DeviceOption>, String> {
    Ok(vec![])
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn configure_heatpump(_config: ConfigureHeatPumpRequest) -> Result<HeatPumpInfo, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn analyze_heatpump(_days: Option<usize>) -> Result<HeatPumpReport, String> {
    Err("Server-side rendering".to_string())
}

// ============================================================================
// Energy Tracker API Types and Functions
// ============================================================================

/// Building profile
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildingProfile {
    pub area_m2: f64,
    pub units: u32,
    pub year_built: u32,
    pub heating_system: String,
    pub distribution: String,
    pub notes: Option<String>,
}

/// Energy prices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyPrices {
    pub gas_ct_kwh: f64,
    pub electricity_ct_kwh: f64,
    pub electricity_hp_ct_kwh: f64,
}

impl Default for EnergyPrices {
    fn default() -> Self {
        Self {
            gas_ct_kwh: 12.0,
            electricity_ct_kwh: 35.0,
            electricity_hp_ct_kwh: 28.0,
        }
    }
}

/// Apartment/Unit in the building
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApartmentUnit {
    pub id: u32,
    pub name: String,            // e.g., "EG links", "1.OG rechts", "WE 3"
    pub area_m2: f64,            // Living area
    pub distribution_units: f64, // Heizkostenverteiler-Einheiten
    pub is_reference: bool,      // Is this the reference unit with consumption data?
    pub notes: Option<String>,
}

/// Cost distribution settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostDistributionSettings {
    pub base_cost_percent: f64, // Grundkostenanteil (typically 30-50%)
                                // Verbrauchskostenanteil = 100 - base_cost_percent
}

impl Default for CostDistributionSettings {
    fn default() -> Self {
        Self {
            base_cost_percent: 30.0, // Standard: 30% Grundkosten, 70% Verbrauch
        }
    }
}

/// Cost breakdown per apartment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApartmentCostBreakdown {
    pub apartment_id: u32,
    pub name: String,
    pub area_m2: f64,
    pub area_share_percent: f64, // Share of total area
    pub distribution_units: f64,
    pub consumption_share_percent: f64, // Share of total consumption
    pub base_cost_eur: f64,             // Grundkosten-Anteil
    pub consumption_cost_eur: f64,      // Verbrauchskosten-Anteil
    pub total_cost_eur: f64,
    pub cost_per_m2: f64,
}

/// Building cost summary with apartment breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildingCostSummary {
    pub total_cost_eur: f64,
    pub total_base_cost_eur: f64,
    pub total_consumption_cost_eur: f64,
    pub apartments: Vec<ApartmentCostBreakdown>,
}

/// Update apartments request
#[derive(Debug, Clone, Serialize)]
pub struct UpdateApartmentsRequest {
    pub apartments: Vec<ApartmentUnit>,
    pub cost_distribution: CostDistributionSettings,
}

/// Monthly consumption record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyConsumption {
    pub year: u32,
    pub month: u32,
    pub heating_kwh: f64,
    pub hot_water_kwh: f64,
    pub source: String,
    pub notes: Option<String>,
}

/// Yearly total
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearlyTotal {
    pub year: u32,
    pub heating_kwh: f64,
    pub hot_water_kwh: f64,
    pub total_kwh: f64,
    pub months_with_data: usize,
}

/// Energy overview response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyOverview {
    pub building: BuildingProfile,
    pub prices: EnergyPrices,
    pub apartments: Vec<ApartmentUnit>,
    pub cost_distribution: CostDistributionSettings,
    pub consumption_count: usize,
    pub available_years: Vec<u32>,
    pub latest_year: Option<u32>,
    pub yearly_totals: Vec<YearlyTotal>,
}

/// Scenario analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioAnalysis {
    pub annual_cost_eur: f64,
    pub annual_co2_kg: f64,
    pub electricity_kwh: f64,
    pub gas_kwh: f64,
    pub avg_cop: f64,
    pub hp_share: f64,
}

/// Monthly scenario breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyScenario {
    pub month: u32,
    pub month_name: String,
    pub heating_kwh: f64,
    pub hot_water_kwh: f64,
    pub hp_suitable: bool,
    pub estimated_cop: f64,
    pub hp_electricity_kwh: f64,
    pub gas_kwh: f64,
    pub cost_current_eur: f64,
    pub cost_hp_eur: f64,
    pub savings_eur: f64,
}

/// Scenario comparison response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioComparison {
    pub year: u32,
    pub current: ScenarioAnalysis,
    pub hybrid: ScenarioAnalysis,
    pub full_hp: ScenarioAnalysis,
    pub monthly: Vec<MonthlyScenario>,
}

/// Consumption response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumptionResponse {
    pub consumption: Vec<MonthlyConsumption>,
}

/// Update building request
#[derive(Debug, Clone, Serialize)]
pub struct UpdateBuildingRequest {
    pub area_m2: f64,
    pub units: u32,
    pub year_built: u32,
    pub heating_system: String,
    pub distribution: String,
    pub notes: Option<String>,
}

/// Update prices request
#[derive(Debug, Clone, Serialize)]
pub struct UpdatePricesRequest {
    pub gas_ct_kwh: f64,
    pub electricity_ct_kwh: f64,
    pub electricity_hp_ct_kwh: f64,
}

/// Add consumption request
#[derive(Debug, Clone, Serialize)]
pub struct AddConsumptionRequest {
    pub year: u32,
    pub month: u32,
    pub heating_kwh: f64,
    pub hot_water_kwh: f64,
    pub source: Option<String>,
    pub notes: Option<String>,
}

/// Delete consumption request
#[derive(Debug, Clone, Serialize)]
pub struct DeleteConsumptionRequest {
    pub year: u32,
    pub month: u32,
}

/// Fetch energy overview
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_energy_overview() -> Result<EnergyOverview, String> {
    use gloo_net::http::Request;

    let url = format!("{}/energy/overview", API_BASE);
    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch consumption data
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_energy_consumption() -> Result<ConsumptionResponse, String> {
    use gloo_net::http::Request;

    let url = format!("{}/energy/consumption", API_BASE);
    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch scenario comparison
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_energy_scenarios(year: Option<u32>) -> Result<ScenarioComparison, String> {
    use gloo_net::http::Request;

    let url = match year {
        Some(y) => format!("{}/energy/scenarios?year={}", API_BASE, y),
        None => format!("{}/energy/scenarios", API_BASE),
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Update building profile
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn update_energy_building(
    request: UpdateBuildingRequest,
) -> Result<BuildingProfile, String> {
    use gloo_net::http::Request;

    let url = format!("{}/energy/building", API_BASE);
    let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    let response = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Update energy prices
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn update_energy_prices(request: UpdatePricesRequest) -> Result<EnergyPrices, String> {
    use gloo_net::http::Request;

    let url = format!("{}/energy/prices", API_BASE);
    let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    let response = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Update apartments
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn update_energy_apartments(
    request: UpdateApartmentsRequest,
) -> Result<Vec<ApartmentUnit>, String> {
    use gloo_net::http::Request;

    let url = format!("{}/energy/apartments", API_BASE);
    let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    let response = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Add consumption record
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn add_energy_consumption(
    request: AddConsumptionRequest,
) -> Result<MonthlyConsumption, String> {
    use gloo_net::http::Request;

    let url = format!("{}/energy/consumption", API_BASE);
    let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    let response = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Delete consumption record
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn delete_energy_consumption(request: DeleteConsumptionRequest) -> Result<(), String> {
    use gloo_net::http::Request;

    let url = format!("{}/energy/consumption", API_BASE);
    let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    let response = Request::delete(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.ok() {
        Ok(())
    } else {
        Err(format!("Delete failed: {}", response.status()))
    }
}

// Server-side stubs for energy API
#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_energy_overview() -> Result<EnergyOverview, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_energy_consumption() -> Result<ConsumptionResponse, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_energy_scenarios(_year: Option<u32>) -> Result<ScenarioComparison, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn update_energy_building(
    _request: UpdateBuildingRequest,
) -> Result<BuildingProfile, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn update_energy_prices(_request: UpdatePricesRequest) -> Result<EnergyPrices, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn update_energy_apartments(
    _request: UpdateApartmentsRequest,
) -> Result<Vec<ApartmentUnit>, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn add_energy_consumption(
    _request: AddConsumptionRequest,
) -> Result<MonthlyConsumption, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn delete_energy_consumption(_request: DeleteConsumptionRequest) -> Result<(), String> {
    Err("Server-side rendering".to_string())
}

// ============================================================================
// Thermal Analysis API Types and Functions
// ============================================================================

/// Thermal analysis report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalAnalysisReport {
    pub period: ThermalPeriod,
    pub device_analysis: Vec<DeviceThermalAnalysis>,
    pub building_metrics: BuildingThermalMetrics,
    pub heating_degree_days: f64,
    pub cooling_degree_days: f64,
    pub data_quality: ThermalDataQuality,
}

/// Analysis period information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalPeriod {
    pub start_date: String,
    pub end_date: String,
    pub days_with_data: usize,
    pub total_readings: usize,
    pub weather_records: usize,
}

/// Thermal analysis for a single device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceThermalAnalysis {
    pub device_id: String,
    pub device_name: Option<String>,
    pub avg_indoor_temp: f32,
    pub indoor_temp_range: (f32, f32),
    pub temp_stability: f32,
    pub outdoor_correlation: f32,
    pub damping_factor: f32,
    pub thermal_lag_hours: Option<f32>,
    pub avg_temp_difference: f32,
    pub daily_profiles: Vec<DailyThermalProfile>,
}

/// Daily thermal profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyThermalProfile {
    pub date: String,
    pub avg_indoor_temp: f32,
    pub indoor_range: (f32, f32),
    pub outdoor_temp: Option<f32>,
    pub outdoor_range: Option<(f32, f32)>,
    pub temp_difference: Option<f32>,
    pub reading_count: usize,
}

/// Building-wide thermal metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildingThermalMetrics {
    pub avg_indoor_temp: f32,
    pub avg_outdoor_temp: Option<f32>,
    pub avg_temp_difference: Option<f32>,
    pub stability_score: f32,
    pub insulation_rating: String,
    pub energy_demand_indicator: f32,
    pub uniformity_score: f32,
}

/// Data quality indicators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalDataQuality {
    pub coverage_percent: f32,
    pub avg_readings_per_day: f32,
    pub devices_with_data: usize,
    pub weather_data_available: bool,
    pub quality_rating: String,
}

/// Thermal summary (quick overview)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalSummary {
    pub avg_indoor_temp: f32,
    pub avg_outdoor_temp: Option<f32>,
    pub temp_difference: Option<f32>,
    pub insulation_rating: String,
    pub insulation_description: String,
    pub heating_degree_days: f64,
    pub devices_analyzed: usize,
    pub data_quality: String,
}

/// Fetch full thermal analysis
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_thermal_analysis(
    days: Option<usize>,
    device: Option<&str>,
) -> Result<ThermalAnalysisReport, String> {
    use gloo_net::http::Request;

    let mut params = Vec::new();
    if let Some(d) = days {
        params.push(format!("days={}", d));
    }
    if let Some(dev) = device {
        params.push(format!("device={}", dev));
    }

    let url = if params.is_empty() {
        format!("{}/thermal/analyze", API_BASE)
    } else {
        format!("{}/thermal/analyze?{}", API_BASE, params.join("&"))
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

/// Fetch thermal summary
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_thermal_summary(days: Option<usize>) -> Result<ThermalSummary, String> {
    use gloo_net::http::Request;

    let url = match days {
        Some(d) => format!("{}/thermal/summary?days={}", API_BASE, d),
        None => format!("{}/thermal/summary", API_BASE),
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

// Server-side stubs for thermal API
#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_thermal_analysis(
    _days: Option<usize>,
    _device: Option<&str>,
) -> Result<ThermalAnalysisReport, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_thermal_summary(_days: Option<usize>) -> Result<ThermalSummary, String> {
    Err("Server-side rendering".to_string())
}

// ============================================================================
// Weather Forecast API Types and Functions
// ============================================================================

/// Forecast response with room predictions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastResponse {
    pub weather: WeatherForecastSummary,
    pub rooms: Vec<RoomForecast>,
    pub generated_at: u64,
}

/// Weather forecast summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherForecastSummary {
    pub coordinates: ForecastCoordinates,
    pub timezone: String,
    pub daily: Vec<DailyForecastSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastCoordinates {
    pub latitude: f64,
    pub longitude: f64,
}

/// Daily forecast summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyForecastSummary {
    pub date: String,
    pub temp_min: f32,
    pub temp_max: f32,
    pub temp_mean: f32,
    pub precipitation: f32,
    pub humidity: f32,
}

/// Room temperature forecast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomForecast {
    pub device_id: String,
    pub device_name: Option<String>,
    pub daily_predictions: Vec<DailyRoomPrediction>,
    pub model: ThermalModel,
}

/// Daily room prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyRoomPrediction {
    pub date: String,
    pub predicted_temp: f32,
    pub predicted_range: (f32, f32),
    pub outdoor_temp: f32,
    pub outdoor_range: (f32, f32),
    pub confidence: f32,
}

/// Thermal model parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalModel {
    pub damping_factor: f32,
    pub outdoor_correlation: f32,
    pub thermal_lag_hours: f32,
    pub base_offset: f32,
    pub training_days: usize,
}

/// Fetch weather forecast only
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_weather_forecast(days: Option<u8>) -> Result<WeatherForecastSummary, String> {
    use gloo_net::http::Request;

    let url = match days {
        Some(d) => format!("{}/forecast?days={}", API_BASE, d),
        None => format!("{}/forecast", API_BASE),
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    // The API returns a tagged enum with status field
    let result: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

    if result.get("status").and_then(|s| s.as_str()) == Some("error") {
        let error = result
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown error");
        return Err(error.to_string());
    }

    serde_json::from_value(result).map_err(|e| e.to_string())
}

/// Fetch room temperature predictions
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub async fn fetch_room_forecast(
    days: Option<u8>,
    device: Option<&str>,
    history_days: Option<usize>,
) -> Result<ForecastResponse, String> {
    use gloo_net::http::Request;

    let mut params = Vec::new();
    if let Some(d) = days {
        params.push(format!("days={}", d));
    }
    if let Some(dev) = device {
        params.push(format!("device={}", dev));
    }
    if let Some(h) = history_days {
        params.push(format!("history_days={}", h));
    }

    let url = if params.is_empty() {
        format!("{}/forecast/rooms", API_BASE)
    } else {
        format!("{}/forecast/rooms?{}", API_BASE, params.join("&"))
    };

    let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;

    response.json().await.map_err(|e| e.to_string())
}

// Server-side stubs for forecast API
#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_weather_forecast(_days: Option<u8>) -> Result<WeatherForecastSummary, String> {
    Err("Server-side rendering".to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
pub async fn fetch_room_forecast(
    _days: Option<u8>,
    _device: Option<&str>,
    _history_days: Option<usize>,
) -> Result<ForecastResponse, String> {
    Err("Server-side rendering".to_string())
}
