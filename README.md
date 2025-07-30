# LoRaWAN Sensor Monitor

A real-time terminal user interface (TUI) application for monitoring LoRaWAN sensor data via Loriot WebSocket API. Features live data visualization, historical data persistence, and comprehensive sensor analytics.

![LoRaWAN Sensor Monitor](https://img.shields.io/badge/LoRaWAN-Sensor%20Monitor-blue)
![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)
![Terminal](https://img.shields.io/badge/Terminal-TUI-green)

## Features

### 📊 Real-Time Monitoring
- **Live data visualization** via Loriot WebSocket connection
- **Multi-sensor support** with automatic device detection
- **Temperature & humidity charts** with chronological time axes
- **PIR motion detection** with activity tracking and alerts
- **Battery monitoring** with voltage levels and status indicators

### 🕒 Time Window Filtering
- **Flexible time ranges**: All time, Last hour, 6 hours, day, or week
- **Quick switching** with `+/-` keyboard shortcuts
- **Real-time chart updates** based on selected time window
- **Local timezone display** for accurate time representation

### 🔍 Advanced PIR Analytics
- **Motion activity tracking** with incremental analysis
- **Three-view PIR dashboard**: Activity rate, detection events, and data table
- **Movement count analysis** showing actual motion patterns
- **Alert status monitoring** with real-time detection flags

### 💾 Data Persistence
- **Automatic data storage** in human-readable JSON Lines format
- **Startup data loading** from existing storage files
- **Duplicate prevention** with intelligent deduplication
- **Memory-efficient caching** (configurable, default 2000 readings)
- **Unlimited historical storage** with two-tier memory management

### 🎯 Device Management
- **Multi-device support** with custom device naming
- **Device filtering** for focused analysis
- **Per-device statistics** including averages and event counts
- **Next packet countdown** based on 20-minute transmission intervals

### ⌨️ Intuitive Controls
- **Keyboard navigation** optimized for German Mac keyboards
- **Tab-based interface**: Overview, PIR Events, Device Selection
- **Quick shortcuts**: `c` for device selection, `+/-` for time windows
- **Real-time status** with storage and filtering information

## Supported Sensors

### Dragino LHT65S-PIR
- **Temperature & Humidity** monitoring with ±0.01°C precision
- **PIR motion detection** with activity flags and movement counting
- **Battery monitoring** with voltage and status reporting
- **20-minute transmission intervals** with packet countdown timers

## Installation

### Prerequisites
- **Rust 1.70+** (2021 edition)
- **Loriot account** with WebSocket API access
- **Terminal** with Unicode and color support

### Quick Start
```bash
# Clone the repository
git clone https://github.com/avrabe/loriot-websocket-mcp.git
cd loriot-websocket-mcp

# Build the application
cargo build --release

# Run with your Loriot token
cargo run --release -- YOUR_LORIOT_TOKEN
```

### Device Configuration (Optional)
Create a `devices.json` file to customize device names:
```json
{
  "A840410A618A0320": "Kitchen Sensor",
  "A84041FDB18A0312": "Basement Sensor", 
  "A840419D718A031D": "Garage Sensor"
}
```

## Usage

### Basic Operation
```bash
# Start monitoring with your Loriot WebSocket token
./target/release/lora_ws_logger YOUR_LORIOT_TOKEN
```

### Keyboard Controls
| Key | Action |
|-----|--------|
| `q` | Quit application |
| `Tab` | Switch between tabs (Overview → PIR Events → Device Select) |
| `↑↓` | Navigate in tables and lists |
| `Space` | Select device in device selection tab |
| `+/-` | Change time window (All → Hour → 6H → Day → Week) |
| `c` | Jump to device selection tab |

### Interface Tabs

#### 1. Overview Tab
- **Temperature & Humidity charts** with local time axes
- **Recent readings table** with next packet countdown
- **Device and time filtering** controls
- **Real-time data updates** from WebSocket connection

#### 2. PIR Events Tab
- **Motion activity chart** showing movement deltas over time
- **Detection status chart** displaying real-time motion alerts
- **PIR data table** with activity status and movement counts
- **Time-synchronized visualization** for activity analysis

#### 3. Device Select Tab
- **Device list** with navigation controls
- **Per-device statistics** including temperature/humidity averages
- **PIR event counts** and total readings per device
- **Next packet information** with countdown timers

## Data Storage

### Storage Format
Data is automatically saved to `sensor_history.jsonl` in JSON Lines format:
```json
{"cmd":"rx","eui":"A840410A618A0320","data":"","ts":1732024800000,"parsed_temperature":23.5,"parsed_humidity":65.2,"parsed_battery_voltage":3.17,"parsed_battery_status":"Good","parsed_activity":false,"parsed_movement_count":42}
```

### Memory Management
- **File storage**: Unlimited historical data
- **Memory cache**: Recent 2000 readings (configurable)
- **Automatic loading**: Startup sync with existing data
- **Duplicate prevention**: Timestamp + device EUI deduplication

### Configuration
Storage behavior can be customized by modifying `StorageConfig` in the source:
```rust
StorageConfig {
    file_path: "sensor_history.jsonl".to_string(),
    memory_limit_readings: 2000,  // Readings kept in memory
    backup_count: 3,              // Backup file versions
    sync_interval_minutes: 60,    // Future sync interval
}
```

## Architecture

### Components
- **StorageManager**: Handles data persistence and memory management
- **WebSocket Handler**: Manages Loriot API connection and message processing
- **TUI Application**: Ratatui-based interface with multi-tab navigation
- **Sensor Decoder**: Dragino LHT65S-PIR payload parser
- **Time Management**: Window filtering and timezone handling

### Data Flow
1. **WebSocket** receives sensor data from Loriot
2. **Decoder** parses binary sensor payloads
3. **StorageManager** persists data and manages memory cache
4. **TUI** renders real-time visualizations and tables
5. **User interaction** controls filtering and navigation

## Development

### Building from Source
```bash
# Debug build
cargo build

# Release build (recommended)
cargo build --release

# Run tests
cargo test

# Check code formatting
cargo fmt --check

# Run clippy lints
cargo clippy
```

### Project Structure
```
src/
├── main.rs           # Main application and TUI logic
├── storage/          # Data persistence (future module)
└── sensors/          # Sensor decoders (future module)

Cargo.toml           # Dependencies and metadata
devices.json         # Device name configuration (optional)
sensor_history.jsonl # Data storage file (auto-created)
```

## Future Roadmap

### Planned Features
- **MCP (Model Context Protocol) integration** for external data access
- **Additional sensor support** (temperature-only, GPS, etc.)
- **Data export capabilities** (CSV, JSON, SQLite)
- **Web dashboard** with remote monitoring
- **Alert system** with configurable thresholds
- **Historical data analysis** with trend detection

### MCP Integration
Future versions will expose sensor data via MCP, enabling:
- **External tool access** to real-time and historical data
- **Automated analysis** and reporting capabilities
- **Integration with AI assistants** for intelligent monitoring
- **Custom dashboards** and data visualization tools

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'feat: add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Code Style
- Use conventional commit messages
- Follow Rust formatting guidelines (`cargo fmt`)
- Add tests for new functionality
- Update documentation for public APIs

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- **Loriot** for providing excellent LoRaWAN network services
- **Dragino** for reliable sensor hardware
- **Ratatui** community for the outstanding TUI framework
- **Rust community** for comprehensive async ecosystem

## Support

- **Issues**: Report bugs via GitHub Issues
- **Documentation**: Check inline code documentation
- **Community**: Join discussions in GitHub Discussions
- **Contact**: Open an issue for questions or feature requests

---

**Made with ❤️ in Rust** | **Real-time LoRaWAN monitoring made simple**