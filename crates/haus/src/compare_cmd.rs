//! `haus compare stats4lox …`: the evidence before a Stats4Lox logger is
//! switched off (REQ-RETIRE-001).

use haus_core::compare::{compare_stats4lox, CompareOptions, CompareReport};
use haus_core::store::Store;
use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: haus compare stats4lox <export dir> --since <time> [--store <path>]
       [--tolerance <x>] [--skew-seconds <s>] [--format text|json]

For every Stats4Lox sample at or after --since, checks that haus had the same
value standing at that time (the last value it received, allowing --skew-seconds
of clock difference, default 60, and a difference up to --tolerance, default
0.01). Run it on a fresh export BEFORE importing that export: afterwards the
store holds the logger's own points and the comparison proves nothing.

  --since   when haus started receiving from the Miniserver:
            2026-11-20, 2026-11-20T08:00, or RFC 3339 (UTC unless an offset is given)
  --store   store file (default: $HAUS_STORE, else haus.redb)

Exit status: 0 every sample agrees, 1 a disagreement, a missing value, a series
haus does not have, or nothing to compare; 2 usage error.";

#[derive(Debug, PartialEq)]
pub struct CompareArgs {
    pub dir: PathBuf,
    pub store: PathBuf,
    pub opts_since_ms: i64,
    pub tolerance: f64,
    pub skew_ms: i64,
    pub json: bool,
}

/// `2026-11-20`, `2026-11-20T08:00[:00]` (UTC) or RFC 3339.
pub fn parse_time(s: &str) -> Option<i64> {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(t.timestamp_millis());
    }
    for f in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(t) = chrono::NaiveDateTime::parse_from_str(s, f) {
            return Some(t.and_utc().timestamp_millis());
        }
    }
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .map(|d| {
            d.and_hms_opt(0, 0, 0)
                .expect("midnight")
                .and_utc()
                .timestamp_millis()
        })
}

pub fn parse(args: &[String], env_store: Option<String>) -> Result<CompareArgs, String> {
    let mut it = args.iter();
    match it.next().map(String::as_str) {
        Some("stats4lox") => {}
        Some(other) => return Err(format!("unknown comparison {other:?}")),
        None => return Err("missing what to compare (stats4lox)".into()),
    }
    let (mut dir, mut since, mut json) = (None, None, false);
    let mut store = env_store.filter(|s| !s.is_empty()).map(PathBuf::from);
    let (mut tolerance, mut skew_ms) = (0.01, 60_000);
    while let Some(a) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match a.as_str() {
            "--since" => {
                let v = value("--since")?;
                since = Some(parse_time(&v).ok_or_else(|| format!("--since: not a time: {v:?}"))?);
            }
            "--store" => store = Some(PathBuf::from(value("--store")?)),
            "--tolerance" => {
                tolerance = value("--tolerance")?
                    .parse::<f64>()
                    .ok()
                    .filter(|t| t.is_finite() && (0.0..=1.0).contains(t))
                    .ok_or("--tolerance must be between 0 and 1")?
            }
            "--skew-seconds" => {
                skew_ms = value("--skew-seconds")?
                    .parse::<i64>()
                    .ok()
                    .filter(|s| (0..=300).contains(s))
                    .ok_or("--skew-seconds must be 0..300")?
                    * 1000
            }
            "--format" => match value("--format")?.as_str() {
                "json" => json = true,
                "text" => json = false,
                f => return Err(format!("unknown format {f:?}")),
            },
            s if s.starts_with('-') => return Err(format!("unknown option {s:?}")),
            s if dir.is_none() => dir = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s:?}")),
        }
    }
    Ok(CompareArgs {
        dir: dir.ok_or("missing the export directory")?,
        store: store.unwrap_or_else(|| PathBuf::from("haus.redb")),
        opts_since_ms: since.ok_or("--since is required: when haus went live")?,
        tolerance,
        skew_ms,
        json,
    })
}

pub fn run(a: &CompareArgs) -> i32 {
    let result = Store::open(&a.store)
        .map_err(|e| e.to_string())
        .and_then(|store| {
            compare_stats4lox(
                &store,
                &a.dir,
                CompareOptions {
                    since_ms: a.opts_since_ms,
                    tolerance: a.tolerance,
                    skew_ms: a.skew_ms,
                },
            )
            .map_err(|e| e.to_string())
        });
    match result {
        Ok(r) => {
            if a.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": r.ok(), "samples": r.total_samples(),
                        "series": r.series, "not_compared": r.not_compared,
                        "skipped": r.skipped, "problems": r.problems,
                    }))
                    .expect("report serialises")
                );
            } else {
                print_text(&r);
            }
            if r.ok() {
                0
            } else {
                1
            }
        }
        Err(e) => {
            eprintln!("haus compare: {e}");
            1
        }
    }
}

fn print_text(r: &CompareReport) {
    for s in &r.series {
        println!(
            "{:<40} {:<8} samples {:>7}  agree {:>7}  differ {:>5}  no value {:>5}{}",
            format!("{} ({})", s.name, s.source_id),
            s.field,
            s.samples,
            s.matched,
            s.mismatched,
            s.no_value,
            s.first_problem
                .as_deref()
                .map(|p| format!("  first: {p}"))
                .unwrap_or_default()
        );
    }
    for n in &r.not_compared {
        println!("not compared: {n}");
    }
    for n in &r.skipped {
        println!("not compared (skipped by the reader): {n}");
    }
    for p in &r.problems {
        eprintln!("problem: {p}");
    }
    println!(
        "{} series, {} samples; {}",
        r.series.len(),
        r.total_samples(),
        if r.ok() {
            "haus agrees with every sample"
        } else {
            "haus does NOT reproduce the logger yet"
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
    fn parses_times_and_flags() {
        assert_eq!(parse_time("2026-11-20"), Some(1_795_132_800_000));
        assert_eq!(
            parse_time("2026-11-20T08:00"),
            Some(1_795_132_800_000 + 8 * 3_600_000)
        );
        assert_eq!(
            parse_time("2026-11-20T08:00:00+01:00"),
            Some(1_795_132_800_000 + 7 * 3_600_000)
        );
        assert_eq!(parse_time("yesterday"), None);
        let got = parse(
            &a(&[
                "stats4lox",
                "exp",
                "--since",
                "2026-11-20",
                "--skew-seconds",
                "30",
            ]),
            None,
        )
        .unwrap();
        assert_eq!(got.skew_ms, 30_000);
        assert_eq!(got.store, PathBuf::from("haus.redb"));
        for bad in [
            &["stats4lox", "exp"][..],
            &["stats4lox", "--since", "2026-11-20"],
            &["loriot", "exp", "--since", "2026-11-20"],
            &["stats4lox", "exp", "--since", "soon"],
            &[
                "stats4lox",
                "exp",
                "--since",
                "2026-11-20",
                "--tolerance",
                "-1",
            ],
            &[
                "stats4lox",
                "exp",
                "--since",
                "2026-11-20",
                "--skew-seconds",
                "301",
            ],
            &[
                "stats4lox",
                "exp",
                "--since",
                "2026-11-20",
                "--tolerance",
                "2",
            ],
        ] {
            assert!(parse(&a(bad), None).is_err(), "{bad:?}");
        }
    }
}
