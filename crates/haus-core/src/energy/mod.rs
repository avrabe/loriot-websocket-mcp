//! Energy tracking and analysis module
//!
//! This module provides data structures and calculations for tracking
//! building energy consumption and comparing different heating scenarios.

use serde::{Deserialize, Serialize};

/// Building profile with basic information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildingProfile {
    /// Total living area in m²
    pub area_m2: f64,
    /// Number of residential units
    pub units: u32,
    /// Year of construction
    pub year_built: u32,
    /// Type of heating system
    pub heating_system: HeatingSystem,
    /// Type of heat distribution
    pub distribution: HeatDistribution,
    /// Optional notes
    pub notes: Option<String>,
}

/// Type of heating system
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HeatingSystem {
    #[default]
    Gas,
    Oil,
    DistrictHeating,
    HeatPump,
    Hybrid,
    Other,
}

impl HeatingSystem {
    pub fn label(&self) -> &'static str {
        match self {
            HeatingSystem::Gas => "Gas",
            HeatingSystem::Oil => "Oil",
            HeatingSystem::DistrictHeating => "Fernwärme",
            HeatingSystem::HeatPump => "Wärmepumpe",
            HeatingSystem::Hybrid => "Hybrid",
            HeatingSystem::Other => "Sonstige",
        }
    }
}

/// Type of heat distribution system
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HeatDistribution {
    #[default]
    Radiators,
    FloorHeating,
    Mixed,
    Other,
}

impl HeatDistribution {
    pub fn label(&self) -> &'static str {
        match self {
            HeatDistribution::Radiators => "Radiatoren",
            HeatDistribution::FloorHeating => "Fußbodenheizung",
            HeatDistribution::Mixed => "Gemischt",
            HeatDistribution::Other => "Sonstige",
        }
    }

    /// Typical flow temperature for this distribution type
    pub fn typical_flow_temp(&self) -> f32 {
        match self {
            HeatDistribution::FloorHeating => 35.0,
            HeatDistribution::Mixed => 45.0,
            HeatDistribution::Radiators => 55.0,
            HeatDistribution::Other => 50.0,
        }
    }
}

/// Energy prices configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyPrices {
    /// Gas price in ct/kWh
    pub gas_ct_kwh: f64,
    /// Standard electricity price in ct/kWh
    pub electricity_ct_kwh: f64,
    /// Heat pump electricity tariff in ct/kWh (often lower)
    pub electricity_hp_ct_kwh: f64,
    /// CO2 emission factor for gas in kg/kWh
    pub co2_gas_kg_kwh: f64,
    /// CO2 emission factor for electricity in kg/kWh
    pub co2_electricity_kg_kwh: f64,
}

/// Monthly energy consumption record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyConsumption {
    /// Year
    pub year: u32,
    /// Month (1-12)
    pub month: u32,
    /// Heating consumption in kWh
    pub heating_kwh: f64,
    /// Hot water consumption in kWh
    pub hot_water_kwh: f64,
    /// Data source
    pub source: DataSource,
    /// Optional notes
    pub notes: Option<String>,
}

impl MonthlyConsumption {
    /// Total consumption
    pub fn total_kwh(&self) -> f64 {
        self.heating_kwh + self.hot_water_kwh
    }

    /// Month name in German
    pub fn month_name(&self) -> &'static str {
        match self.month {
            1 => "Januar",
            2 => "Februar",
            3 => "März",
            4 => "April",
            5 => "Mai",
            6 => "Juni",
            7 => "Juli",
            8 => "August",
            9 => "September",
            10 => "Oktober",
            11 => "November",
            12 => "Dezember",
            _ => "?",
        }
    }

    /// Short month name
    pub fn month_short(&self) -> &'static str {
        match self.month {
            1 => "Jan",
            2 => "Feb",
            3 => "Mär",
            4 => "Apr",
            5 => "Mai",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Okt",
            11 => "Nov",
            12 => "Dez",
            _ => "?",
        }
    }
}

/// Source of consumption data
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    #[default]
    Manual,
    Invoice,
    SmartMeter,
    Estimate,
}

impl DataSource {
    pub fn label(&self) -> &'static str {
        match self {
            DataSource::Manual => "Manuell",
            DataSource::Invoice => "Abrechnung",
            DataSource::SmartMeter => "Smart Meter",
            DataSource::Estimate => "Schätzung",
        }
    }
}

/// Apartment/Unit in the building
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApartmentUnit {
    /// Unique ID
    pub id: u32,
    /// Name/description (e.g., "EG links", "1.OG rechts", "WE 3")
    pub name: String,
    /// Living area in m²
    pub area_m2: f64,
    /// Distribution units (Heizkostenverteiler-Einheiten)
    pub distribution_units: f64,
    /// Is this the reference unit with consumption data?
    pub is_reference: bool,
    /// Optional notes
    pub notes: Option<String>,
}

/// Cost distribution settings (Heizkostenverordnung)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostDistributionSettings {
    /// Base cost share (Grundkostenanteil) as percentage (typically 30-50%)
    /// Distributed by area (m²)
    pub base_cost_percent: f64,
    // Consumption share = 100 - base_cost_percent
    // Distributed by distribution units (Verteilereinheiten)
}

impl Default for CostDistributionSettings {
    fn default() -> Self {
        Self {
            base_cost_percent: 30.0, // Standard: 30% Grundkosten, 70% Verbrauch
        }
    }
}

/// Cost breakdown per apartment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApartmentCostBreakdown {
    pub apartment_id: u32,
    pub name: String,
    pub area_m2: f64,
    pub area_share_percent: f64,
    pub distribution_units: f64,
    pub consumption_share_percent: f64,
    pub base_cost_eur: f64,
    pub consumption_cost_eur: f64,
    pub total_cost_eur: f64,
    pub cost_per_m2: f64,
}

/// Complete energy data for a building
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnergyData {
    /// Building profile
    pub building: BuildingProfile,
    /// Energy prices
    pub prices: EnergyPrices,
    /// Apartment units
    #[serde(default)]
    pub apartments: Vec<ApartmentUnit>,
    /// Cost distribution settings
    #[serde(default)]
    pub cost_distribution: CostDistributionSettings,
    /// Monthly consumption records
    pub consumption: Vec<MonthlyConsumption>,
    /// Last updated timestamp
    pub updated_at: u64,
}

impl Default for EnergyPrices {
    fn default() -> Self {
        Self {
            gas_ct_kwh: 12.0,
            electricity_ct_kwh: 30.0,
            electricity_hp_ct_kwh: 25.0,
            co2_gas_kg_kwh: 0.201,
            co2_electricity_kg_kwh: 0.4,
        }
    }
}

impl EnergyData {
    /// Load from file
    pub fn load(path: &str) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save to file
    pub fn save(&self, path: &str) -> Result<(), std::io::Error> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)
    }

    /// Get total yearly consumption
    pub fn yearly_total(&self, year: u32) -> f64 {
        self.consumption
            .iter()
            .filter(|c| c.year == year)
            .map(|c| c.total_kwh())
            .sum()
    }

    /// Get yearly heating consumption
    pub fn yearly_heating(&self, year: u32) -> f64 {
        self.consumption
            .iter()
            .filter(|c| c.year == year)
            .map(|c| c.heating_kwh)
            .sum()
    }

    /// Get yearly hot water consumption
    pub fn yearly_hot_water(&self, year: u32) -> f64 {
        self.consumption
            .iter()
            .filter(|c| c.year == year)
            .map(|c| c.hot_water_kwh)
            .sum()
    }

    /// Get consumption for a specific month
    pub fn get_month(&self, year: u32, month: u32) -> Option<&MonthlyConsumption> {
        self.consumption
            .iter()
            .find(|c| c.year == year && c.month == month)
    }

    /// Add or update monthly consumption
    pub fn set_month(&mut self, record: MonthlyConsumption) {
        if let Some(existing) = self
            .consumption
            .iter_mut()
            .find(|c| c.year == record.year && c.month == record.month)
        {
            *existing = record;
        } else {
            self.consumption.push(record);
        }
        // Sort by date
        self.consumption.sort_by_key(|c| (c.year, c.month));
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
    }

    /// Remove monthly consumption
    pub fn remove_month(&mut self, year: u32, month: u32) {
        self.consumption
            .retain(|c| !(c.year == year && c.month == month));
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
    }

    /// Get available years
    pub fn available_years(&self) -> Vec<u32> {
        let mut years: Vec<u32> = self.consumption.iter().map(|c| c.year).collect();
        years.sort();
        years.dedup();
        years
    }
}

/// Heating scenario for comparison
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeatingScenario {
    /// Current state (gas/oil)
    Current,
    /// Hybrid: heat pump for transition seasons, gas for winter peaks
    Hybrid,
    /// Full heat pump replacement
    FullHeatPump,
}

/// Scenario analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioAnalysis {
    /// Annual energy cost in EUR
    pub annual_cost_eur: f64,
    /// Annual CO2 emissions in kg
    pub annual_co2_kg: f64,
    /// Electricity consumption in kWh (for heat pump scenarios)
    pub electricity_kwh: f64,
    /// Gas consumption in kWh (remaining)
    pub gas_kwh: f64,
    /// Average COP (for heat pump scenarios)
    pub avg_cop: f64,
    /// Heat pump share (0.0 - 1.0)
    pub hp_share: f64,
}

/// Monthly scenario breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyScenario {
    pub month: u32,
    pub month_name: String,
    pub heating_kwh: f64,
    pub hot_water_kwh: f64,
    pub hp_suitable: bool,
    pub estimated_cop: f64,
    pub hp_electricity_kwh: f64,
    pub gas_kwh: f64,
    pub cost_current_eur: f64,
    pub cost_hp_eur: f64,
    pub savings_eur: f64,
}

/// Calculate scenario analysis
pub fn analyze_scenario(
    data: &EnergyData,
    scenario: HeatingScenario,
    year: u32,
    source_temps: Option<&[(u32, f32)]>, // month -> avg source temp
) -> ScenarioAnalysis {
    let prices = &data.prices;
    let boiler_efficiency = 0.9; // Gas boiler efficiency

    // Get yearly consumption
    let total_consumption: f64 = data
        .consumption
        .iter()
        .filter(|c| c.year == year)
        .map(|c| c.total_kwh())
        .sum();

    if total_consumption == 0.0 {
        return ScenarioAnalysis {
            annual_cost_eur: 0.0,
            annual_co2_kg: 0.0,
            electricity_kwh: 0.0,
            gas_kwh: 0.0,
            avg_cop: 0.0,
            hp_share: 0.0,
        };
    }

    // Heat delivered (accounting for boiler efficiency)
    let heat_delivered = total_consumption * boiler_efficiency;

    match scenario {
        HeatingScenario::Current => {
            let gas_kwh = total_consumption;
            let cost = gas_kwh * prices.gas_ct_kwh / 100.0;
            let co2 = gas_kwh * prices.co2_gas_kg_kwh;

            ScenarioAnalysis {
                annual_cost_eur: cost,
                annual_co2_kg: co2,
                electricity_kwh: 0.0,
                gas_kwh,
                avg_cop: 1.0 / boiler_efficiency,
                hp_share: 0.0,
            }
        }
        HeatingScenario::Hybrid => {
            // Heat pump for Apr-Oct (low heating demand), gas for Nov-Mar
            let mut hp_heat = 0.0;
            let mut gas_heat = 0.0;
            let mut weighted_cop_sum = 0.0;

            for record in data.consumption.iter().filter(|c| c.year == year) {
                let heat = record.total_kwh() * boiler_efficiency;
                let is_transition = record.month >= 4 && record.month <= 10;

                // Get source temp for COP estimation
                let source_temp = source_temps
                    .and_then(|temps| temps.iter().find(|(m, _)| *m == record.month))
                    .map(|(_, t)| *t)
                    .unwrap_or(12.0); // Default cellar temp

                let cop = estimate_cop(source_temp, data.building.distribution.typical_flow_temp());

                if is_transition || record.heating_kwh < 500.0 {
                    // Use heat pump
                    hp_heat += heat;
                    weighted_cop_sum += heat * cop as f64;
                } else {
                    // Use gas for high-demand months
                    // But still use HP for part of it
                    let hp_portion = 0.4; // HP handles 40% even in winter
                    hp_heat += heat * hp_portion;
                    gas_heat += heat * (1.0 - hp_portion);
                    weighted_cop_sum += heat * hp_portion * cop as f64;
                }
            }

            let avg_cop = if hp_heat > 0.0 {
                weighted_cop_sum / hp_heat
            } else {
                4.0
            };
            let electricity_kwh = hp_heat / avg_cop;
            let gas_kwh = gas_heat / boiler_efficiency;

            let cost = electricity_kwh * prices.electricity_hp_ct_kwh / 100.0
                + gas_kwh * prices.gas_ct_kwh / 100.0;
            let co2 =
                electricity_kwh * prices.co2_electricity_kg_kwh + gas_kwh * prices.co2_gas_kg_kwh;

            ScenarioAnalysis {
                annual_cost_eur: cost,
                annual_co2_kg: co2,
                electricity_kwh,
                gas_kwh,
                avg_cop,
                hp_share: hp_heat / heat_delivered,
            }
        }
        HeatingScenario::FullHeatPump => {
            // Full heat pump, avg COP based on distribution type
            let base_cop = match data.building.distribution {
                HeatDistribution::FloorHeating => 4.8,
                HeatDistribution::Mixed => 4.2,
                HeatDistribution::Radiators => 3.5,
                HeatDistribution::Other => 4.0,
            };

            let electricity_kwh = heat_delivered / base_cop;
            let cost = electricity_kwh * prices.electricity_hp_ct_kwh / 100.0;
            let co2 = electricity_kwh * prices.co2_electricity_kg_kwh;

            ScenarioAnalysis {
                annual_cost_eur: cost,
                annual_co2_kg: co2,
                electricity_kwh,
                gas_kwh: 0.0,
                avg_cop: base_cop,
                hp_share: 1.0,
            }
        }
    }
}

/// Estimate COP based on source and flow temperature
pub fn estimate_cop(source_temp: f32, flow_temp: f32) -> f32 {
    // Simplified COP estimation based on temperature lift
    // Real COP depends on many factors, this is a reasonable approximation
    let temp_lift = flow_temp - source_temp;

    if temp_lift <= 0.0 {
        return 6.0; // Maximum reasonable COP
    }

    // Carnot COP = T_hot / (T_hot - T_cold) in Kelvin
    // Real COP is roughly 40-50% of Carnot
    let t_hot_k = flow_temp + 273.15;
    let carnot_cop = t_hot_k / temp_lift;
    let real_cop = carnot_cop * 0.45;

    // Clamp to reasonable range
    real_cop.clamp(2.5, 6.0)
}

/// Generate monthly breakdown for scenario comparison
pub fn monthly_breakdown(
    data: &EnergyData,
    year: u32,
    source_temps: Option<&[(u32, f32)]>,
) -> Vec<MonthlyScenario> {
    let prices = &data.prices;
    let boiler_efficiency = 0.9;
    let flow_temp = data.building.distribution.typical_flow_temp();

    data.consumption
        .iter()
        .filter(|c| c.year == year)
        .map(|record| {
            let source_temp = source_temps
                .and_then(|temps| temps.iter().find(|(m, _)| *m == record.month))
                .map(|(_, t)| *t)
                .unwrap_or(12.0);

            let cop = estimate_cop(source_temp, flow_temp);
            let heat = record.total_kwh() * boiler_efficiency;
            let hp_electricity = heat / cop as f64;

            // HP suitable if heating demand is low or it's transition season
            let hp_suitable =
                record.heating_kwh < 800.0 || (record.month >= 4 && record.month <= 10);

            let cost_current = record.total_kwh() * prices.gas_ct_kwh / 100.0;
            let cost_hp = hp_electricity * prices.electricity_hp_ct_kwh / 100.0;

            MonthlyScenario {
                month: record.month,
                month_name: record.month_name().to_string(),
                heating_kwh: record.heating_kwh,
                hot_water_kwh: record.hot_water_kwh,
                hp_suitable,
                estimated_cop: cop as f64,
                hp_electricity_kwh: hp_electricity,
                gas_kwh: if hp_suitable { 0.0 } else { record.total_kwh() },
                cost_current_eur: cost_current,
                cost_hp_eur: cost_hp,
                savings_eur: cost_current - cost_hp,
            }
        })
        .collect()
}

/// Calculate cost distribution across apartments
///
/// Uses the German Heizkostenverordnung model:
/// - base_cost_percent (e.g. 30%) distributed by area (m²)
/// - consumption_cost (e.g. 70%) distributed by distribution units
pub fn calculate_cost_distribution(
    apartments: &[ApartmentUnit],
    settings: &CostDistributionSettings,
    total_cost_eur: f64,
) -> Vec<ApartmentCostBreakdown> {
    if apartments.is_empty() {
        return vec![];
    }

    let total_area: f64 = apartments.iter().map(|a| a.area_m2).sum();
    let total_units: f64 = apartments.iter().map(|a| a.distribution_units).sum();

    let base_cost = total_cost_eur * settings.base_cost_percent / 100.0;
    let consumption_cost = total_cost_eur * (100.0 - settings.base_cost_percent) / 100.0;

    apartments
        .iter()
        .map(|apt| {
            let area_share = if total_area > 0.0 {
                apt.area_m2 / total_area * 100.0
            } else {
                0.0
            };
            let consumption_share = if total_units > 0.0 {
                apt.distribution_units / total_units * 100.0
            } else {
                0.0
            };

            let apt_base_cost = base_cost * area_share / 100.0;
            let apt_consumption_cost = consumption_cost * consumption_share / 100.0;
            let apt_total = apt_base_cost + apt_consumption_cost;

            ApartmentCostBreakdown {
                apartment_id: apt.id,
                name: apt.name.clone(),
                area_m2: apt.area_m2,
                area_share_percent: area_share,
                distribution_units: apt.distribution_units,
                consumption_share_percent: consumption_share,
                base_cost_eur: apt_base_cost,
                consumption_cost_eur: apt_consumption_cost,
                total_cost_eur: apt_total,
                cost_per_m2: if apt.area_m2 > 0.0 {
                    apt_total / apt.area_m2
                } else {
                    0.0
                },
            }
        })
        .collect()
}

/// Scale reference apartment consumption to building total based on area ratio
pub fn scale_to_building(
    reference_consumption_kwh: f64,
    reference_area_m2: f64,
    building_area_m2: f64,
) -> f64 {
    if reference_area_m2 > 0.0 {
        reference_consumption_kwh * building_area_m2 / reference_area_m2
    } else {
        reference_consumption_kwh
    }
}
