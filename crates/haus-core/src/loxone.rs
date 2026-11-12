//! The pure part of the Loxone Miniserver websocket protocol (REQ-LOX-001,
//! DD-004, DD-012), after Loxone's "Communicating with the Loxone
//! Miniserver" V17.0 (31.3.2026). No IO here: the async client lives in the
//! server crate.
//!
//! * Every command haus can send is a [`Cmd`]; there is no way to send a
//!   free-form string, and no variant changes the state of the home.
//! * Token commands are sent encrypted ([`Session::encrypt`]): the document
//!   requires it for `getjwt` before 11.2, and it is harmless after.
//! * Value-state events are 24-byte records: a 16-byte UUID and an `f64`
//!   ([`parse_value_events`]); [`LoxUuid`]'s text form is the one the
//!   structure file and Stats4Lox use, so live events join imported series.

use aes::cipher::{block_padding::ZeroPadding, BlockEncryptMut, KeyIvInit};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LoxError {
    #[error("loxone: {0}")]
    Protocol(String),
}

fn proto(msg: impl Into<String>) -> LoxError {
    LoxError::Protocol(msg.into())
}

/// A Loxone UUID: `Data1` (u32 LE), `Data2`, `Data3` (u16 LE), `Data4` (8
/// bytes), printed `%08x-%04x-%04x-%02x×8`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LoxUuid(pub [u8; 16]);

impl LoxUuid {
    pub fn from_le_bytes(b: &[u8]) -> Result<Self, LoxError> {
        let a: [u8; 16] = b
            .try_into()
            .map_err(|_| proto(format!("uuid needs 16 bytes, got {}", b.len())))?;
        Ok(Self(a))
    }

    /// Parse `13f00a65-01a9-8a08-ffff196db5c1dff3`.
    pub fn parse(s: &str) -> Result<Self, LoxError> {
        let parts: Vec<&str> = s.split('-').collect();
        let bad = || proto(format!("not a Loxone uuid: {s:?}"));
        if parts.len() != 4
            || parts[0].len() != 8
            || parts[1].len() != 4
            || parts[2].len() != 4
            || parts[3].len() != 16
        {
            return Err(bad());
        }
        let d1 = u32::from_str_radix(parts[0], 16).map_err(|_| bad())?;
        let d2 = u16::from_str_radix(parts[1], 16).map_err(|_| bad())?;
        let d3 = u16::from_str_radix(parts[2], 16).map_err(|_| bad())?;
        let d4 = hex::decode(parts[3]).map_err(|_| bad())?;
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&d1.to_le_bytes());
        b[4..6].copy_from_slice(&d2.to_le_bytes());
        b[6..8].copy_from_slice(&d3.to_le_bytes());
        b[8..16].copy_from_slice(&d4);
        Ok(Self(b))
    }
}

impl fmt::Display for LoxUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{}",
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            u16::from_le_bytes([b[4], b[5]]),
            u16::from_le_bytes([b[6], b[7]]),
            hex::encode(&b[8..16])
        )
    }
}

impl fmt::Debug for LoxUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LoxUuid({self})")
    }
}

/// What the message after a header carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgKind {
    Text,
    BinaryFile,
    ValueEvents,
    TextEvents,
    DaytimerEvents,
    OutOfService,
    Keepalive,
    WeatherEvents,
}

/// The 8-byte binary header before every message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: MsgKind,
    /// The length is an estimate; an exact header follows.
    pub estimated: bool,
    pub len: u32,
}

impl Header {
    pub fn parse(b: &[u8]) -> Result<Self, LoxError> {
        if b.len() != 8 || b[0] != 0x03 {
            return Err(proto(format!("not a message header: {b:02x?}")));
        }
        let kind = match b[1] {
            0 => MsgKind::Text,
            1 => MsgKind::BinaryFile,
            2 => MsgKind::ValueEvents,
            3 => MsgKind::TextEvents,
            4 => MsgKind::DaytimerEvents,
            5 => MsgKind::OutOfService,
            6 => MsgKind::Keepalive,
            7 => MsgKind::WeatherEvents,
            k => return Err(proto(format!("unknown message kind {k}"))),
        };
        Ok(Self {
            kind,
            estimated: b[2] & 0x01 != 0,
            len: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        })
    }

    /// Encode (used by tests and the fake Miniserver).
    pub fn encode(kind: u8, len: u32) -> [u8; 8] {
        let l = len.to_le_bytes();
        [0x03, kind, 0, 0, l[0], l[1], l[2], l[3]]
    }
}

/// A value-state event table: 24-byte records of UUID and `f64` (LE).
pub fn parse_value_events(b: &[u8]) -> Result<Vec<(LoxUuid, f64)>, LoxError> {
    if !b.len().is_multiple_of(24) {
        return Err(proto(format!(
            "value event table of {} bytes is not a multiple of 24",
            b.len()
        )));
    }
    b.as_chunks::<24>()
        .0
        .iter()
        .map(|c| {
            let v = f64::from_le_bytes(c[16..24].try_into().expect("8 bytes"));
            Ok((LoxUuid::from_le_bytes(&c[..16])?, v))
        })
        .collect()
}

/// Encode a value-state event table (tests, fake Miniserver).
pub fn encode_value_events(events: &[(LoxUuid, f64)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(events.len() * 24);
    for (u, v) in events {
        out.extend_from_slice(&u.0);
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// A text response `{"LL": {"control", "value", "Code"}}`.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub control: String,
    pub code: u16,
    pub value: Value,
}

impl Response {
    pub fn parse(text: &str) -> Result<Self, LoxError> {
        let v: Value =
            serde_json::from_str(text).map_err(|e| proto(format!("response is not JSON: {e}")))?;
        let ll = &v["LL"];
        let code = ll["Code"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| ll["Code"].as_u64().map(|c| c as u16))
            .or_else(|| ll["code"].as_u64().map(|c| c as u16))
            .or_else(|| ll["code"].as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| proto("response without a code"))?;
        Ok(Self {
            control: ll["control"].as_str().unwrap_or_default().to_string(),
            code,
            value: ll["value"].clone(),
        })
    }
}

/// The hash algorithm `getkey2` names for a user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlg {
    Sha1,
    Sha256,
}

impl HashAlg {
    pub fn parse(s: &str) -> Result<Self, LoxError> {
        match s.to_ascii_uppercase().as_str() {
            "SHA1" => Ok(Self::Sha1),
            "SHA256" => Ok(Self::Sha256),
            other => Err(proto(format!("unknown hashAlg {other:?}"))),
        }
    }
}

/// `{pwHash}`: the uppercase hex hash of `{password}:{userSalt}`.
pub fn password_hash(alg: HashAlg, password: &str, salt: &str) -> String {
    let data = format!("{password}:{salt}");
    match alg {
        HashAlg::Sha1 => hex::encode_upper(Sha1::digest(data.as_bytes())),
        HashAlg::Sha256 => hex::encode_upper(Sha256::digest(data.as_bytes())),
    }
}

/// HMAC of `msg` with the hex-encoded `key` from getkey/getkey2, as hex.
pub fn hmac_hex(alg: HashAlg, key_hex: &str, msg: &str) -> Result<String, LoxError> {
    let key = hex::decode(key_hex).map_err(|_| proto("key is not hex"))?;
    Ok(match alg {
        HashAlg::Sha1 => {
            let mut m = Hmac::<Sha1>::new_from_slice(&key).expect("any key length");
            m.update(msg.as_bytes());
            hex::encode(m.finalize().into_bytes())
        }
        HashAlg::Sha256 => {
            let mut m = Hmac::<Sha256>::new_from_slice(&key).expect("any key length");
            m.update(msg.as_bytes());
            hex::encode(m.finalize().into_bytes())
        }
    })
}

/// A secret string that never prints itself.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Every command haus sends to a Miniserver (DD-012). None of them changes
/// the state of a control; `jdev/sps/io/...` is not representable.
#[derive(Clone, PartialEq, Eq)]
pub enum Cmd {
    /// `jdev/sys/getPublicKey`
    GetPublicKey,
    /// `jdev/sys/keyexchange/{rsa-encrypted session key}`
    KeyExchange(String),
    /// `jdev/sys/getkey2/{user}`
    GetKey2 { user: String },
    /// `jdev/sys/getjwt/{hash}/{user}/{permission}/{uuid}/{info}`
    GetJwt {
        hash: Secret,
        user: String,
        permission: u32,
        client_uuid: String,
        info: String,
    },
    /// `authwithtoken/{hash}/{user}`
    AuthWithToken { hash: Secret, user: String },
    /// `jdev/sys/refreshjwt/{tokenHash}/{user}`
    RefreshJwt { hash: Secret, user: String },
    /// `data/LoxAPP3.json`
    StructureFile,
    /// `jdev/sps/enablebinstatusupdate`
    EnableStatusUpdates,
    /// `keepalive`
    Keepalive,
}

impl fmt::Debug for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Command names, in [`Cmd`] order: the allow-list a test pins.
pub const ALLOWED_COMMANDS: [&str; 9] = [
    "jdev/sys/getPublicKey",
    "jdev/sys/keyexchange/",
    "jdev/sys/getkey2/",
    "jdev/sys/getjwt/",
    "authwithtoken/",
    "jdev/sys/refreshjwt/",
    "data/LoxAPP3.json",
    "jdev/sps/enablebinstatusupdate",
    "keepalive",
];

impl Cmd {
    /// The command's prefix in [`ALLOWED_COMMANDS`]; safe to log.
    pub fn name(&self) -> &'static str {
        match self {
            Cmd::GetPublicKey => ALLOWED_COMMANDS[0],
            Cmd::KeyExchange(_) => ALLOWED_COMMANDS[1],
            Cmd::GetKey2 { .. } => ALLOWED_COMMANDS[2],
            Cmd::GetJwt { .. } => ALLOWED_COMMANDS[3],
            Cmd::AuthWithToken { .. } => ALLOWED_COMMANDS[4],
            Cmd::RefreshJwt { .. } => ALLOWED_COMMANDS[5],
            Cmd::StructureFile => ALLOWED_COMMANDS[6],
            Cmd::EnableStatusUpdates => ALLOWED_COMMANDS[7],
            Cmd::Keepalive => ALLOWED_COMMANDS[8],
        }
    }

    /// Whether the command carries a secret and must go encrypted.
    pub fn must_encrypt(&self) -> bool {
        matches!(
            self,
            Cmd::GetJwt { .. } | Cmd::AuthWithToken { .. } | Cmd::RefreshJwt { .. }
        )
    }

    /// The plain command text. Contains secrets for the token commands: only
    /// ever passed to [`Session::encrypt`], never logged.
    pub fn text(&self) -> String {
        let enc = |s: &str| urlencoding::encode(s).into_owned();
        match self {
            Cmd::GetPublicKey | Cmd::StructureFile | Cmd::EnableStatusUpdates | Cmd::Keepalive => {
                self.name().to_string()
            }
            Cmd::KeyExchange(k) => format!("{}{}", self.name(), k),
            Cmd::GetKey2 { user } => format!("{}{}", self.name(), enc(user)),
            Cmd::GetJwt {
                hash,
                user,
                permission,
                client_uuid,
                info,
            } => format!(
                "{}{}/{}/{permission}/{client_uuid}/{}",
                self.name(),
                hash.expose(),
                enc(user),
                enc(info)
            ),
            Cmd::AuthWithToken { hash, user } | Cmd::RefreshJwt { hash, user } => {
                format!("{}{}/{}", self.name(), hash.expose(), enc(user))
            }
        }
    }
}

/// The AES session of one websocket connection.
pub struct Session {
    key: [u8; 32],
    iv: [u8; 16],
    salt: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Session(***)")
    }
}

impl Session {
    pub fn random() -> Self {
        use rand::RngCore;
        let mut rng = rand::thread_rng();
        let mut key = [0u8; 32];
        let mut iv = [0u8; 16];
        let mut salt = [0u8; 2];
        rng.fill_bytes(&mut key);
        rng.fill_bytes(&mut iv);
        rng.fill_bytes(&mut salt);
        Self {
            key,
            iv,
            salt: hex::encode(salt),
        }
    }

    /// `{key-hex}:{iv-hex}` RSA-encrypted (PKCS#1 v1.5) with the Miniserver's
    /// public key, base64: the argument of `jdev/sys/keyexchange/`.
    pub fn encrypted_key(&self, public_key_pem: &str) -> Result<String, LoxError> {
        use rsa::pkcs8::DecodePublicKey;
        let der = public_key_der(public_key_pem)?;
        let key = rsa::RsaPublicKey::from_public_key_der(&der)
            .map_err(|e| proto(format!("public key: {e}")))?;
        let plain = format!("{}:{}", hex::encode(self.key), hex::encode(self.iv));
        let cipher = key
            .encrypt(
                &mut rand::thread_rng(),
                rsa::Pkcs1v15Encrypt,
                plain.as_bytes(),
            )
            .map_err(|e| proto(format!("rsa: {e}")))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(cipher))
    }

    /// `jdev/sys/enc/{urlencoded base64 AES(salt/{salt}/{cmd})}`.
    pub fn encrypt(&self, cmd: &Cmd) -> String {
        let plain = format!("salt/{}/{}", self.salt, cmd.text());
        let cipher = cbc::Encryptor::<aes::Aes256>::new(&self.key.into(), &self.iv.into())
            .encrypt_padded_vec_mut::<ZeroPadding>(plain.as_bytes());
        let b64 = base64::engine::general_purpose::STANDARD.encode(cipher);
        format!("jdev/sys/enc/{}", urlencoding::encode(&b64))
    }

    /// Decrypt an `enc` command (fake Miniserver in tests).
    pub fn decrypt_with(key: &[u8; 32], iv: &[u8; 16], enc_cmd: &str) -> Result<String, LoxError> {
        use aes::cipher::BlockDecryptMut;
        let b64 = enc_cmd
            .strip_prefix("jdev/sys/enc/")
            .ok_or_else(|| proto("not an enc command"))?;
        let b64 = urlencoding::decode(b64).map_err(|_| proto("bad url encoding"))?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(b64.as_bytes())
            .map_err(|_| proto("bad base64"))?;
        let plain = cbc::Decryptor::<aes::Aes256>::new(key.into(), iv.into())
            .decrypt_padded_vec_mut::<ZeroPadding>(&data)
            .map_err(|_| proto("bad padding"))?;
        String::from_utf8(plain).map_err(|_| proto("not utf-8"))
    }
}

/// The DER of the Miniserver's public key. It labels its
/// SubjectPublicKeyInfo as a certificate and may send it on one line, so take
/// the base64 between the markers.
pub fn public_key_der(public_key_pem: &str) -> Result<Vec<u8>, LoxError> {
    let body: String = public_key_pem
        .split("-----")
        .filter(|part| {
            let p = part.trim();
            !p.is_empty() && !p.starts_with("BEGIN") && !p.starts_with("END")
        })
        .flat_map(|part| part.chars().filter(|c| !c.is_whitespace()))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|e| proto(format!("public key: {e}")))
}

/// The SHA-256 fingerprint (lowercase hex) of the Miniserver's public key:
/// what `LOXONE_KEY_SHA256` pins and the token file remembers.
pub fn public_key_sha256(public_key_pem: &str) -> Result<String, LoxError> {
    Ok(hex::encode(Sha256::digest(public_key_der(public_key_pem)?)))
}

/// Seconds between 1970-01-01 and 2009-01-01 (Loxone's `validUntil` epoch).
pub const LOXONE_EPOCH_UNIX_S: i64 = 1_230_768_000;

/// How a series key of the store maps to a state UUID of the Miniserver.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolution {
    /// State UUID → (control uuid, field) of the series it feeds.
    pub states: HashMap<LoxUuid, (String, String)>,
    /// `control uuid/field` that could not be mapped, with the reason.
    pub unresolved: Vec<String>,
}

/// Map wanted series `(control uuid, field)` to state UUIDs using the
/// structure file (`LoxAPP3.json`).
///
/// * `overrides["{uuid}/{field}"]` names a state of that control (`"tempActual"`)
///   or a state UUID directly; it wins.
/// * Field `Default` is the control's `value` state, else its only state.
/// * A wanted uuid that is itself a state UUID maps to that state.
pub fn resolve_states(
    structure: &Value,
    wanted: &[(String, String)],
    overrides: &HashMap<String, String>,
) -> Resolution {
    // uuidAction -> states (name -> uuid), including subControls.
    let mut controls: HashMap<String, &serde_json::Map<String, Value>> = HashMap::new();
    let mut state_uuids: HashMap<String, ()> = HashMap::new();
    fn add<'a>(c: &'a Value, controls: &mut HashMap<String, &'a serde_json::Map<String, Value>>) {
        if let (Some(u), Some(states)) = (c["uuidAction"].as_str(), c["states"].as_object()) {
            controls.insert(u.to_lowercase(), states);
        }
    }
    if let Some(cs) = structure["controls"].as_object() {
        for c in cs.values() {
            // `states` values are strings or arrays of strings.
            add(c, &mut controls);
            if let Some(subs) = c["subControls"].as_object() {
                for s in subs.values() {
                    add(s, &mut controls);
                }
            }
        }
    }
    for states in controls.values() {
        for v in states.values() {
            if let Some(s) = v.as_str() {
                state_uuids.insert(s.to_lowercase(), ());
            }
        }
    }

    let mut res = Resolution {
        states: HashMap::new(),
        unresolved: Vec::new(),
    };
    for (uuid, field) in wanted {
        let id = format!("{uuid}/{field}");
        let u = uuid.to_lowercase();
        let pick = |name: &str| -> Option<String> {
            controls
                .get(&u)
                .and_then(|st| st.get(name))
                .and_then(Value::as_str)
                .map(str::to_lowercase)
        };
        let state = if let Some(o) = overrides.get(&id) {
            if LoxUuid::parse(o).is_ok() {
                Some(o.to_lowercase())
            } else {
                pick(o)
            }
        } else if field == "Default" {
            pick("value").or_else(|| {
                controls.get(&u).and_then(|st| {
                    let single: Vec<&str> = st.values().filter_map(Value::as_str).collect();
                    (single.len() == 1).then(|| single[0].to_lowercase())
                })
            })
        } else {
            None
        }
        .or_else(|| (field == "Default" && state_uuids.contains_key(&u)).then(|| u.clone()));

        match state.as_deref().map(LoxUuid::parse) {
            Some(Ok(s)) => {
                res.states.insert(s, (uuid.clone(), field.clone()));
            }
            Some(Err(_)) => res
                .unresolved
                .push(format!("{id}: state uuid is malformed")),
            None => res.unresolved.push(format!(
                "{id}: no state found{}",
                if controls.contains_key(&u) {
                    " (control known; map the field in the override file)"
                } else {
                    " (control not in the structure file)"
                }
            )),
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uuid_round_trips_in_the_documented_layout() {
        let s = "13f00a65-01a9-8a08-ffff196db5c1dff3";
        let u = LoxUuid::parse(s).unwrap();
        // Data1..3 little endian, Data4 as is.
        assert_eq!(&u.0[..4], &[0x65, 0x0a, 0xf0, 0x13]);
        assert_eq!(&u.0[4..6], &[0xa9, 0x01]);
        assert_eq!(&u.0[6..8], &[0x08, 0x8a]);
        assert_eq!(&u.0[8..], &hex::decode("ffff196db5c1dff3").unwrap()[..]);
        assert_eq!(u.to_string(), s);
        assert_eq!(LoxUuid::from_le_bytes(&u.0).unwrap(), u);
        for bad in [
            "",
            "13f00a65-01a9-8a08",
            "13f00a6g-01a9-8a08-ffff196db5c1dff3",
            "x-y-z-w",
        ] {
            assert!(LoxUuid::parse(bad).is_err(), "{bad}");
        }
    }

    // rivet: partially-verifies REQ-LOX-001
    #[test]
    fn headers_and_value_events_parse() {
        let h = Header::parse(&[0x03, 2, 0, 0, 48, 0, 0, 0]).unwrap();
        assert_eq!(
            h,
            Header {
                kind: MsgKind::ValueEvents,
                estimated: false,
                len: 48
            }
        );
        assert!(
            Header::parse(&[0x03, 0, 1, 0, 0, 1, 0, 0])
                .unwrap()
                .estimated
        );
        assert_eq!(
            Header::parse(&Header::encode(6, 0)).unwrap().kind,
            MsgKind::Keepalive
        );
        assert!(Header::parse(&[0x02, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(Header::parse(&[0x03, 9, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(Header::parse(&[0x03, 0, 0]).is_err());

        let a = LoxUuid::parse("13f00a65-01a9-8a08-ffff196db5c1dff3").unwrap();
        let b = LoxUuid::parse("0b1e2c3d-0001-0002-0102030405060708").unwrap();
        let table = encode_value_events(&[(a, 20.5), (b, -1.25)]);
        assert_eq!(table.len(), 48);
        assert_eq!(
            parse_value_events(&table).unwrap(),
            vec![(a, 20.5), (b, -1.25)]
        );
        assert!(parse_value_events(&table[..47]).is_err());
        assert_eq!(parse_value_events(&[]).unwrap(), vec![]);
    }

    #[test]
    fn responses_parse_with_either_code_spelling() {
        let r = Response::parse(
            r#"{"LL":{"control":"jdev/sys/getkey2/u","value":{"key":"ab"},"Code":"200"}}"#,
        )
        .unwrap();
        assert_eq!(r.code, 200);
        assert_eq!(r.value["key"], "ab");
        assert_eq!(
            Response::parse(r#"{"LL":{"control":"x","value":"","code":401}}"#)
                .unwrap()
                .code,
            401
        );
        assert!(Response::parse(r#"{"LL":{"control":"x"}}"#).is_err());
        assert!(Response::parse("nope").is_err());
    }

    #[test]
    fn hashes_match_reference_values() {
        // Reference values computed independently with python hashlib/hmac.
        assert_eq!(
            password_hash(HashAlg::Sha1, "pw", "salt"),
            "3659CD609A6B15AB49C66BA2D196191806EF13C5"
        );
        assert_eq!(
            password_hash(HashAlg::Sha256, "pw", "salt"),
            "927E95F9222CF4B9795686B35660BE117887451617F4B57ADF248BC10288CDFA"
        );
        assert_eq!(
            hmac_hex(HashAlg::Sha1, "41424344", "user:ABC").unwrap(),
            "aedd906e0110a8a94d153e040542f23b35ae78e6"
        );
        assert_eq!(
            hmac_hex(HashAlg::Sha256, "41424344", "user:ABC").unwrap(),
            "65d065cd15b8562f488a5533c4788480199c7af2602e2e6958b65511f142efe9"
        );
        assert!(hmac_hex(HashAlg::Sha1, "zz", "x").is_err());
        assert_eq!(HashAlg::parse("sha256").unwrap(), HashAlg::Sha256);
        assert!(HashAlg::parse("MD5").is_err());
    }

    // rivet: verifies REQ-LOX-002
    #[test]
    fn the_command_set_is_the_allow_list_and_hides_secrets() {
        let all = [
            Cmd::GetPublicKey,
            Cmd::KeyExchange("k".into()),
            Cmd::GetKey2 { user: "u".into() },
            Cmd::GetJwt {
                hash: Secret::new("H"),
                user: "u".into(),
                permission: 4,
                client_uuid: "c".into(),
                info: "haus".into(),
            },
            Cmd::AuthWithToken {
                hash: Secret::new("H"),
                user: "u".into(),
            },
            Cmd::RefreshJwt {
                hash: Secret::new("H"),
                user: "u".into(),
            },
            Cmd::StructureFile,
            Cmd::EnableStatusUpdates,
            Cmd::Keepalive,
        ];
        let names: Vec<&str> = all.iter().map(Cmd::name).collect();
        assert_eq!(names, ALLOWED_COMMANDS.to_vec());
        for c in &all {
            let t = c.text();
            assert!(t.starts_with(c.name()), "{t}");
            assert!(!t.contains("sps/io"), "a state-changing command: {t}");
            // Debug never shows a secret.
            assert!(!format!("{c:?}").contains('H'), "{c:?}");
            assert_eq!(c.must_encrypt(), t.contains('H'), "{t}");
        }
        assert_eq!(format!("{:?}", Secret::new("hunter2")), "Secret(***)");
    }

    #[test]
    fn session_encryption_round_trips() {
        let s = Session::random();
        let cmd = Cmd::AuthWithToken {
            hash: Secret::new("abc"),
            user: "user name".into(),
        };
        let enc = s.encrypt(&cmd);
        assert!(enc.starts_with("jdev/sys/enc/"));
        assert!(!enc.contains("abc"));
        let plain = Session::decrypt_with(&s.key, &s.iv, &enc).unwrap();
        assert_eq!(
            plain,
            format!("salt/{}/authwithtoken/abc/user%20name", s.salt)
        );
    }

    fn structure() -> Value {
        json!({"controls": {
            "13f00a65-01a9-8a08-ffff196db5c1dff3": {
                "type": "InfoOnlyAnalog", "uuidAction": "13f00a65-01a9-8a08-ffff196db5c1dff3",
                "states": {"value": "13f00a65-01a9-8a08-ffff196db5c1dff4"}},
            "1d000000-0000-0001-ffff000000000001": {
                "type": "IRoomControllerV2", "uuidAction": "1d000000-0000-0001-ffff000000000001",
                "states": {"tempActual": "1d000000-0000-0001-ffff000000000002",
                           "tempTarget": "1d000000-0000-0001-ffff000000000003"},
                "subControls": {"x": {"uuidAction": "1d000000-0000-0002-ffff000000000001",
                                      "states": {"active": "1d000000-0000-0002-ffff000000000002"}}}}
        }})
    }

    #[test]
    fn series_resolve_to_state_uuids() {
        let wanted: Vec<(String, String)> = [
            ("13f00a65-01a9-8a08-ffff196db5c1dff3", "Default"), // value state
            ("1d000000-0000-0002-ffff000000000001", "Default"), // only state, subControl
            ("1d000000-0000-0001-ffff000000000002", "Default"), // already a state uuid
            ("1d000000-0000-0001-ffff000000000001", "HCm"),     // needs an override
            ("1d000000-0000-0001-ffff000000000001", "Default"), // two states, no value
            ("ffffffff-0000-0000-0000000000000000", "Default"), // unknown control
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let r = resolve_states(&structure(), &wanted, &HashMap::new());
        let get = |s: &str| r.states.get(&LoxUuid::parse(s).unwrap()).cloned();
        assert_eq!(
            get("13f00a65-01a9-8a08-ffff196db5c1dff4"),
            Some((
                "13f00a65-01a9-8a08-ffff196db5c1dff3".into(),
                "Default".into()
            ))
        );
        assert!(get("1d000000-0000-0002-ffff000000000002").is_some());
        assert!(get("1d000000-0000-0001-ffff000000000002").is_some());
        assert_eq!(r.unresolved.len(), 3, "{:?}", r.unresolved);
        assert!(r
            .unresolved
            .iter()
            .any(|u| u.contains("/HCm") && u.contains("override")));
        assert!(r
            .unresolved
            .iter()
            .any(|u| u.starts_with("ffffffff") && u.contains("not in the structure")));

        let overrides = HashMap::from([(
            "1d000000-0000-0001-ffff000000000001/HCm".to_string(),
            "tempTarget".to_string(),
        )]);
        let r = resolve_states(&structure(), &wanted[3..4], &overrides);
        assert!(r.unresolved.is_empty(), "{:?}", r.unresolved);
        assert!(r
            .states
            .contains_key(&LoxUuid::parse("1d000000-0000-0001-ffff000000000003").unwrap()));
    }
}
