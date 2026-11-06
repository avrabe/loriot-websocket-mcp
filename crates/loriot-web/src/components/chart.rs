//! Simple SVG line chart component

use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Data point for the chart (timestamp, value)
pub type DataPoint = (u64, f32);

/// Line chart configuration
#[derive(Clone)]
pub struct ChartConfig {
    pub width: u32,
    pub height: u32,
    pub padding: u32,
    pub line_color: String,
    pub fill_color: String,
    pub show_grid: bool,
    pub show_points: bool,
    pub label: String,
}

impl Default for ChartConfig {
    fn default() -> Self {
        ChartConfig {
            width: 600,
            height: 200,
            padding: 40,
            line_color: "#3b82f6".to_string(),
            fill_color: "rgba(59, 130, 246, 0.1)".to_string(),
            show_grid: true,
            show_points: false,
            label: String::new(),
        }
    }
}

/// SVG Line chart component
#[component]
pub fn LineChart(
    data: Vec<DataPoint>,
    #[prop(default = ChartConfig::default())] config: ChartConfig,
) -> impl IntoView {
    if data.len() < 2 {
        return view! {
            <div class="flex items-center justify-center h-32 bg-base-200 rounded-lg">
                <span class="opacity-50">"Not enough data for chart"</span>
            </div>
        }
        .into_any();
    }

    let width = config.width;
    let height = config.height;
    let padding = config.padding;

    let chart_width = width - 2 * padding;
    let chart_height = height - 2 * padding;

    // Find data ranges
    let min_ts = data.iter().map(|(t, _)| *t).min().unwrap_or(0);
    let max_ts = data.iter().map(|(t, _)| *t).max().unwrap_or(1);
    let min_val = data.iter().map(|(_, v)| *v).fold(f32::INFINITY, f32::min);
    let max_val = data
        .iter()
        .map(|(_, v)| *v)
        .fold(f32::NEG_INFINITY, f32::max);

    // Add some padding to value range
    let val_range = max_val - min_val;
    let val_padding = if val_range > 0.0 {
        val_range * 0.1
    } else {
        1.0
    };
    let min_val = min_val - val_padding;
    let max_val = max_val + val_padding;

    let ts_range = (max_ts - min_ts) as f32;
    let val_range = max_val - min_val;

    // Convert data to SVG coordinates
    let points: Vec<(f32, f32)> = data
        .iter()
        .map(|(ts, val)| {
            let x = if ts_range > 0.0 {
                padding as f32 + ((*ts - min_ts) as f32 / ts_range) * chart_width as f32
            } else {
                padding as f32 + chart_width as f32 / 2.0
            };
            let y = if val_range > 0.0 {
                padding as f32 + (1.0 - (val - min_val) / val_range) * chart_height as f32
            } else {
                padding as f32 + chart_height as f32 / 2.0
            };
            (x, y)
        })
        .collect();

    // Create path data for line
    let line_path = points
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

    // Create path data for filled area
    let area_path = if let (Some((first_x, _)), Some((last_x, _))) = (points.first(), points.last())
    {
        format!(
            "{} L {:.1} {} L {:.1} {} Z",
            line_path,
            last_x,
            height - padding,
            first_x,
            height - padding
        )
    } else {
        line_path.clone()
    };

    // Format axis labels
    let y_labels: Vec<(f32, String)> = (0..=4)
        .map(|i| {
            let val = min_val + (val_range * i as f32 / 4.0);
            let y = padding as f32 + (1.0 - i as f32 / 4.0) * chart_height as f32;
            (y, format!("{:.1}", val))
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
                {config.show_grid.then(|| view! {
                    <g class="opacity-20">
                        {y_labels.iter().map(|(y, _)| {
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
                })}

                // Y-axis labels
                <g class="text-xs fill-current opacity-70">
                    {y_labels.iter().map(|(y, label)| {
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

                // Chart title/label
                {(!config.label.is_empty()).then(|| view! {
                    <text
                        x={(width / 2).to_string()}
                        y="15"
                        text-anchor="middle"
                        class="fill-current font-semibold"
                        font-size="12"
                    >
                        {config.label.clone()}
                    </text>
                })}

                // Filled area
                <path
                    d={area_path}
                    fill={config.fill_color.clone()}
                />

                // Line
                <path
                    d={line_path}
                    fill="none"
                    stroke={config.line_color.clone()}
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                />

                // Data points
                {config.show_points.then(|| view! {
                    <g>
                        {points.iter().map(|(x, y)| {
                            view! {
                                <circle
                                    cx={format!("{:.1}", x)}
                                    cy={format!("{:.1}", y)}
                                    r="3"
                                    fill={config.line_color.clone()}
                                />
                            }
                        }).collect::<Vec<_>>()}
                    </g>
                })}
            </svg>
        </div>
    }
    .into_any()
}

/// Format timestamp for tooltip
fn format_tooltip_time(ts: u64) -> String {
    let secs = (ts / 1000) as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%d.%m %H:%M").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Dual line chart for temperature and humidity with hover tooltips
#[component]
pub fn DualLineChart(
    temp_data: Vec<DataPoint>,
    humidity_data: Vec<DataPoint>,
    #[prop(default = 600)] width: u32,
    #[prop(default = 250)] height: u32,
) -> impl IntoView {
    if temp_data.len() < 2 && humidity_data.len() < 2 {
        return view! {
            <div class="flex items-center justify-center h-32 bg-base-200 rounded-lg">
                <span class="opacity-50">"Not enough data for chart"</span>
            </div>
        }
        .into_any();
    }

    // Hover state: (x_position, temp_value, humidity_value, timestamp)
    let (hover_info, set_hover_info) = signal(Option::<(f32, Option<f32>, Option<f32>, u64)>::None);

    let padding: u32 = 50;
    let chart_width = width - 2 * padding;
    let chart_height = height - 2 * padding;

    // Find time range from both datasets
    let all_timestamps: Vec<u64> = temp_data
        .iter()
        .chain(humidity_data.iter())
        .map(|(t, _)| *t)
        .collect();

    let min_ts = all_timestamps.iter().min().copied().unwrap_or(0);
    let max_ts = all_timestamps.iter().max().copied().unwrap_or(1);
    let ts_range = (max_ts - min_ts) as f32;

    // Temperature range (left Y axis)
    let temp_min = temp_data
        .iter()
        .map(|(_, v)| *v)
        .fold(f32::INFINITY, f32::min);
    let temp_max = temp_data
        .iter()
        .map(|(_, v)| *v)
        .fold(f32::NEG_INFINITY, f32::max);
    let temp_range_raw = temp_max - temp_min;
    let temp_padding_val = if temp_range_raw > 0.0 {
        temp_range_raw * 0.1
    } else {
        5.0
    };
    let temp_min = temp_min - temp_padding_val;
    let temp_max = temp_max + temp_padding_val;
    let temp_range = temp_max - temp_min;

    // Humidity is always 0-100%
    let hum_min: f32 = 0.0;
    let hum_max: f32 = 100.0;
    let hum_range = hum_max - hum_min;

    // Convert temperature to SVG coordinates
    let temp_points: Vec<(f32, f32)> = temp_data
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

    // Convert humidity to SVG coordinates
    let hum_points: Vec<(f32, f32)> = humidity_data
        .iter()
        .map(|(ts, val)| {
            let x = if ts_range > 0.0 {
                padding as f32 + ((*ts - min_ts) as f32 / ts_range) * chart_width as f32
            } else {
                padding as f32 + chart_width as f32 / 2.0
            };
            let y = if hum_range > 0.0 {
                padding as f32 + (1.0 - (val - hum_min) / hum_range) * chart_height as f32
            } else {
                padding as f32 + chart_height as f32 / 2.0
            };
            (x, y)
        })
        .collect();

    // Create path data
    let temp_path = temp_points
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

    let hum_path = hum_points
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
            (y, format!("{:.0}°", val))
        })
        .collect();

    let hum_labels: Vec<(f32, String)> = (0..=4)
        .map(|i| {
            let val = hum_min + (hum_range * i as f32 / 4.0);
            let y = padding as f32 + (1.0 - i as f32 / 4.0) * chart_height as f32;
            (y, format!("{:.0}%", val))
        })
        .collect();

    // Clone data for hover handler
    let temp_data_for_hover = temp_data.clone();
    let humidity_data_for_hover = humidity_data.clone();

    view! {
        <div class="relative">
            <svg
                width={width.to_string()}
                height={height.to_string()}
                viewBox={format!("0 0 {} {}", width, height)}
                class="w-full h-auto cursor-crosshair"
                on:mousemove=move |ev| {
                    // Get mouse position relative to SVG
                    let target = ev.target().unwrap();
                    let svg = target.dyn_ref::<web_sys::SvgsvgElement>();
                    if let Some(svg) = svg {
                        let rect = svg.get_bounding_client_rect();
                        let scale_x = width as f64 / rect.width();
                        let mouse_x = ((ev.client_x() as f64 - rect.left()) * scale_x) as f32;

                        // Convert mouse X to timestamp
                        if mouse_x >= padding as f32 && mouse_x <= (width - padding) as f32 {
                            let ratio = (mouse_x - padding as f32) / chart_width as f32;
                            let ts = min_ts + (ratio * ts_range) as u64;

                            // Find nearest data points
                            let temp_val = temp_data_for_hover.iter()
                                .min_by_key(|(t, _)| (*t as i64 - ts as i64).abs())
                                .map(|(_, v)| *v);

                            let hum_val = humidity_data_for_hover.iter()
                                .min_by_key(|(t, _)| (*t as i64 - ts as i64).abs())
                                .map(|(_, v)| *v);

                            set_hover_info.set(Some((mouse_x, temp_val, hum_val, ts)));
                        }
                    }
                }
                on:mouseleave=move |_| {
                    set_hover_info.set(None);
                }
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

                // Left Y-axis labels (Temperature)
                <g class="text-xs fill-primary opacity-70">
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

                // Right Y-axis labels (Humidity)
                <g class="text-xs fill-secondary opacity-70">
                    {hum_labels.iter().map(|(y, label)| {
                        view! {
                            <text
                                x={(width - padding + 5).to_string()}
                                y={format!("{:.1}", y + 4.0)}
                                text-anchor="start"
                                font-size="10"
                            >
                                {label.clone()}
                            </text>
                        }
                    }).collect::<Vec<_>>()}
                </g>

                // X-axis time labels
                <g class="text-xs fill-current opacity-50">
                    {
                        // Generate 5 time labels across the X axis
                        let time_labels: Vec<(f32, String)> = (0..=4).map(|i| {
                            let ratio = i as f32 / 4.0;
                            let x = padding as f32 + ratio * chart_width as f32;
                            let ts = min_ts + (ratio * ts_range) as u64;
                            let label = format_tooltip_time(ts);
                            (x, label)
                        }).collect();

                        time_labels.into_iter().map(|(x, label)| {
                            view! {
                                <text
                                    x={format!("{:.1}", x)}
                                    y={(height - padding + 15).to_string()}
                                    text-anchor="middle"
                                    font-size="9"
                                >
                                    {label}
                                </text>
                            }
                        }).collect::<Vec<_>>()
                    }
                </g>

                // Temperature line
                {(!temp_path.is_empty()).then(|| view! {
                    <path
                        d={temp_path.clone()}
                        fill="none"
                        stroke="#f97316"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        style="pointer-events: none;"
                    />
                })}

                // Humidity line
                {(!hum_path.is_empty()).then(|| view! {
                    <path
                        d={hum_path.clone()}
                        fill="none"
                        stroke="#06b6d4"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        style="pointer-events: none;"
                    />
                })}

                // Hover vertical line and tooltip
                {move || {
                    if let Some((x, temp, hum, ts)) = hover_info.get() {
                        let time_str = format_tooltip_time(ts);
                        let temp_str = temp.map(|t| format!("{:.1}°C", t)).unwrap_or_else(|| "-".to_string());
                        let hum_str = hum.map(|h| format!("{:.1}%", h)).unwrap_or_else(|| "-".to_string());

                        view! {
                            <g>
                                // Vertical line
                                <line
                                    x1={format!("{:.1}", x)}
                                    y1={padding.to_string()}
                                    x2={format!("{:.1}", x)}
                                    y2={(height - padding).to_string()}
                                    stroke="currentColor"
                                    stroke-width="1"
                                    stroke-dasharray="4 2"
                                    class="opacity-50"
                                    style="pointer-events: none;"
                                />

                                // Tooltip background
                                <rect
                                    x={format!("{:.1}", (x - 60.0).max(padding as f32))}
                                    y={(padding - 5).to_string()}
                                    width="120"
                                    height="50"
                                    rx="4"
                                    fill="oklch(var(--b2))"
                                    stroke="oklch(var(--bc))"
                                    stroke-width="1"
                                    class="opacity-95"
                                    style="pointer-events: none;"
                                />

                                // Tooltip text - Time
                                <text
                                    x={format!("{:.1}", (x - 55.0).max(padding as f32 + 5.0))}
                                    y={(padding + 10).to_string()}
                                    font-size="10"
                                    fill="currentColor"
                                    style="pointer-events: none;"
                                >
                                    {time_str}
                                </text>

                                // Tooltip text - Temperature
                                <text
                                    x={format!("{:.1}", (x - 55.0).max(padding as f32 + 5.0))}
                                    y={(padding + 24).to_string()}
                                    font-size="11"
                                    fill="#f97316"
                                    font-weight="bold"
                                    style="pointer-events: none;"
                                >
                                    {"Temp: "}{temp_str}
                                </text>

                                // Tooltip text - Humidity
                                <text
                                    x={format!("{:.1}", (x - 55.0).max(padding as f32 + 5.0))}
                                    y={(padding + 38).to_string()}
                                    font-size="11"
                                    fill="#06b6d4"
                                    font-weight="bold"
                                    style="pointer-events: none;"
                                >
                                    {"Hum: "}{hum_str}
                                </text>
                            </g>
                        }.into_any()
                    } else {
                        view! { <g></g> }.into_any()
                    }
                }}
            </svg>

            // Legend
            <div class="flex justify-center gap-6 mt-2 text-sm">
                <div class="flex items-center gap-2">
                    <div class="w-4 h-1 bg-orange-500 rounded"></div>
                    <span class="opacity-70">"Temperature"</span>
                </div>
                <div class="flex items-center gap-2">
                    <div class="w-4 h-1 bg-cyan-500 rounded"></div>
                    <span class="opacity-70">"Humidity"</span>
                </div>
            </div>
        </div>
    }
    .into_any()
}
