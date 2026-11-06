//! Weather data storage and persistence
//!
//! Stores weather data in JSONL format similar to sensor readings.

use super::{Coordinates, DailyWeather, WeatherConfig};
use crate::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;

/// Stored weather record with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherRecord {
    /// Station ID
    pub station_id: String,
    /// Station name
    pub station_name: String,
    /// Station coordinates
    pub station_coordinates: Coordinates,
    /// Distance from configured location (km)
    pub distance_km: f64,
    /// Daily weather data
    pub weather: DailyWeather,
    /// Timestamp when this record was fetched
    pub fetched_at: u64,
}

/// Weather storage manager
pub struct WeatherStorage {
    config: WeatherConfig,
    /// In-memory cache of recent weather data (by date)
    cache: HashMap<String, WeatherRecord>,
    /// Total records on disk
    total_records: usize,
}

impl WeatherStorage {
    /// Create a new weather storage with given config
    pub fn new(config: WeatherConfig) -> Self {
        Self {
            config,
            cache: HashMap::new(),
            total_records: 0,
        }
    }

    /// Load existing data from storage file
    pub fn load(&mut self) -> Result<()> {
        let path = Path::new(&self.config.storage_path);
        if !path.exists() {
            return Ok(());
        }

        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(record) = serde_json::from_str::<WeatherRecord>(&line) {
                self.cache.insert(record.weather.date.clone(), record);
                self.total_records += 1;
            }
        }

        // Keep only recent days in memory
        self.trim_cache();

        Ok(())
    }

    /// Store a new weather record
    pub fn store(&mut self, record: WeatherRecord) -> Result<bool> {
        let date = record.weather.date.clone();

        // Check if we already have this date
        if self.cache.contains_key(&date) {
            return Ok(false);
        }

        // Append to file
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.storage_path)?;

        let line = serde_json::to_string(&record).map_err(|e| {
            AppError::Storage(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        writeln!(file, "{}", line)?;

        // Add to cache
        self.cache.insert(date, record);
        self.total_records += 1;

        // Trim cache if needed
        self.trim_cache();

        Ok(true)
    }

    /// Store multiple weather records
    pub fn store_batch(&mut self, records: Vec<WeatherRecord>) -> Result<usize> {
        let mut stored = 0;
        for record in records {
            if self.store(record)? {
                stored += 1;
            }
        }
        Ok(stored)
    }

    /// Get weather for a specific date
    pub fn get_by_date(&self, date: &str) -> Option<&WeatherRecord> {
        self.cache.get(date)
    }

    /// Get the latest weather record
    pub fn get_latest(&self) -> Option<&WeatherRecord> {
        self.cache
            .values()
            .max_by(|a, b| a.weather.date.cmp(&b.weather.date))
    }

    /// Get weather records for a date range (inclusive)
    pub fn get_range(&self, start_date: &str, end_date: &str) -> Vec<&WeatherRecord> {
        let mut records: Vec<_> = self
            .cache
            .values()
            .filter(|r| {
                r.weather.date.as_str() >= start_date && r.weather.date.as_str() <= end_date
            })
            .collect();
        records.sort_by(|a, b| b.weather.date.cmp(&a.weather.date));
        records
    }

    /// Get all cached weather records
    pub fn get_all(&self) -> Vec<&WeatherRecord> {
        let mut records: Vec<_> = self.cache.values().collect();
        records.sort_by(|a, b| b.weather.date.cmp(&a.weather.date));
        records
    }

    /// Get weather data range (earliest and latest dates)
    pub fn get_date_range(&self) -> (Option<String>, Option<String>) {
        let dates: Vec<_> = self.cache.keys().collect();
        if dates.is_empty() {
            return (None, None);
        }
        let earliest = dates.iter().min().map(|s| s.to_string());
        let latest = dates.iter().max().map(|s| s.to_string());
        (earliest, latest)
    }

    /// Get total number of records
    pub fn total_count(&self) -> usize {
        self.total_records
    }

    /// Get number of records in cache
    pub fn cache_count(&self) -> usize {
        self.cache.len()
    }

    /// Trim cache to configured history days
    fn trim_cache(&mut self) {
        if self.cache.len() <= self.config.history_days {
            return;
        }

        // Get sorted dates
        let mut dates: Vec<_> = self.cache.keys().cloned().collect();
        dates.sort();

        // Remove oldest dates
        let to_remove = self.cache.len() - self.config.history_days;
        for date in dates.into_iter().take(to_remove) {
            self.cache.remove(&date);
        }
    }

    /// Read all records from disk (for analytics that need full history)
    pub fn read_all_from_disk(&self) -> Result<Vec<WeatherRecord>> {
        let path = Path::new(&self.config.storage_path);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let mut records = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(record) = serde_json::from_str::<WeatherRecord>(&line) {
                records.push(record);
            }
        }

        records.sort_by(|a, b| b.weather.date.cmp(&a.weather.date));
        Ok(records)
    }
}

/// Summary of outdoor weather conditions for comparison with indoor sensors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdoorSummary {
    /// Date (YYYYMMDD)
    pub date: String,
    /// Station used
    pub station_name: String,
    /// Distance to station (km)
    pub station_distance_km: f64,
    /// Mean outdoor temperature (°C)
    pub temp_mean: Option<f32>,
    /// Max outdoor temperature (°C)
    pub temp_max: Option<f32>,
    /// Min outdoor temperature (°C)
    pub temp_min: Option<f32>,
    /// Outdoor humidity (%)
    pub humidity: Option<f32>,
    /// Precipitation (mm)
    pub precipitation: Option<f32>,
}

impl From<&WeatherRecord> for OutdoorSummary {
    fn from(record: &WeatherRecord) -> Self {
        Self {
            date: record.weather.date.clone(),
            station_name: record.station_name.clone(),
            station_distance_km: record.distance_km,
            temp_mean: record.weather.temp_mean,
            temp_max: record.weather.temp_max,
            temp_min: record.weather.temp_min,
            humidity: record.weather.humidity,
            precipitation: record.weather.precipitation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weather_storage_basic() {
        let config = WeatherConfig {
            storage_path: "/tmp/test_weather.jsonl".to_string(),
            history_days: 7,
            ..Default::default()
        };

        let mut storage = WeatherStorage::new(config);

        let record = WeatherRecord {
            station_id: "05717".to_string(),
            station_name: "Bochum".to_string(),
            station_coordinates: Coordinates::new(51.4, 7.2),
            distance_km: 5.0,
            weather: DailyWeather {
                date: "20241201".to_string(),
                station_id: "05717".to_string(),
                station_name: Some("Bochum".to_string()),
                temp_mean: Some(5.0),
                temp_max: Some(8.0),
                temp_min: Some(2.0),
                temp_ground_min: None,
                humidity: Some(80.0),
                precipitation: Some(2.5),
                sunshine_hours: None,
                snow_depth: None,
                wind_speed: None,
                wind_gust: None,
                vapor_pressure: None,
                air_pressure: None,
                cloud_cover: None,
                quality_level: Some(10),
            },
            fetched_at: 1700000000000,
        };

        // Store and verify
        let stored = storage.store(record.clone()).unwrap();
        assert!(stored);

        // Duplicate should not store
        let stored_again = storage.store(record).unwrap();
        assert!(!stored_again);

        // Verify retrieval
        let retrieved = storage.get_by_date("20241201").unwrap();
        assert_eq!(retrieved.weather.temp_mean, Some(5.0));

        // Cleanup
        let _ = std::fs::remove_file("/tmp/test_weather.jsonl");
    }
}
