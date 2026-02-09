//! Heat Pump Analytics Page

use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::api::{
    fetch_heatpump_info, fetch_heatpump_devices, configure_heatpump, analyze_heatpump,
    fetch_weather_info,
    HeatPumpInfo, HeatPumpReport, DeviceOption, ConfigureHeatPumpRequest,
    SeasonSummary, DailyHeatPumpAnalysis, WeatherInfo,
};
use crate::components::Navbar;

/// Heat Pump analytics page component
#[component]
pub fn HeatPumpPage() -> impl IntoView {
    // Signals for data
    let info = RwSignal::new(None::<HeatPumpInfo>);
    let devices = RwSignal::new(Vec::<DeviceOption>::new());
    let report = RwSignal::new(None::<HeatPumpReport>);
    let weather_info = RwSignal::new(None::<WeatherInfo>);
    let error = RwSignal::new(None::<String>);
    let loading = RwSignal::new(false);
    let analyzing = RwSignal::new(false);

    // Configuration form signals
    let selected_devices = RwSignal::new(Vec::<String>::new());
    let days = RwSignal::new(30usize);

    // Load initial data
    Effect::new(move |_| {
        loading.set(true);
        spawn_local(async move {
            // Fetch heat pump info
            match fetch_heatpump_info().await {
                Ok(i) => {
                    // Pre-select configured devices
                    selected_devices.set(i.config.source_devices.clone());
                    info.set(Some(i));
                }
                Err(e) => error.set(Some(e)),
            }

            // Fetch devices
            match fetch_heatpump_devices().await {
                Ok(d) => devices.set(d),
                Err(e) => error.set(Some(e)),
            }

            // Fetch weather info for station display
            if let Ok(w) = fetch_weather_info().await {
                weather_info.set(Some(w));
            }

            loading.set(false);
        });
    });

    // Run analysis action
    let run_analysis = move |_| {
        analyzing.set(true);
        error.set(None);
        let days_val = days.get();

        spawn_local(async move {
            match analyze_heatpump(Some(days_val)).await {
                Ok(r) => report.set(Some(r)),
                Err(e) => error.set(Some(e)),
            }
            analyzing.set(false);
        });
    };

    // Configure action
    let save_config = move |_| {
        loading.set(true);
        error.set(None);
        let devices = selected_devices.get();

        spawn_local(async move {
            let config = ConfigureHeatPumpRequest {
                source_devices: devices,
                device_names: None,
                heating_target: None,
                cooling_target: None,
            };

            match configure_heatpump(config).await {
                Ok(i) => info.set(Some(i)),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    view! {
        <div>
            <Navbar />

            <div class="container mx-auto p-4 space-y-6">
                <h1 class="text-3xl font-bold">"Heat Pump Analytics"</h1>

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

                // Loading indicator
                <Show when=move || loading.get()>
                    <div class="flex justify-center">
                        <span class="loading loading-spinner loading-lg"></span>
                    </div>
                </Show>

                // Configuration card
                <ConfigurationCard
                    devices=devices
                    selected=selected_devices
                    on_save=save_config
                    info=info
                />

                // Analysis controls
                <div class="card bg-base-200">
                    <div class="card-body">
                        <h2 class="card-title">"Run Analysis"</h2>
                        <div class="flex flex-wrap items-end gap-4">
                            <div class="form-control">
                                <label class="label">
                                    <span class="label-text">"Analysis Period (days)"</span>
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
                                    <option value="365">"365 days"</option>
                                </select>
                            </div>
                            <button
                                class="btn btn-primary"
                                on:click=run_analysis
                                disabled=move || analyzing.get() || info.get().map(|i| !i.has_data).unwrap_or(true)
                            >
                                {move || if analyzing.get() {
                                    view! { <span class="loading loading-spinner"></span> }.into_any()
                                } else {
                                    view! { "Analyze" }.into_any()
                                }}
                            </button>
                        </div>
                        <Show when=move || info.get().map(|i| !i.has_data).unwrap_or(false)>
                            <p class="text-warning">"Configure source devices first to enable analysis"</p>
                        </Show>
                    </div>
                </div>

                // Results
                {move || report.get().map(|r| view! {
                    <OverallStatsCards report=r.clone() />
                    <SeasonalBreakdown report=r.clone() />
                    <DeviceStatsTable report=r.clone() />
                    <DailyAnalysisTable report=r />
                })}
            </div>
        </div>
    }
}

/// Configuration card component
#[component]
fn ConfigurationCard(
    devices: RwSignal<Vec<DeviceOption>>,
    selected: RwSignal<Vec<String>>,
    on_save: impl Fn(()) + 'static + Copy,
    info: RwSignal<Option<HeatPumpInfo>>,
) -> impl IntoView {
    let toggle_device = move |eui: String| {
        let mut current = selected.get();
        if current.contains(&eui) {
            current.retain(|e| e != &eui);
        } else {
            current.push(eui);
        }
        selected.set(current);
    };

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Heat Source Configuration"</h2>
                <p class="text-sm opacity-70">
                    "Select underground sensors to use as heat source reference (e.g., garage, basement)"
                </p>

                // Current config display
                {move || info.get().map(|i| view! {
                    <div class="flex gap-4 text-sm">
                        <span>"Heating target: " <strong>{format!("{:.1}°C", i.config.heating_target)}</strong></span>
                        <span>"Cooling target: " <strong>{format!("{:.1}°C", i.config.cooling_target)}</strong></span>
                    </div>
                })}

                // Device selection
                <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mt-4">
                    <For
                        each=move || devices.get()
                        key=|d| d.eui.clone()
                        children=move |device| {
                            let eui = device.eui.clone();
                            let is_selected = Signal::derive(move || selected.get().contains(&eui));

                            view! {
                                <DeviceCard
                                    device=device.clone()
                                    is_selected=is_selected
                                    eui=device.eui.clone()
                                    toggle_device=toggle_device
                                />
                            }
                        }
                    />
                </div>

                <div class="card-actions justify-end mt-4">
                    <button class="btn btn-primary" on:click=move |_| on_save(())>
                        "Save Configuration"
                    </button>
                </div>
            </div>
        </div>
    }
}

/// Individual device card for selection
#[component]
fn DeviceCard(
    device: DeviceOption,
    #[prop(into)] is_selected: Signal<bool>,
    eui: String,
    toggle_device: impl Fn(String) + 'static + Copy,
) -> impl IntoView {
    // Check if name is different from EUI (meaning it has a friendly name)
    let has_friendly_name = device.name != device.eui;
    let display_name = if has_friendly_name {
        device.name.clone()
    } else {
        // No friendly name, show shortened EUI
        if device.eui.len() > 8 {
            format!("...{}", &device.eui[device.eui.len()-8..])
        } else {
            device.eui.clone()
        }
    };
    let eui_display = device.eui.clone();
    let avg_temp = device.avg_temperature.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "-".to_string());
    let reading_count = device.reading_count;
    let eui_clone = eui.clone();

    view! {
        <div
            class=move || format!(
                "card cursor-pointer transition-all {}",
                if is_selected.get() { "bg-primary text-primary-content" } else { "bg-base-100" }
            )
            on:click=move |_| toggle_device(eui_clone.clone())
        >
            <div class="card-body p-4">
                <h3 class="font-bold">{display_name}</h3>
                {has_friendly_name.then(|| view! {
                    <p class="text-xs opacity-70 font-mono">{eui_display.clone()}</p>
                })}
                <div class="flex justify-between text-sm mt-2">
                    <span>"Avg: " <strong>{avg_temp}</strong></span>
                    <span>{reading_count} " readings"</span>
                </div>
            </div>
        </div>
    }
}

/// Overall statistics cards
#[component]
fn OverallStatsCards(report: HeatPumpReport) -> impl IntoView {
    let overall = report.overall;
    let period = report.period;

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Overall Statistics"</h2>
                <p class="text-sm opacity-70">
                    {format!("Period: {} to {} ({} days)", period.start_date, period.end_date, period.days)}
                </p>

                <div class="stats stats-vertical lg:stats-horizontal shadow w-full">
                    <div class="stat">
                        <div class="stat-title">"Avg Source Temp"</div>
                        <div class="stat-value text-primary">{format!("{:.1}°C", overall.avg_source_temp)}</div>
                        <div class="stat-desc">"Underground sensors"</div>
                    </div>

                    <div class="stat">
                        <div class="stat-title">"Avg Outdoor Temp"</div>
                        <div class="stat-value">
                            {overall.avg_outdoor_temp.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "-".to_string())}
                        </div>
                        <div class="stat-desc">"From DWD weather"</div>
                    </div>

                    <div class="stat">
                        <div class="stat-title">"Temp Advantage"</div>
                        <div class="stat-value text-success">
                            {overall.avg_temp_advantage.map(|t| format!("{:+.1}°C", t)).unwrap_or_else(|| "-".to_string())}
                        </div>
                        <div class="stat-desc">"Source vs outdoor"</div>
                    </div>

                    <div class="stat">
                        <div class="stat-title">"Est. Heating COP"</div>
                        <div class="stat-value text-info">
                            {overall.avg_heating_cop.map(|c| format!("{:.1}", c)).unwrap_or_else(|| "-".to_string())}
                        </div>
                        <div class="stat-desc">"Coefficient of Performance"</div>
                    </div>

                    <div class="stat">
                        <div class="stat-title">"Est. Energy Savings"</div>
                        <div class="stat-value text-success">
                            {overall.energy_savings_estimate.map(|s| format!("{:.0}%", s)).unwrap_or_else(|| "-".to_string())}
                        </div>
                        <div class="stat-desc">"vs air-source heat pump"</div>
                    </div>
                </div>

                <div class="flex gap-8 mt-2">
                    <div class="text-sm">
                        <span class="opacity-70">"Heating Degree Days: "</span>
                        <strong>{format!("{:.1}", overall.total_heating_degree_days)}</strong>
                    </div>
                    <div class="text-sm">
                        <span class="opacity-70">"Cooling Degree Days: "</span>
                        <strong>{format!("{:.1}", overall.total_cooling_degree_days)}</strong>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Seasonal breakdown component
#[component]
fn SeasonalBreakdown(report: HeatPumpReport) -> impl IntoView {
    let seasonal = report.seasonal;

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Seasonal Analysis"</h2>
                <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
                    <SeasonCard name="Winter" icon="❄️" season=seasonal.winter />
                    <SeasonCard name="Spring" icon="🌱" season=seasonal.spring />
                    <SeasonCard name="Summer" icon="☀️" season=seasonal.summer />
                    <SeasonCard name="Autumn" icon="🍂" season=seasonal.autumn />
                </div>
            </div>
        </div>
    }
}

/// Individual season card
#[component]
fn SeasonCard(
    name: &'static str,
    icon: &'static str,
    season: Option<SeasonSummary>,
) -> impl IntoView {
    match season {
        Some(s) => view! {
            <div class="card bg-base-100">
                <div class="card-body p-4">
                    <h3 class="font-bold">{icon} " " {name}</h3>
                    <div class="text-sm space-y-1">
                        <p>{format!("{} days analyzed", s.days)}</p>
                        <p>"Source: " <strong>{format!("{:.1}°C", s.avg_source_temp)}</strong></p>
                        <p>"Outdoor: " <strong>
                            {s.avg_outdoor_temp.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "-".to_string())}
                        </strong></p>
                        <p>"Advantage: " <strong class="text-success">
                            {s.avg_temp_advantage.map(|t| format!("{:+.1}°C", t)).unwrap_or_else(|| "-".to_string())}
                        </strong></p>
                        <p>"Est. COP: " <strong class="text-info">
                            {s.avg_cop.map(|c| format!("{:.1}", c)).unwrap_or_else(|| "-".to_string())}
                        </strong></p>
                    </div>
                </div>
            </div>
        }.into_any(),
        None => view! {
            <div class="card bg-base-100 opacity-50">
                <div class="card-body p-4">
                    <h3 class="font-bold">{icon} " " {name}</h3>
                    <p class="text-sm">"No data"</p>
                </div>
            </div>
        }.into_any(),
    }
}

/// Device statistics table
#[component]
fn DeviceStatsTable(report: HeatPumpReport) -> impl IntoView {
    let stats: Vec<_> = report.device_stats.into_iter().collect();

    if stats.is_empty() {
        return view! { <div></div> }.into_any();
    }

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Device Statistics"</h2>
                <div class="overflow-x-auto">
                    <table class="table table-zebra">
                        <thead>
                            <tr>
                                <th>"Device"</th>
                                <th>"Avg Temp"</th>
                                <th>"Min"</th>
                                <th>"Max"</th>
                                <th>"Humidity"</th>
                                <th>"Readings"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {stats.into_iter().map(|(eui, device)| {
                                view! {
                                    <tr>
                                        <td>
                                            <div class="font-bold">{device.name}</div>
                                            <div class="text-xs opacity-50">{eui}</div>
                                        </td>
                                        <td>{format!("{:.1}°C", device.avg_temp)}</td>
                                        <td>{format!("{:.1}°C", device.min_temp)}</td>
                                        <td>{format!("{:.1}°C", device.max_temp)}</td>
                                        <td>
                                            {device.avg_humidity.map(|h| format!("{:.0}%", h)).unwrap_or_else(|| "-".to_string())}
                                        </td>
                                        <td>{device.total_readings}</td>
                                    </tr>
                                }
                            }).collect::<Vec<_>>()}
                        </tbody>
                    </table>
                </div>
            </div>
        </div>
    }.into_any()
}

/// Daily analysis table
#[component]
fn DailyAnalysisTable(report: HeatPumpReport) -> impl IntoView {
    // Show most recent first, limit to 30 rows
    let daily: Vec<DailyHeatPumpAnalysis> = report.daily.into_iter().rev().take(30).collect();

    if daily.is_empty() {
        return view! { <div></div> }.into_any();
    }

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <h2 class="card-title">"Daily Analysis" <span class="text-sm font-normal opacity-70">"(last 30 days)"</span></h2>
                <div class="overflow-x-auto">
                    <table class="table table-zebra table-sm">
                        <thead>
                            <tr>
                                <th>"Date"</th>
                                <th>"Season"</th>
                                <th>"Source"</th>
                                <th>"Outdoor"</th>
                                <th>"Advantage"</th>
                                <th>"COP"</th>
                                <th>"HDD"</th>
                                <th>"Readings"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {daily.into_iter().map(|d| {
                                view! {
                                    <DailyAnalysisRow analysis=d />
                                }
                            }).collect::<Vec<_>>()}
                        </tbody>
                    </table>
                </div>
            </div>
        </div>
    }.into_any()
}

/// Single row in daily analysis table
#[component]
fn DailyAnalysisRow(analysis: DailyHeatPumpAnalysis) -> impl IntoView {
    let date_formatted = format_date(&analysis.date);
    let season_icon = match analysis.season.as_str() {
        "Winter" => "❄️",
        "Spring" => "🌱",
        "Summer" => "☀️",
        "Autumn" => "🍂",
        _ => "",
    };

    let advantage_class = analysis.temp_advantage
        .map(|a| if a > 0.0 { "text-success" } else { "text-error" })
        .unwrap_or("");

    view! {
        <tr>
            <td>{date_formatted}</td>
            <td>{season_icon} " " {analysis.season.clone()}</td>
            <td>
                {analysis.source_temp.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "-".to_string())}
            </td>
            <td>
                {analysis.outdoor_temp.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "-".to_string())}
            </td>
            <td class=advantage_class>
                {analysis.temp_advantage.map(|t| format!("{:+.1}°C", t)).unwrap_or_else(|| "-".to_string())}
            </td>
            <td>
                {analysis.heating_cop_estimate.map(|c| format!("{:.1}", c)).unwrap_or_else(|| "-".to_string())}
            </td>
            <td>{format!("{:.1}", analysis.heating_degree_days)}</td>
            <td>{analysis.source_readings}</td>
        </tr>
    }
}

/// Format date from YYYYMMDD to YYYY-MM-DD
fn format_date(date: &str) -> String {
    if date.len() == 8 {
        format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8])
    } else {
        date.to_string()
    }
}
