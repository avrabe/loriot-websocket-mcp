//! The 3D house page (REQ-UI-006, DD-017): the operator's house model
//! (HAUS_HOUSE_MODEL) drawn with vendored three.js, each room coloured by
//! its current air temperature. The page is the one exception to "the only
//! script is htmx" (REQ-UI-004): it also loads three.js 0.160.0 and its own
//! viewer, all vendored and served by haus, still built with cargo alone.

use crate::dashboard::{age, esc, is_stale, live, now_ms, page, Live, SeriesView};
use crate::state::{run_blocking, AppState};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use haus_core::house::House;
use haus_core::store::Quantity;
use haus_core::warn;
use serde::Serialize;
use std::collections::BTreeMap;

pub const THREE: &str = include_str!("../assets/three-0.160.0.module.min.js");
pub const ORBIT: &str = include_str!("../assets/three-0.160.0-OrbitControls.js");
pub const CSS2D: &str = include_str!("../assets/three-0.160.0-CSS2DRenderer.js");
pub const VIEWER: &str = include_str!("../assets/house3d.js");
/// warte's idle scene, adapted (REQ-UI-012).
pub const IDLE: &str = include_str!("../assets/idle.js");

/// The static files of this page: (name, body).
pub const ASSETS: [(&str, &str); 5] = [
    ("three-0.160.0.module.min.js", THREE),
    ("three-0.160.0-OrbitControls.js", ORBIT),
    ("three-0.160.0-CSS2DRenderer.js", CSS2D),
    ("house3d.js", VIEWER),
    ("idle.js", IDLE),
];

static HOUSE: std::sync::OnceLock<House> = std::sync::OnceLock::new();

/// Read and check `HAUS_HOUSE_MODEL` once at startup; unset: no house page.
pub fn load(file: Option<&str>) -> Result<(), String> {
    if let Some(h) = read(file)? {
        let _ = HOUSE.set(h);
    }
    Ok(())
}

fn read(file: Option<&str>) -> Result<Option<House>, String> {
    let Some(f) = file.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let json = std::fs::read_to_string(f).map_err(|e| format!("HAUS_HOUSE_MODEL {f}: {e}"))?;
    haus_core::house::parse(&json)
        .map(Some)
        .map_err(|e| format!("HAUS_HOUSE_MODEL {f}: {e}"))
}

/// Whether the page exists (the header links to it only then).
pub fn configured() -> bool {
    HOUSE.get().is_some()
}

/// The page body: the canvas, the panel, the info box and the scripts.
/// The idle scene's settings: it draws inside the house view; with a
/// weather place, the sky is computed for it and the weather is asked of
/// haus. JSON in a script element, so `<` is escaped.
fn idle_cfg() -> String {
    // The weather may come from the Miniserver without any place set, so
    // the scene always asks haus (a 404 just means none).
    let mut c = serde_json::json!({"host": ".house3d", "weather": true});
    if let Some(p) = crate::weather::place() {
        c["lat"] = p.lat.into();
        c["lon"] = p.lon.into();
    }
    c.to_string().replace('<', "\\u003c")
}

pub fn body() -> String {
    format!(
        r#"<div class="house3d"><div id="view"></div>
<div class="panel"><div class="panel-head"><b>House</b> <span><button id="fullscreen" type="button" title="Full screen (f)" aria-label="Full screen">⛶</button> <button id="collapse" type="button" title="Hide or show the panel">–</button></span></div><div id="pbody">
<h2>Floors</h2><div id="levels" class="row"></div>
<h2>Show</h2><div class="row">
<label><input type="checkbox" id="t-others" checked> other units</label>
<label><input type="checkbox" id="t-site" checked> surroundings</label>
<label><input type="checkbox" id="t-roof"> walls and roofs</label>
<label><input type="checkbox" id="t-tags"> unit numbers</label>
<label><input type="checkbox" id="t-weather" checked> weather</label>
<label>labels <select id="t-labels"><option value="compact" selected>compact</option><option value="full">full</option><option value="off">off</option></select></label>
</div>
<p id="views" class="row"></p>
<p class="row"><button id="tour" type="button" aria-pressed="false" title="Visit the rooms one after another; rooms with a warning come up every other step">▶ Tour</button> <label>every <select id="every"><option value="5">5 s</option><option value="10" selected>10 s</option><option value="20">20 s</option><option value="30">30 s</option></select></label></p>
<label for="explode">Explode floors</label><input id="explode" type="range" min="0" max="8" step="0.1" value="2.5">
<div class="scale"></div><div class="ticks"><span>16 °C</span><span>20</span><span>23</span><span>26 °C</span></div>
<p class="dim">Colour: the room's air temperature (median of its air sensors; floor and radiator probes, stuck and silent sensors left out; a room with no air sensor shows its floor probe, marked "floor"). Grey: no value. Red pulse: the room has a warning. Click a room for its sensors. On a wall screen open <code>house?tour=10</code>; touching the view pauses the tour for a minute.</p>
<p class="dim" id="wx"></p>
<p class="dim" id="asof"></p>
</div></div>
<div id="alerts" aria-live="polite"></div><div id="info"></div></div>
<script type="importmap">{{"imports": {{"three": "./static/three-0.160.0.module.min.js", "three/addons/controls/OrbitControls.js": "./static/three-0.160.0-OrbitControls.js", "three/addons/renderers/CSS2DRenderer.js": "./static/three-0.160.0-CSS2DRenderer.js"}}}}</script>
<script type="module" src="static/house3d.js?v={v}"></script>
<script type="application/json" id="idle-cfg">{idle}</script>
<script src="static/idle.js?v={v}" defer></script>"#,
        v = esc(env!("CARGO_PKG_VERSION")),
        idle = idle_cfg(),
    )
}

/// GET /house
pub async fn house_page() -> Response {
    if configured() {
        page("House", &body()).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            page(
                "House",
                r#"<p class="empty">No house model: set HAUS_HOUSE_MODEL to show the 3D view.</p>"#,
            ),
        )
            .into_response()
    }
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SeriesOut {
    pub id: u32,
    pub name: String,
    /// "Temperature · Spiegel": what the room overview shows (REQ-UI-007).
    pub label: String,
    pub quantity: String,
    pub value: Option<f64>,
    pub unit: String,
    pub age: String,
    pub stale: bool,
    /// Counts towards the room's colour.
    pub used: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct WarnOut {
    pub kind: warn::Kind,
    pub message: String,
    /// In a few words (REQ-UI-011).
    pub short: String,
    /// A sensor to check, not an alarm about the room.
    pub check: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct RoomOut {
    pub air_c: Option<f64>,
    /// air_c comes from the floor probe: the room has no air sensor.
    pub floor: bool,
    pub rh: Option<f64>,
    pub series: Vec<SeriesOut>,
    pub warnings: Vec<WarnOut>,
}

#[derive(Serialize, Debug)]
pub struct HouseOut<'a> {
    pub model: &'a House,
    /// By model room id; only rooms with haus rooms mapped.
    pub rooms: BTreeMap<String, RoomOut>,
    pub now_ms: i64,
}

fn quantity(q: Quantity) -> &'static str {
    match q {
        Quantity::Temperature => "Temperature",
        Quantity::Humidity => "Humidity",
        Quantity::BatteryVoltage => "Battery",
        Quantity::Motion => "Movements",
        Quantity::Activity => "Activity",
        Quantity::ValvePosition => "Valve",
        Quantity::Heating => "Heating",
        Quantity::Other => "Value",
    }
}

/// Each model room's values: its haus rooms' series, its climate by the
/// warning rules, and its warnings.
pub fn rooms(
    model: &House,
    data: &[warn::SeriesData],
    warnings: &[warn::Warning],
    lv: Live,
    now: i64,
    cfg: &warn::WarnConfig,
) -> BTreeMap<String, RoomOut> {
    let mut out = BTreeMap::new();
    for r in model.rooms.iter().filter(|r| !r.haus_rooms.is_empty()) {
        let mine: Vec<&warn::SeriesData> = data
            .iter()
            .filter(|s| {
                let room = s.meta.room.as_deref().unwrap_or(&s.meta.name);
                r.haus_rooms.iter().any(|h| h == room)
            })
            .collect();
        let c = warn::climate(&mine, now, cfg);
        let mut series: Vec<SeriesOut> = mine
            .iter()
            .filter(|s| s.followed)
            .map(|s| {
                let v = SeriesView {
                    id: s.id,
                    meta: s.meta.clone(),
                    latest: s.latest,
                };
                SeriesOut {
                    id: s.id,
                    name: s.meta.name.clone(),
                    label: crate::dashboard::what(
                        &s.meta,
                        s.meta.room.as_deref().unwrap_or(&s.meta.name),
                    ),
                    quantity: quantity(s.meta.quantity).into(),
                    value: s.latest.map(|(_, x)| x),
                    unit: s.meta.unit.clone(),
                    age: s
                        .latest
                        .map(|(t, _)| age(now, t))
                        .unwrap_or("no data".into()),
                    stale: is_stale(&v, lv, now),
                    used: c.used.contains(&s.id),
                }
            })
            .collect();
        series
            .sort_by(|a, b| (!a.used, &a.quantity, &a.name).cmp(&(!b.used, &b.quantity, &b.name)));
        out.insert(
            r.id.clone(),
            RoomOut {
                air_c: c.air_c,
                floor: c.floor,
                rh: c.rh,
                series,
                warnings: warnings
                    .iter()
                    .filter(|w| r.haus_rooms.contains(&w.room))
                    .map(|w| WarnOut {
                        kind: w.kind,
                        message: w.message.clone(),
                        short: crate::dashboard::short_text(
                            w,
                            |id| data.iter().find(|s| s.id == id).map(|s| &s.meta),
                            now,
                        ),
                        check: w.kind.is_check(),
                    })
                    .collect(),
            },
        );
    }
    out
}

/// GET /api/house: the model and every room's current values.
pub async fn house_api(State(state): State<AppState>) -> Response {
    let Some(model) = HOUSE.get() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let s = state.clone();
    let lv = live(&state);
    let now = now_ms();
    let result = run_blocking(move || -> Result<BTreeMap<String, RoomOut>, String> {
        let data = crate::dashboard::series_data(&s, now)?;
        let w = crate::dashboard::evaluate(&data, lv, now, &s.warn);
        Ok(rooms(model, &data, &w, lv, now, &s.warn))
    })
    .await;
    match result {
        Ok(rooms) => Json(HouseOut {
            model,
            rooms,
            now_ms: now,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("house: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use haus_core::store::{SeriesKey, SeriesMeta, Source};

    // rivet: verifies REQ-UI-004
    // rivet: verifies DD-017
    #[test]
    fn the_vendored_three_files_are_the_published_ones() {
        use sha2::{Digest, Sha256};
        // sha256 of the files in the npm tarball three-0.160.0.tgz, whose
        // sha512 matched the registry's integrity value when vendored.
        let pins = [
            (
                THREE,
                "3e690ac7d180b0aadf0891bea39eec643e29e2d3e75c99b18689518665f69ba6",
            ),
            (
                ORBIT,
                "5a44a9e86a2a0fb11933eed69bc2cd33c76a496854c1aed6ed776efa87d7b064",
            ),
            (
                CSS2D,
                "a4f0f79184c043f6b9d2654d8ba051e49a7d631d34e8f437c1804798a68c379f",
            ),
        ];
        for (body, want) in pins {
            let got: String = Sha256::digest(body.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(got, want);
        }
        // The addons import only from "three", which the import map serves.
        for addon in [ORBIT, CSS2D] {
            let imports: Vec<&str> = addon
                .lines()
                .filter(|l| {
                    (l.starts_with('}') || l.starts_with("import ")) && l.contains(" from ")
                })
                .collect();
            assert!(!imports.is_empty());
            for line in imports {
                assert!(line.contains("from 'three'"), "{line}");
            }
        }
    }

    // rivet: verifies REQ-UI-006
    // rivet: verifies REQ-UI-004
    // rivet: verifies DD-017
    #[test]
    fn the_page_loads_only_vendored_scripts() {
        let b = body();
        let srcs: Vec<&str> = b
            .split("src=\"")
            .skip(1)
            .map(|r| r.split('"').next().unwrap())
            .collect();
        // The viewer and the idle scene, both served by haus.
        assert_eq!(srcs.len(), 2, "{srcs:?}");
        for src in &srcs {
            let name = src
                .strip_prefix("static/")
                .unwrap()
                .split('?')
                .next()
                .unwrap();
            assert!(ASSETS.iter().any(|(n, _)| *n == name), "{src}");
        }
        // Neither script asks anything outside haus (the weather comes from
        // haus's api/weather, REQ-UI-012).
        for (name, js) in [("house3d.js", VIEWER), ("idle.js", IDLE)] {
            assert!(
                !js.contains("http://") && !js.contains("https://"),
                "{name} names an outside URL"
            );
        }
        let map = b.split(r#"<script type="importmap">"#).nth(1).unwrap();
        let map: serde_json::Value =
            serde_json::from_str(map.split("</script>").next().unwrap()).unwrap();
        for (_, url) in map["imports"].as_object().unwrap() {
            // Import map addresses must start with "./" (or "/"): a bare
            // "static/..." is not a URL there and the whole entry is dropped.
            let name = url.as_str().unwrap().strip_prefix("./static/").unwrap();
            assert!(ASSETS.iter().any(|(n, _)| *n == name), "{name}");
        }
        assert!(!b.contains("http://") && !b.contains("https://"));
        // The viewer imports exactly what the import map names.
        for line in VIEWER.lines().filter(|l| l.starts_with("import ")) {
            let spec = line.split(['\'', '"']).nth(1).unwrap();
            assert!(map["imports"].get(spec).is_some(), "{spec}");
        }
    }

    // Every element the viewer looks up exists on the page, the tour and
    // full-screen controls included.
    // rivet: verifies REQ-UI-010
    // rivet: verifies DD-019
    // rivet: verifies REQ-UI-014
    #[test]
    fn every_element_the_viewer_uses_is_on_the_page() {
        let b = body();
        let ids: Vec<&str> = VIEWER
            .split("$('")
            .skip(1)
            .map(|r| r.split('\'').next().unwrap())
            .collect();
        for id in [
            "tour",
            "every",
            "fullscreen",
            "alerts",
            "info",
            "view",
            "t-weather",
            "wx",
        ] {
            assert!(ids.contains(&id), "viewer does not use #{id}");
        }
        for id in &ids {
            assert!(
                b.contains(&format!(r#"id="{id}""#)),
                "#{id} is not on the page"
            );
        }
    }

    // rivet: verifies REQ-UI-006
    // rivet: verifies DD-017
    #[tokio::test]
    async fn without_a_model_there_is_no_house_page_and_no_link() {
        // No test sets a model: the process-wide one stays unset.
        assert!(!configured());
        assert!(read(None).unwrap().is_none() && read(Some("")).unwrap().is_none());
        assert!(read(Some("/nonexistent/house.json"))
            .unwrap_err()
            .starts_with("HAUS_HOUSE_MODEL /nonexistent/house.json"));
        let page = page("t", "b").0;
        assert!(
            !page.contains(r#"href="house""#) && !page.contains("{{HOUSE}}"),
            "{page}"
        );
        assert_eq!(house_page().await.status(), StatusCode::NOT_FOUND);
    }

    // rivet: verifies REQ-UI-006
    // rivet: verifies DD-017
    #[test]
    fn each_room_gets_its_haus_rooms_values_and_warnings() {
        let model =
            haus_core::house::parse(include_str!("../../haus-core/examples/house.json")).unwrap();
        const NOW: i64 = 1_800_000_000_000;
        let s = |id, room: &str, q, name: &str, v: f64| warn::SeriesData {
            id,
            meta: SeriesMeta {
                key: SeriesKey::new(Source::Loxone, format!("u{id}"), "Default"),
                quantity: q,
                unit: if q == Quantity::Humidity { "%" } else { "°C" }.into(),
                name: name.into(),
                room: Some(room.into()),
            },
            latest: Some((NOW - 60_000, v)),
            recent: vec![(NOW - 60_000, v)],
            followed: true,
        };
        let t = Quantity::Temperature;
        let mut retired = s(9, "Bedroom", t, "Old", 5.0);
        retired.followed = false;
        let data = vec![
            s(1, "Bedroom", t, "Air", 15.0),
            s(2, "Bedroom", Quantity::Humidity, "Humidity", 50.0),
            s(3, "Bedroom", t, "Bedroom Fußbodenheizung Temperatur", 28.0),
            retired,
            s(4, "Kitchen", t, "Air", 21.0),
            s(5, "Somewhere else", t, "Air", 30.0),
        ];
        let cfg = warn::WarnConfig::default();
        let lv = Live {
            loriot: true,
            loxone: Some(true),
        };
        let w = warn::evaluate(&data, &[(Source::Loxone, true)], NOW, &cfg);
        let r = rooms(&model, &data, &w, lv, NOW, &cfg);
        // Only model rooms with haus rooms appear.
        assert_eq!(
            r.keys().collect::<Vec<_>>(),
            ["bath", "bedroom", "kitchen", "laundry", "living"]
        );
        let bed = &r["bedroom"];
        assert_eq!((bed.air_c, bed.rh), (Some(15.0), Some(50.0)));
        // The retired series is not listed; the floor probe is, not counted.
        assert_eq!(
            bed.series
                .iter()
                .map(|s| (s.id, s.used))
                .collect::<Vec<_>>(),
            [(2, true), (1, true), (3, false)]
        );
        assert_eq!(
            bed.warnings.iter().map(|w| w.kind).collect::<Vec<_>>(),
            [warn::Kind::TooCold]
        );
        assert_eq!(r["kitchen"].air_c, Some(21.0));
        assert!(r["kitchen"].warnings.is_empty());
        assert_eq!(r["living"].air_c, None);
        assert!(r["living"].series.is_empty());
    }
}
