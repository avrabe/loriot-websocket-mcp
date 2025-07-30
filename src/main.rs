use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::{SinkExt, StreamExt};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    symbols,
    widgets::{
        Axis, Block, Borders, Chart, Dataset, GraphType, Row, Table, TableState,
        Cell, Paragraph, Wrap, Tabs, List, ListItem, ListState,
    },
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    error::Error,
    io::{self, BufRead, Write},
    path::Path,
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use fxhash::{FxHashMap, FxHashSet};

#[derive(Debug, Deserialize, Clone)]
struct LoriotPayload {
    cmd: String,
    #[serde(rename = "EUI")]
    eui: String,
    data: String,
    ts: u64,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct StoredReading {
    // Loriot WebSocket format fields (raw data)
    cmd: String,
    eui: String,
    data: String,
    ts: u64,
    name: Option<String>,
    
    // Parsed fields (for faster loading)
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_humidity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_battery_voltage: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_battery_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_activity: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_movement_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_lux: Option<u16>,
}

#[derive(Clone, Debug)]
struct StorageConfig {
    file_path: String,
    memory_limit_readings: usize,
    backup_count: usize,
    sync_interval_minutes: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig {
            file_path: "sensor_history.jsonl".to_string(),
            memory_limit_readings: 2000,
            backup_count: 3,
            sync_interval_minutes: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TimeWindow {
    All,
    LastHour,
    Last6Hours,
    LastDay,
    LastWeek,
}

impl TimeWindow {
    fn display_name(&self) -> &str {
        match self {
            TimeWindow::All => "All Time",
            TimeWindow::LastHour => "Last Hour",
            TimeWindow::Last6Hours => "Last 6 Hours", 
            TimeWindow::LastDay => "Last Day",
            TimeWindow::LastWeek => "Last Week",
        }
    }
    
    fn get_cutoff_timestamp(&self) -> Option<u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            
        match self {
            TimeWindow::All => None,
            TimeWindow::LastHour => Some(now - (60 * 60 * 1000)), // 1 hour
            TimeWindow::Last6Hours => Some(now - (6 * 60 * 60 * 1000)), // 6 hours
            TimeWindow::LastDay => Some(now - (24 * 60 * 60 * 1000)), // 24 hours
            TimeWindow::LastWeek => Some(now - (7 * 24 * 60 * 60 * 1000)), // 7 days
        }
    }
}

#[derive(Clone, Debug)]
struct SensorReading {
    timestamp: u64,
    device_eui: String,
    device_name: String,
    temperature: f32,
    humidity: f32,
    battery_voltage: f32,
    battery_status: String,
    activity: bool,
    movement_count: u32,
    lux: Option<u16>,
}

struct StorageManager {
    config: StorageConfig,
    memory_cache: Vec<SensorReading>,
    duplicate_tracker: FxHashSet<(u64, String)>, // (timestamp, device_eui)
    device_latest_timestamps: FxHashMap<String, u64>,
    total_stored_count: usize,
    file_handle: Option<std::fs::File>,
}

impl StorageManager {
    fn new(config: StorageConfig) -> Self {
        StorageManager {
            config,
            memory_cache: Vec::new(),
            duplicate_tracker: FxHashSet::default(),
            device_latest_timestamps: FxHashMap::default(),
            total_stored_count: 0,
            file_handle: None,
        }
    }

    async fn initialize(&mut self, device_names: &HashMap<String, String>) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Load existing data from file
        self.load_existing_data(device_names).await?;
        
        // Open file for appending
        self.file_handle = Some(std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.file_path)?);
        
        Ok(())
    }

    async fn load_existing_data(&mut self, device_names: &HashMap<String, String>) -> Result<(), Box<dyn Error + Send + Sync>> {
        if !Path::new(&self.config.file_path).exists() {
            return Ok(());
        }

        let file = std::fs::File::open(&self.config.file_path)?;
        let reader = std::io::BufReader::new(file);
        
        let mut all_readings = Vec::new();
        
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            
            match serde_json::from_str::<StoredReading>(&line) {
                Ok(stored_reading) => {
                    if let Some(sensor_reading) = self.stored_to_sensor_reading(&stored_reading, device_names) {
                        all_readings.push(sensor_reading);
                        self.duplicate_tracker.insert((stored_reading.ts, stored_reading.eui.clone()));
                        self.device_latest_timestamps.insert(
                            stored_reading.eui.clone(),
                            stored_reading.ts.max(*self.device_latest_timestamps.get(&stored_reading.eui).unwrap_or(&0))
                        );
                        self.total_stored_count += 1;
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse line in storage file: {}", e);
                    continue;
                }
            }
        }

        // Sort by timestamp and keep most recent readings in memory
        all_readings.sort_by_key(|r| r.timestamp);
        
        // Keep only the most recent readings in memory cache
        if all_readings.len() > self.config.memory_limit_readings {
            let start_index = all_readings.len() - self.config.memory_limit_readings;
            self.memory_cache = all_readings.into_iter().skip(start_index).collect();
        } else {
            self.memory_cache = all_readings;
        }

        println!("Loaded {} readings from storage ({} in memory cache)", 
                self.total_stored_count, self.memory_cache.len());
        
        Ok(())
    }

    fn stored_to_sensor_reading(&self, stored: &StoredReading, device_names: &HashMap<String, String>) -> Option<SensorReading> {
        // Try to use parsed fields first, fall back to parsing raw data
        if let (Some(temp), Some(hum), Some(bat_v), Some(bat_s), Some(act), Some(mov)) = (
            stored.parsed_temperature,
            stored.parsed_humidity,
            stored.parsed_battery_voltage,
            stored.parsed_battery_status.as_ref(),
            stored.parsed_activity,
            stored.parsed_movement_count
        ) {
            Some(SensorReading {
                timestamp: stored.ts,
                device_eui: stored.eui.clone(),
                device_name: device_names.get(&stored.eui).cloned().unwrap_or_else(|| stored.eui.clone()),
                temperature: temp,
                humidity: hum,
                battery_voltage: bat_v,
                battery_status: bat_s.clone(),
                activity: act,
                movement_count: mov,
                lux: stored.parsed_lux,
            })
        } else {
            // Fall back to parsing raw data
            if let Some((bat_v, bat_s, temp, hum, activity, movement_count, lux, _)) = decode_lht65s_pir(&stored.data) {
                Some(SensorReading {
                    timestamp: stored.ts,
                    device_eui: stored.eui.clone(),
                    device_name: device_names.get(&stored.eui).cloned().unwrap_or_else(|| stored.eui.clone()),
                    temperature: temp,
                    humidity: hum,
                    battery_voltage: bat_v,
                    battery_status: bat_s,
                    activity,
                    movement_count,
                    lux,
                })
            } else {
                None
            }
        }
    }

    fn add_reading(&mut self, reading: SensorReading) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let key = (reading.timestamp, reading.device_eui.clone());
        
        // Check for duplicates
        if self.duplicate_tracker.contains(&key) {
            return Ok(false); // Duplicate, not added
        }

        // Create stored reading with parsed fields
        let stored_reading = StoredReading {
            cmd: "rx".to_string(), // Default command
            eui: reading.device_eui.clone(),
            data: "".to_string(), // We'll need the raw data if we want to store it
            ts: reading.timestamp,
            name: None,
            parsed_temperature: Some(reading.temperature),
            parsed_humidity: Some(reading.humidity),
            parsed_battery_voltage: Some(reading.battery_voltage),
            parsed_battery_status: Some(reading.battery_status.clone()),
            parsed_activity: Some(reading.activity),
            parsed_movement_count: Some(reading.movement_count),
            parsed_lux: reading.lux,
        };

        // Write to file
        if let Some(ref mut file) = self.file_handle {
            let line = serde_json::to_string(&stored_reading)?;
            writeln!(file, "{}", line)?;
            file.flush()?;
        }

        // Add to memory cache
        let insert_pos = self.memory_cache
            .binary_search_by_key(&reading.timestamp, |r| r.timestamp)
            .unwrap_or_else(|pos| pos);
        
        self.memory_cache.insert(insert_pos, reading);
        
        // Maintain memory limit
        if self.memory_cache.len() > self.config.memory_limit_readings {
            self.memory_cache.remove(0);
        }

        // Update tracking
        self.duplicate_tracker.insert(key);
        self.device_latest_timestamps.insert(stored_reading.eui.clone(), stored_reading.ts);
        self.total_stored_count += 1;

        Ok(true) // Successfully added
    }

    fn get_readings(&self) -> &Vec<SensorReading> {
        &self.memory_cache
    }

    fn get_total_count(&self) -> usize {
        self.total_stored_count
    }

    fn get_memory_count(&self) -> usize {
        self.memory_cache.len()
    }
}

struct App {
    storage_manager: StorageManager,
    table_state: TableState,
    device_list_state: ListState,
    should_quit: bool,
    device_names: HashMap<String, String>,
    selected_tab: usize,
    selected_device: Option<String>,
    show_all_devices: bool,
    time_window: TimeWindow,
}

impl Default for App {
    fn default() -> Self {
        App {
            storage_manager: StorageManager::new(StorageConfig::default()),
            table_state: TableState::default(),
            device_list_state: ListState::default(),
            should_quit: false,
            device_names: HashMap::new(),
            selected_tab: 0,
            selected_device: None,
            show_all_devices: true,
            time_window: TimeWindow::All,
        }
    }
}

impl App {
    fn get_unique_devices(&self) -> Vec<String> {
        let mut devices: Vec<String> = self.storage_manager.get_readings()
            .iter()
            .map(|r| r.device_name.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        devices.sort();
        devices
    }

    fn get_filtered_readings(&self) -> Vec<&SensorReading> {
        let time_cutoff = self.time_window.get_cutoff_timestamp();
        
        let time_filtered: Vec<&SensorReading> = if let Some(cutoff) = time_cutoff {
            self.storage_manager.get_readings().iter().filter(|r| r.timestamp >= cutoff).collect()
        } else {
            self.storage_manager.get_readings().iter().collect()
        };
        
        if self.show_all_devices || self.selected_device.is_none() {
            time_filtered
        } else {
            time_filtered
                .into_iter()
                .filter(|r| Some(&r.device_name) == self.selected_device.as_ref())
                .collect()
        }
    }

    fn get_pir_events(&self) -> Vec<&SensorReading> {
        self.storage_manager.get_readings()
            .iter()
            .filter(|r| r.activity)
            .collect()
    }

    fn get_next_packet_info(&self, device_eui: &str) -> Option<(u64, String)> {
        let device_readings: Vec<_> = self.storage_manager.get_readings()
            .iter()
            .filter(|r| r.device_eui == device_eui)
            .collect();
        
        if let Some(latest) = device_readings.last() {
            // LHT65S-PIR sends every 20 minutes (1200 seconds)
            let interval_ms = 20 * 60 * 1000; // 20 minutes in milliseconds
            let next_expected = latest.timestamp + interval_ms;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            
            if next_expected > now {
                let remaining_ms = next_expected - now;
                let remaining_minutes = remaining_ms / (60 * 1000);
                let remaining_seconds = (remaining_ms % (60 * 1000)) / 1000;
                let countdown = format!("{}:{:02}", remaining_minutes, remaining_seconds);
                Some((next_expected, countdown))
            } else {
                let overdue_ms = now - next_expected;
                let overdue_minutes = overdue_ms / (60 * 1000);
                let overdue_seconds = (overdue_ms % (60 * 1000)) / 1000;
                let overdue = format!("-{}:{:02}", overdue_minutes, overdue_seconds);
                Some((next_expected, overdue))
            }
        } else {
            None
        }
    }

    fn next_time_window(&mut self) {
        self.time_window = match self.time_window {
            TimeWindow::All => TimeWindow::LastHour,
            TimeWindow::LastHour => TimeWindow::Last6Hours,
            TimeWindow::Last6Hours => TimeWindow::LastDay,
            TimeWindow::LastDay => TimeWindow::LastWeek,
            TimeWindow::LastWeek => TimeWindow::All,
        };
    }

    fn previous_time_window(&mut self) {
        self.time_window = match self.time_window {
            TimeWindow::All => TimeWindow::LastWeek,
            TimeWindow::LastHour => TimeWindow::All,
            TimeWindow::Last6Hours => TimeWindow::LastHour,
            TimeWindow::LastDay => TimeWindow::Last6Hours,
            TimeWindow::LastWeek => TimeWindow::LastDay,
        };
    }
}

impl App {
    fn new() -> App {
        let device_names: HashMap<String, String> = if let Ok(config_data) = std::fs::read_to_string("devices.json") {
            serde_json::from_str(&config_data).unwrap_or_default()
        } else {
            HashMap::new()
        };

        let mut app = App {
            device_names,
            show_all_devices: true,
            ..Default::default()
        };
        app.table_state.select(Some(0));
        app.device_list_state.select(Some(0));
        app
    }

    async fn initialize_storage(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.storage_manager.initialize(&self.device_names).await
    }

    fn add_reading(&mut self, reading: SensorReading) {
        if let Err(e) = self.storage_manager.add_reading(reading) {
            eprintln!("Error storing reading: {}", e);
        }
    }

    fn get_device_name(&self, eui: &str) -> String {
        self.device_names.get(eui).cloned().unwrap_or_else(|| eui.to_string())
    }

    fn next(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.storage_manager.get_memory_count().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.storage_manager.get_memory_count().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % 3; // 3 tabs: Overview, PIR Events, Device Select
    }

    fn previous_tab(&mut self) {
        if self.selected_tab > 0 {
            self.selected_tab -= 1;
        } else {
            self.selected_tab = 2;
        }
    }

    fn next_device(&mut self) {
        let devices = self.get_unique_devices();
        if devices.is_empty() {
            return;
        }
        
        let i = match self.device_list_state.selected() {
            Some(i) => (i + 1) % (devices.len() + 1), // +1 for "All devices"
            None => 0,
        };
        self.device_list_state.select(Some(i));
        
        if i == 0 {
            self.show_all_devices = true;
            self.selected_device = None;
        } else {
            self.show_all_devices = false;
            self.selected_device = Some(devices[i - 1].clone());
        }
    }

    fn previous_device(&mut self) {
        let devices = self.get_unique_devices();
        if devices.is_empty() {
            return;
        }
        
        let i = match self.device_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    devices.len() // All devices is at index 0, so go to last device
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.device_list_state.select(Some(i));
        
        if i == 0 {
            self.show_all_devices = true;
            self.selected_device = None;
        } else {
            self.show_all_devices = false;
            self.selected_device = Some(devices[i - 1].clone());
        }
    }
}

fn decode_lht65s_pir(data: &str) -> Option<(f32, String, f32, f32, bool, u32, Option<u16>, u8)> {
    let bytes = hex::decode(data).ok()?;
    if bytes.len() < 11 {
        return None;
    }

    // Battery status and voltage (bytes 0-1)
    let bat_full = ((bytes[0] as u16) << 8) | bytes[1] as u16;
    let bat_status_code = (bat_full >> 14) & 0x03;
    let bat_status = match bat_status_code {
        0 => "Ultra Low",
        1 => "Low",
        2 => "OK",
        3 => "Good",
        _ => "Unknown"
    };
    let bat_voltage = (bat_full & 0x3FFF) as f32 / 1000.0;

    // Temperature (bytes 2-3) - can be signed for negative temps
    let temp_raw = ((bytes[2] as u16) << 8 | bytes[3] as u16) as i16;
    let temp = temp_raw as f32 / 100.0;

    // Humidity (bytes 4-5)
    let hum_raw = (bytes[4] as u16) << 8 | bytes[5] as u16;
    let hum = hum_raw as f32 / 10.0;

    // External sensor type (byte 6)
    let ext_sensor = bytes[6];
    
    // Activity detection (bit 0 of byte 7)
    let activity_triggered = (bytes[7] & 0x01) != 0;

    // Check if this is an illumination sensor (E5 model)
    let lux = if ext_sensor == 0x05 && bytes.len() >= 11 {
        Some(((bytes[9] as u16) << 8) | bytes[10] as u16)
    } else {
        None
    };

    // Movement count (bytes 8-10) - for PIR model (0x0E)
    let movement_count = if ext_sensor == 0x0E {
        ((bytes[8] as u32) << 16) | ((bytes[9] as u32) << 8) | (bytes[10] as u32)
    } else {
        0
    };

    Some((bat_voltage, bat_status.to_string(), temp, hum, activity_triggered, movement_count, lux, ext_sensor))
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(6),
        ])
        .split(f.area());

    // Title
    let title = Paragraph::new("LoRaWAN Sensor Monitor")
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Tabs
    let tab_titles = vec!["Overview", "PIR Events", "Device Select"];
    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title("Views"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .select(app.selected_tab);
    f.render_widget(tabs, chunks[1]);

    match app.selected_tab {
        0 => render_overview_tab(f, chunks[2], app),
        1 => render_pir_events_tab(f, chunks[2], app),
        2 => render_device_select_tab(f, chunks[2], app),
        _ => {}
    }

    // Help/Status
    let current_filter = if app.show_all_devices {
        "All devices".to_string()
    } else {
        app.selected_device.as_ref().unwrap_or(&"None".to_string()).clone()
    };
    
    let storage_info = format!("({}/{} readings)", 
        app.storage_manager.get_memory_count(), 
        app.storage_manager.get_total_count());
    
    let help_text = format!(
        "Device: {} | Time: {} | Storage: {} | q:quit | Tab:views | ↑↓:navigate | +/-:time | c:devices",
        current_filter,
        app.time_window.display_name(),
        storage_info
    );
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[3]);
}

fn render_overview_tab(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Chart with filtered data
    let filtered_readings = app.get_filtered_readings();
    if !filtered_readings.is_empty() {
        // Sort readings by timestamp and use normalized time for x-axis
        let mut sorted_readings = filtered_readings.clone();
        sorted_readings.sort_by_key(|r| r.timestamp);
        
        let start_time = if sorted_readings.is_empty() { 0 } else { sorted_readings.first().unwrap().timestamp };
        
        let temp_data: Vec<(f64, f64)> = sorted_readings
            .iter()
            .enumerate()
            .map(|(i, r)| (i as f64, r.temperature as f64))
            .collect();

        let hum_data: Vec<(f64, f64)> = sorted_readings
            .iter()
            .enumerate()
            .map(|(i, r)| (i as f64, r.humidity as f64))
            .collect();

        let datasets = vec![
            Dataset::default()
                .name("🌡️  Temperature (°C)")
                .marker(symbols::Marker::Dot)
                .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                .graph_type(GraphType::Line)
                .data(&temp_data),
            Dataset::default()
                .name("💧 Humidity (%)")
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .graph_type(GraphType::Line)
                .data(&hum_data),
        ];

        let chart_title = if app.show_all_devices {
            "📊 All Sensors - Temperature & Humidity Trends".to_string()
        } else {
            format!("📊 {} - Temperature & Humidity Trends", 
                app.selected_device.as_ref().unwrap_or(&"Unknown".to_string()))
        };

        // Calculate better axis bounds
        let temp_min = if temp_data.is_empty() { 0.0 } else { 
            temp_data.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min).floor() - 2.0 
        };
        let temp_max = if temp_data.is_empty() { 30.0 } else { 
            temp_data.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max).ceil() + 2.0 
        };
        let hum_max = if hum_data.is_empty() { 100.0 } else { 
            hum_data.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max).ceil().max(100.0) 
        };
        
        let y_min = temp_min.min(0.0);
        let y_max = temp_max.max(hum_max);

        // Create time labels for x-axis using the sorted readings
        let time_labels: Vec<String> = if sorted_readings.len() > 1 {
            let start_time = sorted_readings.first().unwrap().timestamp;
            let end_time = sorted_readings.last().unwrap().timestamp;
            
            (0..=5).map(|i| {
                let timestamp = start_time + (i * (end_time - start_time)) / 5;
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp as i64)
                    .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
                    .unwrap_or_else(|| format!("T{}", i))
            }).collect()
        } else {
            vec![]
        };

        let chart = Chart::new(datasets)
            .block(Block::default().title(chart_title).borders(Borders::ALL))
            .x_axis(
                Axis::default()
                    .title("Time (Hours:Minutes)")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, sorted_readings.len() as f64])
                    .labels(time_labels.iter().map(|s| s.as_str()).collect::<Vec<_>>())
            )
            .y_axis(
                Axis::default()
                    .title("Temperature (°C) / Humidity (%)")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([y_min, y_max])
                    .labels(vec![
                        format!("{:.0}", y_min),
                        format!("{:.0}", y_min + (y_max - y_min) * 0.25),
                        format!("{:.0}", y_min + (y_max - y_min) * 0.50),
                        format!("{:.0}", y_min + (y_max - y_min) * 0.75),
                        format!("{:.0}", y_max),
                    ])
            );
        f.render_widget(chart, chunks[0]);
    }

    // Recent readings table with timestamps
    let header_cells = ["Time", "Device", "Temp", "Hum", "Battery", "Motion", "Next"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Red)));
    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::Blue))
        .height(1)
        .bottom_margin(1);

    let rows: Vec<Row> = filtered_readings
        .iter()
        .rev()
        .take(15)
        .map(|reading| {
            let device = if reading.device_name.len() > 12 {
                format!("{}...", &reading.device_name[..9])
            } else {
                reading.device_name.clone()
            };

            let motion = if reading.activity { "🚨" } else { "-" };
            
            let time = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(reading.timestamp as i64)
                .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "??:??:??".to_string());

            let next_packet = app.get_next_packet_info(&reading.device_eui)
                .map(|(_, countdown)| countdown)
                .unwrap_or_else(|| "-".to_string());

            let next_packet_style = if next_packet.starts_with('-') {
                Style::default().fg(Color::Red) // Overdue
            } else {
                Style::default().fg(Color::Green) // On time
            };

            Row::new(vec![
                Cell::from(time),
                Cell::from(device),
                Cell::from(format!("{:.1}°C", reading.temperature)),
                Cell::from(format!("{:.1}%", reading.humidity)),
                Cell::from(format!("{:.2}V", reading.battery_voltage)),
                Cell::from(motion.to_string()),
                Cell::from(next_packet).style(next_packet_style),
            ])
        })
        .collect();

    let table = Table::new(rows, [
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(7),
    ])
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Recent Readings"))
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_symbol(">> ");

    f.render_stateful_widget(table, chunks[1], &mut app.table_state.clone());
}

fn render_pir_events_tab(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);
        
    let chart_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[0]);

    // Show all PIR sensor readings (not just active motion events)
    let pir_readings: Vec<_> = app.storage_manager.get_readings()
        .iter()
        .filter(|r| r.movement_count > 0 || app.storage_manager.get_readings().iter().any(|x| x.device_eui == r.device_eui && x.movement_count > 0))
        .collect();
    
    if pir_readings.is_empty() {
        let no_events = Paragraph::new("No PIR sensors detected in data")
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL).title("PIR Motion Data"));
        f.render_widget(no_events, area);
        return;
    }

    // Render only the two most useful charts
    render_pir_incremental_chart(f, chart_chunks[0], &pir_readings);
    render_pir_activity_chart(f, chart_chunks[1], &pir_readings);
    render_pir_table(f, main_chunks[1], &pir_readings);
}

fn render_pir_chart(f: &mut Frame, area: ratatui::layout::Rect, pir_readings: &[&SensorReading]) {
    // Sort readings by timestamp first
    let mut sorted_pir_readings = pir_readings.to_vec();
    sorted_pir_readings.sort_by_key(|r| r.timestamp);

    // Group by device and create datasets
    let mut device_datasets = std::collections::HashMap::new();
    
    for (i, reading) in sorted_pir_readings.iter().enumerate() {
        let entry = device_datasets.entry(reading.device_name.clone()).or_insert(Vec::new());
        entry.push((i as f64, reading.movement_count as f64));
    }

    if device_datasets.is_empty() {
        return;
    }

    let colors = [Color::Red, Color::Blue, Color::Green, Color::Yellow, Color::Magenta, Color::Cyan];
    let mut datasets = Vec::new();
    
    for (i, (device_name, data)) in device_datasets.iter().enumerate() {
        let color = colors[i % colors.len()];
        let short_name = if device_name.len() > 12 {
            format!("{}...", &device_name[..9])
        } else {
            device_name.clone()
        };
        
        datasets.push(
            Dataset::default()
                .name(format!("📊 {}", short_name))
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
                .graph_type(GraphType::Line)
                .data(data)
        );
    }

    let max_count = device_datasets.values()
        .flat_map(|data| data.iter().map(|(_, y)| *y))
        .fold(0.0, f64::max);

    // Create time labels for PIR chart
    let time_labels: Vec<String> = if sorted_pir_readings.len() > 1 {
        let start_time = sorted_pir_readings.first().unwrap().timestamp;
        let end_time = sorted_pir_readings.last().unwrap().timestamp;
        
        (0..=4).map(|i| {
            let timestamp = start_time + (i * (end_time - start_time)) / 4;
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp as i64)
                .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_else(|| format!("T{}", i))
        }).collect()
    } else {
        vec![]
    };

    let chart = Chart::new(datasets)
        .block(Block::default().title("📊 Total Movement Count (Cumulative)").borders(Borders::ALL))
        .x_axis(
            Axis::default()
                .title("Time (Hours:Minutes)")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, sorted_pir_readings.len() as f64])
                .labels(time_labels.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        )
        .y_axis(
            Axis::default()
                .title("Total Movement Count")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_count + 10.0])
        );
    f.render_widget(chart, area);
}

fn render_pir_activity_chart(f: &mut Frame, area: ratatui::layout::Rect, pir_readings: &[&SensorReading]) {
    // Sort readings by timestamp first
    let mut sorted_pir_readings = pir_readings.to_vec();
    sorted_pir_readings.sort_by_key(|r| r.timestamp);

    // Create bar chart data for motion events (1 = motion, 0 = no motion)
    let motion_data: Vec<(f64, f64)> = sorted_pir_readings
        .iter()
        .enumerate()
        .map(|(i, r)| (i as f64, if r.activity { 1.0 } else { 0.0 }))
        .collect();

    if motion_data.is_empty() {
        return;
    }

    let datasets = vec![
        Dataset::default()
            .name("🚨 Motion Detected")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .graph_type(GraphType::Line)
            .data(&motion_data),
    ];

    // Create time labels for activity chart
    let time_labels: Vec<String> = if sorted_pir_readings.len() > 1 {
        let start_time = sorted_pir_readings.first().unwrap().timestamp;
        let end_time = sorted_pir_readings.last().unwrap().timestamp;
        
        (0..=4).map(|i| {
            let timestamp = start_time + (i * (end_time - start_time)) / 4;
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp as i64)
                .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_else(|| format!("T{}", i))
        }).collect()
    } else {
        vec![]
    };

    let chart = Chart::new(datasets)
        .block(Block::default().title("🚨 Motion Detection - Alert Status").borders(Borders::ALL))
        .x_axis(
            Axis::default()
                .title("Time (Hours:Minutes)")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, sorted_pir_readings.len() as f64])
                .labels(time_labels.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        )
        .y_axis(
            Axis::default()
                .title("Detection Status")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, 1.2])
                .labels(vec!["0", "0.3", "0.6", "0.9", "1"])
        );
    f.render_widget(chart, area);
}

fn render_pir_incremental_chart(f: &mut Frame, area: ratatui::layout::Rect, pir_readings: &[&SensorReading]) {
    // Sort readings by timestamp first
    let mut sorted_pir_readings = pir_readings.to_vec();
    sorted_pir_readings.sort_by_key(|r| r.timestamp);

    // Group by device and calculate incremental movement counts
    let mut device_datasets = std::collections::HashMap::new();
    
    // Track previous movement count for each device to calculate deltas
    let mut device_prev_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    
    for (i, reading) in sorted_pir_readings.iter().enumerate() {
        let prev_count = device_prev_counts.get(&reading.device_name).unwrap_or(&0);
        let delta = if reading.movement_count >= *prev_count {
            reading.movement_count - *prev_count
        } else {
            // Handle case where device may have reset, show the full count as delta
            reading.movement_count
        };
        
        let entry = device_datasets.entry(reading.device_name.clone()).or_insert(Vec::new());
        entry.push((i as f64, delta as f64));
        
        device_prev_counts.insert(reading.device_name.clone(), reading.movement_count);
    }

    if device_datasets.is_empty() {
        return;
    }

    let colors = [Color::Red, Color::Blue, Color::Green, Color::Yellow, Color::Magenta, Color::Cyan];
    let mut datasets = Vec::new();
    
    for (i, (device_name, data)) in device_datasets.iter().enumerate() {
        let color = colors[i % colors.len()];
        let short_name = if device_name.len() > 8 {
            format!("{}...", &device_name[..5])
        } else {
            device_name.clone()
        };
        
        datasets.push(
            Dataset::default()
                .name(format!("📈 {}", short_name))
                .marker(symbols::Marker::Dot)
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
                .graph_type(GraphType::Line)
                .data(data)
        );
    }

    let max_delta = device_datasets.values()
        .flat_map(|data| data.iter().map(|(_, y)| *y))
        .fold(0.0, f64::max)
        .max(1.0); // Ensure at least 1 for scale

    // Create time labels for incremental chart
    let time_labels: Vec<String> = if sorted_pir_readings.len() > 1 {
        let start_time = sorted_pir_readings.first().unwrap().timestamp;
        let end_time = sorted_pir_readings.last().unwrap().timestamp;
        
        (0..=3).map(|i| {
            let timestamp = start_time + (i * (end_time - start_time)) / 3;
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp as i64)
                .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_else(|| format!("T{}", i))
        }).collect()
    } else {
        vec![]
    };

    let chart = Chart::new(datasets)
        .block(Block::default().title("📊 Motion Activity - New Movements").borders(Borders::ALL))
        .x_axis(
            Axis::default()
                .title("Time (Hours:Minutes)")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, sorted_pir_readings.len() as f64])
                .labels(time_labels.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        )
        .y_axis(
            Axis::default()
                .title("New Motions")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_delta + 1.0])
                .labels(vec![
                    "0".to_string(),
                    format!("{:.0}", (max_delta + 1.0) * 0.25),
                    format!("{:.0}", (max_delta + 1.0) * 0.50),
                    format!("{:.0}", (max_delta + 1.0) * 0.75),
                    format!("{:.0}", max_delta + 1.0),
                ])
        );
    f.render_widget(chart, area);
}

fn render_pir_table(f: &mut Frame, area: ratatui::layout::Rect, pir_readings: &[&SensorReading]) {
    let header_cells = ["Time", "Device", "Status", "Moves", "Battery"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Red)));
    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::Red))
        .height(1)
        .bottom_margin(1);

    let rows: Vec<Row> = pir_readings
        .iter()
        .rev()
        .take(15) // Show more entries
        .map(|reading| {
            let device = if reading.device_name.len() > 12 {
                format!("{}...", &reading.device_name[..9])
            } else {
                reading.device_name.clone()
            };
            
            let time = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(reading.timestamp as i64)
                .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "??:??:??".to_string());

            let status = if reading.activity {
                "🚨 MOTION"
            } else if reading.movement_count > 0 {
                "📊 Data"
            } else {
                "💤 Idle"
            };

            let status_style = if reading.activity {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else if reading.movement_count > 0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Gray)
            };

            Row::new(vec![
                Cell::from(time),
                Cell::from(device),
                Cell::from(status).style(status_style),
                Cell::from(format!("{}", reading.movement_count)),
                Cell::from(format!("{:.2}V", reading.battery_voltage)),
            ])
        })
        .collect();

    let table = Table::new(rows, [
        Constraint::Length(9),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(6),
        Constraint::Length(8),
    ])
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("🚨 PIR Data"))
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_symbol(">> ");

    f.render_widget(table, area);
}

fn render_device_select_tab(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Device list
    let devices = app.get_unique_devices();
    let mut device_items = vec![ListItem::new("All devices")];
    device_items.extend(devices.iter().map(|d| ListItem::new(d.as_str())));

    let device_list = List::new(device_items)
        .block(Block::default().borders(Borders::ALL).title("Select Device"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    f.render_stateful_widget(device_list, chunks[0], &mut app.device_list_state.clone());

    // Device stats
    if let Some(selected_device) = &app.selected_device {
        let device_readings: Vec<_> = app.storage_manager.get_readings()
            .iter()
            .filter(|r| &r.device_name == selected_device)
            .collect();

        if !device_readings.is_empty() {
            let latest = device_readings.last().unwrap();
            let avg_temp = device_readings.iter().map(|r| r.temperature).sum::<f32>() / device_readings.len() as f32;
            let avg_hum = device_readings.iter().map(|r| r.humidity).sum::<f32>() / device_readings.len() as f32;
            let motion_events = device_readings.iter().filter(|r| r.activity).count();

            let next_packet_info = app.get_next_packet_info(&latest.device_eui)
                .map(|(_, countdown)| {
                    if countdown.starts_with('-') {
                        format!("⚠️  Overdue by {}", &countdown[1..])
                    } else {
                        format!("⏱️  Next in {}", countdown)
                    }
                })
                .unwrap_or_else(|| "❓ Unknown".to_string());

            let stats_text = format!(
                "Device: {}\n\nLatest Reading:\n  Temperature: {:.1}°C\n  Humidity: {:.1}%\n  Battery: {:.2}V ({})\n\nNext Packet:\n  {}\n\nAverages:\n  Temperature: {:.1}°C\n  Humidity: {:.1}%\n\nPIR Events: {}\nTotal Readings: {}",
                selected_device,
                latest.temperature,
                latest.humidity,
                latest.battery_voltage,
                latest.battery_status,
                next_packet_info,
                avg_temp,
                avg_hum,
                motion_events,
                device_readings.len()
            );

            let stats = Paragraph::new(stats_text)
                .style(Style::default().fg(Color::White))
                .block(Block::default().borders(Borders::ALL).title("Device Statistics"))
                .wrap(Wrap { trim: true });
            f.render_widget(stats, chunks[1]);
        }
    } else if app.show_all_devices {
        let total_readings = app.storage_manager.get_memory_count();
        let total_devices = devices.len();
        let total_motion_events = app.get_pir_events().len();

        let stats_text = format!(
            "All Devices Overview\n\nTotal Devices: {}\nTotal Readings: {}\nTotal PIR Events: {}\n\nDevices:\n{}",
            total_devices,
            total_readings,
            total_motion_events,
            devices.join("\n")
        );

        let stats = Paragraph::new(stats_text)
            .style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL).title("Overall Statistics"))
            .wrap(Wrap { trim: true });
        f.render_widget(stats, chunks[1]);
    }
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    mut rx: mpsc::Receiver<SensorReading>,
) -> Result<(), Box<dyn Error>> {
    loop {
        terminal.draw(|f| ui(f, &app))?;

        // Handle events with timeout
        if crossterm::event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => {
                        app.should_quit = true;
                        break;
                    }
                    KeyCode::Tab => app.next_tab(),
                    KeyCode::BackTab => app.previous_tab(),
                    KeyCode::Down => {
                        if app.selected_tab == 2 {
                            app.next_device();
                        } else {
                            app.next();
                        }
                    }
                    KeyCode::Up => {
                        if app.selected_tab == 2 {
                            app.previous_device();
                        } else {
                            app.previous();
                        }
                    }
                    KeyCode::Char(' ') => {
                        if app.selected_tab == 2 {
                            // Space selects device in device select tab
                            app.next_device();
                        }
                    }
                    KeyCode::Char('-') => app.previous_time_window(),
                    KeyCode::Char('+') => app.next_time_window(),
                    KeyCode::Char('c') => app.selected_tab = 2, // Jump to device select tab
                    _ => {}
                }
            }
        }

        // Check for new sensor readings
        while let Ok(reading) = rx.try_recv() {
            app.add_reading(reading);
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

async fn websocket_handler(tx: mpsc::Sender<SensorReading>, device_names: HashMap<String, String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let token = env::args()
        .nth(1)
        .expect("Please provide Loriot Token as argument.");
    let url = format!("wss://eu1.loriot.io/app?token={}", token);
    let (ws_stream, _) = connect_async(&url).await?;

    let (mut write, mut read) = ws_stream.split();

    // Send cache query
    let cache_query = serde_json::json!({
        "cmd": "cq",
        "page": 1,
        "perPage": 100,
        "filter": {
            "from": chrono::Utc::now().timestamp_millis() - 86400000
        }
    });

    write.send(Message::Text(cache_query.to_string().into())).await?;

    while let Some(msg) = read.next().await {
        match msg {
            Ok(msg) if msg.is_text() => {
                let txt = msg.to_text().unwrap();
                if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(txt) {
                    if let Some(cmd) = json_value.get("cmd").and_then(|v| v.as_str()) {
                        match cmd {
                            "rx" | "gw" => {
                                if let Ok(payload) = serde_json::from_value::<LoriotPayload>(json_value.clone()) {
                                    if let Some((battery, bat_status, temp, hum, activity, movement_count, lux, _ext_sensor)) = decode_lht65s_pir(&payload.data) {
                                        let device_name = device_names.get(&payload.eui).cloned().unwrap_or_else(|| payload.eui.clone());
                                        
                                        let reading = SensorReading {
                                            timestamp: payload.ts,
                                            device_eui: payload.eui,
                                            device_name,
                                            temperature: temp,
                                            humidity: hum,
                                            battery_voltage: battery,
                                            battery_status: bat_status,
                                            activity,
                                            movement_count,
                                            lux,
                                        };
                                        
                                        if let Err(_) = tx.send(reading).await {
                                            break; // Receiver dropped
                                        }
                                    }
                                }
                            }
                            "cq" => {
                                // Handle cached messages
                                if let Some(cache_array) = json_value.get("cache").and_then(|v| v.as_array()) {
                                    for cached_msg in cache_array {
                                        if let Ok(payload) = serde_json::from_value::<LoriotPayload>(cached_msg.clone()) {
                                            if let Some((battery, bat_status, temp, hum, activity, movement_count, lux, _ext_sensor)) = decode_lht65s_pir(&payload.data) {
                                                let device_name = device_names.get(&payload.eui).cloned().unwrap_or_else(|| payload.eui.clone());
                                                
                                                let reading = SensorReading {
                                                    timestamp: payload.ts,
                                                    device_eui: payload.eui,
                                                    device_name,
                                                    temperature: temp,
                                                    humidity: hum,
                                                    battery_voltage: battery,
                                                    battery_status: bat_status,
                                                    activity,
                                                    movement_count,
                                                    lux,
                                                };
                                                
                                                if let Err(_) = tx.send(reading).await {
                                                    return Ok(()); // Receiver dropped
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(e) => eprintln!("WebSocket error: {}", e),
            _ => {}
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and communication channel
    let mut app = App::new();
    let device_names = app.device_names.clone();
    
    // Initialize storage
    if let Err(e) = app.initialize_storage().await {
        eprintln!("Warning: Failed to initialize storage: {}", e);
    }
    
    let (tx, rx) = mpsc::channel(100);

    // Spawn WebSocket handler
    let websocket_handle = tokio::spawn(async move {
        if let Err(e) = websocket_handler(tx, device_names).await {
            eprintln!("WebSocket error: {}", e);
        }
    });

    // Run the TUI
    let res = run_app(&mut terminal, app, rx);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Clean up
    websocket_handle.abort();

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}