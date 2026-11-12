//! REST API handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use haus_core::{AnalyticsEngine, DeviceStats, DiskReader, SensorReading, TimeWindow};
use serde::{Deserialize, Serialize};

use crate::state::run_blocking;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ReadingsQuery {
    pub time_window: Option<String>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    /// Start timestamp in milliseconds (overrides time_window)
    pub start_ts: Option<u64>,
    /// End timestamp in milliseconds (overrides time_window)
    pub end_ts: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedReadings {
    pub readings: Vec<SensorReading>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    /// Total readings on disk (may be larger than in-memory cache)
    pub total_on_disk: usize,
}

#[derive(Debug, Serialize)]
pub struct SystemInfo {
    pub readings_in_memory: usize,
    pub readings_on_disk: usize,
    pub device_count: usize,
    pub connection_status: String,
}

#[derive(Debug, Serialize)]
pub struct DataRange {
    /// Earliest timestamp in the dataset (milliseconds)
    pub earliest_ts: Option<u64>,
    /// Latest timestamp in the dataset (milliseconds)
    pub latest_ts: Option<u64>,
    /// Total number of readings
    pub total_readings: usize,
}

fn parse_time_window(s: Option<&str>) -> TimeWindow {
    match s {
        Some("hour") | Some("LastHour") => TimeWindow::LastHour,
        Some("6hours") | Some("Last6Hours") => TimeWindow::Last6Hours,
        Some("day") | Some("LastDay") => TimeWindow::LastDay,
        Some("week") | Some("LastWeek") => TimeWindow::LastWeek,
        Some("month") | Some("LastMonth") => TimeWindow::LastMonth,
        _ => TimeWindow::All,
    }
}

/// GET /api/info - Get system information
pub async fn get_system_info(State(state): State<AppState>) -> Json<SystemInfo> {
    let storage = state.storage.read().await;
    let status = state.connection_status.read().await;

    Json(SystemInfo {
        readings_in_memory: storage.reading_count(),
        readings_on_disk: storage.total_count(),
        device_count: storage.device_count(),
        connection_status: format!("{:?}", *status),
    })
}

/// GET /api/data-range - Get the overall data range (earliest and latest timestamps)
pub async fn get_data_range(State(state): State<AppState>) -> Json<DataRange> {
    let (disk, total_readings, memory_range) = {
        let storage = state.storage.read().await;
        (
            storage.disk(),
            storage.total_count(),
            storage.memory_data_range(),
        )
    };

    // Full scan of the history file, off the runtime and outside the lock;
    // fall back to the in-memory range if the file cannot be read.
    let (earliest, latest) = run_blocking(move || disk.data_range())
        .await
        .unwrap_or(memory_range);

    Json(DataRange {
        earliest_ts: earliest,
        latest_ts: latest,
        total_readings,
    })
}

/// GET /api/readings - Get paginated sensor readings
pub async fn get_readings(
    State(state): State<AppState>,
    Query(query): Query<ReadingsQuery>,
) -> Json<PaginatedReadings> {
    let page = query.page.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(50);

    let storage = state.storage.read().await;
    let total_on_disk = storage.total_count();

    // Use explicit time range if provided, otherwise use time_window
    let (readings, total) = if let (Some(start), Some(end)) = (query.start_ts, query.end_ts) {
        // Filter by explicit time range
        storage.get_readings_in_range(start, end, page, page_size)
    } else {
        let time_window = parse_time_window(query.time_window.as_deref());
        let cutoff = time_window.get_cutoff_timestamp();
        storage.get_readings_paginated(cutoff, page, page_size)
    };

    Json(PaginatedReadings {
        readings: readings.into_iter().cloned().collect(),
        total,
        page,
        page_size,
        total_on_disk,
    })
}

/// GET /api/readings/:eui - Get readings for a specific device
pub async fn get_device_readings(
    State(state): State<AppState>,
    Path(eui): Path<String>,
    Query(query): Query<ReadingsQuery>,
) -> Json<Vec<SensorReading>> {
    let storage = state.storage.read().await;

    // Use explicit time range if provided, otherwise use time_window
    let readings: Vec<SensorReading> =
        if let (Some(start), Some(end)) = (query.start_ts, query.end_ts) {
            storage
                .get_readings_for_device(&eui)
                .into_iter()
                .filter(|r| r.ts >= start && r.ts <= end)
                .cloned()
                .collect()
        } else {
            let time_window = parse_time_window(query.time_window.as_deref());
            let cutoff = time_window.get_cutoff_timestamp();
            storage
                .get_readings_for_device(&eui)
                .into_iter()
                .filter(|r| cutoff.map(|c| r.ts >= c).unwrap_or(true))
                .cloned()
                .collect()
        };

    Json(readings)
}

/// GET /api/devices - Get all devices with stats
pub async fn get_devices(
    State(state): State<AppState>,
    Query(query): Query<ReadingsQuery>,
) -> Json<Vec<DeviceStats>> {
    let time_window = parse_time_window(query.time_window.as_deref());
    let cutoff = time_window.get_cutoff_timestamp();

    let storage = state.storage.read().await;
    let device_names = state.device_names.read().await;
    let mut stats = storage.get_device_statistics(cutoff);

    // Apply device names from mapping
    for stat in stats.values_mut() {
        if stat.device_name.is_none() {
            if let Some(name) = device_names.get(&stat.device_id) {
                stat.device_name = Some(name.clone());
            }
        }
    }

    Json(stats.into_values().collect())
}

/// GET /api/devices/:eui - Get stats for a specific device
pub async fn get_device_stats(
    State(state): State<AppState>,
    Path(eui): Path<String>,
    Query(query): Query<ReadingsQuery>,
) -> Result<Json<DeviceStats>, StatusCode> {
    let time_window = parse_time_window(query.time_window.as_deref());
    let cutoff = time_window.get_cutoff_timestamp();

    let storage = state.storage.read().await;
    let device_names = state.device_names.read().await;
    let stats = storage.get_device_statistics(cutoff);

    stats
        .get(&eui)
        .cloned()
        .map(|mut stat| {
            // Apply device name from mapping
            if stat.device_name.is_none() {
                if let Some(name) = device_names.get(&stat.device_id) {
                    stat.device_name = Some(name.clone());
                }
            }
            Json(stat)
        })
        .ok_or(StatusCode::NOT_FOUND)
}

/// GET /api/analytics - Get analytics for all readings
/// Uses full disk data for comprehensive analysis
pub async fn get_analytics(
    State(state): State<AppState>,
    Query(query): Query<ReadingsQuery>,
) -> Json<haus_core::AnalyticsReport> {
    let disk = state.disk().await;
    Json(run_blocking(move || analytics_from_disk(&disk, &query, None)).await)
}

/// Read the requested slice of history and analyse it. Blocking.
fn analytics_from_disk(
    disk: &DiskReader,
    query: &ReadingsQuery,
    eui: Option<&str>,
) -> haus_core::AnalyticsReport {
    // Use explicit time range if provided, otherwise use time_window
    let readings = if let (Some(start), Some(end)) = (query.start_ts, query.end_ts) {
        disk.read_in_range(start, end, eui).unwrap_or_default()
    } else {
        let time_window = parse_time_window(query.time_window.as_deref());
        let cutoff = time_window.get_cutoff_timestamp();
        disk.read_filtered(cutoff, eui).unwrap_or_default()
    };

    AnalyticsEngine::analyze_readings(&readings.iter().collect::<Vec<_>>(), eui)
}

/// GET /api/analytics/:eui - Get analytics for a specific device
/// Uses full disk data for comprehensive analysis
pub async fn get_device_analytics(
    State(state): State<AppState>,
    Path(eui): Path<String>,
    Query(query): Query<ReadingsQuery>,
) -> Json<haus_core::AnalyticsReport> {
    let disk = state.disk().await;
    Json(run_blocking(move || analytics_from_disk(&disk, &query, Some(&eui))).await)
}

#[cfg(test)]
mod tests {
    //! The runtime must stay responsive while a handler reads the whole JSONL
    //! history from disk (the file grows without bound on a long-running box).
    use super::*;
    use haus_core::{AppConfig, StorageConfig, StorageManager};
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// AppState over a JSONL file with `lines` readings.
    fn state_with_history(name: &str, lines: usize) -> AppState {
        let path =
            std::env::temp_dir().join(format!("loriot-api-{}-{}.jsonl", name, std::process::id()));
        let mut f = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        for i in 0..lines {
            writeln!(
                f,
                r#"{{"cmd":"rx","eui":"E{}","data":"00","ts":{},"name":null,"parsed_temperature":21.5,"parsed_humidity":40.0}}"#,
                i % 5,
                1_700_000_000_000u64 + i as u64 * 1000
            )
            .unwrap();
        }
        f.flush().unwrap();
        let storage = StorageManager::new(StorageConfig {
            file_path: path.to_string_lossy().into_owned(),
            ..Default::default()
        });
        AppState::new(
            storage,
            HashMap::new(),
            AppConfig {
                device_names_file: None,
                ..Default::default()
            },
        )
    }

    /// Longest gap between 5 ms ticks of a ticker on the current runtime
    /// while `fut` runs.
    async fn max_tick_gap<F: std::future::Future>(fut: F) -> (F::Output, Duration) {
        let stop = Arc::new(AtomicBool::new(false));
        let max_gap = Arc::new(Mutex::new(Duration::ZERO));
        let (s, m) = (stop.clone(), max_gap.clone());
        let ticker = tokio::spawn(async move {
            let mut last = Instant::now();
            while !s.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
                let gap = last.elapsed();
                let mut g = m.lock().unwrap();
                if gap > *g {
                    *g = gap;
                }
                last = Instant::now();
            }
        });
        // Let the ticker start before the work begins.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let out = fut.await;
        stop.store(true, Ordering::SeqCst);
        ticker.await.unwrap();
        let gap = *max_gap.lock().unwrap();
        (out, gap)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn data_range_does_not_block_the_runtime() {
        let state = state_with_history("range", 300_000);
        let start = Instant::now();
        let (Json(range), gap) = max_tick_gap(get_data_range(State(state))).await;
        let took = start.elapsed();
        assert_eq!(range.earliest_ts, Some(1_700_000_000_000));
        assert_eq!(range.latest_ts, Some(1_700_000_000_000 + 299_999 * 1000));
        // Only meaningful if the read itself takes a while.
        assert!(
            took > Duration::from_millis(200),
            "history too small: {took:?}"
        );
        assert!(
            gap < Duration::from_millis(100),
            "runtime blocked for {gap:?} while get_data_range read {took:?} of history"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn analytics_does_not_block_the_runtime() {
        let state = state_with_history("analytics", 300_000);
        let query = ReadingsQuery {
            time_window: Some("all".into()),
            page: None,
            page_size: None,
            start_ts: None,
            end_ts: None,
        };
        let (_, gap) = max_tick_gap(get_analytics(State(state), Query(query))).await;
        assert!(
            gap < Duration::from_millis(100),
            "runtime blocked for {gap:?}"
        );
    }
}
