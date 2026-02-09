//! Energy Tracker Page - WEG Heating Analysis

use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::api::{
    fetch_energy_overview, fetch_energy_scenarios, fetch_energy_consumption,
    update_energy_building, update_energy_prices, add_energy_consumption,
    delete_energy_consumption, update_energy_apartments,
    ApartmentUnit, CostDistributionSettings, UpdateApartmentsRequest,
    EnergyOverview, ScenarioComparison, ScenarioAnalysis, MonthlyConsumption,
    MonthlyScenario, UpdateBuildingRequest, UpdatePricesRequest, AddConsumptionRequest,
    DeleteConsumptionRequest, YearlyTotal,
};
use crate::components::Navbar;

/// Month names in German
const MONTH_NAMES: [&str; 12] = [
    "Januar", "Februar", "März", "April", "Mai", "Juni",
    "Juli", "August", "September", "Oktober", "November", "Dezember"
];

/// Energy Tracker page component
#[component]
pub fn EnergyPage() -> impl IntoView {
    // Main data signals
    let overview = RwSignal::new(None::<EnergyOverview>);
    let scenarios = RwSignal::new(None::<ScenarioComparison>);
    let consumption = RwSignal::new(Vec::<MonthlyConsumption>::new());
    let error = RwSignal::new(None::<String>);
    let loading = RwSignal::new(false);

    // Selected scenario (0 = Current, 1 = Hybrid, 2 = Full HP)
    let scenario_position = RwSignal::new(0i32);
    let selected_year = RwSignal::new(None::<u32>);

    // Modal state
    let show_building_modal = RwSignal::new(false);
    let show_prices_modal = RwSignal::new(false);
    let show_consumption_modal = RwSignal::new(false);
    let show_apartments_modal = RwSignal::new(false);
    let show_export_modal = RwSignal::new(false);

    // Load initial data
    Effect::new(move |_| {
        loading.set(true);
        spawn_local(async move {
            // Fetch overview
            match fetch_energy_overview().await {
                Ok(o) => {
                    if let Some(year) = o.latest_year {
                        selected_year.set(Some(year));
                    }
                    overview.set(Some(o));
                }
                Err(e) => error.set(Some(e)),
            }

            // Fetch consumption
            match fetch_energy_consumption().await {
                Ok(c) => consumption.set(c.consumption),
                Err(e) => error.set(Some(e)),
            }

            loading.set(false);
        });
    });

    // Load scenarios when year changes
    Effect::new(move |_| {
        let year = selected_year.get();
        if year.is_some() {
            spawn_local(async move {
                if let Ok(s) = fetch_energy_scenarios(year).await {
                    scenarios.set(Some(s));
                }
            });
        }
    });

    // Derived scenario based on slider position
    let current_scenario = Signal::derive(move || {
        scenarios.get().map(|s| match scenario_position.get() {
            0 => s.current.clone(),
            1 => s.hybrid.clone(),
            _ => s.full_hp.clone(),
        })
    });

    // Reload data after changes
    let reload_data = move || {
        spawn_local(async move {
            if let Ok(o) = fetch_energy_overview().await {
                overview.set(Some(o));
            }
            if let Ok(c) = fetch_energy_consumption().await {
                consumption.set(c.consumption);
            }
            let year = selected_year.get();
            if let Ok(s) = fetch_energy_scenarios(year).await {
                scenarios.set(Some(s));
            }
        });
    };

    view! {
        <div>
            <Navbar />

            <div class="container mx-auto p-4 space-y-6">
                // Header
                <div class="flex flex-wrap justify-between items-center gap-4">
                    <div>
                        <h1 class="text-3xl font-bold">"Energie-Tracker"</h1>
                        <p class="text-sm opacity-70">"WEG Heizungsanalyse & Szenarienvergleich"</p>
                    </div>
                    <div class="flex gap-2">
                        <button
                            class="btn btn-outline btn-sm"
                            on:click=move |_| show_export_modal.set(true)
                        >
                            "Export"
                        </button>
                    </div>
                </div>

                // Error display
                {move || error.get().map(|e| view! {
                    <div class="alert alert-error">
                        <span>{e}</span>
                        <button class="btn btn-sm btn-ghost" on:click=move |_| error.set(None)>"×"</button>
                    </div>
                })}

                // Loading indicator
                <Show when=move || loading.get()>
                    <div class="flex justify-center py-8">
                        <span class="loading loading-spinner loading-lg"></span>
                    </div>
                </Show>

                // Main content when loaded
                <Show when=move || !loading.get()>
                    // Quick stats row
                    {move || overview.get().map(|o| view! {
                        <QuickStatsRow
                            overview=o.clone()
                            on_edit_building=move |_| show_building_modal.set(true)
                            on_edit_prices=move |_| show_prices_modal.set(true)
                            on_edit_apartments=move |_| show_apartments_modal.set(true)
                        />
                    })}

                    // Scenario comparison section
                    {move || scenarios.get().map(|s| view! {
                        <ScenarioSection
                            scenarios=s.clone()
                            position=scenario_position
                            current_scenario=current_scenario
                        />
                    })}

                    // Yearly totals
                    {move || overview.get().map(|o| {
                        if o.yearly_totals.is_empty() {
                            view! {
                                <div class="alert alert-info">
                                    <span>"Noch keine Verbrauchsdaten vorhanden. Füge monatliche Daten hinzu."</span>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <YearlyTotalsCard
                                    totals=o.yearly_totals.clone()
                                    selected_year=selected_year
                                    available_years=o.available_years.clone()
                                />
                            }.into_any()
                        }
                    })}

                    // Monthly data table
                    <MonthlyDataCard
                        consumption=consumption
                        on_add=move |_| show_consumption_modal.set(true)
                        on_delete=move |(year, month): (u32, u32)| {
                            let reload = reload_data;
                            spawn_local(async move {
                                let _ = delete_energy_consumption(DeleteConsumptionRequest { year, month }).await;
                                reload();
                            });
                        }
                        selected_year=selected_year
                    />

                    // Monthly scenario chart
                    {move || scenarios.get().map(|s| view! {
                        <MonthlyChartCard monthly=s.monthly.clone() />
                    })}
                </Show>
            </div>

            // Building Modal
            <BuildingModal
                show=show_building_modal
                overview=overview
                on_save=move |_| {
                    reload_data();
                }
            />

            // Prices Modal
            <PricesModal
                show=show_prices_modal
                overview=overview
                on_save=move |_| {
                    reload_data();
                }
            />

            // Add Consumption Modal
            <ConsumptionModal
                show=show_consumption_modal
                on_save=move |_| {
                    reload_data();
                }
            />

            // Apartments Modal
            <ApartmentsModal
                show=show_apartments_modal
                overview=overview
                on_save=move |_| {
                    reload_data();
                }
            />

            // Export Modal
            <ExportModal
                show=show_export_modal
                overview=overview
                scenarios=scenarios
                consumption=consumption
            />
        </div>
    }
}

/// Quick stats row with building info and prices
#[component]
fn QuickStatsRow(
    overview: EnergyOverview,
    on_edit_building: impl Fn(()) + 'static + Copy,
    on_edit_prices: impl Fn(()) + 'static + Copy,
    on_edit_apartments: impl Fn(()) + 'static + Copy,
) -> impl IntoView {
    let building = overview.building;
    let prices = overview.prices;
    let apartments = overview.apartments;
    let cost_distribution = overview.cost_distribution;

    // Find reference apartment
    let reference_apt = apartments.iter().find(|a| a.is_reference);
    let total_units = apartments.len();
    let total_area: f64 = apartments.iter().map(|a| a.area_m2).sum();

    view! {
        <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
            // Building card
            <div class="card bg-base-200 cursor-pointer hover:bg-base-300 transition-colors" on:click=move |_| on_edit_building(())>
                <div class="card-body p-4">
                    <div class="flex justify-between items-start">
                        <h3 class="card-title text-lg">"Gebäude"</h3>
                        <span class="badge badge-ghost">"Bearbeiten"</span>
                    </div>
                    <div class="grid grid-cols-2 gap-2 text-sm">
                        <div>
                            <span class="opacity-70">"Fläche: "</span>
                            <strong>{format!("{:.0} m²", building.area_m2)}</strong>
                        </div>
                        <div>
                            <span class="opacity-70">"Einheiten: "</span>
                            <strong>{building.units}</strong>
                        </div>
                        <div>
                            <span class="opacity-70">"Baujahr: "</span>
                            <strong>{building.year_built}</strong>
                        </div>
                        <div>
                            <span class="opacity-70">"Heizung: "</span>
                            <strong>{format_heating_system(&building.heating_system)}</strong>
                        </div>
                    </div>
                </div>
            </div>

            // Prices card
            <div class="card bg-base-200 cursor-pointer hover:bg-base-300 transition-colors" on:click=move |_| on_edit_prices(())>
                <div class="card-body p-4">
                    <div class="flex justify-between items-start">
                        <h3 class="card-title text-lg">"Energiepreise"</h3>
                        <span class="badge badge-ghost">"Bearbeiten"</span>
                    </div>
                    <div class="grid grid-cols-3 gap-2 text-sm">
                        <div>
                            <span class="opacity-70">"Gas"</span>
                            <div class="font-bold text-warning">{format!("{:.1} ct/kWh", prices.gas_ct_kwh)}</div>
                        </div>
                        <div>
                            <span class="opacity-70">"Strom"</span>
                            <div class="font-bold text-info">{format!("{:.1} ct/kWh", prices.electricity_ct_kwh)}</div>
                        </div>
                        <div>
                            <span class="opacity-70">"WP-Tarif"</span>
                            <div class="font-bold text-success">{format!("{:.1} ct/kWh", prices.electricity_hp_ct_kwh)}</div>
                        </div>
                    </div>
                </div>
            </div>

            // Apartments/Units card
            <div class="card bg-base-200 cursor-pointer hover:bg-base-300 transition-colors" on:click=move |_| on_edit_apartments(())>
                <div class="card-body p-4">
                    <div class="flex justify-between items-start">
                        <h3 class="card-title text-lg">"Wohneinheiten"</h3>
                        <span class="badge badge-ghost">"Bearbeiten"</span>
                    </div>
                    <div class="grid grid-cols-2 gap-2 text-sm">
                        <div>
                            <span class="opacity-70">"Erfasst: "</span>
                            <strong>{format!("{} / {}", total_units, building.units)}</strong>
                        </div>
                        <div>
                            <span class="opacity-70">"Fläche: "</span>
                            <strong>{format!("{:.0} m²", total_area)}</strong>
                        </div>
                        {reference_apt.map(|apt| view! {
                            <div class="col-span-2">
                                <span class="opacity-70">"Referenz: "</span>
                                <strong>{format!("{} ({:.0} m²)", apt.name.clone(), apt.area_m2)}</strong>
                            </div>
                        })}
                        <div class="col-span-2">
                            <span class="opacity-70">"Verteilung: "</span>
                            <strong>{format!("{:.0}% Grund / {:.0}% Verbr.", cost_distribution.base_cost_percent, 100.0 - cost_distribution.base_cost_percent)}</strong>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Scenario comparison section with slider
#[component]
fn ScenarioSection(
    scenarios: ScenarioComparison,
    position: RwSignal<i32>,
    current_scenario: Signal<Option<ScenarioAnalysis>>,
) -> impl IntoView {
    let current = scenarios.current.clone();
    let hybrid = scenarios.hybrid.clone();
    let full_hp = scenarios.full_hp.clone();

    // Calculate savings
    let current_cost = current.annual_cost_eur;
    let hybrid_savings = ((current_cost - hybrid.annual_cost_eur) / current_cost * 100.0).max(0.0);
    let fullhp_savings = ((current_cost - full_hp.annual_cost_eur) / current_cost * 100.0).max(0.0);

    let current_co2 = current.annual_co2_kg;
    let hybrid_co2_savings = ((current_co2 - hybrid.annual_co2_kg) / current_co2 * 100.0).max(0.0);
    let fullhp_co2_savings = ((current_co2 - full_hp.annual_co2_kg) / current_co2 * 100.0).max(0.0);

    view! {
        <div class="card bg-gradient-to-br from-base-200 to-base-300">
            <div class="card-body">
                <h2 class="card-title">"Szenarienvergleich " {scenarios.year}</h2>

                // Slider
                <div class="py-4">
                    <div class="flex justify-between text-sm mb-2">
                        <span class=move || if position.get() == 0 { "font-bold text-warning" } else { "opacity-70" }>
                            "Gas aktuell"
                        </span>
                        <span class=move || if position.get() == 1 { "font-bold text-info" } else { "opacity-70" }>
                            "Hybrid WP"
                        </span>
                        <span class=move || if position.get() == 2 { "font-bold text-success" } else { "opacity-70" }>
                            "Voll WP"
                        </span>
                    </div>
                    <input
                        type="range"
                        min="0"
                        max="2"
                        prop:value=move || position.get()
                        class="range range-primary w-full"
                        step="1"
                        on:input=move |ev| {
                            let value: i32 = event_target_value(&ev).parse().unwrap_or(0);
                            position.set(value);
                        }
                    />
                    <div class="w-full flex justify-between text-xs px-2">
                        <span>"|"</span>
                        <span>"|"</span>
                        <span>"|"</span>
                    </div>
                </div>

                // Current scenario stats
                {move || current_scenario.get().map(|s| {
                    let total_kwh = s.gas_kwh + s.electricity_kwh;
                    view! {
                        <div class="stats stats-vertical lg:stats-horizontal shadow w-full bg-base-100">
                            <div class="stat">
                                <div class="stat-figure text-primary">
                                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-8 h-8 stroke-current">
                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z"></path>
                                    </svg>
                                </div>
                                <div class="stat-title">"Energieverbrauch"</div>
                                <div class="stat-value text-lg">{format!("{:.0} kWh", total_kwh)}</div>
                                <div class="stat-desc">
                                    {format!("Gas: {:.0} | Strom: {:.0}", s.gas_kwh, s.electricity_kwh)}
                                </div>
                            </div>

                            <div class="stat">
                                <div class="stat-figure text-secondary">
                                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-8 h-8 stroke-current">
                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8c-1.657 0-3 .895-3 2s1.343 2 3 2 3 .895 3 2-1.343 2-3 2m0-8c1.11 0 2.08.402 2.599 1M12 8V7m0 1v8m0 0v1m0-1c-1.11 0-2.08-.402-2.599-1M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
                                    </svg>
                                </div>
                                <div class="stat-title">"Jahreskosten"</div>
                                <div class="stat-value text-lg">{format!("{:.0} EUR", s.annual_cost_eur)}</div>
                                <div class="stat-desc">
                                    {format!("WP-Anteil: {:.0}%", s.hp_share * 100.0)}
                                </div>
                            </div>

                            <div class="stat">
                                <div class="stat-figure text-success">
                                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="inline-block w-8 h-8 stroke-current">
                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
                                    </svg>
                                </div>
                                <div class="stat-title">"CO2-Emission"</div>
                                <div class="stat-value text-lg">{format!("{:.0} kg", s.annual_co2_kg)}</div>
                                <div class="stat-desc">{format!("Ø COP: {:.1}", s.avg_cop)}</div>
                            </div>
                        </div>
                    }
                })}

                // Comparison badges
                <div class="flex flex-wrap gap-4 mt-4 justify-center">
                    <div class="badge badge-lg badge-info gap-2">
                        "Hybrid: "
                        <span class="font-bold">{format!("-{:.0}% Kosten", hybrid_savings)}</span>
                        " | "
                        <span class="font-bold">{format!("-{:.0}% CO2", hybrid_co2_savings)}</span>
                    </div>
                    <div class="badge badge-lg badge-success gap-2">
                        "Voll WP: "
                        <span class="font-bold">{format!("-{:.0}% Kosten", fullhp_savings)}</span>
                        " | "
                        <span class="font-bold">{format!("-{:.0}% CO2", fullhp_co2_savings)}</span>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Yearly totals card
#[component]
fn YearlyTotalsCard(
    totals: Vec<YearlyTotal>,
    selected_year: RwSignal<Option<u32>>,
    available_years: Vec<u32>,
) -> impl IntoView {
    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <div class="flex justify-between items-center">
                    <h2 class="card-title">"Jahresübersicht"</h2>
                    <select
                        class="select select-bordered select-sm"
                        on:change=move |ev| {
                            let value: u32 = event_target_value(&ev).parse().unwrap_or(2024);
                            selected_year.set(Some(value));
                        }
                    >
                        {available_years.iter().map(|&year| {
                            view! {
                                <option
                                    value={year.to_string()}
                                    selected=move || selected_year.get() == Some(year)
                                >
                                    {year}
                                </option>
                            }
                        }).collect::<Vec<_>>()}
                    </select>
                </div>

                <div class="overflow-x-auto">
                    <table class="table table-zebra">
                        <thead>
                            <tr>
                                <th>"Jahr"</th>
                                <th class="text-right">"Heizung"</th>
                                <th class="text-right">"Warmwasser"</th>
                                <th class="text-right">"Gesamt"</th>
                                <th class="text-right">"Monate"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {totals.iter().map(|t| {
                                let year = t.year;
                                view! {
                                    <tr class=move || if selected_year.get() == Some(year) { "bg-primary/20" } else { "" }>
                                        <td class="font-bold">{t.year}</td>
                                        <td class="text-right">{format!("{:.0} kWh", t.heating_kwh)}</td>
                                        <td class="text-right">{format!("{:.0} kWh", t.hot_water_kwh)}</td>
                                        <td class="text-right font-bold">{format!("{:.0} kWh", t.total_kwh)}</td>
                                        <td class="text-right">
                                            <span class="badge badge-sm">{t.months_with_data}" / 12"</span>
                                        </td>
                                    </tr>
                                }
                            }).collect::<Vec<_>>()}
                        </tbody>
                    </table>
                </div>
            </div>
        </div>
    }
}

/// Monthly data card with consumption table
#[component]
fn MonthlyDataCard(
    consumption: RwSignal<Vec<MonthlyConsumption>>,
    on_add: impl Fn(()) + 'static + Copy + Send + Sync,
    on_delete: impl Fn((u32, u32)) + 'static + Copy + Send + Sync,
    selected_year: RwSignal<Option<u32>>,
) -> impl IntoView {
    let filtered = Signal::derive(move || {
        let year = selected_year.get();
        let mut data: Vec<_> = consumption.get().into_iter()
            .filter(|c| year.is_none_or(|y| c.year == y))
            .collect();
        data.sort_by(|a, b| {
            a.year.cmp(&b.year).reverse()
                .then(a.month.cmp(&b.month).reverse())
        });
        data
    });

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <div class="flex justify-between items-center">
                    <h2 class="card-title">"Monatliche Verbrauchsdaten"</h2>
                    <button class="btn btn-primary btn-sm" on:click=move |_| on_add(())>
                        "+ Hinzufügen"
                    </button>
                </div>

                <Show
                    when=move || !filtered.get().is_empty()
                    fallback=|| view! {
                        <div class="text-center py-8 opacity-70">
                            "Keine Daten für das gewählte Jahr"
                        </div>
                    }
                >
                    <div class="overflow-x-auto">
                        <table class="table table-zebra table-sm">
                            <thead>
                                <tr>
                                    <th>"Monat"</th>
                                    <th class="text-right">"Heizung"</th>
                                    <th class="text-right">"Warmwasser"</th>
                                    <th class="text-right">"Gesamt"</th>
                                    <th>"Quelle"</th>
                                    <th></th>
                                </tr>
                            </thead>
                            <tbody>
                                <For
                                    each=move || filtered.get()
                                    key=|c| (c.year, c.month)
                                    children=move |c| {
                                        let year = c.year;
                                        let month = c.month;
                                        let month_name = MONTH_NAMES.get((c.month - 1) as usize).unwrap_or(&"?").to_string();
                                        let total = c.heating_kwh + c.hot_water_kwh;
                                        let year_str = c.year.to_string();

                                        view! {
                                            <tr>
                                                <td>
                                                    <span class="font-bold">{month_name}</span>
                                                    " " <span class="opacity-70">{year_str}</span>
                                                </td>
                                                <td class="text-right">{format!("{:.0} kWh", c.heating_kwh)}</td>
                                                <td class="text-right">{format!("{:.0} kWh", c.hot_water_kwh)}</td>
                                                <td class="text-right font-bold">{format!("{:.0} kWh", total)}</td>
                                                <td>
                                                    <span class="badge badge-sm badge-ghost">
                                                        {format_source(&c.source)}
                                                    </span>
                                                </td>
                                                <td>
                                                    <button
                                                        class="btn btn-ghost btn-xs text-error"
                                                        on:click=move |_| on_delete((year, month))
                                                    >
                                                        "×"
                                                    </button>
                                                </td>
                                            </tr>
                                        }
                                    }
                                />
                            </tbody>
                        </table>
                    </div>
                </Show>
            </div>
        </div>
    }
}

/// Monthly chart card showing cost comparison
#[component]
fn MonthlyChartCard(monthly: Vec<MonthlyScenario>) -> impl IntoView {
    if monthly.is_empty() {
        return view! { <div></div> }.into_any();
    }

    // Find max cost for scaling
    let max_cost = monthly.iter()
        .map(|m| m.cost_current_eur.max(m.cost_hp_eur))
        .fold(0.0f64, |a, b| a.max(b));

    // Calculate total savings
    let total_savings: f64 = monthly.iter().map(|m| m.savings_eur).sum();

    view! {
        <div class="card bg-base-200">
            <div class="card-body">
                <div class="flex justify-between items-center">
                    <h2 class="card-title">"Monatlicher Kostenvergleich"</h2>
                    <div class="badge badge-success badge-lg">
                        {format!("Ersparnis: {:.0} EUR/Jahr", total_savings)}
                    </div>
                </div>

                <div class="grid grid-cols-12 gap-1 mt-4">
                    {monthly.iter().map(|m| {
                        let month_short = &m.month_name[0..3.min(m.month_name.len())];
                        let current_pct = if max_cost > 0.0 { m.cost_current_eur / max_cost * 100.0 } else { 0.0 };
                        let hp_pct = if max_cost > 0.0 { m.cost_hp_eur / max_cost * 100.0 } else { 0.0 };
                        let suitable_class = if m.hp_suitable { "bg-success" } else { "bg-warning" };

                        view! {
                            <div class="flex flex-col items-center">
                                <div class="h-24 w-full flex items-end justify-center gap-1">
                                    <div
                                        class="w-3 bg-warning rounded-t"
                                        style=format!("height: {}%", current_pct)
                                        title=format!("Gas: {:.0} EUR", m.cost_current_eur)
                                    ></div>
                                    <div
                                        class=format!("w-3 {} rounded-t", suitable_class)
                                        style=format!("height: {}%", hp_pct)
                                        title=format!("WP: {:.0} EUR (COP {:.1})", m.cost_hp_eur, m.estimated_cop)
                                    ></div>
                                </div>
                                <div class="text-xs mt-1 opacity-70">{month_short.to_string()}</div>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>

                <div class="flex justify-center gap-4 mt-4 text-sm">
                    <div class="flex items-center gap-2">
                        <div class="w-3 h-3 bg-warning rounded"></div>
                        <span>"Gas aktuell"</span>
                    </div>
                    <div class="flex items-center gap-2">
                        <div class="w-3 h-3 bg-success rounded"></div>
                        <span>"Wärmepumpe"</span>
                    </div>
                </div>
            </div>
        </div>
    }.into_any()
}

/// Building edit modal
#[component]
fn BuildingModal(
    show: RwSignal<bool>,
    overview: RwSignal<Option<EnergyOverview>>,
    on_save: impl Fn(()) + 'static + Copy,
) -> impl IntoView {
    let area = RwSignal::new(String::new());
    let units = RwSignal::new(String::new());
    let year_built = RwSignal::new(String::new());
    let heating_system = RwSignal::new(String::new());
    let distribution = RwSignal::new(String::new());
    let notes = RwSignal::new(String::new());
    let saving = RwSignal::new(false);

    // Initialize from overview when opened
    Effect::new(move |_| {
        if show.get() {
            if let Some(o) = overview.get() {
                area.set(o.building.area_m2.to_string());
                units.set(o.building.units.to_string());
                year_built.set(o.building.year_built.to_string());
                heating_system.set(o.building.heating_system.clone());
                distribution.set(o.building.distribution.clone());
                notes.set(o.building.notes.unwrap_or_default());
            }
        }
    });

    let save = move |_| {
        saving.set(true);
        let request = UpdateBuildingRequest {
            area_m2: area.get().parse().unwrap_or(0.0),
            units: units.get().parse().unwrap_or(0),
            year_built: year_built.get().parse().unwrap_or(0),
            heating_system: heating_system.get(),
            distribution: distribution.get(),
            notes: if notes.get().is_empty() { None } else { Some(notes.get()) },
        };

        spawn_local(async move {
            let _ = update_energy_building(request).await;
            saving.set(false);
            show.set(false);
            on_save(());
        });
    };

    view! {
        <dialog class="modal" class:modal-open=move || show.get()>
            <div class="modal-box">
                <h3 class="font-bold text-lg">"Gebäudeprofil bearbeiten"</h3>

                <div class="py-4 space-y-4">
                    <div class="grid grid-cols-2 gap-4">
                        <div class="form-control">
                            <label class="label"><span class="label-text">"Fläche (m²)"</span></label>
                            <input
                                type="number"
                                class="input input-bordered"
                                prop:value=move || area.get()
                                on:input=move |ev| area.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="form-control">
                            <label class="label"><span class="label-text">"Einheiten"</span></label>
                            <input
                                type="number"
                                class="input input-bordered"
                                prop:value=move || units.get()
                                on:input=move |ev| units.set(event_target_value(&ev))
                            />
                        </div>
                    </div>

                    <div class="form-control">
                        <label class="label"><span class="label-text">"Baujahr"</span></label>
                        <input
                            type="number"
                            class="input input-bordered"
                            prop:value=move || year_built.get()
                            on:input=move |ev| year_built.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="form-control">
                        <label class="label"><span class="label-text">"Heizungssystem"</span></label>
                        <select
                            class="select select-bordered"
                            on:change=move |ev| heating_system.set(event_target_value(&ev))
                        >
                            <option value="gas" selected=move || heating_system.get() == "gas">"Gas"</option>
                            <option value="oil" selected=move || heating_system.get() == "oil">"Öl"</option>
                            <option value="district_heating" selected=move || heating_system.get() == "district_heating">"Fernwärme"</option>
                            <option value="heat_pump" selected=move || heating_system.get() == "heat_pump">"Wärmepumpe"</option>
                            <option value="hybrid" selected=move || heating_system.get() == "hybrid">"Hybrid"</option>
                        </select>
                    </div>

                    <div class="form-control">
                        <label class="label"><span class="label-text">"Wärmeverteilung"</span></label>
                        <select
                            class="select select-bordered"
                            on:change=move |ev| distribution.set(event_target_value(&ev))
                        >
                            <option value="radiators" selected=move || distribution.get() == "radiators">"Heizkörper"</option>
                            <option value="floor_heating" selected=move || distribution.get() == "floor_heating">"Fußbodenheizung"</option>
                            <option value="mixed" selected=move || distribution.get() == "mixed">"Gemischt"</option>
                        </select>
                    </div>

                    <div class="form-control">
                        <label class="label"><span class="label-text">"Notizen"</span></label>
                        <textarea
                            class="textarea textarea-bordered"
                            prop:value=move || notes.get()
                            on:input=move |ev| notes.set(event_target_value(&ev))
                        ></textarea>
                    </div>
                </div>

                <div class="modal-action">
                    <button class="btn" on:click=move |_| show.set(false)>"Abbrechen"</button>
                    <button class="btn btn-primary" on:click=save disabled=move || saving.get()>
                        {move || if saving.get() { "Speichere..." } else { "Speichern" }}
                    </button>
                </div>
            </div>
            <form method="dialog" class="modal-backdrop">
                <button on:click=move |_| show.set(false)>"close"</button>
            </form>
        </dialog>
    }
}

/// Prices edit modal
#[component]
fn PricesModal(
    show: RwSignal<bool>,
    overview: RwSignal<Option<EnergyOverview>>,
    on_save: impl Fn(()) + 'static + Copy,
) -> impl IntoView {
    let gas = RwSignal::new(String::new());
    let electricity = RwSignal::new(String::new());
    let electricity_hp = RwSignal::new(String::new());
    let saving = RwSignal::new(false);

    // Initialize from overview when opened
    Effect::new(move |_| {
        if show.get() {
            if let Some(o) = overview.get() {
                gas.set(o.prices.gas_ct_kwh.to_string());
                electricity.set(o.prices.electricity_ct_kwh.to_string());
                electricity_hp.set(o.prices.electricity_hp_ct_kwh.to_string());
            }
        }
    });

    let save = move |_| {
        saving.set(true);
        let request = UpdatePricesRequest {
            gas_ct_kwh: gas.get().parse().unwrap_or(12.0),
            electricity_ct_kwh: electricity.get().parse().unwrap_or(35.0),
            electricity_hp_ct_kwh: electricity_hp.get().parse().unwrap_or(28.0),
        };

        spawn_local(async move {
            let _ = update_energy_prices(request).await;
            saving.set(false);
            show.set(false);
            on_save(());
        });
    };

    view! {
        <dialog class="modal" class:modal-open=move || show.get()>
            <div class="modal-box">
                <h3 class="font-bold text-lg">"Energiepreise bearbeiten"</h3>

                <div class="py-4 space-y-4">
                    <div class="form-control">
                        <label class="label">
                            <span class="label-text">"Gaspreis (ct/kWh)"</span>
                        </label>
                        <input
                            type="number"
                            step="0.1"
                            class="input input-bordered"
                            prop:value=move || gas.get()
                            on:input=move |ev| gas.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="form-control">
                        <label class="label">
                            <span class="label-text">"Strompreis (ct/kWh)"</span>
                        </label>
                        <input
                            type="number"
                            step="0.1"
                            class="input input-bordered"
                            prop:value=move || electricity.get()
                            on:input=move |ev| electricity.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="form-control">
                        <label class="label">
                            <span class="label-text">"WP-Stromtarif (ct/kWh)"</span>
                            <span class="label-text-alt">"Wärmepumpen-Tarif"</span>
                        </label>
                        <input
                            type="number"
                            step="0.1"
                            class="input input-bordered"
                            prop:value=move || electricity_hp.get()
                            on:input=move |ev| electricity_hp.set(event_target_value(&ev))
                        />
                    </div>
                </div>

                <div class="modal-action">
                    <button class="btn" on:click=move |_| show.set(false)>"Abbrechen"</button>
                    <button class="btn btn-primary" on:click=save disabled=move || saving.get()>
                        {move || if saving.get() { "Speichere..." } else { "Speichern" }}
                    </button>
                </div>
            </div>
            <form method="dialog" class="modal-backdrop">
                <button on:click=move |_| show.set(false)>"close"</button>
            </form>
        </dialog>
    }
}

/// Add consumption modal
#[component]
fn ConsumptionModal(
    show: RwSignal<bool>,
    on_save: impl Fn(()) + 'static + Copy,
) -> impl IntoView {
    let year = RwSignal::new(2024u32);
    let month = RwSignal::new(1u32);
    let heating = RwSignal::new(String::new());
    let hot_water = RwSignal::new(String::new());
    let source = RwSignal::new("manual".to_string());
    let saving = RwSignal::new(false);

    let save = move |_| {
        saving.set(true);
        let request = AddConsumptionRequest {
            year: year.get(),
            month: month.get(),
            heating_kwh: heating.get().parse().unwrap_or(0.0),
            hot_water_kwh: hot_water.get().parse().unwrap_or(0.0),
            source: Some(source.get()),
            notes: None,
        };

        spawn_local(async move {
            let _ = add_energy_consumption(request).await;
            saving.set(false);
            show.set(false);
            heating.set(String::new());
            hot_water.set(String::new());
            on_save(());
        });
    };

    view! {
        <dialog class="modal" class:modal-open=move || show.get()>
            <div class="modal-box">
                <h3 class="font-bold text-lg">"Verbrauchsdaten hinzufügen"</h3>

                <div class="py-4 space-y-4">
                    <div class="grid grid-cols-2 gap-4">
                        <div class="form-control">
                            <label class="label"><span class="label-text">"Jahr"</span></label>
                            <input
                                type="number"
                                class="input input-bordered"
                                prop:value=move || year.get().to_string()
                                on:input=move |ev| {
                                    let v: u32 = event_target_value(&ev).parse().unwrap_or(2024);
                                    year.set(v);
                                }
                            />
                        </div>
                        <div class="form-control">
                            <label class="label"><span class="label-text">"Monat"</span></label>
                            <select
                                class="select select-bordered"
                                on:change=move |ev| {
                                    let v: u32 = event_target_value(&ev).parse().unwrap_or(1);
                                    month.set(v);
                                }
                            >
                                {MONTH_NAMES.iter().enumerate().map(|(i, name)| {
                                    let m = (i + 1) as u32;
                                    view! {
                                        <option value={m.to_string()} selected=move || month.get() == m>
                                            {*name}
                                        </option>
                                    }
                                }).collect::<Vec<_>>()}
                            </select>
                        </div>
                    </div>

                    <div class="form-control">
                        <label class="label"><span class="label-text">"Heizung (kWh)"</span></label>
                        <input
                            type="number"
                            step="0.1"
                            class="input input-bordered"
                            prop:value=move || heating.get()
                            on:input=move |ev| heating.set(event_target_value(&ev))
                            placeholder="z.B. 450"
                        />
                    </div>

                    <div class="form-control">
                        <label class="label"><span class="label-text">"Warmwasser (kWh)"</span></label>
                        <input
                            type="number"
                            step="0.1"
                            class="input input-bordered"
                            prop:value=move || hot_water.get()
                            on:input=move |ev| hot_water.set(event_target_value(&ev))
                            placeholder="z.B. 120"
                        />
                    </div>

                    <div class="form-control">
                        <label class="label"><span class="label-text">"Datenquelle"</span></label>
                        <select
                            class="select select-bordered"
                            on:change=move |ev| source.set(event_target_value(&ev))
                        >
                            <option value="manual" selected>"Manuell"</option>
                            <option value="invoice">"Rechnung"</option>
                            <option value="smart_meter">"Smart Meter"</option>
                            <option value="estimate">"Schätzung"</option>
                        </select>
                    </div>
                </div>

                <div class="modal-action">
                    <button class="btn" on:click=move |_| show.set(false)>"Abbrechen"</button>
                    <button class="btn btn-primary" on:click=save disabled=move || saving.get()>
                        {move || if saving.get() { "Speichere..." } else { "Speichern" }}
                    </button>
                </div>
            </div>
            <form method="dialog" class="modal-backdrop">
                <button on:click=move |_| show.set(false)>"close"</button>
            </form>
        </dialog>
    }
}

/// Export modal
#[component]
fn ExportModal(
    show: RwSignal<bool>,
    overview: RwSignal<Option<EnergyOverview>>,
    scenarios: RwSignal<Option<ScenarioComparison>>,
    consumption: RwSignal<Vec<MonthlyConsumption>>,
) -> impl IntoView {
    let export_text = Signal::derive(move || {
        let mut lines = Vec::new();
        lines.push("=== Energie-Analyse Export ===".to_string());
        lines.push(format!("Erstellt: {}", chrono::Local::now().format("%Y-%m-%d %H:%M")));
        lines.push(String::new());

        if let Some(o) = overview.get() {
            lines.push("--- Gebäudeprofil ---".to_string());
            lines.push(format!("Fläche: {} m²", o.building.area_m2));
            lines.push(format!("Einheiten: {}", o.building.units));
            lines.push(format!("Baujahr: {}", o.building.year_built));
            lines.push(format!("Heizung: {}", format_heating_system(&o.building.heating_system)));
            lines.push(format!("Verteilung: {}", format_distribution(&o.building.distribution)));
            lines.push(String::new());

            lines.push("--- Energiepreise ---".to_string());
            lines.push(format!("Gas: {:.2} ct/kWh", o.prices.gas_ct_kwh));
            lines.push(format!("Strom: {:.2} ct/kWh", o.prices.electricity_ct_kwh));
            lines.push(format!("WP-Tarif: {:.2} ct/kWh", o.prices.electricity_hp_ct_kwh));
            lines.push(String::new());
        }

        if let Some(s) = scenarios.get() {
            lines.push(format!("--- Szenarienvergleich {} ---", s.year));
            lines.push(String::new());

            for (name, scenario) in [("Gas aktuell", &s.current), ("Hybrid WP", &s.hybrid), ("Voll WP", &s.full_hp)] {
                lines.push(format!("{}:", name));
                lines.push(format!("  Jahreskosten: {:.0} EUR", scenario.annual_cost_eur));
                lines.push(format!("  CO2-Emission: {:.0} kg", scenario.annual_co2_kg));
                lines.push(format!("  Durchschn. COP: {:.1}", scenario.avg_cop));
                lines.push(format!("  WP-Anteil: {:.0}%", scenario.hp_share * 100.0));
                lines.push(String::new());
            }

            let savings = (s.current.annual_cost_eur - s.hybrid.annual_cost_eur).max(0.0);
            let savings_full = (s.current.annual_cost_eur - s.full_hp.annual_cost_eur).max(0.0);
            lines.push(format!("Einsparung Hybrid: {:.0} EUR/Jahr ({:.1}%)",
                savings, savings / s.current.annual_cost_eur * 100.0));
            lines.push(format!("Einsparung Voll WP: {:.0} EUR/Jahr ({:.1}%)",
                savings_full, savings_full / s.current.annual_cost_eur * 100.0));
            lines.push(String::new());
        }

        let data = consumption.get();
        if !data.is_empty() {
            lines.push("--- Monatliche Verbrauchsdaten ---".to_string());
            lines.push("Jahr;Monat;Heizung kWh;Warmwasser kWh;Gesamt kWh".to_string());
            for c in data {
                let month_name = MONTH_NAMES.get((c.month - 1) as usize).unwrap_or(&"?");
                lines.push(format!("{};{};{:.1};{:.1};{:.1}",
                    c.year, month_name, c.heating_kwh, c.hot_water_kwh,
                    c.heating_kwh + c.hot_water_kwh));
            }
        }

        lines.join("\n")
    });

    view! {
        <dialog class="modal" class:modal-open=move || show.get()>
            <div class="modal-box max-w-3xl">
                <h3 class="font-bold text-lg">"Export für Heizungsfachmann"</h3>

                <div class="py-4">
                    <textarea
                        class="textarea textarea-bordered w-full h-96 font-mono text-xs"
                        readonly
                        prop:value=move || export_text.get()
                    ></textarea>
                </div>

                <p class="text-sm opacity-70">"Tipp: Text auswählen und mit Cmd+C kopieren"</p>

                <div class="modal-action">
                    <button class="btn" on:click=move |_| show.set(false)>"Schließen"</button>
                </div>
            </div>
            <form method="dialog" class="modal-backdrop">
                <button on:click=move |_| show.set(false)>"close"</button>
            </form>
        </dialog>
    }
}

/// Apartments editing modal
#[component]
fn ApartmentsModal(
    show: RwSignal<bool>,
    overview: RwSignal<Option<EnergyOverview>>,
    on_save: impl Fn(()) + 'static + Copy,
) -> impl IntoView {
    // Local state for editing
    let apartments = RwSignal::new(Vec::<ApartmentUnit>::new());
    let base_cost_percent = RwSignal::new(30.0f64);
    let saving = RwSignal::new(false);
    let next_id = RwSignal::new(1u32);

    // Initialize from overview when modal opens
    Effect::new(move |_| {
        if show.get() {
            if let Some(o) = overview.get() {
                apartments.set(o.apartments.clone());
                base_cost_percent.set(o.cost_distribution.base_cost_percent);
                let max_id = o.apartments.iter().map(|a| a.id).max().unwrap_or(0);
                next_id.set(max_id + 1);
            }
        }
    });

    let add_apartment = move |_| {
        let id = next_id.get();
        next_id.set(id + 1);
        apartments.update(|apts| {
            apts.push(ApartmentUnit {
                id,
                name: format!("WE {}", id),
                area_m2: 0.0,
                distribution_units: 0.0,
                is_reference: apts.is_empty(), // First one is reference
                notes: None,
            });
        });
    };

    let remove_apartment = move |id: u32| {
        apartments.update(|apts| {
            apts.retain(|a| a.id != id);
        });
    };

    let update_apartment = move |id: u32, field: &str, value: String| {
        apartments.update(|apts| {
            if let Some(apt) = apts.iter_mut().find(|a| a.id == id) {
                match field {
                    "name" => apt.name = value,
                    "area" => apt.area_m2 = value.parse().unwrap_or(0.0),
                    "units" => apt.distribution_units = value.parse().unwrap_or(0.0),
                    _ => {}
                }
            }
        });
    };

    let set_reference = move |id: u32| {
        apartments.update(|apts| {
            for apt in apts.iter_mut() {
                apt.is_reference = apt.id == id;
            }
        });
    };

    let save = move |_| {
        saving.set(true);
        let apts = apartments.get();
        let base_pct = base_cost_percent.get();

        spawn_local(async move {
            let request = UpdateApartmentsRequest {
                apartments: apts.into_iter().map(|a| crate::api::ApartmentUnit {
                    id: a.id,
                    name: a.name,
                    area_m2: a.area_m2,
                    distribution_units: a.distribution_units,
                    is_reference: a.is_reference,
                    notes: a.notes,
                }).collect(),
                cost_distribution: CostDistributionSettings {
                    base_cost_percent: base_pct,
                },
            };
            let _ = update_energy_apartments(request).await;
            saving.set(false);
            show.set(false);
            on_save(());
        });
    };

    // Calculate totals for preview
    let total_area = Signal::derive(move || {
        apartments.get().iter().map(|a| a.area_m2).sum::<f64>()
    });

    let total_units = Signal::derive(move || {
        apartments.get().iter().map(|a| a.distribution_units).sum::<f64>()
    });

    view! {
        <dialog class="modal" class:modal-open=move || show.get()>
            <div class="modal-box max-w-4xl">
                <h3 class="font-bold text-lg mb-4">"Wohneinheiten & Kostenverteilung"</h3>

                // Cost distribution settings
                <div class="bg-base-200 p-4 rounded-lg mb-4">
                    <h4 class="font-semibold mb-2">"Kostenverteilung (Heizkostenverordnung)"</h4>
                    <div class="flex items-center gap-4">
                        <label class="label">
                            <span class="label-text">"Grundkostenanteil:"</span>
                        </label>
                        <input
                            type="range"
                            min="30" max="50" step="5"
                            class="range range-primary w-32"
                            prop:value=move || base_cost_percent.get()
                            on:input=move |ev| {
                                let val: f64 = event_target_value(&ev).parse().unwrap_or(30.0);
                                base_cost_percent.set(val);
                            }
                        />
                        <span class="font-bold text-primary">{move || format!("{:.0}%", base_cost_percent.get())}</span>
                        <span class="opacity-70">"(nach m²)"</span>
                        <span class="mx-2">"|"</span>
                        <span class="font-bold text-secondary">{move || format!("{:.0}%", 100.0 - base_cost_percent.get())}</span>
                        <span class="opacity-70">"(nach Verteiler)"</span>
                    </div>
                </div>

                // Apartments table
                <div class="overflow-x-auto">
                    <table class="table table-sm">
                        <thead>
                            <tr>
                                <th class="w-8">"Ref"</th>
                                <th>"Bezeichnung"</th>
                                <th class="text-right">"Fläche (m²)"</th>
                                <th class="text-right">"Verteiler-Einh."</th>
                                <th class="text-right">"Flächen-%"</th>
                                <th class="text-right">"Verbr.-%"</th>
                                <th class="w-12"></th>
                            </tr>
                        </thead>
                        <tbody>
                            <For
                                each=move || apartments.get()
                                key=|apt| apt.id
                                children=move |apt| {
                                    let id = apt.id;
                                    let name = apt.name.clone();
                                    let area = apt.area_m2;
                                    let units = apt.distribution_units;
                                    let is_ref = apt.is_reference;

                                    view! {
                                        <tr class=move || if is_ref { "bg-primary/10" } else { "" }>
                                            <td>
                                                <input
                                                    type="radio"
                                                    name="reference"
                                                    class="radio radio-xs radio-primary"
                                                    checked=is_ref
                                                    on:change=move |_| set_reference(id)
                                                />
                                            </td>
                                            <td>
                                                <input
                                                    type="text"
                                                    class="input input-sm input-bordered w-full max-w-xs"
                                                    value=name.clone()
                                                    on:input=move |ev| update_apartment(id, "name", event_target_value(&ev))
                                                />
                                            </td>
                                            <td class="text-right">
                                                <input
                                                    type="number"
                                                    step="0.1"
                                                    class="input input-sm input-bordered w-20 text-right"
                                                    value=area
                                                    on:input=move |ev| update_apartment(id, "area", event_target_value(&ev))
                                                />
                                            </td>
                                            <td class="text-right">
                                                <input
                                                    type="number"
                                                    step="0.1"
                                                    class="input input-sm input-bordered w-20 text-right"
                                                    value=units
                                                    on:input=move |ev| update_apartment(id, "units", event_target_value(&ev))
                                                />
                                            </td>
                                            <td class="text-right opacity-70">
                                                {move || {
                                                    let total = total_area.get();
                                                    if total > 0.0 {
                                                        format!("{:.1}%", area / total * 100.0)
                                                    } else {
                                                        "-".to_string()
                                                    }
                                                }}
                                            </td>
                                            <td class="text-right opacity-70">
                                                {move || {
                                                    let total = total_units.get();
                                                    if total > 0.0 {
                                                        format!("{:.1}%", units / total * 100.0)
                                                    } else {
                                                        "-".to_string()
                                                    }
                                                }}
                                            </td>
                                            <td>
                                                <button
                                                    class="btn btn-ghost btn-xs text-error"
                                                    on:click=move |_| remove_apartment(id)
                                                >
                                                    "×"
                                                </button>
                                            </td>
                                        </tr>
                                    }
                                }
                            />
                        </tbody>
                        <tfoot>
                            <tr class="font-bold">
                                <td colspan="2">"Gesamt"</td>
                                <td class="text-right">{move || format!("{:.1} m²", total_area.get())}</td>
                                <td class="text-right">{move || format!("{:.1}", total_units.get())}</td>
                                <td class="text-right">"100%"</td>
                                <td class="text-right">"100%"</td>
                                <td></td>
                            </tr>
                        </tfoot>
                    </table>
                </div>

                <div class="mt-4">
                    <button class="btn btn-outline btn-sm" on:click=add_apartment>
                        "+ Wohnung hinzufügen"
                    </button>
                </div>

                <div class="alert alert-info mt-4">
                    <span>"Die Referenz-Wohnung (Ref) ist die Einheit, für die monatliche Verbrauchsdaten erfasst werden. Die Kosten anderer Einheiten werden anteilig berechnet."</span>
                </div>

                <div class="modal-action">
                    <button class="btn btn-ghost" on:click=move |_| show.set(false)>"Abbrechen"</button>
                    <button
                        class="btn btn-primary"
                        disabled=move || saving.get()
                        on:click=save
                    >
                        {move || if saving.get() { "Speichern..." } else { "Speichern" }}
                    </button>
                </div>
            </div>
            <form method="dialog" class="modal-backdrop">
                <button on:click=move |_| show.set(false)>"close"</button>
            </form>
        </dialog>
    }
}

// Helper functions

fn format_heating_system(system: &str) -> &'static str {
    match system {
        "gas" => "Gas",
        "oil" => "Öl",
        "district_heating" => "Fernwärme",
        "heat_pump" => "Wärmepumpe",
        "hybrid" => "Hybrid",
        _ => "Sonstige",
    }
}

fn format_distribution(dist: &str) -> &'static str {
    match dist {
        "radiators" => "Heizkörper",
        "floor_heating" => "Fußbodenheizung",
        "mixed" => "Gemischt",
        _ => "Sonstige",
    }
}

fn format_source(source: &str) -> &'static str {
    match source {
        "invoice" => "Rechnung",
        "smart_meter" => "Smart Meter",
        "estimate" => "Schätzung",
        _ => "Manuell",
    }
}
