//! The Loxone Miniserver source (REQ-LOX-001..003, DD-004, DD-012): a
//! read-only websocket client that stores value events for the series haus
//! knows, keyed by the same control UUID the Stats4Lox import used.
//!
//! Per connection: key exchange (RSA + AES session), token authentication
//! (a stored token, else a new one from the user's password), structure file,
//! state resolution, `enablebinstatusupdate`, then events until the
//! connection drops. Keepalive every 60 s; no frame for 150 s counts as dead;
//! reconnect with exponential backoff. Token commands always go encrypted.
//! Credentials, tokens and hashes are never logged.

use crate::state::{run_blocking, AppState};
use futures::{SinkExt, StreamExt};
use haus_core::import::loxone_quantity;
use haus_core::ingest::Item;
use haus_core::loxone::{
    describe_control, hmac_hex, parse_value_events, password_hash, public_key_sha256,
    resolve_states, Cmd, HashAlg, Header, LoxUuid, MsgKind, Response, Secret, Session,
    LOXONE_EPOCH_UNIX_S,
};
use haus_core::store::{SeriesKey, SeriesMeta, Source};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Token permission: "App", a long-lived token (V17.0, Permissions).
const PERMISSION_APP: u32 = 4;
/// Refresh the token when it has less than this left.
const REFRESH_BEFORE_S: i64 = 24 * 3600;

/// How to reach and log in to the Miniserver. No `Debug`: it holds the
/// password.
pub struct LoxoneConfig {
    /// `ws://host[:port]` or `wss://host[:port]`; `/ws/rfc6455` is appended.
    pub url: String,
    pub user: String,
    pub password: Secret,
    /// Where the token is kept between connections (mode 0600).
    pub token_file: PathBuf,
    /// Optional `{"uuid/field": "stateName or state uuid"}` overrides.
    pub overrides: HashMap<String, String>,
    /// Controls to follow live although no history was imported for them
    /// (LOXONE_TRACK): their series are created from the structure file.
    pub track: Vec<String>,
    /// SHA-256 (hex) of the Miniserver's public key. Without it the first
    /// key seen is trusted and remembered in the token file.
    pub key_pin: Option<String>,
    pub keepalive: Duration,
    pub silence: Duration,
    pub request_timeout: Duration,
    pub backoff_base: Duration,
    pub backoff_max: Duration,
}

impl LoxoneConfig {
    /// From `LOXONE_URL`, `LOXONE_USER`, `LOXONE_PASSWORD`, `LOXONE_TOKEN_FILE`
    /// (default `loxone-token.json`) and `LOXONE_MAP`. `Ok(None)` when
    /// `LOXONE_URL` is unset: the source is off.
    pub fn from_env() -> Result<Option<Self>, String> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let Some(url) = var("LOXONE_URL") else {
            return Ok(None);
        };
        let user = var("LOXONE_USER").ok_or("LOXONE_URL is set but LOXONE_USER is not")?;
        let password =
            var("LOXONE_PASSWORD").ok_or("LOXONE_URL is set but LOXONE_PASSWORD is not")?;
        let overrides = match var("LOXONE_MAP") {
            Some(p) => serde_json::from_str(
                &std::fs::read_to_string(&p).map_err(|e| format!("LOXONE_MAP {p}: {e}"))?,
            )
            .map_err(|e| format!("LOXONE_MAP {p}: {e}"))?,
            None => HashMap::new(),
        };
        Ok(Some(Self {
            url,
            user,
            password: Secret::new(password),
            token_file: var("LOXONE_TOKEN_FILE")
                .unwrap_or_else(|| "loxone-token.json".into())
                .into(),
            overrides,
            key_pin: var("LOXONE_KEY_SHA256").map(|k| normalize_fingerprint(&k)),
            track: var("LOXONE_TRACK")
                .map(|t| {
                    t.split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|u| !u.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            ..Self::defaults()
        }))
    }

    fn defaults() -> Self {
        Self {
            url: String::new(),
            user: String::new(),
            password: Secret::new(""),
            token_file: PathBuf::new(),
            overrides: HashMap::new(),
            key_pin: None,
            track: Vec::new(),
            keepalive: Duration::from_secs(60),
            silence: Duration::from_secs(150),
            request_timeout: Duration::from_secs(20),
            backoff_base: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
        }
    }
}

/// The token kept between connections.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredToken {
    user: String,
    token: String,
    /// Seconds since 2009-01-01 (Loxone epoch).
    valid_until: i64,
    /// Identifies haus to the Miniserver across token requests.
    client_uuid: String,
    /// SHA-256 of the Miniserver public key this token was obtained under.
    #[serde(default)]
    key_sha256: Option<String>,
}

/// `AB:cd:…` or `abcd…` to lowercase hex without separators.
fn normalize_fingerprint(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_lowercase()
}

/// The key the Miniserver must present: the configured pin, else the one
/// remembered from the first connection.
fn expected_key(cfg: &LoxoneConfig) -> Option<String> {
    cfg.key_pin
        .clone()
        .or_else(|| load_token(cfg).and_then(|t| t.key_sha256))
}

fn now_loxone_s() -> i64 {
    chrono::Utc::now().timestamp() - LOXONE_EPOCH_UNIX_S
}

fn load_token(cfg: &LoxoneConfig) -> Option<StoredToken> {
    let t: StoredToken =
        serde_json::from_str(&std::fs::read_to_string(&cfg.token_file).ok()?).ok()?;
    (t.user == cfg.user).then_some(t)
}

fn save_token(cfg: &LoxoneConfig, t: &StoredToken) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = cfg.token_file.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    o.mode(0o600);
    let mut f = o.open(&tmp)?;
    f.write_all(serde_json::to_string(t).expect("serialises").as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, &cfg.token_file)
}

/// A client UUID in Loxone's form, random.
fn new_client_uuid() -> String {
    let b: [u8; 16] = std::array::from_fn(|_| fastrand::u8(..));
    LoxUuid(b).to_string()
}

/// Delay before reconnect attempt `attempt` (1-based), before jitter:
/// base * 2^(attempt-1), capped at the maximum.
fn backoff_delay(cfg: &LoxoneConfig, attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1).min(16);
    cfg.backoff_base
        .saturating_mul(1u32 << exp)
        .min(cfg.backoff_max)
}

/// Run the source forever: connect, read events, reconnect with backoff.
pub async fn run_loxone_connection(state: AppState, cfg: LoxoneConfig) {
    let health = state
        .sources
        .loxone
        .as_ref()
        .expect("loxone source enabled in Sources");
    if cfg.url.starts_with("ws://") && cfg.key_pin.is_none() {
        tracing::warn!(
            "Loxone: plain ws:// without LOXONE_KEY_SHA256: the Miniserver's public key \
             is trusted on first use; prefer wss:// or pin the key"
        );
    }
    let mut attempt: u32 = 0;
    loop {
        attempt = attempt.saturating_add(1);
        match session(&state, &cfg).await {
            Ok(()) => {
                tracing::info!("Loxone connection closed");
                attempt = 0;
            }
            Err(e) => tracing::warn!("Loxone connection failed: {e}"),
        }
        health.set_connected(false);
        let delay = backoff_delay(&cfg, attempt);
        let jitter = Duration::from_millis(fastrand::u64(0..=(delay.as_millis() as u64 / 4)));
        sleep(delay + jitter).await;
    }
}

/// One connection: framing per V17.0 "Understanding the messages".
struct Conn<'a> {
    ws: Ws,
    cfg: &'a LoxoneConfig,
    state: &'a AppState,
    /// A header whose payload has not arrived yet. Kept here, not in the
    /// `next` future, so cancelling `next` (in `select!`) loses nothing.
    pending: Option<Header>,
    /// When the last frame arrived: the silence deadline counts from here,
    /// whatever else the loop does in between.
    last_rx: tokio::time::Instant,
    /// Until status updates are on, every request must finish by then, so a
    /// peer trickling frames cannot hold the setup open forever.
    setup_deadline: Option<tokio::time::Instant>,
}

/// A complete message: its header and the payload that followed it.
enum Msg {
    Text(String),
    Binary(MsgKind, Vec<u8>),
    Keepalive,
}

impl Conn<'_> {
    async fn send(&mut self, cmd: &Cmd, session: Option<&Session>) -> Result<(), BoxError> {
        let text = match session {
            Some(s) if cmd.must_encrypt() => s.encrypt(cmd),
            _ if cmd.must_encrypt() => return Err("token command without a session".into()),
            _ => cmd.text(),
        };
        tracing::debug!("loxone -> {}", cmd.name());
        timeout(self.cfg.request_timeout, self.ws.send(Message::text(text)))
            .await
            .map_err(|_| "send timed out")??;
        Ok(())
    }

    /// The next complete message, or an error after `wait` without a frame.
    /// Cancel-safe: a frame is either fully handled or not taken.
    async fn next(&mut self, wait: Duration) -> Result<Msg, BoxError> {
        loop {
            let frame = timeout(wait, self.ws.next())
                .await
                .map_err(|_| format!("no frame from the Miniserver for {wait:?}"))?
                .ok_or("connection closed")??;
            // Only protocol frames are signs of life: a peer that answers
            // websocket pings but sends nothing else is as good as dead.
            if matches!(frame, Message::Binary(_) | Message::Text(_)) {
                self.last_rx = tokio::time::Instant::now();
                if let Some(h) = self.state.sources.loxone.as_ref() {
                    h.record_message();
                }
            }
            match (frame, self.pending.take()) {
                (Message::Binary(b), None) => {
                    let h = Header::parse(&b)?;
                    match h.kind {
                        MsgKind::Keepalive => return Ok(Msg::Keepalive),
                        MsgKind::OutOfService => return Err("Miniserver out of service".into()),
                        // An estimated header is always followed by an exact one.
                        _ if h.estimated => {}
                        _ => self.pending = Some(h),
                    }
                }
                (Message::Binary(b), Some(h)) => return Ok(Msg::Binary(h.kind, b.to_vec())),
                (Message::Text(t), _) => return Ok(Msg::Text(t.as_str().to_string())),
                (Message::Close(_), _) => return Err("connection closed by the Miniserver".into()),
                (_, pending) => self.pending = pending,
            }
        }
    }

    /// Send `cmd` and return the next text message (before status updates
    /// are enabled, nothing else arrives in between).
    async fn request_text(
        &mut self,
        cmd: &Cmd,
        session: Option<&Session>,
    ) -> Result<String, BoxError> {
        self.send(cmd, session).await?;
        loop {
            let wait = match self.setup_deadline {
                Some(d) => d
                    .checked_duration_since(tokio::time::Instant::now())
                    .ok_or("the Miniserver setup took too long")?
                    .min(self.cfg.request_timeout),
                None => self.cfg.request_timeout,
            };
            match self.next(wait).await? {
                Msg::Text(t) => return Ok(t),
                Msg::Keepalive | Msg::Binary(..) => {}
            }
        }
    }

    async fn request(
        &mut self,
        cmd: &Cmd,
        session: Option<&Session>,
    ) -> Result<Response, BoxError> {
        let r = Response::parse(&self.request_text(cmd, session).await?)?;
        if r.code != 200 {
            return Err(format!("{} answered {}", cmd.name(), r.code).into());
        }
        Ok(r)
    }

    /// key, salt and hashAlg for the user.
    async fn getkey2(&mut self) -> Result<(String, String, HashAlg), BoxError> {
        let r = self
            .request(
                &Cmd::GetKey2 {
                    user: self.cfg.user.clone(),
                },
                None,
            )
            .await?;
        let v = &r.value;
        let field = |k: &str| {
            v[k].as_str()
                .map(str::to_string)
                .ok_or(format!("getkey2: no {k}"))
        };
        Ok((
            field("key")?,
            field("salt")?,
            HashAlg::parse(&field("hashAlg")?)?,
        ))
    }

    /// Authenticate the socket: the stored token if it works, else a new one.
    async fn authenticate(
        &mut self,
        session: &Session,
        key_sha256: &str,
    ) -> Result<StoredToken, BoxError> {
        if let Some(t) = load_token(self.cfg).filter(|t| t.valid_until > now_loxone_s() + 3600) {
            let (key, _, alg) = self.getkey2().await?;
            let hash = Secret::new(hmac_hex(alg, &key, &t.token)?);
            let cmd = Cmd::AuthWithToken {
                hash,
                user: self.cfg.user.clone(),
            };
            match self.request(&cmd, Some(session)).await {
                Ok(_) => {
                    tracing::info!("Loxone: authenticated with the stored token");
                    let mut t = t;
                    if t.key_sha256.is_none() {
                        t.key_sha256 = Some(key_sha256.to_string());
                        save_token(self.cfg, &t)?;
                    }
                    return Ok(t);
                }
                Err(e) => {
                    tracing::warn!("Loxone: stored token rejected ({e}); requesting a new one")
                }
            }
        }
        let (key, salt, alg) = self.getkey2().await?;
        let pw = password_hash(alg, self.cfg.password.expose(), &salt);
        let hash = Secret::new(hmac_hex(alg, &key, &format!("{}:{pw}", self.cfg.user))?);
        let client_uuid = load_token(self.cfg)
            .map(|t| t.client_uuid)
            .unwrap_or_else(new_client_uuid);
        let r = self
            .request(
                &Cmd::GetJwt {
                    hash,
                    user: self.cfg.user.clone(),
                    permission: PERMISSION_APP,
                    client_uuid: client_uuid.clone(),
                    info: "haus".into(),
                },
                Some(session),
            )
            .await?;
        let t = StoredToken {
            user: self.cfg.user.clone(),
            token: r.value["token"]
                .as_str()
                .ok_or("getjwt: no token")?
                .to_string(),
            valid_until: r.value["validUntil"]
                .as_i64()
                .ok_or("getjwt: no validUntil")?,
            client_uuid,
            key_sha256: Some(key_sha256.to_string()),
        };
        save_token(self.cfg, &t)?;
        tracing::info!("Loxone: new token acquired");
        Ok(t)
    }

    async fn refresh(&mut self, session: &Session, t: &mut StoredToken) -> Result<(), BoxError> {
        let (key, _, alg) = self.getkey2().await?;
        let hash = Secret::new(hmac_hex(alg, &key, &t.token)?);
        let r = self
            .request(
                &Cmd::RefreshJwt {
                    hash,
                    user: self.cfg.user.clone(),
                },
                Some(session),
            )
            .await?;
        if let Some(tok) = r.value["token"].as_str() {
            t.token = tok.to_string();
        }
        t.valid_until = r.value["validUntil"]
            .as_i64()
            .ok_or("refreshjwt: no validUntil")?;
        save_token(self.cfg, t)?;
        tracing::info!("Loxone: token refreshed");
        Ok(())
    }
}

/// The Loxone series the store knows: `(control uuid, field)` → metadata.
fn wanted_series(state: &AppState) -> Result<HashMap<(String, String), SeriesMeta>, BoxError> {
    Ok(state
        .ingest
        .store()
        .series()?
        .into_iter()
        .filter(|(_, m)| m.key.source == Source::Loxone)
        .map(|(_, m)| ((m.key.source_id.clone(), m.key.field.clone()), m))
        .collect())
}

/// When most series are missing because their controls are not in the
/// structure file, and the file has fewer controls than haus has Loxone
/// sensors, the likely cause is the user's rights: the Miniserver shows a
/// user only the controls it may see. One line saying so beats a list of
/// "not in the structure file".
fn rights_hint(
    structure: &serde_json::Value,
    wanted: &[(String, String)],
    unresolved: &[String],
    user: &str,
) -> Option<String> {
    let missing = unresolved
        .iter()
        .filter(|u| u.contains("not in the structure file"))
        .count();
    let sensors: std::collections::HashSet<&str> = wanted.iter().map(|(u, _)| u.as_str()).collect();
    let controls = structure["controls"].as_object().map_or(0, |c| c.len());
    (missing > 0 && missing * 2 >= wanted.len() && controls < sensors.len()).then(|| {
        format!(
            "Loxone: the structure file for user {user:?} lists only {controls} control(s), \
             but haus knows {} Loxone sensor(s). The Miniserver shows a user only the controls \
             it may see: give {user:?} visualisation rights on these controls in Loxone Config, \
             or use another user.",
            sensors.len()
        )
    })
}

/// The HTTP(S) base of the Miniserver behind a `ws://` / `wss://` URL.
fn http_base(ws_url: &str) -> Result<String, BoxError> {
    let u = ws_url.trim_end_matches('/');
    if let Some(rest) = u.strip_prefix("wss://") {
        Ok(format!("https://{rest}"))
    } else if let Some(rest) = u.strip_prefix("ws://") {
        Ok(format!("http://{rest}"))
    } else {
        Err(format!("LOXONE_URL must start with ws:// or wss://, got {u:?}").into())
    }
}

/// The Miniserver's public key, over HTTP(S). The websocket does not serve
/// `jdev/sys/getPublicKey` (a real Miniserver, 17.2, answers 400 there); V17.0
/// gets the key before the websocket is opened.
async fn fetch_public_key(cfg: &LoxoneConfig) -> Result<String, BoxError> {
    let url = format!("{}/{}", http_base(&cfg.url)?, Cmd::GetPublicKey.text());
    tracing::debug!("loxone -> {} (http)", Cmd::GetPublicKey.name());
    let client = reqwest::Client::builder()
        .timeout(cfg.request_timeout)
        .build()?;
    let body = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let r = Response::parse(&body)?;
    if r.code != 200 {
        return Err(format!("getPublicKey answered {}", r.code).into());
    }
    Ok(r.value.as_str().ok_or("getPublicKey: no key")?.to_string())
}

async fn session(state: &AppState, cfg: &LoxoneConfig) -> Result<(), BoxError> {
    // The public key first, over HTTP, and refuse a Miniserver whose key is
    // not the pinned or remembered one before anything derived from the
    // credentials is sent (a host on the path could otherwise read the
    // session and relay or attack the login).
    let pem = fetch_public_key(cfg).await?;
    let key_sha256 = public_key_sha256(&pem)?;
    if let Some(want) = expected_key(cfg) {
        if want != key_sha256 {
            return Err(format!(
                "the Miniserver presented public key sha256 {key_sha256}, expected {want}; \
                 refusing to authenticate (set LOXONE_KEY_SHA256 or remove the token file \
                 if the Miniserver's key really changed)"
            )
            .into());
        }
    }

    let url = format!("{}/ws/rfc6455", cfg.url.trim_end_matches('/'));
    let mut req = url.as_str().into_client_request()?;
    req.headers_mut()
        .insert("Sec-WebSocket-Protocol", "remotecontrol".parse()?);
    let (ws, _) = timeout(cfg.request_timeout, connect_async(req))
        .await
        .map_err(|_| "connect timed out")??;
    let mut c = Conn {
        ws,
        cfg,
        state,
        pending: None,
        last_rx: tokio::time::Instant::now(),
        setup_deadline: Some(tokio::time::Instant::now() + cfg.request_timeout * 6),
    };

    // Key exchange, then authentication.
    let sess = Session::random();
    c.request(&Cmd::KeyExchange(sess.encrypted_key(&pem)?), None)
        .await?;
    let mut token = c.authenticate(&sess, &key_sha256).await?;
    if token.valid_until - now_loxone_s() < REFRESH_BEFORE_S {
        c.refresh(&sess, &mut token).await?;
    }

    // Which state UUIDs feed which series.
    let structure: serde_json::Value =
        serde_json::from_str(&c.request_text(&Cmd::StructureFile, None).await?)?;
    // Tracked controls without imported history get their series now.
    let mut tracked = Vec::new();
    for uuid in &cfg.track {
        match describe_control(&structure, uuid) {
            Some(info) if info.value_state.is_some() => {
                let (quantity, unit) = loxone_quantity(&info.category, &info.name);
                // The quantity's unit, else what the Miniserver displays (lx).
                let unit = if unit.is_empty() {
                    info.unit.clone().unwrap_or_default()
                } else {
                    unit.to_string()
                };
                tracked.push(SeriesMeta {
                    key: SeriesKey::new(Source::Loxone, uuid.to_lowercase(), "Default"),
                    quantity,
                    unit,
                    name: info.name,
                    room: info.room,
                });
            }
            Some(_) => tracing::warn!("Loxone: tracked control {uuid} has no value state"),
            None => tracing::warn!("Loxone: tracked control {uuid} is not in the structure file"),
        }
    }
    let wanted_meta = {
        let s = state.clone();
        run_blocking(move || {
            for meta in &tracked {
                // Keep the metadata of a series that exists already.
                if s.ingest.store().series_id(&meta.key)?.is_none() {
                    s.ingest.store().ensure_series(meta)?;
                }
            }
            wanted_series(&s)
        })
        .await
        .map_err(|e| e.to_string())?
    };
    let wanted: Vec<(String, String)> = wanted_meta.keys().cloned().collect();
    let res = resolve_states(&structure, &wanted, &cfg.overrides);
    tracing::info!(
        "Loxone: {} of {} series mapped to states, {} retired",
        res.states.values().map(Vec::len).sum::<usize>(),
        wanted.len(),
        res.retired.len()
    );
    for u in &res.unresolved {
        tracing::warn!("Loxone: unmapped series {u}");
    }
    if let Some(hint) = rights_hint(&structure, &wanted, &res.unresolved, &cfg.user) {
        tracing::warn!("{hint}");
    }

    c.request(&Cmd::EnableStatusUpdates, None).await?;
    c.setup_deadline = None;
    if let Some(h) = state.sources.loxone.as_ref() {
        h.set_connected(true);
    }

    let mut keepalive = tokio::time::interval(cfg.keepalive);
    keepalive.tick().await;
    let mut refresh_check = tokio::time::interval(Duration::from_secs(3600));
    refresh_check.tick().await;
    loop {
        let silent_at = c.last_rx + cfg.silence;
        tokio::select! {
            _ = tokio::time::sleep_until(silent_at) => {
                return Err(format!("no frame from the Miniserver for {:?}", cfg.silence).into());
            }
            msg = c.next(cfg.silence) => match msg? {
                Msg::Binary(MsgKind::ValueEvents, b) => {
                    let ts_ms = chrono::Utc::now().timestamp_millis();
                    let items: Vec<Item> = parse_value_events(&b)?
                        .into_iter()
                        .flat_map(|(u, value)| {
                            res.states
                                .get(&u)
                                .into_iter()
                                .flatten()
                                .filter_map(|key| wanted_meta.get(key))
                                .map(move |meta| Item { meta: meta.clone(), ts_ms, value })
                                .collect::<Vec<_>>()
                        })
                        .collect();
                    if !items.is_empty() {
                        let ingest = state.ingest.clone();
                        // A wedged store must not stop keepalives and silence
                        // detection for long; the time spent storing is not
                        // silence from the Miniserver.
                        timeout(cfg.silence, run_blocking(move || ingest.ingest(&items)))
                            .await
                            .map_err(|_| "storing Loxone events timed out")??;
                        c.last_rx = tokio::time::Instant::now();
                    }
                }
                Msg::Text(t) => {
                    if let Ok(r) = Response::parse(&t) {
                        if r.code != 200 {
                            tracing::warn!("Loxone: {} answered {}", r.control.split('/').take(3).collect::<Vec<_>>().join("/"), r.code);
                        }
                    }
                }
                Msg::Keepalive | Msg::Binary(..) => {}
            },
            _ = keepalive.tick() => c.send(&Cmd::Keepalive, None).await?,
            _ = refresh_check.tick() => {
                // Refreshing needs request/response, which would interleave
                // with events; reconnect instead, the setup refreshes it.
                if token.valid_until - now_loxone_s() < REFRESH_BEFORE_S {
                    tracing::info!("Loxone: token expires soon; reconnecting to refresh it");
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! The client against an in-process fake Miniserver that implements the
    //! server side of the V17.0 protocol: RSA key exchange, AES `enc`
    //! commands, getkey2/getjwt/authwithtoken hashing, structure file, value
    //! event tables, keepalive.
    use super::*;
    use crate::state::{tests::test_state, Sources, UpstreamHealth};
    use base64::Engine as _;
    use haus_core::loxone::{encode_value_events, ALLOWED_COMMANDS};
    use haus_core::store::{Quantity, SeriesKey};
    use haus_core::AppConfig;
    use rsa::pkcs8::EncodePublicKey;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response as WsResponse};

    const USER: &str = "haus";
    const PASSWORD: &str = "correct horse battery staple";
    const KEY_HEX: &str = "4b6579"; // getkey2 key ("Key")
    const SALT: &str = "5a17";
    const CTRL: &str = "13f00a65-01a9-8a08-ffff196db5c1dff3";
    const CTRL_STATE: &str = "13f00a65-01a9-8a08-ffff196db5c1dff4";
    const ROOM: &str = "1d000000-0000-0001-ffff000000000001";
    const ROOM_TARGET: &str = "1d000000-0000-0001-ffff000000000003";
    const UNKNOWN: &str = "0e000000-0000-0000-0000000000000099";
    /// A sensor with no imported history, followed through LOXONE_TRACK.
    const TRACKED: &str = "1e000000-0000-0002-ffff000000000001";
    const TRACKED_STATE: &str = "1e000000-0000-0002-ffff000000000002";
    /// A brightness sensor: no climate category, unit only in its format.
    const LUX: &str = "1e000000-0000-0003-ffff000000000001";

    #[derive(Clone, Copy, PartialEq)]
    enum After {
        /// Keep answering keepalives.
        Stay,
        /// Close the connection after the events.
        Drop,
        /// Stop answering anything (a wedged peer).
        Silent,
        /// Answer only with websocket pings: alive at the transport, dead in
        /// the protocol.
        PingOnly,
    }

    struct Fake {
        /// How long issued tokens are valid, in seconds.
        token_valid_s: Mutex<i64>,
        rsa: rsa::RsaPrivateKey,
        pem: String,
        token: Mutex<String>,
        after: After,
        /// Every command received, decrypted, with "enc:" for encrypted ones.
        received: Mutex<Vec<String>>,
        accepts: Mutex<usize>,
    }

    impl Fake {
        async fn new(after: After) -> Arc<Self> {
            let rsa = tokio::task::spawn_blocking(|| {
                rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap()
            })
            .await
            .unwrap();
            // The Miniserver labels its public key as a certificate, one line.
            let pem = rsa
                .to_public_key()
                .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
                .unwrap()
                .replace("-----BEGIN PUBLIC KEY-----", "-----BEGIN CERTIFICATE-----")
                .replace("-----END PUBLIC KEY-----", "-----END CERTIFICATE-----")
                .replace('\n', "");
            Arc::new(Self {
                token_valid_s: Mutex::new(30 * 86_400),
                rsa,
                pem,
                token: Mutex::new("tok-1".into()),
                after,
                received: Mutex::new(Vec::new()),
                accepts: Mutex::new(0),
            })
        }

        fn received(&self) -> Vec<String> {
            self.received.lock().unwrap().clone()
        }
    }

    fn structure() -> serde_json::Value {
        json!({"lastModified": "2026-11-12 10:00:00",
            "rooms": {"r1": {"name": "Room A"}},
            "cats": {"c1": {"name": "Luftfeuchte"}, "c2": {"name": "Einstellungen"}},
            "controls": {
            TRACKED: {"type": "InfoOnlyAnalog", "uuidAction": TRACKED, "name": "Air sensor",
                      "room": "r1", "cat": "c1", "states": {"value": TRACKED_STATE}},
            LUX: {"type": "InfoOnlyAnalog", "uuidAction": LUX, "name": "Brightness",
                  "room": "r1", "cat": "c2", "details": {"format": "%.0fLx"},
                  "states": {"value": LUX}},
            CTRL: {"type": "InfoOnlyAnalog", "uuidAction": CTRL, "states": {"value": CTRL_STATE}},
            ROOM: {"type": "IRoomControllerV2", "uuidAction": ROOM,
                   "states": {"tempActual": "1d000000-0000-0001-ffff000000000002",
                              "tempTarget": ROOM_TARGET}}
        }})
    }

    type ServerWs = WebSocketStream<TcpStream>;

    async fn send_text(ws: &mut ServerWs, text: String) {
        ws.send(Message::Binary(
            Header::encode(0, text.len() as u32).to_vec().into(),
        ))
        .await
        .unwrap();
        ws.send(Message::text(text)).await.unwrap();
    }

    async fn reply(ws: &mut ServerWs, control: &str, code: u16, value: serde_json::Value) {
        let body = json!({"LL": {"control": control, "value": value, "Code": code.to_string()}});
        send_text(ws, body.to_string()).await;
    }

    async fn serve(fake: Arc<Fake>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                *fake.accepts.lock().unwrap() += 1;
                let fake = fake.clone();
                tokio::spawn(async move {
                    // Same port, like the Miniserver: a websocket upgrade, or
                    // a plain HTTP request (the public key).
                    let mut head = [0u8; 2048];
                    let n = tcp.peek(&mut head).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&head[..n]).to_lowercase();
                    if !head.contains("upgrade: websocket") {
                        http(&fake, tcp).await;
                        return;
                    }
                    // The Err type is fixed by tungstenite's callback signature.
                    #[allow(clippy::result_large_err)]
                    let cb = |req: &Request, mut resp: WsResponse| {
                        assert_eq!(req.uri().path(), "/ws/rfc6455");
                        assert_eq!(
                            req.headers().get("Sec-WebSocket-Protocol").unwrap(),
                            "remotecontrol"
                        );
                        resp.headers_mut()
                            .insert("Sec-WebSocket-Protocol", "remotecontrol".parse().unwrap());
                        Ok(resp)
                    };
                    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, cb).await else {
                        return;
                    };
                    connection(&fake, &mut ws).await;
                });
            }
        });
        format!("ws://{addr}")
    }

    /// One plain HTTP request: GET /jdev/sys/getPublicKey, else 404.
    async fn http(fake: &Fake, mut tcp: TcpStream) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = vec![0u8; 4096];
        let n = tcp.read(&mut buf).await.unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
        fake.received
            .lock()
            .unwrap()
            .push(format!("http:{}", path.trim_start_matches('/')));
        let (status, body) = if path == "/jdev/sys/getPublicKey" {
            (
                "200 OK",
                json!({"LL": {"control": "dev/sys/getPublicKey", "value": fake.pem, "Code": "200"}})
                    .to_string(),
            )
        } else {
            ("404 Not Found", String::new())
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = tcp.write_all(resp.as_bytes()).await;
        let _ = tcp.shutdown().await;
    }

    async fn connection(fake: &Fake, ws: &mut ServerWs) {
        let mut session: Option<([u8; 32], [u8; 16])> = None;
        let mut silent = false;
        let mut ping_only = false;
        while let Some(Ok(msg)) = ws.next().await {
            let Message::Text(t) = msg else { continue };
            let raw = t.as_str().to_string();
            let (cmd, encrypted) = if raw.starts_with("jdev/sys/enc/") {
                let (k, iv) = session.expect("enc before keyexchange");
                let plain = Session::decrypt_with(&k, &iv, &raw).unwrap();
                let rest = plain.strip_prefix("salt/").unwrap();
                (rest.split_once('/').unwrap().1.to_string(), true)
            } else {
                (raw.clone(), false)
            };
            fake.received
                .lock()
                .unwrap()
                .push(format!("{}{cmd}", if encrypted { "enc:" } else { "" }));
            if silent {
                continue;
            }
            if ping_only {
                ws.send(Message::Ping(vec![1u8].into())).await.unwrap();
                continue;
            }
            // The base64 key may contain '/': the argument is the whole rest.
            let parts: Vec<&str> = match cmd.strip_prefix("jdev/sys/keyexchange/") {
                Some(rest) => vec!["jdev", "sys", "keyexchange", rest],
                None => cmd.split('/').collect(),
            };
            let expect_hash = |msg: &str| hmac_hex(HashAlg::Sha256, KEY_HEX, msg).unwrap();
            match parts.as_slice() {
                // A real Miniserver (17.2) does not serve the key here.
                ["jdev", "sys", "getPublicKey"] => reply(ws, &cmd, 400, json!("")).await,
                ["jdev", "sys", "keyexchange", b64] => {
                    let c = base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .unwrap();
                    let plain = fake.rsa.decrypt(rsa::Pkcs1v15Encrypt, &c).unwrap();
                    let plain = String::from_utf8(plain).unwrap();
                    let (k, iv) = plain.split_once(':').unwrap();
                    session = Some((
                        hex::decode(k).unwrap().try_into().unwrap(),
                        hex::decode(iv).unwrap().try_into().unwrap(),
                    ));
                    reply(ws, "jdev/sys/keyexchange", 200, json!("")).await
                }
                ["jdev", "sys", "getkey2", user] if *user == USER => {
                    reply(
                        ws,
                        &cmd,
                        200,
                        json!({"key": KEY_HEX, "salt": SALT, "hashAlg": "SHA256"}),
                    )
                    .await
                }
                ["jdev", "sys", "getjwt", hash, user, "4", _uuid, "haus"] if encrypted => {
                    let pw = password_hash(HashAlg::Sha256, PASSWORD, SALT);
                    if *user == USER && *hash == expect_hash(&format!("{USER}:{pw}")) {
                        let tok = fake.token.lock().unwrap().clone();
                        reply(
                            ws,
                            "getjwt",
                            200,
                            json!({"token": tok,
                            "validUntil": now_loxone_s() + *fake.token_valid_s.lock().unwrap(), "tokenRights": 4}),
                        )
                        .await
                    } else {
                        reply(ws, "getjwt", 401, json!("")).await
                    }
                }
                ["authwithtoken", hash, user] if encrypted => {
                    let tok = fake.token.lock().unwrap().clone();
                    let ok = *user == USER && *hash == expect_hash(&tok);
                    reply(ws, "authwithtoken", if ok { 200 } else { 401 }, json!("")).await
                }
                ["jdev", "sys", "refreshjwt", hash, user] if encrypted => {
                    let old = fake.token.lock().unwrap().clone();
                    if *user == USER && *hash == expect_hash(&old) {
                        *fake.token.lock().unwrap() = "tok-2".into();
                        reply(
                            ws,
                            "refreshjwt",
                            200,
                            json!({"token": "tok-2",
                            "validUntil": now_loxone_s() + 30 * 86_400}),
                        )
                        .await
                    } else {
                        reply(ws, "refreshjwt", 401, json!("")).await
                    }
                }
                ["data", "LoxAPP3.json"] => send_text(ws, structure().to_string()).await,
                ["jdev", "sps", "enablebinstatusupdate"] => {
                    reply(ws, &cmd, 200, json!("1")).await;
                    let ev = encode_value_events(&[
                        (LoxUuid::parse(CTRL_STATE).unwrap(), 21.5),
                        (LoxUuid::parse(ROOM_TARGET).unwrap(), 19.0),
                        (LoxUuid::parse(UNKNOWN).unwrap(), 5.0),
                        (LoxUuid::parse(TRACKED_STATE).unwrap(), 55.0),
                    ]);
                    ws.send(Message::Binary(
                        Header::encode(2, ev.len() as u32).to_vec().into(),
                    ))
                    .await
                    .unwrap();
                    ws.send(Message::Binary(ev.into())).await.unwrap();
                    match fake.after {
                        After::Drop => return,
                        After::Silent => silent = true,
                        After::PingOnly => ping_only = true,
                        After::Stay => {}
                    }
                }
                ["keepalive"] => {
                    ws.send(Message::Binary(Header::encode(6, 0).to_vec().into()))
                        .await
                        .unwrap();
                }
                _ => reply(ws, &cmd, 400, json!("")).await,
            }
        }
    }

    /// A state with the Loxone source on and two imported Loxone series.
    fn lox_state(name: &str) -> AppState {
        let mut st = test_state(name, AppConfig::default());
        st.sources = Arc::new(Sources {
            loriot: UpstreamHealth::default(),
            loxone: Some(UpstreamHealth::default()),
        });
        let store = st.ingest.store();
        for (id, field, q) in [
            (CTRL, "Default", Quantity::Temperature),
            (ROOM, "HCm", Quantity::Heating),
        ] {
            store
                .ensure_series(&SeriesMeta {
                    key: SeriesKey::new(Source::Loxone, id, field),
                    quantity: q,
                    unit: String::new(),
                    name: format!("{id} {field}"),
                    room: None,
                })
                .unwrap();
        }
        st
    }

    fn config(url: String, password: &str, token_file: PathBuf) -> LoxoneConfig {
        LoxoneConfig {
            url,
            user: USER.into(),
            password: Secret::new(password),
            token_file,
            overrides: HashMap::from([(format!("{ROOM}/HCm"), "tempTarget".to_string())]),
            key_pin: None,
            track: Vec::new(),
            keepalive: Duration::from_millis(100),
            silence: Duration::from_millis(400),
            request_timeout: Duration::from_secs(5),
            backoff_base: Duration::from_millis(50),
            backoff_max: Duration::from_millis(200),
        }
    }

    fn tmp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "haus-lox-{name}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn points(st: &AppState, id: &str, field: &str) -> Vec<f64> {
        let store = st.ingest.store();
        let sid = store
            .series_id(&SeriesKey::new(Source::Loxone, id, field))
            .unwrap()
            .unwrap();
        store
            .range(sid, i64::MIN, i64::MAX)
            .unwrap()
            .into_iter()
            .map(|(_, v)| v)
            .collect()
    }

    async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
        let t0 = std::time::Instant::now();
        while !f() {
            assert!(
                t0.elapsed() < Duration::from_secs(15),
                "timed out waiting for {what}"
            );
            sleep(Duration::from_millis(20)).await;
        }
    }

    /// Collects formatted log output.
    #[derive(Clone, Default)]
    struct LogBuf(Arc<Mutex<Vec<u8>>>);
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

    /// Every log line of this test binary, from a process-wide subscriber.
    /// A thread-local one races with parallel tests: their threads cache
    /// callsites as disabled and the capture comes back empty.
    fn global_logs() -> LogBuf {
        static LOGS: std::sync::OnceLock<LogBuf> = std::sync::OnceLock::new();
        LOGS.get_or_init(|| {
            let buf = LogBuf::default();
            let _ = tracing::subscriber::set_global_default(
                tracing_subscriber::fmt()
                    .with_writer(buf.clone())
                    .with_ansi(false)
                    .with_max_level(tracing::Level::TRACE)
                    .finish(),
            );
            buf
        })
        .clone()
    }

    // rivet: verifies REQ-LOX-001
    // rivet: verifies REQ-LOX-002
    // rivet: verifies DD-004
    #[tokio::test(flavor = "current_thread")]
    async fn a_session_stores_events_of_known_series_only() {
        let _serial = crate::state::tests::serial().await;
        let logs = global_logs();
        let fake = Fake::new(After::Stay).await;
        let url = serve(fake.clone()).await;
        let st = lox_state("lox-session");
        let token_file = tmp_file("session");
        let task = tokio::spawn(run_loxone_connection(
            st.clone(),
            config(url, PASSWORD, token_file.clone()),
        ));

        wait_for("both series", || {
            points(&st, CTRL, "Default") == vec![21.5] && points(&st, ROOM, "HCm") == vec![19.0]
        })
        .await;
        assert!(st.sources.loxone.as_ref().unwrap().connected());
        // The unknown state created no series.
        assert_eq!(st.ingest.store().series().unwrap().len(), 2);
        // Keepalives flow.
        wait_for("a keepalive", || {
            fake.received().iter().any(|c| c == "keepalive")
        })
        .await;
        task.abort();

        let received = fake.received();
        // Only allow-listed commands; token commands only encrypted.
        // The public key comes over HTTP, everything else over the websocket.
        assert!(received.iter().any(|c| c == "http:jdev/sys/getPublicKey"));
        assert!(!received.iter().any(|c| c == "jdev/sys/getPublicKey"));
        for c in received.iter().filter(|c| !c.starts_with("http:")) {
            let plain = c.strip_prefix("enc:").unwrap_or(c);
            assert!(
                ALLOWED_COMMANDS.iter().any(|a| plain.starts_with(a)),
                "not allow-listed: {plain}"
            );
            assert!(
                !plain.contains("sps/io"),
                "state-changing command sent: {plain}"
            );
            if plain.starts_with("jdev/sys/getjwt")
                || plain.starts_with("authwithtoken")
                || plain.starts_with("jdev/sys/refreshjwt")
            {
                assert!(c.starts_with("enc:"), "sent in the clear: {plain}");
            }
        }
        assert!(received
            .iter()
            .any(|c| c.starts_with("enc:jdev/sys/getjwt/")));

        // The token is kept, readable by the owner only.
        let saved: StoredToken =
            serde_json::from_str(&std::fs::read_to_string(&token_file).unwrap()).unwrap();
        assert_eq!(saved.token, "tok-1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&token_file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // No secret in the log.
        let log = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("2 of 2 series mapped"), "{log}");
        for secret in [
            PASSWORD,
            "tok-1",
            &password_hash(HashAlg::Sha256, PASSWORD, SALT),
        ] {
            assert!(!log.contains(secret), "secret in the log: {secret}\n{log}");
        }
        let _ = std::fs::remove_file(&token_file);
    }

    // rivet: verifies REQ-LOX-003
    #[tokio::test(flavor = "current_thread")]
    async fn reconnects_after_a_drop_or_silence_and_reuses_the_token() {
        let _serial = crate::state::tests::serial().await;
        for after in [After::Drop, After::Silent, After::PingOnly] {
            let fake = Fake::new(after).await;
            let url = serve(fake.clone()).await;
            let st = lox_state("lox-reconnect");
            let token_file = tmp_file("reconnect");
            let task = tokio::spawn(run_loxone_connection(
                st.clone(),
                config(url, PASSWORD, token_file.clone()),
            ));
            wait_for("a reconnect", || *fake.accepts.lock().unwrap() >= 2).await;
            wait_for("authentication with the stored token", || {
                fake.received()
                    .iter()
                    .any(|c| c.starts_with("enc:authwithtoken/"))
            })
            .await;
            task.abort();
            // One password login, then the token.
            let jwt = fake
                .received()
                .iter()
                .filter(|c| c.starts_with("enc:jdev/sys/getjwt/"))
                .count();
            assert_eq!(jwt, 1);
            let _ = std::fs::remove_file(&token_file);
        }
    }

    // rivet: verifies REQ-LOX-002
    #[tokio::test(flavor = "current_thread")]
    async fn a_different_miniserver_key_is_refused_before_any_login() {
        let _serial = crate::state::tests::serial().await;
        // First contact with A: its key is remembered with the token.
        let a = Fake::new(After::Stay).await;
        let url_a = serve(a.clone()).await;
        let st = lox_state("lox-pin");
        let token_file = tmp_file("pin");
        let task = tokio::spawn(run_loxone_connection(
            st.clone(),
            config(url_a, PASSWORD, token_file.clone()),
        ));
        wait_for("a token", || token_file.exists()).await;
        task.abort();
        let saved: StoredToken =
            serde_json::from_str(&std::fs::read_to_string(&token_file).unwrap()).unwrap();
        assert_eq!(
            saved.key_sha256.unwrap(),
            public_key_sha256(&a.pem).unwrap()
        );

        // B (another key, e.g. a host on the path) at the address: refused.
        let b = Fake::new(After::Stay).await;
        let url_b = serve(b.clone()).await;
        let st = lox_state("lox-pin-b");
        let task = tokio::spawn(run_loxone_connection(
            st.clone(),
            config(url_b.clone(), PASSWORD, token_file.clone()),
        ));
        wait_for("two attempts at B", || *b.accepts.lock().unwrap() >= 2).await;
        task.abort();
        assert!(
            b.received()
                .iter()
                .all(|c| c == "http:jdev/sys/getPublicKey"),
            "sent more than getPublicKey to an unknown key: {:?}",
            b.received()
        );
        assert!(!st.sources.loxone.as_ref().unwrap().connected());

        // A configured pin wins over trust on first use.
        let fresh = tmp_file("pin-fresh");
        let mut cfg = config(url_b, PASSWORD, fresh.clone());
        cfg.key_pin = Some(normalize_fingerprint(&public_key_sha256(&a.pem).unwrap()));
        let before = b.received().len();
        let task = tokio::spawn(run_loxone_connection(lox_state("lox-pin-c"), cfg));
        wait_for("an attempt", || b.received().len() > before).await;
        sleep(Duration::from_millis(200)).await;
        task.abort();
        assert!(b
            .received()
            .iter()
            .all(|c| c == "http:jdev/sys/getPublicKey"));
        assert!(!fresh.exists());
        let _ = std::fs::remove_file(&token_file);
    }

    // rivet: verifies REQ-LOX-003
    #[tokio::test(flavor = "current_thread")]
    async fn a_token_close_to_expiry_is_refreshed() {
        let _serial = crate::state::tests::serial().await;
        let fake = Fake::new(After::Stay).await;
        // The Miniserver hands out a token valid for 2 h: under a day.
        *fake.token_valid_s.lock().unwrap() = 2 * 3600;
        let url = serve(fake.clone()).await;
        let st = lox_state("lox-refresh");
        let token_file = tmp_file("refresh");
        let task = tokio::spawn(run_loxone_connection(
            st.clone(),
            config(url, PASSWORD, token_file.clone()),
        ));
        wait_for("events after a refresh", || {
            points(&st, CTRL, "Default") == vec![21.5]
        })
        .await;
        task.abort();
        assert!(fake
            .received()
            .iter()
            .any(|c| c.starts_with("enc:jdev/sys/refreshjwt/")));
        let saved: StoredToken =
            serde_json::from_str(&std::fs::read_to_string(&token_file).unwrap()).unwrap();
        assert_eq!(saved.token, "tok-2");
        assert!(saved.valid_until > now_loxone_s() + 29 * 86_400);
        let _ = std::fs::remove_file(&token_file);
    }

    // rivet: verifies REQ-LOX-003
    #[test]
    fn backoff_grows_and_is_capped() {
        let c = LoxoneConfig::defaults();
        assert_eq!(backoff_delay(&c, 1), Duration::from_secs(1));
        assert_eq!(backoff_delay(&c, 2), Duration::from_secs(2));
        assert_eq!(backoff_delay(&c, 5), Duration::from_secs(16));
        assert_eq!(backoff_delay(&c, 10), Duration::from_secs(300));
        assert_eq!(backoff_delay(&c, u32::MAX), Duration::from_secs(300));
        assert_eq!(c.keepalive, Duration::from_secs(60));
        assert!(c.silence > c.keepalive * 2 && c.silence < Duration::from_secs(300));
    }

    #[test]
    fn a_rights_filtered_structure_file_is_named_as_such() {
        let wanted: Vec<(String, String)> = (0..5)
            .map(|i| {
                (
                    format!("0e000000-0000-0000-00000000000000{i:02}"),
                    "Default".into(),
                )
            })
            .collect();
        let only_one = json!({"controls": {"x": {"uuidAction": "x", "states": {"input": "y"}}}});
        let res = resolve_states(&only_one, &wanted, &HashMap::new());
        assert_eq!(res.states.len(), 0);
        let hint = rights_hint(&only_one, &wanted, &res.unresolved, "haus").expect("a hint");
        assert!(
            hint.contains("only 1 control") && hint.contains("visualisation rights"),
            "{hint}"
        );
        // A full structure with one stray series: no hint.
        let full = structure();
        let mixed: Vec<(String, String)> = vec![
            (CTRL.into(), "Default".into()),
            (ROOM.into(), "Default".into()),
            (UNKNOWN.into(), "Default".into()),
        ];
        let res = resolve_states(&full, &mixed, &HashMap::new());
        assert!(rights_hint(&full, &mixed, &res.unresolved, "haus").is_none());
    }

    #[test]
    fn the_public_key_url_follows_the_websocket_url() {
        assert_eq!(http_base("ws://192.0.2.7").unwrap(), "http://192.0.2.7");
        assert_eq!(http_base("ws://host:8080/").unwrap(), "http://host:8080");
        assert_eq!(
            http_base("wss://ms.example:443").unwrap(),
            "https://ms.example:443"
        );
        assert!(http_base("http://host").is_err());
    }

    // rivet: verifies REQ-LOX-005
    // rivet: verifies DD-013
    #[tokio::test(flavor = "current_thread")]
    async fn tracked_controls_get_series_and_retired_fields_get_nothing() {
        let _serial = crate::state::tests::serial().await;
        let logs = global_logs();
        let fake = Fake::new(After::Stay).await;
        let url = serve(fake.clone()).await;
        let st = lox_state("lox-track");
        let token_file = tmp_file("track");
        let mut cfg = config(url, PASSWORD, token_file.clone());
        cfg.overrides = HashMap::from([(format!("{ROOM}/HCm"), "-".to_string())]);
        cfg.track = vec![TRACKED.to_uppercase(), LUX.to_string()];
        let task = tokio::spawn(run_loxone_connection(st.clone(), cfg));
        wait_for("the tracked sensor's value", || {
            st.ingest
                .store()
                .series_id(&SeriesKey::new(Source::Loxone, TRACKED, "Default"))
                .unwrap()
                .is_some_and(|_| points(&st, TRACKED, "Default") == vec![55.0])
        })
        .await;
        task.abort();
        let metas = st.ingest.store().series().unwrap();
        let t = metas
            .iter()
            .find(|(_, m)| m.key.source_id == TRACKED)
            .unwrap();
        assert_eq!(t.1.name, "Air sensor");
        assert_eq!(t.1.room.as_deref(), Some("Room A"));
        assert_eq!(t.1.quantity, haus_core::store::Quantity::Humidity);
        assert_eq!(t.1.unit, "%");
        // No climate category: the unit comes from the display format.
        let l = metas.iter().find(|(_, m)| m.key.source_id == LUX).unwrap();
        assert_eq!(
            (l.1.quantity, l.1.unit.as_str()),
            (haus_core::store::Quantity::Other, "lx")
        );
        // The retired field got nothing, the plain one did.
        assert!(points(&st, ROOM, "HCm").is_empty());
        assert_eq!(points(&st, CTRL, "Default"), vec![21.5]);
        let log = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(
            log.contains("3 of 4 series mapped to states, 1 retired"),
            "{log}"
        );
        let _ = std::fs::remove_file(&token_file);
    }

    #[test]
    fn fingerprints_normalize() {
        assert_eq!(normalize_fingerprint("AB:cd:0F"), "abcd0f");
        assert_eq!(normalize_fingerprint(" abcd "), "abcd");
    }

    // rivet: partially-verifies REQ-LOX-002
    #[tokio::test(flavor = "current_thread")]
    async fn a_wrong_password_never_connects_and_stores_nothing() {
        let _serial = crate::state::tests::serial().await;
        let fake = Fake::new(After::Stay).await;
        let url = serve(fake.clone()).await;
        let st = lox_state("lox-badpw");
        let token_file = tmp_file("badpw");
        let task = tokio::spawn(run_loxone_connection(
            st.clone(),
            config(url, "wrong", token_file.clone()),
        ));
        wait_for("two login attempts", || {
            fake.received()
                .iter()
                .filter(|c| c.starts_with("enc:jdev/sys/getjwt/"))
                .count()
                >= 2
        })
        .await;
        task.abort();
        assert!(!st.sources.loxone.as_ref().unwrap().connected());
        assert!(points(&st, CTRL, "Default").is_empty());
        assert!(!token_file.exists());
        assert!(!fake
            .received()
            .iter()
            .any(|c| c.contains("enablebinstatusupdate")));
    }
}
