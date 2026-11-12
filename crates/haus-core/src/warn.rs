//! Warnings: values that need attention (REQ-WARN-001..005, DD-014).
//!
//! A pure evaluation over each series' metadata, its recent points and the
//! sources' connection state at a given time. No stored state, no
//! notifications; the same input always gives the same warnings.

use crate::store::{Quantity, SeriesMeta, Source};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Thresholds; every field has a default, every room can override.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Thresholds {
    /// Mould risk: humidity at or above this, % …
    pub mould_rh: f64,
    /// … for this many hours without a break.
    pub mould_hours: f64,
    /// Too cold below this air temperature, °C.
    pub cold_c: f64,
    /// Too warm above this air temperature, °C.
    pub warm_c: f64,
    /// A LoRaWAN sensor without a value for this many minutes is silent.
    pub silent_minutes: f64,
    /// Battery low below this voltage, V …
    pub battery_v: f64,
    /// … or below this level when the battery reports %.
    pub battery_pct: f64,
    /// Heating anomaly: valve at or above this opening, % …
    pub valve_open_pct: f64,
    /// … for this many hours …
    pub valve_hours: f64,
    /// … while the room warmed by less than this, K.
    pub valve_min_rise_k: f64,
    /// A Loxone temperature or humidity unchanged for this many hours is
    /// stuck (a sensor that stopped sending, e.g. an empty battery: the
    /// Miniserver keeps repeating its last value).
    pub stuck_hours: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            mould_rh: 65.0,
            mould_hours: 4.0,
            cold_c: 16.0,
            warm_c: 26.0,
            silent_minutes: 60.0,
            battery_v: 2.6,
            battery_pct: 20.0,
            valve_open_pct: 50.0,
            valve_hours: 2.0,
            valve_min_rise_k: 0.3,
            stuck_hours: 72.0,
        }
    }
}

/// Per-room settings on top of the defaults.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RoomSettings {
    /// Outdoor: no climate warnings (outdoor humidity is not mould risk).
    pub outdoor: bool,
    /// Frost-only: only warn below this temperature (cellar, garage); no
    /// warm or comfort warnings.
    pub frost_c: Option<f64>,
    pub mould_rh: Option<f64>,
    pub cold_c: Option<f64>,
    pub warm_c: Option<f64>,
}

/// The HAUS_WARNINGS file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WarnConfig {
    pub defaults: Thresholds,
    pub rooms: HashMap<String, RoomSettings>,
    /// Series whose name contains one of these are not room air (floor
    /// probes, radiators, valve actuators).
    pub not_air: Vec<String>,
}

impl Default for WarnConfig {
    fn default() -> Self {
        Self {
            defaults: Thresholds::default(),
            rooms: HashMap::new(),
            not_air: [
                "Fußboden",
                "Heizkörper",
                "Stellantrieb",
                "Radiator",
                "Floor",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    MouldRisk,
    TooCold,
    TooWarm,
    Frost,
    SensorSilent,
    SensorStuck,
    SourceDown,
    BatteryLow,
    HeatingAnomaly,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Warning {
    pub kind: Kind,
    /// The room, or the source for SourceDown.
    pub room: String,
    /// The series it is about (empty for SourceDown and room-wide ones).
    pub series: Vec<u32>,
    pub message: String,
    /// Since when the condition holds, as far as the data shows (ms).
    pub since_ms: Option<i64>,
}

/// One series and its recent points (oldest first; the window may start with
/// the point standing before it).
#[derive(Clone, Debug)]
pub struct SeriesData {
    pub id: u32,
    pub meta: SeriesMeta,
    pub latest: Option<(i64, f64)>,
    pub recent: Vec<(i64, f64)>,
}

/// How far back the evaluator needs points of `meta`'s series, in ms. The
/// caller passes the points in the window plus the one standing at its start.
pub fn window_ms(cfg: &WarnConfig, meta: &SeriesMeta) -> i64 {
    let mut h = cfg.defaults.mould_hours.max(cfg.defaults.valve_hours);
    if can_stick(meta) {
        h = h.max(cfg.defaults.stuck_hours);
    }
    ((h + 1.0) * 3_600_000.0) as i64
}

/// Series the stuck rule watches: Loxone temperatures and humidities (a
/// LoRaWAN sensor that stops sending goes silent instead).
fn can_stick(m: &SeriesMeta) -> bool {
    m.key.source == Source::Loxone
        && matches!(m.quantity, Quantity::Temperature | Quantity::Humidity)
}

/// When `s` has held its latest value for at least `hours`: since when, as
/// far as its points show (the start of the unchanged run).
fn stuck_since(s: &SeriesData, now_ms: i64, hours: f64) -> Option<i64> {
    if !can_stick(&s.meta) {
        return None;
    }
    let (_, v) = s.latest?;
    let same = |x: f64| (x - v).abs() < 0.05;
    let from = now_ms - (hours * 3_600_000.0) as i64;
    if !held_since(&s.recent, from, same) {
        return None;
    }
    let mut since = None;
    for (t, x) in s.recent.iter().rev() {
        if !same(*x) {
            break;
        }
        since = Some(*t);
    }
    since
}

fn room_of(m: &SeriesMeta) -> &str {
    m.room.as_deref().unwrap_or(&m.name)
}

fn is_air(cfg: &WarnConfig, m: &SeriesMeta) -> bool {
    m.quantity == Quantity::Temperature && !cfg.not_air.iter().any(|p| m.name.contains(p.as_str()))
}

/// True when every value of `points` from `from` to `now` satisfies `ok`,
/// counting the value standing at `from`; false without such a value.
fn held_since(points: &[(i64, f64)], from: i64, ok: impl Fn(f64) -> bool) -> bool {
    let standing = points.iter().rev().find(|(t, _)| *t <= from);
    standing.is_some_and(|(_, v)| ok(*v))
        && points
            .iter()
            .filter(|(t, _)| *t > from)
            .all(|(_, v)| ok(*v))
}

/// The room's air temperature now and `ago_ms` ago: medians over its air
/// series' standing values.
fn room_air(cfg: &WarnConfig, series: &[&SeriesData], at: i64) -> Option<f64> {
    let mut v: Vec<f64> = series
        .iter()
        .filter(|s| is_air(cfg, &s.meta))
        .filter_map(|s| {
            s.recent
                .iter()
                .rev()
                .find(|(t, _)| *t <= at)
                .map(|(_, v)| *v)
        })
        .filter(|v| v.is_finite())
        .collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

/// Evaluate every rule at `now_ms`. `connected` holds the configured
/// sources and whether each is connected.
pub fn evaluate(
    series: &[SeriesData],
    connected: &[(Source, bool)],
    now_ms: i64,
    cfg: &WarnConfig,
) -> Vec<Warning> {
    let d = &cfg.defaults;
    let mut out = Vec::new();
    for (src, up) in connected {
        if !up {
            out.push(Warning {
                kind: Kind::SourceDown,
                room: src.to_string(),
                series: vec![],
                message: format!("{src} is not connected: no new values arrive"),
                since_ms: None,
            });
        }
    }
    let mut rooms: BTreeMap<&str, Vec<&SeriesData>> = BTreeMap::new();
    for s in series {
        rooms.entry(room_of(&s.meta)).or_default().push(s);
    }
    for (room, all) in &rooms {
        let rs = cfg.rooms.get(*room).cloned().unwrap_or_default();
        // A stuck series gets its own warning and takes no part in the room's
        // climate rules: its value is old, whatever it says.
        let mut ss: Vec<&SeriesData> = Vec::new();
        for s in all {
            match stuck_since(s, now_ms, d.stuck_hours) {
                Some(since) => {
                    let (_, v) = s.latest.expect("stuck has a latest value");
                    out.push(Warning {
                        kind: Kind::SensorStuck,
                        room: room.to_string(),
                        series: vec![s.id],
                        message: format!(
                            "{} unchanged at {v:.1}{}{} for over {} h: the sensor may have stopped (battery?); left out of the room's values",
                            s.meta.name,
                            if s.meta.unit.is_empty() { "" } else { " " },
                            s.meta.unit,
                            d.stuck_hours
                        ),
                        since_ms: Some(since),
                    });
                }
                None => ss.push(s),
            }
        }
        let ss = &ss;
        // Per-series rules.
        for s in ss {
            let Some((ts, v)) = s.latest else { continue };
            match s.meta.quantity {
                Quantity::BatteryVoltage => {
                    let (low, unit) = if s.meta.unit == "%" {
                        (d.battery_pct, "%")
                    } else {
                        (d.battery_v, " V")
                    };
                    if v < low {
                        out.push(Warning {
                            kind: Kind::BatteryLow,
                            room: room.to_string(),
                            series: vec![s.id],
                            message: format!(
                                "battery {} at {v:.2}{unit} (below {low}{unit})",
                                s.meta.name
                            ),
                            since_ms: Some(ts),
                        });
                    }
                }
                _ if s.meta.key.source == Source::Loriot
                    && (now_ms - ts) as f64 > d.silent_minutes * 60_000.0 =>
                {
                    out.push(Warning {
                        kind: Kind::SensorSilent,
                        room: room.to_string(),
                        series: vec![s.id],
                        message: format!(
                            "{} sent nothing for {} min",
                            s.meta.name,
                            (now_ms - ts) / 60_000
                        ),
                        since_ms: Some(ts),
                    });
                }
                _ => {}
            }
        }
        if rs.outdoor {
            continue;
        }
        // Room temperature.
        if let Some(t) = room_air(cfg, ss, now_ms) {
            let ids: Vec<u32> = ss
                .iter()
                .filter(|s| is_air(cfg, &s.meta))
                .map(|s| s.id)
                .collect();
            if let Some(frost) = rs.frost_c {
                if t < frost {
                    out.push(Warning {
                        kind: Kind::Frost,
                        room: room.to_string(),
                        series: ids,
                        message: format!("{t:.1} °C, frost risk (below {frost} °C)"),
                        since_ms: None,
                    });
                }
            } else {
                let cold = rs.cold_c.unwrap_or(d.cold_c);
                let warm = rs.warm_c.unwrap_or(d.warm_c);
                if t < cold {
                    out.push(Warning {
                        kind: Kind::TooCold,
                        room: room.to_string(),
                        series: ids,
                        message: format!("{t:.1} °C, too cold (below {cold} °C)"),
                        since_ms: None,
                    });
                } else if t > warm {
                    out.push(Warning {
                        kind: Kind::TooWarm,
                        room: room.to_string(),
                        series: ids,
                        message: format!("{t:.1} °C, too warm (above {warm} °C)"),
                        since_ms: None,
                    });
                }
            }
        }
        // Mould risk: any humidity series of the room held high all window.
        let rh = rs.mould_rh.unwrap_or(d.mould_rh);
        let from = now_ms - (d.mould_hours * 3_600_000.0) as i64;
        for s in ss.iter().filter(|s| s.meta.quantity == Quantity::Humidity) {
            if held_since(&s.recent, from, |v| v >= rh) {
                out.push(Warning {
                    kind: Kind::MouldRisk,
                    room: room.to_string(),
                    series: vec![s.id],
                    message: format!(
                        "{} at or above {rh} % for {} h: mould risk",
                        s.meta.name, d.mould_hours
                    ),
                    since_ms: Some(from),
                });
            }
        }
        // Heating anomaly: a valve open all window, the room not warming.
        let from = now_ms - (d.valve_hours * 3_600_000.0) as i64;
        let valves: Vec<&&SeriesData> = ss
            .iter()
            .filter(|s| s.meta.quantity == Quantity::ValvePosition)
            .filter(|s| held_since(&s.recent, from, |v| v >= d.valve_open_pct))
            .collect();
        if !valves.is_empty() {
            if let (Some(then), Some(now)) = (room_air(cfg, ss, from), room_air(cfg, ss, now_ms)) {
                if now - then < d.valve_min_rise_k {
                    out.push(Warning {
                        kind: Kind::HeatingAnomaly,
                        room: room.to_string(),
                        series: valves.iter().map(|s| s.id).collect(),
                        message: format!(
                            "valve open for {} h but the room went from {then:.1} to {now:.1} °C",
                            d.valve_hours
                        ),
                        since_ms: Some(from),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| (a.kind, &a.room).cmp(&(b.kind, &b.room)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SeriesKey;

    const NOW: i64 = 1_800_000_000_000;
    const H: i64 = 3_600_000;

    fn s(
        id: u32,
        src: Source,
        q: Quantity,
        name: &str,
        room: &str,
        unit: &str,
        pts: &[(i64, f64)],
    ) -> SeriesData {
        SeriesData {
            id,
            meta: SeriesMeta {
                key: SeriesKey::new(src, format!("s{id}"), "f"),
                quantity: q,
                unit: unit.into(),
                name: name.into(),
                room: Some(room.into()),
            },
            latest: pts.last().copied(),
            recent: pts.to_vec(),
        }
    }

    fn kinds(w: &[Warning]) -> Vec<(Kind, String)> {
        w.iter().map(|w| (w.kind, w.room.clone())).collect()
    }

    // rivet: verifies REQ-WARN-002
    // rivet: verifies DD-014
    #[test]
    fn mould_risk_needs_the_whole_window() {
        let cfg = WarnConfig::default();
        let up = [(Source::Loxone, true)];
        // Held at 70 % from 6 h ago (one event, Loxone-style): warns.
        let held = s(
            1,
            Source::Loxone,
            Quantity::Humidity,
            "RH",
            "Bath",
            "%",
            &[(NOW - 6 * H, 70.0)],
        );
        assert_eq!(
            kinds(&evaluate(&[held], &up, NOW, &cfg)),
            vec![(Kind::MouldRisk, "Bath".into())]
        );
        // Dipped to 60 % two hours ago: no warning.
        let dipped = s(
            1,
            Source::Loxone,
            Quantity::Humidity,
            "RH",
            "Bath",
            "%",
            &[(NOW - 6 * H, 70.0), (NOW - 2 * H, 60.0), (NOW - H, 70.0)],
        );
        assert!(evaluate(&[dipped], &up, NOW, &cfg).is_empty());
        // High only for the last 3 h: no warning yet.
        let recent = s(
            1,
            Source::Loxone,
            Quantity::Humidity,
            "RH",
            "Bath",
            "%",
            &[(NOW - 3 * H, 80.0)],
        );
        assert!(evaluate(&[recent], &up, NOW, &cfg).is_empty());
        // Outdoor humidity is never mould risk.
        let mut c2 = cfg.clone();
        c2.rooms.insert(
            "Terrace".into(),
            RoomSettings {
                outdoor: true,
                ..Default::default()
            },
        );
        let out = s(
            1,
            Source::Loxone,
            Quantity::Humidity,
            "RH",
            "Terrace",
            "%",
            &[(NOW - 6 * H, 95.0)],
        );
        assert!(evaluate(&[out], &up, NOW, &c2).is_empty());
    }

    // rivet: verifies REQ-WARN-003
    #[test]
    fn room_air_temperature_ignores_floor_probes_and_honours_frost_rooms() {
        let cfg = WarnConfig::default();
        let up = [(Source::Loxone, true)];
        // Air 15 °C (median of 14, 15, 16) is too cold; the 30 °C floor probe
        // does not count and does not make it warm.
        let room = vec![
            s(
                1,
                Source::Loxone,
                Quantity::Temperature,
                "Temp A",
                "Office",
                "°C",
                &[(NOW - H, 14.0)],
            ),
            s(
                2,
                Source::Loxone,
                Quantity::Temperature,
                "Temp B",
                "Office",
                "°C",
                &[(NOW - H, 15.0)],
            ),
            s(
                3,
                Source::Loxone,
                Quantity::Temperature,
                "Temp C",
                "Office",
                "°C",
                &[(NOW - H, 16.0)],
            ),
            s(
                4,
                Source::Loxone,
                Quantity::Temperature,
                "Office Fußbodenheizung Temperatur",
                "Office",
                "°C",
                &[(NOW - H, 30.0)],
            ),
        ];
        let w = evaluate(&room, &up, NOW, &cfg);
        assert_eq!(kinds(&w), vec![(Kind::TooCold, "Office".into())]);
        assert!(w[0].message.contains("15.0 °C") && !w[0].series.contains(&4));
        // Too warm.
        let hot = vec![s(
            1,
            Source::Loxone,
            Quantity::Temperature,
            "T",
            "Office",
            "°C",
            &[(NOW - H, 27.0)],
        )];
        assert_eq!(
            kinds(&evaluate(&hot, &up, NOW, &cfg)),
            vec![(Kind::TooWarm, "Office".into())]
        );
        // A cellar at 12 °C is fine as a frost room, 2 °C is not.
        let mut c2 = cfg.clone();
        c2.rooms.insert(
            "Cellar".into(),
            RoomSettings {
                frost_c: Some(3.0),
                ..Default::default()
            },
        );
        let cool = vec![s(
            1,
            Source::Loriot,
            Quantity::Temperature,
            "T",
            "Cellar",
            "°C",
            &[(NOW - 60_000, 12.0)],
        )];
        assert!(evaluate(&cool, &[(Source::Loriot, true)], NOW, &c2).is_empty());
        let frost = vec![s(
            1,
            Source::Loriot,
            Quantity::Temperature,
            "T",
            "Cellar",
            "°C",
            &[(NOW - 60_000, 2.0)],
        )];
        assert_eq!(
            kinds(&evaluate(&frost, &[(Source::Loriot, true)], NOW, &c2)),
            vec![(Kind::Frost, "Cellar".into())]
        );
        // Per-room comfort override.
        let mut c3 = cfg.clone();
        c3.rooms.insert(
            "Office".into(),
            RoomSettings {
                cold_c: Some(14.0),
                ..Default::default()
            },
        );
        assert!(evaluate(&room, &up, NOW, &c3).is_empty());
    }

    // rivet: verifies REQ-WARN-004
    #[test]
    fn silence_sources_and_batteries() {
        let cfg = WarnConfig::default();
        let data = vec![
            // LoRaWAN, last value 90 min ago: silent.
            s(
                1,
                Source::Loriot,
                Quantity::Temperature,
                "T",
                "Cellar",
                "°C",
                &[(NOW - 90 * 60_000, 15.0)],
            ),
            // Loxone, 5 h without change: not silent (sends changes only).
            s(
                2,
                Source::Loxone,
                Quantity::Temperature,
                "T",
                "Office",
                "°C",
                &[(NOW - 5 * H, 20.0)],
            ),
            s(
                3,
                Source::Loriot,
                Quantity::BatteryVoltage,
                "Battery",
                "Laundry",
                "V",
                &[(NOW - 60_000, 2.5)],
            ),
            s(
                4,
                Source::Loxone,
                Quantity::BatteryVoltage,
                "Air battery",
                "Office",
                "%",
                &[(NOW - H, 15.0)],
            ),
            s(
                5,
                Source::Loxone,
                Quantity::BatteryVoltage,
                "Air battery 2",
                "Office",
                "%",
                &[(NOW - H, 80.0)],
            ),
        ];
        let w = evaluate(
            &data,
            &[(Source::Loriot, true), (Source::Loxone, false)],
            NOW,
            &cfg,
        );
        let k = kinds(&w);
        assert!(k.contains(&(Kind::SensorSilent, "Cellar".into())), "{k:?}");
        assert!(!k
            .iter()
            .any(|(k, r)| *k == Kind::SensorSilent && r == "Office"));
        assert!(k.contains(&(Kind::SourceDown, "loxone".into())));
        assert_eq!(w.iter().filter(|w| w.kind == Kind::BatteryLow).count(), 2);
        assert!(w
            .iter()
            .any(|w| w.kind == Kind::BatteryLow && w.message.contains("15.00%")));
    }

    // rivet: verifies REQ-WARN-005
    #[test]
    fn a_valve_open_without_warming_is_an_anomaly() {
        let cfg = WarnConfig::default();
        let up = [(Source::Loxone, true)];
        let valve = s(
            1,
            Source::Loxone,
            Quantity::ValvePosition,
            "Valve",
            "Office",
            "%",
            &[(NOW - 3 * H, 80.0)],
        );
        let flat = s(
            2,
            Source::Loxone,
            Quantity::Temperature,
            "T",
            "Office",
            "°C",
            &[(NOW - 3 * H, 18.0), (NOW - H, 18.1)],
        );
        let w = evaluate(&[valve.clone(), flat], &up, NOW, &cfg);
        assert_eq!(kinds(&w), vec![(Kind::HeatingAnomaly, "Office".into())]);
        // The room warmed by 1 K: fine.
        let warming = s(
            2,
            Source::Loxone,
            Quantity::Temperature,
            "T",
            "Office",
            "°C",
            &[(NOW - 3 * H, 18.0), (NOW - H, 19.0)],
        );
        assert!(evaluate(&[valve.clone(), warming.clone()], &up, NOW, &cfg).is_empty());
        // The valve closed in between: no anomaly.
        let closed = s(
            1,
            Source::Loxone,
            Quantity::ValvePosition,
            "Valve",
            "Office",
            "%",
            &[(NOW - 3 * H, 80.0), (NOW - H, 0.0)],
        );
        let flat2 = s(
            2,
            Source::Loxone,
            Quantity::Temperature,
            "T",
            "Office",
            "°C",
            &[(NOW - 3 * H, 18.0)],
        );
        assert!(evaluate(&[closed, flat2], &up, NOW, &cfg).is_empty());
    }

    #[test]
    fn the_config_file_rejects_unknown_keys_and_fills_defaults() {
        let c: WarnConfig =
            serde_json::from_str(r#"{"rooms": {"Cellar": {"frost_c": 3}}}"#).unwrap();
        assert_eq!(c.defaults, Thresholds::default());
        assert_eq!(c.rooms["Cellar"].frost_c, Some(3.0));
        assert!(
            serde_json::from_str::<WarnConfig>(r#"{"rooms": {"Cellar": {"frost": 3}}}"#).is_err()
        );
        let meta = |src, q| SeriesMeta {
            key: SeriesKey::new(src, "x", "f"),
            quantity: q,
            unit: String::new(),
            name: "x".into(),
            room: None,
        };
        assert!(window_ms(&c, &meta(Source::Loriot, Quantity::Temperature)) >= 4 * H);
        assert!(window_ms(&c, &meta(Source::Loriot, Quantity::Temperature)) < 24 * H);
        assert!(window_ms(&c, &meta(Source::Loxone, Quantity::Humidity)) >= 72 * H);
        assert!(window_ms(&c, &meta(Source::Loxone, Quantity::ValvePosition)) < 24 * H);
    }

    // rivet: verifies REQ-WARN-006
    // rivet: verifies DD-014
    #[test]
    fn a_loxone_sensor_unchanged_for_days_is_stuck_and_left_out_of_the_room() {
        let cfg = WarnConfig::default();
        let up = [(Source::Loxone, true), (Source::Loriot, true)];
        const D: i64 = 24 * H;
        // Standing since 80 days, repeated by the Miniserver at a reconnect.
        let stuck = s(
            1,
            Source::Loxone,
            Quantity::Temperature,
            "Touch temperature",
            "Bath",
            "°C",
            &[(NOW - 80 * D, 27.7), (NOW - H, 27.7)],
        );
        let alive = s(
            2,
            Source::Loxone,
            Quantity::Temperature,
            "Temperatur Heizkörper",
            "Bath",
            "°C",
            &[(NOW - 4 * D, 18.0), (NOW - 2 * D, 18.6), (NOW - H, 18.5)],
        );
        let w = evaluate(&[stuck.clone(), alive.clone()], &up, NOW, &cfg);
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].kind, Kind::SensorStuck);
        assert_eq!(w[0].series, vec![1]);
        assert_eq!(w[0].since_ms, Some(NOW - 80 * D));
        assert!(w[0].message.contains("27.7 °C"), "{}", w[0].message);
        // Without the stuck rule its 27.7 °C would make the room too warm.
        let mut never = cfg.clone();
        never.defaults.stuck_hours = 1e9;
        let w = evaluate(&[stuck.clone(), alive.clone()], &up, NOW, &never);
        assert!(w.iter().any(|w| w.kind == Kind::TooWarm), "{w:?}");
        // A stuck humidity raises no mould risk.
        let wet = s(
            3,
            Source::Loxone,
            Quantity::Humidity,
            "Luftfeuchte",
            "Bath",
            "%",
            &[(NOW - 10 * D, 80.0)],
        );
        let w = evaluate(&[wet], &up, NOW, &cfg);
        assert_eq!(
            w.iter().map(|w| w.kind).collect::<Vec<_>>(),
            vec![Kind::SensorStuck]
        );
        // A change inside the window: not stuck. Exactly at the edge: stuck.
        let moved = s(
            4,
            Source::Loxone,
            Quantity::Temperature,
            "T",
            "Hall",
            "°C",
            &[(NOW - 80 * D, 20.0), (NOW - 71 * H, 20.4)],
        );
        assert!(evaluate(&[moved], &up, NOW, &cfg).is_empty());
        let edge = s(
            5,
            Source::Loxone,
            Quantity::Temperature,
            "T",
            "Hall",
            "°C",
            &[(NOW - 72 * H, 20.0)],
        );
        assert_eq!(evaluate(&[edge], &up, NOW, &cfg)[0].kind, Kind::SensorStuck);
        // No point before the window (a new series): unknown, not stuck.
        let young = s(
            6,
            Source::Loxone,
            Quantity::Temperature,
            "T",
            "Hall",
            "°C",
            &[(NOW - 10 * H, 20.0)],
        );
        assert!(evaluate(&[young], &up, NOW, &cfg).is_empty());
        // LoRaWAN goes silent instead; valves are not watched.
        let lora = s(
            7,
            Source::Loriot,
            Quantity::Temperature,
            "T",
            "Cellar",
            "°C",
            &[(NOW - 10 * D, 20.0), (NOW - 5 * 60_000, 20.0)],
        );
        let valve = s(
            8,
            Source::Loxone,
            Quantity::ValvePosition,
            "Valve",
            "Hall",
            "%",
            &[(NOW - 10 * D, 0.0)],
        );
        assert!(evaluate(&[lora, valve], &up, NOW, &cfg).is_empty());
    }
}
