//! DWD weather station database and selection
//!
//! Provides functionality to:
//! - Load station metadata from DWD station list
//! - Find nearest station to given coordinates
//! - Filter stations by active status and data availability

use super::Coordinates;
use crate::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

#[cfg(feature = "async")]
use std::sync::Arc;
#[cfg(feature = "async")]
use tokio::sync::RwLock;

/// DWD station list URL
pub const DWD_STATION_LIST_URL: &str = "https://opendata.dwd.de/climate_environment/CDC/observations_germany/climate/daily/kl/recent/KL_Tageswerte_Beschreibung_Stationen.txt";

/// Default cache timeout for station list (24 hours in seconds)
pub const DEFAULT_CACHE_TIMEOUT_SECS: u64 = 24 * 60 * 60;

/// A DWD weather station
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherStation {
    /// Station ID (5 digits, zero-padded)
    pub id: String,
    /// Station name
    pub name: String,
    /// Federal state (Bundesland)
    pub state: Option<String>,
    /// Station coordinates
    pub coordinates: Coordinates,
    /// Station elevation (meters above sea level)
    pub elevation: f32,
    /// Start date of data availability (YYYYMMDD)
    pub start_date: String,
    /// End date of data availability (YYYYMMDD), empty if still active
    pub end_date: String,
    /// Data availability status
    pub status: StationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StationStatus {
    /// Station is actively reporting
    Active,
    /// Station is historical (no longer reporting)
    Historical,
    /// Status unknown
    Unknown,
}

impl WeatherStation {
    /// Check if station was active on a given date (YYYYMMDD)
    pub fn is_active_on(&self, date: &str) -> bool {
        date >= self.start_date.as_str()
            && (self.end_date.is_empty() || date <= self.end_date.as_str())
    }

    /// Check if station is currently active
    pub fn is_currently_active(&self) -> bool {
        self.status == StationStatus::Active
    }

    /// Calculate distance to a point (km)
    pub fn distance_to(&self, coords: &Coordinates) -> f64 {
        self.coordinates.distance_to(coords)
    }
}

/// Database of DWD weather stations
pub struct StationDatabase {
    stations: HashMap<String, WeatherStation>,
}

impl StationDatabase {
    /// Create a new empty database
    pub fn new() -> Self {
        Self {
            stations: HashMap::new(),
        }
    }

    /// Load stations from DWD station list file
    ///
    /// Expected format (semicolon or whitespace separated):
    /// Stations_id von_datum bis_datum Stationshoehe geoBreite geoLaenge Stationsname Bundesland
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = std::fs::File::open(path.as_ref()).map_err(|e| {
            AppError::Storage(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Station file not found: {}", e),
            ))
        })?;

        let reader = std::io::BufReader::new(file);
        let mut stations = HashMap::new();
        let mut line_num = 0;

        for line in reader.lines() {
            line_num += 1;
            let line = line?;

            // Skip header lines
            if line_num <= 2 || line.trim().is_empty() || line.starts_with("Stations_id") {
                continue;
            }

            if let Some(station) = Self::parse_station_line(&line) {
                stations.insert(station.id.clone(), station);
            }
        }

        Ok(Self { stations })
    }

    /// Parse a single station line from DWD format
    pub fn parse_station_line(line: &str) -> Option<WeatherStation> {
        // DWD uses fixed-width format, but fields are space-separated
        // Format: ID START END HEIGHT LAT LON NAME STATE STATUS
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() < 7 {
            return None;
        }

        let id = parts[0].to_string();
        let start_date = parts[1].to_string();
        let end_date = parts[2].to_string();
        let elevation: f32 = parts[3].parse().ok()?;
        let latitude: f64 = parts[4].parse().ok()?;
        let longitude: f64 = parts[5].parse().ok()?;

        // Name might contain spaces, state is after name
        // Find the last element that looks like a state (contains no digits, after position 6)
        let remaining: Vec<&str> = parts[6..].to_vec();

        let (name, state) = if remaining.len() >= 2 {
            // Try to identify where name ends and state begins
            // State is typically the last word(s) before "Frei" marker
            let last_idx = remaining.len() - 1;
            let state_str = remaining[last_idx];

            // Check if last part is "Frei" (data availability marker)
            if state_str == "Frei" && remaining.len() >= 3 {
                let state = remaining[last_idx - 1].to_string();
                let name = remaining[..last_idx - 1].join(" ");
                (name, Some(state))
            } else {
                let name = remaining.join(" ");
                (name, None)
            }
        } else {
            (remaining.join(" "), None)
        };

        // Determine if station is still active
        // If end_date is in the future or very recent, it's active
        let status = if end_date.is_empty() || end_date.as_str() >= "20251101" {
            StationStatus::Active
        } else {
            StationStatus::Historical
        };

        Some(WeatherStation {
            id,
            name,
            state,
            coordinates: Coordinates::new(latitude, longitude),
            elevation,
            start_date,
            end_date,
            status,
        })
    }

    /// Get a station by ID
    pub fn get(&self, id: &str) -> Option<&WeatherStation> {
        // Normalize ID to 5 digits with leading zeros
        let normalized_id = format!("{:0>5}", id);
        self.stations
            .get(&normalized_id)
            .or_else(|| self.stations.get(id))
    }

    /// Get all stations
    pub fn all(&self) -> impl Iterator<Item = &WeatherStation> {
        self.stations.values()
    }

    /// Get only active stations
    pub fn active(&self) -> impl Iterator<Item = &WeatherStation> {
        self.stations
            .values()
            .filter(|s| s.status == StationStatus::Active)
    }

    /// Find the nearest station to given coordinates
    pub fn find_nearest(&self, coords: &Coordinates) -> Option<&WeatherStation> {
        self.stations
            .values()
            .filter(|s| s.status == StationStatus::Active)
            .min_by(|a, b| {
                let dist_a = a.distance_to(coords);
                let dist_b = b.distance_to(coords);
                dist_a
                    .partial_cmp(&dist_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Find the N nearest stations to given coordinates
    pub fn find_nearest_n(&self, coords: &Coordinates, n: usize) -> Vec<(&WeatherStation, f64)> {
        let mut stations_with_distance: Vec<_> = self
            .stations
            .values()
            .filter(|s| s.status == StationStatus::Active)
            .map(|s| (s, s.distance_to(coords)))
            .collect();

        stations_with_distance
            .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        stations_with_distance.into_iter().take(n).collect()
    }

    /// Search stations by name (case-insensitive partial match)
    pub fn search_by_name(&self, query: &str) -> Vec<&WeatherStation> {
        let query_lower = query.to_lowercase();
        self.stations
            .values()
            .filter(|s| s.name.to_lowercase().contains(&query_lower))
            .collect()
    }

    /// Get station count
    pub fn len(&self) -> usize {
        self.stations.len()
    }

    /// Check if database is empty
    pub fn is_empty(&self) -> bool {
        self.stations.is_empty()
    }
}

impl Default for StationDatabase {
    fn default() -> Self {
        Self::new()
    }
}

/// Cached station data with timestamp
#[cfg(feature = "async")]
struct CachedStations {
    stations: Vec<WeatherStation>,
    fetched_at: std::time::Instant,
}

/// Station fetcher with caching
///
/// Fetches the full DWD station list from the official URL and caches it.
/// The cache has a configurable timeout (default 24 hours).
#[cfg(feature = "async")]
pub struct StationFetcher {
    cache: Arc<RwLock<Option<CachedStations>>>,
    cache_timeout_secs: u64,
}

#[cfg(feature = "async")]
impl StationFetcher {
    /// Create a new station fetcher with default cache timeout (24 hours)
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(None)),
            cache_timeout_secs: DEFAULT_CACHE_TIMEOUT_SECS,
        }
    }

    /// Create a new station fetcher with custom cache timeout
    pub fn with_timeout(cache_timeout_secs: u64) -> Self {
        Self {
            cache: Arc::new(RwLock::new(None)),
            cache_timeout_secs,
        }
    }

    /// Get all stations, fetching from DWD if cache is expired or empty
    pub async fn get_stations(&self) -> Result<Vec<WeatherStation>> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(ref cached) = *cache {
                if cached.fetched_at.elapsed().as_secs() < self.cache_timeout_secs {
                    return Ok(cached.stations.clone());
                }
            }
        }

        // Cache miss or expired, fetch fresh data
        let stations = self.fetch_from_dwd().await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            *cache = Some(CachedStations {
                stations: stations.clone(),
                fetched_at: std::time::Instant::now(),
            });
        }

        Ok(stations)
    }

    /// Force refresh the cache
    pub async fn refresh(&self) -> Result<Vec<WeatherStation>> {
        let stations = self.fetch_from_dwd().await?;

        let mut cache = self.cache.write().await;
        *cache = Some(CachedStations {
            stations: stations.clone(),
            fetched_at: std::time::Instant::now(),
        });

        Ok(stations)
    }

    /// Find the nearest station to given coordinates
    pub async fn find_nearest(&self, coords: &Coordinates) -> Result<Option<WeatherStation>> {
        let stations = self.get_stations().await?;

        let nearest = stations
            .into_iter()
            .filter(|s| s.status == StationStatus::Active)
            .min_by(|a, b| {
                let dist_a = a.distance_to(coords);
                let dist_b = b.distance_to(coords);
                dist_a
                    .partial_cmp(&dist_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        Ok(nearest)
    }

    /// Find the N nearest stations to given coordinates
    pub async fn find_nearest_n(
        &self,
        coords: &Coordinates,
        n: usize,
    ) -> Result<Vec<(WeatherStation, f64)>> {
        let stations = self.get_stations().await?;

        let mut stations_with_distance: Vec<_> = stations
            .into_iter()
            .filter(|s| s.status == StationStatus::Active)
            .map(|s| {
                let dist = s.distance_to(coords);
                (s, dist)
            })
            .collect();

        stations_with_distance
            .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(stations_with_distance.into_iter().take(n).collect())
    }

    /// Fetch station list from DWD
    async fn fetch_from_dwd(&self) -> Result<Vec<WeatherStation>> {
        let client = reqwest::Client::new();

        let response = client
            .get(DWD_STATION_LIST_URL)
            .send()
            .await
            .map_err(|e| AppError::Weather(format!("Failed to fetch station list: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::Weather(format!(
                "DWD returned status: {}",
                response.status()
            )));
        }

        let text = response
            .text()
            .await
            .map_err(|e| AppError::Weather(format!("Failed to read station list: {}", e)))?;

        self.parse_station_list(&text)
    }

    /// Parse DWD station list text format
    fn parse_station_list(&self, text: &str) -> Result<Vec<WeatherStation>> {
        let mut stations = Vec::new();

        for (line_num, line) in text.lines().enumerate() {
            // Skip header lines (first 2 lines) and empty lines
            if line_num < 2
                || line.trim().is_empty()
                || line.starts_with("Stations_id")
                || line.starts_with("-")
            {
                continue;
            }

            if let Some(station) = StationDatabase::parse_station_line(line) {
                stations.push(station);
            }
        }

        if stations.is_empty() {
            return Err(AppError::Weather(
                "No stations found in DWD response".to_string(),
            ));
        }

        Ok(stations)
    }

    /// Get cache status
    pub async fn cache_status(&self) -> Option<(usize, u64)> {
        let cache = self.cache.read().await;
        cache
            .as_ref()
            .map(|c| (c.stations.len(), c.fetched_at.elapsed().as_secs()))
    }
}

#[cfg(feature = "async")]
impl Default for StationFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Embedded subset of common German weather stations
/// Used as fallback when station file is not available
pub fn get_default_stations() -> Vec<WeatherStation> {
    vec![
        WeatherStation {
            id: "00433".to_string(),
            name: "Berlin-Tempelhof".to_string(),
            state: Some("Berlin".to_string()),
            coordinates: Coordinates::new(52.4675, 13.4021),
            elevation: 48.0,
            start_date: "19480101".to_string(),
            end_date: "".to_string(),
            status: StationStatus::Active,
        },
        WeatherStation {
            id: "01262".to_string(),
            name: "München-Stadt".to_string(),
            state: Some("Bayern".to_string()),
            coordinates: Coordinates::new(48.1632, 11.5429),
            elevation: 515.0,
            start_date: "19920601".to_string(),
            end_date: "".to_string(),
            status: StationStatus::Active,
        },
        WeatherStation {
            id: "02667".to_string(),
            name: "Hamburg-Fuhlsbüttel".to_string(),
            state: Some("Hamburg".to_string()),
            coordinates: Coordinates::new(53.6332, 9.9881),
            elevation: 11.0,
            start_date: "18910101".to_string(),
            end_date: "".to_string(),
            status: StationStatus::Active,
        },
        WeatherStation {
            id: "01078".to_string(),
            name: "Frankfurt/Main".to_string(),
            state: Some("Hessen".to_string()),
            coordinates: Coordinates::new(50.0259, 8.5213),
            elevation: 100.0,
            start_date: "19490101".to_string(),
            end_date: "".to_string(),
            status: StationStatus::Active,
        },
        WeatherStation {
            id: "05717".to_string(),
            name: "Wuppertal-Buchenhofen".to_string(),
            state: Some("Nordrhein-Westfalen".to_string()),
            coordinates: Coordinates::new(51.2256, 7.1052),
            elevation: 134.0,
            start_date: "19370101".to_string(),
            end_date: "".to_string(),
            status: StationStatus::Active,
        },
        WeatherStation {
            id: "03379".to_string(),
            name: "Köln-Bonn".to_string(),
            state: Some("Nordrhein-Westfalen".to_string()),
            coordinates: Coordinates::new(50.8659, 7.1427),
            elevation: 92.0,
            start_date: "19570701".to_string(),
            end_date: "".to_string(),
            status: StationStatus::Active,
        },
        WeatherStation {
            id: "04928".to_string(),
            name: "Stuttgart-Echterdingen".to_string(),
            state: Some("Baden-Württemberg".to_string()),
            coordinates: Coordinates::new(48.6883, 9.2215),
            elevation: 371.0,
            start_date: "19530101".to_string(),
            end_date: "".to_string(),
            status: StationStatus::Active,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_nearest_station() {
        let mut db = StationDatabase::new();
        for station in get_default_stations() {
            db.stations.insert(station.id.clone(), station);
        }

        // Test finding nearest to Berlin center
        let berlin = Coordinates::new(52.52, 13.405);
        let nearest = db.find_nearest(&berlin).unwrap();
        assert_eq!(nearest.id, "00433"); // Berlin-Tempelhof
    }

    #[test]
    fn test_station_distance() {
        let station = WeatherStation {
            id: "00433".to_string(),
            name: "Berlin-Tempelhof".to_string(),
            state: None,
            coordinates: Coordinates::new(52.4675, 13.4021),
            elevation: 48.0,
            start_date: "19480101".to_string(),
            end_date: "".to_string(),
            status: StationStatus::Active,
        };

        let point = Coordinates::new(52.52, 13.405);
        let distance = station.distance_to(&point);
        assert!(distance < 10.0); // Should be within 10 km
    }
}
