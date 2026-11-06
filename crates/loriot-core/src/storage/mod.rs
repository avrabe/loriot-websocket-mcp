//! Storage management for sensor readings with JSONL persistence
//!
//! This module provides a two-tier storage approach:
//! - Fast in-memory cache for recent readings (configurable limit)
//! - Full JSONL file on disk for historical data and analytics

use crate::sensors::SensorReading;
use crate::{AppError, Result};
use fxhash::FxHashSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;

/// Configuration for the storage manager
#[derive(Clone, Debug)]
pub struct StorageConfig {
    /// Path to the JSONL storage file
    pub file_path: String,
    /// Maximum number of readings to keep in memory (for fast access)
    pub memory_limit_readings: usize,
    /// Number of backup files to maintain
    pub backup_count: usize,
    /// Sync interval in minutes (for future periodic sync)
    pub sync_interval_minutes: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig {
            file_path: "sensor_history.jsonl".to_string(),
            memory_limit_readings: 2000,
            backup_count: 3,
            sync_interval_minutes: 60,
        }
    }
}

/// Manages sensor reading storage with memory caching and JSONL persistence
pub struct StorageManager {
    config: StorageConfig,
    /// Recent readings kept in memory for fast access
    memory_cache: Vec<SensorReading>,
    /// Keys of all readings (memory + disk) to prevent duplicates
    seen_keys: FxHashSet<String>,
    /// Total count of all readings (including those only on disk)
    total_readings_count: usize,
}

impl StorageManager {
    /// Create a new storage manager with the given configuration
    pub fn new(config: StorageConfig) -> Self {
        StorageManager {
            config,
            memory_cache: Vec::new(),
            seen_keys: FxHashSet::default(),
            total_readings_count: 0,
        }
    }

    /// Load existing data from the storage file into memory
    /// Only the most recent readings (up to memory_limit_readings) are kept in RAM,
    /// but all readings are tracked for duplicate detection.
    pub fn load_existing_data(&mut self) -> Result<()> {
        if !Path::new(&self.config.file_path).exists() {
            return Ok(());
        }

        let file = std::fs::File::open(&self.config.file_path)?;
        let reader = std::io::BufReader::new(file);

        let mut all_readings = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<SensorReading>(&line) {
                Ok(reading) => {
                    let key = format!("{}:{}", reading.eui, reading.ts);
                    if !self.seen_keys.contains(&key) {
                        self.seen_keys.insert(key);
                        all_readings.push(reading);
                    }
                }
                Err(_) => continue,
            }
        }

        // Track total count of all readings on disk
        self.total_readings_count = all_readings.len();

        // Sort by timestamp
        all_readings.sort_by_key(|r| r.ts);

        // Keep only recent readings in memory for fast access
        if all_readings.len() > self.config.memory_limit_readings {
            let start_idx = all_readings.len() - self.config.memory_limit_readings;
            self.memory_cache = all_readings[start_idx..].to_vec();
        } else {
            self.memory_cache = all_readings;
        }

        Ok(())
    }

    /// Read ALL readings from the storage file (for analytics/export)
    /// This is slower than get_readings() but provides complete historical data.
    pub fn read_all_from_disk(&self) -> Result<Vec<SensorReading>> {
        self.read_from_disk_filtered(None, None)
    }

    /// Read readings from disk with optional time and device filters
    /// This is the primary method for getting complete historical data for analytics.
    pub fn read_from_disk_filtered(
        &self,
        cutoff_timestamp: Option<u64>,
        device_eui: Option<&str>,
    ) -> Result<Vec<SensorReading>> {
        if !Path::new(&self.config.file_path).exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&self.config.file_path)?;
        let reader = std::io::BufReader::new(file);
        let mut readings = Vec::new();
        let mut seen = FxHashSet::default();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(reading) = serde_json::from_str::<SensorReading>(&line) {
                // Apply time filter
                if let Some(cutoff) = cutoff_timestamp {
                    if reading.ts < cutoff {
                        continue;
                    }
                }

                // Apply device filter
                if let Some(eui) = device_eui {
                    if reading.eui != eui {
                        continue;
                    }
                }

                let key = format!("{}:{}", reading.eui, reading.ts);
                if !seen.contains(&key) {
                    seen.insert(key);
                    readings.push(reading);
                }
            }
        }

        readings.sort_by_key(|r| r.ts);
        Ok(readings)
    }

    /// Read readings from disk within a specific time range
    /// Used for analytics when navigating to historical periods
    pub fn read_from_disk_in_range(
        &self,
        start_ts: u64,
        end_ts: u64,
        device_eui: Option<&str>,
    ) -> Result<Vec<SensorReading>> {
        if !Path::new(&self.config.file_path).exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&self.config.file_path)?;
        let reader = std::io::BufReader::new(file);
        let mut readings = Vec::new();
        let mut seen = FxHashSet::default();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(reading) = serde_json::from_str::<SensorReading>(&line) {
                // Apply time range filter
                if reading.ts < start_ts || reading.ts > end_ts {
                    continue;
                }

                // Apply device filter
                if let Some(eui) = device_eui {
                    if reading.eui != eui {
                        continue;
                    }
                }

                let key = format!("{}:{}", reading.eui, reading.ts);
                if !seen.contains(&key) {
                    seen.insert(key);
                    readings.push(reading);
                }
            }
        }

        readings.sort_by_key(|r| r.ts);
        Ok(readings)
    }

    /// Get total count of all readings (memory + disk)
    pub fn total_count(&self) -> usize {
        self.total_readings_count
    }

    /// Get the overall data range (earliest and latest timestamps)
    /// Reads from disk for accurate range if memory cache is limited
    pub fn get_data_range(&self) -> (Option<u64>, Option<u64>) {
        // Try to get from disk for accurate range
        if let Ok(readings) = self.read_from_disk_filtered(None, None) {
            if readings.is_empty() {
                return (None, None);
            }
            let earliest = readings.iter().map(|r| r.ts).min();
            let latest = readings.iter().map(|r| r.ts).max();
            return (earliest, latest);
        }

        // Fallback to memory cache
        if self.memory_cache.is_empty() {
            return (None, None);
        }
        let earliest = self.memory_cache.iter().map(|r| r.ts).min();
        let latest = self.memory_cache.iter().map(|r| r.ts).max();
        (earliest, latest)
    }

    /// Store a new reading, returning true if it was new (not a duplicate)
    pub fn store_reading(&mut self, reading: SensorReading) -> Result<bool> {
        let key = format!("{}:{}", reading.eui, reading.ts);

        if self.seen_keys.contains(&key) {
            return Ok(false);
        }

        self.seen_keys.insert(key);

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.file_path)?;

        let line = serde_json::to_string(&reading).map_err(|e| {
            AppError::Storage(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        writeln!(file, "{}", line)?;

        // Update total count
        self.total_readings_count += 1;

        let insert_pos = self
            .memory_cache
            .binary_search_by_key(&reading.ts, |r| r.ts)
            .unwrap_or_else(|e| e);
        self.memory_cache.insert(insert_pos, reading);

        if self.memory_cache.len() > self.config.memory_limit_readings {
            self.memory_cache.remove(0);
        }

        Ok(true)
    }

    /// Get all readings in memory
    pub fn get_readings(&self) -> &[SensorReading] {
        &self.memory_cache
    }

    /// Get readings within a time window
    pub fn get_readings_in_window(&self, cutoff_timestamp: Option<u64>) -> Vec<&SensorReading> {
        match cutoff_timestamp {
            Some(cutoff) => self
                .memory_cache
                .iter()
                .filter(|r| r.ts >= cutoff)
                .collect(),
            None => self.memory_cache.iter().collect(),
        }
    }

    /// Get paginated readings with optional time window filter
    pub fn get_readings_paginated(
        &self,
        cutoff_timestamp: Option<u64>,
        page: usize,
        page_size: usize,
    ) -> (Vec<&SensorReading>, usize) {
        let filtered: Vec<_> = self.get_readings_in_window(cutoff_timestamp);
        let total = filtered.len();

        let start = page * page_size;
        let readings = filtered.into_iter().skip(start).take(page_size).collect();

        (readings, total)
    }

    /// Get paginated readings within a specific time range (start_ts to end_ts)
    pub fn get_readings_in_range(
        &self,
        start_ts: u64,
        end_ts: u64,
        page: usize,
        page_size: usize,
    ) -> (Vec<&SensorReading>, usize) {
        let filtered: Vec<_> = self
            .memory_cache
            .iter()
            .filter(|r| r.ts >= start_ts && r.ts <= end_ts)
            .collect();
        let total = filtered.len();

        let start = page * page_size;
        let readings = filtered.into_iter().skip(start).take(page_size).collect();

        (readings, total)
    }

    /// Get readings for a specific device
    pub fn get_readings_for_device(&self, eui: &str) -> Vec<&SensorReading> {
        self.memory_cache.iter().filter(|r| r.eui == eui).collect()
    }

    /// Get the latest reading for each device
    pub fn get_latest_readings(&self) -> HashMap<String, &SensorReading> {
        let mut latest: HashMap<String, &SensorReading> = HashMap::new();

        for reading in &self.memory_cache {
            latest
                .entry(reading.eui.clone())
                .and_modify(|existing| {
                    if reading.ts > existing.ts {
                        *existing = reading;
                    }
                })
                .or_insert(reading);
        }

        latest
    }

    /// Compute statistics per device
    pub fn get_device_statistics(
        &self,
        cutoff_timestamp: Option<u64>,
    ) -> HashMap<String, DeviceStats> {
        let readings = self.get_readings_in_window(cutoff_timestamp);
        let mut stats: HashMap<String, DeviceStats> = HashMap::new();

        for reading in readings {
            let device_stats = stats
                .entry(reading.eui.clone())
                .or_insert_with(|| DeviceStats {
                    device_id: reading.eui.clone(),
                    device_name: reading.name.clone(),
                    total_readings: 0,
                    temperature_sum: 0.0,
                    temperature_count: 0,
                    humidity_sum: 0.0,
                    humidity_count: 0,
                    pir_events: 0,
                    last_reading_ts: 0,
                    battery_voltage: None,
                    battery_status: None,
                    last_temperature: None,
                    last_humidity: None,
                });

            device_stats.total_readings += 1;

            // Track latest reading values
            if reading.ts >= device_stats.last_reading_ts {
                device_stats.last_reading_ts = reading.ts;
                if reading.parsed_temperature.is_some() {
                    device_stats.last_temperature = reading.parsed_temperature;
                }
                if reading.parsed_humidity.is_some() {
                    device_stats.last_humidity = reading.parsed_humidity;
                }
            }

            if let Some(temp) = reading.parsed_temperature {
                device_stats.temperature_sum += temp;
                device_stats.temperature_count += 1;
            }

            if let Some(hum) = reading.parsed_humidity {
                device_stats.humidity_sum += hum;
                device_stats.humidity_count += 1;
            }

            if reading.parsed_activity == Some(true) {
                device_stats.pir_events += 1;
            }

            if reading.parsed_battery_voltage.is_some() {
                device_stats.battery_voltage = reading.parsed_battery_voltage;
            }

            if reading.parsed_battery_status.is_some() {
                device_stats.battery_status = reading.parsed_battery_status.clone();
            }
        }

        stats
    }

    /// Get the number of unique devices
    pub fn device_count(&self) -> usize {
        self.memory_cache
            .iter()
            .map(|r| &r.eui)
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    /// Get the total number of readings in memory
    pub fn reading_count(&self) -> usize {
        self.memory_cache.len()
    }

    /// Compute device statistics from ALL readings on disk (not just memory cache)
    /// This is slower but provides complete historical data for analytics.
    pub fn get_full_device_statistics(&self) -> Result<HashMap<String, DeviceStats>> {
        let all_readings = self.read_all_from_disk()?;
        let mut stats: HashMap<String, DeviceStats> = HashMap::new();

        for reading in &all_readings {
            let device_stats = stats
                .entry(reading.eui.clone())
                .or_insert_with(|| DeviceStats {
                    device_id: reading.eui.clone(),
                    device_name: reading.name.clone(),
                    total_readings: 0,
                    temperature_sum: 0.0,
                    temperature_count: 0,
                    humidity_sum: 0.0,
                    humidity_count: 0,
                    pir_events: 0,
                    last_reading_ts: 0,
                    battery_voltage: None,
                    battery_status: None,
                    last_temperature: None,
                    last_humidity: None,
                });

            device_stats.total_readings += 1;

            // Track latest reading values
            if reading.ts >= device_stats.last_reading_ts {
                device_stats.last_reading_ts = reading.ts;
                if reading.parsed_temperature.is_some() {
                    device_stats.last_temperature = reading.parsed_temperature;
                }
                if reading.parsed_humidity.is_some() {
                    device_stats.last_humidity = reading.parsed_humidity;
                }
            }

            if let Some(temp) = reading.parsed_temperature {
                device_stats.temperature_sum += temp;
                device_stats.temperature_count += 1;
            }

            if let Some(hum) = reading.parsed_humidity {
                device_stats.humidity_sum += hum;
                device_stats.humidity_count += 1;
            }

            if reading.parsed_activity == Some(true) {
                device_stats.pir_events += 1;
            }

            if reading.parsed_battery_voltage.is_some() {
                device_stats.battery_voltage = reading.parsed_battery_voltage;
            }

            if reading.parsed_battery_status.is_some() {
                device_stats.battery_status = reading.parsed_battery_status.clone();
            }
        }

        Ok(stats)
    }

    /// Get the file path for the storage
    pub fn file_path(&self) -> &str {
        &self.config.file_path
    }
}

/// Statistics for a single device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStats {
    pub device_id: String,
    pub device_name: Option<String>,
    pub total_readings: usize,
    pub temperature_sum: f32,
    pub temperature_count: usize,
    pub humidity_sum: f32,
    pub humidity_count: usize,
    pub pir_events: usize,
    pub last_reading_ts: u64,
    pub battery_voltage: Option<f32>,
    pub battery_status: Option<String>,
    /// Last (most recent) temperature reading
    pub last_temperature: Option<f32>,
    /// Last (most recent) humidity reading
    pub last_humidity: Option<f32>,
}

impl DeviceStats {
    /// Calculate average temperature
    pub fn avg_temperature(&self) -> Option<f32> {
        if self.temperature_count > 0 {
            Some(self.temperature_sum / self.temperature_count as f32)
        } else {
            None
        }
    }

    /// Calculate average humidity
    pub fn avg_humidity(&self) -> Option<f32> {
        if self.humidity_count > 0 {
            Some(self.humidity_sum / self.humidity_count as f32)
        } else {
            None
        }
    }

    /// Calculate expected timestamp for next reading (20-minute interval)
    pub fn next_expected_reading(&self) -> u64 {
        self.last_reading_ts + (20 * 60 * 1000)
    }

    /// Check if the device is overdue for a reading
    pub fn is_overdue(&self) -> bool {
        let now = crate::current_timestamp_ms();
        now > self.next_expected_reading() + (5 * 60 * 1000) // 5 minute grace period
    }

    /// Time until next expected reading in seconds (negative if overdue)
    pub fn seconds_until_next(&self) -> i64 {
        let now = crate::current_timestamp_ms();
        let next = self.next_expected_reading();
        ((next as i64) - (now as i64)) / 1000
    }

    /// Get display name (device name or ID)
    pub fn display_name(&self) -> &str {
        self.device_name.as_deref().unwrap_or(&self.device_id)
    }
}
