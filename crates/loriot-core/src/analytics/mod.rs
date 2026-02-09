//! Advanced analytics and statistical tools for LoRaWAN sensor data
//!
//! This module provides comprehensive analysis capabilities including:
//! - Environmental metrics (heat index, dew point, comfort zones)
//! - Statistical analysis (trends, correlations, anomaly detection)
//! - PIR motion pattern analysis
//! - Battery health monitoring
//! - Data quality assessment

pub mod anomaly;
pub mod battery;
pub mod environmental;
pub mod pir_analysis;
pub mod statistics;
pub mod trends;

use crate::sensors::SensorReading;
use serde::{Deserialize, Serialize};

/// Helper function to format timestamp for display
pub fn format_timestamp(ts: u64) -> String {
    let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ts as i64);
    if let Some(dt) = datetime {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        "Invalid timestamp".to_string()
    }
}

/// Helper function to format duration for display
pub fn format_duration(minutes: f64) -> String {
    if minutes < 60.0 {
        format!("{:.0} minutes", minutes)
    } else if minutes < 1440.0 {
        let hours = minutes / 60.0;
        format!("{:.1} hours", hours)
    } else {
        let days = minutes / 1440.0;
        format!("{:.1} days", days)
    }
}

/// Comprehensive analytics result for a dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsReport {
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub analysis_period: AnalysisPeriod,
    pub environmental: environmental::EnvironmentalMetrics,
    pub statistics: statistics::StatisticalSummary,
    pub pir_analysis: Option<pir_analysis::PIRAnalysis>,
    pub battery_health: Option<battery::BatteryHealth>,
    pub trends: trends::TrendAnalysis,
    pub anomalies: anomaly::AnomalyReport,
    pub data_quality: DataQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisPeriod {
    pub start_timestamp: u64,
    pub end_timestamp: u64,
    pub duration_hours: f64,
    pub total_readings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataQuality {
    pub completeness_score: f64,
    pub consistency_score: f64,
    pub accuracy_score: f64,
    pub missing_readings_count: usize,
    pub outlier_count: usize,
    pub data_gaps: Vec<DataGap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataGap {
    pub start_timestamp: u64,
    pub end_timestamp: u64,
    pub duration_minutes: f64,
    pub expected_readings: usize,
    pub actual_readings: usize,
    pub severity: DataGapSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataGapSeverity {
    Minor,      // < 1 hour
    Moderate,   // 1-6 hours  
    Major,      // 6-24 hours
    Critical,   // > 24 hours
}

impl DataGap {
    pub fn classify_severity(duration_minutes: f64) -> DataGapSeverity {
        if duration_minutes < 60.0 {
            DataGapSeverity::Minor
        } else if duration_minutes < 360.0 {
            DataGapSeverity::Moderate
        } else if duration_minutes < 1440.0 {
            DataGapSeverity::Major
        } else {
            DataGapSeverity::Critical
        }
    }
}

/// Main analytics engine
pub struct AnalyticsEngine;

impl AnalyticsEngine {
    /// Generate comprehensive analytics report for sensor readings
    pub fn analyze_readings(
        readings: &[&SensorReading],
        device_id: Option<&str>,
    ) -> AnalyticsReport {
        if readings.is_empty() {
            return Self::empty_report(device_id);
        }

        let device_name = readings
            .first()
            .and_then(|r| r.name.as_ref())
            .cloned();

        let analysis_period = Self::calculate_analysis_period(readings);
        let environmental = environmental::EnvironmentalAnalyzer::analyze(readings);
        let statistics = statistics::StatisticalAnalyzer::analyze(readings);
        let pir_analysis = pir_analysis::PIRAnalyzer::analyze(readings);
        let battery_health = battery::BatteryAnalyzer::analyze(readings);
        let trends = trends::TrendAnalyzer::analyze(readings);
        let anomalies = anomaly::AnomalyDetector::detect(readings);
        let data_quality = Self::assess_data_quality(readings);

        AnalyticsReport {
            device_id: device_id.map(String::from),
            device_name,
            analysis_period,
            environmental,
            statistics,
            pir_analysis,
            battery_health,
            trends,
            anomalies,
            data_quality,
        }
    }

    fn empty_report(device_id: Option<&str>) -> AnalyticsReport {
        AnalyticsReport {
            device_id: device_id.map(String::from),
            device_name: None,
            analysis_period: AnalysisPeriod {
                start_timestamp: 0,
                end_timestamp: 0,
                duration_hours: 0.0,
                total_readings: 0,
            },
            environmental: environmental::EnvironmentalMetrics::default(),
            statistics: statistics::StatisticalSummary::default(),
            pir_analysis: None,
            battery_health: None,
            trends: trends::TrendAnalysis::default(),
            anomalies: anomaly::AnomalyReport::default(),
            data_quality: DataQuality {
                completeness_score: 0.0,
                consistency_score: 0.0,
                accuracy_score: 0.0,
                missing_readings_count: 0,
                outlier_count: 0,
                data_gaps: vec![],
            },
        }
    }

    fn calculate_analysis_period(readings: &[&SensorReading]) -> AnalysisPeriod {
        if readings.is_empty() {
            return AnalysisPeriod {
                start_timestamp: 0,
                end_timestamp: 0,
                duration_hours: 0.0,
                total_readings: 0,
            };
        }

        let timestamps: Vec<u64> = readings.iter().map(|r| r.ts).collect();
        let start_timestamp = *timestamps.iter().min().unwrap();
        let end_timestamp = *timestamps.iter().max().unwrap();
        let duration_hours = (end_timestamp - start_timestamp) as f64 / (1000.0 * 60.0 * 60.0);

        AnalysisPeriod {
            start_timestamp,
            end_timestamp,
            duration_hours,
            total_readings: readings.len(),
        }
    }

    fn assess_data_quality(readings: &[&SensorReading]) -> DataQuality {
        if readings.is_empty() {
            return DataQuality {
                completeness_score: 0.0,
                consistency_score: 0.0,
                accuracy_score: 0.0,
                missing_readings_count: 0,
                outlier_count: 0,
                data_gaps: vec![],
            };
        }

        // Calculate expected readings based on 20-minute intervals
        let timestamps: Vec<u64> = readings.iter().map(|r| r.ts).collect();
        let start_ts = *timestamps.iter().min().unwrap();
        let end_ts = *timestamps.iter().max().unwrap();
        let duration_minutes = (end_ts - start_ts) as f64 / (1000.0 * 60.0);
        let expected_readings = (duration_minutes / 20.0).ceil() as usize;

        let completeness_score = readings.len() as f64 / expected_readings.max(1) as f64;
        let missing_readings_count = expected_readings.saturating_sub(readings.len());

        // Detect data gaps
        let data_gaps = Self::detect_data_gaps(readings);

        // Simple outlier detection for temperature
        let outlier_count = Self::count_outliers(readings);

        // Consistency score based on regular intervals
        let consistency_score = Self::calculate_consistency_score(readings);

        // Accuracy score based on sensor value ranges
        let accuracy_score = Self::calculate_accuracy_score(readings);

        DataQuality {
            completeness_score: completeness_score.min(1.0),
            consistency_score,
            accuracy_score,
            missing_readings_count,
            outlier_count,
            data_gaps,
        }
    }

    fn detect_data_gaps(readings: &[&SensorReading]) -> Vec<DataGap> {
        let mut gaps = Vec::new();
        let mut sorted_readings = readings.to_vec();
        sorted_readings.sort_by_key(|r| r.ts);

        for window in sorted_readings.windows(2) {
            let gap_duration = (window[1].ts - window[0].ts) as f64 / (1000.0 * 60.0);
            if gap_duration > 25.0 {
                // More than 25 minutes gap (expected is 20 min)
                let expected_readings = (gap_duration / 20.0).floor() as usize;
                let severity = DataGap::classify_severity(gap_duration);
                gaps.push(DataGap {
                    start_timestamp: window[0].ts,
                    end_timestamp: window[1].ts,
                    duration_minutes: gap_duration,
                    expected_readings,
                    actual_readings: 0,
                    severity,
                });
            }
        }

        gaps
    }

    fn count_outliers(readings: &[&SensorReading]) -> usize {
        let temps: Vec<f32> = readings
            .iter()
            .filter_map(|r| r.parsed_temperature)
            .collect();

        if temps.len() < 3 {
            return 0;
        }

        let mean = temps.iter().sum::<f32>() / temps.len() as f32;
        let variance = temps.iter().map(|t| (t - mean).powi(2)).sum::<f32>() / temps.len() as f32;
        let std_dev = variance.sqrt();

        temps
            .iter()
            .filter(|&&temp| (temp - mean).abs() > 3.0 * std_dev)
            .count()
    }

    fn calculate_consistency_score(readings: &[&SensorReading]) -> f64 {
        if readings.len() < 2 {
            return 1.0;
        }

        let mut sorted_readings = readings.to_vec();
        sorted_readings.sort_by_key(|r| r.ts);

        let intervals: Vec<f64> = sorted_readings
            .windows(2)
            .map(|w| (w[1].ts - w[0].ts) as f64 / (1000.0 * 60.0))
            .collect();

        let mean_interval = intervals.iter().sum::<f64>() / intervals.len() as f64;
        let variance = intervals
            .iter()
            .map(|i| (i - mean_interval).powi(2))
            .sum::<f64>()
            / intervals.len() as f64;
        let coefficient_of_variation = variance.sqrt() / mean_interval;

        // Lower coefficient of variation = higher consistency
        (1.0 / (1.0 + coefficient_of_variation)).min(1.0)
    }

    fn calculate_accuracy_score(readings: &[&SensorReading]) -> f64 {
        let mut score = 1.0;
        let mut total_checks = 0;

        for reading in readings {
            // Temperature range check (-40°C to 85°C)
            if let Some(temp) = reading.parsed_temperature {
                total_checks += 1;
                if !(-40.0..=85.0).contains(&temp) {
                    score -= 0.1;
                }
            }

            // Humidity range check (0% to 100%)
            if let Some(humidity) = reading.parsed_humidity {
                total_checks += 1;
                if !(0.0..=100.0).contains(&humidity) {
                    score -= 0.1;
                }
            }

            // Battery voltage check (reasonable range)
            if let Some(voltage) = reading.parsed_battery_voltage {
                total_checks += 1;
                if !(2.0..=4.0).contains(&voltage) {
                    score -= 0.1;
                }
            }
        }

        (score / total_checks.max(1) as f64).clamp(0.0, 1.0)
    }
}
