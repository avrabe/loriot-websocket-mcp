//! REST API handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use loriot_core::{AnalyticsEngine, DeviceStats, SensorReading, TimeWindow};
use serde::{Deserialize, Serialize};

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
    let storage = state.storage.read().await;

    // Get data range from storage
    let (earliest, latest) = storage.get_data_range();

    Json(DataRange {
        earliest_ts: earliest,
        latest_ts: latest,
        total_readings: storage.total_count(),
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
) -> Json<loriot_core::AnalyticsReport> {
    let storage = state.storage.read().await;

    // Use explicit time range if provided, otherwise use time_window
    let readings = if let (Some(start), Some(end)) = (query.start_ts, query.end_ts) {
        storage
            .read_from_disk_in_range(start, end, None)
            .unwrap_or_default()
    } else {
        let time_window = parse_time_window(query.time_window.as_deref());
        let cutoff = time_window.get_cutoff_timestamp();
        storage
            .read_from_disk_filtered(cutoff, None)
            .unwrap_or_default()
    };

    let report = AnalyticsEngine::analyze_readings(&readings.iter().collect::<Vec<_>>(), None);
    Json(report)
}

/// GET /api/analytics/:eui - Get analytics for a specific device
/// Uses full disk data for comprehensive analysis
pub async fn get_device_analytics(
    State(state): State<AppState>,
    Path(eui): Path<String>,
    Query(query): Query<ReadingsQuery>,
) -> Json<loriot_core::AnalyticsReport> {
    let storage = state.storage.read().await;

    // Use explicit time range if provided, otherwise use time_window
    let readings = if let (Some(start), Some(end)) = (query.start_ts, query.end_ts) {
        storage
            .read_from_disk_in_range(start, end, Some(&eui))
            .unwrap_or_default()
    } else {
        let time_window = parse_time_window(query.time_window.as_deref());
        let cutoff = time_window.get_cutoff_timestamp();
        storage
            .read_from_disk_filtered(cutoff, Some(&eui))
            .unwrap_or_default()
    };

    let report =
        AnalyticsEngine::analyze_readings(&readings.iter().collect::<Vec<_>>(), Some(&eui));
    Json(report)
}
