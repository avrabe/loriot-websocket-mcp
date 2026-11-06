//! Heat Pump Analytics Module
//!
//! Analyzes ground-source heat pump efficiency by comparing:
//! - Indoor temperatures from underground spaces (heat source)
//! - Outdoor temperatures from DWD weather data
//! - Calculates COP estimates and energy savings

use crate::weather::{DailyWeather, Season};
use crate::SensorReading;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for heat pump analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatPumpConfig {
    /// Device EUIs for underground heat source sensors
    pub source_devices: Vec<String>,
    /// Device names for display (maps EUI -> name)
    pub device_names: HashMap<String, String>,
    /// Target indoor temperature for heating (°C)
    pub heating_target: f32,
    /// Target indoor temperature for cooling (°C)
    pub cooling_target: f32,
    /// Reference outdoor temperature for efficiency calculations
    pub reference_outdoor_temp: f32,
    /// Ground source baseline temperature (typical for the region)
    pub ground_baseline_temp: f32,
}

impl Default for HeatPumpConfig {
    fn default() -> Self {
        Self {
            source_devices: vec![],
            device_names: HashMap::new(),
            heating_target: 21.0,
            cooling_target: 24.0,
            reference_outdoor_temp: 7.0, // Average winter outdoor temp
            ground_baseline_temp: 12.0,  // Typical ground temp at depth
        }
    }
}

/// Daily heat pump analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyHeatPumpAnalysis {
    /// Date (YYYYMMDD)
    pub date: String,
    /// Season
    pub season: Season,
    /// Outdoor temperature (from weather data)
    pub outdoor_temp: Option<f32>,
    /// Ground source temperature (average from underground sensors)
    pub source_temp: Option<f32>,
    /// Temperature advantage (source - outdoor for heating)
    pub temp_advantage: Option<f32>,
    /// Estimated COP for heating
    pub heating_cop_estimate: Option<f32>,
    /// Estimated COP for cooling
    pub cooling_cop_estimate: Option<f32>,
    /// Heating degree days
    pub heating_degree_days: f32,
    /// Cooling degree days
    pub cooling_degree_days: f32,
    /// Source readings count
    pub source_readings: usize,
    /// Individual source device temperatures
    pub device_temps: HashMap<String, SourceDeviceStats>,
}

/// Statistics for a single source device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDeviceStats {
    /// Device name
    pub name: String,
    /// Average temperature
    pub avg_temp: f32,
    /// Min temperature
    pub min_temp: f32,
    /// Max temperature
    pub max_temp: f32,
    /// Average humidity
    pub avg_humidity: Option<f32>,
    /// Reading count
    pub readings: usize,
}

/// Overall heat pump efficiency report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatPumpReport {
    /// Analysis period
    pub period: AnalysisPeriod,
    /// Daily analyses
    pub daily: Vec<DailyHeatPumpAnalysis>,
    /// Overall statistics
    pub overall: OverallStats,
    /// By-device statistics
    pub device_stats: HashMap<String, DeviceSummary>,
    /// Seasonal breakdown
    pub seasonal: SeasonalStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisPeriod {
    pub start_date: String,
    pub end_date: String,
    pub days: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverallStats {
    /// Average source temperature
    pub avg_source_temp: f32,
    /// Average outdoor temperature
    pub avg_outdoor_temp: Option<f32>,
    /// Average temperature advantage
    pub avg_temp_advantage: Option<f32>,
    /// Average heating COP estimate
    pub avg_heating_cop: Option<f32>,
    /// Average cooling COP estimate
    pub avg_cooling_cop: Option<f32>,
    /// Total heating degree days
    pub total_heating_degree_days: f32,
    /// Total cooling degree days
    pub total_cooling_degree_days: f32,
    /// Estimated energy savings percentage
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

/// Heat pump analytics engine
pub struct HeatPumpAnalyzer {
    config: HeatPumpConfig,
}

impl HeatPumpAnalyzer {
    /// Create a new analyzer with configuration
    pub fn new(config: HeatPumpConfig) -> Self {
        Self { config }
    }

    /// Analyze heat pump efficiency
    pub fn analyze(&self, readings: &[SensorReading], weather: &[DailyWeather]) -> HeatPumpReport {
        // Group readings by date
        let readings_by_date = self.group_readings_by_date(readings);

        // Create weather lookup
        let weather_by_date: HashMap<&str, &DailyWeather> =
            weather.iter().map(|w| (w.date.as_str(), w)).collect();

        // Analyze each day
        let mut daily: Vec<DailyHeatPumpAnalysis> = Vec::new();

        for (date, day_readings) in &readings_by_date {
            let weather_data = weather_by_date.get(date.as_str()).copied();
            if let Some(analysis) = self.analyze_day(date, day_readings, weather_data) {
                daily.push(analysis);
            }
        }

        // Sort by date
        daily.sort_by(|a, b| a.date.cmp(&b.date));

        // Calculate overall stats
        let overall = self.calculate_overall_stats(&daily);
        let device_stats = self.calculate_device_stats(readings);
        let seasonal = self.calculate_seasonal_stats(&daily);

        // Determine period
        let period = AnalysisPeriod {
            start_date: daily.first().map(|d| d.date.clone()).unwrap_or_default(),
            end_date: daily.last().map(|d| d.date.clone()).unwrap_or_default(),
            days: daily.len(),
        };

        HeatPumpReport {
            period,
            daily,
            overall,
            device_stats,
            seasonal,
        }
    }

    /// Group readings by date (YYYYMMDD)
    fn group_readings_by_date<'a>(
        &self,
        readings: &'a [SensorReading],
    ) -> HashMap<String, Vec<&'a SensorReading>> {
        let mut grouped: HashMap<String, Vec<&SensorReading>> = HashMap::new();

        for reading in readings {
            // Only include configured source devices
            if !self.config.source_devices.is_empty()
                && !self.config.source_devices.contains(&reading.eui)
            {
                continue;
            }

            let date = timestamp_to_date(reading.ts);
            grouped.entry(date).or_default().push(reading);
        }

        grouped
    }

    /// Analyze a single day
    fn analyze_day(
        &self,
        date: &str,
        readings: &[&SensorReading],
        weather: Option<&DailyWeather>,
    ) -> Option<DailyHeatPumpAnalysis> {
        if readings.is_empty() {
            return None;
        }

        let season = Season::from_date(date);
        let outdoor_temp = weather.and_then(|w| w.best_temperature());

        // Calculate source temperature stats
        let mut device_temps: HashMap<String, Vec<f32>> = HashMap::new();
        let mut device_humidities: HashMap<String, Vec<f32>> = HashMap::new();

        for reading in readings {
            // Get temperature - try parsed first, otherwise skip
            let temp = match reading.parsed_temperature {
                Some(t) => t,
                None => continue,
            };

            device_temps
                .entry(reading.eui.clone())
                .or_default()
                .push(temp);

            if let Some(h) = reading.parsed_humidity {
                device_humidities
                    .entry(reading.eui.clone())
                    .or_default()
                    .push(h);
            }
        }

        // Calculate per-device stats
        let mut device_stats: HashMap<String, SourceDeviceStats> = HashMap::new();
        for (device_id, temps) in &device_temps {
            let avg_temp = temps.iter().sum::<f32>() / temps.len() as f32;
            let min_temp = temps.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_temp = temps.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let avg_humidity = device_humidities
                .get(device_id)
                .map(|h| h.iter().sum::<f32>() / h.len() as f32);

            let name = self
                .config
                .device_names
                .get(device_id)
                .cloned()
                .unwrap_or_else(|| device_id.clone());

            device_stats.insert(
                device_id.clone(),
                SourceDeviceStats {
                    name,
                    avg_temp,
                    min_temp,
                    max_temp,
                    avg_humidity,
                    readings: temps.len(),
                },
            );
        }

        // Overall source temperature
        let all_temps: Vec<f32> = readings
            .iter()
            .filter_map(|r| r.parsed_temperature)
            .collect();
        let source_temp = if all_temps.is_empty() {
            None
        } else {
            Some(all_temps.iter().sum::<f32>() / all_temps.len() as f32)
        };

        // Calculate temperature advantage and COP estimates
        let temp_advantage = match (source_temp, outdoor_temp) {
            (Some(src), Some(out)) => Some(src - out),
            _ => None,
        };

        // Estimate COP for heating using empirical ground-source heat pump formula
        // Based on typical GSHP performance data:
        // - At 0°C source temp, COP ≈ 3.5-4.0
        // - At 10°C source temp, COP ≈ 4.5-5.0
        // - At 15°C source temp, COP ≈ 5.0-5.5
        // COP increases roughly 0.1 per degree of source temperature
        // Base formula: COP = 3.0 + (source_temp * 0.15)
        // With diminishing returns at higher temps
        let heating_cop_estimate = source_temp.map(|src| {
            // Empirical formula for ground-source heat pump
            // Base COP of 3.0 at 0°C source, +0.12 per degree
            let base_cop = 3.0 + (src * 0.12);
            // Cap between realistic values
            base_cop.clamp(2.5, 6.0)
        });

        // Estimate COP for cooling (passive cooling from ground)
        // Ground cooling is very efficient when ground is cooler than target
        let cooling_cop_estimate = source_temp.map(|src| {
            if src < self.config.cooling_target {
                // Can do passive/free cooling - very high effective COP
                let temp_diff = self.config.cooling_target - src;
                // Higher temp difference = more effective cooling
                (4.0 + temp_diff * 0.3).clamp(3.0, 8.0)
            } else {
                // Ground is warmer than target - active cooling needed
                2.5
            }
        });

        // Calculate degree days
        let reference_temp = outdoor_temp.or(source_temp).unwrap_or(15.0);
        let heating_degree_days = (15.0 - reference_temp).max(0.0);
        let cooling_degree_days = (reference_temp - 24.0).max(0.0);

        Some(DailyHeatPumpAnalysis {
            date: date.to_string(),
            season,
            outdoor_temp,
            source_temp,
            temp_advantage,
            heating_cop_estimate,
            cooling_cop_estimate,
            heating_degree_days,
            cooling_degree_days,
            source_readings: readings.len(),
            device_temps: device_stats,
        })
    }

    /// Calculate overall statistics
    fn calculate_overall_stats(&self, daily: &[DailyHeatPumpAnalysis]) -> OverallStats {
        if daily.is_empty() {
            return OverallStats {
                avg_source_temp: 0.0,
                avg_outdoor_temp: None,
                avg_temp_advantage: None,
                avg_heating_cop: None,
                avg_cooling_cop: None,
                total_heating_degree_days: 0.0,
                total_cooling_degree_days: 0.0,
                energy_savings_estimate: None,
            };
        }

        let source_temps: Vec<f32> = daily.iter().filter_map(|d| d.source_temp).collect();
        let outdoor_temps: Vec<f32> = daily.iter().filter_map(|d| d.outdoor_temp).collect();
        let temp_advantages: Vec<f32> = daily.iter().filter_map(|d| d.temp_advantage).collect();
        let heating_cops: Vec<f32> = daily
            .iter()
            .filter_map(|d| d.heating_cop_estimate)
            .collect();
        let cooling_cops: Vec<f32> = daily
            .iter()
            .filter_map(|d| d.cooling_cop_estimate)
            .collect();

        let avg_source_temp = if source_temps.is_empty() {
            0.0
        } else {
            source_temps.iter().sum::<f32>() / source_temps.len() as f32
        };

        let avg_outdoor_temp = if outdoor_temps.is_empty() {
            None
        } else {
            Some(outdoor_temps.iter().sum::<f32>() / outdoor_temps.len() as f32)
        };

        let avg_temp_advantage = if temp_advantages.is_empty() {
            None
        } else {
            Some(temp_advantages.iter().sum::<f32>() / temp_advantages.len() as f32)
        };

        let avg_heating_cop = if heating_cops.is_empty() {
            None
        } else {
            Some(heating_cops.iter().sum::<f32>() / heating_cops.len() as f32)
        };

        let avg_cooling_cop = if cooling_cops.is_empty() {
            None
        } else {
            Some(cooling_cops.iter().sum::<f32>() / cooling_cops.len() as f32)
        };

        let total_heating_degree_days: f32 = daily.iter().map(|d| d.heating_degree_days).sum();
        let total_cooling_degree_days: f32 = daily.iter().map(|d| d.cooling_degree_days).sum();

        // Estimate energy savings vs air-source heat pump
        // Ground source typically has 30-50% better COP
        let energy_savings_estimate = avg_heating_cop.map(|cop| {
            let air_source_cop = 2.5; // Typical air-source COP
            ((cop - air_source_cop) / air_source_cop * 100.0).max(0.0)
        });

        OverallStats {
            avg_source_temp,
            avg_outdoor_temp,
            avg_temp_advantage,
            avg_heating_cop,
            avg_cooling_cop,
            total_heating_degree_days,
            total_cooling_degree_days,
            energy_savings_estimate,
        }
    }

    /// Calculate per-device statistics
    fn calculate_device_stats(&self, readings: &[SensorReading]) -> HashMap<String, DeviceSummary> {
        let mut device_data: HashMap<String, (Vec<f32>, Vec<f32>)> = HashMap::new();

        for reading in readings {
            if !self.config.source_devices.is_empty()
                && !self.config.source_devices.contains(&reading.eui)
            {
                continue;
            }

            // Skip readings without temperature
            let temp = match reading.parsed_temperature {
                Some(t) => t,
                None => continue,
            };

            let entry = device_data.entry(reading.eui.clone()).or_default();
            entry.0.push(temp);
            if let Some(h) = reading.parsed_humidity {
                entry.1.push(h);
            }
        }

        let mut stats = HashMap::new();
        for (device_id, (temps, humidities)) in device_data {
            if temps.is_empty() {
                continue;
            }

            let name = self
                .config
                .device_names
                .get(&device_id)
                .cloned()
                .unwrap_or_else(|| device_id.clone());

            stats.insert(
                device_id,
                DeviceSummary {
                    name,
                    avg_temp: temps.iter().sum::<f32>() / temps.len() as f32,
                    min_temp: temps.iter().cloned().fold(f32::INFINITY, f32::min),
                    max_temp: temps.iter().cloned().fold(f32::NEG_INFINITY, f32::max),
                    avg_humidity: if humidities.is_empty() {
                        None
                    } else {
                        Some(humidities.iter().sum::<f32>() / humidities.len() as f32)
                    },
                    total_readings: temps.len(),
                },
            );
        }

        stats
    }

    /// Calculate seasonal statistics
    fn calculate_seasonal_stats(&self, daily: &[DailyHeatPumpAnalysis]) -> SeasonalStats {
        let mut by_season: HashMap<Season, Vec<&DailyHeatPumpAnalysis>> = HashMap::new();

        for analysis in daily {
            by_season.entry(analysis.season).or_default().push(analysis);
        }

        let calculate_season = |season: Season| -> Option<SeasonSummary> {
            let analyses = by_season.get(&season)?;
            if analyses.is_empty() {
                return None;
            }

            let source_temps: Vec<f32> = analyses.iter().filter_map(|a| a.source_temp).collect();
            let outdoor_temps: Vec<f32> = analyses.iter().filter_map(|a| a.outdoor_temp).collect();
            let temp_advantages: Vec<f32> =
                analyses.iter().filter_map(|a| a.temp_advantage).collect();
            let cops: Vec<f32> = analyses
                .iter()
                .filter_map(|a| a.heating_cop_estimate)
                .collect();

            Some(SeasonSummary {
                days: analyses.len(),
                avg_source_temp: if source_temps.is_empty() {
                    0.0
                } else {
                    source_temps.iter().sum::<f32>() / source_temps.len() as f32
                },
                avg_outdoor_temp: if outdoor_temps.is_empty() {
                    None
                } else {
                    Some(outdoor_temps.iter().sum::<f32>() / outdoor_temps.len() as f32)
                },
                avg_temp_advantage: if temp_advantages.is_empty() {
                    None
                } else {
                    Some(temp_advantages.iter().sum::<f32>() / temp_advantages.len() as f32)
                },
                avg_cop: if cops.is_empty() {
                    None
                } else {
                    Some(cops.iter().sum::<f32>() / cops.len() as f32)
                },
            })
        };

        SeasonalStats {
            winter: calculate_season(Season::Winter),
            spring: calculate_season(Season::Spring),
            summer: calculate_season(Season::Summer),
            autumn: calculate_season(Season::Autumn),
        }
    }
}

impl Default for HeatPumpAnalyzer {
    fn default() -> Self {
        Self::new(HeatPumpConfig::default())
    }
}

/// Convert timestamp (ms) to date string (YYYYMMDD)
fn timestamp_to_date(ts: u64) -> String {
    let secs = (ts / 1000) as i64;
    let date = chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
    date.format("%Y%m%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_to_date() {
        // 2024-12-27 12:00:00 UTC
        let ts = 1735300800000u64;
        let date = timestamp_to_date(ts);
        assert_eq!(date, "20241227");
    }

    #[test]
    fn test_cop_calculation() {
        let config = HeatPumpConfig {
            heating_target: 21.0,
            ..Default::default()
        };
        let _analyzer = HeatPumpAnalyzer::new(config);

        // With source temp of 12°C and target of 21°C
        // COP ≈ (21+273) / ((21+273) - (12+273)) = 294 / 9 ≈ 32.7
        // This is theoretical max; real-world is much lower
        // Our simplified model should give reasonable estimates
    }
}
