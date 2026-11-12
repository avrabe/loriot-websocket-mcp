//! Weather forecast integration using Open-Meteo API
//!
//! Fetches weather forecasts and combines with historical thermal data
//! to predict indoor room temperatures.

use super::{Coordinates, DailyWeather};
#[cfg(any(feature = "async", feature = "blocking"))]
use crate::{AppError, Result};
use serde::{Deserialize, Serialize};

/// Open-Meteo API base URL
const OPEN_METEO_URL: &str = "https://api.open-meteo.com/v1/forecast";

/// Hourly forecast data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyForecast {
    /// Timestamp (ISO 8601)
    pub time: String,
    /// Temperature at 2m (°C)
    pub temperature: f32,
    /// Relative humidity (%)
    pub humidity: f32,
    /// Precipitation (mm)
    pub precipitation: f32,
    /// Cloud cover (%)
    pub cloud_cover: Option<f32>,
    /// Wind speed at 10m (km/h)
    pub wind_speed: Option<f32>,
    /// Apparent temperature (°C)
    pub apparent_temperature: Option<f32>,
}

/// Daily forecast summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyForecast {
    /// Date (YYYYMMDD format)
    pub date: String,
    /// Minimum temperature (°C)
    pub temp_min: f32,
    /// Maximum temperature (°C)
    pub temp_max: f32,
    /// Mean temperature (°C) - calculated from hourly
    pub temp_mean: f32,
    /// Total precipitation (mm)
    pub precipitation: f32,
    /// Mean humidity (%)
    pub humidity: f32,
    /// Sunrise time (ISO 8601)
    pub sunrise: Option<String>,
    /// Sunset time (ISO 8601)
    pub sunset: Option<String>,
    /// Hourly forecasts for this day
    pub hourly: Vec<HourlyForecast>,
}

impl DailyForecast {
    /// Convert to DailyWeather format for compatibility
    pub fn to_daily_weather(&self, station_id: &str) -> DailyWeather {
        DailyWeather {
            date: self.date.clone(),
            station_id: station_id.to_string(),
            station_name: Some("Open-Meteo Forecast".to_string()),
            temp_mean: Some(self.temp_mean),
            temp_max: Some(self.temp_max),
            temp_min: Some(self.temp_min),
            temp_ground_min: None,
            humidity: Some(self.humidity),
            precipitation: Some(self.precipitation),
            sunshine_hours: None,
            snow_depth: None,
            wind_speed: None,
            wind_gust: None,
            vapor_pressure: None,
            air_pressure: None,
            cloud_cover: None,
            quality_level: None,
        }
    }
}

/// Weather forecast for a location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherForecast {
    /// Location coordinates
    pub coordinates: Coordinates,
    /// Timezone
    pub timezone: String,
    /// Generation time (ms)
    pub generation_time_ms: f64,
    /// Daily forecasts (up to 16 days)
    pub daily: Vec<DailyForecast>,
    /// Hourly forecasts (all hours)
    pub hourly: Vec<HourlyForecast>,
}

/// Room temperature prediction based on forecast and thermal characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomForecast {
    /// Device/room ID
    pub device_id: String,
    /// Device name
    pub device_name: Option<String>,
    /// Daily predictions
    pub daily_predictions: Vec<DailyRoomPrediction>,
    /// Model parameters used
    pub model: ThermalModel,
}

/// Daily room temperature prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyRoomPrediction {
    /// Date (YYYYMMDD format)
    pub date: String,
    /// Predicted mean indoor temperature (°C)
    pub predicted_temp: f32,
    /// Predicted temperature range (min, max)
    pub predicted_range: (f32, f32),
    /// Outdoor forecast temperature
    pub outdoor_temp: f32,
    /// Outdoor temperature range
    pub outdoor_range: (f32, f32),
    /// Confidence level (0-100)
    pub confidence: f32,
}

/// Thermal model parameters for a room
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalModel {
    /// Damping factor (indoor swing / outdoor swing)
    pub damping_factor: f32,
    /// Correlation with outdoor temperature
    pub outdoor_correlation: f32,
    /// Thermal lag in hours (how long indoor follows outdoor)
    pub thermal_lag_hours: f32,
    /// Base temperature offset (indoor - outdoor average)
    pub base_offset: f32,
    /// Number of historical days used for model
    pub training_days: usize,
}

impl ThermalModel {
    /// Predict indoor temperature from outdoor temperature
    pub fn predict(&self, outdoor_temp: f32, outdoor_range: (f32, f32)) -> (f32, (f32, f32)) {
        // Predicted mean = outdoor temp + base offset (for unheated spaces this is small)
        let predicted_mean = outdoor_temp + self.base_offset;

        // Indoor range is dampened version of outdoor range
        let outdoor_swing = outdoor_range.1 - outdoor_range.0;
        let indoor_swing = outdoor_swing * self.damping_factor;

        let predicted_min = predicted_mean - (indoor_swing / 2.0);
        let predicted_max = predicted_mean + (indoor_swing / 2.0);

        (predicted_mean, (predicted_min, predicted_max))
    }
}

/// Open-Meteo API response structure
#[cfg(any(feature = "async", feature = "blocking"))]
#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    latitude: f64,
    longitude: f64,
    generationtime_ms: f64,
    timezone: String,
    hourly: Option<OpenMeteoHourly>,
    daily: Option<OpenMeteoDaily>,
}

#[cfg(any(feature = "async", feature = "blocking"))]
#[derive(Debug, Deserialize)]
struct OpenMeteoHourly {
    time: Vec<String>,
    temperature_2m: Vec<Option<f32>>,
    relative_humidity_2m: Vec<Option<f32>>,
    precipitation: Vec<Option<f32>>,
    cloud_cover: Option<Vec<Option<f32>>>,
    wind_speed_10m: Option<Vec<Option<f32>>>,
    apparent_temperature: Option<Vec<Option<f32>>>,
}

#[cfg(any(feature = "async", feature = "blocking"))]
#[derive(Debug, Deserialize)]
struct OpenMeteoDaily {
    time: Vec<String>,
    temperature_2m_max: Vec<Option<f32>>,
    temperature_2m_min: Vec<Option<f32>>,
    precipitation_sum: Vec<Option<f32>>,
    sunrise: Option<Vec<String>>,
    sunset: Option<Vec<String>>,
}

/// Forecast fetcher for Open-Meteo API
pub struct ForecastFetcher {
    /// Number of forecast days (1-16)
    pub forecast_days: u8,
}

impl Default for ForecastFetcher {
    fn default() -> Self {
        Self { forecast_days: 7 }
    }
}

impl ForecastFetcher {
    pub fn new(forecast_days: u8) -> Self {
        Self {
            forecast_days: forecast_days.clamp(1, 16),
        }
    }

    /// Build the API URL for forecast request
    pub fn build_url(&self, coords: &Coordinates) -> String {
        format!(
            "{}?latitude={:.4}&longitude={:.4}&hourly=temperature_2m,relative_humidity_2m,precipitation,cloud_cover,wind_speed_10m,apparent_temperature&daily=temperature_2m_max,temperature_2m_min,precipitation_sum,sunrise,sunset&timezone=Europe%2FBerlin&forecast_days={}",
            OPEN_METEO_URL,
            coords.latitude,
            coords.longitude,
            self.forecast_days
        )
    }

    /// Parse Open-Meteo response into our forecast format
    #[cfg(any(feature = "async", feature = "blocking"))]
    fn parse_response(&self, response: OpenMeteoResponse) -> Result<WeatherForecast> {
        let coords = Coordinates::new(response.latitude, response.longitude);

        // Parse hourly data
        let hourly = if let Some(h) = response.hourly {
            h.time
                .iter()
                .enumerate()
                .filter_map(|(i, time)| {
                    let temp = h.temperature_2m.get(i)?.as_ref()?;
                    let humidity = h.relative_humidity_2m.get(i)?.as_ref()?;
                    let precip = h.precipitation.get(i).and_then(|v| *v).unwrap_or(0.0);

                    Some(HourlyForecast {
                        time: time.clone(),
                        temperature: *temp,
                        humidity: *humidity,
                        precipitation: precip,
                        cloud_cover: h
                            .cloud_cover
                            .as_ref()
                            .and_then(|v| v.get(i))
                            .and_then(|v| *v),
                        wind_speed: h
                            .wind_speed_10m
                            .as_ref()
                            .and_then(|v| v.get(i))
                            .and_then(|v| *v),
                        apparent_temperature: h
                            .apparent_temperature
                            .as_ref()
                            .and_then(|v| v.get(i))
                            .and_then(|v| *v),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        // Parse daily data and attach hourly forecasts
        let daily = if let Some(d) = response.daily {
            d.time
                .iter()
                .enumerate()
                .filter_map(|(i, date_str)| {
                    let temp_max = d.temperature_2m_max.get(i)?.as_ref()?;
                    let temp_min = d.temperature_2m_min.get(i)?.as_ref()?;
                    let precip = d.precipitation_sum.get(i).and_then(|v| *v).unwrap_or(0.0);

                    // Convert date from YYYY-MM-DD to YYYYMMDD
                    let date = date_str.replace('-', "");

                    // Get hourly forecasts for this day
                    let day_hourly: Vec<_> = hourly
                        .iter()
                        .filter(|h| h.time.starts_with(date_str))
                        .cloned()
                        .collect();

                    // Calculate mean and humidity from hourly
                    let (temp_mean, humidity) = if !day_hourly.is_empty() {
                        let mean_temp = day_hourly.iter().map(|h| h.temperature).sum::<f32>()
                            / day_hourly.len() as f32;
                        let mean_humidity = day_hourly.iter().map(|h| h.humidity).sum::<f32>()
                            / day_hourly.len() as f32;
                        (mean_temp, mean_humidity)
                    } else {
                        ((*temp_max + *temp_min) / 2.0, 70.0)
                    };

                    Some(DailyForecast {
                        date,
                        temp_min: *temp_min,
                        temp_max: *temp_max,
                        temp_mean,
                        precipitation: precip,
                        humidity,
                        sunrise: d.sunrise.as_ref().and_then(|v| v.get(i)).cloned(),
                        sunset: d.sunset.as_ref().and_then(|v| v.get(i)).cloned(),
                        hourly: day_hourly,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(WeatherForecast {
            coordinates: coords,
            timezone: response.timezone,
            generation_time_ms: response.generationtime_ms,
            daily,
            hourly,
        })
    }

    /// Fetch forecast (async version)
    #[cfg(feature = "async")]
    pub async fn fetch(&self, coords: &Coordinates) -> Result<WeatherForecast> {
        use std::time::Duration;

        let url = self.build_url(coords);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;

        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Failed to fetch forecast: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::Network(format!(
                "Open-Meteo returned status: {}",
                response.status()
            )));
        }

        let data: OpenMeteoResponse = response
            .json()
            .await
            .map_err(|e| AppError::Network(format!("Failed to parse forecast: {}", e)))?;

        self.parse_response(data)
    }

    /// Fetch forecast (blocking version)
    #[cfg(feature = "blocking")]
    pub fn fetch_blocking(&self, coords: &Coordinates) -> Result<WeatherForecast> {
        use std::time::Duration;

        let url = self.build_url(coords);

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;

        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .map_err(|e| AppError::Network(format!("Failed to fetch forecast: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::Network(format!(
                "Open-Meteo returned status: {}",
                response.status()
            )));
        }

        let data: OpenMeteoResponse = response
            .json()
            .map_err(|e| AppError::Network(format!("Failed to parse forecast: {}", e)))?;

        self.parse_response(data)
    }
}

/// Room temperature predictor
///
/// Uses thermal analysis data to predict indoor temperatures
/// based on weather forecasts.
pub struct RoomPredictor;

impl RoomPredictor {
    /// Create room forecast from weather forecast and thermal model
    pub fn predict(
        forecast: &WeatherForecast,
        device_id: &str,
        device_name: Option<String>,
        model: &ThermalModel,
    ) -> RoomForecast {
        let daily_predictions: Vec<DailyRoomPrediction> = forecast
            .daily
            .iter()
            .map(|day| {
                let outdoor_range = (day.temp_min, day.temp_max);
                let (predicted_temp, predicted_range) = model.predict(day.temp_mean, outdoor_range);

                // Confidence decreases for forecasts further in the future
                // and for rooms with lower correlation
                let base_confidence = model.outdoor_correlation.abs() * 100.0;

                DailyRoomPrediction {
                    date: day.date.clone(),
                    predicted_temp,
                    predicted_range,
                    outdoor_temp: day.temp_mean,
                    outdoor_range,
                    confidence: base_confidence.clamp(30.0, 95.0),
                }
            })
            .collect();

        RoomForecast {
            device_id: device_id.to_string(),
            device_name,
            daily_predictions,
            model: model.clone(),
        }
    }

    /// Build thermal model from device analysis data
    pub fn build_model(
        damping_factor: f32,
        outdoor_correlation: f32,
        avg_temp_difference: f32,
        training_days: usize,
    ) -> ThermalModel {
        ThermalModel {
            damping_factor,
            outdoor_correlation,
            thermal_lag_hours: 2.0, // Default lag for basement spaces
            base_offset: avg_temp_difference,
            training_days,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thermal_model_predict() {
        let model = ThermalModel {
            damping_factor: 0.1,
            outdoor_correlation: 0.9,
            thermal_lag_hours: 2.0,
            base_offset: 5.0, // Indoor is 5°C warmer than outdoor on average
            training_days: 80,
        };

        // Outdoor temp 10°C, range 5-15°C
        let (predicted, range) = model.predict(10.0, (5.0, 15.0));

        // Should be ~15°C (10 + 5 offset)
        assert!((predicted - 15.0).abs() < 0.1);

        // Indoor swing should be 10°C * 0.1 = 1°C
        let indoor_swing = range.1 - range.0;
        assert!((indoor_swing - 1.0).abs() < 0.1);
    }

    #[test]
    fn test_build_url() {
        let fetcher = ForecastFetcher::new(7);
        let coords = Coordinates::new(50.11, 8.68);
        let url = fetcher.build_url(&coords);

        assert!(url.contains("latitude=50.11"));
        assert!(url.contains("longitude=8.68"));
        assert!(url.contains("forecast_days=7"));
    }

    #[test]
    fn test_daily_forecast_to_daily_weather() {
        let forecast = DailyForecast {
            date: "20251228".to_string(),
            temp_min: 2.0,
            temp_max: 8.0,
            temp_mean: 5.0,
            precipitation: 1.5,
            humidity: 85.0,
            sunrise: None,
            sunset: None,
            hourly: vec![],
        };

        let weather = forecast.to_daily_weather("open-meteo");
        assert_eq!(weather.date, "20251228");
        assert_eq!(weather.temp_mean, Some(5.0));
    }
}
