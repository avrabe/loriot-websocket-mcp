//! Weather data integration module
//!
//! Provides abstracted weather services including:
//! - DWD (Deutscher Wetterdienst) data fetching
//! - Station selection based on coordinates
//! - Geocoding (address to coordinates)
//! - Weather data storage and querying

pub mod dwd;
pub mod forecast;
pub mod geocoding;
pub mod station;
pub mod storage;

use serde::{Deserialize, Serialize};

/// Geographic coordinates
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

impl Coordinates {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
        }
    }

    /// Calculate distance to another point using Haversine formula (in km)
    pub fn distance_to(&self, other: &Coordinates) -> f64 {
        const EARTH_RADIUS_KM: f64 = 6371.0;

        let lat1_rad = self.latitude.to_radians();
        let lat2_rad = other.latitude.to_radians();
        let delta_lat = (other.latitude - self.latitude).to_radians();
        let delta_lon = (other.longitude - self.longitude).to_radians();

        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();

        EARTH_RADIUS_KM * c
    }
}

/// Daily weather observation data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyWeather {
    /// Date in YYYYMMDD format
    pub date: String,
    /// Station ID
    pub station_id: String,
    /// Station name
    pub station_name: Option<String>,
    /// Mean temperature (°C)
    pub temp_mean: Option<f32>,
    /// Maximum temperature (°C)
    pub temp_max: Option<f32>,
    /// Minimum temperature (°C)
    pub temp_min: Option<f32>,
    /// Ground minimum temperature at 5cm (°C)
    pub temp_ground_min: Option<f32>,
    /// Mean relative humidity (%)
    pub humidity: Option<f32>,
    /// Precipitation (mm)
    pub precipitation: Option<f32>,
    /// Sunshine duration (hours)
    pub sunshine_hours: Option<f32>,
    /// Snow depth (cm)
    pub snow_depth: Option<f32>,
    /// Mean wind speed (m/s)
    pub wind_speed: Option<f32>,
    /// Maximum wind gust (m/s)
    pub wind_gust: Option<f32>,
    /// Mean vapor pressure (hPa)
    pub vapor_pressure: Option<f32>,
    /// Mean air pressure at station height (hPa)
    pub air_pressure: Option<f32>,
    /// Cloud cover (1/8)
    pub cloud_cover: Option<f32>,
    /// Quality level (1-10, higher is better)
    pub quality_level: Option<i32>,
}

impl DailyWeather {
    /// Check if this record has valid temperature data
    pub fn has_temperature(&self) -> bool {
        self.temp_mean.is_some() || self.temp_max.is_some() || self.temp_min.is_some()
    }

    /// Get the best available temperature (mean, or average of max/min)
    pub fn best_temperature(&self) -> Option<f32> {
        self.temp_mean
            .or_else(|| match (self.temp_max, self.temp_min) {
                (Some(max), Some(min)) => Some((max + min) / 2.0),
                (Some(max), None) => Some(max),
                (None, Some(min)) => Some(min),
                (None, None) => None,
            })
    }
}

/// Weather data for a specific location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationWeather {
    /// Location coordinates
    pub coordinates: Coordinates,
    /// Location address (if resolved from geocoding)
    pub address: Option<String>,
    /// Nearest weather station
    pub station: station::WeatherStation,
    /// Distance to station (km)
    pub station_distance_km: f64,
    /// Current/latest weather data
    pub current: Option<DailyWeather>,
    /// Historical weather data (sorted by date, newest first)
    pub history: Vec<DailyWeather>,
}

/// Season classification for energy calculations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    /// Determine season from date (YYYYMMDD format)
    pub fn from_date(date: &str) -> Self {
        if date.len() < 8 {
            return Season::Winter;
        }

        let month: u32 = date[4..6].parse().unwrap_or(1);
        let day: u32 = date[6..8].parse().unwrap_or(1);

        match month {
            3 if day >= 20 => Season::Spring,
            4..=5 => Season::Spring,
            6 if day < 21 => Season::Spring,
            6 if day >= 21 => Season::Summer,
            7..=8 => Season::Summer,
            9 if day < 23 => Season::Summer,
            9 if day >= 23 => Season::Autumn,
            10..=11 => Season::Autumn,
            12 if day < 21 => Season::Autumn,
            _ => Season::Winter,
        }
    }

    /// Check if this is a heating season
    pub fn is_heating_season(&self) -> bool {
        matches!(self, Season::Winter | Season::Autumn | Season::Spring)
    }
}

/// Configuration for weather service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherConfig {
    /// Location coordinates (required)
    pub coordinates: Option<Coordinates>,
    /// Location address (optional, will be geocoded)
    pub address: Option<String>,
    /// Preferred station ID (optional, will find nearest if not set)
    pub station_id: Option<String>,
    /// Path to weather data storage file
    pub storage_path: String,
    /// Days of history to keep
    pub history_days: usize,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            coordinates: None,
            address: None,
            station_id: None,
            storage_path: "weather_history.jsonl".to_string(),
            history_days: 365,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinates_distance() {
        // Berlin to Munich approximately 504 km
        let berlin = Coordinates::new(52.52, 13.405);
        let munich = Coordinates::new(48.1351, 11.582);
        let distance = berlin.distance_to(&munich);
        assert!((distance - 504.0).abs() < 10.0);
    }

    #[test]
    fn test_season_from_date() {
        assert_eq!(Season::from_date("20240115"), Season::Winter);
        assert_eq!(Season::from_date("20240415"), Season::Spring);
        assert_eq!(Season::from_date("20240715"), Season::Summer);
        assert_eq!(Season::from_date("20241015"), Season::Autumn);
    }
}
