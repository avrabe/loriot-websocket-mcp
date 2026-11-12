//! Write synthetic series into a fresh store and time the operations the
//! dashboard needs. Meant to be run on the target host for REQ-RES-001:
//!
//!   cargo run --release -p haus-core --example store_bench -- <dir> [series] [points_per_series]
//!
//! Defaults: 35 series × 200 000 points at 5-minute spacing (about 1.9 years),
//! about the size of a Stats4Lox history.
use haus_core::store::{Point, Quantity, SeriesKey, SeriesMeta, Source, Store};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .expect("usage: store_bench <dir> [series] [points]");
    let series: u32 = args.next().map_or(35, |s| s.parse().unwrap());
    let per: i64 = args.next().map_or(200_000, |s| s.parse().unwrap());
    let path = std::path::Path::new(&dir).join("bench.redb");
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).unwrap();
    const STEP: i64 = 300_000; // 5 min in ms
    let t0 = 1_700_000_000_000_i64;

    let start = Instant::now();
    let mut ids = Vec::new();
    for s in 0..series {
        let id = store
            .ensure_series(&SeriesMeta {
                key: SeriesKey::new(Source::Loxone, format!("uuid-{s}"), "Default"),
                quantity: Quantity::Temperature,
                unit: "°C".into(),
                name: format!("series {s}"),
                room: None,
            })
            .unwrap();
        ids.push(id);
        let mut batch = Vec::with_capacity(50_000);
        for i in 0..per {
            batch.push(Point {
                series: id,
                ts_ms: t0 + i * STEP,
                value: 20.0 + ((i % 288) as f64) / 100.0,
            });
            if batch.len() == 50_000 {
                store.write(&batch).unwrap();
                batch.clear();
            }
        }
        store.write(&batch).unwrap();
    }
    let total = series as i64 * per;
    let w = start.elapsed();
    let size = std::fs::metadata(&path).unwrap().len();
    println!(
        "write: {total} points in {:.1} s ({:.0} points/s), file {:.0} MiB ({:.1} bytes/point)",
        w.as_secs_f64(),
        total as f64 / w.as_secs_f64(),
        size as f64 / 1048576.0,
        size as f64 / total as f64
    );

    let end = t0 + (per - 1) * STEP;
    let from = end - 90 * 86_400_000;
    let start = Instant::now();
    let r = store.range(ids[ids.len() / 2], from, end).unwrap();
    println!(
        "range 90 d, one series: {} points in {:.1} ms",
        r.len(),
        start.elapsed().as_secs_f64() * 1e3
    );
    let start = Instant::now();
    let mut n = 0;
    for id in &ids {
        n += store.range(*id, from, end).unwrap().len();
    }
    println!(
        "range 90 d, all {series} series: {n} points in {:.1} ms",
        start.elapsed().as_secs_f64() * 1e3
    );
    let start = Instant::now();
    for id in &ids {
        store.latest(*id).unwrap();
    }
    println!(
        "latest, all series: {:.2} ms",
        start.elapsed().as_secs_f64() * 1e3
    );
}
