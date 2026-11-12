//! The time-series store: every reading of every source in one redb file
//! (DD-003).
//!
//! A *series* is one value stream of one sensor, identified by its
//! [`SeriesKey`]: the source, the source's own stable id for the sensor (a
//! LoRaWAN device EUI, a Loxone control UUID) and the field. Display names
//! and rooms are metadata and can change without changing the series
//! (SC-004, DD-005).
//!
//! Points are keyed by `(series id, time in ms)`, so a range read touches only
//! the requested series and time span (REQ-STORE-001). Writing a point that is
//! already stored with the same value changes nothing, which makes imports
//! idempotent (SC-001).
//!
//! Every method does blocking file IO; async callers run them on a blocking
//! thread.

use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// `(series id, time in ms since the Unix epoch)` → value.
const POINTS: TableDefinition<(u32, i64), f64> = TableDefinition::new("points");
/// Series key (see [`SeriesKey::index_key`]) → series id.
const SERIES_INDEX: TableDefinition<&str, u32> = TableDefinition::new("series_index");
/// Series id → [`SeriesMeta`] as JSON.
const SERIES_META: TableDefinition<u32, &str> = TableDefinition::new("series_meta");

/// Page cache used when none is given. redb's own default is 1 GiB, more than
/// the whole service may use on a small host (REQ-RES-001).
pub const DEFAULT_CACHE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store: {0}")]
    Db(String),
    #[error("store: series metadata: {0}")]
    Meta(#[from] serde_json::Error),
    #[error("store: series key part must not be empty or contain '\\u{{1f}}': {0:?}")]
    BadKey(String),
}

macro_rules! db_err {
    ($($t:ty),*) => {$(
        impl From<$t> for StoreError {
            fn from(e: $t) -> Self {
                StoreError::Db(e.to_string())
            }
        }
    )*};
}
db_err!(
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError
);

pub type StoreResult<T> = std::result::Result<T, StoreError>;

/// Where a series comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// LoRaWAN uplinks through the LORIOT websocket.
    Loriot,
    /// A Loxone Miniserver (live) or its Stats4Lox history (import).
    Loxone,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Loriot => "loriot",
            Source::Loxone => "loxone",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The identity of a series. Nothing in it is a display name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SeriesKey {
    pub source: Source,
    /// The source's stable id for the sensor: device EUI, control UUID.
    pub source_id: String,
    /// The value within the sensor: `temperature`, `Default`, `HCm`, ...
    pub field: String,
}

/// Separates the parts of [`SeriesKey::index_key`]; not allowed inside them.
const SEP: char = '\u{1f}';

impl SeriesKey {
    pub fn new(source: Source, source_id: impl Into<String>, field: impl Into<String>) -> Self {
        Self {
            source,
            source_id: source_id.into(),
            field: field.into(),
        }
    }

    /// The index table's key: `source SEP source_id SEP field`.
    fn index_key(&self) -> StoreResult<String> {
        for part in [&self.source_id, &self.field] {
            if part.is_empty() || part.contains(SEP) {
                return Err(StoreError::BadKey(part.clone()));
            }
        }
        Ok(format!(
            "{}{SEP}{}{SEP}{}",
            self.source, self.source_id, self.field
        ))
    }
}

/// What a series measures. Decides how the dashboard groups and shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantity {
    /// °C
    Temperature,
    /// % relative humidity
    Humidity,
    /// V
    BatteryVoltage,
    /// PIR movement counter
    Motion,
    /// 0/1 activity flag
    Activity,
    /// Heating valve / actuator position, %
    ValvePosition,
    /// A heating state: room controller mode, pump on/off, ...
    Heating,
    /// Anything else the source reports.
    Other,
}

/// Everything about a series that is not its identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeriesMeta {
    pub key: SeriesKey,
    pub quantity: Quantity,
    /// Display unit, e.g. `°C`, `%`, `V`; empty if none.
    pub unit: String,
    /// Display name; may change.
    pub name: String,
    /// Room, if the source names one; may change.
    pub room: Option<String>,
}

/// Small integer id of a series inside one store.
pub type SeriesId = u32;

/// One value of one series at one time (ms since the Unix epoch).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub series: SeriesId,
    pub ts_ms: i64,
    pub value: f64,
}

/// What a [`Store::write`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteStats {
    /// Points that were not stored before.
    pub inserted: u64,
    /// Points already stored with the same value: nothing changed.
    pub unchanged: u64,
    /// Points already stored with a *different* value, now overwritten.
    pub changed: u64,
}

impl std::ops::AddAssign for WriteStats {
    fn add_assign(&mut self, o: Self) {
        self.inserted += o.inserted;
        self.unchanged += o.unchanged;
        self.changed += o.changed;
    }
}

/// The store. Cheap to share behind an `Arc`; redb serialises writers.
pub struct Store {
    db: Database,
}

impl Store {
    /// Open or create the store at `path` with the default page cache.
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        Self::open_with_cache(path, DEFAULT_CACHE_BYTES)
    }

    /// Open or create the store with a page cache of `cache_bytes`.
    pub fn open_with_cache(path: impl AsRef<Path>, cache_bytes: usize) -> StoreResult<Self> {
        let db = Database::builder()
            .set_cache_size(cache_bytes)
            .create(path)?;
        let tx = db.begin_write()?;
        tx.open_table(POINTS)?;
        tx.open_table(SERIES_INDEX)?;
        tx.open_table(SERIES_META)?;
        tx.commit()?;
        Ok(Self { db })
    }

    /// The id of `key`, if the series exists.
    pub fn series_id(&self, key: &SeriesKey) -> StoreResult<Option<SeriesId>> {
        let k = key.index_key()?;
        let tx = self.db.begin_read()?;
        let index = tx.open_table(SERIES_INDEX)?;
        Ok(index.get(k.as_str())?.map(|v| v.value()))
    }

    /// The id of `meta.key`, creating the series if needed. The stored
    /// metadata is replaced by `meta` (a renamed sensor keeps its id).
    pub fn ensure_series(&self, meta: &SeriesMeta) -> StoreResult<SeriesId> {
        let k = meta.key.index_key()?;
        let json = serde_json::to_string(meta)?;
        let tx = self.db.begin_write()?;
        let id = {
            let mut index = tx.open_table(SERIES_INDEX)?;
            let existing = index.get(k.as_str())?.map(|v| v.value());
            let id = match existing {
                Some(id) => id,
                None => {
                    let next = index.len()? as SeriesId;
                    index.insert(k.as_str(), next)?;
                    next
                }
            };
            let mut metas = tx.open_table(SERIES_META)?;
            metas.insert(id, json.as_str())?;
            id
        };
        tx.commit()?;
        Ok(id)
    }

    /// Every series with its metadata, by id.
    pub fn series(&self) -> StoreResult<Vec<(SeriesId, SeriesMeta)>> {
        let tx = self.db.begin_read()?;
        let metas = tx.open_table(SERIES_META)?;
        let mut out = Vec::new();
        for row in metas.iter()? {
            let (id, json) = row?;
            out.push((id.value(), serde_json::from_str(json.value())?));
        }
        Ok(out)
    }

    /// Store `points` in one transaction.
    pub fn write(&self, points: &[Point]) -> StoreResult<WriteStats> {
        let mut stats = WriteStats::default();
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(POINTS)?;
            for p in points {
                match table.insert((p.series, p.ts_ms), p.value)? {
                    None => stats.inserted += 1,
                    Some(old) if same_value(old.value(), p.value) => stats.unchanged += 1,
                    Some(_) => stats.changed += 1,
                }
            }
        }
        tx.commit()?;
        Ok(stats)
    }

    /// The points of `series` with `from_ms <= ts <= to_ms`, oldest first.
    pub fn range(
        &self,
        series: SeriesId,
        from_ms: i64,
        to_ms: i64,
    ) -> StoreResult<Vec<(i64, f64)>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(POINTS)?;
        let mut out = Vec::new();
        if from_ms > to_ms {
            return Ok(out);
        }
        for row in table.range((series, from_ms)..=(series, to_ms))? {
            let (k, v) = row?;
            out.push((k.value().1, v.value()));
        }
        Ok(out)
    }

    /// The newest point of `series`.
    pub fn latest(&self, series: SeriesId) -> StoreResult<Option<(i64, f64)>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(POINTS)?;
        let mut r = table.range((series, i64::MIN)..=(series, i64::MAX))?;
        Ok(match r.next_back() {
            Some(row) => {
                let (k, v) = row?;
                Some((k.value().1, v.value()))
            }
            None => None,
        })
    }

    /// How many points `series` has with `from_ms <= ts <= to_ms`.
    pub fn count(&self, series: SeriesId, from_ms: i64, to_ms: i64) -> StoreResult<u64> {
        if from_ms > to_ms {
            return Ok(0);
        }
        let tx = self.db.begin_read()?;
        let table = tx.open_table(POINTS)?;
        let mut n = 0;
        for row in table.range((series, from_ms)..=(series, to_ms))? {
            row?;
            n += 1;
        }
        Ok(n)
    }
}

/// Equal values, with NaN equal to NaN so re-importing a NaN is not a change.
fn same_value(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "haus-store-{name}-{}-{}.redb",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn meta(source: Source, id: &str, field: &str, name: &str) -> SeriesMeta {
        SeriesMeta {
            key: SeriesKey::new(source, id, field),
            quantity: Quantity::Temperature,
            unit: "°C".into(),
            name: name.into(),
            room: None,
        }
    }

    fn pts(series: SeriesId, v: &[(i64, f64)]) -> Vec<Point> {
        v.iter()
            .map(|&(ts_ms, value)| Point {
                series,
                ts_ms,
                value,
            })
            .collect()
    }

    // rivet: verifies REQ-MODEL-001
    #[test]
    fn series_identity_is_the_key_not_the_name() {
        let path = tmp("identity");
        let store = Store::open(&path).unwrap();
        let a = store
            .ensure_series(&meta(Source::Loxone, "uuid-1", "Default", "Kitchen"))
            .unwrap();
        // Renamed and moved: same series, metadata replaced.
        let mut renamed = meta(Source::Loxone, "uuid-1", "Default", "Kitchen (new)");
        renamed.room = Some("Upstairs".into());
        assert_eq!(store.ensure_series(&renamed).unwrap(), a);
        // Same name, other field / other id / other source: other series.
        let b = store
            .ensure_series(&meta(Source::Loxone, "uuid-1", "HCm", "Kitchen"))
            .unwrap();
        let c = store
            .ensure_series(&meta(Source::Loxone, "uuid-2", "Default", "Kitchen"))
            .unwrap();
        let d = store
            .ensure_series(&meta(Source::Loriot, "uuid-1", "Default", "Kitchen"))
            .unwrap();
        let ids = [a, b, c, d];
        for (i, x) in ids.iter().enumerate() {
            for y in &ids[i + 1..] {
                assert_ne!(x, y);
            }
        }
        let all = store.series().unwrap();
        assert_eq!(all.len(), 4);
        let got = all.iter().find(|(id, _)| *id == a).unwrap();
        assert_eq!(got.1, renamed);
        assert_eq!(
            store
                .series_id(&SeriesKey::new(Source::Loxone, "uuid-1", "Default"))
                .unwrap(),
            Some(a)
        );
        assert_eq!(
            store
                .series_id(&SeriesKey::new(Source::Loxone, "nope", "Default"))
                .unwrap(),
            None
        );

        // Ids survive a reopen.
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .ensure_series(&meta(Source::Loxone, "uuid-2", "Default", "x"))
                .unwrap(),
            c
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn key_parts_must_be_nonempty_and_free_of_the_separator() {
        let path = tmp("badkey");
        let store = Store::open(&path).unwrap();
        for (id, field) in [("", "f"), ("a", ""), ("a\u{1f}b", "f"), ("a", "f\u{1f}")] {
            assert!(matches!(
                store.ensure_series(&meta(Source::Loxone, id, field, "n")),
                Err(StoreError::BadKey(_))
            ));
        }
        let _ = std::fs::remove_file(&path);
    }

    // rivet: partially-verifies REQ-STORE-001
    #[test]
    fn range_reads_one_series_inclusive_and_ordered() {
        let path = tmp("range");
        let store = Store::open(&path).unwrap();
        let a = store
            .ensure_series(&meta(Source::Loriot, "A", "temperature", "a"))
            .unwrap();
        let b = store
            .ensure_series(&meta(Source::Loriot, "B", "temperature", "b"))
            .unwrap();
        // Written out of order, and b's points sit at the same times as a's.
        store
            .write(&pts(a, &[(30, 3.0), (10, 1.0), (20, 2.0), (-5, -0.5)]))
            .unwrap();
        store
            .write(&pts(b, &[(10, 100.0), (20, 200.0), (i64::MIN, 0.0)]))
            .unwrap();

        assert_eq!(
            store.range(a, 10, 30).unwrap(),
            vec![(10, 1.0), (20, 2.0), (30, 3.0)]
        );
        assert_eq!(store.range(a, 11, 29).unwrap(), vec![(20, 2.0)]);
        assert_eq!(
            store.range(a, i64::MIN, i64::MAX).unwrap(),
            vec![(-5, -0.5), (10, 1.0), (20, 2.0), (30, 3.0)]
        );
        assert_eq!(store.range(a, 31, 40).unwrap(), vec![]);
        assert_eq!(store.range(a, 30, 10).unwrap(), vec![]);
        assert_eq!(
            store.range(b, i64::MIN, i64::MAX).unwrap(),
            vec![(i64::MIN, 0.0), (10, 100.0), (20, 200.0)]
        );
        assert_eq!(store.latest(a).unwrap(), Some((30, 3.0)));
        assert_eq!(store.latest(b).unwrap(), Some((20, 200.0)));
        assert_eq!(store.latest(b + 1).unwrap(), None);
        assert_eq!(store.count(a, i64::MIN, i64::MAX).unwrap(), 4);
        assert_eq!(store.count(a, 0, 20).unwrap(), 2);
        assert_eq!(store.count(a, 20, 0).unwrap(), 0);

        // Durable across a reopen.
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.count(b, i64::MIN, i64::MAX).unwrap(), 3);
        let _ = std::fs::remove_file(&path);
    }

    // rivet: partially-verifies REQ-IMPORT-001
    // rivet: partially-verifies REQ-IMPORT-002
    #[test]
    fn rewriting_points_is_idempotent_and_reports_changes() {
        let path = tmp("idempotent");
        let store = Store::open(&path).unwrap();
        let a = store
            .ensure_series(&meta(Source::Loxone, "u", "Default", "a"))
            .unwrap();
        let first = store
            .write(&pts(a, &[(1, 20.0), (2, 20.5), (3, f64::NAN)]))
            .unwrap();
        assert_eq!(
            first,
            WriteStats {
                inserted: 3,
                unchanged: 0,
                changed: 0
            }
        );
        // The same points again (an import re-run, or an overlapping delta).
        let again = store
            .write(&pts(a, &[(1, 20.0), (2, 20.5), (3, f64::NAN), (4, 21.0)]))
            .unwrap();
        assert_eq!(
            again,
            WriteStats {
                inserted: 1,
                unchanged: 3,
                changed: 0
            }
        );
        // A different value at a stored time is reported, not silent.
        let altered = store.write(&pts(a, &[(2, 99.0)])).unwrap();
        assert_eq!(altered.changed, 1);
        assert_eq!(store.count(a, i64::MIN, i64::MAX).unwrap(), 4);
        let _ = std::fs::remove_file(&path);
    }
}
