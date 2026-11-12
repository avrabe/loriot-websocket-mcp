//! Geocoding service for converting addresses to coordinates
//!
//! Uses OpenStreetMap Nominatim API (free, no API key required)
//! Rate limited to 1 request per second as per OSM usage policy

use super::Coordinates;
#[cfg(any(feature = "async", feature = "blocking"))]
use crate::{AppError, Result};
use serde::{Deserialize, Serialize};

/// Result of a geocoding lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeocodingResult {
    /// Resolved coordinates
    pub coordinates: Coordinates,
    /// Full display name of the location
    pub display_name: String,
    /// Normalized address components
    pub address: AddressComponents,
    /// Bounding box [south, north, west, east]
    pub bounding_box: Option<[f64; 4]>,
    /// Importance score (0-1)
    pub importance: f64,
}

/// Parsed address components
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AddressComponents {
    pub house_number: Option<String>,
    pub road: Option<String>,
    pub suburb: Option<String>,
    pub city: Option<String>,
    pub county: Option<String>,
    pub state: Option<String>,
    pub postcode: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
}

/// Nominatim API response structure
#[cfg(any(feature = "async", feature = "blocking"))]
#[derive(Debug, Deserialize)]
struct NominatimResponse {
    lat: String,
    lon: String,
    display_name: String,
    address: Option<NominatimAddress>,
    boundingbox: Option<Vec<String>>,
    importance: Option<f64>,
}

#[cfg(any(feature = "async", feature = "blocking"))]
#[derive(Debug, Deserialize)]
struct NominatimAddress {
    house_number: Option<String>,
    road: Option<String>,
    suburb: Option<String>,
    city: Option<String>,
    town: Option<String>,
    village: Option<String>,
    county: Option<String>,
    state: Option<String>,
    postcode: Option<String>,
    country: Option<String>,
    country_code: Option<String>,
}

/// Geocoding service using OpenStreetMap Nominatim
pub struct GeocodingService {
    /// Base URL for Nominatim API
    base_url: String,
    /// User agent for API requests (required by Nominatim)
    #[cfg_attr(not(any(feature = "async", feature = "blocking")), allow(dead_code))]
    user_agent: String,
}

impl GeocodingService {
    /// Create a new geocoding service
    pub fn new() -> Self {
        Self {
            base_url: "https://nominatim.openstreetmap.org".to_string(),
            user_agent: "loriot-sensor-monitor/1.0".to_string(),
        }
    }

    /// Create with custom user agent
    pub fn with_user_agent(user_agent: &str) -> Self {
        Self {
            base_url: "https://nominatim.openstreetmap.org".to_string(),
            user_agent: user_agent.to_string(),
        }
    }

    /// Geocode an address to coordinates (blocking)
    #[cfg(feature = "blocking")]
    pub fn geocode_blocking(&self, address: &str) -> Result<Vec<GeocodingResult>> {
        use std::time::Duration;

        let url = format!(
            "{}/search?q={}&format=json&addressdetails=1&limit=5",
            self.base_url,
            urlencoding::encode(address)
        );

        let client = reqwest::blocking::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;

        let response: Vec<NominatimResponse> = client
            .get(&url)
            .send()
            .map_err(|e| AppError::Network(e.to_string()))?
            .json()
            .map_err(|e| AppError::Network(e.to_string()))?;

        Ok(response
            .into_iter()
            .filter_map(Self::parse_response)
            .collect())
    }

    /// Geocode an address to coordinates (async)
    #[cfg(feature = "async")]
    pub async fn geocode(&self, address: &str) -> Result<Vec<GeocodingResult>> {
        use std::time::Duration;

        let url = format!(
            "{}/search?q={}&format=json&addressdetails=1&limit=5",
            self.base_url,
            urlencoding::encode(address)
        );

        let client = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;

        let response: Vec<NominatimResponse> = client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;

        Ok(response
            .into_iter()
            .filter_map(Self::parse_response)
            .collect())
    }

    /// Reverse geocode coordinates to address (blocking)
    #[cfg(feature = "blocking")]
    pub fn reverse_geocode_blocking(
        &self,
        coords: &Coordinates,
    ) -> Result<Option<GeocodingResult>> {
        use std::time::Duration;

        let url = format!(
            "{}/reverse?lat={}&lon={}&format=json&addressdetails=1",
            self.base_url, coords.latitude, coords.longitude
        );

        let client = reqwest::blocking::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;

        let response: NominatimResponse = client
            .get(&url)
            .send()
            .map_err(|e| AppError::Network(e.to_string()))?
            .json()
            .map_err(|e| AppError::Network(e.to_string()))?;

        Ok(Self::parse_response(response))
    }

    /// Reverse geocode coordinates to address (async)
    #[cfg(feature = "async")]
    pub async fn reverse_geocode(&self, coords: &Coordinates) -> Result<Option<GeocodingResult>> {
        use std::time::Duration;

        let url = format!(
            "{}/reverse?lat={}&lon={}&format=json&addressdetails=1",
            self.base_url, coords.latitude, coords.longitude
        );

        let client = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;

        let response: NominatimResponse = client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;

        Ok(Self::parse_response(response))
    }

    #[cfg(any(feature = "async", feature = "blocking"))]
    fn parse_response(response: NominatimResponse) -> Option<GeocodingResult> {
        let latitude: f64 = response.lat.parse().ok()?;
        let longitude: f64 = response.lon.parse().ok()?;

        let address = response
            .address
            .map(|a| AddressComponents {
                house_number: a.house_number,
                road: a.road,
                suburb: a.suburb,
                city: a.city.or(a.town).or(a.village),
                county: a.county,
                state: a.state,
                postcode: a.postcode,
                country: a.country,
                country_code: a.country_code,
            })
            .unwrap_or_default();

        let bounding_box = response.boundingbox.and_then(|bb| {
            if bb.len() == 4 {
                Some([
                    bb[0].parse().ok()?,
                    bb[1].parse().ok()?,
                    bb[2].parse().ok()?,
                    bb[3].parse().ok()?,
                ])
            } else {
                None
            }
        });

        Some(GeocodingResult {
            coordinates: Coordinates::new(latitude, longitude),
            display_name: response.display_name,
            address,
            bounding_box,
            importance: response.importance.unwrap_or(0.0),
        })
    }

    /// Build the geocoding URL for an address (useful for manual testing)
    pub fn build_search_url(&self, address: &str) -> String {
        format!(
            "{}/search?q={}&format=json&addressdetails=1&limit=5",
            self.base_url,
            urlencoding::encode(address)
        )
    }
}

impl Default for GeocodingService {
    fn default() -> Self {
        Self::new()
    }
}

/// URL encode a string
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::new();
        for c in s.chars() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                    result.push(c);
                }
                ' ' => result.push('+'),
                _ => {
                    for byte in c.to_string().as_bytes() {
                        result.push_str(&format!("%{:02X}", byte));
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encoding() {
        assert_eq!(urlencoding::encode("Berlin"), "Berlin");
        // Note: urlencoding encodes comma as %2C and space as +
        assert_eq!(
            urlencoding::encode("München, Germany"),
            "M%C3%BCnchen%2C+Germany"
        );
        assert_eq!(urlencoding::encode("123 Main St"), "123+Main+St");
    }

    #[test]
    fn test_build_search_url() {
        let service = GeocodingService::new();
        let url = service.build_search_url("Berlin, Germany");
        assert!(url.contains("nominatim.openstreetmap.org"));
        // URL should contain encoded query
        assert!(url.contains("q=Berlin"));
    }
}
