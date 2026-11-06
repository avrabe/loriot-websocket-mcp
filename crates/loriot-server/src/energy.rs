//! Energy tracking API handlers

use axum::{extract::State, http::StatusCode, Json};
use loriot_core::energy::{
    analyze_scenario, monthly_breakdown, ApartmentUnit, BuildingProfile, CostDistributionSettings,
    DataSource, EnergyData, EnergyPrices, HeatingScenario, MonthlyConsumption, MonthlyScenario,
    ScenarioAnalysis,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

const ENERGY_DATA_PATH: &str = "energy_data.json";

/// Energy state wrapper
pub struct EnergyState {
    pub data: EnergyData,
}

impl EnergyState {
    pub fn new() -> Self {
        let data = EnergyData::load(ENERGY_DATA_PATH).unwrap_or_default();
        if data.consumption.is_empty() {
            tracing::info!("No energy data found, starting fresh");
        } else {
            tracing::info!(
                "Loaded energy data: {} months, {} m², {} units",
                data.consumption.len(),
                data.building.area_m2,
                data.building.units
            );
        }
        Self { data }
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        self.data.save(ENERGY_DATA_PATH)
    }
}

impl Default for EnergyState {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedEnergyState = Arc<RwLock<EnergyState>>;

// ============================================================================
// API Response Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct EnergyOverview {
    pub building: BuildingProfile,
    pub prices: EnergyPrices,
    pub apartments: Vec<ApartmentUnit>,
    pub cost_distribution: CostDistributionSettings,
    pub consumption_count: usize,
    pub available_years: Vec<u32>,
    pub latest_year: Option<u32>,
    pub yearly_totals: Vec<YearlyTotal>,
}

#[derive(Debug, Serialize)]
pub struct YearlyTotal {
    pub year: u32,
    pub heating_kwh: f64,
    pub hot_water_kwh: f64,
    pub total_kwh: f64,
    pub months_with_data: usize,
}

#[derive(Debug, Serialize)]
pub struct ScenarioComparison {
    pub year: u32,
    pub current: ScenarioAnalysis,
    pub hybrid: ScenarioAnalysis,
    pub full_hp: ScenarioAnalysis,
    pub monthly: Vec<MonthlyScenario>,
}

#[derive(Debug, Serialize)]
pub struct ConsumptionResponse {
    pub consumption: Vec<MonthlyConsumption>,
}

// ============================================================================
// API Request Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct UpdateBuildingRequest {
    pub area_m2: f64,
    pub units: u32,
    pub year_built: u32,
    pub heating_system: String,
    pub distribution: String,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePricesRequest {
    pub gas_ct_kwh: f64,
    pub electricity_ct_kwh: f64,
    pub electricity_hp_ct_kwh: f64,
}

#[derive(Debug, Deserialize)]
pub struct AddConsumptionRequest {
    pub year: u32,
    pub month: u32,
    pub heating_kwh: f64,
    pub hot_water_kwh: f64,
    pub source: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteConsumptionRequest {
    pub year: u32,
    pub month: u32,
}

#[derive(Debug, Deserialize)]
pub struct ScenarioQuery {
    pub year: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApartmentsRequest {
    pub apartments: Vec<ApartmentUnitRequest>,
    pub cost_distribution: CostDistributionSettingsRequest,
}

#[derive(Debug, Deserialize)]
pub struct ApartmentUnitRequest {
    pub id: u32,
    pub name: String,
    pub area_m2: f64,
    pub distribution_units: f64,
    pub is_reference: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CostDistributionSettingsRequest {
    pub base_cost_percent: f64,
}

// ============================================================================
// API Handlers
// ============================================================================

/// GET /api/energy/overview - Get energy overview
pub async fn get_overview(State(state): State<SharedEnergyState>) -> Json<EnergyOverview> {
    let energy = state.read().await;
    let data = &energy.data;

    let available_years = data.available_years();
    let latest_year = available_years.last().copied();

    let yearly_totals: Vec<YearlyTotal> = available_years
        .iter()
        .map(|&year| {
            let months: Vec<_> = data.consumption.iter().filter(|c| c.year == year).collect();

            YearlyTotal {
                year,
                heating_kwh: months.iter().map(|c| c.heating_kwh).sum(),
                hot_water_kwh: months.iter().map(|c| c.hot_water_kwh).sum(),
                total_kwh: months.iter().map(|c| c.total_kwh()).sum(),
                months_with_data: months.len(),
            }
        })
        .collect();

    Json(EnergyOverview {
        building: data.building.clone(),
        prices: data.prices.clone(),
        apartments: data.apartments.clone(),
        cost_distribution: data.cost_distribution.clone(),
        consumption_count: data.consumption.len(),
        available_years,
        latest_year,
        yearly_totals,
    })
}

/// GET /api/energy/consumption - Get all consumption data
pub async fn get_consumption(State(state): State<SharedEnergyState>) -> Json<ConsumptionResponse> {
    let energy = state.read().await;
    Json(ConsumptionResponse {
        consumption: energy.data.consumption.clone(),
    })
}

/// POST /api/energy/building - Update building profile
pub async fn update_building(
    State(state): State<SharedEnergyState>,
    Json(request): Json<UpdateBuildingRequest>,
) -> Result<Json<BuildingProfile>, StatusCode> {
    let mut energy = state.write().await;

    let heating_system = match request.heating_system.as_str() {
        "gas" => loriot_core::energy::HeatingSystem::Gas,
        "oil" => loriot_core::energy::HeatingSystem::Oil,
        "district_heating" => loriot_core::energy::HeatingSystem::DistrictHeating,
        "heat_pump" => loriot_core::energy::HeatingSystem::HeatPump,
        "hybrid" => loriot_core::energy::HeatingSystem::Hybrid,
        _ => loriot_core::energy::HeatingSystem::Other,
    };

    let distribution = match request.distribution.as_str() {
        "radiators" => loriot_core::energy::HeatDistribution::Radiators,
        "floor_heating" => loriot_core::energy::HeatDistribution::FloorHeating,
        "mixed" => loriot_core::energy::HeatDistribution::Mixed,
        _ => loriot_core::energy::HeatDistribution::Other,
    };

    energy.data.building = BuildingProfile {
        area_m2: request.area_m2,
        units: request.units,
        year_built: request.year_built,
        heating_system,
        distribution,
        notes: request.notes,
    };

    if let Err(e) = energy.save() {
        tracing::error!("Failed to save energy data: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!(
        "Updated building profile: {} m², {} units, built {}",
        request.area_m2,
        request.units,
        request.year_built
    );

    Ok(Json(energy.data.building.clone()))
}

/// POST /api/energy/prices - Update energy prices
pub async fn update_prices(
    State(state): State<SharedEnergyState>,
    Json(request): Json<UpdatePricesRequest>,
) -> Result<Json<EnergyPrices>, StatusCode> {
    let mut energy = state.write().await;

    energy.data.prices.gas_ct_kwh = request.gas_ct_kwh;
    energy.data.prices.electricity_ct_kwh = request.electricity_ct_kwh;
    energy.data.prices.electricity_hp_ct_kwh = request.electricity_hp_ct_kwh;

    if let Err(e) = energy.save() {
        tracing::error!("Failed to save energy data: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!(
        "Updated prices: Gas {}ct, Strom {}ct, WP-Strom {}ct",
        request.gas_ct_kwh,
        request.electricity_ct_kwh,
        request.electricity_hp_ct_kwh
    );

    Ok(Json(energy.data.prices.clone()))
}

/// POST /api/energy/apartments - Update apartments and cost distribution
pub async fn update_apartments(
    State(state): State<SharedEnergyState>,
    Json(request): Json<UpdateApartmentsRequest>,
) -> Result<Json<Vec<ApartmentUnit>>, StatusCode> {
    let mut energy = state.write().await;

    // Convert request to ApartmentUnit
    energy.data.apartments = request
        .apartments
        .into_iter()
        .map(|a| ApartmentUnit {
            id: a.id,
            name: a.name,
            area_m2: a.area_m2,
            distribution_units: a.distribution_units,
            is_reference: a.is_reference,
            notes: a.notes,
        })
        .collect();

    energy.data.cost_distribution = CostDistributionSettings {
        base_cost_percent: request.cost_distribution.base_cost_percent,
    };

    if let Err(e) = energy.save() {
        tracing::error!("Failed to save energy data: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!(
        "Updated apartments: {} units, {}% Grundkosten",
        energy.data.apartments.len(),
        request.cost_distribution.base_cost_percent
    );

    Ok(Json(energy.data.apartments.clone()))
}

/// POST /api/energy/consumption - Add or update consumption record
pub async fn add_consumption(
    State(state): State<SharedEnergyState>,
    Json(request): Json<AddConsumptionRequest>,
) -> Result<Json<MonthlyConsumption>, StatusCode> {
    let mut energy = state.write().await;

    let source = match request.source.as_deref() {
        Some("invoice") => DataSource::Invoice,
        Some("smart_meter") => DataSource::SmartMeter,
        Some("estimate") => DataSource::Estimate,
        _ => DataSource::Manual,
    };

    let record = MonthlyConsumption {
        year: request.year,
        month: request.month,
        heating_kwh: request.heating_kwh,
        hot_water_kwh: request.hot_water_kwh,
        source,
        notes: request.notes,
    };

    energy.data.set_month(record.clone());

    if let Err(e) = energy.save() {
        tracing::error!("Failed to save energy data: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!(
        "Added consumption: {}/{} - {} kWh heating, {} kWh hot water",
        request.year,
        request.month,
        request.heating_kwh,
        request.hot_water_kwh
    );

    Ok(Json(record))
}

/// DELETE /api/energy/consumption - Delete consumption record
pub async fn delete_consumption(
    State(state): State<SharedEnergyState>,
    Json(request): Json<DeleteConsumptionRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut energy = state.write().await;

    energy.data.remove_month(request.year, request.month);

    if let Err(e) = energy.save() {
        tracing::error!("Failed to save energy data: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!("Deleted consumption: {}/{}", request.year, request.month);

    Ok(StatusCode::OK)
}

/// GET /api/energy/scenarios - Get scenario comparison
pub async fn get_scenarios(
    State(state): State<SharedEnergyState>,
    axum::extract::Query(query): axum::extract::Query<ScenarioQuery>,
) -> Json<ScenarioComparison> {
    let energy = state.read().await;
    let data = &energy.data;

    // Use specified year or latest year with data
    let year = query
        .year
        .unwrap_or_else(|| data.available_years().last().copied().unwrap_or(2024));

    // TODO: Get source temps from sensor data
    let source_temps: Option<&[(u32, f32)]> = None;

    let current = analyze_scenario(data, HeatingScenario::Current, year, source_temps);
    let hybrid = analyze_scenario(data, HeatingScenario::Hybrid, year, source_temps);
    let full_hp = analyze_scenario(data, HeatingScenario::FullHeatPump, year, source_temps);
    let monthly = monthly_breakdown(data, year, source_temps);

    Json(ScenarioComparison {
        year,
        current,
        hybrid,
        full_hp,
        monthly,
    })
}

/// Create energy router
pub fn energy_router() -> axum::Router<SharedEnergyState> {
    use axum::routing::{delete, get, post};

    axum::Router::new()
        .route("/overview", get(get_overview))
        .route("/consumption", get(get_consumption))
        .route("/consumption", post(add_consumption))
        .route("/consumption", delete(delete_consumption))
        .route("/building", post(update_building))
        .route("/prices", post(update_prices))
        .route("/apartments", post(update_apartments))
        .route("/scenarios", get(get_scenarios))
}
