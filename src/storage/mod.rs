use crate::sensors::SensorReading;
use crate::{AppError, Result};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;
use fxhash::FxHashSet;

#[derive(Clone, Debug)]
pub struct StorageConfig {
    pub file_path: String,
    pub memory_limit_readings: usize,
    pub backup_count: usize,
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

pub struct StorageManager {
    config: StorageConfig,
    memory_cache: Vec<SensorReading>,
    seen_keys: FxHashSet<String>,
}

impl StorageManager {
    pub fn new(config: StorageConfig) -> Self {
        StorageManager {
            config,
            memory_cache: Vec::new(),
            seen_keys: FxHashSet::default(),
        }
    }

    pub fn load_existing_data(&mut self) -> Result<()> {
        if !Path::new(&self.config.file_path).exists() {
            return Ok(());
        }

        let file = std::fs::File::open(&self.config.file_path)?;
        let reader = std::io::BufReader::new(file);
        
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
                        self.memory_cache.push(reading);
                    }
                }
                Err(_) => continue,
            }
        }

        self.memory_cache.sort_by_key(|r| r.ts);
        
        if self.memory_cache.len() > self.config.memory_limit_readings {
            let start_idx = self.memory_cache.len() - self.config.memory_limit_readings;
            self.memory_cache = self.memory_cache[start_idx..].to_vec();
        }

        Ok(())
    }

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
        
        let line = serde_json::to_string(&reading)
            .map_err(|e| AppError::Storage(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
        writeln!(file, "{}", line)?;

        let insert_pos = self.memory_cache
            .binary_search_by_key(&reading.ts, |r| r.ts)
            .unwrap_or_else(|e| e);
        self.memory_cache.insert(insert_pos, reading);

        if self.memory_cache.len() > self.config.memory_limit_readings {
            self.memory_cache.remove(0);
        }

        Ok(true)
    }

    pub fn get_readings(&self) -> &[SensorReading] {
        &self.memory_cache
    }

    pub fn get_readings_in_window(&self, cutoff_timestamp: Option<u64>) -> Vec<&SensorReading> {
        match cutoff_timestamp {
            Some(cutoff) => self.memory_cache
                .iter()
                .filter(|r| r.ts >= cutoff)
                .collect(),
            None => self.memory_cache.iter().collect(),
        }
    }

    pub fn get_device_statistics(&self, cutoff_timestamp: Option<u64>) -> HashMap<String, DeviceStats> {
        let readings = self.get_readings_in_window(cutoff_timestamp);
        let mut stats: HashMap<String, DeviceStats> = HashMap::new();

        for reading in readings {
            let device_stats = stats.entry(reading.eui.clone()).or_insert_with(|| DeviceStats {
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
            });

            device_stats.total_readings += 1;
            device_stats.last_reading_ts = device_stats.last_reading_ts.max(reading.ts);

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
}

#[derive(Debug, Clone)]
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
}

impl DeviceStats {
    pub fn avg_temperature(&self) -> Option<f32> {
        if self.temperature_count > 0 {
            Some(self.temperature_sum / self.temperature_count as f32)
        } else {
            None
        }
    }

    pub fn avg_humidity(&self) -> Option<f32> {
        if self.humidity_count > 0 {
            Some(self.humidity_sum / self.humidity_count as f32)
        } else {
            None
        }
    }

    pub fn next_expected_reading(&self) -> u64 {
        // Assuming 20-minute intervals (1200 seconds)
        self.last_reading_ts + (20 * 60 * 1000)
    }
}