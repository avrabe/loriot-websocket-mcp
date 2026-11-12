//! `haus import …`: load a history into the store and prove it arrived
//! (REQ-IMPORT-001, REQ-IMPORT-002).

use haus_core::import::{import_loriot, import_stats4lox, ImportReport};
use haus_core::store::Store;
use std::collections::HashMap;
use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: haus import loriot <sensor_history.jsonl> [--names <devices.json>] [--store <path>] [--format text|json]
       haus import stats4lox <export dir> [--store <path>] [--format text|json]

Imports a history into the store, then checks it: every parsed point must be
stored with its value, and a Stats4Lox export's per-field counts must equal the
counts InfluxDB reported in its manifest.json. Re-running an import, or
importing a later export over an earlier one, stores nothing twice.

  --store   store file (default: $HAUS_STORE, else haus.redb)
  --names   EUI -> display name map (devices.json)
  --format  text (default) or json

Exit status: 0 imported and checked, 1 the import did not reproduce its
source (problems are listed), 2 usage error.";

#[derive(Debug, PartialEq)]
pub enum Kind {
    Loriot,
    Stats4lox,
}

#[derive(Debug, PartialEq)]
pub struct ImportArgs {
    pub kind: Kind,
    pub path: PathBuf,
    pub store: PathBuf,
    pub names: Option<PathBuf>,
    pub json: bool,
}

/// Parse the arguments after `import`. `Err` carries a usage message.
pub fn parse(args: &[String], env_store: Option<String>) -> Result<ImportArgs, String> {
    let mut it = args.iter();
    let kind = match it.next().map(String::as_str) {
        Some("loriot") => Kind::Loriot,
        Some("stats4lox") => Kind::Stats4lox,
        Some(other) => return Err(format!("unknown import kind {other:?}")),
        None => return Err("missing import kind".into()),
    };
    let mut path = None;
    let mut store = env_store.filter(|s| !s.is_empty()).map(PathBuf::from);
    let mut names = None;
    let mut json = false;
    while let Some(a) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match a.as_str() {
            "--store" => store = Some(PathBuf::from(value("--store")?)),
            "--names" if kind == Kind::Loriot => names = Some(PathBuf::from(value("--names")?)),
            "--format" => match value("--format")?.as_str() {
                "json" => json = true,
                "text" => json = false,
                f => return Err(format!("unknown format {f:?}")),
            },
            s if s.starts_with('-') => return Err(format!("unknown option {s:?}")),
            s if path.is_none() => path = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s:?}")),
        }
    }
    Ok(ImportArgs {
        kind,
        path: path.ok_or("missing the file or directory to import")?,
        store: store.unwrap_or_else(|| PathBuf::from("haus.redb")),
        names,
        json,
    })
}

/// Run the import; returns the process exit code.
pub fn run(a: &ImportArgs) -> i32 {
    match execute(a) {
        Ok(report) => {
            if a.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": report.ok(),
                        "parsed": report.total_parsed(),
                        "inserted": report.total_inserted(),
                        "series": report.series,
                        "skipped": report.skipped,
                        "problems": report.problems,
                    }))
                    .expect("report serialises")
                );
            } else {
                print_text(&report);
            }
            if report.ok() {
                0
            } else {
                1
            }
        }
        Err(e) => {
            eprintln!("haus import: {e}");
            1
        }
    }
}

fn execute(a: &ImportArgs) -> anyhow::Result<ImportReport> {
    let store = Store::open(&a.store)?;
    Ok(match a.kind {
        Kind::Loriot => {
            let names: HashMap<String, String> = match &a.names {
                Some(p) => serde_json::from_str(&std::fs::read_to_string(p)?)?,
                None => HashMap::new(),
            };
            import_loriot(&store, &a.path, &names)?
        }
        Kind::Stats4lox => import_stats4lox(&store, &a.path)?,
    })
}

fn print_text(r: &ImportReport) {
    for s in &r.series {
        println!(
            "{:<40} {:<16} parsed {:>8}  new {:>8}  unchanged {:>8}{}",
            format!("{} ({})", s.name, s.source_id),
            s.field,
            s.parsed,
            s.inserted,
            s.unchanged,
            if s.changed + s.missing_after_write > 0 {
                "  PROBLEM"
            } else {
                ""
            }
        );
    }
    for s in &r.skipped {
        println!("skipped: {s}");
    }
    for p in &r.problems {
        eprintln!("problem: {p}");
    }
    println!(
        "{} series, {} points parsed, {} new; {}",
        r.series.len(),
        r.total_parsed(),
        r.total_inserted(),
        if r.ok() {
            "every point checked back from the store".to_string()
        } else {
            format!("FAILED with {} problem(s)", r.problems.len())
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_kinds_flags_and_the_store_default() {
        let got = parse(&a(&["loriot", "h.jsonl", "--names", "d.json"]), None).unwrap();
        assert_eq!(
            got,
            ImportArgs {
                kind: Kind::Loriot,
                path: "h.jsonl".into(),
                store: "haus.redb".into(),
                names: Some("d.json".into()),
                json: false
            }
        );
        let got = parse(
            &a(&["stats4lox", "exp", "--format", "json"]),
            Some("/data/x.redb".into()),
        )
        .unwrap();
        assert_eq!(got.store, PathBuf::from("/data/x.redb"));
        assert!(got.json);
        let got = parse(
            &a(&["stats4lox", "exp", "--store", "y.redb"]),
            Some("/data/x.redb".into()),
        )
        .unwrap();
        assert_eq!(got.store, PathBuf::from("y.redb"));
    }

    #[test]
    fn rejects_bad_usage() {
        for bad in [
            &[][..],
            &["nope", "x"],
            &["loriot"],
            &["loriot", "a", "b"],
            &["loriot", "a", "--store"],
            &["loriot", "a", "--format", "xml"],
            &["stats4lox", "d", "--names", "n.json"],
            &["loriot", "a", "--bogus"],
        ] {
            assert!(parse(&a(bad), None).is_err(), "{bad:?}");
        }
    }
}
