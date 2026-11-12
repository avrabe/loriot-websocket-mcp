//! Imports into the [`Store`]: the LoRaWAN history file and a Stats4Lox
//! InfluxDB export (REQ-IMPORT-001, REQ-IMPORT-002, DD-006).
//!
//! An import proves it reproduced its source (SC-001). It parses every point,
//! writes them, then reads each series back and checks that every parsed
//! point is stored with the same value. A Stats4Lox import also checks the
//! parsed count per measurement and field against the count InfluxDB itself
//! reported in the export's manifest. Anything that does not add up is an
//! [`ImportReport::problems`] entry, and the caller exits non-zero.
//!
//! Points already stored with the same value are not stored twice, so an
//! import can be re-run, and a later delta export can be imported over an
//! earlier full one.

use crate::store::{Point, Quantity, SeriesId, SeriesKey, SeriesMeta, Source, Store, StoreError};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{path}: {msg}")]
    Format { path: String, msg: String },
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> ImportError + '_ {
    move |source| ImportError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// The outcome of one series.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SeriesReport {
    pub source_id: String,
    pub field: String,
    pub name: String,
    /// Distinct points parsed from the source.
    pub parsed: u64,
    pub inserted: u64,
    pub unchanged: u64,
    /// Stored with a different value before this import: a problem.
    pub changed: u64,
    /// Parsed points not found back in the store with the same value.
    pub missing_after_write: u64,
}

/// What an import did and what does not add up.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ImportReport {
    pub series: Vec<SeriesReport>,
    /// Measurements or files left out on purpose, with the reason.
    pub skipped: Vec<String>,
    /// Every way the import failed to reproduce its source. Empty = success.
    pub problems: Vec<String>,
}

impl ImportReport {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }

    pub fn total_parsed(&self) -> u64 {
        self.series.iter().map(|s| s.parsed).sum()
    }

    pub fn total_inserted(&self) -> u64 {
        self.series.iter().map(|s| s.inserted).sum()
    }
}

/// Points of one series collected from the source, by time.
struct Pending {
    meta: SeriesMeta,
    points: BTreeMap<i64, f64>,
}

/// Turn a JSON number or bool into a value; `None` for null.
fn number(v: &Value) -> Result<Option<f64>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(if *b { 1.0 } else { 0.0 })),
        Value::Number(n) => n
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("number {n} is not representable")),
        other => Err(format!("expected a number, got {other}")),
    }
}

/// Add a point, failing on a second, different value at the same time.
fn add(p: &mut Pending, ts: i64, value: f64, problems: &mut Vec<String>) {
    match p.points.get(&ts) {
        Some(old) if old.to_bits() != value.to_bits() && !(old.is_nan() && value.is_nan()) => {
            problems.push(format!(
                "{}/{}: two different values at ts {ts} ms: {old} and {value}",
                p.meta.key.source_id, p.meta.key.field
            ));
        }
        _ => {
            p.points.insert(ts, value);
        }
    }
}

/// Write every pending series, then read it back and compare.
fn write_and_check(
    store: &Store,
    pending: BTreeMap<SeriesKey, Pending>,
    report: &mut ImportReport,
) -> Result<(), ImportError> {
    const BATCH: usize = 50_000;
    for (_, p) in pending {
        let id: SeriesId = store.ensure_series(&p.meta)?;
        let mut sr = SeriesReport {
            source_id: p.meta.key.source_id.clone(),
            field: p.meta.key.field.clone(),
            name: p.meta.name.clone(),
            parsed: p.points.len() as u64,
            ..Default::default()
        };
        let all: Vec<Point> = p
            .points
            .iter()
            .map(|(&ts_ms, &value)| Point {
                series: id,
                ts_ms,
                value,
            })
            .collect();
        for chunk in all.chunks(BATCH) {
            let st = store.write(chunk)?;
            sr.inserted += st.inserted;
            sr.unchanged += st.unchanged;
            sr.changed += st.changed;
        }
        // Read back: every parsed point must be there with its value.
        if let (Some(&first), Some(&last)) = (p.points.keys().next(), p.points.keys().last()) {
            let stored: HashMap<i64, f64> = store.range(id, first, last)?.into_iter().collect();
            sr.missing_after_write = p
                .points
                .iter()
                .filter(|(ts, v)| match stored.get(ts) {
                    Some(s) => s.to_bits() != v.to_bits() && !(s.is_nan() && v.is_nan()),
                    None => true,
                })
                .count() as u64;
        }
        let what = format!("{}/{} ({})", sr.source_id, sr.field, sr.name);
        if sr.changed > 0 {
            report.problems.push(format!(
                "{what}: {} point(s) were stored with a different value and are now overwritten",
                sr.changed
            ));
        }
        if sr.missing_after_write > 0 {
            report.problems.push(format!(
                "{what}: {} parsed point(s) not found back in the store",
                sr.missing_after_write
            ));
        }
        report.series.push(sr);
    }
    Ok(())
}

/// The LoRaWAN fields the import keeps: (JSON key, series field, quantity, unit).
pub(crate) const LORIOT_FIELDS: [(&str, &str, Quantity, &str); 6] = [
    (
        "parsed_temperature",
        "temperature",
        Quantity::Temperature,
        "°C",
    ),
    ("parsed_humidity", "humidity", Quantity::Humidity, "%"),
    (
        "parsed_battery_voltage",
        "battery_voltage",
        Quantity::BatteryVoltage,
        "V",
    ),
    (
        "parsed_movement_count",
        "movement_count",
        Quantity::Motion,
        "",
    ),
    ("parsed_activity", "activity", Quantity::Activity, ""),
    ("parsed_lux", "lux", Quantity::Other, "lx"),
];

/// Import a LoRaWAN `sensor_history.jsonl` (REQ-IMPORT-001).
///
/// Every non-empty line must be a JSON object with a string `eui` and an
/// integer `ts` (ms); the import fails on any other line. `names` maps an
/// EUI to a display name (`devices.json`).
pub fn import_loriot(
    store: &Store,
    path: &Path,
    names: &HashMap<String, String>,
) -> Result<ImportReport, ImportError> {
    let file = std::fs::File::open(path).map_err(io_err(path))?;
    let mut report = ImportReport::default();
    let mut pending: BTreeMap<SeriesKey, Pending> = BTreeMap::new();
    for (no, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line.map_err(io_err(path))?;
        if line.trim().is_empty() {
            continue;
        }
        let at = format!("{}:{}", path.display(), no + 1);
        let obj: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                report.problems.push(format!("{at}: not JSON: {e}"));
                continue;
            }
        };
        let (Some(eui), Some(ts)) = (obj["eui"].as_str(), obj["ts"].as_i64()) else {
            report
                .problems
                .push(format!("{at}: needs a string `eui` and an integer `ts`"));
            continue;
        };
        for (json_key, field, quantity, unit) in LORIOT_FIELDS {
            let value = match number(&obj[json_key]) {
                Ok(Some(v)) => v,
                Ok(None) => continue,
                Err(e) => {
                    report.problems.push(format!("{at}: {json_key}: {e}"));
                    continue;
                }
            };
            let key = SeriesKey::new(Source::Loriot, eui, field);
            let p = pending.entry(key.clone()).or_insert_with(|| Pending {
                meta: SeriesMeta {
                    key,
                    quantity,
                    unit: unit.to_string(),
                    name: names
                        .get(eui)
                        .cloned()
                        .or_else(|| obj["name"].as_str().map(str::to_string))
                        .unwrap_or_else(|| eui.to_string()),
                    room: None,
                },
                points: BTreeMap::new(),
            });
            add(p, ts, value, &mut report.problems);
        }
    }
    if pending.is_empty() && report.problems.is_empty() {
        report
            .problems
            .push(format!("{}: no readings found", path.display()));
    }
    write_and_check(store, pending, &mut report)?;
    Ok(report)
}

/// Quantity and unit of a Loxone series, from the Stats4Lox `category` tag
/// and the measurement name. Unknown categories become [`Quantity::Other`].
pub fn loxone_quantity(category: &str, measurement: &str) -> (Quantity, &'static str) {
    let c = category.to_lowercase();
    let m = measurement.to_lowercase();
    if c.starts_with("temperatur") || m.starts_with("temperatur") || c.starts_with("temperature") {
        (Quantity::Temperature, "°C")
    } else if c.starts_with("luftfeucht") || m.starts_with("luftfeucht") || c.contains("humidity") {
        (Quantity::Humidity, "%")
    } else if m.starts_with("stellantrieb") || c.contains("valve") {
        (Quantity::ValvePosition, "%")
    } else if c.contains("heiz")
        || m.contains("raumregelung")
        || m.starts_with("pumpe")
        || c.contains("heating")
    {
        (Quantity::Heating, "")
    } else {
        (Quantity::Other, "")
    }
}

/// Import a Stats4Lox export directory (REQ-IMPORT-002): `manifest.json` and
/// `<measurement dir>/<YYYY-MM>.jsonl.gz`, one InfluxDB point per line with
/// `time` in ns, the measurement's fields and its tags.
///
/// Series are keyed by the point's `uuid` tag and field. `internal_*`
/// (telegraf's own metrics) and measurements whose points carry no `uuid`
/// are skipped and named in [`ImportReport::skipped`].
pub fn import_stats4lox(store: &Store, dir: &Path) -> Result<ImportReport, ImportError> {
    let manifest_path = dir.join("manifest.json");
    let fmt = |msg: String| ImportError::Format {
        path: manifest_path.display().to_string(),
        msg,
    };
    let manifest: Value = serde_json::from_reader(
        std::fs::File::open(&manifest_path).map_err(io_err(&manifest_path))?,
    )
    .map_err(|e| fmt(e.to_string()))?;
    let measurements = manifest["measurements"]
        .as_object()
        .ok_or_else(|| fmt("no `measurements` object".into()))?;
    if measurements.is_empty() {
        return Err(fmt("`measurements` is empty".into()));
    }

    let mut report = ImportReport::default();
    for (name, m) in measurements {
        if name.starts_with("internal_") {
            report
                .skipped
                .push(format!("{name}: telegraf internal metrics"));
            continue;
        }
        let sub = m["dir"]
            .as_str()
            .ok_or_else(|| fmt(format!("{name}: no `dir`")))?;
        let raw_fields = m["fields"]
            .as_array()
            .ok_or_else(|| fmt(format!("{name}: no `fields`")))?;
        let fields: Vec<String> = raw_fields
            .iter()
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect();
        if fields.is_empty() || fields.len() != raw_fields.len() {
            report.problems.push(format!(
                "{name}: `fields` must be a non-empty list of names, got {}",
                m["fields"]
            ));
            continue;
        }
        // With the tag list known, any other key in a point is a field the
        // manifest does not count: the import would not check it.
        let tags: Option<Vec<&str>> = m["tags"]
            .as_array()
            .map(|t| t.iter().filter_map(Value::as_str).collect());
        let counts = m["influx_counts"].as_object();

        let mdir = dir.join(sub);
        let mut files: Vec<_> = std::fs::read_dir(&mdir)
            .map_err(io_err(&mdir))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().ends_with(".jsonl.gz"))
            .collect();
        files.sort();

        let mut pending: BTreeMap<SeriesKey, Pending> = BTreeMap::new();
        let (mut with_uuid, mut without_uuid) = (0u64, 0u64);
        for file in &files {
            let gz = flate2::read::GzDecoder::new(std::fs::File::open(file).map_err(io_err(file))?);
            for (no, line) in std::io::BufReader::new(gz).lines().enumerate() {
                let line = line.map_err(io_err(file))?;
                if line.trim().is_empty() {
                    continue;
                }
                let at = format!("{}:{}", file.display(), no + 1);
                let p: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        report.problems.push(format!("{at}: not JSON: {e}"));
                        continue;
                    }
                };
                let Some(time_ns) = p["time"].as_i64() else {
                    report.problems.push(format!("{at}: no integer `time`"));
                    continue;
                };
                if time_ns % 1_000_000 != 0 {
                    report.problems.push(format!(
                        "{at}: time {time_ns} ns is not a whole millisecond"
                    ));
                    continue;
                }
                let ts = time_ns / 1_000_000;
                if let (Some(tags), Some(obj)) = (&tags, p.as_object()) {
                    if let Some(k) = obj.keys().find(|k| {
                        k.as_str() != "time"
                            && !tags.contains(&k.as_str())
                            && !fields.iter().any(|f| f == *k)
                    }) {
                        report.problems.push(format!(
                            "{at}: `{k}` is neither a field nor a tag in the manifest"
                        ));
                        continue;
                    }
                }
                let Some(uuid) = p["uuid"].as_str().filter(|u| !u.is_empty()) else {
                    without_uuid += 1;
                    continue;
                };
                with_uuid += 1;
                let (quantity, unit) = loxone_quantity(p["category"].as_str().unwrap_or(""), name);
                for field in &fields {
                    let value = match number(&p[field.as_str()]) {
                        Ok(Some(v)) => v,
                        Ok(None) => continue,
                        Err(e) => {
                            report.problems.push(format!("{at}: {field}: {e}"));
                            continue;
                        }
                    };
                    let key = SeriesKey::new(Source::Loxone, uuid, field.clone());
                    let pe = pending.entry(key.clone()).or_insert_with(|| Pending {
                        meta: SeriesMeta {
                            key,
                            quantity,
                            unit: unit.to_string(),
                            name: p["name"].as_str().unwrap_or(name).to_string(),
                            room: p["room"].as_str().map(str::to_string),
                        },
                        points: BTreeMap::new(),
                    });
                    add(pe, ts, value, &mut report.problems);
                }
            }
        }

        if with_uuid == 0 && without_uuid > 0 {
            report.skipped.push(format!(
                "{name}: {without_uuid} point(s) without a uuid tag"
            ));
            continue;
        }
        if without_uuid > 0 {
            report.problems.push(format!(
                "{name}: {without_uuid} point(s) without a uuid next to {with_uuid} with one"
            ));
        }
        // The manifest counts per measurement and field; compare per field
        // across this measurement's series (one uuid each, in practice).
        let mut per_field: HashMap<&str, u64> = HashMap::new();
        for (k, p) in &pending {
            *per_field.entry(k.field.as_str()).or_default() += p.points.len() as u64;
        }
        if let Some(counts) = counts {
            for field in &fields {
                let Some(expected) = counts.get(field).and_then(Value::as_u64) else {
                    report
                        .problems
                        .push(format!("{name}/{field}: no InfluxDB count in the manifest"));
                    continue;
                };
                let parsed = per_field.get(field.as_str()).copied().unwrap_or(0);
                if parsed != expected {
                    report.problems.push(format!(
                        "{name}/{field}: InfluxDB counts {expected} point(s), {parsed} were parsed"
                    ));
                }
            }
        } else {
            report
                .problems
                .push(format!("{name}: no `influx_counts` in the manifest"));
        }
        write_and_check(store, pending, &mut report)?;
    }
    if report.series.is_empty() && report.problems.is_empty() {
        report
            .problems
            .push(format!("{}: no series imported", dir.display()));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::path::PathBuf;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "haus-import-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn store_in(d: &Path) -> Store {
        Store::open(d.join("haus.redb")).unwrap()
    }

    fn values(store: &Store, src: Source, id: &str, field: &str) -> Vec<(i64, f64)> {
        let sid = store
            .series_id(&SeriesKey::new(src, id, field))
            .unwrap()
            .unwrap_or_else(|| panic!("no series {id}/{field}"));
        store.range(sid, i64::MIN, i64::MAX).unwrap()
    }

    fn reading(eui: &str, ts: i64, t: f64) -> String {
        json!({"cmd":"rx","eui":eui,"data":"","ts":ts,"name":null,
               "parsed_temperature":t,"parsed_humidity":60.5,"parsed_battery_voltage":3.1,
               "parsed_battery_status":"Good","parsed_activity":false,"parsed_movement_count":7})
        .to_string()
    }

    fn write_lines(path: &Path, lines: &[String]) {
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    // rivet: verifies REQ-IMPORT-001
    #[test]
    fn loriot_import_reproduces_the_file_and_reruns_cleanly() {
        let d = tmpdir("loriot-ok");
        let file = d.join("sensor_history.jsonl");
        write_lines(
            &file,
            &[
                reading("00000000000000A1", 1_000, 18.88),
                String::new(),
                reading("00000000000000A1", 2_000, 19.0),
                reading("00000000000000B2", 1_500, -2.5),
            ],
        );
        let names = HashMap::from([("00000000000000A1".to_string(), "Cellar".to_string())]);
        let store = store_in(&d);
        let r = import_loriot(&store, &file, &names).unwrap();
        assert!(r.ok(), "{:?}", r.problems);
        // 2 devices × 5 fields (lux absent everywhere).
        assert_eq!(r.series.len(), 10);
        assert_eq!(r.total_parsed(), 15);
        assert_eq!(r.total_inserted(), 15);
        assert_eq!(
            values(&store, Source::Loriot, "00000000000000A1", "temperature"),
            vec![(1_000, 18.88), (2_000, 19.0)]
        );
        assert_eq!(
            values(&store, Source::Loriot, "00000000000000B2", "activity"),
            vec![(1_500, 0.0)]
        );
        let metas = store.series().unwrap();
        let cellar = metas
            .iter()
            .find(|(_, m)| m.key.source_id == "00000000000000A1" && m.key.field == "humidity")
            .unwrap();
        assert_eq!(cellar.1.name, "Cellar");
        assert_eq!(cellar.1.quantity, Quantity::Humidity);

        // Re-run: nothing new, nothing changed, still ok.
        let again = import_loriot(&store, &file, &names).unwrap();
        assert!(again.ok(), "{:?}", again.problems);
        assert_eq!(again.total_inserted(), 0);
        assert_eq!(again.series.iter().map(|s| s.unchanged).sum::<u64>(), 15);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn loriot_import_fails_on_anything_it_cannot_reproduce() {
        // Each case: lines, and a fragment the problem must mention.
        let cases: Vec<(Vec<String>, &str)> = vec![
            (
                vec![reading("00000000000000A1", 1, 1.0), "{not json".into()],
                "not JSON",
            ),
            (vec![r#"{"eui":"00000000000000A1"}"#.into()], "integer `ts`"),
            (
                vec![
                    reading("00000000000000A1", 1, 1.0),
                    reading("00000000000000A1", 1, 2.0),
                ],
                "two different values",
            ),
            (
                vec![r#"{"eui":"00000000000000A1","ts":1,"parsed_temperature":"warm"}"#.into()],
                "expected a number",
            ),
            (vec![], "no readings found"),
        ];
        for (lines, want) in cases {
            let d = tmpdir("loriot-bad");
            let file = d.join("h.jsonl");
            write_lines(&file, &lines);
            let r = import_loriot(&store_in(&d), &file, &HashMap::new()).unwrap();
            assert!(!r.ok(), "{lines:?} should fail");
            assert!(
                r.problems.iter().any(|p| p.contains(want)),
                "{want:?} not in {:?}",
                r.problems
            );
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn a_value_already_stored_differently_fails_the_import() {
        let d = tmpdir("loriot-changed");
        let store = store_in(&d);
        let file = d.join("h.jsonl");
        write_lines(&file, &[reading("00000000000000A1", 1, 20.0)]);
        assert!(import_loriot(&store, &file, &HashMap::new()).unwrap().ok());
        write_lines(&file, &[reading("00000000000000A1", 1, 21.0)]);
        let r = import_loriot(&store, &file, &HashMap::new()).unwrap();
        assert!(!r.ok());
        assert!(r.problems.iter().any(|p| p.contains("different value")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// One measurement of a test export: name, fields, influx_counts, and
    /// (month, points) per file.
    type Measurement<'a> = (&'a str, &'a [&'a str], Value, &'a [(&'a str, Vec<Value>)]);

    /// Write a Stats4Lox export: manifest.json and one gzip file per month.
    fn export(dir: &Path, measurements: &[Measurement]) {
        let mut m = serde_json::Map::new();
        for (name, fields, counts, files) in measurements {
            let sub = name.replace(' ', "_");
            std::fs::create_dir_all(dir.join(&sub)).unwrap();
            for (month, points) in *files {
                let f = std::fs::File::create(dir.join(&sub).join(format!("{month}.jsonl.gz")))
                    .unwrap();
                let mut gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
                for p in points {
                    writeln!(gz, "{p}").unwrap();
                }
                gz.finish().unwrap();
            }
            m.insert(
                name.to_string(),
                json!({"dir": sub, "fields": fields, "influx_counts": counts,
                       "tags": ["category", "name", "room", "type", "uuid"]}),
            );
        }
        std::fs::write(
            dir.join("manifest.json"),
            json!({"measurements": m}).to_string(),
        )
        .unwrap();
    }

    fn point(ms: i64, uuid: Option<&str>, name: &str, category: &str, fields: Value) -> Value {
        let mut p = json!({"time": ms * 1_000_000, "name": name, "room": "Room A",
                           "category": category, "type": "Sensor"});
        if let Some(u) = uuid {
            p["uuid"] = json!(u);
        }
        for (k, v) in fields.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    }

    // rivet: verifies REQ-IMPORT-002
    #[test]
    fn stats4lox_import_matches_influx_counts_and_takes_a_delta() {
        let d = tmpdir("s4l-ok");
        let full = d.join("full");
        let t = "Temperatur Test";
        let rr = "Intelligente Raumregelung";
        export(
            &full,
            &[
                (
                    t,
                    &["Default"],
                    json!({"Default": 3}),
                    &[
                        (
                            "2026-01",
                            vec![
                                point(
                                    1_000,
                                    Some("u-temp"),
                                    t,
                                    "Temperatur",
                                    json!({"Default": 20}),
                                ),
                                point(
                                    2_000,
                                    Some("u-temp"),
                                    t,
                                    "Temperatur",
                                    json!({"Default": 20.5}),
                                ),
                            ],
                        ),
                        (
                            "2026-02",
                            vec![
                                point(
                                    3_000,
                                    Some("u-temp"),
                                    t,
                                    "Temperatur",
                                    json!({"Default": null}),
                                ),
                                point(
                                    4_000,
                                    Some("u-temp"),
                                    t,
                                    "Temperatur",
                                    json!({"Default": -1.25}),
                                ),
                            ],
                        ),
                    ],
                ),
                (
                    rr,
                    &["Default", "HCm"],
                    json!({"Default": 1, "HCm": 2}),
                    &[(
                        "2026-01",
                        vec![
                            point(
                                1_000,
                                Some("u-rr"),
                                rr,
                                "Heizung",
                                json!({"Default": 1, "HCm": 3}),
                            ),
                            point(2_000, Some("u-rr"), rr, "Heizung", json!({"HCm": 4})),
                        ],
                    )],
                ),
                (
                    "internal_agent",
                    &["gather_errors"],
                    json!({"gather_errors": 1}),
                    &[(
                        "2026-01",
                        vec![json!({"time": 1_000_000_000, "gather_errors": 0})],
                    )],
                ),
                (
                    "stats_miniserver",
                    &["cpu"],
                    json!({"cpu": 1}),
                    &[(
                        "2026-01",
                        vec![point(1_000, None, "x", "", json!({"cpu": 12}))],
                    )],
                ),
            ],
        );
        let store = store_in(&d);
        let r = import_stats4lox(&store, &full).unwrap();
        assert!(r.ok(), "{:?}", r.problems);
        assert_eq!(r.skipped.len(), 2, "{:?}", r.skipped);
        assert!(r.skipped.iter().any(|s| s.starts_with("internal_agent")));
        assert!(r.skipped.iter().any(|s| s.starts_with("stats_miniserver")));
        assert_eq!(r.series.len(), 3);
        assert_eq!(
            values(&store, Source::Loxone, "u-temp", "Default"),
            vec![(1_000, 20.0), (2_000, 20.5), (4_000, -1.25)]
        );
        assert_eq!(
            values(&store, Source::Loxone, "u-rr", "HCm"),
            vec![(1_000, 3.0), (2_000, 4.0)]
        );
        let metas = store.series().unwrap();
        let temp = metas
            .iter()
            .find(|(_, m)| m.key.source_id == "u-temp")
            .unwrap();
        assert_eq!(temp.1.quantity, Quantity::Temperature);
        assert_eq!(temp.1.room.as_deref(), Some("Room A"));
        let heat = metas
            .iter()
            .find(|(_, m)| m.key.source_id == "u-rr")
            .unwrap();
        assert_eq!(heat.1.quantity, Quantity::Heating);

        // A later delta export that overlaps the full one.
        let delta = d.join("delta");
        export(
            &delta,
            &[(
                t,
                &["Default"],
                json!({"Default": 2}),
                &[(
                    "2026-02",
                    vec![
                        point(
                            4_000,
                            Some("u-temp"),
                            t,
                            "Temperatur",
                            json!({"Default": -1.25}),
                        ),
                        point(
                            5_000,
                            Some("u-temp"),
                            t,
                            "Temperatur",
                            json!({"Default": 21}),
                        ),
                    ],
                )],
            )],
        );
        let r = import_stats4lox(&store, &delta).unwrap();
        assert!(r.ok(), "{:?}", r.problems);
        assert_eq!(r.total_inserted(), 1);
        assert_eq!(values(&store, Source::Loxone, "u-temp", "Default").len(), 4);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stats4lox_import_fails_when_the_export_does_not_add_up() {
        let t = "Temperatur Test";
        let ok = |ms| point(ms, Some("u"), t, "Temperatur", json!({"Default": 1}));
        let cases: Vec<(Value, Vec<Value>, &str)> = vec![
            // InfluxDB counted more than the files hold.
            (
                json!({"Default": 3}),
                vec![ok(1), ok(2)],
                "InfluxDB counts 3",
            ),
            // Not a whole millisecond.
            (
                json!({"Default": 1}),
                vec![json!({"time": 1_500_000, "uuid": "u", "Default": 1})],
                "whole millisecond",
            ),
            // Some points with a uuid, some without.
            (
                json!({"Default": 2}),
                vec![
                    ok(1),
                    point(2, None, t, "Temperatur", json!({"Default": 1})),
                ],
                "without a uuid next to",
            ),
            // A string where a number belongs.
            (
                json!({"Default": 1}),
                vec![point(
                    1,
                    Some("u"),
                    t,
                    "Temperatur",
                    json!({"Default": "hot"}),
                )],
                "expected a number",
            ),
            // No count for a field.
            (json!({}), vec![ok(1)], "no InfluxDB count"),
            // A key the manifest lists neither as field nor as tag.
            (
                json!({"Default": 1}),
                vec![point(
                    1,
                    Some("u"),
                    t,
                    "Temperatur",
                    json!({"Default": 1, "Extra": 2}),
                )],
                "neither a field nor a tag",
            ),
        ];
        for (counts, points, want) in cases {
            let d = tmpdir("s4l-bad");
            export(&d, &[(t, &["Default"], counts, &[("2026-01", points)])]);
            let r = import_stats4lox(&store_in(&d), &d).unwrap();
            assert!(!r.ok(), "{want}: should fail");
            assert!(
                r.problems.iter().any(|p| p.contains(want)),
                "{want:?} not in {:?}",
                r.problems
            );
            let _ = std::fs::remove_dir_all(&d);
        }
        // A measurement without field names is a problem, not an empty success.
        for fields in [json!([]), json!([1])] {
            let d = tmpdir("s4l-nofields");
            export(
                &d,
                &[(
                    t,
                    &["Default"],
                    json!({"Default": 1}),
                    &[("2026-01", vec![ok(1)])],
                )],
            );
            let mf = d.join("manifest.json");
            let mut v: Value =
                serde_json::from_str(&std::fs::read_to_string(&mf).unwrap()).unwrap();
            v["measurements"][t]["fields"] = fields;
            std::fs::write(&mf, v.to_string()).unwrap();
            let r = import_stats4lox(&store_in(&d), &d).unwrap();
            assert!(
                r.problems.iter().any(|p| p.contains("`fields` must be")),
                "{:?}",
                r.problems
            );
            let _ = std::fs::remove_dir_all(&d);
        }
        // A manifest with nothing in it is an error, not an empty success.
        let d = tmpdir("s4l-empty");
        std::fs::write(d.join("manifest.json"), r#"{"measurements":{}}"#).unwrap();
        assert!(import_stats4lox(&store_in(&d), &d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn loxone_quantities_from_category_and_name() {
        assert_eq!(loxone_quantity("Temperatur", "x").0, Quantity::Temperature);
        assert_eq!(loxone_quantity("", "Luftfeuchte Bad").0, Quantity::Humidity);
        assert_eq!(
            loxone_quantity("Heizung", "Stellantrieb Air").0,
            Quantity::ValvePosition
        );
        assert_eq!(
            loxone_quantity("", "Intelligente Raumregelung").0,
            Quantity::Heating
        );
        assert_eq!(loxone_quantity("", "Pumpe").0, Quantity::Heating);
        assert_eq!(loxone_quantity("Licht", "Dimmer").0, Quantity::Other);
    }
}
