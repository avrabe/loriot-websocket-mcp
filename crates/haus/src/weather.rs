//! Current weather for the house view's idle scene (REQ-UI-012, DD-021).
//!
//! With HAUS_WEATHER_AT ("<lat>,<lon>") set, the server asks Open-Meteo every
//! 30 minutes for the current temperature, weather code and wind at that
//! place, rounded to 0.1 degree (about 10 km), and serves the last answer at
//! GET /api/weather. The browsers of the household only ever talk to haus.
//! Unset, there is no weather: /api/weather answers 404 and the scene shows
//! the season and the sky only.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

/// A place, rounded to 0.1 degree.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Place {
    pub lat: f64,
    pub lon: f64,
}

/// What Open-Meteo says now; the field names are Open-Meteo's, so the
/// scene reads them as warte's does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Current {
    pub temperature_2m: f64,
    pub weather_code: i64,
    pub wind_speed_10m: f64,
    /// Where the wind comes from, degrees clockwise from north; absent
    /// where the source does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_direction_10m: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
struct Answer {
    current: Current,
    fetched_ms: i64,
}

static PLACE: OnceLock<Place> = OnceLock::new();
static LAST: RwLock<Option<Answer>> = RwLock::new(None);

/// An answer older than this is not served: no weather beats old weather.
const MAX_AGE_MS: i64 = 3 * 3_600_000;
const EVERY: Duration = Duration::from_secs(30 * 60);

/// "<lat>,<lon>" in degrees, rounded to 0.1 degree; errors name the setting.
pub fn parse_place(s: &str) -> Result<Place, String> {
    let err = || format!("HAUS_WEATHER_AT: {s:?} must be \"<lat>,<lon>\" in degrees");
    let (a, b) = s.split_once(',').ok_or_else(err)?;
    let lat: f64 = a.trim().parse().map_err(|_| err())?;
    let lon: f64 = b.trim().parse().map_err(|_| err())?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(err());
    }
    let r = |x: f64| (x * 10.0).round() / 10.0;
    Ok(Place {
        lat: r(lat),
        lon: r(lon),
    })
}

/// Read HAUS_WEATHER_AT once at startup.
pub fn load(v: Option<&str>) -> Result<(), String> {
    if let Some(v) = v.filter(|v| !v.trim().is_empty()) {
        let _ = PLACE.set(parse_place(v)?);
    }
    Ok(())
}

pub fn place() -> Option<Place> {
    PLACE.get().copied()
}

/// The request for a place: only the rounded coordinates leave the house.
pub fn url(p: Place) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.1}&longitude={:.1}&current=temperature_2m,weather_code,wind_speed_10m,wind_direction_10m",
        p.lat, p.lon
    )
}

/// The `current` object of an Open-Meteo answer.
pub fn parse_answer(body: &str) -> Result<Current, String> {
    #[derive(Deserialize)]
    struct Body {
        current: Current,
    }
    serde_json::from_str::<Body>(body)
        .map(|b| b.current)
        .map_err(|e| format!("Open-Meteo answer: {e}"))
}

fn store(c: Current, now_ms: i64) {
    *LAST.write().unwrap_or_else(|e| e.into_inner()) = Some(Answer {
        current: c,
        fetched_ms: now_ms,
    });
}

fn fresh(now_ms: i64) -> Option<Answer> {
    LAST.read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .filter(|a| now_ms - a.fetched_ms <= MAX_AGE_MS)
}

/// Ask Open-Meteo now and every 30 minutes; a failure keeps the last answer
/// (served while it is under 3 h old) and is logged.
pub async fn run(p: Place) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("haus/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("weather: no HTTP client: {e}");
            return;
        }
    };
    let mut tick = tokio::time::interval(EVERY);
    loop {
        tick.tick().await;
        let got = async {
            let r = client.get(url(p)).send().await.map_err(|e| e.to_string())?;
            if !r.status().is_success() {
                return Err(format!("HTTP {}", r.status()));
            }
            parse_answer(&r.text().await.map_err(|e| e.to_string())?)
        }
        .await;
        match got {
            Ok(c) => store(c, crate::dashboard::now_ms()),
            Err(e) => tracing::warn!("weather: Open-Meteo: {e}"),
        }
    }
}

/// GET /api/weather: the last answer, or 404 without a place or a fresh one.
pub async fn weather_api() -> Response {
    match fresh(crate::dashboard::now_ms()) {
        Some(a) if place().is_some() => Json(a).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // rivet: verifies REQ-UI-012
    #[test]
    fn the_place_is_rounded_and_checked() {
        assert_eq!(
            parse_place("12.34567, 45.67891").unwrap(),
            Place {
                lat: 12.3,
                lon: 45.7
            }
        );
        assert_eq!(
            parse_place("-33.87,151.21").unwrap(),
            Place {
                lat: -33.9,
                lon: 151.2
            }
        );
        for bad in ["", "12.3", "x,45", "91,45", "12,181", "12;45"] {
            let e = parse_place(bad).unwrap_err();
            assert!(e.starts_with("HAUS_WEATHER_AT"), "{bad}: {e}");
        }
        // Only the rounded place leaves the house.
        let u = url(parse_place("12.34567,45.67891").unwrap());
        assert!(u.contains("latitude=12.3&longitude=45.7&"), "{u}");
        assert!(!u.contains("12.34") && !u.contains("45.67"), "{u}");
    }

    // rivet: verifies REQ-UI-012
    #[test]
    fn the_answer_is_read_and_old_weather_is_not_served() {
        let body = r#"{"latitude":12.3,"longitude":45.7,"current_units":{},
            "current":{"time":"2026-11-14T12:00","interval":900,"temperature_2m":6.4,
            "weather_code":61,"wind_speed_10m":12.0,"wind_direction_10m":250}}"#;
        let c = parse_answer(body).unwrap();
        assert_eq!(
            c,
            Current {
                temperature_2m: 6.4,
                weather_code: 61,
                wind_speed_10m: 12.0,
                wind_direction_10m: Some(250.0)
            }
        );
        assert!(parse_answer(r#"{"error":true,"reason":"x"}"#).is_err());
        store(c.clone(), 1_000);
        assert_eq!(fresh(1_000 + MAX_AGE_MS).map(|a| a.current), Some(c));
        assert!(fresh(1_001 + MAX_AGE_MS).is_none());
    }
}
