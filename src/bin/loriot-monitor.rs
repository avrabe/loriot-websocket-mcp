use clap::{Parser, ValueEnum};
use loriot_websocket_mcp::{AppConfig, OperationMode, AppError, Result};

#[derive(Parser)]
#[command(name = "loriot-monitor")]
#[command(about = "LoRaWAN sensor monitoring with MCP server capabilities")]
#[command(version = "0.2.0")]
struct Cli {
    /// Loriot WebSocket token
    #[arg(value_name = "TOKEN")]
    token: String,

    /// Operation mode
    #[arg(short, long, value_enum, default_value_t = Mode::Tui)]
    mode: Mode,

    /// Device names configuration file
    #[arg(short, long, default_value = "devices.json")]
    devices: Option<String>,

    /// Storage file for sensor data
    #[arg(short, long, default_value = "sensor_history.jsonl")]
    storage: Option<String>,

    /// Disable TUI and run in background mode
    #[arg(long)]
    no_tui: bool,

    /// MCP server port (for HTTP mode)
    #[arg(short, long, default_value_t = 8080)]
    port: u16,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Mode {
    /// Terminal user interface (default)
    Tui,
    /// MCP server via stdin/stdout
    McpStdin,
    /// MCP server via Server-Sent Events
    McpSse,
    /// MCP server via HTTP streaming
    McpHttp,
}

impl From<Mode> for OperationMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Tui => OperationMode::Tui,
            Mode::McpStdin => OperationMode::McpStdin,
            Mode::McpSse => OperationMode::McpSse,
            Mode::McpHttp => OperationMode::McpHttp,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = AppConfig {
        mode: cli.mode.into(),
        disable_tui: cli.no_tui,
        loriot_token: cli.token,
        device_names_file: cli.devices,
        storage_file: cli.storage,
    };

    match config.mode {
        OperationMode::Tui => {
            // Import and run the TUI application
            if config.disable_tui {
                return Err(AppError::Config("Cannot disable TUI in TUI mode".to_string()));
            }
            
            // For now, we'll just print a message since the full TUI hasn't been extracted yet
            println!("Starting TUI mode with token: {}", config.loriot_token);
            println!("This will be implemented after the modular refactoring is complete.");
            Ok(())
        }
        OperationMode::McpStdin => {
            println!("MCP stdin mode not yet implemented - waiting for MCP 0.7.0 release");
            Ok(())
        }
        OperationMode::McpSse => {
            println!("MCP SSE mode not yet implemented - waiting for MCP 0.7.0 release");
            Ok(())
        }
        OperationMode::McpHttp => {
            println!("MCP HTTP mode not yet implemented - waiting for MCP 0.7.0 release");
            println!("Would listen on port: {}", cli.port);
            Ok(())
        }
    }
}