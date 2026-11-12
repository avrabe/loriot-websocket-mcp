//! The server-rendered dashboard (REQ-UI-001..004, DD-002): HTML built on the
//! server from the store, vendored htmx for refreshing the overview, SVG
//! trends drawn here. No wasm, no npm.
//!
//! Every value is shown with the time it was measured; a value older than
//! its source's hold, or from a source that is not connected, is marked
//! stale (SC-003). Store reads run on a blocking thread (REQ-STORE-001).

use crate::state::{run_blocking, AppState};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
};
use haus_core::chart;
use haus_core::store::{Quantity, SeriesId, SeriesMeta, Source};
use haus_core::warn::{self, Kind, Warning};
use std::collections::BTreeMap;

const PAGE: &str = include_str!("../assets/page.html");
const CSS: &str = include_str!("../assets/haus.css");
const HTMX: &str = include_str!("../assets/htmx-2.0.4.min.js");

/// One series with its newest point.
#[derive(Clone, Debug)]
pub struct SeriesView {
    pub id: SeriesId,
    pub meta: SeriesMeta,
    pub latest: Option<(i64, f64)>,
}

/// Which sources are connected right now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Live {
    pub loriot: bool,
    /// `None`: the Loxone source is not configured.
    pub loxone: Option<bool>,
}

fn live(state: &AppState) -> Live {
    Live {
        loriot: state.sources.loriot.connected(),
        loxone: state.sources.loxone.as_ref().map(|h| h.connected()),
    }
}

/// HTML-escape text.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// "just now", "5 min ago", "3 h ago", "2 d ago".
pub fn age(now_ms: i64, ts_ms: i64) -> String {
    let s = (now_ms - ts_ms).max(0) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=172_799 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

/// Whether a value no longer says how things are now (SC-003).
pub fn is_stale(v: &SeriesView, live: Live, now_ms: i64) -> bool {
    let Some((ts, _)) = v.latest else {
        return true;
    };
    match v.meta.key.source {
        Source::Loriot => now_ms - ts > chart::hold_ms(Source::Loriot),
        // Loxone sends only changes: an old value is current while the
        // source is connected.
        Source::Loxone => live.loxone != Some(true) || now_ms - ts > chart::hold_ms(Source::Loxone),
    }
}

fn label(q: Quantity) -> &'static str {
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

fn value(v: f64, unit: &str) -> String {
    let n = if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    };
    if unit.is_empty() {
        n
    } else {
        format!("{n} {unit}")
    }
}

/// Series silent for longer than this are left off the overview (retired
/// sensors); the status page still lists them.
pub const OVERVIEW_MAX_AGE_MS: i64 = 30 * 86_400_000;

/// "Temperature · Kitchen sensor", plus the field when a sensor has several.
fn what(m: &SeriesMeta, room: &str) -> String {
    let mut w = label(m.quantity).to_string();
    if m.name != room {
        w = format!("{w} · {}", m.name);
    }
    let f = m.key.field.as_str();
    if m.key.source == Source::Loxone && f != "Default" {
        w = format!("{w} ({f})");
    }
    w
}

/// The room a series belongs to: its room, else its display name.
fn room_of(m: &SeriesMeta) -> &str {
    m.room.as_deref().unwrap_or(&m.name)
}

/// Order of quantities within a room.
fn rank(q: Quantity) -> u8 {
    match q {
        Quantity::Temperature => 0,
        Quantity::Humidity => 1,
        Quantity::Heating => 2,
        Quantity::ValvePosition => 3,
        _ => 9,
    }
}

/// The path prefix every page URL starts with (`HAUS_BASE_PATH`, REQ-UI-008):
/// "" or e.g. "/haus". Pages link relatively against `<base href>`, so only
/// the template needs it.
static BASE_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Set once at startup, before the server answers.
pub fn set_base_path(base: &str) {
    let _ = BASE_PATH.set(base.to_string());
}

fn base_href() -> String {
    format!("{}/", BASE_PATH.get().map(String::as_str).unwrap_or(""))
}

/// `HAUS_BASE_PATH`: unset, empty or "/" is no prefix; otherwise "/" followed
/// by segments of letters, digits, '-', '_' and '.', without "." or "..".
/// A trailing "/" is dropped.
pub fn parse_base_path(v: Option<&str>) -> Result<String, String> {
    let v = v.unwrap_or("").trim();
    let v = v.strip_suffix('/').unwrap_or(v);
    if v.is_empty() {
        return Ok(String::new());
    }
    let ok = v.starts_with('/')
        && v[1..].split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        });
    if ok {
        Ok(v.to_string())
    } else {
        Err(format!(
            "HAUS_BASE_PATH {v:?}: expected a path like /haus (letters, digits, - _ . between slashes)"
        ))
    }
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(
        PAGE.replace("{{BASE}}", &esc(&base_href()))
            .replace("{{TITLE}}", &esc(title))
            .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
            .replace("{{BODY}}", body),
    )
}

fn source_chips(live: Live) -> String {
    let chip = |name: &str, up: bool| {
        format!(
            r#"<span class="chip {}">{name} {}</span>"#,
            if up { "up" } else { "down" },
            if up { "connected" } else { "not connected" }
        )
    };
    let mut s = String::from(r#"<div class="sources">"#);
    s += &chip("LoRaWAN", live.loriot);
    if let Some(l) = live.loxone {
        s += &chip("Loxone", l);
    }
    s + "</div>"
}

/// The overview: every room, its climate and heating values with their age
/// (REQ-UI-001). The wrapper re-fetches itself every 30 s.
fn warn_icon(k: Kind) -> &'static str {
    match k {
        Kind::MouldRisk => "💧",
        Kind::TooCold | Kind::Frost => "❄️",
        Kind::TooWarm => "🔥",
        Kind::SensorSilent | Kind::SourceDown => "📡",
        Kind::SensorStuck => "⏸️",
        Kind::BatteryLow => "🔋",
        Kind::HeatingAnomaly => "♨️",
    }
}

fn warn_item(w: &Warning) -> String {
    format!(
        r#"<li class="warn">{} {}</li>"#,
        warn_icon(w.kind),
        esc(&w.message)
    )
}

pub fn render_overview(
    views: &[SeriesView],
    live: Live,
    now_ms: i64,
    warnings: &[Warning],
) -> String {
    let mut rooms: BTreeMap<String, Vec<&SeriesView>> = BTreeMap::new();
    for v in views.iter().filter(|v| {
        rank(v.meta.quantity) < 9
            && v.latest
                .is_some_and(|(ts, _)| now_ms - ts <= OVERVIEW_MAX_AGE_MS)
    }) {
        rooms
            .entry(room_of(&v.meta).to_string())
            .or_default()
            .push(v);
    }
    let mut s = String::from(
        r#"<div id="overview" hx-get="partials/overview" hx-trigger="every 30s" hx-swap="outerHTML">"#,
    );
    s += &source_chips(live);
    let room_names: std::collections::HashSet<&str> = rooms.keys().map(String::as_str).collect();
    let elsewhere: Vec<&Warning> = warnings
        .iter()
        .filter(|w| !room_names.contains(w.room.as_str()))
        .collect();
    if !elsewhere.is_empty() {
        s += r#"<ul class="warnings">"#;
        for w in elsewhere {
            s += &format!(
                r#"<li class="warn">{} <b>{}</b>: {}</li>"#,
                warn_icon(w.kind),
                esc(&w.room),
                esc(&w.message)
            );
        }
        s += "</ul>";
    }
    if rooms.is_empty() {
        s += r#"<p class="empty">No series yet. Import a history or connect a source.</p>"#;
    }
    s += r#"<section class="rooms">"#;
    for (room, mut vs) in rooms {
        vs.sort_by_key(|v| (rank(v.meta.quantity), v.meta.name.clone(), v.id));
        let mine: Vec<&Warning> = warnings.iter().filter(|w| w.room == room).collect();
        s += &format!(
            r#"<article class="room{}"><h2>{}</h2><ul>"#,
            if mine.is_empty() { "" } else { " warned" },
            esc(&room)
        );
        for w in mine {
            s += &warn_item(w);
        }
        for v in vs {
            let stale = is_stale(v, live, now_ms);
            let (val, when) = match v.latest {
                Some((ts, x)) => (value(x, &v.meta.unit), age(now_ms, ts)),
                None => ("–".into(), "no data".into()),
            };
            let what = what(&v.meta, &room);
            s += &format!(
                r#"<li{}><a href="sensor/{}"><span class="label">{}</span><span class="val">{}</span><span class="age">{}</span></a></li>"#,
                if stale { r#" class="stale""# } else { "" },
                v.id,
                esc(&what),
                esc(&val),
                esc(&when)
            );
        }
        s += "</ul></article>";
    }
    s + "</section></div>"
}

fn local_label(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%d.%m. %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// One series: latest value, age, and its trend over the chosen range
/// (REQ-UI-002).
pub fn render_sensor(
    v: &SeriesView,
    points: &[(i64, f64)],
    range_key: &str,
    range_ms: i64,
    live: Live,
    now_ms: i64,
) -> String {
    let from = now_ms - range_ms;
    let cols = chart::columns(
        points,
        from,
        now_ms,
        chart::COLUMNS,
        chart::hold_ms(v.meta.key.source),
    );
    let svg = chart::svg(
        &cols,
        &v.meta.unit,
        (&local_label(from), &local_label(now_ms)),
    );
    let mut s = format!(
        r#"<h1>{} · {}</h1><div class="facts">"#,
        esc(room_of(&v.meta)),
        esc(&what(&v.meta, room_of(&v.meta))),
    );
    match v.latest {
        Some((ts, x)) => {
            s += &format!(
                "<span>now <b>{}</b></span><span{}>measured {} ({})</span>",
                esc(&value(x, &v.meta.unit)),
                if is_stale(v, live, now_ms) {
                    r#" class="stale""#
                } else {
                    ""
                },
                esc(&age(now_ms, ts)),
                esc(&local_label(ts))
            )
        }
        None => s += "<span>no data yet</span>",
    }
    s += &format!(
        "<span>source <b>{}</b> {}</span></div><div class=\"ranges\">",
        v.meta.key.source,
        esc(&v.meta.key.source_id)
    );
    for (k, _, l) in chart::RANGES {
        s += &format!(
            r#"<a href="sensor/{}?range={k}"{}>{l}</a>"#,
            v.id,
            if k == range_key { r#" class="on""# } else { "" }
        );
    }
    s + "</div>" + &svg
}

/// Sources, batteries and the age of every series (REQ-UI-003).
pub fn render_status(
    views: &[SeriesView],
    live: Live,
    now_ms: i64,
    health: &str,
    warnings: &[Warning],
) -> String {
    let mut s = String::from("<h1>Status</h1>");
    if warnings.is_empty() {
        s += r#"<p class="facts"><span>No warnings.</span></p>"#;
    } else {
        s += "<h2>Warnings</h2><table><tr><th>Room</th><th>Warning</th><th>Since</th></tr>";
        for w in warnings {
            s += &format!(
                "<tr><td>{}</td><td>{} {}</td><td>{}</td></tr>",
                esc(&w.room),
                warn_icon(w.kind),
                esc(&w.message),
                w.since_ms.map(|t| esc(&age(now_ms, t))).unwrap_or_default()
            );
        }
        s += "</table>";
    }
    s += &source_chips(live);
    s += &format!(r#"<p class="facts"><span>{}</span></p>"#, esc(health));
    let batteries: Vec<&SeriesView> = views
        .iter()
        .filter(|v| v.meta.quantity == Quantity::BatteryVoltage)
        .collect();
    if !batteries.is_empty() {
        s += "<h2>Batteries</h2><table><tr><th>Sensor</th><th>Voltage</th><th>Measured</th></tr>";
        for v in batteries {
            let (val, when) = v
                .latest
                .map(|(ts, x)| (value(x, &v.meta.unit), age(now_ms, ts)))
                .unwrap_or_else(|| ("–".into(), "no data".into()));
            s += &format!(
                r#"<tr><td><a href="sensor/{}">{}</a></td><td class="num">{}</td><td>{}</td></tr>"#,
                v.id,
                esc(&v.meta.name),
                esc(&val),
                esc(&when)
            );
        }
        s += "</table>";
    }
    let mut all: Vec<&SeriesView> = views.iter().collect();
    // Oldest first: what has gone quiet is what to look at.
    all.sort_by_key(|v| v.latest.map(|(ts, _)| ts).unwrap_or(i64::MIN));
    s += "<h2>All series</h2><table><tr><th>Room</th><th>Series</th><th>Source</th><th>Last value</th><th>Measured</th></tr>";
    for v in all {
        let (val, when) = v
            .latest
            .map(|(ts, x)| (value(x, &v.meta.unit), age(now_ms, ts)))
            .unwrap_or_else(|| ("–".into(), "no data".into()));
        s += &format!(
            r#"<tr{}><td>{}</td><td><a href="sensor/{}">{}</a></td><td>{}</td><td class="num">{}</td><td>{}</td></tr>"#,
            if is_stale(v, live, now_ms) {
                r#" class="stale""#
            } else {
                ""
            },
            esc(room_of(&v.meta)),
            v.id,
            esc(&what(&v.meta, room_of(&v.meta))),
            v.meta.key.source,
            esc(&val),
            esc(&when)
        );
    }
    s + "</table>"
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Every series with its newest point; blocking.
fn load_views(state: &AppState) -> Result<Vec<SeriesView>, String> {
    let store = state.ingest.store();
    let mut out = Vec::new();
    for (id, meta) in store.series().map_err(|e| e.to_string())? {
        out.push(SeriesView {
            id,
            latest: store.latest(id).map_err(|e| e.to_string())?,
            meta,
        });
    }
    Ok(out)
}

fn error(e: String) -> Response {
    tracing::error!("dashboard: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        page(
            "Error",
            "<p class=\"empty\">The store could not be read.</p>",
        ),
    )
        .into_response()
}

async fn views(state: &AppState) -> Result<Vec<SeriesView>, String> {
    let s = state.clone();
    run_blocking(move || load_views(&s)).await
}

/// The current warnings (REQ-WARN-001), evaluated from the store; blocking
/// reads off the runtime. An unreadable store gives no warnings, logged.
pub async fn warnings(state: &AppState) -> Vec<Warning> {
    let s = state.clone();
    let now = now_ms();
    let lv = live(state);
    let result = run_blocking(move || -> Result<Vec<Warning>, String> {
        let store = s.ingest.store();
        let mut data = Vec::new();
        for (id, meta) in store.series().map_err(|e| e.to_string())? {
            // The window, and the value standing at its start (however old:
            // a sensor stuck for months has no point inside the window).
            let from = now - warn::window_ms(&s.warn, &meta);
            let mut recent: Vec<(i64, f64)> = store
                .before(id, from)
                .map_err(|e| e.to_string())?
                .into_iter()
                .collect();
            recent.extend(store.range(id, from, now).map_err(|e| e.to_string())?);
            data.push(warn::SeriesData {
                id,
                latest: recent.last().copied(),
                meta,
                recent,
            });
        }
        let mut sources = vec![(haus_core::store::Source::Loriot, lv.loriot)];
        if let Some(l) = lv.loxone {
            sources.push((haus_core::store::Source::Loxone, l));
        }
        Ok(warn::evaluate(&data, &sources, now, &s.warn))
    })
    .await;
    result.unwrap_or_else(|e| {
        tracing::error!("warnings: {e}");
        Vec::new()
    })
}

/// GET /api/warnings
pub async fn warnings_api(State(state): State<AppState>) -> axum::Json<Vec<Warning>> {
    axum::Json(warnings(&state).await)
}

/// GET /
pub async fn overview_page(State(state): State<AppState>) -> Response {
    match views(&state).await {
        Ok(v) => {
            let w = warnings(&state).await;
            page("Rooms", &render_overview(&v, live(&state), now_ms(), &w)).into_response()
        }
        Err(e) => error(e),
    }
}

/// GET /partials/overview
pub async fn overview_partial(State(state): State<AppState>) -> Response {
    match views(&state).await {
        Ok(v) => {
            let w = warnings(&state).await;
            Html(render_overview(&v, live(&state), now_ms(), &w)).into_response()
        }
        Err(e) => error(e),
    }
}

#[derive(serde::Deserialize)]
pub struct RangeQuery {
    range: Option<String>,
}

/// A series and the points of its chart.
type SensorData = (SeriesView, Vec<(i64, f64)>);

/// GET /sensor/{id}?range=1h|24h|7d|90d
pub async fn sensor_page(
    State(state): State<AppState>,
    Path(id): Path<SeriesId>,
    Query(q): Query<RangeQuery>,
) -> Response {
    let (key, ms) = chart::range(q.range.as_deref());
    let now = now_ms();
    let s = state.clone();
    let loaded = run_blocking(move || -> Result<Option<SensorData>, String> {
        let store = s.ingest.store();
        let Some((_, meta)) = store
            .series()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|(i, _)| *i == id)
        else {
            return Ok(None);
        };
        // Start early enough that the value standing at the range start shows.
        let hold = chart::hold_ms(meta.key.source);
        let points = store
            .range(id, now - ms - hold, now)
            .map_err(|e| e.to_string())?;
        let latest = store.latest(id).map_err(|e| e.to_string())?;
        Ok(Some((SeriesView { id, meta, latest }, points)))
    })
    .await;
    match loaded {
        Ok(Some((v, points))) => page(
            &v.meta.name,
            &render_sensor(&v, &points, key, ms, live(&state), now),
        )
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            page("Not found", "<p class=\"empty\">No such series.</p>"),
        )
            .into_response(),
        Err(e) => error(e),
    }
}

/// GET /status
pub async fn status_page(State(state): State<AppState>) -> Response {
    match views(&state).await {
        Ok(v) => {
            let health = format!(
                "{} series in the store, version {}",
                v.len(),
                env!("CARGO_PKG_VERSION")
            );
            page(
                "Status",
                &render_status(&v, live(&state), now_ms(), &health, &warnings(&state).await),
            )
            .into_response()
        }
        Err(e) => error(e),
    }
}

/// GET /static/haus.css and /static/htmx-2.0.4.min.js
pub async fn static_file(Path(name): Path<String>) -> Response {
    let (body, ctype) = match name.as_str() {
        "haus.css" => (CSS, "text/css; charset=utf-8"),
        "htmx-2.0.4.min.js" => (HTMX, "text/javascript; charset=utf-8"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, ctype),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::test_state;
    use haus_core::store::{Point, SeriesKey};
    use haus_core::AppConfig;

    const NOW: i64 = 1_800_000_000_000;
    const MIN: i64 = 60_000;

    fn view(
        id: u32,
        src: Source,
        q: Quantity,
        name: &str,
        room: Option<&str>,
        age_ms: Option<i64>,
        v: f64,
    ) -> SeriesView {
        SeriesView {
            id,
            meta: SeriesMeta {
                key: SeriesKey::new(src, format!("s{id}"), "f"),
                quantity: q,
                unit: match q {
                    Quantity::Temperature => "°C".into(),
                    Quantity::Humidity | Quantity::ValvePosition => "%".into(),
                    Quantity::BatteryVoltage => "V".into(),
                    _ => String::new(),
                },
                name: name.into(),
                room: room.map(str::to_string),
            },
            latest: age_ms.map(|a| (NOW - a, v)),
        }
    }

    fn sample() -> Vec<SeriesView> {
        vec![
            view(
                0,
                Source::Loriot,
                Quantity::Humidity,
                "Cellar",
                None,
                Some(2 * 3600 * 1000),
                71.0,
            ),
            view(
                1,
                Source::Loriot,
                Quantity::Temperature,
                "Cellar",
                None,
                Some(2 * MIN),
                18.88,
            ),
            view(
                2,
                Source::Loriot,
                Quantity::BatteryVoltage,
                "Cellar",
                None,
                Some(2 * MIN),
                3.1,
            ),
            view(
                3,
                Source::Loxone,
                Quantity::Temperature,
                "Room A sensor",
                Some("Room A"),
                Some(5 * 3600 * 1000),
                21.5,
            ),
            view(
                4,
                Source::Loxone,
                Quantity::ValvePosition,
                "Valve <b>",
                Some("Room A"),
                Some(10 * MIN),
                40.0,
            ),
            view(
                5,
                Source::Loxone,
                Quantity::Heating,
                "Controller",
                Some("Room A"),
                None,
                0.0,
            ),
        ]
    }

    // rivet: verifies REQ-UI-001
    // rivet: verifies DD-002
    #[test]
    fn overview_groups_rooms_shows_ages_and_marks_stale_values() {
        let live = Live {
            loriot: true,
            loxone: Some(true),
        };
        let html = render_overview(&sample(), live, NOW, &[]);
        // Rooms in order, each a card; the battery is not on the overview.
        let cellar = html.find("<h2>Cellar</h2>").expect("Cellar room");
        let room_a = html.find("<h2>Room A</h2>").expect("Room A room");
        assert!(cellar < room_a);
        assert!(!html.contains("sensor/2\""), "battery on the overview");
        // Temperature before humidity within a room.
        assert!(html.find("sensor/1\"").unwrap() < html.find("sensor/0\"").unwrap());
        // Values with units and ages.
        assert!(
            html.contains("18.9 °C") && html.contains("2 min ago"),
            "{html}"
        );
        // LoRaWAN humidity 2 h old: stale. Loxone 5 h old while connected: not.
        let li = |id: u32| {
            let i = html.find(&format!("sensor/{id}\"")).unwrap();
            html[..i]
                .rfind("<li")
                .map(|j| html[j..i].to_string())
                .unwrap()
        };
        assert!(li(0).contains("stale"), "{}", li(0));
        assert!(!li(1).contains("stale"));
        assert!(!li(3).contains("stale"));
        // No value yet, or silent for months: not on the overview.
        assert!(!html.contains("sensor/5\""));
        let mut old = sample();
        old[1].latest = Some((NOW - OVERVIEW_MAX_AGE_MS - 1, 5.0));
        assert!(!render_overview(&old, live, NOW, &[]).contains("sensor/1\""));
        // Names are escaped.
        assert!(html.contains("Valve &lt;b&gt;") && !html.contains("Valve <b>"));
        // The overview refreshes itself.
        assert!(html.contains(r#"hx-get="partials/overview""#));

        // Loxone down: every Loxone value is stale, LoRaWAN unaffected.
        let html = render_overview(
            &sample(),
            Live {
                loriot: true,
                loxone: Some(false),
            },
            NOW,
            &[],
        );
        assert!(html.contains("Loxone not connected"));
        let i = html.find("sensor/3\"").unwrap();
        assert!(html[html[..i].rfind("<li").unwrap()..i].contains("stale"));
    }

    // rivet: verifies REQ-UI-003
    #[test]
    fn status_lists_sources_batteries_and_oldest_series_first() {
        let html = render_status(
            &sample(),
            Live {
                loriot: false,
                loxone: Some(true),
            },
            NOW,
            "6 series",
            &[],
        );
        assert!(html.contains("LoRaWAN not connected") && html.contains("Loxone connected"));
        assert!(html.contains("<h2>Batteries</h2>") && html.contains("3.1 V"));
        // The series without data comes first, then the oldest.
        let first = html.find("sensor/5\"").unwrap();
        let oldest = html.find("sensor/3\"").unwrap();
        let newest = html.find("sensor/1\"").unwrap();
        assert!(first < oldest && oldest < newest);
    }

    #[test]
    fn sensor_page_has_trend_ranges_and_age() {
        let v = &sample()[1];
        let pts: Vec<(i64, f64)> = (0..200)
            .map(|i| (NOW - i * 10 * MIN, 18.0 + i as f64 / 100.0))
            .rev()
            .collect();
        let html = render_sensor(
            v,
            &pts,
            "24h",
            86_400_000,
            Live {
                loriot: true,
                loxone: None,
            },
            NOW,
        );
        assert!(html.contains("<svg") && html.contains(r#"class="line""#));
        assert!(html.contains(r#"href="sensor/1?range=24h" class="on""#));
        assert!(html.contains("2 min ago"));
        for (k, _, _) in chart::RANGES {
            assert!(html.contains(&format!("range={k}")));
        }
    }

    // rivet: verifies REQ-WARN-001
    #[test]
    fn warnings_show_on_their_room_and_on_the_status_page() {
        let live = Live {
            loriot: true,
            loxone: Some(false),
        };
        let w = vec![
            Warning {
                kind: Kind::MouldRisk,
                room: "Room A".into(),
                series: vec![4],
                message: "RH <x> at or above 65 % for 4 h: mould risk".into(),
                since_ms: Some(NOW - 4 * 3_600_000),
            },
            Warning {
                kind: Kind::SourceDown,
                room: "loxone".into(),
                series: vec![],
                message: "loxone is not connected".into(),
                since_ms: None,
            },
        ];
        let html = render_overview(&sample(), live, NOW, &w);
        // On Room A's card, escaped; the card is marked.
        let card = &html[html.find("<h2>Room A</h2>").unwrap()..];
        let card = &card[..card.find("</article>").unwrap()];
        assert!(
            card.contains("mould risk") && card.contains("RH &lt;x&gt;"),
            "{card}"
        );
        assert!(html.contains(r#"<article class="room warned"><h2>Room A</h2>"#));
        // Not on another room's card.
        let other = &html[html.find("<h2>Cellar</h2>").unwrap()..];
        assert!(!other[..other.find("</article>").unwrap()].contains("mould"));
        // A source warning sits above the rooms.
        assert!(html.find("loxone is not connected").unwrap() < html.find("<section").unwrap());
        // The status page lists both, with the age.
        let st = render_status(&sample(), live, NOW, "x", &w);
        assert!(
            st.contains("<h2>Warnings</h2>")
                && st.contains("4 h ago")
                && st.contains("not connected")
        );
        assert!(render_status(&sample(), live, NOW, "x", &[]).contains("No warnings."));
    }

    #[test]
    fn ages_read_naturally() {
        assert_eq!(age(NOW, NOW), "just now");
        assert_eq!(age(NOW, NOW - 5 * MIN), "5 min ago");
        assert_eq!(age(NOW, NOW - 3 * 3600 * 1000), "3 h ago");
        assert_eq!(age(NOW, NOW - 3 * 86_400_000), "3 d ago");
        assert_eq!(
            esc(r#"<a href="x">'&"#),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;"
        );
    }

    // rivet: verifies REQ-UI-004
    // rivet: verifies DD-008
    #[test]
    fn the_only_script_is_the_vendored_htmx() {
        let page = page("t", "b").0;
        assert_eq!(page.matches("<script").count(), 1, "{page}");
        assert!(page.contains(r#"<script src="static/htmx-2.0.4.min.js" defer>"#));
        assert!(!page.contains("wasm"));
        // The vendored file is the published htmx 2.0.4 (sha256 checked
        // against unpkg when vendored).
        assert!(HTMX.starts_with("var htmx=function()") || HTMX.contains("htmx"));
        assert_eq!(HTMX.len(), 50917);
        // No JavaScript toolchain in the repository.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for f in [
            "package.json",
            "package-lock.json",
            "crates/loriot-web",
            "Trunk.toml",
        ] {
            assert!(!root.join(f).exists(), "{f} is back");
        }
    }

    // rivet: verifies REQ-UI-008
    // rivet: verifies DD-015
    #[test]
    fn pages_link_relative_to_the_base_path() {
        assert_eq!(parse_base_path(None).unwrap(), "");
        for empty in ["", "/", " "] {
            assert_eq!(parse_base_path(Some(empty)).unwrap(), "", "{empty:?}");
        }
        assert_eq!(parse_base_path(Some("/haus")).unwrap(), "/haus");
        assert_eq!(parse_base_path(Some("/haus/")).unwrap(), "/haus");
        assert_eq!(parse_base_path(Some("/a/b-c_1.2")).unwrap(), "/a/b-c_1.2");
        for bad in ["haus", "/haus//x", "/../x", "/a/.", "/ha us", "/h\"x", "//"] {
            assert!(parse_base_path(Some(bad)).is_err(), "{bad:?}");
        }
        // The template is the only place with the prefix: no other URL in
        // a page or partial starts with "/".
        let page = page("t", "b").0;
        assert!(page.contains(r#"<base href="/">"#), "{page}");
        let live = Live {
            loriot: true,
            loxone: Some(true),
        };
        let views = sample();
        let pages = [
            page,
            render_overview(&views, live, NOW, &[]),
            render_sensor(&views[1], &[], "24h", 86_400_000, live, NOW),
            render_status(&views, live, NOW, "x", &[]),
        ];
        assert!(pages[1].contains(r#"href="sensor/1""#), "{}", pages[1]);
        for html in pages {
            let html = html.replacen(r#"<base href="/">"#, "", 1);
            for attr in ["href=\"/", "src=\"/", "hx-get=\"/", "action=\"/"] {
                assert!(!html.contains(attr), "{attr} in {html}");
            }
        }
    }

    fn app(state: AppState) -> axum::Router {
        use axum::routing::get;
        axum::Router::new()
            .route("/", get(overview_page))
            .route("/partials/overview", get(overview_partial))
            .route("/sensor/{id}", get(sensor_page))
            .route("/status", get(status_page))
            .route("/static/{name}", get(static_file))
            .route("/api/warnings", get(warnings_api))
            .with_state(state)
    }

    async fn get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
        use tower::ServiceExt;
        let r = app
            .clone()
            .oneshot(
                axum::http::Request::get(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = r.status();
        let body = axum::body::to_bytes(r.into_body(), 10 << 20).await.unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    // rivet: verifies REQ-WARN-006
    #[tokio::test]
    async fn warnings_see_the_value_standing_long_before_the_window() {
        let state = test_state("dash-stuck", AppConfig::default());
        let store = state.ingest.store().clone();
        let meta = view(
            9,
            Source::Loxone,
            Quantity::Temperature,
            "Touch temperature",
            Some("Bathroom"),
            None,
            0.0,
        )
        .meta;
        let id = store.ensure_series(&meta).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let day = 24 * 60 * MIN;
        // Last real change 80 days ago, repeated at a reconnect an hour ago.
        store
            .write(&[
                Point {
                    series: id,
                    ts_ms: now - 80 * day,
                    value: 27.7,
                },
                Point {
                    series: id,
                    ts_ms: now - 60 * MIN,
                    value: 27.7,
                },
            ])
            .unwrap();
        let w = warnings(&state).await;
        let stuck: Vec<_> = w.iter().filter(|w| w.kind == Kind::SensorStuck).collect();
        assert_eq!(stuck.len(), 1, "{w:?}");
        assert_eq!(stuck[0].since_ms, Some(now - 80 * day));
        assert!(!w.iter().any(|w| w.kind == Kind::TooWarm), "{w:?}");
    }

    #[tokio::test]
    async fn routes_answer() {
        let state = test_state("dash-routes", AppConfig::default());
        let store = state.ingest.store().clone();
        let id = store.ensure_series(&sample()[1].meta).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        store
            .write(&[Point {
                series: id,
                ts_ms: now - MIN,
                value: 19.5,
            }])
            .unwrap();
        let app = app(state);
        let (s, b) = get(&app, "/").await;
        assert_eq!(s, StatusCode::OK);
        assert!(b.contains("19.5 °C") && b.contains("<title>Rooms · haus</title>"));
        let (s, b) = get(&app, "/partials/overview").await;
        assert_eq!(s, StatusCode::OK);
        assert!(b.starts_with(r#"<div id="overview""#));
        let (s, b) = get(&app, &format!("/sensor/{id}?range=7d")).await;
        assert_eq!(s, StatusCode::OK);
        assert!(b.contains(r#"range=7d" class="on""#));
        assert_eq!(get(&app, "/sensor/999").await.0, StatusCode::NOT_FOUND);
        assert_eq!(get(&app, "/status").await.0, StatusCode::OK);
        let (s, b) = get(&app, "/api/warnings").await;
        assert_eq!(s, StatusCode::OK);
        // 19.5 °C in Kitchen: no climate warning; the source is not connected.
        assert!(b.starts_with('[') && b.contains("\"source_down\""), "{b}");
        assert_eq!(get(&app, "/static/haus.css").await.0, StatusCode::OK);
        assert_eq!(
            get(&app, "/static/htmx-2.0.4.min.js").await.0,
            StatusCode::OK
        );
        assert_eq!(
            get(&app, "/static/../Cargo.toml").await.0,
            StatusCode::NOT_FOUND
        );
    }

    // rivet: verifies REQ-STORE-001
    #[tokio::test(flavor = "current_thread")]
    async fn the_runtime_stays_responsive_while_a_long_trend_is_read() {
        let _serial = crate::state::tests::serial().await;
        let state = test_state("dash-responsive", AppConfig::default());
        let store = state.ingest.store().clone();
        let id = store.ensure_series(&sample()[3].meta).unwrap();
        // 90 days at one point a minute: about 130 000 points.
        let now = chrono::Utc::now().timestamp_millis();
        let pts: Vec<Point> = (0..130_000)
            .map(|i| Point {
                series: id,
                ts_ms: now - i * MIN,
                value: (i % 50) as f64,
            })
            .collect();
        for c in pts.chunks(50_000) {
            store.write(c).unwrap();
        }
        let app = app(state);

        let (resp, ticks, took) =
            crate::api::tests::ticks_during(get(&app, &format!("/sensor/{id}?range=90d"))).await;
        let (status, body) = resp;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(r#"class="line""#));
        // The read must not run on the runtime: the 5 ms ticker keeps ticking.
        crate::api::tests::assert_runtime_was_free(ticks, took, "/sensor 90d");
    }
}
