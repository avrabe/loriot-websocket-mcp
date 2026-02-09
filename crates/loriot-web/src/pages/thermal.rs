//! Thermal Analysis Page
//!
//! Correlates indoor sensor data with outdoor weather to analyze
//! building thermal performance and insulation quality.

use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::api::{
    fetch_thermal_analysis, fetch_weather_info,
    ThermalAnalysisReport, DeviceThermalAnalysis, DailyThermalProfile,
    BuildingThermalMetrics, ThermalDataQuality, WeatherInfo,
};
use crate::components::Navbar;

/// Thermal analysis page component
#[component]
pub fn ThermalPage() -> impl IntoView {
    // Signals for data
    let report = RwSignal::new(None::<ThermalAnalysisReport>);
    let weather_info = RwSignal::new(None::<WeatherInfo>);
    let error = RwSignal::new(None::<String>);
    let loading = RwSignal::new(false);

    // Analysis parameters
    let days = RwSignal::new(30usize);
    let selected_device = RwSignal::new(None::<String>);

    // Load initial data
    Effect::new(move |_| {
        loading.set(true);
        spawn_local(async move {
            // Fetch weather info for station display
            if let Ok(w) = fetch_weather_info().await {
                weather_info.set(Some(w));
            }
            loading.set(false);
        });
    });

    // Run analysis action
    let run_analysis = move |_| {
        loading.set(true);
        error.set(None);
        let days_val = days.get();
        let device = selected_device.get();

        spawn_local(async move {
            match fetch_thermal_analysis(Some(days_val), device.as_deref()).await {
                Ok(r) => report.set(Some(r)),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    view! {
        <div>
            <Navbar />

            <div class="container mx-auto p-4 space-y-6">
                <h1 class="text-3xl font-bold">"Thermal Analysis"</h1>
                <p class="text-base-content/70">
                    "Correlates indoor sensor data with outdoor weather to analyze building thermal performance and insulation quality."
                </p>

                // Weather station info banner
                {move || weather_info.get().map(|w| {
                    let station_name = w.station.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                    let station_id = w.station.as_ref().map(|s| s.id.clone()).unwrap_or_default();
                    let distance = w.station_distance_km.map(|d| format!("{:.1} km", d)).unwrap_or_else(|| "-".to_string());

                    view! {
                        <div class="alert alert-info">
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="stroke-current shrink-0 w-6 h-6">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
                            </svg>
                            <span>
                                "Outdoor weather data from DWD station: "
                                <strong>{station_name}</strong>
                                " (ID: " {station_id} ", " {distance} " away)"
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

                // Analysis controls
                <div class="card bg-base-200">
                    <div class="card-body">
                        <h2 class="card-title">"Run Thermal Analysis"</h2>
                        <div class="flex flex-wrap items-end gap-4">
                            <div class="form-control">
                                <label class="label">
                                    <span class="label-text">"Analysis Period"</span>
                                </label>
                                <select
                                    class="select select-bordered"
                                    on:change=move |ev| {
                                        let value: usize = event_target_value(&ev).parse().unwrap_or(30);
                                        days.set(value);
                                    }
                                >
                                    <option value="7">"7 days"</option>
                                    <option value="14">"14 days"</option>
                                    <option value="30" selected>"30 days"</option>
                                    <option value="60">"60 days"</option>
                                    <option value="90">"90 days"</option>
                                    <option value="180">"180 days"</option>
                                    <option value="365">"1 year"</option>
                                </select>
                            </div>
                            <button
                                class="btn btn-primary"
                                on:click=run_analysis
                                disabled=move || loading.get()
                            >
                                {move || if loading.get() {
                                    view! { <span class="loading loading-spinner"></span> }.into_any()
                                } else {
                                    view! { "Analyze Thermal Performance" }.into_any()
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
                {move || report.get().map(|r| view! {
                    <BuildingMetricsCard metrics=r.building_metrics.clone() />
                    <DataQualityCard quality=r.data_quality.clone() />
                    <DegreeDaysCard
                        heating_dd=r.heating_degree_days
                        cooling_dd=r.cooling_degree_days
                    />
                    <DeviceAnalysisList
                        devices=r.device_analysis.clone()
                    />
                })}
            </div>
        </div>
    }
}

/// Building-wide thermal metrics card
#[component]
fn BuildingMetricsCard(metrics: BuildingThermalMetrics) -> impl IntoView {
    let insulation_color = match metrics.insulation_rating.as_str() {
        "Excellent" => "text-success",
        "Good" => "text-info",
        "Average" => "text-warning",
        "Poor" => "text-error",
        _ => "text-base-content/50",
    };

    let insulation_description = match metrics.insulation_rating.as_str() {
        "Excellent" => "Ausgezeichnet - Passivhaus-Niveau",
        "Good" => "Gut - Moderne Baustandards",
        "Average" => "Durchschnittlich - Typischer Altbau",
        "Poor" => "Verbesserungswürdig - Deutlicher Wärmeverlust",
        _ => "Unbekannt - Unzureichende Daten",
    };

    let insulation_rating = metrics.insulation_rating.clone();

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Building Thermal Performance"</h2>

                <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
                    // Insulation Rating
                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-title">"Insulation Rating"</div>
                        <div class=format!("stat-value text-2xl {}", insulation_color)>
                            {insulation_rating}
                        </div>
                        <div class="stat-desc">{insulation_description}</div>
                    </div>

                    // Average Indoor Temp
                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-title">"Avg Indoor Temp"</div>
                        <div class="stat-value text-2xl">
                            {format!("{:.1}°C", metrics.avg_indoor_temp)}
                        </div>
                        <div class="stat-desc">"Across all sensors"</div>
                    </div>

                    // Average Outdoor Temp
                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-title">"Avg Outdoor Temp"</div>
                        <div class="stat-value text-2xl">
                            {metrics.avg_outdoor_temp.map(|t| format!("{:.1}°C", t)).unwrap_or("-".to_string())}
                        </div>
                        <div class="stat-desc">"From DWD weather station"</div>
                    </div>

                    // Temperature Difference
                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-title">"Temp Difference"</div>
                        <div class="stat-value text-2xl">
                            {metrics.avg_temp_difference.map(|d| {
                                let sign = if d >= 0.0 { "+" } else { "" };
                                format!("{}{}K", sign, d as i32)
                            }).unwrap_or("-".to_string())}
                        </div>
                        <div class="stat-desc">"Indoor - Outdoor"</div>
                    </div>
                </div>

                <div class="grid grid-cols-2 md:grid-cols-3 gap-4 mt-4">
                    // Stability Score
                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-title">"Temperature Stability"</div>
                        <div class="stat-value text-xl">
                            {format!("{:.0}/100", metrics.stability_score)}
                        </div>
                        <div class="stat-desc">"Higher = more stable"</div>
                    </div>

                    // Uniformity Score
                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-title">"Room Uniformity"</div>
                        <div class="stat-value text-xl">
                            {format!("{:.0}%", metrics.uniformity_score * 100.0)}
                        </div>
                        <div class="stat-desc">"Temperature consistency"</div>
                    </div>

                    // Energy Demand Indicator
                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-title">"Energy Demand"</div>
                        <div class="stat-value text-xl">
                            {format!("{:.1}", metrics.energy_demand_indicator)}
                        </div>
                        <div class="stat-desc">"Relative indicator"</div>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Data quality card
#[component]
fn DataQualityCard(quality: ThermalDataQuality) -> impl IntoView {
    let quality_color = match quality.quality_rating.as_str() {
        "Excellent" => "badge-success",
        "Good" => "badge-info",
        "Fair" => "badge-warning",
        _ => "badge-error",
    };
    let quality_rating = quality.quality_rating.clone();

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">
                    "Data Quality"
                    <span class=format!("badge {}", quality_color)>
                        {quality_rating}
                    </span>
                </h2>

                <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
                    <div>
                        <span class="text-sm text-base-content/70">"Coverage"</span>
                        <p class="text-lg font-bold">{format!("{:.0}%", quality.coverage_percent)}</p>
                    </div>
                    <div>
                        <span class="text-sm text-base-content/70">"Readings/Day"</span>
                        <p class="text-lg font-bold">{format!("{:.1}", quality.avg_readings_per_day)}</p>
                    </div>
                    <div>
                        <span class="text-sm text-base-content/70">"Devices"</span>
                        <p class="text-lg font-bold">{quality.devices_with_data}</p>
                    </div>
                    <div>
                        <span class="text-sm text-base-content/70">"Weather Data"</span>
                        <p class="text-lg font-bold">
                            {if quality.weather_data_available { "Available" } else { "Missing" }}
                        </p>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Degree days card
#[component]
fn DegreeDaysCard(heating_dd: f64, cooling_dd: f64) -> impl IntoView {
    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Heating & Cooling Degree Days"</h2>
                <p class="text-base-content/70">
                    "Degree days measure heating/cooling demand. Higher values indicate more energy needed."
                </p>

                <div class="grid grid-cols-2 gap-4 mt-4">
                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-figure text-primary">
                            <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17.657 18.657A8 8 0 016.343 7.343S7 9 9 10c0-2 .5-5 2.986-7C14 5 16.09 5.777 17.656 7.343A7.975 7.975 0 0120 13a7.975 7.975 0 01-2.343 5.657z" />
                            </svg>
                        </div>
                        <div class="stat-title">"Heating Degree Days"</div>
                        <div class="stat-value text-primary">{format!("{:.0}", heating_dd)}</div>
                        <div class="stat-desc">"Base: 15°C"</div>
                    </div>

                    <div class="stat bg-base-100 rounded-lg">
                        <div class="stat-figure text-info">
                            <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4" />
                            </svg>
                        </div>
                        <div class="stat-title">"Cooling Degree Days"</div>
                        <div class="stat-value text-info">{format!("{:.0}", cooling_dd)}</div>
                        <div class="stat-desc">"Base: 18°C"</div>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Device analysis list
#[component]
fn DeviceAnalysisList(
    devices: Vec<DeviceThermalAnalysis>,
    #[prop(optional)] _selected: Option<RwSignal<Option<String>>>,
) -> impl IntoView {
    let devices_for_check = devices.clone();

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Per-Device Analysis"</h2>

                <Show
                    when=move || !devices_for_check.is_empty()
                    fallback=|| view! { <p class="text-base-content/70">"No device data available"</p> }
                >
                    <div class="overflow-x-auto">
                        <table class="table table-zebra">
                            <thead>
                                <tr>
                                    <th>"Device"</th>
                                    <th class="text-right">"Avg Temp"</th>
                                    <th class="text-right">"Range"</th>
                                    <th class="text-right">"Stability"</th>
                                    <th class="text-right">"Outdoor Corr."</th>
                                    <th class="text-right">"Damping"</th>
                                    <th class="text-right">"Temp Diff"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {devices.iter().map(|d| {
                                    let name = d.device_name.clone().unwrap_or_else(|| d.device_id.clone());
                                    let device_id = d.device_id.clone();
                                    let correlation_color = if d.outdoor_correlation.abs() > 0.7 {
                                        "text-error"
                                    } else if d.outdoor_correlation.abs() > 0.4 {
                                        "text-warning"
                                    } else {
                                        "text-success"
                                    };
                                    let damping_color = if d.damping_factor > 0.4 {
                                        "text-error"
                                    } else if d.damping_factor > 0.2 {
                                        "text-warning"
                                    } else {
                                        "text-success"
                                    };

                                    view! {
                                        <tr>
                                            <td>
                                                <div class="font-medium">{name}</div>
                                                <div class="text-xs text-base-content/50">{device_id}</div>
                                            </td>
                                            <td class="text-right font-mono">
                                                {format!("{:.1}°C", d.avg_indoor_temp)}
                                            </td>
                                            <td class="text-right font-mono text-sm">
                                                {format!("{:.1} - {:.1}°C", d.indoor_temp_range.0, d.indoor_temp_range.1)}
                                            </td>
                                            <td class="text-right font-mono">
                                                {format!("±{:.1}°C", d.temp_stability)}
                                            </td>
                                            <td class=format!("text-right font-mono {}", correlation_color)>
                                                {format!("{:.2}", d.outdoor_correlation)}
                                            </td>
                                            <td class=format!("text-right font-mono {}", damping_color)>
                                                {format!("{:.2}", d.damping_factor)}
                                            </td>
                                            <td class="text-right font-mono">
                                                {format!("{:+.1}K", d.avg_temp_difference)}
                                            </td>
                                        </tr>
                                    }
                                }).collect::<Vec<_>>()}
                            </tbody>
                        </table>
                    </div>

                    <div class="mt-4 text-sm text-base-content/70">
                        <p><strong>"Outdoor Correlation:"</strong>" Lower values (closer to 0) indicate better insulation - indoor temps don't follow outdoor temps."</p>
                        <p><strong>"Damping Factor:"</strong>" Lower values indicate better thermal mass - indoor temp swings are smaller than outdoor."</p>
                    </div>
                </Show>
            </div>
        </div>
    }
}

/// Device detail with daily profiles (expandable)
/// Reserved for future per-device detail expansion
#[allow(dead_code)]
#[component]
fn DeviceDetailCard(device: DeviceThermalAnalysis) -> impl IntoView {
    let expanded = RwSignal::new(false);
    let name = device.device_name.clone().unwrap_or_else(|| device.device_id.clone());

    view! {
        <div class="collapse collapse-arrow bg-base-100">
            <input
                type="checkbox"
                prop:checked=move || expanded.get()
                on:change=move |_| expanded.update(|e| *e = !*e)
            />
            <div class="collapse-title text-lg font-medium">
                {name}
                <span class="ml-4 text-sm text-base-content/70">
                    {format!("Avg: {:.1}°C | Correlation: {:.2}", device.avg_indoor_temp, device.outdoor_correlation)}
                </span>
            </div>
            <div class="collapse-content">
                <Show when=move || expanded.get()>
                    <DailyProfilesTable profiles=device.daily_profiles.clone() />
                </Show>
            </div>
        </div>
    }
}

/// Daily profiles table
#[component]
fn DailyProfilesTable(profiles: Vec<DailyThermalProfile>) -> impl IntoView {
    view! {
        <div class="overflow-x-auto max-h-96 overflow-y-auto">
            <table class="table table-xs">
                <thead class="sticky top-0 bg-base-200">
                    <tr>
                        <th>"Date"</th>
                        <th class="text-right">"Indoor Avg"</th>
                        <th class="text-right">"Indoor Range"</th>
                        <th class="text-right">"Outdoor"</th>
                        <th class="text-right">"Outdoor Range"</th>
                        <th class="text-right">"Difference"</th>
                        <th class="text-right">"Readings"</th>
                    </tr>
                </thead>
                <tbody>
                    {profiles.iter().map(|p| {
                        let date_display = format!("{}-{}-{}", &p.date[0..4], &p.date[4..6], &p.date[6..8]);

                        view! {
                            <tr>
                                <td>{date_display}</td>
                                <td class="text-right font-mono">{format!("{:.1}°C", p.avg_indoor_temp)}</td>
                                <td class="text-right font-mono text-xs">
                                    {format!("{:.1}-{:.1}", p.indoor_range.0, p.indoor_range.1)}
                                </td>
                                <td class="text-right font-mono">
                                    {p.outdoor_temp.map(|t| format!("{:.1}°C", t)).unwrap_or("-".to_string())}
                                </td>
                                <td class="text-right font-mono text-xs">
                                    {p.outdoor_range.map(|(min, max)| format!("{:.1}-{:.1}", min, max)).unwrap_or("-".to_string())}
                                </td>
                                <td class="text-right font-mono">
                                    {p.temp_difference.map(|d| format!("{:+.1}K", d)).unwrap_or("-".to_string())}
                                </td>
                                <td class="text-right">{p.reading_count}</td>
                            </tr>
                        }
                    }).collect::<Vec<_>>()}
                </tbody>
            </table>
        </div>
    }
}
