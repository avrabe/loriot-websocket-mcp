# haus

`haus` keeps the climate history of a home: temperature, humidity and heating
state from LoRaWAN sensors (through the LORIOT cloud websocket) and, from
v0.4.0, from a Loxone Miniserver. It stores every reading on disk, serves a
REST/SSE API and a web dashboard, and is a single static binary meant to run
under systemd, for example on a Raspberry Pi 4.

> **Status: v0.4.0 in progress.** This repository was `loriot-server` up to
> 0.3.0. Milestone
> [v0.4.0](https://github.com/avrabe/loriot-websocket-mcp/milestone/1); scope
> and readiness in rivet: `rivet release status v0.4.0`.

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
| `HAUS_STORE`      | `haus.redb`                    | the time-series store (all series of all sources) |
| `LOXONE_URL`      | (unset: Loxone off)            | `ws://host[:port]` or `wss://host[:port]` of a Loxone Miniserver |
| `LOXONE_USER`, `LOXONE_PASSWORD` | (required with `LOXONE_URL`) | a Miniserver user; read only from the environment |
| `LOXONE_TOKEN_FILE` | `loxone-token.json`          | where the Miniserver token is kept (written with mode 0600) |
| `LOXONE_MAP`      | (none)                         | JSON `{"<control uuid>/<field>": "<state name or state uuid>"}` for fields other than `Default` |
| `LOXONE_KEY_SHA256` | (trust on first use)         | SHA-256 of the Miniserver's public key; a different key is refused before any login |
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

### The Loxone source

With `LOXONE_URL` set, haus fetches the Miniserver's public key over HTTP(S)
(same host and port), connects to its websocket, exchanges an AES session key, and authenticates with its stored token. It uses the
password only to acquire a token, and token commands are always sent
encrypted. It then loads the structure file and listens for value events. It
sends only an allow-listed set of read commands; nothing that switches or
sets anything in the home.

Over plain `ws://` the session is only as trustworthy as the Miniserver's
public key, so haus pins it. Either set `LOXONE_KEY_SHA256` or, without it,
haus trusts the first key it sees and remembers it in the token file. A
different key later is refused before anything derived from the credentials
is sent. Prefer `wss://` where the Miniserver supports TLS. A Miniserver user
without control rights limits what a stolen token could do.

Events are stored for the Loxone series the store already has, normally the
ones a Stats4Lox import created, keyed by control UUID and field. A field
`Default` maps to the control's `value` state. Other fields, such as a room
controller's outputs, need a `LOXONE_MAP` entry naming the state. At startup
haus logs `Loxone: N of M series mapped to states` and every series it
could not map.

### The dashboard

| Page | |
|------|-|
| `/` | every room with its temperature, humidity and heating values, each with its age; values that no longer describe now are marked stale; refreshes every 30 s |
| `/sensor/<id>?range=1h\|24h\|7d\|90d` | one series: current value, when it was measured, its trend |
| `/status` | source connections, battery voltages, every series' last value, oldest first |

Pages are rendered by the server. The only script is the vendored htmx
2.0.4, and the release builds with cargo alone. A trend line breaks where
there was no data: a LoRaWAN value stands for 30 minutes, a Loxone value
(sent only on change) for 24 hours.

### Importing history

```sh
haus import loriot sensor_history.jsonl [--names devices.json]
haus import stats4lox <export dir>      # manifest.json + <measurement>/<YYYY-MM>.jsonl.gz
```

Each import reads every point back from the store, and a Stats4Lox import
also compares its counts with InfluxDB's own count in the manifest. Any
difference makes it exit 1. Re-running an import stores nothing twice.

### Replacing a Stats4Lox logger

Before switching off a logger such as a LoxBerry with Stats4Lox, let haus
receive from the Miniserver for a while. Then export the logger again and
compare. Do this **before** importing that export:

```sh
haus compare stats4lox <fresh export> --since <when haus went live>
haus import stats4lox <fresh export>     # afterwards: fills the history up to the switch-off
```

`compare` checks every Stats4Lox sample since `--since` against the value
haus had at that time, using only points haus received live (imported
points are kept apart and never count as evidence). It exits 1 on any difference, on any sample haus has
no value for, and on any series haus does not have.

### Health check

`GET /health` answers `200` whenever the process and its runtime are alive,
even while LORIOT is unreachable, so a watchdog does not restart the service
in a loop. The upstream state is part of the body:

```json
{"status":"ok","version":"0.4.0",
 "sources":{"loriot":{"connected":true,"last_message_at":"2026-11-06T21:00:00+00:00","last_message_age_secs":42},
            "loxone":{"connected":true,"last_message_at":"2026-11-06T21:00:30+00:00","last_message_age_secs":12}},
 "upstream":{"connected":true,"last_message_at":"2026-11-06T21:00:00+00:00","last_message_age_secs":42}}
```

`sources` has one entry per configured source. `upstream` repeats
`sources.loriot` for watchdogs written against 0.3.0.

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
