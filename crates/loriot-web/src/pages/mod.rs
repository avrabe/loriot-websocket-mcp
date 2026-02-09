//! Page components

mod analytics;
mod dashboard;
mod device_detail;
mod energy;
mod forecast;
mod heatpump;
mod history;
mod sensor_health;
mod thermal;
mod weather;

pub use analytics::AnalyticsPage;
pub use dashboard::DashboardPage;
pub use device_detail::DeviceDetailPage;
pub use energy::EnergyPage;
pub use forecast::ForecastPage;
pub use heatpump::HeatPumpPage;
pub use history::HistoryPage;
pub use sensor_health::SensorHealthPage;
pub use thermal::ThermalPage;
pub use weather::WeatherPage;
