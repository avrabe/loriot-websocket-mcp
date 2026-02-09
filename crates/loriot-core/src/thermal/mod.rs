//! Thermal analysis module for building insulation quality assessment
//!
//! This module correlates indoor sensor data with outdoor weather data
//! to analyze building thermal performance and insulation quality.

use crate::sensors::SensorReading;
use crate::weather::DailyWeather;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of thermal correlation analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalAnalysisReport {
    /// Analysis period info
    pub period: AnalysisPeriod,
    /// Per-device thermal performance
    pub device_analysis: Vec<DeviceThermalAnalysis>,
    /// Building-wide metrics (aggregated)
    pub building_metrics: BuildingThermalMetrics,
    /// Heating degree days for the period
    pub heating_degree_days: f64,
    /// Cooling degree days for the period
    pub cooling_degree_days: f64,
    /// Data quality indicators
    pub data_quality: DataQuality,
}

/// Analysis period information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisPeriod {
    /// Start date (YYYYMMDD format)
    pub start_date: String,
    /// End date (YYYYMMDD format)
    pub end_date: String,
    /// Number of days with data
    pub days_with_data: usize,
    /// Total sensor readings analyzed
    pub total_readings: usize,
    /// Total weather records used
    pub weather_records: usize,
}

/// Thermal analysis for a single sensor/device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceThermalAnalysis {
    /// Device EUI
    pub device_id: String,
    /// Device name (if available)
    pub device_name: Option<String>,
    /// Average indoor temperature
    pub avg_indoor_temp: f32,
    /// Indoor temperature range (min to max)
    pub indoor_temp_range: (f32, f32),
    /// Temperature stability (std deviation)
    pub temp_stability: f32,
    /// Correlation with outdoor temperature (-1 to 1)
    pub outdoor_correlation: f32,
    /// Temperature damping factor (indoor swing / outdoor swing)
    pub damping_factor: f32,
    /// Estimated thermal lag (hours)
    pub thermal_lag_hours: Option<f32>,
    /// Average temperature difference (indoor - outdoor)
    pub avg_temp_difference: f32,
    /// Daily temperature profiles
    pub daily_profiles: Vec<DailyThermalProfile>,
}

/// Daily thermal profile for a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyThermalProfile {
    /// Date (YYYYMMDD format)
    pub date: String,
    /// Average indoor temperature for the day
    pub avg_indoor_temp: f32,
    /// Indoor temperature range
    pub indoor_range: (f32, f32),
    /// Outdoor mean temperature (from weather)
    pub outdoor_temp: Option<f32>,
    /// Outdoor temperature range (min to max)
    pub outdoor_range: Option<(f32, f32)>,
    /// Temperature difference (indoor - outdoor mean)
    pub temp_difference: Option<f32>,
    /// Number of readings
    pub reading_count: usize,
}

/// Building-wide thermal metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildingThermalMetrics {
    /// Average indoor temperature across all sensors
    pub avg_indoor_temp: f32,
    /// Average outdoor temperature for the period
    pub avg_outdoor_temp: Option<f32>,
    /// Average temperature difference (indoor - outdoor)
    pub avg_temp_difference: Option<f32>,
    /// Building thermal stability score (0-100)
    pub stability_score: f32,
    /// Estimated insulation quality rating
    pub insulation_rating: InsulationRating,
    /// Daily energy demand indicator (arbitrary units)
    pub energy_demand_indicator: f32,
    /// Temperature uniformity across sensors (0-1, higher = more uniform)
    pub uniformity_score: f32,
}

/// Insulation quality rating based on thermal analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsulationRating {
    /// Excellent insulation (modern passive house level)
    Excellent,
    /// Good insulation (modern building standards)
    Good,
    /// Average insulation (typical older buildings)
    Average,
    /// Poor insulation (significant heat loss)
    Poor,
    /// Insufficient data for rating
    Unknown,
}

impl InsulationRating {
    pub fn description(&self) -> &'static str {
        match self {
            Self::Excellent => "Ausgezeichnet - Sehr gute Dämmung/thermische Masse",
            Self::Good => "Gut - Stabile Temperaturen, gute thermische Masse",
            Self::Average => "Durchschnittlich - Moderate Temperaturschwankungen",
            Self::Poor => "Verbesserungswürdig - Starke Temperaturschwankungen",
            Self::Unknown => "Unbekannt - Unzureichende Daten",
        }
    }

    pub fn color_class(&self) -> &'static str {
        match self {
            Self::Excellent => "text-success",
            Self::Good => "text-info",
            Self::Average => "text-warning",
            Self::Poor => "text-error",
            Self::Unknown => "text-base-content/50",
        }
    }
}

/// Data quality indicators for the analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataQuality {
    /// Percentage of days with both indoor and outdoor data (0-100)
    pub coverage_percent: f32,
    /// Average readings per device per day
    pub avg_readings_per_day: f32,
    /// Number of devices with sufficient data
    pub devices_with_data: usize,
    /// Weather data availability
    pub weather_data_available: bool,
    /// Quality rating
    pub quality_rating: QualityRating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityRating {
    Excellent,
    Good,
    Fair,
    Poor,
}

/// Thermal analyzer that performs the correlation analysis
pub struct ThermalAnalyzer {
    /// Base temperature for heating degree days (typically 15°C in Germany)
    pub heating_base_temp: f32,
    /// Base temperature for cooling degree days (typically 18°C)
    pub cooling_base_temp: f32,
}

impl Default for ThermalAnalyzer {
    fn default() -> Self {
        Self {
            heating_base_temp: 15.0,
            cooling_base_temp: 18.0,
        }
    }
}

impl ThermalAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run thermal analysis correlating sensor readings with weather data
    pub fn analyze(
        &self,
        readings: &[SensorReading],
        weather: &[DailyWeather],
    ) -> ThermalAnalysisReport {
        // Group readings by device and date
        let readings_by_device_date = self.group_readings_by_device_date(readings);

        // Create weather lookup by date
        let weather_by_date: HashMap<&str, &DailyWeather> = weather
            .iter()
            .map(|w| (w.date.as_str(), w))
            .collect();

        // Analyze each device
        let mut device_analysis: Vec<DeviceThermalAnalysis> = Vec::new();

        for (device_id, by_date) in &readings_by_device_date {
            if let Some(analysis) = self.analyze_device(device_id, by_date, &weather_by_date) {
                device_analysis.push(analysis);
            }
        }

        // Calculate building-wide metrics
        let building_metrics = self.calculate_building_metrics(&device_analysis, weather);

        // Calculate degree days
        let (heating_dd, cooling_dd) = self.calculate_degree_days(weather);

        // Determine period
        let period = self.calculate_period(readings, weather);

        // Assess data quality
        let data_quality = self.assess_data_quality(&period, &device_analysis, weather);

        ThermalAnalysisReport {
            period,
            device_analysis,
            building_metrics,
            heating_degree_days: heating_dd,
            cooling_degree_days: cooling_dd,
            data_quality,
        }
    }

    /// Group readings by device and then by date
    fn group_readings_by_device_date<'a>(
        &self,
        readings: &'a [SensorReading],
    ) -> HashMap<String, HashMap<String, Vec<&'a SensorReading>>> {
        let mut result: HashMap<String, HashMap<String, Vec<&SensorReading>>> = HashMap::new();

        for reading in readings {
            // Convert timestamp to date (YYYYMMDD)
            let date = timestamp_to_date(reading.ts);

            result
                .entry(reading.eui.clone())
                .or_default()
                .entry(date)
                .or_default()
                .push(reading);
        }

        result
    }

    /// Analyze thermal performance for a single device
    fn analyze_device(
        &self,
        device_id: &str,
        readings_by_date: &HashMap<String, Vec<&SensorReading>>,
        weather_by_date: &HashMap<&str, &DailyWeather>,
    ) -> Option<DeviceThermalAnalysis> {
        if readings_by_date.is_empty() {
            return None;
        }

        let device_name = readings_by_date
            .values()
            .flatten()
            .find_map(|r| r.name.clone());

        let mut all_indoor_temps: Vec<f32> = Vec::new();
        let mut daily_profiles: Vec<DailyThermalProfile> = Vec::new();
        let mut temp_differences: Vec<f32> = Vec::new();
        let mut indoor_swings: Vec<f32> = Vec::new();
        let mut outdoor_swings: Vec<f32> = Vec::new();

        for (date, readings) in readings_by_date {
            let indoor_temps: Vec<f32> = readings
                .iter()
                .filter_map(|r| r.parsed_temperature)
                .collect();

            if indoor_temps.is_empty() {
                continue;
            }

            let avg_indoor = indoor_temps.iter().sum::<f32>() / indoor_temps.len() as f32;
            let min_indoor = indoor_temps.iter().cloned().fold(f32::MAX, f32::min);
            let max_indoor = indoor_temps.iter().cloned().fold(f32::MIN, f32::max);
            let indoor_swing = max_indoor - min_indoor;

            all_indoor_temps.extend(indoor_temps.iter());
            indoor_swings.push(indoor_swing);

            // Match with weather data
            let weather = weather_by_date.get(date.as_str());
            let outdoor_temp = weather.and_then(|w| w.temp_mean);
            let outdoor_range = weather.and_then(|w| {
                match (w.temp_min, w.temp_max) {
                    (Some(min), Some(max)) => Some((min, max)),
                    _ => None,
                }
            });

            if let Some(outdoor) = outdoor_temp {
                temp_differences.push(avg_indoor - outdoor);
            }

            if let Some((min, max)) = outdoor_range {
                outdoor_swings.push(max - min);
            }

            daily_profiles.push(DailyThermalProfile {
                date: date.clone(),
                avg_indoor_temp: avg_indoor,
                indoor_range: (min_indoor, max_indoor),
                outdoor_temp,
                outdoor_range,
                temp_difference: outdoor_temp.map(|o| avg_indoor - o),
                reading_count: readings.len(),
            });
        }

        if all_indoor_temps.is_empty() {
            return None;
        }

        // Sort daily profiles by date
        daily_profiles.sort_by(|a, b| a.date.cmp(&b.date));

        // Calculate aggregate metrics
        let avg_indoor_temp = all_indoor_temps.iter().sum::<f32>() / all_indoor_temps.len() as f32;
        let min_indoor = all_indoor_temps.iter().cloned().fold(f32::MAX, f32::min);
        let max_indoor = all_indoor_temps.iter().cloned().fold(f32::MIN, f32::max);

        // Temperature stability (standard deviation)
        let variance = all_indoor_temps
            .iter()
            .map(|t| (t - avg_indoor_temp).powi(2))
            .sum::<f32>()
            / all_indoor_temps.len() as f32;
        let temp_stability = variance.sqrt();

        // Correlation coefficient with outdoor temperature
        let outdoor_correlation = self.calculate_correlation(&daily_profiles);

        // Damping factor (how much the building dampens outdoor temperature swings)
        let damping_factor = if !indoor_swings.is_empty() && !outdoor_swings.is_empty() {
            let avg_indoor_swing = indoor_swings.iter().sum::<f32>() / indoor_swings.len() as f32;
            let avg_outdoor_swing = outdoor_swings.iter().sum::<f32>() / outdoor_swings.len() as f32;
            if avg_outdoor_swing > 0.0 {
                avg_indoor_swing / avg_outdoor_swing
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Average temperature difference
        let avg_temp_difference = if !temp_differences.is_empty() {
            temp_differences.iter().sum::<f32>() / temp_differences.len() as f32
        } else {
            0.0
        };

        Some(DeviceThermalAnalysis {
            device_id: device_id.to_string(),
            device_name,
            avg_indoor_temp,
            indoor_temp_range: (min_indoor, max_indoor),
            temp_stability,
            outdoor_correlation,
            damping_factor,
            thermal_lag_hours: None, // Would need hourly weather data for accurate lag estimation
            avg_temp_difference,
            daily_profiles,
        })
    }

    /// Calculate Pearson correlation coefficient between indoor and outdoor temperatures
    fn calculate_correlation(&self, profiles: &[DailyThermalProfile]) -> f32 {
        let paired: Vec<(f32, f32)> = profiles
            .iter()
            .filter_map(|p| p.outdoor_temp.map(|o| (p.avg_indoor_temp, o)))
            .collect();

        if paired.len() < 3 {
            return 0.0;
        }

        let n = paired.len() as f32;
        let sum_x: f32 = paired.iter().map(|(x, _)| x).sum();
        let sum_y: f32 = paired.iter().map(|(_, y)| y).sum();
        let sum_xy: f32 = paired.iter().map(|(x, y)| x * y).sum();
        let sum_x2: f32 = paired.iter().map(|(x, _)| x * x).sum();
        let sum_y2: f32 = paired.iter().map(|(_, y)| y * y).sum();

        let numerator = n * sum_xy - sum_x * sum_y;
        let denominator = ((n * sum_x2 - sum_x * sum_x) * (n * sum_y2 - sum_y * sum_y)).sqrt();

        if denominator.abs() < 0.001 {
            0.0
        } else {
            (numerator / denominator).clamp(-1.0, 1.0)
        }
    }

    /// Calculate building-wide thermal metrics
    fn calculate_building_metrics(
        &self,
        device_analysis: &[DeviceThermalAnalysis],
        _weather: &[DailyWeather],
    ) -> BuildingThermalMetrics {
        if device_analysis.is_empty() {
            return BuildingThermalMetrics {
                avg_indoor_temp: 0.0,
                avg_outdoor_temp: None,
                avg_temp_difference: None,
                stability_score: 0.0,
                insulation_rating: InsulationRating::Unknown,
                energy_demand_indicator: 0.0,
                uniformity_score: 0.0,
            };
        }

        // Average indoor temperature across all devices
        let avg_indoor_temp = device_analysis
            .iter()
            .map(|d| d.avg_indoor_temp)
            .sum::<f32>()
            / device_analysis.len() as f32;

        // Calculate outdoor temp ONLY from days with indoor readings (from daily profiles)
        let mut outdoor_temps: Vec<f32> = Vec::new();
        for device in device_analysis {
            for profile in &device.daily_profiles {
                if let Some(outdoor) = profile.outdoor_temp {
                    outdoor_temps.push(outdoor);
                }
            }
        }
        let avg_outdoor_temp = if !outdoor_temps.is_empty() {
            Some(outdoor_temps.iter().sum::<f32>() / outdoor_temps.len() as f32)
        } else {
            None
        };

        // Average temperature difference from actual daily comparisons
        let avg_temp_difference = if !device_analysis.is_empty() {
            let total_diff: f32 = device_analysis.iter().map(|d| d.avg_temp_difference).sum();
            Some(total_diff / device_analysis.len() as f32)
        } else {
            None
        };

        // Stability score (0-100, based on average temperature stability)
        let avg_stability = device_analysis
            .iter()
            .map(|d| d.temp_stability)
            .sum::<f32>()
            / device_analysis.len() as f32;
        // Lower stability (std dev) = higher score
        let stability_score = (100.0 - avg_stability * 20.0).clamp(0.0, 100.0);

        // Temperature uniformity (how similar are temperatures across devices)
        let uniformity_score = if device_analysis.len() > 1 {
            let temps: Vec<f32> = device_analysis.iter().map(|d| d.avg_indoor_temp).collect();
            let mean = temps.iter().sum::<f32>() / temps.len() as f32;
            let variance = temps.iter().map(|t| (t - mean).powi(2)).sum::<f32>() / temps.len() as f32;
            let std_dev = variance.sqrt();
            // Lower std dev = higher uniformity (normalize to 0-1)
            (1.0 - std_dev / 5.0).clamp(0.0, 1.0)
        } else {
            1.0
        };

        // Insulation rating based on damping factor and correlation
        let avg_damping = device_analysis
            .iter()
            .map(|d| d.damping_factor)
            .sum::<f32>()
            / device_analysis.len() as f32;
        let avg_correlation = device_analysis
            .iter()
            .map(|d| d.outdoor_correlation.abs())
            .sum::<f32>()
            / device_analysis.len() as f32;

        let insulation_rating = self.rate_insulation(avg_damping, avg_correlation);

        // Energy demand indicator (higher = more heating/cooling needed)
        // Based on average temperature difference from comfort zone (20°C)
        let energy_demand_indicator = if let Some(diff) = avg_temp_difference {
            if diff > 0.0 {
                // Indoor warmer than outdoor (heating season demand)
                diff * (1.0 - avg_damping).max(0.1)
            } else {
                // Indoor cooler than outdoor (cooling season demand)
                diff.abs() * (1.0 - avg_damping).max(0.1)
            }
        } else {
            0.0
        };

        BuildingThermalMetrics {
            avg_indoor_temp,
            avg_outdoor_temp,
            avg_temp_difference,
            stability_score,
            insulation_rating,
            energy_demand_indicator,
            uniformity_score,
        }
    }

    /// Rate insulation quality based on thermal metrics
    ///
    /// For unheated spaces (high correlation), damping is the key metric.
    /// For heated spaces (low correlation), both matter.
    fn rate_insulation(&self, damping_factor: f32, correlation: f32) -> InsulationRating {
        // Low damping = good thermal mass (temperature swings are dampened)
        // Low correlation = space is well-insulated/heated (doesn't follow outdoor)

        // Excellent thermal mass - indoor swings < 15% of outdoor
        if damping_factor < 0.15 {
            if correlation < 0.5 {
                // Low correlation + low damping = excellent heated space
                InsulationRating::Excellent
            } else {
                // High correlation but low damping = excellent unheated space with thermal mass
                InsulationRating::Good
            }
        } else if damping_factor < 0.3 {
            if correlation < 0.6 {
                InsulationRating::Good
            } else {
                InsulationRating::Average
            }
        } else if damping_factor < 0.5 {
            InsulationRating::Average
        } else {
            InsulationRating::Poor
        }
    }

    /// Calculate heating and cooling degree days
    fn calculate_degree_days(&self, weather: &[DailyWeather]) -> (f64, f64) {
        let mut heating_dd = 0.0;
        let mut cooling_dd = 0.0;

        for w in weather {
            if let Some(temp) = w.temp_mean {
                if temp < self.heating_base_temp {
                    heating_dd += (self.heating_base_temp - temp) as f64;
                }
                if temp > self.cooling_base_temp {
                    cooling_dd += (temp - self.cooling_base_temp) as f64;
                }
            }
        }

        (heating_dd, cooling_dd)
    }

    /// Calculate analysis period info
    fn calculate_period(
        &self,
        readings: &[SensorReading],
        weather: &[DailyWeather],
    ) -> AnalysisPeriod {
        let reading_dates: std::collections::HashSet<String> = readings
            .iter()
            .map(|r| timestamp_to_date(r.ts))
            .collect();

        let weather_dates: std::collections::HashSet<&str> = weather
            .iter()
            .map(|w| w.date.as_str())
            .collect();

        let all_dates: std::collections::HashSet<&str> = reading_dates
            .iter()
            .map(|s| s.as_str())
            .chain(weather_dates.iter().cloned())
            .collect();

        let mut dates: Vec<&str> = all_dates.into_iter().collect();
        dates.sort();

        AnalysisPeriod {
            start_date: dates.first().map(|s| s.to_string()).unwrap_or_default(),
            end_date: dates.last().map(|s| s.to_string()).unwrap_or_default(),
            days_with_data: reading_dates.len(),
            total_readings: readings.len(),
            weather_records: weather.len(),
        }
    }

    /// Assess data quality
    fn assess_data_quality(
        &self,
        period: &AnalysisPeriod,
        device_analysis: &[DeviceThermalAnalysis],
        weather: &[DailyWeather],
    ) -> DataQuality {
        let days_total = if !period.start_date.is_empty() && !period.end_date.is_empty() {
            // Approximate days from date strings
            let start = period.start_date.parse::<u32>().unwrap_or(0);
            let end = period.end_date.parse::<u32>().unwrap_or(0);
            ((end - start) / 100).max(1) as usize // Very rough approximation
        } else {
            1
        };

        let coverage_percent = if days_total > 0 {
            (period.days_with_data as f32 / days_total as f32 * 100.0).min(100.0)
        } else {
            0.0
        };

        let avg_readings_per_day = if period.days_with_data > 0 && !device_analysis.is_empty() {
            period.total_readings as f32 / (period.days_with_data * device_analysis.len()) as f32
        } else {
            0.0
        };

        let devices_with_data = device_analysis.len();
        let weather_data_available = !weather.is_empty();

        let quality_rating = if coverage_percent > 80.0 && avg_readings_per_day > 50.0 && weather_data_available {
            QualityRating::Excellent
        } else if coverage_percent > 60.0 && avg_readings_per_day > 30.0 && weather_data_available {
            QualityRating::Good
        } else if coverage_percent > 40.0 && avg_readings_per_day > 10.0 {
            QualityRating::Fair
        } else {
            QualityRating::Poor
        };

        DataQuality {
            coverage_percent,
            avg_readings_per_day,
            devices_with_data,
            weather_data_available,
            quality_rating,
        }
    }
}

/// Convert timestamp (ms) to date string (YYYYMMDD)
fn timestamp_to_date(ts: u64) -> String {
    // Convert milliseconds to seconds and create date
    let secs = (ts / 1000) as i64;
    let dt = chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
    dt.format("%Y%m%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_to_date() {
        // 2024-12-28 00:00:00 UTC
        let ts = 1735344000000u64;
        let date = timestamp_to_date(ts);
        assert_eq!(date, "20241228");
    }

    #[test]
    fn test_insulation_rating_description() {
        assert!(!InsulationRating::Excellent.description().is_empty());
        assert!(!InsulationRating::Poor.description().is_empty());
    }

    #[test]
    fn test_empty_analysis() {
        let analyzer = ThermalAnalyzer::new();
        let report = analyzer.analyze(&[], &[]);

        assert_eq!(report.device_analysis.len(), 0);
        assert_eq!(report.building_metrics.insulation_rating, InsulationRating::Unknown);
    }
}
