//! History page

use leptos::prelude::*;
use loriot_core::{SensorReading, TimeWindow};
use wasm_bindgen::JsCast;

use crate::api::{fetch_data_range, fetch_readings, ReadingsQuery};
use crate::components::{Navbar, TimeSelectorWithNav};

/// Format timestamp to readable date string
fn format_time(ts: u64) -> String {
    let secs = (ts / 1000) as i64;
    let naive = chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    naive
}

/// Generate CSV content from readings
fn generate_csv(readings: &[SensorReading]) -> String {
    let mut csv = String::from("Time,Device,DeviceName,Temperature,Humidity,Battery,Motion\n");
    for reading in readings {
        let time = format_time(reading.ts);
        let device = &reading.eui;
        let name = reading.name.as_deref().unwrap_or("-");
        let temp = reading
            .parsed_temperature
            .map(|t| format!("{:.1}", t))
            .unwrap_or_else(|| "-".to_string());
        let humidity = reading
            .parsed_humidity
            .map(|h| format!("{:.1}", h))
            .unwrap_or_else(|| "-".to_string());
        let battery = reading.parsed_battery_status.as_deref().unwrap_or("-");
        let motion = if reading.parsed_activity.unwrap_or(false) {
            "Yes"
        } else {
            "-"
        };
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            time, device, name, temp, humidity, battery, motion
        ));
    }
    csv
}

/// History page for browsing historical sensor data
#[component]
pub fn HistoryPage() -> impl IntoView {
    let (page, set_page) = signal(1usize);
    let (page_size, set_page_size) = signal(50usize);
    let (time_window, set_time_window) = signal(TimeWindow::LastDay);
    let (time_offset, set_time_offset) = signal(0i32);
    let (readings, set_readings) = signal(Vec::<SensorReading>::new());
    let (total, set_total) = signal(0usize);
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

    // Fetch readings when filters change
    Effect::new(move |_| {
        let tw = time_window.get();
        let off = time_offset.get();
        let p = page.get();
        let ps = page_size.get();
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

            let query = if duration > 0 {
                let end = now - (off as u64 * duration);
                let start = end - duration;
                ReadingsQuery::new()
                    .with_time_range(start, end)
                    .with_page(p)
                    .with_page_size(ps)
            } else {
                ReadingsQuery::new()
                    .with_time_window(&tw)
                    .with_page(p)
                    .with_page_size(ps)
            };

            match fetch_readings(query).await {
                Ok(result) => {
                    set_readings.set(result.readings);
                    set_total.set(result.total);
                }
                Err(e) => {
                    leptos::logging::error!("Failed to fetch readings: {}", e);
                }
            }
            set_loading.set(false);
        });
    });

    let total_pages = Signal::derive(move || {
        let t = total.get();
        let ps = page_size.get();
        if t == 0 {
            1
        } else {
            t.div_ceil(ps)
        }
    });

    let can_go_prev = Signal::derive(move || page.get() > 1);
    let can_go_next = Signal::derive(move || page.get() < total_pages.get());

    view! {
        <div>
            <Navbar />

            <div class="container mx-auto p-4">
                <div class="flex justify-between items-center mb-6">
                    <h1 class="text-3xl font-bold">"Historical Data"</h1>
                    <TimeSelectorWithNav
                        time_window=time_window
                        offset=time_offset
                        set_time_window=set_time_window
                        set_offset=set_time_offset
                        data_start=data_start
                        data_end=data_end
                    />
                </div>

                // Filters and Export
                <div class="card bg-base-100 shadow-xl mb-6">
                    <div class="card-body">
                        <div class="flex flex-wrap justify-between items-center gap-4">
                            <div class="flex flex-wrap items-center gap-4">
                                <div class="form-control">
                                    <label class="label">
                                        <span class="label-text">"Page Size"</span>
                                    </label>
                                    <select class="select select-bordered select-sm"
                                        on:change=move |ev| {
                                            let value = event_target_value(&ev);
                                            if let Ok(size) = value.parse() {
                                                set_page_size.set(size);
                                                set_page.set(1);
                                            }
                                        }
                                    >
                                        <option value="25">"25"</option>
                                        <option value="50" selected>"50"</option>
                                        <option value="100">"100"</option>
                                    </select>
                                </div>
                                <div class="text-sm opacity-70 pt-6">
                                    {move || format!("{} total readings", total.get())}
                                </div>
                            </div>
                            <button
                                class="btn btn-primary btn-sm"
                                on:click=move |_| {
                                    let data = readings.get();
                                    if data.is_empty() {
                                        return;
                                    }
                                    let csv = generate_csv(&data);

                                    // Create and trigger download
                                    if let Some(window) = web_sys::window() {
                                        if let Some(document) = window.document() {
                                            if let Ok(blob) = web_sys::Blob::new_with_str_sequence(&js_sys::Array::of1(&csv.into())) {
                                                if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                                                    if let Ok(a) = document.create_element("a") {
                                                        let _ = a.set_attribute("href", &url);
                                                        let _ = a.set_attribute("download", "sensor_readings.csv");
                                                        if let Some(html_a) = a.dyn_ref::<web_sys::HtmlElement>() {
                                                            html_a.click();
                                                        }
                                                        let _ = web_sys::Url::revoke_object_url(&url);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            >
                                "Export CSV"
                            </button>
                        </div>
                    </div>
                </div>

                // Data table
                <div class="card bg-base-100 shadow-xl">
                    <div class="card-body">
                        <div class="overflow-x-auto">
                            <table class="table table-zebra table-sm">
                                <thead>
                                    <tr>
                                        <th>"Time"</th>
                                        <th>"Device"</th>
                                        <th>"Temperature"</th>
                                        <th>"Humidity"</th>
                                        <th>"Battery"</th>
                                        <th>"Motion"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {move || {
                                        if loading.get() {
                                            view! {
                                                <tr>
                                                    <td colspan="6" class="text-center py-8">
                                                        <span class="loading loading-spinner loading-md"></span>
                                                        " Loading data..."
                                                    </td>
                                                </tr>
                                            }.into_any()
                                        } else {
                                            let data = readings.get();
                                            if data.is_empty() {
                                                view! {
                                                    <tr>
                                                        <td colspan="6" class="text-center py-8 opacity-70">
                                                            "No readings found for selected filters"
                                                        </td>
                                                    </tr>
                                                }.into_any()
                                            } else {
                                                view! {
                                                    {data.into_iter().map(|reading| {
                                                        let time = format_time(reading.ts);
                                                        let device = reading.name.clone().unwrap_or_else(|| reading.eui.clone());
                                                        let temp = reading.parsed_temperature.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "-".to_string());
                                                        let humidity = reading.parsed_humidity.map(|h| format!("{:.1}%", h)).unwrap_or_else(|| "-".to_string());
                                                        let battery = reading.parsed_battery_status.clone().unwrap_or_else(|| "-".to_string());
                                                        let motion = if reading.parsed_activity.unwrap_or(false) { "Yes" } else { "-" };

                                                        view! {
                                                            <tr>
                                                                <td class="font-mono text-xs">{time}</td>
                                                                <td><a href={format!("/device/{}", reading.eui)} class="link link-primary">{device}</a></td>
                                                                <td class="text-primary">{temp}</td>
                                                                <td class="text-secondary">{humidity}</td>
                                                                <td>{battery}</td>
                                                                <td>{motion}</td>
                                                            </tr>
                                                        }
                                                    }).collect::<Vec<_>>()}
                                                }.into_any()
                                            }
                                        }
                                    }}
                                </tbody>
                            </table>
                        </div>

                        // Pagination
                        <div class="flex justify-between items-center mt-4">
                            <div class="text-sm opacity-70">
                                {move || {
                                    let p = page.get();
                                    let ps = page_size.get();
                                    let t = total.get();
                                    let start = (p - 1) * ps + 1;
                                    let end = std::cmp::min(p * ps, t);
                                    format!("Showing {} - {} of {}", start, end, t)
                                }}
                            </div>
                            <div class="join">
                                {move || {
                                    let prev_disabled = !can_go_prev.get();
                                    let next_disabled = !can_go_next.get();
                                    let page_display = format!("{} / {}", page.get(), total_pages.get());

                                    view! {
                                        <button
                                            class="join-item btn btn-sm"
                                            disabled=prev_disabled
                                            on:click=move |_| set_page.update(|p| *p = p.saturating_sub(1).max(1))
                                        >
                                            "Prev"
                                        </button>
                                        <button class="join-item btn btn-sm btn-active">
                                            {page_display}
                                        </button>
                                        <button
                                            class="join-item btn btn-sm"
                                            disabled=next_disabled
                                            on:click=move |_| set_page.update(|p| *p += 1)
                                        >
                                            "Next"
                                        </button>
                                    }
                                }}
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}
