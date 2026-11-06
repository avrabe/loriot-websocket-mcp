//! Device detail page

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use loriot_core::{DeviceStats, SensorReading, TimeWindow};

use crate::api::{fetch_data_range, fetch_device_readings, fetch_devices, ReadingsQuery};
use crate::components::{DataPoint, DualLineChart, Navbar, TimeSelectorWithNav};

/// Format timestamp to readable date string
fn format_time(ts: u64) -> String {
    let secs = (ts / 1000) as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Format seconds into human-readable countdown
fn format_countdown(seconds: i64) -> String {
    if seconds < 0 {
        let abs_secs = seconds.abs();
        let mins = abs_secs / 60;
        if mins > 0 {
            format!("{} min overdue", mins)
        } else {
            format!("{} sec overdue", abs_secs)
        }
    } else {
        let mins = seconds / 60;
        let secs = seconds % 60;
        if mins > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}s", secs)
        }
    }
}

/// Device detail page showing full history and analytics
#[component]
pub fn DeviceDetailPage() -> impl IntoView {
    let params = use_params_map();
    let eui = move || params.read().get("eui").unwrap_or_default();

    let (time_window, set_time_window) = signal(TimeWindow::LastDay);
    let (time_offset, set_time_offset) = signal(0i32);
    let (device_stats, set_device_stats) = signal(Option::<DeviceStats>::None);
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

    // Fetch device data when eui, time window, or offset changes
    Effect::new(move |_| {
        let device_eui = eui();
        let tw = time_window.get();
        let off = time_offset.get();
        set_loading.set(true);

        leptos::task::spawn_local(async move {
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

            // Fetch device stats
            if let Ok(devices) = fetch_devices(Some(&tw)).await {
                if let Some(stats) = devices.into_iter().find(|d| d.device_id == device_eui) {
                    set_device_stats.set(Some(stats));
                }
            }

            // Fetch device readings with time range
            let query = if duration > 0 {
                let end = now - (off as u64 * duration);
                let start = end - duration;
                ReadingsQuery::new().with_time_range(start, end)
            } else {
                ReadingsQuery::new().with_time_window(&tw)
            };

            match fetch_device_readings(&device_eui, query).await {
                Ok(data) => {
                    set_readings.set(data);
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
                <div class="breadcrumbs text-sm mb-4">
                    <ul>
                        <li><a href="/">"Dashboard"</a></li>
                        <li>"Device"</li>
                        <li>{eui}</li>
                    </ul>
                </div>

                <div class="flex justify-between items-center mb-6">
                    <div>
                        <h1 class="text-3xl font-bold">
                            {move || {
                                if let Some(stats) = device_stats.get() {
                                    stats.device_name.unwrap_or_else(&eui)
                                } else {
                                    eui()
                                }
                            }}
                        </h1>
                        <p class="text-sm opacity-70 font-mono">"EUI: " {eui}</p>
                    </div>
                    <TimeSelectorWithNav
                        time_window=time_window
                        offset=time_offset
                        set_time_window=set_time_window
                        set_offset=set_time_offset
                        data_start=data_start
                        data_end=data_end
                    />
                </div>

                {move || {
                    if loading.get() {
                        view! {
                            <div class="flex justify-center items-center h-64">
                                <span class="loading loading-spinner loading-lg"></span>
                            </div>
                        }.into_any()
                    } else if let Some(stats) = device_stats.get() {
                        let avg_temp = stats.avg_temperature().map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "--".to_string());
                        let avg_humidity = stats.avg_humidity().map(|h| format!("{:.1}%", h)).unwrap_or_else(|| "--".to_string());
                        let battery_voltage = stats.battery_voltage.map(|v| format!("{:.2}V", v)).unwrap_or_else(|| "--".to_string());
                        let battery_status = stats.battery_status.clone().unwrap_or_else(|| "Unknown".to_string());
                        let total_readings = stats.total_readings;
                        let is_overdue = stats.is_overdue();
                        let seconds_until = stats.seconds_until_next();
                        let countdown = format_countdown(seconds_until);
                        let last_seen = format_time(stats.last_reading_ts);

                        view! {
                            // Device stats cards
                            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-4 mb-6">
                                <div class="stat bg-base-100 rounded-lg shadow">
                                    <div class="stat-title">"Temperature"</div>
                                    <div class="stat-value text-primary">{avg_temp}</div>
                                    <div class="stat-desc">"Average"</div>
                                </div>
                                <div class="stat bg-base-100 rounded-lg shadow">
                                    <div class="stat-title">"Humidity"</div>
                                    <div class="stat-value text-secondary">{avg_humidity}</div>
                                    <div class="stat-desc">"Average"</div>
                                </div>
                                <div class="stat bg-base-100 rounded-lg shadow">
                                    <div class="stat-title">"Battery"</div>
                                    <div class="stat-value text-accent">{battery_voltage}</div>
                                    <div class="stat-desc">{battery_status}</div>
                                </div>
                                <div class="stat bg-base-100 rounded-lg shadow">
                                    <div class="stat-title">"Readings"</div>
                                    <div class="stat-value">{total_readings}</div>
                                    <div class="stat-desc">"In time window"</div>
                                </div>
                                <div class="stat bg-base-100 rounded-lg shadow">
                                    <div class="stat-title">"Next Packet"</div>
                                    <div class={if is_overdue { "stat-value text-warning text-lg" } else { "stat-value text-info text-lg" }}>
                                        {countdown}
                                    </div>
                                    <div class="stat-desc">"Last: " {last_seen}</div>
                                </div>
                            </div>

                            // Temperature & Humidity Chart
                            <div class="card bg-base-100 shadow-xl mb-6">
                                <div class="card-body">
                                    <h2 class="card-title">"Temperature & Humidity"</h2>
                                    {move || {
                                        let reading_list = readings.get();
                                        let temp_data: Vec<DataPoint> = reading_list
                                            .iter()
                                            .filter_map(|r| r.parsed_temperature.map(|t| (r.ts, t)))
                                            .collect();
                                        let humidity_data: Vec<DataPoint> = reading_list
                                            .iter()
                                            .filter_map(|r| r.parsed_humidity.map(|h| (r.ts, h)))
                                            .collect();

                                        view! {
                                            <DualLineChart
                                                temp_data=temp_data
                                                humidity_data=humidity_data
                                                width=800
                                                height=300
                                            />
                                        }
                                    }}
                                </div>
                            </div>

                            // Status indicator
                            <div class={if is_overdue { "alert alert-warning mb-6" } else { "alert alert-success mb-6" }}>
                                {if is_overdue {
                                    view! {
                                        <svg xmlns="http://www.w3.org/2000/svg" class="stroke-current shrink-0 h-6 w-6" fill="none" viewBox="0 0 24 24">
                                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                                        </svg>
                                        <span>"Device is overdue for a reading. Expected every 20 minutes."</span>
                                    }.into_any()
                                } else {
                                    view! {
                                        <svg xmlns="http://www.w3.org/2000/svg" class="stroke-current shrink-0 h-6 w-6" fill="none" viewBox="0 0 24 24">
                                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                                        </svg>
                                        <span>"Device is online and reporting normally."</span>
                                    }.into_any()
                                }}
                            </div>

                            // Recent readings table
                            <div class="card bg-base-100 shadow-xl">
                                <div class="card-body">
                                    <h2 class="card-title">"Recent Readings"</h2>
                                    <div class="overflow-x-auto">
                                        <table class="table table-zebra table-sm">
                                            <thead>
                                                <tr>
                                                    <th>"Time"</th>
                                                    <th>"Temperature"</th>
                                                    <th>"Humidity"</th>
                                                    <th>"Battery"</th>
                                                    <th>"Motion"</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                {move || {
                                                    let data = readings.get();
                                                    if data.is_empty() {
                                                        view! {
                                                            <tr>
                                                                <td colspan="5" class="text-center py-4 opacity-70">
                                                                    "No readings found"
                                                                </td>
                                                            </tr>
                                                        }.into_any()
                                                    } else {
                                                        view! {
                                                            {data.into_iter().take(50).map(|reading| {
                                                                let time = format_time(reading.ts);
                                                                let temp = reading.parsed_temperature.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "-".to_string());
                                                                let humidity = reading.parsed_humidity.map(|h| format!("{:.1}%", h)).unwrap_or_else(|| "-".to_string());
                                                                let battery = reading.parsed_battery_status.clone().unwrap_or_else(|| "-".to_string());
                                                                let motion = if reading.parsed_activity.unwrap_or(false) { "Yes" } else { "-" };

                                                                view! {
                                                                    <tr>
                                                                        <td class="font-mono text-xs">{time}</td>
                                                                        <td class="text-primary">{temp}</td>
                                                                        <td class="text-secondary">{humidity}</td>
                                                                        <td>{battery}</td>
                                                                        <td>{motion}</td>
                                                                    </tr>
                                                                }
                                                            }).collect::<Vec<_>>()}
                                                        }.into_any()
                                                    }
                                                }}
                                            </tbody>
                                        </table>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="alert alert-warning">
                                <span>"Device not found or no data available"</span>
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}
