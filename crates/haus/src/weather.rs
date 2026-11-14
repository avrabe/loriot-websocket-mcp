//! Current weather for the house view (REQ-UI-012, DD-021).
//!
//! First choice is the Miniserver's Loxone Weather Service, which arrives
//! over the websocket haus already has: nothing leaves the house. Open-Meteo
//! is the fallback for a deployment without one.
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
    /// The source's own words for the weather (the Miniserver's type text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Answer {
    current: Current,
    fetched_ms: i64,
    /// "loxone" or "open-meteo".
    source: &'static str,
}

static PLACE: OnceLock<Place> = OnceLock::new();
static LAST: RwLock<Option<Answer>> = RwLock::new(None);
static LOXONE: RwLock<Option<Answer>> = RwLock::new(None);

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
        source: "open-meteo",
    });
}

/// The Miniserver's current weather (its weatherServer "actual" state).
pub fn store_loxone(e: &haus_core::loxone::WeatherEntry, text: Option<&str>, now_ms: i64) {
    let c = Current {
        temperature_2m: e.temperature,
        weather_code: wmo_from(text.unwrap_or(""), e.precipitation, e.temperature),
        wind_speed_10m: e.wind_speed,
        wind_direction_10m: Some(e.wind_direction as f64),
        text: text.map(str::to_string),
    };
    *LOXONE.write().unwrap_or_else(|e| e.into_inner()) = Some(Answer {
        current: c,
        fetched_ms: now_ms,
        source: "loxone",
    });
}

/// A WMO weather code for a weather text (German or English, as the
/// Miniserver names its types), so the page draws rain, snow, fog or a
/// storm the same way for either source. Without a known word the
/// precipitation decides.
pub fn wmo_from(text: &str, precipitation: f64, temperature: f64) -> i64 {
    let t = text.to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| t.contains(w));
    let light = has(&["leicht", "light"]);
    let heavy = has(&["stark", "heavy", "kräftig"]);
    if has(&["gewitter", "thunder"]) {
        95
    } else if has(&["schneeregen", "sleet", "gefrier", "freezing"]) {
        66
    } else if has(&["schnee", "snow"]) {
        if light {
            71
        } else if heavy {
            75
        } else {
            73
        }
    } else if has(&["schauer", "shower"]) {
        if heavy {
            82
        } else {
            80
        }
    } else if has(&["niesel", "drizzle"]) {
        53
    } else if has(&["regen", "rain"]) {
        if light {
            61
        } else if heavy {
            65
        } else {
            63
        }
    } else if has(&["nebel", "fog", "dunst", "mist"]) {
        45
    } else if has(&["bedeckt", "overcast", "stark bewölkt"]) {
        3
    } else if has(&["bewölkt", "wolkig", "cloud"]) {
        2
    } else if has(&["wolkenlos", "klar", "clear", "sonnig", "sunny"]) {
        0
    } else if has(&["heiter", "fair"]) {
        1
    } else if precipitation > 0.0 {
        if temperature <= 0.0 {
            71
        } else {
            61
        }
    } else {
        2
    }
}

/// The weather to show: the Miniserver's while fresh, else Open-Meteo's.
fn fresh(now_ms: i64) -> Option<Answer> {
    let ok = |a: &Option<Answer>| a.clone().filter(|a| now_ms - a.fetched_ms <= MAX_AGE_MS);
    ok(&LOXONE.read().unwrap_or_else(|e| e.into_inner()))
        .or_else(|| ok(&LAST.read().unwrap_or_else(|e| e.into_inner())))
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

/// The Miniserver's weather as stored (tests).
#[cfg(test)]
pub fn current_for_tests() -> Option<Current> {
    LOXONE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .map(|a| a.current)
}

/// GET /api/weather: the freshest answer, or 404 when there is none under
/// 3 h old.
pub async fn weather_api() -> Response {
    match fresh(crate::dashboard::now_ms()) {
        Some(a) => Json(a).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
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

    // The Miniserver's weather words become codes the page draws.
    // rivet: verifies REQ-UI-012
    #[test]
    fn weather_words_become_codes() {
        for (t, code) in [
            ("wolkenlos", 0),
            ("heiter", 1),
            ("wolkig", 2),
            ("bedeckt", 3),
            ("Nebel", 45),
            ("Nieselregen", 53),
            ("leichter Regen", 61),
            ("Regen", 63),
            ("starker Regen", 65),
            ("Regenschauer", 80),
            ("Schneeregen", 66),
            ("leichter Schneefall", 71),
            ("Schneefall", 73),
            ("Gewitter", 95),
            ("light rain", 61),
        ] {
            assert_eq!(wmo_from(t, 0.0, 5.0), code, "{t}");
        }
        // No known word: the precipitation and temperature decide.
        assert_eq!(wmo_from("", 1.2, -2.0), 71);
        assert_eq!(wmo_from("", 1.2, 4.0), 61);
        assert_eq!(wmo_from("", 0.0, 4.0), 2);
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
                wind_direction_10m: Some(250.0),
                text: None
            }
        );
        assert!(parse_answer(r#"{"error":true,"reason":"x"}"#).is_err());
        store(c.clone(), 1_000);
        assert_eq!(fresh(1_000 + MAX_AGE_MS).map(|a| a.current), Some(c));
        assert!(fresh(1_001 + MAX_AGE_MS).is_none());
    }
}
