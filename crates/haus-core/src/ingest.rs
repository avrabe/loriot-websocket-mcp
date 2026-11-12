//! Live ingestion into the [`Store`]: turn a source's message into series
//! points and write them, creating series on first sight.
//!
//! Imports and live sources must store the same value for the same reading,
//! or a delta import over live data would report every point as changed.
//! LoRaWAN readings arrive here as `f32`; [`f32_decimal`] stores the decimal
//! the device reported (`18.88`), the same `f64` the import parses from JSON.

use crate::import::LORIOT_FIELDS;
use crate::sensors::SensorReading;
use crate::store::{
    Point, SeriesId, SeriesKey, SeriesMeta, Source, Store, StoreResult, WriteStats,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One value for one series at one time, with the series' metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub meta: SeriesMeta,
    pub ts_ms: i64,
    pub value: f64,
}

/// The shortest decimal that round-trips `v`, as `f64`: `18.88f32` becomes
/// `18.88`, not `18.879999160766602`.
pub fn f32_decimal(v: f32) -> f64 {
    format!("{v}").parse().unwrap_or(v as f64)
}

/// The series points of one decoded LoRaWAN reading. `name` is the display
/// name (from `devices.json`, else the EUI).
pub fn loriot_items(r: &SensorReading, name: &str) -> Vec<Item> {
    let values: [Option<f64>; 6] = [
        r.parsed_temperature.map(f32_decimal),
        r.parsed_humidity.map(f32_decimal),
        r.parsed_battery_voltage.map(f32_decimal),
        r.parsed_movement_count.map(f64::from),
        r.parsed_activity.map(|a| if a { 1.0 } else { 0.0 }),
        r.parsed_lux.map(f64::from),
    ];
    LORIOT_FIELDS
        .iter()
        .zip(values)
        .filter_map(|((_, field, quantity, unit), v)| {
            v.map(|value| Item {
                meta: SeriesMeta {
                    key: SeriesKey::new(Source::Loriot, r.eui.clone(), *field),
                    quantity: *quantity,
                    unit: unit.to_string(),
                    name: name.to_string(),
                    room: None,
                },
                ts_ms: r.ts as i64,
                value,
            })
        })
        .collect()
}

/// Writes items to the store, remembering series ids so a known series costs
/// no extra transaction. Blocking; call it from a blocking thread.
pub struct Ingestor {
    store: Arc<Store>,
    known: Mutex<HashMap<SeriesKey, (SeriesId, SeriesMeta)>>,
}

impl Ingestor {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            known: Mutex::new(HashMap::new()),
        }
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// Write `items` in one transaction. A series whose metadata changed (a
    /// renamed sensor) has its metadata updated; its id stays.
    pub fn ingest(&self, items: &[Item]) -> StoreResult<WriteStats> {
        let mut points = Vec::with_capacity(items.len());
        {
            let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
            for it in items {
                let id = match known.get(&it.meta.key) {
                    Some((id, meta)) if *meta == it.meta => *id,
                    _ => {
                        let id = self.store.ensure_series(&it.meta)?;
                        known.insert(it.meta.key.clone(), (id, it.meta.clone()));
                        id
                    }
                };
                points.push(Point {
                    series: id,
                    ts_ms: it.ts_ms,
                    value: it.value,
                });
            }
        }
        self.store.write(&points)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::import_loriot;

    #[test]
    fn f32_decimal_keeps_the_reported_decimal() {
        for s in ["18.88", "64.7", "3.03", "-2.5", "0", "19.96", "3.141"] {
            let f: f32 = s.parse().unwrap();
            assert_eq!(f32_decimal(f), s.parse::<f64>().unwrap(), "{s}");
        }
    }

    // rivet: partially-verifies REQ-IMPORT-001
    #[test]
    fn live_and_imported_readings_store_the_same_values() {
        let dir = std::env::temp_dir().join(format!(
            "haus-ingest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let line = r#"{"cmd":"rx","eui":"00000000000000A1","data":"","ts":1753863966004,"name":null,"parsed_temperature":18.88,"parsed_humidity":64.7,"parsed_battery_voltage":3.03,"parsed_battery_status":"Good","parsed_activity":true,"parsed_movement_count":297,"parsed_lux":412}"#;
        let file = dir.join("h.jsonl");
        std::fs::write(&file, format!("{line}\n")).unwrap();

        // Live first, then the same reading imported: nothing may change.
        let store = Arc::new(Store::open(dir.join("s.redb")).unwrap());
        let reading: SensorReading = serde_json::from_str(line).unwrap();
        let items = loriot_items(&reading, "00000000000000A1");
        assert_eq!(items.len(), 6);
        let ing = Ingestor::new(store.clone());
        assert_eq!(ing.ingest(&items).unwrap().inserted, 6);
        // Ingesting again uses the cached ids and changes nothing.
        assert_eq!(ing.ingest(&items).unwrap().unchanged, 6);

        let r = import_loriot(&store, &file, &HashMap::new()).unwrap();
        assert!(r.ok(), "{:?}", r.problems);
        assert_eq!(r.total_inserted(), 0);
        assert_eq!(r.series.iter().map(|s| s.unchanged).sum::<u64>(), 6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_renamed_sensor_keeps_its_series() {
        let path = std::env::temp_dir().join(format!(
            "haus-ingest-rename-{}-{}.redb",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Arc::new(Store::open(&path).unwrap());
        let ing = Ingestor::new(store.clone());
        let mut r: SensorReading = serde_json::from_str(
            r#"{"cmd":"rx","eui":"E1","data":"","ts":1,"name":null,"parsed_temperature":20.0}"#,
        )
        .unwrap();
        ing.ingest(&loriot_items(&r, "Old")).unwrap();
        r.ts = 2;
        ing.ingest(&loriot_items(&r, "New")).unwrap();
        let series = store.series().unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].1.name, "New");
        assert_eq!(store.count(series[0].0, 0, 10).unwrap(), 2);
        let _ = std::fs::remove_file(&path);
    }
}
