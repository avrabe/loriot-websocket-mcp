//! Room Temperature Forecast Page
//!
//! Displays weather forecast combined with room temperature predictions
//! based on thermal analysis of historical sensor data.

use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::api::{
    fetch_room_forecast, fetch_weather_info,
    ForecastResponse, RoomForecast, DailyForecastSummary,
    ThermalModel, WeatherInfo,
};
use crate::components::Navbar;

/// Forecast page component
#[component]
pub fn ForecastPage() -> impl IntoView {
    // Signals for data
    let forecast = RwSignal::new(None::<ForecastResponse>);
    let weather_info = RwSignal::new(None::<WeatherInfo>);
    let error = RwSignal::new(None::<String>);
    let loading = RwSignal::new(false);

    // Parameters
    let forecast_days = RwSignal::new(7u8);
    let history_days = RwSignal::new(90usize);

    // Load weather info on mount
    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(w) = fetch_weather_info().await {
                weather_info.set(Some(w));
            }
        });
    });

    // Fetch forecast action
    let load_forecast = move |_| {
        loading.set(true);
        error.set(None);
        let days = forecast_days.get();
        let hist = history_days.get();

        spawn_local(async move {
            match fetch_room_forecast(Some(days), None, Some(hist)).await {
                Ok(f) => forecast.set(Some(f)),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    view! {
        <div>
            <Navbar />

            <div class="container mx-auto p-4 space-y-6">
                <h1 class="text-3xl font-bold">"Temperatur-Vorhersage"</h1>
                <p class="text-base-content/70">
                    "Kombiniert Wettervorhersagen von Open-Meteo (DWD ICON) mit thermischen Modellen um Innentemperaturen vorherzusagen."
                </p>

                // Weather station info banner
                {move || weather_info.get().map(|w| {
                    let coords = w.location.as_ref().map(|l| format!("{:.4}°N, {:.4}°E", l.latitude, l.longitude)).unwrap_or_default();

                    view! {
                        <div class="alert alert-info">
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="stroke-current shrink-0 w-6 h-6">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
                            </svg>
                            <span>
                                "Wettervorhersage fur Standort: "
                                <strong>{coords}</strong>
                            </span>
                        </div>
                    }
                })}

                // Error display
                {move || error.get().map(|e| view! {
                    <div class="alert alert-error">
                        <span>{e}</span>
                    </div>
                })}

                // Controls card
                <div class="card bg-base-200">
                    <div class="card-body">
                        <h2 class="card-title">"Vorhersage konfigurieren"</h2>
                        <div class="flex flex-wrap items-end gap-4">
                            <div class="form-control">
                                <label class="label">
                                    <span class="label-text">"Vorhersage-Zeitraum"</span>
                                </label>
                                <select
                                    class="select select-bordered"
                                    on:change=move |ev| {
                                        let value: u8 = event_target_value(&ev).parse().unwrap_or(7);
                                        forecast_days.set(value);
                                    }
                                >
                                    <option value="3">"3 Tage"</option>
                                    <option value="7" selected>"7 Tage"</option>
                                    <option value="14">"14 Tage"</option>
                                </select>
                            </div>
                            <div class="form-control">
                                <label class="label">
                                    <span class="label-text">"Trainingsdaten"</span>
                                </label>
                                <select
                                    class="select select-bordered"
                                    on:change=move |ev| {
                                        let value: usize = event_target_value(&ev).parse().unwrap_or(90);
                                        history_days.set(value);
                                    }
                                >
                                    <option value="30">"30 Tage"</option>
                                    <option value="60">"60 Tage"</option>
                                    <option value="90" selected>"90 Tage"</option>
                                    <option value="180">"180 Tage"</option>
                                </select>
                            </div>
                            <button
                                class="btn btn-primary"
                                on:click=load_forecast
                                disabled=move || loading.get()
                            >
                                {move || if loading.get() {
                                    view! { <span class="loading loading-spinner"></span> }.into_any()
                                } else {
                                    view! { "Vorhersage laden" }.into_any()
                                }}
                            </button>
                        </div>
                    </div>
                </div>

                // Loading indicator
                <Show when=move || loading.get()>
                    <div class="flex justify-center py-8">
                        <span class="loading loading-spinner loading-lg"></span>
                    </div>
                </Show>

                // Results
                {move || forecast.get().map(|f| {
                    let weather_daily = f.weather.daily.clone();
                    let rooms = f.rooms.clone();
                    let rooms_for_summary = f.rooms.clone();

                    view! {
                        // Summary stats
                        <ForecastSummaryStats weather=weather_daily.clone() rooms=rooms_for_summary />

                        // Weather forecast timeline
                        <WeatherForecastTimeline daily=weather_daily />

                        // Room predictions
                        <div class="space-y-4">
                            <h2 class="text-2xl font-bold">"Raumtemperatur-Vorhersagen"</h2>
                            {rooms.into_iter().map(|room| {
                                view! { <RoomForecastCard room=room /> }
                            }).collect::<Vec<_>>()}
                        </div>
                    }
                })}
            </div>
        </div>
    }
}

/// Summary statistics cards
#[component]
fn ForecastSummaryStats(weather: Vec<DailyForecastSummary>, rooms: Vec<RoomForecast>) -> impl IntoView {
    // Calculate summary stats
    let outdoor_min = weather.iter().map(|d| d.temp_min).fold(f32::MAX, f32::min);
    let outdoor_max = weather.iter().map(|d| d.temp_max).fold(f32::MIN, f32::max);
    let outdoor_avg = if !weather.is_empty() {
        weather.iter().map(|d| d.temp_mean).sum::<f32>() / weather.len() as f32
    } else { 0.0 };

    let precip_total = weather.iter().map(|d| d.precipitation).sum::<f32>();

    // Room predictions summary
    let (room_min, room_max) = if !rooms.is_empty() {
        let mins: Vec<f32> = rooms.iter()
            .flat_map(|r| r.daily_predictions.iter().map(|p| p.predicted_range.0))
            .collect();
        let maxs: Vec<f32> = rooms.iter()
            .flat_map(|r| r.daily_predictions.iter().map(|p| p.predicted_range.1))
            .collect();
        (
            mins.iter().cloned().fold(f32::MAX, f32::min),
            maxs.iter().cloned().fold(f32::MIN, f32::max),
        )
    } else {
        (0.0, 0.0)
    };

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Vorhersage-Ubersicht"</h2>

                <div class="stats stats-vertical lg:stats-horizontal shadow w-full">
                    // Outdoor stats
                    <div class="stat">
                        <div class="stat-figure text-info">
                            <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 15a4 4 0 004 4h9a5 5 0 10-.1-9.999 5.002 5.002 0 10-9.78 2.096A4.001 4.001 0 003 15z" />
                            </svg>
                        </div>
                        <div class="stat-title">"Aussen (Durchschnitt)"</div>
                        <div class="stat-value text-info">{format!("{:.1}°C", outdoor_avg)}</div>
                        <div class="stat-desc">{format!("{:.1}°C bis {:.1}°C", outdoor_min, outdoor_max)}</div>
                    </div>

                    // Indoor prediction
                    <div class="stat">
                        <div class="stat-figure text-primary">
                            <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
                            </svg>
                        </div>
                        <div class="stat-title">"Innen (Vorhersage)"</div>
                        <div class="stat-value text-primary">{format!("{:.1}°C - {:.1}°C", room_min, room_max)}</div>
                        <div class="stat-desc">{format!("{} Raume", rooms.len())}</div>
                    </div>

                    // Precipitation
                    <div class="stat">
                        <div class="stat-figure text-secondary">
                            <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19.428 15.428a2 2 0 00-1.022-.547l-2.387-.477a6 6 0 00-3.86.517l-.318.158a6 6 0 01-3.86.517L6.05 15.21a2 2 0 00-1.806.547M8 4h8l-1 1v5.172a2 2 0 00.586 1.414l5 5c1.26 1.26.367 3.414-1.415 3.414H4.828c-1.782 0-2.674-2.154-1.414-3.414l5-5A2 2 0 009 10.172V5L8 4z" />
                            </svg>
                        </div>
                        <div class="stat-title">"Niederschlag"</div>
                        <div class="stat-value">{format!("{:.1} mm", precip_total)}</div>
                        <div class="stat-desc">{format!("{} Tage", weather.len())}</div>
                    </div>

                    // Forecast days
                    <div class="stat">
                        <div class="stat-figure text-accent">
                            <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
                            </svg>
                        </div>
                        <div class="stat-title">"Vorhersage"</div>
                        <div class="stat-value">{weather.len()}</div>
                        <div class="stat-desc">"Tage"</div>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Weather forecast timeline with visual bars
#[component]
fn WeatherForecastTimeline(daily: Vec<DailyForecastSummary>) -> impl IntoView {
    // Find min/max for scaling
    let global_min = daily.iter().map(|d| d.temp_min).fold(f32::MAX, f32::min) - 2.0;
    let global_max = daily.iter().map(|d| d.temp_max).fold(f32::MIN, f32::max) + 2.0;
    let range = (global_max - global_min).max(1.0);

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Wettervorhersage"</h2>

                <div class="grid grid-cols-1 md:grid-cols-7 gap-2">
                    {daily.iter().map(|d| {
                        let date_display = format_date_short(&d.date);
                        let weekday = weekday_name(&d.date);

                        // Calculate bar positions (percentage from bottom)
                        let min_pos = ((d.temp_min - global_min) / range * 100.0).clamp(0.0, 100.0);
                        let max_pos = ((d.temp_max - global_min) / range * 100.0).clamp(0.0, 100.0);
                        let bar_height = (max_pos - min_pos).max(5.0);

                        let is_rainy = d.precipitation > 0.5;
                        let temp_class = temp_color_class(d.temp_mean);

                        view! {
                            <div class="card bg-base-100 p-3">
                                <div class="text-center">
                                    <div class="font-bold">{weekday}</div>
                                    <div class="text-xs text-base-content/50">{date_display}</div>
                                </div>

                                // Temperature bar visualization
                                <div class="relative h-24 my-2 flex justify-center">
                                    <div class="w-8 bg-base-300 rounded-full relative overflow-hidden">
                                        <div
                                            class="absolute w-full bg-gradient-to-t from-info via-success to-error rounded-full"
                                            style:bottom=format!("{}%", min_pos)
                                            style:height=format!("{}%", bar_height)
                                        ></div>
                                    </div>
                                </div>

                                <div class="text-center space-y-1">
                                    <div class="text-error text-sm font-bold">{format!("{:.0}°", d.temp_max)}</div>
                                    <div class=format!("text-lg font-bold {}", temp_class)>{format!("{:.0}°", d.temp_mean)}</div>
                                    <div class="text-info text-sm font-bold">{format!("{:.0}°", d.temp_min)}</div>
                                </div>

                                // Weather icon
                                <div class="text-center mt-2">
                                    {if is_rainy {
                                        view! { <span class="text-xl">"🌧️"</span> }.into_any()
                                    } else if d.humidity > 80.0 {
                                        view! { <span class="text-xl">"☁️"</span> }.into_any()
                                    } else {
                                        view! { <span class="text-xl">"☀️"</span> }.into_any()
                                    }}
                                    {(d.precipitation > 0.0).then(|| view! {
                                        <div class="text-xs text-base-content/50">{format!("{:.1}mm", d.precipitation)}</div>
                                    })}
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </div>
        </div>
    }
}

/// Room forecast card with improved layout
#[component]
fn RoomForecastCard(room: RoomForecast) -> impl IntoView {
    let name = room.device_name.clone().unwrap_or_else(|| room.device_id.clone());
    let device_id = room.device_id.clone();
    let model = room.model.clone();
    let predictions = room.daily_predictions.clone();

    // Calculate average confidence
    let avg_confidence = if !predictions.is_empty() {
        predictions.iter().map(|p| p.confidence).sum::<f32>() / predictions.len() as f32
    } else { 0.0 };

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                // Header with name and model info
                <div class="flex flex-wrap items-start justify-between gap-4">
                    <div>
                        <h3 class="card-title">{name}</h3>
                        <p class="text-sm text-base-content/50 font-mono">{device_id}</p>
                    </div>
                    <ThermalModelInfo model=model avg_confidence=avg_confidence />
                </div>

                // Prediction grid - visual cards for each day
                <div class="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-7 gap-2 mt-4">
                    {predictions.iter().map(|p| {
                        let date_display = format_date_short(&p.date);
                        let weekday = weekday_name(&p.date);
                        let confidence_color = confidence_color_class(p.confidence);
                        let temp_diff = p.predicted_temp - p.outdoor_temp;

                        view! {
                            <div class="card bg-base-100 p-3">
                                <div class="text-center">
                                    <div class="text-xs font-medium text-base-content/50">{weekday}</div>
                                    <div class="text-xs text-base-content/30">{date_display}</div>
                                </div>

                                // Comparison visualization
                                <div class="my-3 space-y-2">
                                    // Outdoor temp
                                    <div class="flex items-center justify-between text-sm">
                                        <span class="text-info">"A:"</span>
                                        <span class="font-mono">{format!("{:.1}°", p.outdoor_temp)}</span>
                                    </div>

                                    // Arrow indicating difference
                                    <div class="flex justify-center">
                                        {if temp_diff > 0.0 {
                                            view! { <span class="text-success">"↑ +" {format!("{:.1}K", temp_diff)}</span> }.into_any()
                                        } else {
                                            view! { <span class="text-warning">"↓ " {format!("{:.1}K", temp_diff)}</span> }.into_any()
                                        }}
                                    </div>

                                    // Indoor prediction (main)
                                    <div class="flex items-center justify-between">
                                        <span class="text-primary font-medium">"I:"</span>
                                        <span class="font-mono text-lg font-bold text-primary">
                                            {format!("{:.1}°", p.predicted_temp)}
                                        </span>
                                    </div>
                                </div>

                                // Range
                                <div class="text-center text-xs text-base-content/50">
                                    {format!("{:.1}° - {:.1}°", p.predicted_range.0, p.predicted_range.1)}
                                </div>

                                // Confidence bar
                                <div class="mt-2">
                                    <div class="w-full bg-base-300 rounded-full h-1">
                                        <div
                                            class=format!("h-1 rounded-full {}", confidence_bar_class(p.confidence))
                                            style:width=format!("{}%", p.confidence)
                                        ></div>
                                    </div>
                                    <div class=format!("text-xs text-center mt-1 {}", confidence_color)>
                                        {format!("{:.0}%", p.confidence)}
                                    </div>
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </div>
        </div>
    }
}

/// Thermal model info component
#[component]
fn ThermalModelInfo(model: ThermalModel, avg_confidence: f32) -> impl IntoView {
    let damping_rating = if model.damping_factor < 0.15 {
        ("Exzellent", "badge-success")
    } else if model.damping_factor < 0.3 {
        ("Gut", "badge-info")
    } else if model.damping_factor < 0.5 {
        ("Moderat", "badge-warning")
    } else {
        ("Hoch", "badge-error")
    };

    view! {
        <div class="flex flex-col items-end gap-2">
            // Main confidence indicator
            <div class="flex items-center gap-2">
                <span class="text-sm text-base-content/50">"Modell-Konfidenz:"</span>
                <div class="radial-progress text-primary" style:--value=format!("{}", avg_confidence as i32) style:--size="2.5rem" style:--thickness="3px" role="progressbar">
                    <span class="text-xs font-bold">{format!("{:.0}%", avg_confidence)}</span>
                </div>
            </div>

            // Model parameters
            <div class="flex flex-wrap gap-2 justify-end">
                <div class="tooltip tooltip-left" data-tip="Thermische Dampfung - niedriger = bessere Warmedammung/Masse">
                    <span class=format!("badge badge-sm {}", damping_rating.1)>
                        {"Dampfung: "}{damping_rating.0}
                    </span>
                </div>
                <div class="tooltip tooltip-left" data-tip="Basis-Temperaturunterschied Innen/Aussen">
                    <span class="badge badge-sm badge-ghost">
                        {"Offset: "}{format!("{:+.1}K", model.base_offset)}
                    </span>
                </div>
                <div class="tooltip tooltip-left" data-tip="Anzahl Tage mit Trainingsdaten">
                    <span class="badge badge-sm badge-ghost">
                        {model.training_days}{" Tage"}</span>
                </div>
            </div>
        </div>
    }
}

/// Format date from YYYYMMDD to short format
fn format_date_short(date: &str) -> String {
    if date.len() >= 8 {
        format!("{}.{}", &date[6..8], &date[4..6])
    } else {
        date.to_string()
    }
}

/// Get weekday name from YYYYMMDD
fn weekday_name(date: &str) -> &'static str {
    if date.len() >= 8 {
        // Simple calculation - not perfect but works for recent dates
        let year: i32 = date[0..4].parse().unwrap_or(2025);
        let month: i32 = date[4..6].parse().unwrap_or(1);
        let day: i32 = date[6..8].parse().unwrap_or(1);

        // Zeller's congruence (simplified)
        let m = if month < 3 { month + 12 } else { month };
        let y = if month < 3 { year - 1 } else { year };
        let k = y % 100;
        let j = y / 100;
        let h = (day + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 - 2 * j) % 7;
        let d = ((h + 5) % 7) as usize; // Convert to Monday=0

        ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"][d]
    } else {
        "-"
    }
}

/// Get color class for temperature
fn temp_color_class(temp: f32) -> &'static str {
    if temp < 0.0 {
        "text-info"
    } else if temp < 10.0 {
        "text-base-content"
    } else if temp < 20.0 {
        "text-success"
    } else if temp < 25.0 {
        "text-warning"
    } else {
        "text-error"
    }
}

/// Get color class for confidence
fn confidence_color_class(confidence: f32) -> &'static str {
    if confidence >= 80.0 {
        "text-success"
    } else if confidence >= 60.0 {
        "text-info"
    } else if confidence >= 40.0 {
        "text-warning"
    } else {
        "text-error"
    }
}

/// Get bar color class for confidence
fn confidence_bar_class(confidence: f32) -> &'static str {
    if confidence >= 80.0 {
        "bg-success"
    } else if confidence >= 60.0 {
        "bg-info"
    } else if confidence >= 40.0 {
        "bg-warning"
    } else {
        "bg-error"
    }
}
