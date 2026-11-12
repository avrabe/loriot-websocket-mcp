# LoRaWAN Sensor Monitor

`loriot-server` reads LoRaWAN sensor uplinks from the LORIOT cloud websocket,
keeps their history on disk and serves a web dashboard plus a REST/SSE API.
It is a single static binary meant to run under systemd (for example on a
Raspberry Pi 4) next to a directory with the dashboard's static files.

## What is in the workspace

| Crate                  | What it is |
|------------------------|------------|
| `crates/loriot-server` | the server binary: LORIOT websocket client, storage, REST/SSE API, static file serving |
| `crates/loriot-web`    | the dashboard, a Leptos (wasm) single-page app built with `trunk` |
| `crates/loriot-core`   | shared types and logic: sensor decoding, JSONL storage, analytics, weather (DWD, Open-Meteo), thermal, heat pump and energy models |

Dashboard pages: dashboard, device detail (`/device/<eui>`), analytics,
history, weather, heat pump, thermal, forecast, energy (`/energy`), and a
placeholder sensor health page (`/sensor-health`).

Supported sensor: Dragino LHT65S-PIR (temperature, humidity, PIR activity and
movement count, battery voltage and status).

## Running

```sh
LORIOT_WS_TOKEN=... BIND=127.0.0.1 PORT=3000 ./loriot-server
```

### Configuration (environment)

| Variable          | Default                                 | Meaning |
|-------------------|-----------------------------------------|---------|
| `LORIOT_WS_TOKEN` | (required)                              | LORIOT application token |
| `BIND`            | `HOST`, else `0.0.0.0`                  | listen address, e.g. `127.0.0.1` behind a reverse proxy |
| `PORT`            | `3000`                                  | listen port |
| `RUST_LOG`        | `loriot_server=debug,tower_http=info`   | log filter ([`EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) syntax) |

`loriot-server --version` prints `loriot-server <version>`; `--help` lists the
variables above and exits 0; unknown arguments exit 2.

With the default filter every HTTP request is logged once at info when its
response is sent. `/health` is never logged, so a watchdog polling it does not
fill the journal.

### Files in the working directory

| Path                    | Content |
|-------------------------|---------|
| `assets/`               | the dashboard (see below); read only |
| `sensor_history.jsonl`  | every sensor reading, one JSON object per line, append-only |
| `devices.json`          | optional EUI → display name map, e.g. `{"0011223344556677": "Kitchen"}` |
| `weather_history.jsonl` | fetched DWD daily weather |
| `weather_location.json` | configured weather location |
| `energy_data.json`      | energy / consumption data entered in the dashboard |

The last 2000 readings are kept in memory; requests that need the full history
(data range, analytics, thermal, forecast, heat pump, weather statistics) read
the JSONL file on a blocking thread, so they never stall the server.

### The dashboard (`assets/`)

The server serves the dashboard from `assets/` **in its working directory**.
Unknown paths fall back to `assets/index.html` (status 200), so the
single-page app's own routes work on reload. Without `assets/` the API still
works and `/` answers 404.

Releases ship it as `loriot-web-<ver>-assets.tar.gz`, which contains the
`assets/` directory. Unpack it in the server's working directory:

```sh
cd /path/to/working/dir && tar -xzf loriot-web-<ver>-assets.tar.gz
```

To build it yourself: `npm ci` in the repository root (tailwind), then
`trunk build --release` in `crates/loriot-web` (output goes to `assets/` in the
repository root).

The dashboard uses absolute paths (`/api`, `/sse`, `/`), so serve it at the
root of a (sub)domain, not under a path prefix.

### Health check

`GET /health` answers `200` whenever the process and its runtime are alive,
even while LORIOT is unreachable, so a watchdog does not restart the service
in a loop. The upstream state is part of the body:

```json
{"status":"ok","version":"0.3.0",
 "upstream":{"connected":true,"last_message_at":"2026-11-06T21:00:00+00:00","last_message_age_secs":42}}
```

The websocket client detects a dead or silent connection (no frame for 75 s,
pinging every 30 s), bounds the handshake and every write with a timeout, and
reconnects with exponential backoff (1 s doubling to 60 s, with jitter).

### HTTP API

| Path | |
|------|-|
| `GET /health` | liveness, see above |
| `GET /api/info`, `/api/data-range` | storage and connection summary, earliest/latest reading |
| `GET /api/readings[/{eui}]`, `/api/devices[/{eui}]` | recent readings and per-device statistics (query: `time_window`, `start_ts`, `end_ts`, `page`, `page_size`) |
| `GET /api/analytics[/{eui}]` | analytics over the full history |
| `GET /sse/readings`, `/sse/status` | server-sent events: new readings, connection status |
| `/api/weather/*`, `/api/heatpump/*`, `/api/thermal/*`, `/api/forecast/*`, `/api/energy/*` | the dashboard's weather, heat pump, thermal, forecast and energy pages |

## Releases and verification

Tags `v*` build these assets (`<ver>` without the `v`):

- `loriot-server-<ver>-linux-arm64` (static, aarch64 musl, e.g. Raspberry Pi 4)
- `loriot-server-<ver>-linux-amd64` (static, x86_64 musl)
- `loriot-server-<ver>-macos-arm64`
- `loriot-web-<ver>-assets.tar.gz` (the dashboard, see above)
- `sbom.json` (CycloneDX)
- `SHA256SUMS.txt` over all of the above, and its keyless cosign signature
  `SHA256SUMS.txt.cosign.bundle`

Verify the signature over the checksums, then the files against the checksums:

```sh
cosign verify-blob \
  --certificate-identity-regexp '^https://github.com/avrabe/loriot-websocket-mcp/\.github/workflows/' \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  --bundle SHA256SUMS.txt.cosign.bundle \
  SHA256SUMS.txt
sha256sum --ignore-missing -c SHA256SUMS.txt
```

Each binary and the assets tarball also carry a GitHub build-provenance
attestation:

```sh
gh attestation verify loriot-server-<ver>-linux-arm64 --repo avrabe/loriot-websocket-mcp
```

## Development

```sh
cargo build                     # server (default workspace members)
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
npm ci && (cd crates/loriot-web && trunk build)   # dashboard into ./assets
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy (workspace; `loriot-core`
without features; `loriot-web` for wasm32), the tests, `npm audit` and the
trunk build on every pull request.

## MCP Server Integration

> **Not built at the moment.** `src/mcp_server.rs` (from main) is not part of
> any crate in the Cargo workspace yet, so the commands below do not work until
> it is ported to the workspace.

This project now includes **Model Context Protocol (MCP) server support**, enabling AI assistants and external tools to access sensor data programmatically.

### What is MCP?

The Model Context Protocol (MCP) is a standardized protocol that allows AI assistants (like Claude) to securely connect to external data sources and tools. With MCP, you can query your sensor data through natural language or programmatic interfaces.

### Available Tools

The MCP server exposes the following tools:

- **get_latest_readings** - Get the latest sensor readings from all devices (default: 10)
- **list_devices** - List all known sensor devices with their basic information
- **get_pir_events** - Get PIR motion detection events (default limit: 20)
- **get_summary** - Get summary statistics across all devices

### Running the MCP Server

```bash
# Build the MCP server
cargo build --release --bin mcp_server

# Run the MCP server (stdio transport)
./target/release/mcp_server
```

The MCP server loads data from `sensor_history.jsonl` and responds to MCP protocol requests via standard input/output (stdio transport).

### Integrating with Claude Desktop

To use the MCP server with Claude Desktop, add it to your MCP settings configuration:

```json
{
  "mcpServers": {
    "loriot-sensor-data": {
      "command": "/path/to/loriot-websocket-mcp/target/release/mcp_server",
      "args": []
    }
  }
}
```

After configuration, Claude can query your sensor data using natural language:
- "Show me the latest readings from all sensors"
- "What are the PIR motion events?"
- "Give me a summary of all sensor data"

### MCP Dependencies

The MCP server uses:
- **rmcp 0.8** - Official Rust SDK for Model Context Protocol
- **schemars 1** - JSON Schema generation for tool parameters
- **anyhow 1** - Error handling

## License

MIT (see `license` in `Cargo.toml`).

## Traceability and toolchain

Requirements, design decisions and the STPA analysis (losses, hazards, system
constraints) live in `artifacts/` and are validated by
[rivet](https://github.com/pulseengine/rivet). The plan for the next release
(v0.4.0, the move to `haus` with Loxone as a second source) is scoped there:

```sh
rivet validate
rivet release status v0.4.0
```

rivet comes from the toolchain layer pinned in `varve.toml` (realm `covalent`,
roots in `varve-realms.toml`); `varve install` fetches and verifies it. CI runs
`rivet validate` the same way.
