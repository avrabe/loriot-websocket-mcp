//! Analytics page

use leptos::prelude::*;
use loriot_core::{AnalyticsReport, DataGapSeverity, TimeWindow};

use crate::api::{fetch_analytics, fetch_data_range, AnalyticsQuery};
use crate::components::{Navbar, TimeSelectorWithNav};

fn format_severity(severity: &DataGapSeverity) -> (&'static str, &'static str) {
    match severity {
        DataGapSeverity::Minor => ("Minor", "bg-green-100 text-green-800"),
        DataGapSeverity::Moderate => ("Moderate", "bg-yellow-100 text-yellow-800"),
        DataGapSeverity::Major => ("Major", "bg-orange-100 text-orange-800"),
        DataGapSeverity::Critical => ("Critical", "bg-red-100 text-red-800"),
    }
}

/// Analytics page showing comprehensive data analysis
#[component]
pub fn AnalyticsPage() -> impl IntoView {
    let (time_window, set_time_window) = signal(TimeWindow::LastDay);
    let (time_offset, set_time_offset) = signal(0i32);
    let (report, set_report) = signal(Option::<AnalyticsReport>::None);
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

    // Fetch analytics when time window or offset changes
    Effect::new(move |_| {
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

            let query = if duration > 0 {
                let end = now - (off as u64 * duration);
                let start = end - duration;
                leptos::logging::log!("Fetching analytics with time range: start={}, end={}", start, end);
                AnalyticsQuery::new().with_time_range(start, end)
            } else {
                leptos::logging::log!("Fetching analytics for All Time");
                AnalyticsQuery::new().with_time_window(&tw)
            };

            match fetch_analytics(None, query).await {
                Ok(analytics) => {
                    leptos::logging::log!("Analytics received: {} readings", analytics.analysis_period.total_readings);
                    set_report.set(Some(analytics));
                }
                Err(e) => {
                    leptos::logging::error!("Failed to fetch analytics: {}", e);
                }
            }
            set_loading.set(false);
        });
    });

    view! {
        <div>
            <Navbar />

            <div class="container mx-auto p-4">
                <div class="flex justify-between items-center mb-6">
                    <h1 class="text-3xl font-bold">"Analytics Dashboard"</h1>
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
                    } else if let Some(analytics) = report.get() {
                        view! {
                            // Analysis period info
                            <div class="alert alert-info mb-6">
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="stroke-current shrink-0 w-6 h-6">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
                                </svg>
                                <span>
                                    {format!("Analyzing {} readings over {:.1} hours",
                                        analytics.analysis_period.total_readings,
                                        analytics.analysis_period.duration_hours
                                    )}
                                </span>
                            </div>

                            // Environmental metrics
                            <div class="card bg-base-100 shadow-xl mb-6">
                                <div class="card-body">
                                    <h2 class="card-title">"Environmental Metrics"</h2>
                                    <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                                        <div class="stat bg-base-200 rounded-lg">
                                            <div class="stat-title">"Heat Index"</div>
                                            <div class="stat-value text-warning">
                                                {format!("{:.1}°C", analytics.environmental.heat_index.average)}
                                            </div>
                                            <div class="stat-desc">
                                                {format!("Min: {:.1}°C / Max: {:.1}°C",
                                                    analytics.environmental.heat_index.min,
                                                    analytics.environmental.heat_index.max
                                                )}
                                            </div>
                                        </div>
                                        <div class="stat bg-base-200 rounded-lg">
                                            <div class="stat-title">"Dew Point"</div>
                                            <div class="stat-value text-info">
                                                {format!("{:.1}°C", analytics.environmental.dew_point.average)}
                                            </div>
                                            <div class="stat-desc">
                                                {format!("Min: {:.1}°C / Max: {:.1}°C",
                                                    analytics.environmental.dew_point.min,
                                                    analytics.environmental.dew_point.max
                                                )}
                                            </div>
                                        </div>
                                        <div class="stat bg-base-200 rounded-lg">
                                            <div class="stat-title">"Comfort Zone"</div>
                                            <div class="stat-value text-success">
                                                {format!("{:.0}%", analytics.environmental.comfort_zone.comfort_percentage)}
                                            </div>
                                            <div class="stat-desc">"Time in optimal range"</div>
                                        </div>
                                    </div>
                                </div>
                            </div>

                            // Temperature stats
                            <div class="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
                                <div class="card bg-base-100 shadow-xl">
                                    <div class="card-body">
                                        <h2 class="card-title text-primary">"Temperature"</h2>
                                        <div class="stats stats-vertical shadow">
                                            <div class="stat">
                                                <div class="stat-title">"Average"</div>
                                                <div class="stat-value text-primary">
                                                    {format!("{:.1}°C", analytics.environmental.heat_index.average)}
                                                </div>
                                            </div>
                                            <div class="stat">
                                                <div class="stat-title">"Range"</div>
                                                <div class="stat-value text-sm">
                                                    {format!("{:.1}°C - {:.1}°C",
                                                        analytics.environmental.heat_index.min,
                                                        analytics.environmental.heat_index.max
                                                    )}
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                </div>

                                <div class="card bg-base-100 shadow-xl">
                                    <div class="card-body">
                                        <h2 class="card-title text-secondary">"Humidity"</h2>
                                        <div class="stats stats-vertical shadow">
                                            <div class="stat">
                                                <div class="stat-title">"Average Dew Point"</div>
                                                <div class="stat-value text-secondary">
                                                    {format!("{:.1}°C", analytics.environmental.dew_point.average)}
                                                </div>
                                            </div>
                                            <div class="stat">
                                                <div class="stat-title">"Dew Point Range"</div>
                                                <div class="stat-value text-sm">
                                                    {format!("{:.1}°C - {:.1}°C",
                                                        analytics.environmental.dew_point.min,
                                                        analytics.environmental.dew_point.max
                                                    )}
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </div>

                            // Data quality
                            <div class="card bg-base-100 shadow-xl mb-6">
                                <div class="card-body">
                                    <h2 class="card-title">"Data Quality"</h2>
                                    <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                                        <div>
                                            <div class="flex justify-between mb-1">
                                                <span>"Completeness"</span>
                                                <span>{format!("{:.0}%", analytics.data_quality.completeness_score * 100.0)}</span>
                                            </div>
                                            <progress class="progress progress-primary w-full"
                                                value={format!("{}", (analytics.data_quality.completeness_score * 100.0) as u32)}
                                                max="100"></progress>
                                        </div>
                                        <div>
                                            <div class="flex justify-between mb-1">
                                                <span>"Consistency"</span>
                                                <span>{format!("{:.0}%", analytics.data_quality.consistency_score * 100.0)}</span>
                                            </div>
                                            <progress class="progress progress-secondary w-full"
                                                value={format!("{}", (analytics.data_quality.consistency_score * 100.0) as u32)}
                                                max="100"></progress>
                                        </div>
                                        <div>
                                            <div class="flex justify-between mb-1">
                                                <span>"Accuracy"</span>
                                                <span>{format!("{:.0}%", analytics.data_quality.accuracy_score * 100.0)}</span>
                                            </div>
                                            <progress class="progress progress-accent w-full"
                                                value={format!("{}", (analytics.data_quality.accuracy_score * 100.0) as u32)}
                                                max="100"></progress>
                                        </div>
                                    </div>
                                    <div class="text-sm opacity-70 mt-2">
                                        {format!("Missing readings: {} | Outliers: {}",
                                            analytics.data_quality.missing_readings_count,
                                            analytics.data_quality.outlier_count
                                        )}
                                    </div>
                                </div>
                            </div>

                            // Data gaps section
                            {
                                let gaps = analytics.data_quality.data_gaps.clone();
                                if !gaps.is_empty() {
                                    view! {
                                        <div class="card bg-base-100 shadow-xl mb-6">
                                            <div class="card-body">
                                                <h2 class="card-title text-error">"Data Gaps Detected"</h2>
                                                <p class="text-sm opacity-70 mb-4">
                                                    "Periods where sensor data was expected but not received (expected interval: 20 minutes)"
                                                </p>
                                                <div class="overflow-x-auto">
                                                    <table class="table table-sm">
                                                        <thead>
                                                            <tr>
                                                                <th>"Severity"</th>
                                                                <th>"Start Time"</th>
                                                                <th>"End Time"</th>
                                                                <th>"Duration"</th>
                                                                <th>"Missing Readings"</th>
                                                            </tr>
                                                        </thead>
                                                        <tbody>
                                                            {gaps.iter().map(|gap| {
                                                                let (severity_text, severity_class) = format_severity(&gap.severity);
                                                                let start_time = format!("{}", gap.start_timestamp);
                                                                let end_time = format!("{}", gap.end_timestamp);
                                                                let duration = format!("{:.1} mins", gap.duration_minutes);
                                                                
                                                                view! {
                                                                    <tr>
                                                                        <td>
                                                                            <span class=format!("px-2 py-1 rounded-full text-xs font-semibold {}", severity_class)>
                                                                                {severity_text}
                                                                            </span>
                                                                        </td>
                                                                        <td class="font-mono text-sm">{start_time}</td>
                                                                        <td class="font-mono text-sm">{end_time}</td>
                                                                        <td>{duration}</td>
                                                                        <td class="text-center">{gap.expected_readings}</td>
                                                                    </tr>
                                                                }
                                                            }).collect::<Vec<_>>()}
                                                        </tbody>
                                                    </table>
                                                </div>
                                                <div class="mt-4 p-4 bg-base-200 rounded-lg">
                                                    <h3 class="font-semibold mb-2">"Gap Analysis Summary"</h3>
                                                    <div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                                                        <div>
                                                            <span class="font-semibold">"Total Gaps:"</span> 
                                                            <span>{gaps.len()}</span>
                                                        </div>
                                                        <div>
                                                            <span class="font-semibold">"Total Missing Readings:"</span> 
                                                            <span>{gaps.iter().map(|g| g.expected_readings).sum::<usize>()}</span>
                                                        </div>
                                                        <div>
                                                            <span class="font-semibold">"Longest Gap:"</span> 
                                                            <span>{format!("{:.1} mins", gaps.iter().map(|g| g.duration_minutes).fold(0.0, f64::max))}</span>
                                                        </div>
                                                        <div>
                                                            <span class="font-semibold">"Critical Gaps:"</span> 
                                                            <span>{gaps.iter().filter(|g| matches!(g.severity, DataGapSeverity::Critical)).count()}</span>
                                                        </div>
                                                    </div>
                                                </div>
                                            </div>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="alert alert-success mb-6">
                                            <svg xmlns="http://www.w3.org/2000/svg" class="stroke-current shrink-0 h-6 w-6" fill="none" viewBox="0 0 24 24">
                                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                                            </svg>
                                            <span>"No data gaps detected. All sensors reporting as expected."</span>
                                        </div>
                                    }.into_any()
                                }
                            }

                            // Dangerous periods
                            {
                                let dangerous = analytics.environmental.heat_index.dangerous_periods.clone();
                                if !dangerous.is_empty() {
                                    view! {
                                        <div class="card bg-base-100 shadow-xl">
                                            <div class="card-body">
                                                <h2 class="card-title text-error">"Dangerous Heat Index Periods"</h2>
                                                <div class="overflow-x-auto">
                                                    <table class="table table-sm">
                                                        <thead>
                                                            <tr>
                                                                <th>"Risk Level"</th>
                                                                <th>"Max Heat Index"</th>
                                                            </tr>
                                                        </thead>
                                                        <tbody>
                                                            {dangerous.iter().take(5).map(|period| {
                                                                let risk = format!("{:?}", period.risk_level);
                                                                view! {
                                                                    <tr>
                                                                        <td class="font-semibold text-error">{risk}</td>
                                                                        <td>{format!("{:.1}°C", period.max_heat_index)}</td>
                                                                    </tr>
                                                                }
                                                            }).collect::<Vec<_>>()}
                                                        </tbody>
                                                    </table>
                                                </div>
                                            </div>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="alert alert-success">
                                            <svg xmlns="http://www.w3.org/2000/svg" class="stroke-current shrink-0 h-6 w-6" fill="none" viewBox="0 0 24 24">
                                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                                            </svg>
                                            <span>"No dangerous temperature or humidity periods detected."</span>
                                        </div>
                                    }.into_any()
                                }
                            }
                        }.into_any()
                    } else {
                        view! {
                            <div class="alert alert-warning">
                                <span>"No analytics data available"</span>
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}
