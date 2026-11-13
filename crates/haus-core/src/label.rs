//! Readable sensor labels (REQ-UI-007, DD-018). A source's name often
//! repeats what the page already shows: Loxone calls a control "Temperatur
//! Küche" in the room "Küche", so "Temperature · Temperatur Küche" says the
//! quantity and the room twice. The label keeps only what is left once the
//! room and the quantity words are taken out of the name, and an operator
//! file can set the label of any series.

use crate::store::{Quantity, SeriesKey};
use std::collections::HashMap;

/// Words that only restate the quantity (German and English, lower case).
fn quantity_words(q: Quantity) -> &'static [&'static str] {
    match q {
        Quantity::Temperature => &[
            "temperatur",
            "temperature",
            "temp",
            "raumtemperatur",
            "lufttemperatur",
        ],
        Quantity::Humidity => &[
            "luftfeuchte",
            "luftfeuchtigkeit",
            "feuchte",
            "feuchtigkeit",
            "humidity",
        ],
        Quantity::BatteryVoltage => &["batterie", "battery", "akku"],
        Quantity::ValvePosition => &["ventil", "valve", "stellantrieb"],
        Quantity::Motion | Quantity::Activity | Quantity::Heating | Quantity::Other => &[],
    }
}

/// Tokens that carry nothing once their neighbours are gone.
fn is_filler(t: &str) -> bool {
    t.chars()
        .all(|c| matches!(c, '-' | '–' | '·' | ':' | '/' | '|' | ','))
}

/// The part of `name` that is neither the room nor a word for the
/// quantity; `None` when nothing is left (the name only said those).
pub fn short(name: &str, room: Option<&str>, q: Quantity) -> Option<String> {
    let mut tokens: Vec<&str> = name.split_whitespace().collect();
    if let Some(room) = room {
        let r: Vec<String> = room.split_whitespace().map(str::to_lowercase).collect();
        if !r.is_empty() {
            let mut i = 0;
            while i + r.len() <= tokens.len() {
                let hit = tokens[i..i + r.len()]
                    .iter()
                    .zip(&r)
                    .all(|(t, w)| t.to_lowercase() == *w);
                if hit {
                    tokens.drain(i..i + r.len());
                } else {
                    i += 1;
                }
            }
        }
    }
    let words = quantity_words(q);
    tokens.retain(|t| !words.contains(&t.to_lowercase().as_str()));
    while tokens.first().is_some_and(|t| is_filler(t)) {
        tokens.remove(0);
    }
    while tokens.last().is_some_and(|t| is_filler(t)) {
        tokens.pop();
    }
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

/// Labels set by the operator, keyed like LOXONE_MAP: `"<source id>/<field>"`
/// (a control uuid or a device EUI, then the field).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Labels(HashMap<String, String>);

impl Labels {
    /// Parse and check the override file; every error names the key.
    pub fn parse(json: &str) -> Result<Labels, String> {
        let map: HashMap<String, serde_json::Value> =
            serde_json::from_str(json).map_err(|e| format!("labels file: {e}"))?;
        let mut out = HashMap::new();
        for (k, v) in map {
            let ok_key =
                matches!(k.split_once('/'), Some((id, f)) if !id.is_empty() && !f.is_empty());
            if !ok_key {
                return Err(format!("{k:?}: key must be \"<source id>/<field>\""));
            }
            let Some(s) = v.as_str() else {
                return Err(format!("{k:?}: must be a string, got {v}"));
            };
            let n = s.trim().chars().count();
            if !(1..=40).contains(&n) {
                return Err(format!("{k:?}: 1 to 40 characters after trimming, got {n}"));
            }
            out.insert(k, s.trim().to_string());
        }
        Ok(Labels(out))
    }

    /// The operator's label for a series, if there is one.
    pub fn get(&self, key: &SeriesKey) -> Option<&str> {
        self.0
            .get(&format!("{}/{}", key.source_id, key.field))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Source;

    // rivet: verifies REQ-UI-007
    #[test]
    fn the_label_drops_the_room_and_the_quantity() {
        let t = Quantity::Temperature;
        let cases = [
            ("Temperatur Küche", Some("Küche"), t, None),
            ("Raumtemperatur", Some("Bad"), t, None),
            (
                "Touch Gästebad Spiegel",
                Some("Gästebad"),
                t,
                Some("Touch Spiegel"),
            ),
            ("Temperatur - Diele", Some("Diele"), t, None),
            (
                "Fußboden Temperatur Gästebad",
                Some("Gästebad"),
                t,
                Some("Fußboden"),
            ),
            (
                "Luftfeuchte Arbeitsraum",
                Some("Arbeitsraum"),
                Quantity::Humidity,
                None,
            ),
            ("Hobbyraum Sensor", None, t, Some("Hobbyraum Sensor")),
            ("Hobbyraum Sensor", Some("Hobbyraum Sensor"), t, None),
            (
                "Heizung Küche",
                Some("Küche"),
                Quantity::Heating,
                Some("Heizung"),
            ),
            // A room word inside another word stays.
            ("Badheizkörper", Some("Bad"), t, Some("Badheizkörper")),
            // Case does not matter.
            ("TEMPERATUR küche", Some("Küche"), t, None),
        ];
        for (name, room, q, want) in cases {
            assert_eq!(
                short(name, room, q).as_deref(),
                want,
                "{name:?} in {room:?}"
            );
        }
    }

    // rivet: verifies REQ-UI-007
    #[test]
    fn an_operator_label_wins_and_a_bad_file_names_the_key() {
        let l = Labels::parse(
            r#"{"0f1e-uuid/Default": " Spiegel ", "0000000000000001/temperature": "Fenster"}"#,
        )
        .unwrap();
        let k = SeriesKey::new(Source::Loxone, "0f1e-uuid", "Default");
        assert_eq!(l.get(&k), Some("Spiegel"));
        let k = SeriesKey::new(Source::Loriot, "0000000000000001", "temperature");
        assert_eq!(l.get(&k), Some("Fenster"));
        assert_eq!(
            l.get(&SeriesKey::new(Source::Loxone, "0f1e-uuid", "HCm")),
            None
        );
        for (json, names) in [
            (r#"{"nouuid": "x"}"#, "nouuid"),
            (r#"{"/f": "x"}"#, "/f"),
            (r#"{"a/": "x"}"#, "a/"),
            (r#"{"a/f": 3}"#, "a/f"),
            (r#"{"a/f": "  "}"#, "a/f"),
            (
                r#"{"a/f": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}"#,
                "a/f",
            ),
            (r#"["a/f", "x"]"#, "labels file"),
        ] {
            let e = Labels::parse(json).unwrap_err();
            assert!(e.contains(names), "{json}: {e}");
        }
        // 40 characters pass.
        Labels::parse(&format!(r#"{{"a/f": "{}"}}"#, "ü".repeat(40))).unwrap();
    }
}
