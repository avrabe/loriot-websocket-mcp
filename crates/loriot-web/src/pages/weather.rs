//! Weather page - Outdoor weather data from DWD

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    fetch_dwd_data, fetch_weather_history, fetch_weather_info, fetch_weather_statistics,
    geocode_address, set_weather_location, GeocodingResult, OutdoorSummary, WeatherInfo,
    WeatherStatistics,
};
use crate::components::{ChartConfig, DataPoint, DualLineChart, LineChart, Navbar};

/// Available time periods for weather data
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WeatherPeriod {
    Week,     // 7 days
    TwoWeeks, // 14 days
    Month,    // 30 days
    Quarter,  // 90 days
    Year,     // 365 days
    All,      // All available data
}

impl WeatherPeriod {
    fn days(&self) -> Option<usize> {
        match self {
            WeatherPeriod::Week => Some(7),
            WeatherPeriod::TwoWeeks => Some(14),
            WeatherPeriod::Month => Some(30),
            WeatherPeriod::Quarter => Some(90),
            WeatherPeriod::Year => Some(365),
            WeatherPeriod::All => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            WeatherPeriod::Week => "7 Days",
            WeatherPeriod::TwoWeeks => "14 Days",
            WeatherPeriod::Month => "30 Days",
            WeatherPeriod::Quarter => "90 Days",
            WeatherPeriod::Year => "1 Year",
            WeatherPeriod::All => "All Time",
        }
    }
}

/// Convert YYYYMMDD date to timestamp
fn date_to_timestamp(date: &str) -> Option<u64> {
    if date.len() != 8 {
        return None;
    }
    let year: i32 = date[0..4].parse().ok()?;
    let month: u32 = date[4..6].parse().ok()?;
    let day: u32 = date[6..8].parse().ok()?;

    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|d| d.and_hms_opt(12, 0, 0))
        .map(|dt| dt.and_utc().timestamp_millis() as u64)
}

/// Weather page showing outdoor weather data from DWD
#[component]
pub fn WeatherPage() -> impl IntoView {
    let weather_info = RwSignal::new(Option::<WeatherInfo>::None);
    let statistics = RwSignal::new(Option::<WeatherStatistics>::None);
    let history = RwSignal::new(Vec::<OutdoorSummary>::new());
    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);

    // Time period selection
    let selected_period = RwSignal::new(WeatherPeriod::Month);

    // Location configuration state
    let address_input = RwSignal::new(String::new());
    let geocode_results = RwSignal::new(Vec::<GeocodingResult>::new());
    let geocoding = RwSignal::new(false);
    let fetching_dwd = RwSignal::new(false);

    // Function to reload weather data
    let reload_data = move || {
        loading.set(true);
        error.set(None);
        let period = selected_period.get();
        let days = period.days();

        spawn_local(async move {
            // Fetch weather info
            match fetch_weather_info().await {
                Ok(info) => weather_info.set(Some(info)),
                Err(e) => {
                    leptos::logging::error!("Failed to fetch weather info: {}", e);
                    error.set(Some(format!("Failed to load weather info: {}", e)));
                }
            }

            // Fetch statistics (for the full period)
            match fetch_weather_statistics().await {
                Ok(stats) => statistics.set(Some(stats)),
                Err(e) => leptos::logging::warn!("No weather statistics available: {}", e),
            }

            // Fetch history based on selected period
            let history_days = days.unwrap_or(365); // Default to 365 for "All"
            match fetch_weather_history(Some(history_days)).await {
                Ok(data) => history.set(data),
                Err(e) => leptos::logging::warn!("Failed to fetch weather history: {}", e),
            }

            loading.set(false);
        });
    };

    // Fetch weather data on mount and when period changes
    Effect::new(move |_| {
        let _ = selected_period.get(); // Subscribe to changes
        reload_data();
    });

    // Geocode address action
    let do_geocode = move |_| {
        let addr = address_input.get();
        if addr.trim().is_empty() {
            return;
        }
        geocoding.set(true);
        geocode_results.set(vec![]);

        spawn_local(async move {
            match geocode_address(&addr).await {
                Ok(response) => geocode_results.set(response.results),
                Err(e) => error.set(Some(format!("Geocoding failed: {}", e))),
            }
            geocoding.set(false);
        });
    };

    // Set location from geocode result
    let select_location = move |result: GeocodingResult| {
        loading.set(true);
        geocode_results.set(vec![]);
        address_input.set(String::new());

        spawn_local(async move {
            match set_weather_location(result.latitude, result.longitude).await {
                Ok(info) => {
                    weather_info.set(Some(info));
                    reload_data();
                }
                Err(e) => {
                    error.set(Some(format!("Failed to set location: {}", e)));
                    loading.set(false);
                }
            }
        });
    };

    // Fetch DWD data action
    let do_fetch_dwd = move |_| {
        fetching_dwd.set(true);
        error.set(None);

        spawn_local(async move {
            match fetch_dwd_data().await {
                Ok(_) => reload_data(),
                Err(e) => error.set(Some(format!("Failed to fetch DWD data: {}", e))),
            }
            fetching_dwd.set(false);
        });
    };

    view! {
        <div>
            <Navbar />

            <div class="container mx-auto p-4">
                // Header with time selector
                <div class="flex flex-wrap justify-between items-center mb-6 gap-4">
                    <div class="flex items-center gap-4">
                        <h1 class="text-3xl font-bold">"Outdoor Weather"</h1>
                        <div class="badge badge-info">"DWD Data"</div>
                    </div>
                    <WeatherPeriodSelector
                        selected=selected_period
                    />
                </div>

                // Location Configuration Card
                <LocationConfigCard
                    address_input=address_input
                    geocode_results=geocode_results
                    geocoding=geocoding
                    fetching_dwd=fetching_dwd
                    on_geocode=do_geocode
                    on_select=select_location
                    on_fetch_dwd=do_fetch_dwd
                />

                {move || {
                    if loading.get() {
                        view! {
                            <div class="flex justify-center items-center h-64">
                                <span class="loading loading-spinner loading-lg"></span>
                            </div>
                        }.into_any()
                    } else if let Some(err) = error.get() {
                        view! {
                            <div class="alert alert-error mb-4">
                                <svg xmlns="http://www.w3.org/2000/svg" class="stroke-current shrink-0 h-6 w-6" fill="none" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z" />
                                </svg>
                                <span>{err}</span>
                            </div>
                            <StationInfoCard info=weather_info />
                        }.into_any()
                    } else {
                        view! {
                            <StationInfoCard info=weather_info />
                            <WeatherCharts history=history />
                            <StatisticsCards stats=statistics />
                            <WeatherHistoryTable history=history />
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

/// Weather period selector component
#[component]
fn WeatherPeriodSelector(selected: RwSignal<WeatherPeriod>) -> impl IntoView {
    let periods = vec![
        WeatherPeriod::Week,
        WeatherPeriod::TwoWeeks,
        WeatherPeriod::Month,
        WeatherPeriod::Quarter,
        WeatherPeriod::Year,
        WeatherPeriod::All,
    ];

    view! {
        <div class="flex items-center gap-2">
            <span class="text-sm opacity-70">"Period:"</span>
            <select
                class="select select-bordered select-sm"
                on:change=move |ev| {
                    let value = event_target_value(&ev);
                    let period = match value.as_str() {
                        "week" => WeatherPeriod::Week,
                        "2weeks" => WeatherPeriod::TwoWeeks,
                        "month" => WeatherPeriod::Month,
                        "quarter" => WeatherPeriod::Quarter,
                        "year" => WeatherPeriod::Year,
                        _ => WeatherPeriod::All,
                    };
                    selected.set(period);
                }
            >
                {periods.into_iter().map(|p| {
                    let value = match p {
                        WeatherPeriod::Week => "week",
                        WeatherPeriod::TwoWeeks => "2weeks",
                        WeatherPeriod::Month => "month",
                        WeatherPeriod::Quarter => "quarter",
                        WeatherPeriod::Year => "year",
                        WeatherPeriod::All => "all",
                    };
                    view! {
                        <option
                            value=value
                            selected=move || selected.get() == p
                        >
                            {p.label()}
                        </option>
                    }
                }).collect::<Vec<_>>()}
            </select>
        </div>
    }
}

/// Location configuration card
#[component]
fn LocationConfigCard(
    address_input: RwSignal<String>,
    geocode_results: RwSignal<Vec<GeocodingResult>>,
    geocoding: RwSignal<bool>,
    fetching_dwd: RwSignal<bool>,
    on_geocode: impl Fn(()) + 'static + Copy + Send + Sync,
    on_select: impl Fn(GeocodingResult) + 'static + Copy + Send + Sync,
    on_fetch_dwd: impl Fn(()) + 'static + Copy + Send + Sync,
) -> impl IntoView {
    view! {
        <div class="card bg-base-100 shadow-xl mb-6">
            <div class="card-body">
                <h2 class="card-title">"Location Configuration"</h2>
                <p class="text-sm opacity-70">"Enter an address or city name to find the nearest DWD weather station"</p>

                <div class="flex flex-wrap gap-4 mt-4">
                    // Address input
                    <div class="form-control flex-1 min-w-64">
                        <div class="join w-full">
                            <input
                                type="text"
                                placeholder="Enter address (e.g., Berlin, Germany)"
                                class="input input-bordered join-item flex-1"
                                prop:value=move || address_input.get()
                                on:input=move |ev| address_input.set(event_target_value(&ev))
                                on:keydown=move |ev| {
                                    if ev.key() == "Enter" {
                                        on_geocode(());
                                    }
                                }
                            />
                            <button
                                class="btn btn-primary join-item"
                                on:click=move |_| on_geocode(())
                                disabled=move || geocoding.get()
                            >
                                {move || if geocoding.get() {
                                    view! { <span class="loading loading-spinner loading-sm"></span> }.into_any()
                                } else {
                                    view! { "Search" }.into_any()
                                }}
                            </button>
                        </div>
                    </div>

                    // Fetch DWD button
                    <button
                        class="btn btn-secondary"
                        on:click=move |_| on_fetch_dwd(())
                        disabled=move || fetching_dwd.get()
                    >
                        {move || if fetching_dwd.get() {
                            view! {
                                <span class="loading loading-spinner loading-sm"></span>
                                " Fetching..."
                            }.into_any()
                        } else {
                            view! { "Fetch DWD Data" }.into_any()
                        }}
                    </button>
                </div>

                // Geocode results
                {move || {
                    let results = geocode_results.get();
                    if results.is_empty() {
                        view! { <div></div> }.into_any()
                    } else {
                        view! {
                            <div class="mt-4">
                                <p class="text-sm font-semibold mb-2">"Select a location:"</p>
                                <div class="flex flex-wrap gap-2">
                                    {results.into_iter().map(|result| {
                                        let display = result.display_name.clone();
                                        let result_clone = result.clone();
                                        view! {
                                            <button
                                                class="btn btn-outline btn-sm"
                                                on:click=move |_| on_select(result_clone.clone())
                                            >
                                                {display}
                                            </button>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

/// Station info card component
#[component]
fn StationInfoCard(info: RwSignal<Option<WeatherInfo>>) -> impl IntoView {
    view! {
        {move || {
            if let Some(info) = info.get() {
                let station_display = info.station.as_ref()
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "Not Set".to_string());
                let station_detail = info.station.as_ref()
                    .map(|s| format!("ID: {} | {} m elevation", s.id, s.elevation))
                    .unwrap_or_default();
                let location_display = info.location.as_ref()
                    .map(|l| format!("{:.4}°N, {:.4}°E", l.latitude, l.longitude))
                    .unwrap_or_else(|| "Not Set".to_string());
                let distance_display = info.station_distance_km
                    .map(|d| format!("{:.1} km", d))
                    .unwrap_or_else(|| "-".to_string());
                let data_info = format!("Data: {} records ({} cached) | Range: {} to {}",
                    info.data_range.total_records,
                    info.data_range.cached_records,
                    info.data_range.earliest_date.as_deref().unwrap_or("-"),
                    info.data_range.latest_date.as_deref().unwrap_or("-")
                );

                view! {
                    <div class="card bg-base-100 shadow-xl mb-6">
                        <div class="card-body">
                            <h2 class="card-title">"Weather Data Source"</h2>
                            <p class="text-sm opacity-70 mb-2">
                                "Weather data comes from the nearest DWD (German Weather Service) station to your location."
                            </p>
                            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                                <div class="stat bg-base-200 rounded-lg">
                                    <div class="stat-title">"Your Location"</div>
                                    <div class="stat-value text-sm">{location_display}</div>
                                    <div class="stat-desc">"Set via search above"</div>
                                </div>

                                <div class="stat bg-base-200 rounded-lg">
                                    <div class="stat-title">"Nearest DWD Station"</div>
                                    <div class="stat-value text-lg text-primary">{station_display}</div>
                                    <div class="stat-desc">{station_detail}</div>
                                </div>

                                <div class="stat bg-base-200 rounded-lg">
                                    <div class="stat-title">"Distance to Station"</div>
                                    <div class="stat-value text-info">{distance_display}</div>
                                    <div class="stat-desc">"from your location"</div>
                                </div>
                            </div>
                            <div class="text-sm opacity-70 mt-4">{data_info}</div>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="alert alert-info mb-6">
                        <span>"No weather station configured. Set a location to get weather data."</span>
                    </div>
                }.into_any()
            }
        }}
    }
}

/// Weather charts component
#[component]
fn WeatherCharts(history: RwSignal<Vec<OutdoorSummary>>) -> impl IntoView {
    view! {
        <div class="card bg-base-100 shadow-xl mb-6">
            <div class="card-body">
                <h2 class="card-title">"Temperature & Humidity"</h2>
                {move || {
                    let data = history.get();
                    if data.is_empty() {
                        view! {
                            <div class="flex items-center justify-center h-48 bg-base-200 rounded-lg">
                                <span class="opacity-50">"No weather data available"</span>
                            </div>
                        }.into_any()
                    } else {
                        // Convert to chart data points (reversed to show oldest first)
                        let mut sorted_data = data.clone();
                        sorted_data.sort_by(|a, b| a.date.cmp(&b.date));

                        let temp_data: Vec<DataPoint> = sorted_data
                            .iter()
                            .filter_map(|r| {
                                let ts = date_to_timestamp(&r.date)?;
                                r.temp_mean.map(|t| (ts, t))
                            })
                            .collect();

                        let humidity_data: Vec<DataPoint> = sorted_data
                            .iter()
                            .filter_map(|r| {
                                let ts = date_to_timestamp(&r.date)?;
                                r.humidity.map(|h| (ts, h))
                            })
                            .collect();

                        view! {
                            <DualLineChart
                                temp_data=temp_data
                                humidity_data=humidity_data
                                width=800
                                height=300
                            />
                        }.into_any()
                    }
                }}
            </div>
        </div>

        // Precipitation chart
        <div class="card bg-base-100 shadow-xl mb-6">
            <div class="card-body">
                <h2 class="card-title">"Precipitation"</h2>
                {move || {
                    let data = history.get();
                    if data.is_empty() {
                        view! {
                            <div class="flex items-center justify-center h-32 bg-base-200 rounded-lg">
                                <span class="opacity-50">"No precipitation data available"</span>
                            </div>
                        }.into_any()
                    } else {
                        // Convert to chart data points
                        let mut sorted_data = data.clone();
                        sorted_data.sort_by(|a, b| a.date.cmp(&b.date));

                        let precip_data: Vec<DataPoint> = sorted_data
                            .iter()
                            .filter_map(|r| {
                                let ts = date_to_timestamp(&r.date)?;
                                r.precipitation.map(|p| (ts, p))
                            })
                            .collect();

                        if precip_data.len() < 2 {
                            view! {
                                <div class="flex items-center justify-center h-32 bg-base-200 rounded-lg">
                                    <span class="opacity-50">"Not enough precipitation data"</span>
                                </div>
                            }.into_any()
                        } else {
                            let config = ChartConfig {
                                width: 800,
                                height: 200,
                                padding: 50,
                                line_color: "#3b82f6".to_string(),
                                fill_color: "rgba(59, 130, 246, 0.2)".to_string(),
                                show_grid: true,
                                show_points: false,
                                label: "Precipitation (mm)".to_string(),
                            };

                            view! {
                                <LineChart data=precip_data config=config />
                            }.into_any()
                        }
                    }
                }}
            </div>
        </div>

        // Temperature range chart (min/max)
        <div class="card bg-base-100 shadow-xl mb-6">
            <div class="card-body">
                <h2 class="card-title">"Temperature Range (Min/Max)"</h2>
                {move || {
                    let data = history.get();
                    if data.is_empty() {
                        view! {
                            <div class="flex items-center justify-center h-32 bg-base-200 rounded-lg">
                                <span class="opacity-50">"No temperature data available"</span>
                            </div>
                        }.into_any()
                    } else {
                        // Convert to chart data points
                        let mut sorted_data = data.clone();
                        sorted_data.sort_by(|a, b| a.date.cmp(&b.date));

                        let temp_min_data: Vec<DataPoint> = sorted_data
                            .iter()
                            .filter_map(|r| {
                                let ts = date_to_timestamp(&r.date)?;
                                r.temp_min.map(|t| (ts, t))
                            })
                            .collect();

                        let temp_max_data: Vec<DataPoint> = sorted_data
                            .iter()
                            .filter_map(|r| {
                                let ts = date_to_timestamp(&r.date)?;
                                r.temp_max.map(|t| (ts, t))
                            })
                            .collect();

                        if temp_min_data.len() < 2 {
                            view! {
                                <div class="flex items-center justify-center h-32 bg-base-200 rounded-lg">
                                    <span class="opacity-50">"Not enough temperature range data"</span>
                                </div>
                            }.into_any()
                        } else {
                            // Use DualLineChart with min as "temp" and max as "humidity" (hack for dual display)
                            view! {
                                <TemperatureRangeChart
                                    min_data=temp_min_data
                                    max_data=temp_max_data
                                />
                            }.into_any()
                        }
                    }
                }}
            </div>
        </div>
    }
}

/// Temperature range chart (min/max on same scale)
#[component]
fn TemperatureRangeChart(min_data: Vec<DataPoint>, max_data: Vec<DataPoint>) -> impl IntoView {
    if min_data.len() < 2 && max_data.len() < 2 {
        return view! {
            <div class="flex items-center justify-center h-32 bg-base-200 rounded-lg">
                <span class="opacity-50">"Not enough data for chart"</span>
            </div>
        }
        .into_any();
    }

    let width: u32 = 800;
    let height: u32 = 250;
    let padding: u32 = 50;
    let chart_width = width - 2 * padding;
    let chart_height = height - 2 * padding;

    // Find time range from both datasets
    let all_timestamps: Vec<u64> = min_data
        .iter()
        .chain(max_data.iter())
        .map(|(t, _)| *t)
        .collect();

    let min_ts = all_timestamps.iter().min().copied().unwrap_or(0);
    let max_ts = all_timestamps.iter().max().copied().unwrap_or(1);
    let ts_range = (max_ts - min_ts) as f32;

    // Find temperature range from both datasets
    let all_temps: Vec<f32> = min_data
        .iter()
        .chain(max_data.iter())
        .map(|(_, v)| *v)
        .collect();

    let temp_min = all_temps.iter().fold(f32::INFINITY, |a, &b| a.min(b));
    let temp_max = all_temps.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    let temp_range_raw = temp_max - temp_min;
    let temp_padding_val = if temp_range_raw > 0.0 {
        temp_range_raw * 0.1
    } else {
        5.0
    };
    let temp_min = temp_min - temp_padding_val;
    let temp_max = temp_max + temp_padding_val;
    let temp_range = temp_max - temp_min;

    // Convert min temp to SVG coordinates
    let min_points: Vec<(f32, f32)> = min_data
        .iter()
        .map(|(ts, val)| {
            let x = if ts_range > 0.0 {
                padding as f32 + ((*ts - min_ts) as f32 / ts_range) * chart_width as f32
            } else {
                padding as f32 + chart_width as f32 / 2.0
            };
            let y = if temp_range > 0.0 {
                padding as f32 + (1.0 - (val - temp_min) / temp_range) * chart_height as f32
            } else {
                padding as f32 + chart_height as f32 / 2.0
            };
            (x, y)
        })
        .collect();

    // Convert max temp to SVG coordinates
    let max_points: Vec<(f32, f32)> = max_data
        .iter()
        .map(|(ts, val)| {
            let x = if ts_range > 0.0 {
                padding as f32 + ((*ts - min_ts) as f32 / ts_range) * chart_width as f32
            } else {
                padding as f32 + chart_width as f32 / 2.0
            };
            let y = if temp_range > 0.0 {
                padding as f32 + (1.0 - (val - temp_min) / temp_range) * chart_height as f32
            } else {
                padding as f32 + chart_height as f32 / 2.0
            };
            (x, y)
        })
        .collect();

    // Create path data
    let min_path = min_points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            if i == 0 {
                format!("M {:.1} {:.1}", x, y)
            } else {
                format!("L {:.1} {:.1}", x, y)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let max_path = max_points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            if i == 0 {
                format!("M {:.1} {:.1}", x, y)
            } else {
                format!("L {:.1} {:.1}", x, y)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    // Y-axis labels
    let temp_labels: Vec<(f32, String)> = (0..=4)
        .map(|i| {
            let val = temp_min + (temp_range * i as f32 / 4.0);
            let y = padding as f32 + (1.0 - i as f32 / 4.0) * chart_height as f32;
            (y, format!("{:.0}°C", val))
        })
        .collect();

    view! {
        <div class="relative">
            <svg
                width={width.to_string()}
                height={height.to_string()}
                viewBox={format!("0 0 {} {}", width, height)}
                class="w-full h-auto"
            >
                // Grid lines
                <g class="opacity-10">
                    {temp_labels.iter().map(|(y, _)| {
                        view! {
                            <line
                                x1={padding.to_string()}
                                y1={format!("{:.1}", y)}
                                x2={(width - padding).to_string()}
                                y2={format!("{:.1}", y)}
                                stroke="currentColor"
                                stroke-dasharray="4 4"
                            />
                        }
                    }).collect::<Vec<_>>()}
                </g>

                // Y-axis labels
                <g class="text-xs fill-current opacity-70">
                    {temp_labels.iter().map(|(y, label)| {
                        view! {
                            <text
                                x={(padding - 5).to_string()}
                                y={format!("{:.1}", y + 4.0)}
                                text-anchor="end"
                                font-size="10"
                            >
                                {label.clone()}
                            </text>
                        }
                    }).collect::<Vec<_>>()}
                </g>

                // Min temperature line (blue)
                {(!min_path.is_empty()).then(|| view! {
                    <path
                        d={min_path.clone()}
                        fill="none"
                        stroke="#3b82f6"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                })}

                // Max temperature line (red/orange)
                {(!max_path.is_empty()).then(|| view! {
                    <path
                        d={max_path.clone()}
                        fill="none"
                        stroke="#ef4444"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                })}
            </svg>

            // Legend
            <div class="flex justify-center gap-6 mt-2 text-sm">
                <div class="flex items-center gap-2">
                    <div class="w-4 h-1 bg-blue-500 rounded"></div>
                    <span class="opacity-70">"Min Temperature"</span>
                </div>
                <div class="flex items-center gap-2">
                    <div class="w-4 h-1 bg-red-500 rounded"></div>
                    <span class="opacity-70">"Max Temperature"</span>
                </div>
            </div>
        </div>
    }
    .into_any()
}

/// Statistics cards component
#[component]
fn StatisticsCards(stats: RwSignal<Option<WeatherStatistics>>) -> impl IntoView {
    view! {
        {move || {
            if let Some(stats) = stats.get() {
                let temp_mean = format!("{:.1}°C", stats.temperature.mean);
                let temp_range = format!("Range: {:.1}°C - {:.1}°C", stats.temperature.min, stats.temperature.max);
                let humidity_mean = stats.humidity.as_ref().map(|h| format!("{:.0}%", h.mean));
                let humidity_range = stats.humidity.as_ref().map(|h| format!("Range: {:.0}% - {:.0}%", h.min, h.max));
                let heating_dd = format!("{:.0}", stats.heating_degree_days);
                let cooling_dd = format!("{:.0}", stats.cooling_degree_days);
                let precip = stats.precipitation_total.map(|p| format!("{:.1} mm", p));
                let sunshine = stats.sunshine_total.map(|s| format!("{:.0} hrs", s));
                let days = stats.days;

                view! {
                    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
                        <div class="stat bg-base-100 shadow rounded-lg">
                            <div class="stat-title">"Temperature"</div>
                            <div class="stat-value text-primary">{temp_mean}</div>
                            <div class="stat-desc">{temp_range}</div>
                        </div>

                        {if let Some(hum) = humidity_mean {
                            view! {
                                <div class="stat bg-base-100 shadow rounded-lg">
                                    <div class="stat-title">"Humidity"</div>
                                    <div class="stat-value text-secondary">{hum}</div>
                                    <div class="stat-desc">{humidity_range.unwrap_or_default()}</div>
                                </div>
                            }.into_any()
                        } else {
                            view! { <div></div> }.into_any()
                        }}

                        <div class="stat bg-base-100 shadow rounded-lg">
                            <div class="stat-title">"Heating Degree Days"</div>
                            <div class="stat-value text-warning">{heating_dd}</div>
                            <div class="stat-desc">"Base 15°C"</div>
                        </div>

                        <div class="stat bg-base-100 shadow rounded-lg">
                            <div class="stat-title">"Cooling Degree Days"</div>
                            <div class="stat-value text-info">{cooling_dd}</div>
                            <div class="stat-desc">"Base 24°C"</div>
                        </div>
                    </div>

                    <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
                        {if let Some(p) = precip {
                            let days_str = format!("Over {} days", days);
                            view! {
                                <div class="stat bg-base-100 shadow rounded-lg">
                                    <div class="stat-title">"Total Precipitation"</div>
                                    <div class="stat-value text-blue-400">{p}</div>
                                    <div class="stat-desc">{days_str}</div>
                                </div>
                            }.into_any()
                        } else {
                            view! { <div></div> }.into_any()
                        }}

                        {if let Some(s) = sunshine {
                            let days_str = format!("Over {} days", days);
                            view! {
                                <div class="stat bg-base-100 shadow rounded-lg">
                                    <div class="stat-title">"Total Sunshine"</div>
                                    <div class="stat-value text-yellow-400">{s}</div>
                                    <div class="stat-desc">{days_str}</div>
                                </div>
                            }.into_any()
                        } else {
                            view! { <div></div> }.into_any()
                        }}
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="alert alert-warning mb-6">
                        <svg xmlns="http://www.w3.org/2000/svg" class="stroke-current shrink-0 h-6 w-6" fill="none" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                        </svg>
                        <span>"No weather statistics available. Fetch data from DWD to see statistics."</span>
                    </div>
                }.into_any()
            }
        }}
    }
}

/// Weather history table component
#[component]
fn WeatherHistoryTable(history: RwSignal<Vec<OutdoorSummary>>) -> impl IntoView {
    view! {
        <div class="card bg-base-100 shadow-xl">
            <div class="card-body">
                <h2 class="card-title">"Weather History"</h2>
                {move || {
                    let data = history.get();
                    if data.is_empty() {
                        view! {
                            <div class="text-center py-8 opacity-70">
                                "No weather history available"
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="overflow-x-auto max-h-96">
                                <table class="table table-zebra table-sm table-pin-rows">
                                    <thead>
                                        <tr>
                                            <th>"Date"</th>
                                            <th>"Station"</th>
                                            <th>"Temp (Mean)"</th>
                                            <th>"Temp (Min/Max)"</th>
                                            <th>"Humidity"</th>
                                            <th>"Precipitation"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <For
                                            each=move || history.get()
                                            key=|record| record.date.clone()
                                            let:record
                                        >
                                            <WeatherHistoryRow record=record />
                                        </For>
                                    </tbody>
                                </table>
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

/// Single row in weather history table
#[component]
fn WeatherHistoryRow(record: OutdoorSummary) -> impl IntoView {
    let date = format_date(&record.date);
    let station = record.station_name.clone();
    let temp_mean = record
        .temp_mean
        .map(|t| format!("{:.1}°C", t))
        .unwrap_or_else(|| "-".to_string());
    let temp_range = format!(
        "{} / {}",
        record
            .temp_min
            .map(|t| format!("{:.1}°C", t))
            .unwrap_or_else(|| "-".to_string()),
        record
            .temp_max
            .map(|t| format!("{:.1}°C", t))
            .unwrap_or_else(|| "-".to_string())
    );
    let humidity = record
        .humidity
        .map(|h| format!("{:.0}%", h))
        .unwrap_or_else(|| "-".to_string());
    let precipitation = record
        .precipitation
        .map(|p| format!("{:.1} mm", p))
        .unwrap_or_else(|| "-".to_string());

    view! {
        <tr>
            <td class="font-mono">{date}</td>
            <td>{station}</td>
            <td class="text-primary font-semibold">{temp_mean}</td>
            <td>{temp_range}</td>
            <td>{humidity}</td>
            <td>{precipitation}</td>
        </tr>
    }
}

/// Format date from YYYYMMDD to readable format
fn format_date(date: &str) -> String {
    if date.len() == 8 {
        format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8])
    } else {
        date.to_string()
    }
}
