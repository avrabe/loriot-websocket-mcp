//! haus: home climate history from LoRaWAN (LORIOT) and Loxone.
//!
//! The server keeps every reading, serves a REST/SSE API and a dashboard,
//! and reports each source's connection state on `/health`.

mod api;
mod import_cmd;
mod loxone;
mod sse;
mod state;
mod websocket;

use axum::{extract::State, routing::get, Json, Router};
use haus_core::{load_device_names, AppConfig, StorageConfig, StorageManager};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub use state::AppState;
use state::{Sources, UpstreamHealth};

const USAGE: &str = "\
Usage: haus [--help | --version]
       haus import loriot|stats4lox <path> [options]   (haus import --help)

Keeps home climate readings (LoRaWAN via LORIOT) and serves them over HTTP.

Environment:
  LORIOT_WS_TOKEN  LORIOT application token (required to run)
  HAUS_STORE       time-series store file (default haus.redb)
  LOXONE_URL       ws:// or wss:// address of a Loxone Miniserver (optional;
                   turns the Loxone source on)
  LOXONE_USER, LOXONE_PASSWORD   Miniserver user (required with LOXONE_URL)
  LOXONE_TOKEN_FILE  where the Miniserver token is kept (default loxone-token.json)
  LOXONE_MAP       optional JSON file {\"<uuid>/<field>\": \"<state name or uuid>\"}
  BIND             listen address (falls back to HOST, default 0.0.0.0)
  PORT             listen port (default 3000)
  RUST_LOG         log filter (default haus=debug,tower_http=info)";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
enum Cli {
    Run,
    Import(import_cmd::ImportArgs),
    Exit { code: i32, stdout: String },
}

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Cli {
    let args: Vec<String> = args.into_iter().collect();
    match args.first().map(String::as_str) {
        None => Cli::Run,
        Some("-V" | "--version") if args.len() == 1 => Cli::Exit {
            code: 0,
            stdout: format!("haus {}", env!("CARGO_PKG_VERSION")),
        },
        Some("-h" | "--help") if args.len() == 1 => Cli::Exit {
            code: 0,
            stdout: USAGE.to_string(),
        },
        Some("import") if matches!(args.get(1).map(String::as_str), Some("-h" | "--help")) => {
            Cli::Exit {
                code: 0,
                stdout: import_cmd::USAGE.to_string(),
            }
        }
        Some("import") => match import_cmd::parse(&args[1..], std::env::var("HAUS_STORE").ok()) {
            Ok(a) => Cli::Import(a),
            Err(e) => Cli::Exit {
                code: 2,
                stdout: format!("haus import: {e}\n\n{}", import_cmd::USAGE),
            },
        },
        Some(_) => Cli::Exit {
            code: 2,
            stdout: String::new(),
        },
    }
}

/// Listen host from `BIND`, then `HOST`, else all interfaces (the old default).
fn bind_host(bind: Option<String>, host: Option<String>) -> String {
    bind.or(host)
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "0.0.0.0".to_string())
}

/// Default log filter when RUST_LOG is unset.
const DEFAULT_LOG_FILTER: &str = "haus=debug,tower_http=info";

/// Add request logging and CORS. Every request to `app` gets one INFO line
/// when its response is sent (request start is DEBUG). `health` is merged in
/// *after* the trace layer, so watchdog probes are never logged.
fn with_http_layers(app: Router, health: Router) -> Router {
    app.layer(
        TraceLayer::new_for_http()
            .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
            .on_request(DefaultOnRequest::new().level(Level::DEBUG))
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
    )
    .merge(health)
    .layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    )
}

/// One source's entry in `/health`.
fn source_health(h: &UpstreamHealth) -> serde_json::Value {
    let last = h.last_message();
    serde_json::json!({
        "connected": h.connected(),
        "last_message_at": last.map(|t| t.to_rfc3339()),
        "last_message_age_secs": last.map(|t| (chrono::Utc::now() - t).num_seconds().max(0)),
    })
}

/// GET /health - liveness of the process and runtime. Always 200 while the
/// runtime can answer; the source states are informational only (a watchdog
/// must not restart the service just because a source is unreachable).
/// `sources` has one entry per configured source (REQ-HEALTH-001);
/// `upstream` repeats `sources.loriot` for watchdogs written against 0.3.0.
async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut sources = serde_json::Map::new();
    sources.insert("loriot".into(), source_health(&state.sources.loriot));
    if let Some(lox) = &state.sources.loxone {
        sources.insert("loxone".into(), source_health(lox));
    }
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "sources": sources,
        "upstream": source_health(&state.sources.loriot),
    }))
}

fn main() -> anyhow::Result<()> {
    match parse_args(std::env::args().skip(1)) {
        Cli::Run => {}
        Cli::Import(a) => std::process::exit(import_cmd::run(&a)),
        Cli::Exit { code, stdout } => {
            if code == 0 {
                println!("{stdout}");
            } else if stdout.is_empty() {
                eprintln!("haus: unrecognized arguments\n\n{USAGE}");
            } else {
                eprintln!("{stdout}");
            }
            std::process::exit(code);
        }
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> anyhow::Result<()> {
    // Install default rustls crypto provider (required for TLS connections)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| DEFAULT_LOG_FILTER.into()),
        )
        .init();

    // Load configuration from environment
    let loriot_token = std::env::var("LORIOT_WS_TOKEN")
        .map_err(|_| anyhow::anyhow!("LORIOT_WS_TOKEN environment variable must be set"))?;

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

    // The history file can be large: parse it on a blocking thread.
    let storage = state::run_blocking(move || {
        let mut storage = StorageManager::new(storage_config);
        storage.load_existing_data().map(|()| storage)
    })
    .await?;

    tracing::info!("Loaded {} readings from storage", storage.reading_count());

    // Load device names
    let device_names = config
        .device_names_file
        .as_ref()
        .map(|f| load_device_names(f))
        .unwrap_or_default();

    // The time-series store (DD-003); opening may recover a file, so off the runtime.
    let store_path = std::env::var("HAUS_STORE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "haus.redb".to_string());
    let store = {
        let p = store_path.clone();
        state::run_blocking(move || haus_core::store::Store::open(p))
            .await
            .map_err(|e| anyhow::anyhow!("{store_path}: {e}"))?
    };
    tracing::info!("Store {store_path}: {} series", store.series()?.len());
    let ingest = std::sync::Arc::new(haus_core::ingest::Ingestor::new(std::sync::Arc::new(store)));

    // The Loxone source is on when LOXONE_URL is set.
    let loxone = loxone::LoxoneConfig::from_env().map_err(|e| anyhow::anyhow!(e))?;
    let sources = Sources {
        loriot: UpstreamHealth::default(),
        loxone: loxone.as_ref().map(|_| UpstreamHealth::default()),
    };

    // Create shared application state
    let state = AppState::new(storage, device_names, config.clone(), ingest, sources);

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
        .with_state(state.clone());
    // Health check (liveness; see `health`), outside the request log.
    let health_router = Router::new()
        .route("/health", get(health))
        .with_state(state.clone());
    let app = with_http_layers(app, health_router);

    // Spawn the Loriot WebSocket handler
    let ws_state = state.clone();
    tokio::spawn(async move {
        websocket::run_loriot_connection(ws_state).await;
    });

    if let Some(cfg) = loxone {
        let lox_state = state.clone();
        tokio::spawn(async move {
            loxone::run_loxone_connection(lox_state, cfg).await;
        });
    }

    // Start the server
    let host = bind_host(std::env::var("BIND").ok(), std::env::var("HOST").ok());
    let listener = tokio::net::TcpListener::bind((host.as_str(), config.web_port))
        .await
        .map_err(|e| anyhow::anyhow!("cannot listen on {host}:{}: {e}", config.web_port))?;
    tracing::info!("Starting server on {}", listener.local_addr()?);

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    // rivet: verifies REQ-NAME-001
    #[test]
    fn version_flag_prints_name_and_semver() {
        let want = Cli::Exit {
            code: 0,
            stdout: format!("haus {}", env!("CARGO_PKG_VERSION")),
        };
        assert_eq!(env!("CARGO_PKG_NAME"), "haus");
        assert_eq!(env!("CARGO_BIN_NAME"), "haus");
        assert_eq!(parse_args(args(&["--version"])), want);
        assert_eq!(parse_args(args(&["-V"])), want);
    }

    #[test]
    fn help_and_unknown_flags() {
        assert!(matches!(
            parse_args(args(&["--help"])),
            Cli::Exit { code: 0, .. }
        ));
        assert!(matches!(
            parse_args(args(&["--bogus"])),
            Cli::Exit { code: 2, .. }
        ));
        assert!(matches!(
            parse_args(args(&["--version", "x"])),
            Cli::Exit { code: 2, .. }
        ));
        assert_eq!(parse_args(args(&[])), Cli::Run);
    }

    #[test]
    fn bind_host_precedence() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(bind_host(None, None), "0.0.0.0");
        assert_eq!(bind_host(s("127.0.0.1"), s("10.0.0.1")), "127.0.0.1");
        assert_eq!(bind_host(None, s("::1")), "::1");
        assert_eq!(bind_host(s("  "), None), "0.0.0.0");
    }

    /// Collects formatted log output.
    #[derive(Clone, Default)]
    struct LogBuf(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for LogBuf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuf {
        type Writer = LogBuf;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn requests_logged_at_info_except_health() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        let buf = LogBuf::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_ansi(false)
            .with_env_filter(tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let app = with_http_layers(
            Router::new().route("/api/ping", get(|| async { "pong" })),
            Router::new().route("/health", get(|| async { "ok" })),
        );
        for uri in ["/health", "/api/ping"] {
            let resp = app
                .clone()
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "{uri}");
        }

        let log = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(
            log.contains("uri=/api/ping"),
            "API request not logged:\n{log}"
        );
        assert!(log.contains("finished processing request"), "{log}");
        assert!(!log.contains("/health"), "/health was logged:\n{log}");
    }

    // rivet: verifies REQ-HEALTH-001
    #[tokio::test]
    async fn health_is_ok_and_reports_every_source() {
        let state = crate::state::tests::test_state("health", AppConfig::default());

        // Source down and never seen: still "ok". No Loxone configured: absent.
        let Json(body) = health(State(state.clone())).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["sources"]["loriot"]["connected"], false);
        assert!(body["sources"]["loriot"]["last_message_at"].is_null());
        assert!(body["sources"].get("loxone").is_none());
        assert_eq!(body["upstream"], body["sources"]["loriot"]);

        state.sources.loriot.set_connected(true);
        state.sources.loriot.record_message();
        let Json(body) = health(State(state)).await;
        assert_eq!(body["sources"]["loriot"]["connected"], true);
        assert!(body["sources"]["loriot"]["last_message_at"].is_string());
        assert_eq!(body["sources"]["loriot"]["last_message_age_secs"], 0);
        assert_eq!(body["upstream"], body["sources"]["loriot"]);

        // A configured Loxone source shows up next to it.
        let mut with_lox = crate::state::tests::test_state("health-lox", AppConfig::default());
        with_lox.sources = std::sync::Arc::new(Sources {
            loriot: UpstreamHealth::default(),
            loxone: Some(UpstreamHealth::default()),
        });
        let Json(body) = health(State(with_lox)).await;
        assert_eq!(body["sources"]["loxone"]["connected"], false);
    }
}
