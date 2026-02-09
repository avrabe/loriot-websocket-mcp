//! Battery health analysis

use crate::sensors::SensorReading;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryHealth {
    pub current_voltage: f32,
    pub current_status: String,
    pub voltage_trend: VoltageTrend,
    pub estimated_days_remaining: Option<u32>,
    pub voltage_history: Vec<(u64, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoltageTrend {
    Stable,
    SlowDecline,
    RapidDecline,
    Charging,
    Unknown,
}

pub struct BatteryAnalyzer;

impl BatteryAnalyzer {
    pub fn analyze(readings: &[&SensorReading]) -> Option<BatteryHealth> {
        let battery_readings: Vec<_> = readings
            .iter()
            .filter(|r| r.parsed_battery_voltage.is_some())
            .collect();

        if battery_readings.is_empty() {
            return None;
        }

        // Get latest reading
        let latest = battery_readings
            .iter()
            .max_by_key(|r| r.ts)?;

        let current_voltage = latest.parsed_battery_voltage?;
        let current_status = latest
            .parsed_battery_status
            .clone()
            .unwrap_or_else(|| Self::voltage_to_status(current_voltage));

        // Build voltage history
        let mut voltage_history: Vec<(u64, f32)> = battery_readings
            .iter()
            .filter_map(|r| r.parsed_battery_voltage.map(|v| (r.ts, v)))
            .collect();
        voltage_history.sort_by_key(|(ts, _)| *ts);

        // Calculate trend
        let voltage_trend = Self::calculate_trend(&voltage_history);

        // Estimate remaining days (rough calculation)
        let estimated_days_remaining = Self::estimate_days_remaining(&voltage_history, current_voltage);

        Some(BatteryHealth {
            current_voltage,
            current_status,
            voltage_trend,
            estimated_days_remaining,
            voltage_history,
        })
    }

    fn voltage_to_status(voltage: f32) -> String {
        if voltage > 3.0 {
            "Good".to_string()
        } else if voltage > 2.8 {
            "Low".to_string()
        } else {
            "Critical".to_string()
        }
    }

    fn calculate_trend(history: &[(u64, f32)]) -> VoltageTrend {
        if history.len() < 10 {
            return VoltageTrend::Unknown;
        }

        // Compare first and last 10% of readings
        let sample_size = (history.len() / 10).max(1);
        let early_avg: f32 = history[..sample_size].iter().map(|(_, v)| v).sum::<f32>() / sample_size as f32;
        let late_avg: f32 = history[history.len() - sample_size..].iter().map(|(_, v)| v).sum::<f32>() / sample_size as f32;

        let diff = late_avg - early_avg;

        if diff > 0.05 {
            VoltageTrend::Charging
        } else if diff > -0.02 {
            VoltageTrend::Stable
        } else if diff > -0.1 {
            VoltageTrend::SlowDecline
        } else {
            VoltageTrend::RapidDecline
        }
    }

    fn estimate_days_remaining(history: &[(u64, f32)], current: f32) -> Option<u32> {
        if history.len() < 2 || current <= 2.5 {
            return None;
        }

        // Simple linear projection
        let first = history.first()?;
        let last = history.last()?;

        let voltage_drop = first.1 - last.1;
        let time_span_days = (last.0 - first.0) as f64 / (1000.0 * 60.0 * 60.0 * 24.0);

        if voltage_drop <= 0.0 || time_span_days < 1.0 {
            return None;
        }

        let daily_drop = voltage_drop as f64 / time_span_days;
        let voltage_until_critical = (current - 2.5) as f64;

        Some((voltage_until_critical / daily_drop).max(0.0) as u32)
    }
}
