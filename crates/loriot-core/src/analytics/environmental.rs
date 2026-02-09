//! Environmental analysis including heat index, dew point, and comfort zones

use crate::sensors::SensorReading;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentalMetrics {
    pub heat_index: HeatIndexAnalysis,
    pub dew_point: DewPointAnalysis,
    pub comfort_zone: ComfortZoneAnalysis,
    pub vapor_pressure: VaporPressureAnalysis,
    pub absolute_humidity: AbsoluteHumidityAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatIndexAnalysis {
    pub min: f32,
    pub max: f32,
    pub average: f32,
    pub data_points: Vec<(u64, f32)>,
    pub dangerous_periods: Vec<DangerousPeriod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DewPointAnalysis {
    pub min: f32,
    pub max: f32,
    pub average: f32,
    pub data_points: Vec<(u64, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfortZoneAnalysis {
    pub comfort_percentage: f64,
    pub uncomfortable_hot_percentage: f64,
    pub uncomfortable_cold_percentage: f64,
    pub uncomfortable_humid_percentage: f64,
    pub uncomfortable_dry_percentage: f64,
    pub comfort_periods: Vec<ComfortPeriod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaporPressureAnalysis {
    pub min: f32,
    pub max: f32,
    pub average: f32,
    pub data_points: Vec<(u64, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbsoluteHumidityAnalysis {
    pub min: f32,
    pub max: f32,
    pub average: f32,
    pub data_points: Vec<(u64, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DangerousPeriod {
    pub start_timestamp: u64,
    pub end_timestamp: u64,
    pub max_heat_index: f32,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfortPeriod {
    pub start_timestamp: u64,
    pub end_timestamp: u64,
    pub comfort_type: ComfortType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RiskLevel {
    Caution,
    ExtremeHeat,
    Danger,
    ExtremeDanger,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComfortType {
    Optimal,
    Acceptable,
    SlightlyWarm,
    SlightlyCool,
    TooHot,
    TooCold,
    TooHumid,
    TooDry,
}

impl Default for EnvironmentalMetrics {
    fn default() -> Self {
        Self {
            heat_index: HeatIndexAnalysis {
                min: 0.0,
                max: 0.0,
                average: 0.0,
                data_points: vec![],
                dangerous_periods: vec![],
            },
            dew_point: DewPointAnalysis {
                min: 0.0,
                max: 0.0,
                average: 0.0,
                data_points: vec![],
            },
            comfort_zone: ComfortZoneAnalysis {
                comfort_percentage: 0.0,
                uncomfortable_hot_percentage: 0.0,
                uncomfortable_cold_percentage: 0.0,
                uncomfortable_humid_percentage: 0.0,
                uncomfortable_dry_percentage: 0.0,
                comfort_periods: vec![],
            },
            vapor_pressure: VaporPressureAnalysis {
                min: 0.0,
                max: 0.0,
                average: 0.0,
                data_points: vec![],
            },
            absolute_humidity: AbsoluteHumidityAnalysis {
                min: 0.0,
                max: 0.0,
                average: 0.0,
                data_points: vec![],
            },
        }
    }
}

pub struct EnvironmentalAnalyzer;

impl EnvironmentalAnalyzer {
    pub fn analyze(readings: &[&SensorReading]) -> EnvironmentalMetrics {
        let env_readings: Vec<_> = readings
            .iter()
            .filter(|r| r.parsed_temperature.is_some() && r.parsed_humidity.is_some())
            .collect();

        if env_readings.is_empty() {
            return EnvironmentalMetrics::default();
        }

        let heat_index = Self::calculate_heat_index_analysis(&env_readings);
        let dew_point = Self::calculate_dew_point_analysis(&env_readings);
        let comfort_zone = Self::analyze_comfort_zones(&env_readings);
        let vapor_pressure = Self::calculate_vapor_pressure_analysis(&env_readings);
        let absolute_humidity = Self::calculate_absolute_humidity_analysis(&env_readings);

        EnvironmentalMetrics {
            heat_index,
            dew_point,
            comfort_zone,
            vapor_pressure,
            absolute_humidity,
        }
    }

    fn calculate_heat_index_analysis(readings: &[&&SensorReading]) -> HeatIndexAnalysis {
        let mut heat_indices = Vec::new();
        let mut dangerous_periods = Vec::new();

        for reading in readings {
            if let (Some(temp), Some(humidity)) =
                (reading.parsed_temperature, reading.parsed_humidity)
            {
                let heat_index = Self::calculate_heat_index(temp, humidity);
                heat_indices.push((reading.ts, heat_index));
            }
        }

        if heat_indices.is_empty() {
            return HeatIndexAnalysis {
                min: 0.0,
                max: 0.0,
                average: 0.0,
                data_points: vec![],
                dangerous_periods: vec![],
            };
        }

        let values: Vec<f32> = heat_indices.iter().map(|(_, hi)| *hi).collect();
        let min = values.iter().copied().fold(f32::INFINITY, f32::min);
        let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let average = values.iter().sum::<f32>() / values.len() as f32;

        // Detect dangerous periods
        let mut current_danger_start = None;
        let mut current_max_hi: f32 = 0.0;

        for (ts, hi) in &heat_indices {
            if *hi >= 27.0 {
                if current_danger_start.is_none() {
                    current_danger_start = Some(*ts);
                }
                current_max_hi = current_max_hi.max(*hi);
            } else if let Some(start_ts) = current_danger_start {
                dangerous_periods.push(DangerousPeriod {
                    start_timestamp: start_ts,
                    end_timestamp: *ts,
                    max_heat_index: current_max_hi,
                    risk_level: Self::get_risk_level(current_max_hi),
                });
                current_danger_start = None;
                current_max_hi = 0.0;
            }
        }

        HeatIndexAnalysis {
            min,
            max,
            average,
            data_points: heat_indices,
            dangerous_periods,
        }
    }

    fn calculate_heat_index(temp_c: f32, humidity: f32) -> f32 {
        let temp_f = temp_c * 9.0 / 5.0 + 32.0;

        if temp_f < 80.0 {
            return temp_c;
        }

        // Rothfusz regression
        let rh = humidity;
        let t = temp_f;

        let hi = -42.379
            + 2.049_015_3 * t
            + 10.143_332 * rh
            - 0.224_755_4 * t * rh
            - 0.00683783 * t * t
            - 0.05481717 * rh * rh
            + 0.00122874 * t * t * rh
            + 0.00085282 * t * rh * rh
            - 0.00000199 * t * t * rh * rh;

        (hi - 32.0) * 5.0 / 9.0
    }

    fn get_risk_level(heat_index: f32) -> RiskLevel {
        if heat_index >= 54.0 {
            RiskLevel::ExtremeDanger
        } else if heat_index >= 41.0 {
            RiskLevel::Danger
        } else if heat_index >= 32.0 {
            RiskLevel::ExtremeHeat
        } else {
            RiskLevel::Caution
        }
    }

    fn calculate_dew_point_analysis(readings: &[&&SensorReading]) -> DewPointAnalysis {
        let mut dew_points = Vec::new();

        for reading in readings {
            if let (Some(temp), Some(humidity)) =
                (reading.parsed_temperature, reading.parsed_humidity)
            {
                let dew_point = Self::calculate_dew_point(temp, humidity);
                dew_points.push((reading.ts, dew_point));
            }
        }

        if dew_points.is_empty() {
            return DewPointAnalysis {
                min: 0.0,
                max: 0.0,
                average: 0.0,
                data_points: vec![],
            };
        }

        let values: Vec<f32> = dew_points.iter().map(|(_, dp)| *dp).collect();
        let min = values.iter().copied().fold(f32::INFINITY, f32::min);
        let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let average = values.iter().sum::<f32>() / values.len() as f32;

        DewPointAnalysis {
            min,
            max,
            average,
            data_points: dew_points,
        }
    }

    fn calculate_dew_point(temp_c: f32, humidity: f32) -> f32 {
        let a = 17.27;
        let b = 237.7;

        let alpha = ((a * temp_c) / (b + temp_c)) + (humidity / 100.0).ln();
        (b * alpha) / (a - alpha)
    }

    fn analyze_comfort_zones(readings: &[&&SensorReading]) -> ComfortZoneAnalysis {
        let mut comfort_counts = [0; 8];
        let mut comfort_periods = Vec::new();
        let mut current_comfort: Option<ComfortType> = None;
        let mut period_start = None;

        for reading in readings {
            if let (Some(temp), Some(humidity)) =
                (reading.parsed_temperature, reading.parsed_humidity)
            {
                let comfort_type = Self::classify_comfort(temp, humidity);
                let comfort_index = Self::comfort_type_to_index(&comfort_type);
                comfort_counts[comfort_index] += 1;

                if current_comfort.as_ref() != Some(&comfort_type) {
                    if let (Some(prev_comfort), Some(start_ts)) = (&current_comfort, period_start) {
                        comfort_periods.push(ComfortPeriod {
                            start_timestamp: start_ts,
                            end_timestamp: reading.ts,
                            comfort_type: prev_comfort.clone(),
                        });
                    }
                    current_comfort = Some(comfort_type);
                    period_start = Some(reading.ts);
                }
            }
        }

        let total_readings = comfort_counts.iter().sum::<usize>() as f64;
        if total_readings == 0.0 {
            return ComfortZoneAnalysis {
                comfort_percentage: 0.0,
                uncomfortable_hot_percentage: 0.0,
                uncomfortable_cold_percentage: 0.0,
                uncomfortable_humid_percentage: 0.0,
                uncomfortable_dry_percentage: 0.0,
                comfort_periods: vec![],
            };
        }

        let comfort_percentage =
            (comfort_counts[0] + comfort_counts[1]) as f64 / total_readings * 100.0;
        let uncomfortable_hot_percentage = comfort_counts[4] as f64 / total_readings * 100.0;
        let uncomfortable_cold_percentage = comfort_counts[5] as f64 / total_readings * 100.0;
        let uncomfortable_humid_percentage = comfort_counts[6] as f64 / total_readings * 100.0;
        let uncomfortable_dry_percentage = comfort_counts[7] as f64 / total_readings * 100.0;

        ComfortZoneAnalysis {
            comfort_percentage,
            uncomfortable_hot_percentage,
            uncomfortable_cold_percentage,
            uncomfortable_humid_percentage,
            uncomfortable_dry_percentage,
            comfort_periods,
        }
    }

    fn classify_comfort(temp: f32, humidity: f32) -> ComfortType {
        match (temp, humidity) {
            (t, h) if (20.0..=24.0).contains(&t) && (40.0..=60.0).contains(&h) => ComfortType::Optimal,
            (t, h) if (18.0..=26.0).contains(&t) && (30.0..=70.0).contains(&h) => {
                ComfortType::Acceptable
            }
            (t, _) if t > 26.0 => ComfortType::TooHot,
            (t, _) if t < 18.0 => ComfortType::TooCold,
            (_, h) if h > 70.0 => ComfortType::TooHumid,
            (_, h) if h < 30.0 => ComfortType::TooDry,
            _ => ComfortType::Acceptable,
        }
    }

    fn comfort_type_to_index(comfort_type: &ComfortType) -> usize {
        match comfort_type {
            ComfortType::Optimal => 0,
            ComfortType::Acceptable => 1,
            ComfortType::SlightlyWarm => 2,
            ComfortType::SlightlyCool => 3,
            ComfortType::TooHot => 4,
            ComfortType::TooCold => 5,
            ComfortType::TooHumid => 6,
            ComfortType::TooDry => 7,
        }
    }

    fn calculate_vapor_pressure_analysis(readings: &[&&SensorReading]) -> VaporPressureAnalysis {
        let mut vapor_pressures = Vec::new();

        for reading in readings {
            if let (Some(temp), Some(humidity)) =
                (reading.parsed_temperature, reading.parsed_humidity)
            {
                let vapor_pressure = Self::calculate_vapor_pressure(temp, humidity);
                vapor_pressures.push((reading.ts, vapor_pressure));
            }
        }

        if vapor_pressures.is_empty() {
            return VaporPressureAnalysis {
                min: 0.0,
                max: 0.0,
                average: 0.0,
                data_points: vec![],
            };
        }

        let values: Vec<f32> = vapor_pressures.iter().map(|(_, vp)| *vp).collect();
        let min = values.iter().copied().fold(f32::INFINITY, f32::min);
        let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let average = values.iter().sum::<f32>() / values.len() as f32;

        VaporPressureAnalysis {
            min,
            max,
            average,
            data_points: vapor_pressures,
        }
    }

    fn calculate_vapor_pressure(temp_c: f32, humidity: f32) -> f32 {
        let es = 0.6108 * (17.27 * temp_c / (temp_c + 237.3)).exp();
        es * humidity / 100.0
    }

    fn calculate_absolute_humidity_analysis(
        readings: &[&&SensorReading],
    ) -> AbsoluteHumidityAnalysis {
        let mut absolute_humidities = Vec::new();

        for reading in readings {
            if let (Some(temp), Some(humidity)) =
                (reading.parsed_temperature, reading.parsed_humidity)
            {
                let abs_humidity = Self::calculate_absolute_humidity(temp, humidity);
                absolute_humidities.push((reading.ts, abs_humidity));
            }
        }

        if absolute_humidities.is_empty() {
            return AbsoluteHumidityAnalysis {
                min: 0.0,
                max: 0.0,
                average: 0.0,
                data_points: vec![],
            };
        }

        let values: Vec<f32> = absolute_humidities.iter().map(|(_, ah)| *ah).collect();
        let min = values.iter().copied().fold(f32::INFINITY, f32::min);
        let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let average = values.iter().sum::<f32>() / values.len() as f32;

        AbsoluteHumidityAnalysis {
            min,
            max,
            average,
            data_points: absolute_humidities,
        }
    }

    fn calculate_absolute_humidity(temp_c: f32, humidity: f32) -> f32 {
        let es = 6.112 * (17.67 * temp_c / (temp_c + 243.5)).exp();
        let e = es * humidity / 100.0;
        216.7 * e / (273.15 + temp_c)
    }
}
