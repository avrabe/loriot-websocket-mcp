//! Time window selector component with date display and navigation

use leptos::prelude::*;
use loriot_core::TimeWindow;

#[cfg(target_arch = "wasm32")]
use js_sys;

/// Format a timestamp to a readable date string
fn format_date(ts: u64) -> String {
    let secs = (ts / 1000) as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Get time range in milliseconds for a window
fn window_duration_ms(tw: &TimeWindow) -> u64 {
    match tw {
        TimeWindow::LastHour => 60 * 60 * 1000,
        TimeWindow::Last6Hours => 6 * 60 * 60 * 1000,
        TimeWindow::LastDay => 24 * 60 * 60 * 1000,
        TimeWindow::LastWeek => 7 * 24 * 60 * 60 * 1000,
        TimeWindow::LastMonth => 30 * 24 * 60 * 60 * 1000,
        TimeWindow::All => 0,
    }
}

/// Get navigation step label
fn step_label(tw: &TimeWindow) -> &'static str {
    match tw {
        TimeWindow::LastHour => "Hour",
        TimeWindow::Last6Hours => "6h",
        TimeWindow::LastDay => "Day",
        TimeWindow::LastWeek => "Week",
        TimeWindow::LastMonth => "Month",
        TimeWindow::All => "",
    }
}

/// Enhanced time selector with date display and navigation
#[component]
pub fn TimeSelector(
    selected: ReadSignal<TimeWindow>,
    on_change: WriteSignal<TimeWindow>,
) -> impl IntoView {
    view! {
        <div class="flex flex-wrap items-center gap-2">
            <select
                class="select select-bordered select-sm"
                on:change=move |ev| {
                    let value = event_target_value(&ev);
                    let window = match value.as_str() {
                        "hour" => TimeWindow::LastHour,
                        "6hours" => TimeWindow::Last6Hours,
                        "day" => TimeWindow::LastDay,
                        "week" => TimeWindow::LastWeek,
                        "month" => TimeWindow::LastMonth,
                        _ => TimeWindow::All,
                    };
                    on_change.set(window);
                }
            >
                <option value="hour" selected=move || selected.get() == TimeWindow::LastHour>
                    "Last Hour"
                </option>
                <option value="6hours" selected=move || selected.get() == TimeWindow::Last6Hours>
                    "Last 6 Hours"
                </option>
                <option value="day" selected=move || selected.get() == TimeWindow::LastDay>
                    "Last 24 Hours"
                </option>
                <option value="week" selected=move || selected.get() == TimeWindow::LastWeek>
                    "Last Week"
                </option>
                <option value="month" selected=move || selected.get() == TimeWindow::LastMonth>
                    "Last Month"
                </option>
                <option value="all" selected=move || selected.get() == TimeWindow::All>
                    "All Time"
                </option>
            </select>

            // Date range display
            <div class="badge badge-ghost text-xs">
                {move || {
                    let tw = selected.get();
                    let now = loriot_core::current_timestamp_ms();
                    match tw.get_cutoff_timestamp() {
                        Some(cutoff) => {
                            format!("{} - {}", format_date(cutoff), format_date(now))
                        }
                        None => "All available data".to_string()
                    }
                }}
            </div>
        </div>
    }
}

/// Time selector with navigation controls for browsing historical periods
#[component]
pub fn TimeSelectorWithNav(
    /// Current time window type
    time_window: ReadSignal<TimeWindow>,
    /// Time offset in periods (0 = current, 1 = previous, etc.)
    offset: ReadSignal<i32>,
    /// Set time window
    set_time_window: WriteSignal<TimeWindow>,
    /// Set offset
    set_offset: WriteSignal<i32>,
    /// Optional: earliest data timestamp (for "All Time" display)
    #[prop(optional)]
    data_start: Option<ReadSignal<Option<u64>>>,
    /// Optional: latest data timestamp (for "All Time" display)
    #[prop(optional)]
    data_end: Option<ReadSignal<Option<u64>>>,
) -> impl IntoView {
    // Calculate the time range to display
    // For specific time windows, show the calculated window (what user selected)
    // For "All Time", show actual data range if available
    let time_range = move || {
        let tw = time_window.get();
        let off = offset.get();
        let duration = window_duration_ms(&tw);

        // Get current time in milliseconds
        let now = {
            #[cfg(target_arch = "wasm32")]
            {
                let t = js_sys::Date::now() as u64;
                leptos::logging::log!("js_sys::Date::now() = {}", t);
                t
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                loriot_core::current_timestamp_ms()
            }
        };

        if duration == 0 {
            // All time - use actual data range if available from props
            let actual_start = data_start.and_then(|s| s.get());
            let actual_end = data_end.and_then(|e| e.get());
            leptos::logging::log!("All Time mode: start={:?}, end={:?}", actual_start, actual_end);
            (actual_start, actual_end.unwrap_or(now))
        } else {
            // Specific time window - show calculated range based on selection
            let end = now - (off as u64 * duration);
            let start = end - duration;
            leptos::logging::log!("Time window: duration={}, offset={}, now={}, start={}, end={}", duration, off, now, start, end);
            (Some(start), end)
        }
    };

    let can_go_forward = move || offset.get() > 0;
    let is_all_time = move || time_window.get() == TimeWindow::All;

    // Check if we can go further back
    // For "All Time" mode, navigation is disabled
    // Otherwise, allow going back (we can't easily know the overall earliest data)
    let can_go_back = move || {
        let tw = time_window.get();
        let duration = window_duration_ms(&tw);
        // Can't navigate in "All Time" mode
        duration > 0
    };

    view! {
        <div class="flex flex-wrap items-center gap-2">
            // Time window selector
            <select
                class="select select-bordered select-sm"
                on:change=move |ev| {
                    let value = event_target_value(&ev);
                    let window = match value.as_str() {
                        "hour" => TimeWindow::LastHour,
                        "6hours" => TimeWindow::Last6Hours,
                        "day" => TimeWindow::LastDay,
                        "week" => TimeWindow::LastWeek,
                        "month" => TimeWindow::LastMonth,
                        _ => TimeWindow::All,
                    };
                    set_time_window.set(window);
                    set_offset.set(0); // Reset to current period
                }
            >
                <option value="hour" selected=move || time_window.get() == TimeWindow::LastHour>
                    "1 Hour"
                </option>
                <option value="6hours" selected=move || time_window.get() == TimeWindow::Last6Hours>
                    "6 Hours"
                </option>
                <option value="day" selected=move || time_window.get() == TimeWindow::LastDay>
                    "1 Day"
                </option>
                <option value="week" selected=move || time_window.get() == TimeWindow::LastWeek>
                    "1 Week"
                </option>
                <option value="month" selected=move || time_window.get() == TimeWindow::LastMonth>
                    "1 Month"
                </option>
                <option value="all" selected=move || time_window.get() == TimeWindow::All>
                    "All Time"
                </option>
            </select>

            // Navigation buttons (hidden for "All Time")
            {move || {
                if is_all_time() {
                    view! { <div></div> }.into_any()
                } else {
                    let step = step_label(&time_window.get());
                    view! {
                        <div class="join">
                            <button
                                class="join-item btn btn-sm"
                                disabled=move || !can_go_back()
                                title={format!("Previous {}", step)}
                                on:click=move |_| set_offset.update(|o| *o += 1)
                            >
                                "◀"
                            </button>
                            <button
                                class="join-item btn btn-sm"
                                class:btn-primary=move || offset.get() != 0
                                title="Go to current period"
                                on:click=move |_| set_offset.set(0)
                            >
                                "Now"
                            </button>
                            <button
                                class="join-item btn btn-sm"
                                disabled=move || !can_go_forward()
                                title={format!("Next {}", step)}
                                on:click=move |_| set_offset.update(|o| *o = (*o - 1).max(0))
                            >
                                "▶"
                            </button>
                        </div>
                    }.into_any()
                }
            }}

            // Date range display
            <div class="badge badge-ghost text-xs">
                {move || {
                    let (start, end) = time_range();
                    match start {
                        Some(s) => format!("{} - {}", format_date(s), format_date(end)),
                        None => "All available data".to_string()
                    }
                }}
            </div>

            // Period indicator
            {move || {
                let off = offset.get();
                if off > 0 && !is_all_time() {
                    let step = step_label(&time_window.get());
                    view! {
                        <div class="badge badge-warning text-xs">
                            {format!("{} {}s ago", off, step.to_lowercase())}
                        </div>
                    }.into_any()
                } else {
                    view! { <div></div> }.into_any()
                }
            }}
        </div>
    }
}

/// Export the AdvancedTimeSelector for backwards compatibility
pub use TimeSelectorWithNav as AdvancedTimeSelector;
