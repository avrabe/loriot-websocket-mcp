//! LoRaWAN Sensor Monitoring Web Server
//!
//! This server provides:
//! - WebSocket connection to Loriot for sensor data ingestion
//! - SSE endpoints for real-time updates to browser clients
//! - REST API for historical data and analytics
//! - Static file serving for the Leptos frontend

mod api;
mod energy;
mod forecast;
mod heatpump;
mod sse;
mod state;
mod thermal;
mod weather;
mod websocket;

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use loriot_core::{load_device_names, AppConfig, StorageConfig, StorageManager};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install default rustls crypto provider (required for TLS connections)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "loriot_server=debug,tower_http=debug".into()),
        )
        .init();

    // Load configuration from environment
    let loriot_token = std::env::var("LORIOT_WS_TOKEN")
        .expect("LORIOT_WS_TOKEN environment variable must be set");

    let config = AppConfig {
        loriot_token,
        web_port: std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000),
        ..Default::default()
    };

    // Initialize storage
    let storage_config = StorageConfig {
        file_path: config
            .storage_file
            .clone()
            .unwrap_or_else(|| "sensor_history.jsonl".to_string()),
        ..Default::default()
    };

    let mut storage = StorageManager::new(storage_config);
    storage.load_existing_data()?;

    tracing::info!(
        "Loaded {} readings from storage",
        storage.reading_count()
    );

    // Load device names
    let device_names = config
        .device_names_file
        .as_ref()
        .map(|f| load_device_names(f))
        .unwrap_or_default();

    // Create shared application state
    let state = AppState::new(storage, device_names, config.clone());

    // Create weather state
    let weather_state: weather::SharedWeatherState = Arc::new(RwLock::new(weather::WeatherState::new()));

    // Set default location (Bochum area) if not configured
    {
        let mut ws = weather_state.write().await;
        ws.set_location(loriot_core::weather::Coordinates::new(51.4818, 7.2162));
    }

    // Create heat pump state
    let heatpump_state: heatpump::SharedHeatPumpState = Arc::new(RwLock::new(heatpump::HeatPumpState::new()));

    // Create energy state
    let energy_state: energy::SharedEnergyState = Arc::new(RwLock::new(energy::EnergyState::new()));

    // Build weather API router (separate state)
    let weather_router = Router::new()
        .route("/info", get(weather::get_weather_info))
        .route("/current", get(weather::get_current_weather))
        .route("/history", get(weather::get_weather_history))
        .route("/statistics", get(weather::get_weather_statistics))
        .route("/geocode", get(weather::geocode_address))
        .route("/stations", get(weather::get_stations))
        .route("/location", post(weather::set_location))
        .route("/fetch", post(weather::fetch_weather_data))
        .with_state(weather_state.clone());

    // Build heat pump API router (needs app + weather state)
    let heatpump_combined_state = heatpump::CombinedHeatPumpState {
        app: state.clone(),
        weather: weather_state.clone(),
        heatpump: heatpump_state,
    };
    let heatpump_router = Router::new()
        .route("/info", get(heatpump::get_heatpump_info))
        .route("/configure", post(heatpump::configure_heatpump))
        .route("/analyze", get(heatpump::analyze_heatpump))
        .route("/devices", get(heatpump::get_available_devices))
        .with_state(heatpump_combined_state);

    // Build thermal analysis API router (needs app + weather state)
    let thermal_state = thermal::ThermalState {
        app: state.clone(),
        weather: weather_state.clone(),
    };
    let thermal_router = Router::new()
        .route("/analyze", get(thermal::analyze_thermal))
        .route("/summary", get(thermal::get_thermal_summary))
        .with_state(thermal_state);

    // Build forecast API router (needs app + weather state)
    let forecast_state = forecast::ForecastState {
        app: state.clone(),
        weather: weather_state,
    };
    let forecast_router = Router::new()
        .route("/", get(forecast::get_weather_forecast))
        .route("/rooms", get(forecast::get_room_predictions))
        .route("/models", get(forecast::get_thermal_models))
        .with_state(forecast_state);

    // Build energy API router
    let energy_router = energy::energy_router().with_state(energy_state);

    // Build main API router
    let app = Router::new()
        // API routes
        .route("/api/info", get(api::get_system_info))
        .route("/api/data-range", get(api::get_data_range))
        .route("/api/readings", get(api::get_readings))
        .route("/api/readings/{eui}", get(api::get_device_readings))
        .route("/api/devices", get(api::get_devices))
        .route("/api/devices/{eui}", get(api::get_device_stats))
        .route("/api/analytics", get(api::get_analytics))
        .route("/api/analytics/{eui}", get(api::get_device_analytics))
        // SSE routes
        .route("/sse/readings", get(sse::readings_stream))
        .route("/sse/status", get(sse::status_stream))
        .with_state(state.clone())
        // Nest weather routes
        .nest("/api/weather", weather_router)
        // Nest heat pump routes
        .nest("/api/heatpump", heatpump_router)
        // Nest thermal analysis routes
        .nest("/api/thermal", thermal_router)
        // Nest forecast routes
        .nest("/api/forecast", forecast_router)
        // Nest energy routes
        .nest("/api/energy", energy_router)
        // Health check
        .route("/health", get(|| async { "OK" }))
        // Static files with SPA fallback
        .fallback_service(
            ServeDir::new("assets")
                .append_index_html_on_directories(true)
                .not_found_service(tower::service_fn(|_| async {
                    // Serve index.html for SPA routes
                    match tokio::fs::read_to_string("assets/index.html").await {
                        Ok(html) => Ok((StatusCode::OK, Html(html)).into_response()),
                        Err(_) => Ok((StatusCode::NOT_FOUND, "Not found").into_response()),
                    }
                })),
        )
        // Add middleware
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    // Spawn the Loriot WebSocket handler
    let ws_state = state.clone();
    tokio::spawn(async move {
        websocket::run_loriot_connection(ws_state).await;
    });

    // Start the server
    let addr = format!("0.0.0.0:{}", config.web_port);
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
