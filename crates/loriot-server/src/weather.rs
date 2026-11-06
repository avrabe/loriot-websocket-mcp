//! Weather API handlers

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use loriot_core::weather::{
    dwd::{DwdFetcher, WeatherStatistics},
    geocoding::GeocodingService,
    station::{get_default_stations, StationFetcher, WeatherStation},
    storage::{OutdoorSummary, WeatherRecord, WeatherStorage},
    Coordinates, DailyWeather, WeatherConfig,
};
use serde::{Deserialize, Serialize};

use crate::state::run_blocking;
use std::sync::Arc;
use tokio::sync::RwLock;

const LOCATION_CONFIG_PATH: &str = "weather_location.json";

/// Saved location configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedLocation {
    latitude: f64,
    longitude: f64,
    station_id: Option<String>,
    station_name: Option<String>,
}

impl SavedLocation {
    fn load() -> Option<Self> {
        let content = std::fs::read_to_string(LOCATION_CONFIG_PATH).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn save(&self) -> Result<(), std::io::Error> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(LOCATION_CONFIG_PATH, content)
    }
}

/// Weather service state
pub struct WeatherState {
    /// Weather storage
    pub storage: WeatherStorage,
    /// DWD fetcher
    pub fetcher: DwdFetcher,
    /// Geocoding service
    pub geocoder: GeocodingService,
    /// Station fetcher with caching
    pub station_fetcher: StationFetcher,
    /// Current location coordinates
    pub location: Option<Coordinates>,
    /// Current nearest station
    pub nearest_station: Option<WeatherStation>,
}

impl WeatherState {
    /// Create a new weather state with default configuration
    pub fn new() -> Self {
        let config = WeatherConfig::default();
        let mut storage = WeatherStorage::new(config);

        // Load existing weather data from disk
        if let Err(e) = storage.load() {
            tracing::warn!("Failed to load weather data: {}", e);
        } else {
            tracing::info!(
                "Loaded {} weather records from storage",
                storage.total_count()
            );
        }

        // Load saved location
        let (location, nearest_station) = if let Some(saved) = SavedLocation::load() {
            tracing::info!(
                "Loaded saved location: {:.4}, {:.4} (station: {})",
                saved.latitude,
                saved.longitude,
                saved.station_name.as_deref().unwrap_or("unknown")
            );
            let coords = Coordinates::new(saved.latitude, saved.longitude);
            let station = if let (Some(id), Some(name)) = (saved.station_id, saved.station_name) {
                // Reconstruct station from saved data
                Some(WeatherStation {
                    id,
                    name,
                    state: None,
                    coordinates: coords, // Approximate - will be corrected on next fetch
                    elevation: 0.0,
                    start_date: String::new(),
                    end_date: String::new(),
                    status: loriot_core::weather::station::StationStatus::Active,
                })
            } else {
                // Find nearest from defaults
                get_default_stations().into_iter().min_by(|a, b| {
                    let dist_a = a.coordinates.distance_to(&coords);
                    let dist_b = b.coordinates.distance_to(&coords);
                    dist_a
                        .partial_cmp(&dist_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            };
            (Some(coords), station)
        } else {
            (None, None)
        };

        Self {
            storage,
            fetcher: DwdFetcher::new(),
            geocoder: GeocodingService::new(),
            station_fetcher: StationFetcher::new(),
            location,
            nearest_station,
        }
    }

    /// Set location coordinates (does not find station - call find_nearest_station separately)
    #[allow(dead_code)] // Reserved for future granular location API
    pub fn set_location_coords(&mut self, coords: Coordinates) {
        self.location = Some(coords);
    }

    /// Set the nearest station
    #[allow(dead_code)] // Reserved for future manual station selection
    pub fn set_nearest_station(&mut self, station: WeatherStation) {
        self.nearest_station = Some(station);
    }

    /// Set location and find nearest station from defaults (sync fallback)
    pub fn set_location(&mut self, coords: Coordinates) {
        self.location = Some(coords);
        // Find nearest station from defaults (fallback for sync contexts)
        let default_stations = get_default_stations();
        self.nearest_station = default_stations.into_iter().min_by(|a, b| {
            let dist_a = a.coordinates.distance_to(&coords);
            let dist_b = b.coordinates.distance_to(&coords);
            dist_a
                .partial_cmp(&dist_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

impl Default for WeatherState {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for shared weather state
pub type SharedWeatherState = Arc<RwLock<WeatherState>>;

/// Path of the weather history file, taken out of the lock.
pub async fn weather_history_path(state: &SharedWeatherState) -> String {
    state.read().await.storage.storage_path().to_string()
}

/// Daily weather from the history file at `path`. Blocking file IO.
pub fn load_daily_weather(path: &str) -> Vec<DailyWeather> {
    loriot_core::weather::storage::read_records(path)
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.weather)
        .collect()
}

/// Query parameters for weather endpoints
#[derive(Debug, Deserialize)]
pub struct WeatherQuery {
    /// Start date (YYYYMMDD)
    pub start_date: Option<String>,
    /// End date (YYYYMMDD)
    pub end_date: Option<String>,
    /// Number of days to return
    pub days: Option<usize>,
}

/// Query for geocoding
#[derive(Debug, Deserialize)]
pub struct GeocodingQuery {
    /// Address to geocode
    pub address: String,
}

/// Query for station search
#[derive(Debug, Deserialize)]
pub struct StationQuery {
    /// Latitude
    pub lat: Option<f64>,
    /// Longitude
    pub lon: Option<f64>,
    /// Search by name
    pub name: Option<String>,
    /// Number of stations to return
    pub limit: Option<usize>,
}

/// Response for weather info
#[derive(Debug, Serialize)]
pub struct WeatherInfo {
    /// Current location
    pub location: Option<CoordinatesResponse>,
    /// Nearest station
    pub station: Option<StationResponse>,
    /// Distance to station (km)
    pub station_distance_km: Option<f64>,
    /// Data range
    pub data_range: DataRangeResponse,
    /// Latest weather data
    pub current: Option<DailyWeather>,
}

#[derive(Debug, Serialize)]
pub struct CoordinatesResponse {
    pub latitude: f64,
    pub longitude: f64,
}

impl From<Coordinates> for CoordinatesResponse {
    fn from(c: Coordinates) -> Self {
        Self {
            latitude: c.latitude,
            longitude: c.longitude,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StationResponse {
    pub id: String,
    pub name: String,
    pub state: Option<String>,
    pub coordinates: CoordinatesResponse,
    pub elevation: f32,
}

impl From<&WeatherStation> for StationResponse {
    fn from(s: &WeatherStation) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            state: s.state.clone(),
            coordinates: s.coordinates.into(),
            elevation: s.elevation,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DataRangeResponse {
    pub earliest_date: Option<String>,
    pub latest_date: Option<String>,
    pub total_records: usize,
    pub cached_records: usize,
}

#[derive(Debug, Serialize)]
pub struct GeocodingResponse {
    pub results: Vec<GeocodingResultResponse>,
}

#[derive(Debug, Serialize)]
pub struct GeocodingResultResponse {
    pub display_name: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Serialize)]
pub struct StationsResponse {
    pub stations: Vec<StationWithDistance>,
}

#[derive(Debug, Serialize)]
pub struct StationWithDistance {
    pub station: StationResponse,
    pub distance_km: Option<f64>,
}

/// GET /api/weather/info - Get weather service info
pub async fn get_weather_info(State(state): State<SharedWeatherState>) -> Json<WeatherInfo> {
    let weather = state.read().await;

    let (earliest_date, latest_date) = weather.storage.get_date_range();
    let current = weather.storage.get_latest().map(|r| r.weather.clone());

    let station_distance_km = weather.location.as_ref().and_then(|loc| {
        weather
            .nearest_station
            .as_ref()
            .map(|s| s.coordinates.distance_to(loc))
    });

    Json(WeatherInfo {
        location: weather.location.map(|c| c.into()),
        station: weather.nearest_station.as_ref().map(|s| s.into()),
        station_distance_km,
        data_range: DataRangeResponse {
            earliest_date,
            latest_date,
            total_records: weather.storage.total_count(),
            cached_records: weather.storage.cache_count(),
        },
        current,
    })
}

/// GET /api/weather/current - Get current/latest weather
pub async fn get_current_weather(
    State(state): State<SharedWeatherState>,
) -> Result<Json<OutdoorSummary>, StatusCode> {
    let weather = state.read().await;

    weather
        .storage
        .get_latest()
        .map(|r| Json(OutdoorSummary::from(r)))
        .ok_or(StatusCode::NOT_FOUND)
}

/// GET /api/weather/history - Get historical weather data
pub async fn get_weather_history(
    State(state): State<SharedWeatherState>,
    Query(query): Query<WeatherQuery>,
) -> Json<Vec<OutdoorSummary>> {
    let weather = state.read().await;

    let records = if let (Some(start), Some(end)) = (query.start_date, query.end_date) {
        weather.storage.get_range(&start, &end)
    } else {
        weather.storage.get_all()
    };

    let limit = query.days.unwrap_or(30);
    let summaries: Vec<OutdoorSummary> = records
        .into_iter()
        .take(limit)
        .map(OutdoorSummary::from)
        .collect();

    Json(summaries)
}

/// GET /api/weather/statistics - Get weather statistics
pub async fn get_weather_statistics(
    State(state): State<SharedWeatherState>,
    Query(query): Query<WeatherQuery>,
) -> Result<Json<WeatherStatistics>, StatusCode> {
    // Get all records from disk for full statistics (off the runtime, no lock held)
    let path = weather_history_path(&state).await;
    let records = run_blocking(move || {
        loriot_core::weather::storage::read_records(&path).unwrap_or_default()
    })
    .await;

    // Filter by date range if specified
    let filtered: Vec<DailyWeather> = if let (Some(start), Some(end)) =
        (query.start_date, query.end_date)
    {
        records
            .into_iter()
            .filter(|r| {
                r.weather.date.as_str() >= start.as_str() && r.weather.date.as_str() <= end.as_str()
            })
            .map(|r| r.weather)
            .collect()
    } else {
        records.into_iter().map(|r| r.weather).collect()
    };

    WeatherStatistics::from_records(&filtered)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// GET /api/weather/geocode - Geocode an address
pub async fn geocode_address(
    State(state): State<SharedWeatherState>,
    Query(query): Query<GeocodingQuery>,
) -> Result<Json<GeocodingResponse>, StatusCode> {
    let weather = state.read().await;

    match weather.geocoder.geocode(&query.address).await {
        Ok(results) => {
            let response = GeocodingResponse {
                results: results
                    .into_iter()
                    .map(|r| GeocodingResultResponse {
                        display_name: r.display_name,
                        latitude: r.coordinates.latitude,
                        longitude: r.coordinates.longitude,
                    })
                    .collect(),
            };
            Ok(Json(response))
        }
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// GET /api/weather/stations - Get available stations
pub async fn get_stations(
    State(state): State<SharedWeatherState>,
    Query(query): Query<StationQuery>,
) -> Json<StationsResponse> {
    let limit = query.limit.unwrap_or(10);

    // Try to get stations from DWD (with caching)
    let all_stations = {
        let weather = state.read().await;
        weather.station_fetcher.get_stations().await.ok()
    };

    // Fallback to defaults if DWD fetch fails
    let all_stations = all_stations.unwrap_or_else(get_default_stations);

    let stations: Vec<StationWithDistance> = if let (Some(lat), Some(lon)) = (query.lat, query.lon)
    {
        // Find nearest stations to coordinates
        let coords = Coordinates::new(lat, lon);
        let mut with_distance: Vec<_> = all_stations
            .iter()
            .map(|s| {
                let dist = s.coordinates.distance_to(&coords);
                (s, dist)
            })
            .collect();
        with_distance.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        with_distance
            .into_iter()
            .take(limit)
            .map(|(s, dist)| StationWithDistance {
                station: s.into(),
                distance_km: Some(dist),
            })
            .collect()
    } else if let Some(name) = query.name {
        // Search by name
        let name_lower = name.to_lowercase();
        all_stations
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&name_lower))
            .take(limit)
            .map(|s| StationWithDistance {
                station: s.into(),
                distance_km: None,
            })
            .collect()
    } else {
        // Return all stations (sorted by name)
        let mut sorted = all_stations.clone();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        sorted
            .iter()
            .take(limit)
            .map(|s| StationWithDistance {
                station: s.into(),
                distance_km: None,
            })
            .collect()
    };

    Json(StationsResponse { stations })
}

/// POST /api/weather/location - Set current location
#[derive(Debug, Deserialize)]
pub struct SetLocationRequest {
    pub latitude: f64,
    pub longitude: f64,
}

pub async fn set_location(
    State(state): State<SharedWeatherState>,
    Json(request): Json<SetLocationRequest>,
) -> Json<WeatherInfo> {
    let coords = Coordinates::new(request.latitude, request.longitude);

    // Try to find nearest station from DWD list (async)
    let nearest_station = {
        let weather = state.read().await;
        weather
            .station_fetcher
            .find_nearest(&coords)
            .await
            .ok()
            .flatten()
    };

    // Update state and save to disk
    {
        let mut weather = state.write().await;
        weather.location = Some(coords);
        if let Some(ref station) = nearest_station {
            weather.nearest_station = Some(station.clone());
        } else {
            // Fallback to hardcoded stations
            let default_stations = get_default_stations();
            weather.nearest_station = default_stations.into_iter().min_by(|a, b| {
                let dist_a = a.coordinates.distance_to(&coords);
                let dist_b = b.coordinates.distance_to(&coords);
                dist_a
                    .partial_cmp(&dist_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Save location to disk
        let saved = SavedLocation {
            latitude: request.latitude,
            longitude: request.longitude,
            station_id: weather.nearest_station.as_ref().map(|s| s.id.clone()),
            station_name: weather.nearest_station.as_ref().map(|s| s.name.clone()),
        };
        if let Err(e) = saved.save() {
            tracing::warn!("Failed to save location config: {}", e);
        } else {
            tracing::info!(
                "Saved location: {:.4}, {:.4} (station: {})",
                request.latitude,
                request.longitude,
                weather
                    .nearest_station
                    .as_ref()
                    .map(|s| s.name.as_str())
                    .unwrap_or("unknown")
            );
        }
    }

    // Return updated info
    get_weather_info(State(state)).await
}

/// POST /api/weather/fetch - Fetch latest weather data from DWD
pub async fn fetch_weather_data(
    State(state): State<SharedWeatherState>,
) -> Result<Json<Vec<DailyWeather>>, StatusCode> {
    let weather = state.read().await;

    let station = weather
        .nearest_station
        .as_ref()
        .ok_or(StatusCode::BAD_REQUEST)?;

    match weather.fetcher.fetch_recent(station).await {
        Ok(records) => {
            drop(weather);

            // Store the fetched records
            let mut weather = state.write().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let station_clone = weather.nearest_station.clone().unwrap();
            let distance = weather
                .location
                .map(|l| station_clone.coordinates.distance_to(&l))
                .unwrap_or(0.0);

            for daily in &records {
                let record = WeatherRecord {
                    station_id: station_clone.id.clone(),
                    station_name: station_clone.name.clone(),
                    station_coordinates: station_clone.coordinates,
                    distance_km: distance,
                    weather: daily.clone(),
                    fetched_at: now,
                };
                let _ = weather.storage.store(record);
            }

            Ok(Json(records))
        }
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}
