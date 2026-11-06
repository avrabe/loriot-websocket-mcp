//! Main application component with routing

use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

use crate::pages::{
    AnalyticsPage, DashboardPage, DeviceDetailPage, EnergyPage, ForecastPage, HeatPumpPage,
    HistoryPage, SensorHealthPage, ThermalPage, WeatherPage,
};

/// Main application component
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Html attr:lang="en" />
        <Meta charset="UTF-8" />
        <Meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <Title text="LoRaWAN Sensor Monitor" />

        <Router>
            <main class="min-h-screen bg-base-200">
                <Routes fallback=|| "Page not found">
                    <Route path=path!("/") view=DashboardPage />
                    <Route path=path!("/device/:eui") view=DeviceDetailPage />
                    <Route path=path!("/analytics") view=AnalyticsPage />
                    <Route path=path!("/health") view=SensorHealthPage />
                    <Route path=path!("/history") view=HistoryPage />
                    <Route path=path!("/weather") view=WeatherPage />
                    <Route path=path!("/heatpump") view=HeatPumpPage />
                    <Route path=path!("/thermal") view=ThermalPage />
                    <Route path=path!("/forecast") view=ForecastPage />
                    <Route path=path!("/energy") view=EnergyPage />
                </Routes>
            </main>
        </Router>
    }
}
