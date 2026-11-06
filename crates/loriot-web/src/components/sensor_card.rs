//! Sensor device card component

use leptos::prelude::*;
use loriot_core::current_timestamp_ms;
use loriot_core::DeviceStats;

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

/// Card displaying sensor device information
#[component]
pub fn SensorCard(stats: DeviceStats) -> impl IntoView {
    let name = stats.display_name().to_string();
    let device_id = stats.device_id.clone();

    // Current (last) values - shown prominently
    let last_temp = stats
        .last_temperature
        .map(|t| format!("{:.1}°C", t))
        .unwrap_or_else(|| "N/A".to_string());
    let last_humidity = stats
        .last_humidity
        .map(|h| format!("{:.1}%", h))
        .unwrap_or_else(|| "N/A".to_string());

    // Average values - shown smaller
    let avg_temp = stats
        .avg_temperature()
        .map(|t| format!("Ø {:.1}°C", t))
        .unwrap_or_default();
    let avg_humidity = stats
        .avg_humidity()
        .map(|h| format!("Ø {:.0}%", h))
        .unwrap_or_default();

    let battery = stats
        .battery_status
        .clone()
        .unwrap_or_else(|| "Unknown".to_string());
    let battery_voltage = stats
        .battery_voltage
        .map(|v| format!("{:.2}V", v))
        .unwrap_or_default();
    let next_expected_ms = stats.next_expected_reading();
    let grace_ms = 5 * 60 * 1000_u64;
    let total_readings = stats.total_readings;

    // Reactive countdown that ticks every second
    let (seconds_until, set_seconds_until) = signal(stats.seconds_until_next());

    Effect::new(move |_| {
        use gloo_timers::callback::Interval;
        let interval = Interval::new(1_000, move || {
            let now = current_timestamp_ms();
            let remaining = ((next_expected_ms as i64) - (now as i64)) / 1000;
            set_seconds_until.set(remaining);
        });
        interval.forget();
    });

    let countdown = move || format_countdown(seconds_until.get());
    let is_overdue = move || {
        let now = current_timestamp_ms();
        now > next_expected_ms + grace_ms
    };

    view! {
        <div class="card bg-base-100 shadow-xl">
            <div class="card-body">
                <h2 class="card-title">
                    {name}
                    {move || if is_overdue() {
                        view! { <div class="badge badge-warning gap-1">
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-4 h-4 stroke-current">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path>
                            </svg>
                            "Overdue"
                        </div> }.into_any()
                    } else {
                        view! { <div class="badge badge-success gap-1">
                            <div class="w-2 h-2 rounded-full bg-success animate-pulse"></div>
                            "Online"
                        </div> }.into_any()
                    }}
                </h2>

                <div class="grid grid-cols-2 gap-2 text-sm">
                    <div class="flex flex-col">
                        <span class="opacity-70 text-xs">"Temperature"</span>
                        <span class="font-bold text-xl text-primary">{last_temp}</span>
                        <span class="text-xs opacity-50">{avg_temp}</span>
                    </div>
                    <div class="flex flex-col">
                        <span class="opacity-70 text-xs">"Humidity"</span>
                        <span class="font-bold text-xl text-secondary">{last_humidity}</span>
                        <span class="text-xs opacity-50">{avg_humidity}</span>
                    </div>
                    <div class="flex justify-between">
                        <span class="opacity-70">"Battery:"</span>
                        <span class={if battery == "Low" || battery == "Critical" { "font-semibold text-error" } else { "font-semibold text-success" }}>
                            {battery.clone()} " " <span class="opacity-70">{battery_voltage}</span>
                        </span>
                    </div>
                    <div class="flex justify-between">
                        <span class="opacity-70">"Readings:"</span>
                        <span class="font-semibold">{total_readings}</span>
                    </div>
                </div>

                <div class="divider my-1"></div>

                <div class="flex justify-between items-center">
                    <div class="text-sm">
                        <span class="opacity-70">"Next packet: "</span>
                        <span class=move || if is_overdue() { "font-semibold text-warning" } else { "font-semibold text-info" }>
                            {countdown}
                        </span>
                    </div>
                    <a href={format!("/device/{}", device_id)} class="btn btn-primary btn-sm">
                        "Details"
                    </a>
                </div>
            </div>
        </div>
    }
}
