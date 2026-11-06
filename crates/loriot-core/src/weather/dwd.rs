//! DWD (Deutscher Wetterdienst) data fetcher and parser
//!
//! Fetches daily climate data from DWD's Open Data server:
//! https://opendata.dwd.de/climate_environment/CDC/observations_germany/climate/daily/kl/
//!
//! Data format is semicolon-separated CSV with these columns:
//! STATIONS_ID;MESS_DATUM;QN_3;FX;FM;QN_4;RSK;RSKF;SDK;SHK_TAG;NM;VPM;PM;TMK;UPM;TXK;TNK;TGK;eor

#[cfg(any(feature = "async", feature = "blocking"))]
use super::station::WeatherStation;
use super::DailyWeather;
use crate::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::Path;

/// DWD Open Data base URL
const DWD_BASE_URL: &str =
    "https://opendata.dwd.de/climate_environment/CDC/observations_germany/climate/daily/kl";

/// Column indices in DWD CSV format
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Fields parsed from CSV but not all used yet
struct DwdColumnIndices {
    station_id: usize,
    date: usize,
    quality_wind: usize,    // QN_3
    wind_gust: usize,       // FX - max wind gust m/s
    wind_speed: usize,      // FM - mean wind speed m/s
    quality_other: usize,   // QN_4
    precipitation: usize,   // RSK - daily precipitation mm
    precip_form: usize,     // RSKF - precipitation form
    sunshine: usize,        // SDK - sunshine hours
    snow_depth: usize,      // SHK_TAG - snow depth cm
    cloud_cover: usize,     // NM - cloud cover 1/8
    vapor_pressure: usize,  // VPM - vapor pressure hPa
    air_pressure: usize,    // PM - air pressure hPa
    temp_mean: usize,       // TMK - mean temp °C
    humidity: usize,        // UPM - relative humidity %
    temp_max: usize,        // TXK - max temp °C
    temp_min: usize,        // TNK - min temp °C
    temp_ground_min: usize, // TGK - ground min temp °C
}

impl Default for DwdColumnIndices {
    fn default() -> Self {
        Self {
            station_id: 0,
            date: 1,
            quality_wind: 2,
            wind_gust: 3,
            wind_speed: 4,
            quality_other: 5,
            precipitation: 6,
            precip_form: 7,
            sunshine: 8,
            snow_depth: 9,
            cloud_cover: 10,
            vapor_pressure: 11,
            air_pressure: 12,
            temp_mean: 13,
            humidity: 14,
            temp_max: 15,
            temp_min: 16,
            temp_ground_min: 17,
        }
    }
}

/// DWD data fetcher
#[allow(dead_code)] // data_dir reserved for future caching
pub struct DwdFetcher {
    /// Optional cached station data directory
    data_dir: Option<String>,
}

impl DwdFetcher {
    /// Create a new DWD fetcher
    pub fn new() -> Self {
        Self { data_dir: None }
    }

    /// Create with a local data directory for caching
    pub fn with_data_dir(data_dir: &str) -> Self {
        Self {
            data_dir: Some(data_dir.to_string()),
        }
    }

    /// Parse DWD CSV data from a reader
    pub fn parse_csv<R: BufRead>(
        &self,
        reader: R,
        station_name: Option<&str>,
    ) -> Result<Vec<DailyWeather>> {
        let indices = DwdColumnIndices::default();
        let mut records = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            // Skip empty lines and header
            if line.is_empty() || line.starts_with("STATIONS_ID") {
                continue;
            }

            if let Some(record) = self.parse_line(line, &indices, station_name) {
                records.push(record);
            }
        }

        // Sort by date (newest first)
        records.sort_by(|a, b| b.date.cmp(&a.date));

        Ok(records)
    }

    /// Parse a single CSV line
    fn parse_line(
        &self,
        line: &str,
        indices: &DwdColumnIndices,
        station_name: Option<&str>,
    ) -> Option<DailyWeather> {
        let parts: Vec<&str> = line.split(';').map(|s| s.trim()).collect();

        if parts.len() < 18 {
            return None;
        }

        let station_id = parts.get(indices.station_id)?.trim().to_string();
        let date = parts.get(indices.date)?.trim().to_string();

        // Parse optional fields, treating -999 as missing
        let parse_f32 = |idx: usize| -> Option<f32> {
            parts.get(idx).and_then(|s| {
                let val: f32 = s.trim().parse().ok()?;
                if val <= -999.0 || val >= 999.0 {
                    None
                } else {
                    Some(val)
                }
            })
        };

        let parse_i32 = |idx: usize| -> Option<i32> {
            parts.get(idx).and_then(|s| {
                let val: i32 = s.trim().parse().ok()?;
                if val < 0 {
                    None
                } else {
                    Some(val)
                }
            })
        };

        Some(DailyWeather {
            date,
            station_id,
            station_name: station_name.map(String::from),
            temp_mean: parse_f32(indices.temp_mean),
            temp_max: parse_f32(indices.temp_max),
            temp_min: parse_f32(indices.temp_min),
            temp_ground_min: parse_f32(indices.temp_ground_min),
            humidity: parse_f32(indices.humidity),
            precipitation: parse_f32(indices.precipitation),
            sunshine_hours: parse_f32(indices.sunshine),
            snow_depth: parse_f32(indices.snow_depth),
            wind_speed: parse_f32(indices.wind_speed),
            wind_gust: parse_f32(indices.wind_gust),
            vapor_pressure: parse_f32(indices.vapor_pressure),
            air_pressure: parse_f32(indices.air_pressure),
            cloud_cover: parse_f32(indices.cloud_cover),
            quality_level: parse_i32(indices.quality_other),
        })
    }

    /// Load data from a local DWD data file
    pub fn load_from_file<P: AsRef<Path>>(&self, path: P) -> Result<Vec<DailyWeather>> {
        let file = std::fs::File::open(path.as_ref())?;
        let reader = std::io::BufReader::new(file);
        self.parse_csv(reader, None)
    }

    /// Load data from a local DWD data directory (e.g., tageswerte_KL_05717_akt)
    pub fn load_from_directory<P: AsRef<Path>>(&self, dir: P) -> Result<Vec<DailyWeather>> {
        let dir = dir.as_ref();

        // Find the produkt_*.txt file
        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("produkt_") && name_str.ends_with(".txt") {
                return self.load_from_file(entry.path());
            }
        }

        Err(AppError::Storage(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No produkt_*.txt file found in directory",
        )))
    }

    /// Build URL for downloading recent station data
    pub fn build_recent_url(&self, station_id: &str) -> String {
        let id = format!("{:0>5}", station_id);
        format!("{}/recent/tageswerte_KL_{}_akt.zip", DWD_BASE_URL, id)
    }

    /// Build URL for downloading historical station data
    pub fn build_historical_url(
        &self,
        station_id: &str,
        start_date: &str,
        end_date: &str,
    ) -> String {
        let id = format!("{:0>5}", station_id);
        format!(
            "{}/historical/tageswerte_KL_{}_{}_{}hist.zip",
            DWD_BASE_URL, id, start_date, end_date
        )
    }

    /// Fetch recent data for a station (async)
    #[cfg(feature = "async")]
    pub async fn fetch_recent(&self, station: &WeatherStation) -> Result<Vec<DailyWeather>> {
        use std::time::Duration;

        let url = self.build_recent_url(&station.id);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Failed to fetch DWD data: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::Network(format!(
                "DWD returned status: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;

        // Unzip and parse
        self.parse_zip(&bytes, Some(&station.name))
    }

    /// Fetch recent data for a station (blocking)
    #[cfg(feature = "blocking")]
    pub fn fetch_recent_blocking(&self, station: &WeatherStation) -> Result<Vec<DailyWeather>> {
        use std::time::Duration;

        let url = self.build_recent_url(&station.id);

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;

        let response = client
            .get(&url)
            .send()
            .map_err(|e| AppError::Network(format!("Failed to fetch DWD data: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::Network(format!(
                "DWD returned status: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .map_err(|e| AppError::Network(e.to_string()))?;

        // Unzip and parse
        self.parse_zip(&bytes, Some(&station.name))
    }

    /// Parse a ZIP archive containing DWD data
    #[cfg(any(feature = "async", feature = "blocking"))]
    fn parse_zip(&self, data: &[u8], station_name: Option<&str>) -> Result<Vec<DailyWeather>> {
        use std::io::Cursor;

        use std::io::Read;

        let cursor = Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
            AppError::Storage(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid ZIP: {}", e),
            ))
        })?;

        // Find the produkt_*.txt file
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                AppError::Storage(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ))
            })?;

            let name = file.name().to_string();
            if name.starts_with("produkt_") && name.ends_with(".txt") {
                let mut content = String::new();
                file.read_to_string(&mut content)?;
                let reader = std::io::BufReader::new(Cursor::new(content));
                return self.parse_csv(reader, station_name);
            }
        }

        Err(AppError::Storage(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No produkt_*.txt in ZIP archive",
        )))
    }
}

impl Default for DwdFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregate multiple daily records into statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherStatistics {
    /// Number of days analyzed
    pub days: usize,
    /// Temperature statistics
    pub temperature: TempStatistics,
    /// Humidity statistics
    pub humidity: Option<HumidityStatistics>,
    /// Precipitation total (mm)
    pub precipitation_total: Option<f32>,
    /// Total sunshine hours
    pub sunshine_total: Option<f32>,
    /// Heating degree days (base 15°C)
    pub heating_degree_days: f32,
    /// Cooling degree days (base 24°C)
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

impl WeatherStatistics {
    /// Calculate statistics from daily weather records
    pub fn from_records(records: &[DailyWeather]) -> Option<Self> {
        if records.is_empty() {
            return None;
        }

        // Temperature statistics
        let temps: Vec<f32> = records
            .iter()
            .filter_map(|r| r.best_temperature())
            .collect();

        if temps.is_empty() {
            return None;
        }

        let temp_mean = temps.iter().sum::<f32>() / temps.len() as f32;
        let temp_min = temps.iter().copied().fold(f32::INFINITY, f32::min);
        let temp_max = temps.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let variance =
            temps.iter().map(|t| (t - temp_mean).powi(2)).sum::<f32>() / temps.len() as f32;
        let temp_std_dev = variance.sqrt();

        // Humidity statistics
        let humidities: Vec<f32> = records.iter().filter_map(|r| r.humidity).collect();
        let humidity = if !humidities.is_empty() {
            Some(HumidityStatistics {
                mean: humidities.iter().sum::<f32>() / humidities.len() as f32,
                min: humidities.iter().copied().fold(f32::INFINITY, f32::min),
                max: humidities.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            })
        } else {
            None
        };

        // Precipitation total
        let precipitation_total: Option<f32> = {
            let precips: Vec<f32> = records.iter().filter_map(|r| r.precipitation).collect();
            if !precips.is_empty() {
                Some(precips.iter().sum())
            } else {
                None
            }
        };

        // Sunshine total
        let sunshine_total: Option<f32> = {
            let sunshine: Vec<f32> = records.iter().filter_map(|r| r.sunshine_hours).collect();
            if !sunshine.is_empty() {
                Some(sunshine.iter().sum())
            } else {
                None
            }
        };

        // Heating degree days (base 15°C - below this, heating is needed)
        let heating_degree_days = temps.iter().map(|t| (15.0 - t).max(0.0)).sum();

        // Cooling degree days (base 24°C - above this, cooling is needed)
        let cooling_degree_days = temps.iter().map(|t| (t - 24.0).max(0.0)).sum();

        Some(Self {
            days: records.len(),
            temperature: TempStatistics {
                mean: temp_mean,
                min: temp_min,
                max: temp_max,
                std_dev: temp_std_dev,
            },
            humidity,
            precipitation_total,
            sunshine_total,
            heating_degree_days,
            cooling_degree_days,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dwd_line() {
        let fetcher = DwdFetcher::new();
        let indices = DwdColumnIndices::default();

        let line = "5717;20240521;-999;-999;-999;10;73.5;4;-999;-999;-999;15.3;-999;15.2;90.00;23.1;9.9;7.1;eor";
        let record = fetcher.parse_line(line, &indices, Some("Bochum")).unwrap();

        assert_eq!(record.station_id, "5717");
        assert_eq!(record.date, "20240521");
        assert!((record.temp_mean.unwrap() - 15.2).abs() < 0.01);
        assert!((record.humidity.unwrap() - 90.0).abs() < 0.01);
        assert!((record.temp_max.unwrap() - 23.1).abs() < 0.01);
        assert!((record.temp_min.unwrap() - 9.9).abs() < 0.01);
    }

    #[test]
    fn test_build_urls() {
        let fetcher = DwdFetcher::new();

        let recent_url = fetcher.build_recent_url("5717");
        assert!(recent_url.contains("05717"));
        assert!(recent_url.contains("recent"));

        let historical_url = fetcher.build_historical_url("5717", "20230101", "20231231");
        assert!(historical_url.contains("05717"));
        assert!(historical_url.contains("historical"));
    }
}
