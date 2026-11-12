//! Server-side SVG trend lines (REQ-UI-002, DD-002), after warte's charts:
//! no charting library, a fixed number of columns, and a path that restarts
//! after every gap so an outage shows as a break, never as a line across it
//! (SC-003).
//!
//! A source that reports on change (Loxone) is silent while a value holds;
//! a periodic one (LoRaWAN, every ~10 min) is silent only when something is
//! wrong. So each series has a *hold*: an empty column repeats the last value
//! while the last point is younger than the hold, and is a gap after that.

use crate::store::Source;

/// Columns per chart.
pub const COLUMNS: usize = 240;

/// How long a value stands for without a new point.
pub fn hold_ms(source: Source) -> i64 {
    match source {
        // Uplinks every ~10 min: three missed ones are a gap.
        Source::Loriot => 30 * 60 * 1000,
        // Events only on change: a day without one is a gap.
        Source::Loxone => 24 * 3600 * 1000,
    }
}

/// The chart ranges the pages offer: (key, milliseconds, label).
pub const RANGES: [(&str, i64, &str); 4] = [
    ("1h", 3_600_000, "1 h"),
    ("24h", 86_400_000, "24 h"),
    ("7d", 7 * 86_400_000, "7 d"),
    ("90d", 90 * 86_400_000, "90 d"),
];

/// The range for a key, defaulting to 24 h.
pub fn range(key: Option<&str>) -> (&'static str, i64) {
    RANGES
        .iter()
        .find(|(k, _, _)| Some(*k) == key)
        .map(|(k, ms, _)| (*k, *ms))
        .unwrap_or(("24h", 86_400_000))
}

/// The value of each of `columns` equal columns over `[from, to)`: the mean
/// of its points, else the last value while within `hold`, else `None`.
/// `points` are sorted by time and may start before `from` (the value
/// standing at `from`).
pub fn columns(
    points: &[(i64, f64)],
    from: i64,
    to: i64,
    columns: usize,
    hold: i64,
) -> Vec<Option<f64>> {
    if columns == 0 || to <= from {
        return Vec::new();
    }
    let width = (to - from) as f64 / columns as f64;
    let mut out = Vec::with_capacity(columns);
    let mut i = 0;
    let mut last: Option<(i64, f64)> = None;
    while i < points.len() && points[i].0 < from {
        last = Some(points[i]);
        i += 1;
    }
    for c in 0..columns {
        let start = from + (c as f64 * width) as i64;
        let end = if c + 1 == columns {
            to
        } else {
            from + ((c + 1) as f64 * width) as i64
        };
        let (mut sum, mut n) = (0.0, 0u32);
        while i < points.len() && points[i].0 < end {
            if points[i].1.is_finite() {
                sum += points[i].1;
                n += 1;
            }
            last = Some(points[i]);
            i += 1;
        }
        out.push(if n > 0 {
            Some(sum / n as f64)
        } else {
            last.filter(|(ts, v)| start - ts <= hold && v.is_finite())
                .map(|(_, v)| v)
        });
    }
    out
}

fn fmt_num(v: f64) -> String {
    if v.abs() >= 100.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// An SVG line chart of `cols` (from [`columns`]). `unit` is appended to the
/// axis labels; `labels` are the left/right time captions. Returns a short
/// "no data" figure when every column is empty.
pub fn svg(cols: &[Option<f64>], unit: &str, labels: (&str, &str)) -> String {
    const W: f64 = 720.0;
    const H: f64 = 200.0;
    const PAD_L: f64 = 48.0;
    const PAD_B: f64 = 22.0;
    let vals: Vec<f64> = cols.iter().flatten().copied().collect();
    if vals.is_empty() {
        return format!(
            r#"<svg class="chart" viewBox="0 0 {W} {H}" role="img" aria-label="no data"><text x="{x}" y="{y}" class="nodata">no data in this range</text></svg>"#,
            x = W / 2.0,
            y = H / 2.0
        );
    }
    let (mut lo, mut hi) = vals
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    if (hi - lo).abs() < 1e-9 {
        lo -= 1.0;
        hi += 1.0;
    }
    let pad = (hi - lo) * 0.08;
    let (lo, hi) = (lo - pad, hi + pad);
    let plot_w = W - PAD_L;
    let plot_h = H - PAD_B;
    let step = plot_w / cols.len().max(1) as f64;
    let y = |v: f64| plot_h - (v - lo) / (hi - lo) * plot_h;
    let mut d = String::new();
    let mut pen_down = false;
    for (i, c) in cols.iter().enumerate() {
        match c {
            Some(v) => {
                let x = PAD_L + (i as f64 + 0.5) * step;
                d.push_str(&format!(
                    "{}{x:.1} {:.1} ",
                    if pen_down { "L" } else { "M" },
                    y(*v)
                ));
                pen_down = true;
            }
            None => pen_down = false,
        }
    }
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let unit = esc(unit);
    format!(
        concat!(
            r#"<svg class="chart" viewBox="0 0 {W} {H}" role="img" aria-label="trend">"#,
            r#"<line class="axis" x1="{pl}" y1="0" x2="{pl}" y2="{ph}"/>"#,
            r#"<line class="axis" x1="{pl}" y1="{ph}" x2="{W}" y2="{ph}"/>"#,
            r#"<text class="tick" x="{tx}" y="12" text-anchor="end">{hi}{unit}</text>"#,
            r#"<text class="tick" x="{tx}" y="{ph}" text-anchor="end">{lo}{unit}</text>"#,
            r#"<text class="tick" x="{pl}" y="{ty}">{l0}</text>"#,
            r#"<text class="tick" x="{tr}" y="{ty}" text-anchor="end">{l1}</text>"#,
            r#"<path class="line" d="{d}"/></svg>"#
        ),
        W = W,
        H = H,
        pl = PAD_L,
        ph = plot_h,
        tx = PAD_L - 4.0,
        ty = H - 5.0,
        tr = W - 4.0,
        hi = fmt_num(hi - pad),
        lo = fmt_num(lo + pad),
        unit = unit,
        l0 = esc(labels.0),
        l1 = esc(labels.1),
        d = d.trim_end(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // rivet: verifies REQ-UI-002
    #[test]
    fn a_gap_longer_than_the_hold_breaks_the_line() {
        // 10 columns of 10 ms over [0, 100); hold 15 ms.
        let pts = [(0, 1.0), (5, 3.0), (12, 4.0), (60, 6.0), (95, 7.0)];
        let c = columns(&pts, 0, 100, 10, 15);
        assert_eq!(
            c,
            vec![
                Some(2.0), // mean of 1 and 3
                Some(4.0),
                Some(4.0), // held: last point at 12, column starts at 20
                None,      // 30 - 12 > 15: a gap
                None,
                None,
                Some(6.0),
                Some(6.0),
                None,
                Some(7.0),
            ]
        );
        let s = svg(&c, "°C", ("a", "b"));
        // Three segments: three pen-downs.
        assert_eq!(s.matches('M').count(), 3, "{s}");
        assert!(s.contains("°C"));
    }

    #[test]
    fn a_point_before_the_range_stands_at_its_start() {
        let c = columns(&[(-10, 5.0)], 0, 100, 4, 50);
        assert_eq!(c, vec![Some(5.0), Some(5.0), None, None]);
        // Too old to stand for anything.
        assert_eq!(columns(&[(-100, 5.0)], 0, 100, 2, 50), vec![None, None]);
    }

    #[test]
    fn empty_and_degenerate_ranges() {
        assert!(columns(&[], 0, 100, 0, 1).is_empty());
        assert!(columns(&[(1, 1.0)], 100, 100, 10, 1).is_empty());
        assert_eq!(columns(&[], 0, 100, 3, 1), vec![None, None, None]);
        assert!(svg(&[None, None], "%", ("a", "b")).contains("no data"));
        // A flat line still gets a scale.
        let s = svg(&[Some(20.0), Some(20.0)], "°C", ("a", "b"));
        assert!(s.contains("<path"), "{s}");
        // NaN does not poison a column.
        assert_eq!(
            columns(&[(1, f64::NAN), (2, 4.0)], 0, 10, 1, 0),
            vec![Some(4.0)]
        );
    }

    #[test]
    fn labels_are_escaped() {
        let s = svg(&[Some(1.0)], "<u>", ("<a>", "&"));
        assert!(!s.contains("<u>") && !s.contains("<a>"));
        assert!(s.contains("&lt;u&gt;") && s.contains("&amp;"));
    }

    #[test]
    fn all_columns_present_for_long_series() {
        let pts: Vec<(i64, f64)> = (0..100_000)
            .map(|i| (i * 60_000, (i % 100) as f64))
            .collect();
        let c = columns(&pts, 0, 100_000 * 60_000, COLUMNS, hold_ms(Source::Loriot));
        assert_eq!(c.len(), COLUMNS);
        assert!(c.iter().all(Option::is_some));
        assert_eq!(range(Some("7d")), ("7d", 7 * 86_400_000));
        assert_eq!(range(Some("bogus")).0, "24h");
    }
}
