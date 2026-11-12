# haus

`haus` keeps the climate history of a home: temperature, humidity and heating
state from LoRaWAN sensors (through the LORIOT cloud websocket) and, from
v0.4.0, from a Loxone Miniserver. It stores every reading on disk, serves a
REST/SSE API and a web dashboard, and is a single static binary meant to run
under systemd, for example on a Raspberry Pi 4.

> **Status: v0.4.0 in progress.** This repository was `loriot-server` up to
> 0.3.0. The rename is done; the new store, the Loxone source, the imports and
> the server-rendered dashboard are being built (milestone
> [v0.4.0](https://github.com/avrabe/loriot-websocket-mcp/milestone/1), scope in
> rivet: `rivet release status v0.4.0`). Until the dashboard lands, `main`
> serves the API only. Use the 0.3.0 release for a working dashboard.

## What is in the workspace

| Crate              | What it is |
|--------------------|------------|
| `crates/haus`      | the server binary: LORIOT websocket client, storage, REST/SSE API |
| `crates/haus-core` | shared types and logic: sensor decoding, storage, analytics; also the weather (DWD, Open-Meteo), thermal, heat pump and energy models whose pages return after v0.4.0 |

Supported LoRaWAN sensor: Dragino LHT65S-PIR (temperature, humidity, PIR
activity and movement count, battery voltage and status).

## Running

```sh
LORIOT_WS_TOKEN=... BIND=127.0.0.1 PORT=3000 ./haus
```

### Configuration (environment)

| Variable          | Default                        | Meaning |
|-------------------|--------------------------------|---------|
| `LORIOT_WS_TOKEN` | (required)                     | LORIOT application token |
| `BIND`            | `HOST`, else `0.0.0.0`         | listen address, e.g. `127.0.0.1` behind a reverse proxy |
| `PORT`            | `3000`                         | listen port |
| `RUST_LOG`        | `haus=debug,tower_http=info`   | log filter ([`EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) syntax) |

`haus --version` prints `haus <version>`; `--help` lists the variables above
and exits 0; unknown arguments exit 2.

With the default filter every HTTP request is logged once at info when its
response is sent. `/health` is never logged, so a watchdog polling it does not
fill the journal.

### Files in the working directory

| Path                   | Content |
|------------------------|---------|
| `sensor_history.jsonl` | every LoRaWAN reading, one JSON object per line, append-only |
| `devices.json`         | optional EUI → display name map, e.g. `{"0011223344556677": "Kitchen"}` |

The last 2000 readings are kept in memory; requests that need the full history
(data range, analytics) read the JSONL file on a blocking thread, so they
never stall the server.

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

## Releases and verification

Tags `v*` build these assets (`<ver>` without the `v`):

- `haus-<ver>-linux-arm64` (static, aarch64 musl, e.g. Raspberry Pi 4)
- `haus-<ver>-linux-amd64` (static, x86_64 musl)
- `haus-<ver>-macos-arm64`
- `sbom.json` (CycloneDX)
- `SHA256SUMS.txt` over all of the above, and its keyless cosign signature
  `SHA256SUMS.txt.cosign.bundle`

Releases up to 0.3.0 are named `loriot-server-<ver>-*` and also carry
`loriot-web-<ver>-assets.tar.gz`, the Leptos dashboard.

Verify the signature over the checksums, then the files against the checksums:

```sh
cosign verify-blob \
  --certificate-identity-regexp '^https://github.com/avrabe/loriot-websocket-mcp/\.github/workflows/' \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  --bundle SHA256SUMS.txt.cosign.bundle \
  SHA256SUMS.txt
sha256sum --ignore-missing -c SHA256SUMS.txt
```

Each binary also carries a GitHub build-provenance attestation:

```sh
gh attestation verify haus-<ver>-linux-arm64 --repo avrabe/loriot-websocket-mcp
```

## Development

```sh
cargo build
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p haus-core --all-targets --all-features -- -D warnings
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy, the tests (the workspace,
and `haus-core` with all features) and `rivet validate` on every pull request.

## Traceability and toolchain

Requirements, design decisions and the STPA analysis (losses, hazards, system
constraints) live in `artifacts/` and are validated by
[rivet](https://github.com/pulseengine/rivet):

```sh
rivet validate
rivet release status v0.4.0
```

Tests that verify a requirement say so with a `// rivet: verifies REQ-…`
marker.

rivet comes from the toolchain layer pinned in `varve.toml` (realm `covalent`,
roots in `varve-realms.toml`); `varve install` fetches and verifies it. CI runs
`rivet validate` the same way.

## License

MIT (see `license` in `Cargo.toml`).
