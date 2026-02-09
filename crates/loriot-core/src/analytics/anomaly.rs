//! Anomaly detection for sensor readings

use crate::sensors::SensorReading;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnomalyReport {
    pub anomalies: Vec<Anomaly>,
    pub total_anomalies: usize,
    pub anomaly_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub timestamp: u64,
    pub device_eui: String,
    pub anomaly_type: AnomalyType,
    pub severity: Severity,
    pub description: String,
    pub value: Option<f32>,
    pub expected_range: Option<(f32, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalyType {
    TemperatureSpike,
    TemperatureDrop,
    HumiditySpike,
    HumidityDrop,
    BatteryAnomaly,
    MissingData,
    SensorMalfunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

pub struct AnomalyDetector;

impl AnomalyDetector {
    pub fn detect(readings: &[&SensorReading]) -> AnomalyReport {
        if readings.len() < 10 {
            return AnomalyReport::default();
        }

        let mut anomalies = Vec::new();

        // Detect temperature anomalies
        anomalies.extend(Self::detect_temperature_anomalies(readings));

        // Detect humidity anomalies
        anomalies.extend(Self::detect_humidity_anomalies(readings));

        // Detect battery anomalies
        anomalies.extend(Self::detect_battery_anomalies(readings));

        let total_anomalies = anomalies.len();
        let anomaly_rate = total_anomalies as f64 / readings.len() as f64;

        AnomalyReport {
            anomalies,
            total_anomalies,
            anomaly_rate,
        }
    }

    fn detect_temperature_anomalies(readings: &[&SensorReading]) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        let temps: Vec<f32> = readings
            .iter()
            .filter_map(|r| r.parsed_temperature)
            .collect();

        if temps.len() < 3 {
            return anomalies;
        }

        let mean = temps.iter().sum::<f32>() / temps.len() as f32;
        let variance = temps.iter().map(|t| (t - mean).powi(2)).sum::<f32>() / temps.len() as f32;
        let std_dev = variance.sqrt();

        let lower_bound = mean - 3.0 * std_dev;
        let upper_bound = mean + 3.0 * std_dev;

        for reading in readings {
            if let Some(temp) = reading.parsed_temperature {
                if temp > upper_bound {
                    anomalies.push(Anomaly {
                        timestamp: reading.ts,
                        device_eui: reading.eui.clone(),
                        anomaly_type: AnomalyType::TemperatureSpike,
                        severity: if temp > upper_bound + std_dev {
                            Severity::High
                        } else {
                            Severity::Medium
                        },
                        description: format!(
                            "Temperature {:.1}°C exceeds normal range",
                            temp
                        ),
                        value: Some(temp),
                        expected_range: Some((lower_bound, upper_bound)),
                    });
                } else if temp < lower_bound {
                    anomalies.push(Anomaly {
                        timestamp: reading.ts,
                        device_eui: reading.eui.clone(),
                        anomaly_type: AnomalyType::TemperatureDrop,
                        severity: if temp < lower_bound - std_dev {
                            Severity::High
                        } else {
                            Severity::Medium
                        },
                        description: format!(
                            "Temperature {:.1}°C below normal range",
                            temp
                        ),
                        value: Some(temp),
                        expected_range: Some((lower_bound, upper_bound)),
                    });
                }
            }
        }

        anomalies
    }

    fn detect_humidity_anomalies(readings: &[&SensorReading]) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        for reading in readings {
            if let Some(humidity) = reading.parsed_humidity {
                if humidity > 95.0 {
                    anomalies.push(Anomaly {
                        timestamp: reading.ts,
                        device_eui: reading.eui.clone(),
                        anomaly_type: AnomalyType::HumiditySpike,
                        severity: Severity::Medium,
                        description: format!("Very high humidity {:.1}%", humidity),
                        value: Some(humidity),
                        expected_range: Some((20.0, 90.0)),
                    });
                } else if humidity < 10.0 {
                    anomalies.push(Anomaly {
                        timestamp: reading.ts,
                        device_eui: reading.eui.clone(),
                        anomaly_type: AnomalyType::HumidityDrop,
                        severity: Severity::Medium,
                        description: format!("Very low humidity {:.1}%", humidity),
                        value: Some(humidity),
                        expected_range: Some((20.0, 90.0)),
                    });
                }
            }
        }

        anomalies
    }

    fn detect_battery_anomalies(readings: &[&SensorReading]) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        for reading in readings {
            if let Some(voltage) = reading.parsed_battery_voltage {
                if voltage < 2.5 {
                    anomalies.push(Anomaly {
                        timestamp: reading.ts,
                        device_eui: reading.eui.clone(),
                        anomaly_type: AnomalyType::BatteryAnomaly,
                        severity: Severity::Critical,
                        description: format!("Critical battery voltage {:.2}V", voltage),
                        value: Some(voltage),
                        expected_range: Some((2.8, 3.6)),
                    });
                } else if voltage < 2.8 {
                    anomalies.push(Anomaly {
                        timestamp: reading.ts,
                        device_eui: reading.eui.clone(),
                        anomaly_type: AnomalyType::BatteryAnomaly,
                        severity: Severity::High,
                        description: format!("Low battery voltage {:.2}V", voltage),
                        value: Some(voltage),
                        expected_range: Some((2.8, 3.6)),
                    });
                }
            }
        }

        anomalies
    }
}
