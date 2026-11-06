//! PIR (motion) sensor analysis

use crate::sensors::SensorReading;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PIRAnalysis {
    pub total_events: usize,
    pub events_per_hour: f64,
    pub peak_activity_hours: Vec<u8>,
    pub activity_by_hour: [usize; 24],
    pub longest_inactive_period_minutes: f64,
}

pub struct PIRAnalyzer;

impl PIRAnalyzer {
    pub fn analyze(readings: &[&SensorReading]) -> Option<PIRAnalysis> {
        let pir_readings: Vec<_> = readings
            .iter()
            .filter(|r| r.parsed_activity.is_some())
            .collect();

        if pir_readings.is_empty() {
            return None;
        }

        let total_events = pir_readings
            .iter()
            .filter(|r| r.parsed_activity == Some(true))
            .count();

        // Calculate time span in hours
        let timestamps: Vec<u64> = pir_readings.iter().map(|r| r.ts).collect();
        let duration_hours = if timestamps.len() > 1 {
            let min_ts = *timestamps.iter().min().unwrap();
            let max_ts = *timestamps.iter().max().unwrap();
            (max_ts - min_ts) as f64 / (1000.0 * 60.0 * 60.0)
        } else {
            1.0
        };

        let events_per_hour = total_events as f64 / duration_hours.max(1.0);

        // Activity by hour of day
        let mut activity_by_hour = [0usize; 24];
        for reading in &pir_readings {
            if reading.parsed_activity == Some(true) {
                let hour = ((reading.ts / 1000 / 60 / 60) % 24) as usize;
                activity_by_hour[hour] += 1;
            }
        }

        // Find peak hours (top 3)
        let mut hours_with_activity: Vec<(u8, usize)> = activity_by_hour
            .iter()
            .enumerate()
            .map(|(h, &count)| (h as u8, count))
            .collect();
        hours_with_activity.sort_by_key(|h| std::cmp::Reverse(h.1));
        let peak_activity_hours: Vec<u8> = hours_with_activity
            .iter()
            .take(3)
            .filter(|(_, count)| *count > 0)
            .map(|(h, _)| *h)
            .collect();

        // Find longest inactive period
        let mut sorted_readings = pir_readings.clone();
        sorted_readings.sort_by_key(|r| r.ts);

        let mut longest_inactive: f64 = 0.0;
        let mut last_active_ts: Option<u64> = None;

        for reading in &sorted_readings {
            if reading.parsed_activity == Some(true) {
                if let Some(last_ts) = last_active_ts {
                    let gap = (reading.ts - last_ts) as f64 / (1000.0 * 60.0);
                    longest_inactive = longest_inactive.max(gap);
                }
                last_active_ts = Some(reading.ts);
            }
        }

        Some(PIRAnalysis {
            total_events,
            events_per_hour,
            peak_activity_hours,
            activity_by_hour,
            longest_inactive_period_minutes: longest_inactive,
        })
    }
}
