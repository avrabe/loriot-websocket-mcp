//! Dashboard page - main overview

use leptos::prelude::*;
use loriot_core::{DeviceStats, SensorReading, TimeWindow};

use crate::api::{fetch_data_range, fetch_devices, fetch_readings, ReadingsQuery};
use crate::components::{DataPoint, DualLineChart, Navbar, SensorCard, TimeSelectorWithNav};

/// Summary statistics for the dashboard
#[derive(Clone, Default)]
struct DashboardStats {
    device_count: usize,
    avg_temperature: Option<f32>,
    avg_humidity: Option<f32>,
    motion_events: usize,
    battery_warnings: usize,
}

impl DashboardStats {
    fn from_devices(devices: &[DeviceStats]) -> Self {
        let device_count = devices.len();

        let temps: Vec<f32> = devices.iter().filter_map(|d| d.avg_temperature()).collect();
        let avg_temperature = if temps.is_empty() {
            None
        } else {
            Some(temps.iter().sum::<f32>() / temps.len() as f32)
        };

        let humidities: Vec<f32> = devices.iter().filter_map(|d| d.avg_humidity()).collect();
        let avg_humidity = if humidities.is_empty() {
            None
        } else {
            Some(humidities.iter().sum::<f32>() / humidities.len() as f32)
        };

        let motion_events = devices.iter().map(|d| d.pir_events).sum();

        let battery_warnings = devices
            .iter()
            .filter(|d| {
                d.battery_status
                    .as_ref()
                    .map(|s| s == "Low" || s == "Critical")
                    .unwrap_or(false)
            })
            .count();

        Self {
            device_count,
            avg_temperature,
            avg_humidity,
            motion_events,
            battery_warnings,
        }
    }
}

/// Main dashboard page showing all devices and summary
#[component]
pub fn DashboardPage() -> impl IntoView {
    // Time window filter state with navigation
    let (time_window, set_time_window) = signal(TimeWindow::LastDay);
    let (time_offset, set_time_offset) = signal(0i32);

    // Devices state - fetched from API
    let (devices, set_devices) = signal(Vec::<DeviceStats>::new());
    let (readings, set_readings) = signal(Vec::<SensorReading>::new());
    let (loading, set_loading) = signal(true);

    // Data range for "All Time" display
    let (data_start, set_data_start) = signal(Option::<u64>::None);
    let (data_end, set_data_end) = signal(Option::<u64>::None);

    // Fetch overall data range once on mount
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Ok(range) = fetch_data_range().await {
                set_data_start.set(range.earliest_ts);
                set_data_end.set(range.latest_ts);
            }
        });
    });

    // Fetch devices and readings when time window or offset changes
    Effect::new(move |_| {
        let tw = time_window.get();
        let off = time_offset.get();
        set_loading.set(true);

        leptos::task::spawn_local(async move {
            // Fetch devices
            match fetch_devices(Some(&tw)).await {
                Ok(fetched) => {
                    set_devices.set(fetched);
                }
                Err(e) => {
                    leptos::logging::error!("Failed to fetch devices: {}", e);
                }
            }

            // Calculate time range based on window and offset
            let now = js_sys::Date::now() as u64;
            let duration = match tw {
                TimeWindow::LastHour => 60 * 60 * 1000,
                TimeWindow::Last6Hours => 6 * 60 * 60 * 1000,
                TimeWindow::LastDay => 24 * 60 * 60 * 1000,
                TimeWindow::LastWeek => 7 * 24 * 60 * 60 * 1000,
                TimeWindow::LastMonth => 30 * 24 * 60 * 60 * 1000,
                TimeWindow::All => 0,
            };

            // Fetch recent readings for charts (last 200 points)
            let query = if duration > 0 {
                let end = now - (off as u64 * duration);
                let start = end - duration;
                ReadingsQuery::new()
                    .with_time_range(start, end)
                    .with_page(1)
                    .with_page_size(200)
            } else {
                // All time - no range filter
                ReadingsQuery::new()
                    .with_time_window(&tw)
                    .with_page(1)
                    .with_page_size(200)
            };

            match fetch_readings(query).await {
                Ok(result) => {
                    // Log the readings timestamps for debugging
                    if let (Some(first), Some(last)) = (result.readings.first(), result.readings.last()) {
                        leptos::logging::log!(
                            "Readings: {} total, first ts={}, last ts={}",
                            result.readings.len(),
                            first.ts,
                            last.ts
                        );
                    }
                    set_readings.set(result.readings);
                }
                Err(e) => {
                    leptos::logging::error!("Failed to fetch readings: {}", e);
                }
            }

            set_loading.set(false);
        });
    });

    view! {
        <div>
            <Navbar />

            <div class="container mx-auto p-4">
                // Header with time selector
                <div class="flex justify-between items-center mb-6">
                    <h1 class="text-3xl font-bold">"Sensor Dashboard"</h1>
                    <TimeSelectorWithNav
                        time_window=time_window
                        offset=time_offset
                        set_time_window=set_time_window
                        set_offset=set_time_offset
                        data_start=data_start
                        data_end=data_end
                    />
                </div>

                // Summary stats
                {move || {
                    if loading.get() {
                        view! { <StatsLoading /> }.into_any()
                    } else {
                        let device_list = devices.get();
                        let stats = DashboardStats::from_devices(&device_list);
                        view! { <SummaryStats stats=stats /> }.into_any()
                    }
                }}

                // Per-device Temperature & Humidity Charts
                <div class="card bg-base-100 shadow-xl mb-6">
                    <div class="card-body">
                        <h2 class="card-title">"Temperature & Humidity Charts"</h2>
                        {move || {
                            if loading.get() {
                                view! {
                                    <div class="flex items-center justify-center h-48">
                                        <span class="loading loading-spinner loading-lg"></span>
                                    </div>
                                }.into_any()
                            } else {
                                let reading_list = readings.get();
                                let device_list = devices.get();

                                if device_list.is_empty() {
                                    view! {
                                        <div class="text-center py-8 opacity-70">
                                            "No devices available for charting"
                                        </div>
                                    }.into_any()
                                } else {
                                    // Group readings by device
                                    let device_charts: Vec<_> = device_list.iter().map(|device| {
                                        let device_readings: Vec<_> = reading_list
                                            .iter()
                                            .filter(|r| r.eui == device.device_id)
                                            .collect();

                                        let temp_data: Vec<DataPoint> = device_readings
                                            .iter()
                                            .filter_map(|r| r.parsed_temperature.map(|t| (r.ts, t)))
                                            .collect();

                                        let humidity_data: Vec<DataPoint> = device_readings
                                            .iter()
                                            .filter_map(|r| r.parsed_humidity.map(|h| (r.ts, h)))
                                            .collect();

                                        let device_name = device.device_name.clone().unwrap_or_else(|| device.device_id.clone());
                                        let device_id = device.device_id.clone();

                                        (device_name, device_id, temp_data, humidity_data)
                                    }).collect();

                                    view! {
                                        <div class="space-y-6">
                                            {device_charts.into_iter().map(|(name, id, temp_data, humidity_data)| {
                                                view! {
                                                    <div class="collapse collapse-arrow bg-base-200">
                                                        <input type="checkbox" checked />
                                                        <div class="collapse-title font-medium">
                                                            {name.clone()}
                                                            <span class="text-xs opacity-50 ml-2 font-mono">{id.clone()}</span>
                                                        </div>
                                                        <div class="collapse-content">
                                                            <DualLineChart
                                                                temp_data=temp_data.clone()
                                                                humidity_data=humidity_data.clone()
                                                                width=700
                                                                height=250
                                                            />
                                                        </div>
                                                    </div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                }
                            }
                        }}
                    </div>
                </div>

                // Device grid
                <h2 class="text-2xl font-bold mb-4 mt-8">"Devices"</h2>
                {move || {
                    if loading.get() {
                        view! { <DevicesLoading /> }.into_any()
                    } else {
                        let device_list = devices.get();
                        if device_list.is_empty() {
                            view! {
                                <div class="alert alert-info">
                                    <span>"No devices found. Waiting for sensor data..."</span>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                                    {device_list.into_iter().map(|stats| {
                                        view! { <SensorCard stats=stats /> }
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }
                    }
                }}
            </div>
        </div>
    }
}

#[component]
fn SummaryStats(stats: DashboardStats) -> impl IntoView {
    view! {
        <div class="stats shadow w-full mb-6">
            <div class="stat">
                <div class="stat-figure text-primary">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-8 h-8 stroke-current">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z"></path>
                    </svg>
                </div>
                <div class="stat-title">"Active Devices"</div>
                <div class="stat-value text-primary">{stats.device_count}</div>
                <div class="stat-desc">{if stats.device_count == 1 { "sensor" } else { "sensors" }} " online"</div>
            </div>

            <div class="stat">
                <div class="stat-figure text-secondary">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-8 h-8 stroke-current">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6V4m0 2a2 2 0 100 4m0-4a2 2 0 110 4m-6 8a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4m6 6v10m6-2a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4"></path>
                    </svg>
                </div>
                <div class="stat-title">"Avg Temperature"</div>
                <div class="stat-value text-secondary">
                    {stats.avg_temperature.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "--".to_string())}
                </div>
                <div class="stat-desc">"Across all sensors"</div>
            </div>

            <div class="stat">
                <div class="stat-figure text-accent">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-8 h-8 stroke-current">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4"></path>
                    </svg>
                </div>
                <div class="stat-title">"Avg Humidity"</div>
                <div class="stat-value text-accent">
                    {stats.avg_humidity.map(|h| format!("{:.0}%", h)).unwrap_or_else(|| "--".to_string())}
                </div>
                <div class="stat-desc">"Relative humidity"</div>
            </div>

            <div class="stat">
                <div class="stat-figure text-warning">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-8 h-8 stroke-current">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"></path>
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z"></path>
                    </svg>
                </div>
                <div class="stat-title">"Motion Events"</div>
                <div class="stat-value text-warning">{stats.motion_events}</div>
                <div class="stat-desc">"PIR detections"</div>
            </div>

            {(stats.battery_warnings > 0).then(|| view! {
                <div class="stat">
                    <div class="stat-figure text-error">
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-8 h-8 stroke-current">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path>
                        </svg>
                    </div>
                    <div class="stat-title">"Battery Warnings"</div>
                    <div class="stat-value text-error">{stats.battery_warnings}</div>
                    <div class="stat-desc">"Low battery devices"</div>
                </div>
            })}
        </div>
    }
}

#[component]
fn StatsLoading() -> impl IntoView {
    view! {
        <div class="stats shadow w-full mb-6 animate-pulse">
            <div class="stat">
                <div class="stat-title">"Loading..."</div>
                <div class="stat-value">"--"</div>
            </div>
            <div class="stat">
                <div class="stat-title">"Loading..."</div>
                <div class="stat-value">"--"</div>
            </div>
            <div class="stat">
                <div class="stat-title">"Loading..."</div>
                <div class="stat-value">"--"</div>
            </div>
            <div class="stat">
                <div class="stat-title">"Loading..."</div>
                <div class="stat-value">"--"</div>
            </div>
        </div>
    }
}

#[component]
fn DevicesLoading() -> impl IntoView {
    view! {
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            <div class="card bg-base-100 shadow-xl animate-pulse">
                <div class="card-body">
                    <div class="h-6 bg-base-300 rounded w-3/4 mb-4"></div>
                    <div class="h-4 bg-base-300 rounded w-1/2 mb-2"></div>
                    <div class="h-4 bg-base-300 rounded w-2/3"></div>
                </div>
            </div>
            <div class="card bg-base-100 shadow-xl animate-pulse">
                <div class="card-body">
                    <div class="h-6 bg-base-300 rounded w-3/4 mb-4"></div>
                    <div class="h-4 bg-base-300 rounded w-1/2 mb-2"></div>
                    <div class="h-4 bg-base-300 rounded w-2/3"></div>
                </div>
            </div>
        </div>
    }
}
