//! Statistical analysis for sensor readings

use crate::sensors::SensorReading;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatisticalSummary {
    pub temperature: Option<ValueStatistics>,
    pub humidity: Option<ValueStatistics>,
    pub battery_voltage: Option<ValueStatistics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueStatistics {
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    pub median: f32,
    pub std_dev: f32,
    pub count: usize,
}

pub struct StatisticalAnalyzer;

impl StatisticalAnalyzer {
    pub fn analyze(readings: &[&SensorReading]) -> StatisticalSummary {
        let temperatures: Vec<f32> = readings
            .iter()
            .filter_map(|r| r.parsed_temperature)
            .collect();

        let humidities: Vec<f32> = readings.iter().filter_map(|r| r.parsed_humidity).collect();

        let voltages: Vec<f32> = readings
            .iter()
            .filter_map(|r| r.parsed_battery_voltage)
            .collect();

        StatisticalSummary {
            temperature: Self::compute_stats(&temperatures),
            humidity: Self::compute_stats(&humidities),
            battery_voltage: Self::compute_stats(&voltages),
        }
    }

    fn compute_stats(values: &[f32]) -> Option<ValueStatistics> {
        if values.is_empty() {
            return None;
        }

        let count = values.len();
        let min = values.iter().copied().fold(f32::INFINITY, f32::min);
        let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mean = values.iter().sum::<f32>() / count as f32;

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = if count.is_multiple_of(2) {
            (sorted[count / 2 - 1] + sorted[count / 2]) / 2.0
        } else {
            sorted[count / 2]
        };

        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / count as f32;
        let std_dev = variance.sqrt();

        Some(ValueStatistics {
            min,
            max,
            mean,
            median,
            std_dev,
            count,
        })
    }
}
