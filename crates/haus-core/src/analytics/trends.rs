//! Trend analysis for sensor data

use crate::sensors::SensorReading;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrendAnalysis {
    pub temperature_trend: Option<Trend>,
    pub humidity_trend: Option<Trend>,
    pub daily_patterns: Option<DailyPatterns>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trend {
    pub direction: TrendDirection,
    pub slope: f64,
    pub r_squared: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrendDirection {
    Increasing,
    Decreasing,
    Stable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPatterns {
    pub avg_temp_by_hour: [f32; 24],
    pub avg_humidity_by_hour: [f32; 24],
    pub reading_count_by_hour: [usize; 24],
}

pub struct TrendAnalyzer;

impl TrendAnalyzer {
    pub fn analyze(readings: &[&SensorReading]) -> TrendAnalysis {
        if readings.len() < 10 {
            return TrendAnalysis::default();
        }

        let temperature_trend = Self::analyze_value_trend(readings, |r| r.parsed_temperature);
        let humidity_trend = Self::analyze_value_trend(readings, |r| r.parsed_humidity);
        let daily_patterns = Self::analyze_daily_patterns(readings);

        TrendAnalysis {
            temperature_trend,
            humidity_trend,
            daily_patterns,
        }
    }

    fn analyze_value_trend<F>(readings: &[&SensorReading], extractor: F) -> Option<Trend>
    where
        F: Fn(&SensorReading) -> Option<f32>,
    {
        let data: Vec<(f64, f64)> = readings
            .iter()
            .filter_map(|r| extractor(r).map(|v| (r.ts as f64, v as f64)))
            .collect();

        if data.len() < 10 {
            return None;
        }

        // Simple linear regression
        let n = data.len() as f64;
        let sum_x: f64 = data.iter().map(|(x, _)| x).sum();
        let sum_y: f64 = data.iter().map(|(_, y)| y).sum();
        let sum_xy: f64 = data.iter().map(|(x, y)| x * y).sum();
        let sum_x2: f64 = data.iter().map(|(x, _)| x * x).sum();

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x * sum_x);

        // Calculate R-squared
        let mean_y = sum_y / n;
        let ss_tot: f64 = data.iter().map(|(_, y)| (y - mean_y).powi(2)).sum();
        let intercept = (sum_y - slope * sum_x) / n;
        let ss_res: f64 = data
            .iter()
            .map(|(x, y)| (y - (slope * x + intercept)).powi(2))
            .sum();
        let r_squared = if ss_tot > 0.0 {
            1.0 - ss_res / ss_tot
        } else {
            0.0
        };

        // Normalize slope to per-hour change
        let slope_per_hour = slope * (1000.0 * 60.0 * 60.0);

        let direction = if slope_per_hour.abs() < 0.01 {
            TrendDirection::Stable
        } else if slope_per_hour > 0.0 {
            TrendDirection::Increasing
        } else {
            TrendDirection::Decreasing
        };

        Some(Trend {
            direction,
            slope: slope_per_hour,
            r_squared,
        })
    }

    fn analyze_daily_patterns(readings: &[&SensorReading]) -> Option<DailyPatterns> {
        let mut temp_sum = [0.0f64; 24];
        let mut humidity_sum = [0.0f64; 24];
        let mut temp_count = [0usize; 24];
        let mut humidity_count = [0usize; 24];

        for reading in readings {
            let hour = ((reading.ts / 1000 / 60 / 60) % 24) as usize;

            if let Some(temp) = reading.parsed_temperature {
                temp_sum[hour] += temp as f64;
                temp_count[hour] += 1;
            }

            if let Some(humidity) = reading.parsed_humidity {
                humidity_sum[hour] += humidity as f64;
                humidity_count[hour] += 1;
            }
        }

        let avg_temp_by_hour: [f32; 24] = std::array::from_fn(|i| {
            if temp_count[i] > 0 {
                (temp_sum[i] / temp_count[i] as f64) as f32
            } else {
                0.0
            }
        });

        let avg_humidity_by_hour: [f32; 24] = std::array::from_fn(|i| {
            if humidity_count[i] > 0 {
                (humidity_sum[i] / humidity_count[i] as f64) as f32
            } else {
                0.0
            }
        });

        let reading_count_by_hour: [usize; 24] =
            std::array::from_fn(|i| temp_count[i].max(humidity_count[i]));

        Some(DailyPatterns {
            avg_temp_by_hour,
            avg_humidity_by_hour,
            reading_count_by_hour,
        })
    }
}
