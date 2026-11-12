//! `haus compare stats4lox`: does haus hold what a Stats4Lox logger recorded?
//! The evidence before such a logger is switched off (REQ-RETIRE-001, SC-002).
//!
//! haus receives every change from the Miniserver; Stats4Lox samples on an
//! interval. So the comparison is by value, not by count: for each Stats4Lox
//! sample since `since`, the value haus had standing at that time (the last
//! point at or before it, within the source's hold) must equal the sample,
//! allowing a clock skew between the logger's poll and the event.
//!
//! Run it *before* importing the logger's final delta: once the delta is
//! imported, the store holds the logger's own points and every sample would
//! trivially match.

use crate::chart::hold_ms;
use crate::import::{read_stats4lox, ImportError, ImportReport};
use crate::store::{Source, Store};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct CompareOptions {
    /// Only samples at or after this time (ms): when haus went live.
    pub since_ms: i64,
    /// Largest difference still counted as equal.
    pub tolerance: f64,
    /// Accept the value haus had up to this long before or after the sample.
    pub skew_ms: i64,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SeriesComparison {
    pub measurement: String,
    pub source_id: String,
    pub field: String,
    pub name: String,
    pub samples: u64,
    pub matched: u64,
    pub mismatched: u64,
    /// Samples for which haus had no value at all.
    pub no_value: u64,
    /// The first disagreement, for a human to look at.
    pub first_problem: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CompareReport {
    pub series: Vec<SeriesComparison>,
    /// Series of the export with no sample since `since`: nothing to check.
    pub not_compared: Vec<String>,
    /// Measurements the reader leaves out (telegraf internals, no uuid).
    pub skipped: Vec<String>,
    pub problems: Vec<String>,
}

impl CompareReport {
    /// Every sample matched, nothing was missing, and something was compared.
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
            && !self.series.is_empty()
            && self
                .series
                .iter()
                .all(|s| s.mismatched == 0 && s.no_value == 0 && s.samples > 0)
    }

    pub fn total_samples(&self) -> u64 {
        self.series.iter().map(|s| s.samples).sum()
    }
}

/// The value standing at `t` in `points` (sorted): the last one at or before
/// `t`, if it is at most `hold` old.
fn standing(points: &[(i64, f64)], t: i64, hold: i64) -> Option<f64> {
    let i = points.partition_point(|(ts, _)| *ts <= t);
    (i > 0)
        .then(|| points[i - 1])
        .filter(|(ts, _)| t - ts <= hold)
        .map(|(_, v)| v)
}

/// Compare a Stats4Lox export with the store.
pub fn compare_stats4lox(
    store: &Store,
    dir: &std::path::Path,
    opts: CompareOptions,
) -> Result<CompareReport, ImportError> {
    let mut report = CompareReport::default();
    let mut parse = ImportReport::default();
    let hold = hold_ms(Source::Loxone);
    read_stats4lox(dir, &mut parse, |measurement, series, _| {
        for (key, ps) in series {
            let samples: Vec<(i64, f64)> = ps
                .points
                .range(opts.since_ms..)
                .map(|(&t, &v)| (t, v))
                .collect();
            if samples.is_empty() {
                report.not_compared.push(format!(
                    "{measurement} ({}/{}): no sample since the given time",
                    key.source_id, key.field
                ));
                continue;
            }
            let mut c = SeriesComparison {
                measurement: measurement.to_string(),
                source_id: key.source_id.clone(),
                field: key.field.clone(),
                name: ps.meta.name.clone(),
                samples: samples.len() as u64,
                ..Default::default()
            };
            let Some(id) = store.series_id(&key)? else {
                report.problems.push(format!(
                    "{measurement} ({}/{}): not in the store",
                    key.source_id, key.field
                ));
                c.no_value = c.samples;
                report.series.push(c);
                continue;
            };
            let first = samples[0].0;
            let last = samples[samples.len() - 1].0;
            // Only what haus received live: imported points would match themselves.
            let have = store.range_live(id, first - opts.skew_ms - hold, last + opts.skew_ms)?;
            for (t, v) in samples {
                // The value at t - skew, and every value haus took up to t + skew.
                let mut candidates: Vec<f64> = standing(&have, t - opts.skew_ms, hold)
                    .into_iter()
                    .collect();
                let lo = have.partition_point(|(ts, _)| *ts <= t - opts.skew_ms);
                let hi = have.partition_point(|(ts, _)| *ts <= t + opts.skew_ms);
                candidates.extend(have[lo..hi].iter().map(|(_, x)| *x));
                if candidates.is_empty() {
                    c.no_value += 1;
                    c.first_problem
                        .get_or_insert_with(|| format!("{t}: Stats4Lox {v}, haus has no value"));
                } else if candidates.iter().any(|x| (x - v).abs() <= opts.tolerance) {
                    c.matched += 1;
                } else {
                    c.mismatched += 1;
                    c.first_problem
                        .get_or_insert_with(|| format!("{t}: Stats4Lox {v}, haus {candidates:?}"));
                }
            }
            report.series.push(c);
        }
        Ok(())
    })?;
    report.skipped = parse.skipped;
    report.problems.extend(parse.problems);
    if report.series.is_empty() {
        report
            .problems
            .push("no Stats4Lox samples since the given time: nothing was compared".into());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Point, Quantity, SeriesKey, SeriesMeta};
    use serde_json::json;
    use std::io::Write;

    const MIN: i64 = 60_000;
    const T0: i64 = 1_800_000_000_000;

    fn export(dir: &std::path::Path, uuid: &str, samples: &[(i64, f64)]) {
        let sub = "Temperatur_Test";
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        let f = std::fs::File::create(dir.join(sub).join("2027-01.jsonl.gz")).unwrap();
        let mut gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
        for (t, v) in samples {
            writeln!(
                gz,
                "{}",
                json!({"time": t * 1_000_000, "uuid": uuid, "name": "Test", "room": "Room A",
                       "category": "Temperatur", "Default": v})
            )
            .unwrap();
        }
        gz.finish().unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            json!({"measurements": {"Temperatur Test": {"dir": sub, "fields": ["Default"],
                "influx_counts": {"Default": samples.len()},
                "tags": ["uuid", "name", "room", "category"]}}})
            .to_string(),
        )
        .unwrap();
    }

    fn tmp() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "haus-compare-{}-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn opts() -> CompareOptions {
        CompareOptions {
            since_ms: T0,
            tolerance: 0.01,
            skew_ms: MIN,
        }
    }

    /// A store with live (event) points for uuid "u".
    fn store_with(d: &std::path::Path, live: &[(i64, f64)]) -> Store {
        let store = Store::open(d.join("s.redb")).unwrap();
        let id = store
            .ensure_series(&SeriesMeta {
                key: SeriesKey::new(Source::Loxone, "u", "Default"),
                quantity: Quantity::Temperature,
                unit: "°C".into(),
                name: "Test".into(),
                room: None,
            })
            .unwrap();
        let pts: Vec<Point> = live
            .iter()
            .map(|&(ts_ms, value)| Point {
                series: id,
                ts_ms,
                value,
            })
            .collect();
        store.write_live(&pts).unwrap();
        store
    }

    // rivet: verifies REQ-RETIRE-001
    #[test]
    fn imported_points_are_not_evidence() {
        // The store holds the logger's own points (an import), nothing live.
        let d = tmp();
        let store = Store::open(d.join("s.redb")).unwrap();
        let id = store
            .ensure_series(&SeriesMeta {
                key: SeriesKey::new(Source::Loxone, "u", "Default"),
                quantity: Quantity::Temperature,
                unit: "°C".into(),
                name: "Test".into(),
                room: None,
            })
            .unwrap();
        let samples = [(T0, 20.0), (T0 + 5 * MIN, 20.0)];
        let pts: Vec<Point> = samples
            .iter()
            .map(|&(ts_ms, value)| Point {
                series: id,
                ts_ms,
                value,
            })
            .collect();
        store.write(&pts).unwrap();
        export(&d, "u", &samples);
        let r = compare_stats4lox(&store, &d, opts()).unwrap();
        assert!(!r.ok(), "imported points matched themselves: {r:?}");
        assert_eq!(r.series[0].no_value, 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    // rivet: verifies REQ-RETIRE-001
    #[test]
    fn samples_agree_with_the_value_standing_in_haus() {
        let d = tmp();
        // haus got changes at T0-1h (20.0), T0+12 min (20.5), T0+31 min (21.0).
        let store = store_with(
            &d,
            &[
                (T0 - 60 * MIN, 20.0),
                (T0 + 12 * MIN, 20.5),
                (T0 + 31 * MIN, 21.0),
            ],
        );
        // Stats4Lox every 5 min; one sample before `since` is ignored. The
        // sample at +30 min already shows 21.0: within the 1 min skew of the
        // event at +31 min.
        let samples: Vec<(i64, f64)> = vec![
            (T0 - 5 * MIN, 99.0),
            (T0, 20.0),
            (T0 + 5 * MIN, 20.0),
            (T0 + 10 * MIN, 20.0),
            (T0 + 15 * MIN, 20.5),
            (T0 + 30 * MIN, 21.0),
            (T0 + 35 * MIN, 21.0),
        ];
        export(&d, "u", &samples);
        let r = compare_stats4lox(&store, &d, opts()).unwrap();
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.total_samples(), 6);
        assert_eq!(r.series[0].matched, 6);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_disagreement_or_a_missing_value_fails() {
        // Different value.
        let d = tmp();
        let store = store_with(&d, &[(T0 - MIN, 20.0)]);
        export(&d, "u", &[(T0, 20.0), (T0 + 5 * MIN, 22.0)]);
        let r = compare_stats4lox(&store, &d, opts()).unwrap();
        assert!(!r.ok());
        assert_eq!((r.series[0].matched, r.series[0].mismatched), (1, 1));
        assert!(r.series[0].first_problem.as_deref().unwrap().contains("22"));
        let _ = std::fs::remove_dir_all(&d);

        // haus had nothing at that time (older than the hold).
        let d = tmp();
        let store = store_with(&d, &[(T0 - 2 * 86_400_000, 20.0)]);
        export(&d, "u", &[(T0, 20.0)]);
        let r = compare_stats4lox(&store, &d, opts()).unwrap();
        assert!(!r.ok());
        assert_eq!(r.series[0].no_value, 1);
        let _ = std::fs::remove_dir_all(&d);

        // A series haus does not know.
        let d = tmp();
        let store = store_with(&d, &[(T0, 20.0)]);
        export(&d, "other", &[(T0, 20.0)]);
        let r = compare_stats4lox(&store, &d, opts()).unwrap();
        assert!(!r.ok());
        assert!(r.problems.iter().any(|p| p.contains("not in the store")));
        let _ = std::fs::remove_dir_all(&d);

        // Nothing since `since`: no evidence, not a pass.
        let d = tmp();
        let store = store_with(&d, &[(T0, 20.0)]);
        export(&d, "u", &[(T0 - MIN, 20.0)]);
        let r = compare_stats4lox(&store, &d, opts()).unwrap();
        assert!(!r.ok());
        assert!(r
            .problems
            .iter()
            .any(|p| p.contains("nothing was compared")));
        assert_eq!(r.not_compared.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn skew_and_tolerance_are_bounded() {
        let d = tmp();
        // The change came 3 min after the sample: outside a 1 min skew.
        let store = store_with(&d, &[(T0 - MIN, 20.0), (T0 + 3 * MIN, 21.0)]);
        export(&d, "u", &[(T0, 21.0)]);
        assert!(!compare_stats4lox(&store, &d, opts()).unwrap().ok());
        let wide = CompareOptions {
            skew_ms: 5 * MIN,
            ..opts()
        };
        assert!(compare_stats4lox(&store, &d, wide).unwrap().ok());
        let _ = std::fs::remove_dir_all(&d);

        let d = tmp();
        let store = store_with(&d, &[(T0 - MIN, 20.004)]);
        export(&d, "u", &[(T0, 20.0)]);
        assert!(compare_stats4lox(&store, &d, opts()).unwrap().ok());
        let strict = CompareOptions {
            tolerance: 0.001,
            ..opts()
        };
        assert!(!compare_stats4lox(&store, &d, strict).unwrap().ok());
        let _ = std::fs::remove_dir_all(&d);
    }
}
