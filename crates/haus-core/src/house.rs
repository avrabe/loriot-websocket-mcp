//! The house model for the 3D view (REQ-UI-006, DD-017): one JSON file on
//! the deployment (HAUS_HOUSE_MODEL), read and checked once at startup.
//!
//! Coordinates are plan metres: x to the right of the plan, y up it;
//! `north_deg` is the compass direction of plan +y turned to true north
//! (true north lies `north_deg` clockwise from plan +y). Heights are metres above the floor of the level at z = 0.
//! The real model is private deployment data; the repository carries only
//! a made-up example (`crates/haus-core/examples/house.json`).

use serde::{Deserialize, Serialize};

/// A plan point in metres.
pub type Pt = [f64; 2];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct House {
    pub version: u32,
    /// True north, degrees clockwise from plan +y.
    #[serde(default)]
    pub north_deg: f64,
    pub levels: Vec<Level>,
    pub rooms: Vec<Room>,
    /// Other units and shared rooms, drawn as neutral floors.
    #[serde(default)]
    pub others: Vec<Other>,
    /// Walls and roofs, shown on demand.
    #[serde(default)]
    pub buildings: Vec<Building>,
    #[serde(default)]
    pub site: Option<Site>,
    /// Fixed viewpoints, as buttons.
    #[serde(default)]
    pub views: Vec<View>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub id: String,
    pub name: String,
    /// Floor height in metres.
    pub z: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub level: String,
    pub poly: Vec<Pt>,
    /// A shorter name for the compact label; default: `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    /// Where the label sits; default: the middle of the polygon's box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Pt>,
    #[serde(default)]
    pub outdoor: bool,
    /// A terrace or balcony: drawn as a deck with a railing.
    #[serde(default)]
    pub deck: bool,
    /// The haus rooms (as on the overview) whose sensors are in this room.
    #[serde(default)]
    pub haus_rooms: Vec<String>,
    /// Shown when the room is clicked, e.g. the devices it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Other {
    /// The unit it belongs to, tagged once per unit and level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub name: String,
    pub level: String,
    pub poly: Vec<Pt>,
    #[serde(default)]
    pub outdoor: bool,
    /// The layout is assumed, not taken from a plan: drawn fainter.
    #[serde(default)]
    pub assumed: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoofKind {
    Flat,
    /// Ridge along the longer side of the outline's box, gables at its ends.
    Gable,
    /// Ridge along the longer side, all four sides sloped.
    Hip,
    /// A vault along the longer side.
    Barrel,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Building {
    pub name: String,
    pub outline: Vec<Pt>,
    /// Wall height (eaves) in metres.
    pub wall_m: f64,
    pub roof: RoofKind,
    /// Ridge or crown height; required unless the roof is flat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ridge_m: Option<f64>,
    /// Glass walls (a hall or conservatory).
    #[serde(default)]
    pub glass: bool,
    /// Box dormers on the slope facing low x (low y when the ridge runs
    /// along x), each spanning from..to along the ridge.
    #[serde(default)]
    pub dormers: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Site {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parcel: Option<Vec<Pt>>,
    #[serde(default)]
    pub streets: Vec<Street>,
    #[serde(default)]
    pub neighbours: Vec<Neighbour>,
    #[serde(default)]
    pub parking: Vec<Named>,
    #[serde(default)]
    pub trees: Vec<Pt>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreetKind {
    Road,
    Path,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Street {
    pub name: String,
    pub kind: StreetKind,
    pub poly: Vec<Pt>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Neighbour {
    pub name: String,
    pub poly: Vec<Pt>,
    pub height_m: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Named {
    pub name: String,
    pub poly: Vec<Pt>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub name: String,
    /// Compass direction from the house to the camera, degrees (0 = north,
    /// 90 = east).
    pub from_deg: f64,
    /// Horizontal distance from the middle of the rooms; 0 looks straight down.
    pub distance_m: f64,
    pub height_m: f64,
    /// Show the walls and roofs (and close the floors).
    #[serde(default)]
    pub shell: bool,
}

fn name(field: &str, s: &str) -> Result<(), String> {
    let n = s.trim().chars().count();
    if (1..=80).contains(&n) {
        Ok(())
    } else {
        Err(format!(
            "{field}: 1 to 80 characters after trimming, got {n}"
        ))
    }
}

fn num(field: &str, v: f64, lo: f64, hi: f64) -> Result<(), String> {
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(format!("{field}: {v} is not between {lo} and {hi}"))
    }
}

/// Plan coordinates stay within a kilometre of the origin.
const SPAN: f64 = 1000.0;

fn pt(field: &str, p: &Pt) -> Result<(), String> {
    num(field, p[0], -SPAN, SPAN)?;
    num(field, p[1], -SPAN, SPAN)
}

fn poly(field: &str, ps: &[Pt]) -> Result<(), String> {
    if !(3..=200).contains(&ps.len()) {
        return Err(format!("{field}: 3 to 200 points, got {}", ps.len()));
    }
    ps.iter().try_for_each(|p| pt(field, p))
}

/// Parse and check a house model; every error names the field.
pub fn parse(json: &str) -> Result<House, String> {
    let h: House = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if h.version != 1 {
        return Err(format!("version: must be 1, got {}", h.version));
    }
    num("north_deg", h.north_deg, -360.0, 360.0)?;
    if !(1..=20).contains(&h.levels.len()) {
        return Err(format!("levels: 1 to 20 entries, got {}", h.levels.len()));
    }
    let mut levels = std::collections::HashSet::new();
    for (i, l) in h.levels.iter().enumerate() {
        let f = |k: &str| format!("levels[{i}].{k}");
        name(&f("id"), &l.id)?;
        if !levels.insert(l.id.as_str()) {
            return Err(format!("{}: {:?} is not unique", f("id"), l.id));
        }
        name(&f("name"), &l.name)?;
        num(&f("z"), l.z, -50.0, 200.0)?;
    }
    let level = |field: String, id: &str| {
        if levels.contains(id) {
            Ok(())
        } else {
            Err(format!("{field}: {id:?} is not one of the level ids"))
        }
    };
    if !(1..=500).contains(&h.rooms.len()) {
        return Err(format!("rooms: 1 to 500 entries, got {}", h.rooms.len()));
    }
    let mut ids = std::collections::HashSet::new();
    for (i, r) in h.rooms.iter().enumerate() {
        let f = |k: &str| format!("rooms[{i}].{k}");
        name(&f("id"), &r.id)?;
        if !ids.insert(r.id.as_str()) {
            return Err(format!("{}: {:?} is not unique", f("id"), r.id));
        }
        name(&f("name"), &r.name)?;
        if let Some(n) = &r.short {
            name(&f("short"), n)?;
        }
        level(f("level"), &r.level)?;
        poly(&f("poly"), &r.poly)?;
        if let Some(p) = &r.label {
            pt(&f("label"), p)?;
        }
        for (j, hr) in r.haus_rooms.iter().enumerate() {
            name(&f(&format!("haus_rooms[{j}]")), hr)?;
        }
        if let Some(n) = &r.note {
            if n.chars().count() > 500 {
                return Err(format!("{}: at most 500 characters", f("note")));
            }
        }
    }
    if h.others.len() > 2000 {
        return Err(format!(
            "others: at most 2000 entries, got {}",
            h.others.len()
        ));
    }
    for (i, o) in h.others.iter().enumerate() {
        let f = |k: &str| format!("others[{i}].{k}");
        if let Some(u) = &o.unit {
            name(&f("unit"), u)?;
        }
        name(&f("name"), &o.name)?;
        level(f("level"), &o.level)?;
        poly(&f("poly"), &o.poly)?;
    }
    for (i, b) in h.buildings.iter().enumerate() {
        let f = |k: &str| format!("buildings[{i}].{k}");
        name(&f("name"), &b.name)?;
        poly(&f("outline"), &b.outline)?;
        num(&f("wall_m"), b.wall_m, 0.5, 200.0)?;
        match (b.roof, b.ridge_m) {
            (RoofKind::Flat, None) => {}
            (RoofKind::Flat, Some(_)) => {
                return Err(format!("{}: a flat roof has no ridge", f("ridge_m")))
            }
            (_, None) => return Err(format!("{}: required for this roof", f("ridge_m"))),
            (_, Some(r)) => num(&f("ridge_m"), r, b.wall_m, 200.0)?,
        }
        for (j, d) in b.dormers.iter().enumerate() {
            if !(d[0].is_finite() && d[1].is_finite() && d[0] < d[1]) {
                return Err(format!(
                    "{}: from must be below to",
                    f(&format!("dormers[{j}]"))
                ));
            }
        }
    }
    if let Some(s) = &h.site {
        if let Some(p) = &s.parcel {
            poly("site.parcel", p)?;
        }
        for (i, x) in s.streets.iter().enumerate() {
            name(&format!("site.streets[{i}].name"), &x.name)?;
            poly(&format!("site.streets[{i}].poly"), &x.poly)?;
        }
        for (i, x) in s.neighbours.iter().enumerate() {
            name(&format!("site.neighbours[{i}].name"), &x.name)?;
            poly(&format!("site.neighbours[{i}].poly"), &x.poly)?;
            num(
                &format!("site.neighbours[{i}].height_m"),
                x.height_m,
                0.5,
                300.0,
            )?;
        }
        for (i, x) in s.parking.iter().enumerate() {
            name(&format!("site.parking[{i}].name"), &x.name)?;
            poly(&format!("site.parking[{i}].poly"), &x.poly)?;
        }
        for (i, t) in s.trees.iter().enumerate() {
            pt(&format!("site.trees[{i}]"), t)?;
        }
    }
    if h.views.len() > 8 {
        return Err(format!("views: at most 8 entries, got {}", h.views.len()));
    }
    for (i, v) in h.views.iter().enumerate() {
        let f = |k: &str| format!("views[{i}].{k}");
        name(&f("name"), &v.name)?;
        num(&f("from_deg"), v.from_deg, -360.0, 360.0)?;
        num(&f("distance_m"), v.distance_m, 0.0, 2000.0)?;
        num(&f("height_m"), v.height_m, 0.0, 2000.0)?;
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../examples/house.json");

    // rivet: verifies REQ-UI-006
    // rivet: verifies DD-017
    #[test]
    fn the_example_model_is_accepted_and_round_trips() {
        let h = parse(EXAMPLE).unwrap();
        assert!(h.levels.len() >= 2 && h.rooms.len() >= 4);
        assert!(h.rooms.iter().any(|r| r.deck));
        assert!(h.rooms.iter().any(|r| !r.haus_rooms.is_empty()));
        assert!(!h.buildings.is_empty() && h.site.is_some() && !h.views.is_empty());
        let again = parse(&serde_json::to_string(&h).unwrap()).unwrap();
        assert_eq!(again, h);
    }

    /// One way to break the example.
    type Edit = Box<dyn Fn(&mut serde_json::Value)>;

    // rivet: verifies REQ-UI-006
    // rivet: verifies DD-017
    #[test]
    fn a_bad_model_is_refused_naming_the_field() {
        let cases: Vec<(&str, Edit, &str)> = vec![
            ("version", Box::new(|v| v["version"] = 2.into()), "version"),
            ("unknown key", Box::new(|v| v["extra"] = 1.into()), "extra"),
            (
                "unknown room key",
                Box::new(|v| v["rooms"][0]["temp"] = 1.into()),
                "temp",
            ),
            (
                "no levels",
                Box::new(|v| v["levels"] = serde_json::json!([])),
                "levels",
            ),
            (
                "duplicate level",
                Box::new(|v| v["levels"][1]["id"] = v["levels"][0]["id"].clone()),
                "levels[1].id",
            ),
            (
                "unknown level",
                Box::new(|v| v["rooms"][0]["level"] = "XX".into()),
                "rooms[0].level",
            ),
            (
                "duplicate room",
                Box::new(|v| v["rooms"][1]["id"] = v["rooms"][0]["id"].clone()),
                "rooms[1].id",
            ),
            (
                "two points",
                Box::new(|v| v["rooms"][0]["poly"] = serde_json::json!([[0, 0], [1, 1]])),
                "rooms[0].poly",
            ),
            (
                "far point",
                Box::new(|v| v["rooms"][0]["poly"][0] = serde_json::json!([5000, 0])),
                "rooms[0].poly",
            ),
            (
                "blank name",
                Box::new(|v| v["rooms"][0]["name"] = " ".into()),
                "rooms[0].name",
            ),
            (
                "pitched roof without ridge",
                Box::new(|v| {
                    v["buildings"][0]["roof"] = "gable".into();
                    v["buildings"][0].as_object_mut().unwrap().remove("ridge_m");
                }),
                "buildings[0].ridge_m",
            ),
            (
                "ridge below the walls",
                Box::new(|v| {
                    v["buildings"][0]["roof"] = "gable".into();
                    v["buildings"][0]["ridge_m"] = 1.into();
                }),
                "buildings[0].ridge_m",
            ),
            (
                "unknown roof",
                Box::new(|v| v["buildings"][0]["roof"] = "dome".into()),
                "dome",
            ),
            (
                "dormer backwards",
                Box::new(|v| v["buildings"][0]["dormers"] = serde_json::json!([[3, 1]])),
                "buildings[0].dormers[0]",
            ),
            (
                "low neighbour",
                Box::new(|v| v["site"]["neighbours"][0]["height_m"] = 0.into()),
                "site.neighbours[0].height_m",
            ),
            (
                "street kind",
                Box::new(|v| v["site"]["streets"][0]["kind"] = "river".into()),
                "river",
            ),
            (
                "view name",
                Box::new(|v| v["views"][0]["name"] = "".into()),
                "views[0].name",
            ),
        ];
        let base: serde_json::Value = serde_json::from_str(EXAMPLE).unwrap();
        for (what, edit, field) in cases {
            let mut v = base.clone();
            edit(&mut v);
            let err = parse(&v.to_string()).expect_err(what);
            assert!(err.contains(field), "{what}: {err:?} does not name {field}");
        }
    }
}
