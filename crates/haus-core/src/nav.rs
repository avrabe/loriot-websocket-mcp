//! The shared home link and app switcher ("nav v1", REQ-UI-009): one JSON
//! file written by the operator, read and checked once at startup. The
//! contract is warte's (`warte docs nav`, since warte v1.6.25); haus follows
//! it exactly.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub desc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Nav {
    pub version: u32,
    pub home: Link,
    pub apps: Vec<App>,
}

fn check_name(field: &str, s: &str) -> Result<(), String> {
    let n = s.trim().chars().count();
    if (1..=40).contains(&n) {
        Ok(())
    } else {
        Err(format!(
            "{field}: 1 to 40 characters after trimming, got {n}"
        ))
    }
}

fn check_url(field: &str, u: &str) -> Result<(), String> {
    let shape = (u.starts_with('/') && !u.starts_with("//")) || u.starts_with("https://");
    let clean = !u
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\'));
    if shape && clean {
        Ok(())
    } else {
        Err(format!(
            "{field}: {u:?} must be a path starting with \"/\" (not \"//\") or an https:// URL, without whitespace, quotes, <, > or \\"
        ))
    }
}

/// JSON types before serde sees the file: serde accepts an array as a
/// positional struct, and its type errors name a column, not the field.
fn check_types(v: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    fn object<'a>(field: &str, v: &'a Value) -> Result<&'a serde_json::Map<String, Value>, String> {
        v.as_object()
            .ok_or_else(|| format!("{field}: must be a JSON object, got {v}"))
    }
    fn strings(o: &serde_json::Map<String, Value>, at: &str, keys: &[&str]) -> Result<(), String> {
        for k in keys {
            match o.get(*k) {
                None | Some(Value::String(_)) => {}
                Some(Value::Null) if *k == "desc" => {}
                Some(other) => return Err(format!("{at}{k}: must be a string, got {other}")),
            }
        }
        Ok(())
    }
    let top = object("nav file", v)?;
    if let Some(ver) = top.get("version") {
        if ver.as_u64() != Some(1) {
            return Err(format!("version: must be the integer 1, got {ver}"));
        }
    }
    if let Some(home) = top.get("home") {
        strings(object("home", home)?, "home.", &["name", "url"])?;
    }
    if let Some(apps) = top.get("apps") {
        let list = apps
            .as_array()
            .ok_or_else(|| format!("apps: must be a JSON array, got {apps}"))?;
        for (i, a) in list.iter().enumerate() {
            let at = format!("apps[{i}]");
            strings(
                object(&at, a)?,
                &format!("{at}."),
                &["id", "name", "url", "desc"],
            )?;
        }
    }
    Ok(())
}

/// Parse and check a nav v1 file; `me` is this app's own id. Every error
/// names the field.
pub fn parse(json: &str, me: &str) -> Result<Nav, String> {
    // Types first (serde_json::Value keeps the last of duplicate keys, so the
    // typed parse below is what refuses duplicates, naming the key).
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    check_types(&value)?;
    let nav: Nav = serde_json::from_str(json).map_err(|e| e.to_string())?;
    check_name("home.name", &nav.home.name)?;
    check_url("home.url", &nav.home.url)?;
    if !(1..=20).contains(&nav.apps.len()) {
        return Err(format!("apps: 1 to 20 entries, got {}", nav.apps.len()));
    }
    let mut seen = std::collections::HashSet::new();
    for (i, a) in nav.apps.iter().enumerate() {
        let f = |k: &str| format!("apps[{i}].{k}");
        let id_ok = (1..=32).contains(&a.id.len())
            && a.id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err(format!(
                "{}: {:?} must be [a-z0-9-], 1 to 32 characters",
                f("id"),
                a.id
            ));
        }
        if !seen.insert(a.id.as_str()) {
            return Err(format!("{}: {:?} is not unique", f("id"), a.id));
        }
        check_name(&f("name"), &a.name)?;
        check_url(&f("url"), &a.url)?;
        if let Some(d) = &a.desc {
            let n = d.chars().count();
            if n > 120 {
                return Err(format!("{}: at most 120 characters, got {n}", f("desc")));
            }
        }
    }
    if !nav.apps.iter().any(|a| a.id == me) {
        return Err(format!("self: {me:?} is not one of the app ids"));
    }
    Ok(nav)
}

/// The switcher's HTML: "← home", then the apps in file order; this app's
/// own entry is marked aria-current="page" and stays a link, so a page has
/// exactly 1 + apps links. Everything is escaped.
pub fn render(nav: &Nav, me: &str, esc: impl Fn(&str) -> String) -> String {
    let mut s = format!(
        r#"<nav class="apps"><a class="home" href="{}">← {}</a>"#,
        esc(&nav.home.url),
        esc(&nav.home.name)
    );
    for a in &nav.apps {
        let title = a
            .desc
            .as_deref()
            .map(|d| format!(r#" title="{}""#, esc(d)))
            .unwrap_or_default();
        let current = if a.id == me {
            r#" aria-current="page""#
        } else {
            ""
        };
        s += &format!(
            r#"<a href="{}"{current}{title}>{}</a>"#,
            esc(&a.url),
            esc(&a.name)
        );
    }
    s + "</nav>"
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = r#"{"version": 1, "home": {"name": "host", "url": "/"},
      "apps": [{"id": "warte", "name": "warte", "url": "/warte/", "desc": "CI"},
               {"id": "haus", "name": "haus", "url": "/haus/"},
               {"id": "login", "name": "Login", "url": "https://example.org:8444/"}]}"#;

    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    // rivet: verifies REQ-UI-009
    // rivet: verifies DD-016
    #[test]
    fn every_configured_link_renders_and_nothing_else() {
        let nav = parse(OK, "haus").unwrap();
        let html = render(&nav, "haus", esc);
        let hrefs: Vec<&str> = html
            .split("href=\"")
            .skip(1)
            .map(|r| r.split('"').next().unwrap())
            .collect();
        assert_eq!(
            hrefs,
            vec!["/", "/warte/", "/haus/", "https://example.org:8444/"]
        );
        assert_eq!(hrefs.len(), 1 + nav.apps.len());
        assert_eq!(html.matches("aria-current").count(), 1);
        assert!(
            html.contains(r#"<a href="/haus/" aria-current="page">haus</a>"#),
            "{html}"
        );
        assert!(html.contains("← host"));
        assert!(html.contains(r#"title="CI""#));
    }

    // rivet: verifies REQ-UI-009
    // rivet: verifies DD-016
    #[test]
    fn a_bad_file_is_refused_naming_the_field() {
        let bad = |from: &str, to: &str| parse(&OK.replace(from, to), "haus").unwrap_err();
        assert!(bad(r#""version": 1"#, r#""version": 2"#).starts_with("version"));
        assert!(bad(r#""desc": "CI""#, r#""desc": "CI", "icon": "x""#).contains("unknown field"));
        assert!(bad(r#""url": "/warte/""#, r#""url": "//evil/""#).starts_with("apps[0].url"));
        assert!(
            bad(r#""url": "/warte/""#, r#""url": "javascript:alert(1)""#)
                .starts_with("apps[0].url")
        );
        assert!(bad(r#""url": "/warte/""#, r#""url": "http://x/""#).starts_with("apps[0].url"));
        assert!(bad(r#""url": "/warte/""#, r#""url": "/a b/""#).starts_with("apps[0].url"));
        assert!(bad(r#""id": "warte""#, r#""id": "Warte""#).starts_with("apps[0].id"));
        assert!(bad(r#""id": "warte""#, r#""id": "haus""#).contains("not unique"));
        assert!(bad(r#""name": "host""#, r#""name": """#).starts_with("home.name"));
        assert!(bad(r#""name": "host""#, r#""name": "   ""#).starts_with("home.name"));
        assert!(
            bad(r#""url": "/warte/""#, "\"url\": \"/war\\u0007te/\"").starts_with("apps[0].url")
        );
        assert!(bad(
            r#""desc": "CI""#,
            &format!(r#""desc": "{}""#, "x".repeat(121))
        )
        .starts_with("apps[0].desc"));
        assert!(parse(OK, "photos").unwrap_err().starts_with("self"));
        assert!(parse(
            r#"{"version": 1, "home": {"name": "h", "url": "/"}, "apps": []}"#,
            "haus"
        )
        .unwrap_err()
        .starts_with("apps"));
    }

    /// One way to break the example file.
    type Edit = Box<dyn Fn(&mut serde_json::Value)>;

    // The refusal table of warte's tests/nav.rs at pulseengine/warte
    // ec989cb (v1.6.25), case for case, so both apps refuse the same files.
    // Here apps[0] is this app ("haus"), as "warte" is apps[0] there.
    // rivet: verifies REQ-UI-009
    // rivet: verifies DD-016
    #[test]
    fn warte_refusal_cases_are_refused_here_too() {
        let example = || -> serde_json::Value {
            serde_json::json!({"version": 1, "home": {"name": "tower", "url": "/"},
              "apps": [{"id": "haus", "name": "haus", "url": "/haus/", "desc": "climate"},
                       {"id": "photos", "name": "photos", "url": "/photos/"},
                       {"id": "login", "name": "login", "url": "https://login.example.org/"}]})
        };
        let check = |v: &serde_json::Value| parse(&v.to_string(), "haus");
        let long = "x".repeat(41);
        let many: Vec<serde_json::Value> = (0..21)
            .map(|i| serde_json::json!({"id": format!("a{i}"), "name": "a", "url": "/"}))
            .collect();
        let cases: Vec<(&str, Edit, &str)> = vec![
            (
                "version 2",
                Box::new(|v| v["version"] = 2.into()),
                "version",
            ),
            (
                "unknown top key",
                Box::new(|v| v["extra"] = 1.into()),
                "extra",
            ),
            (
                "unknown home key",
                Box::new(|v| v["home"]["x"] = 1.into()),
                "x",
            ),
            (
                "unknown app key",
                Box::new(|v| v["apps"][0]["icon"] = "i".into()),
                "icon",
            ),
            (
                "missing url",
                Box::new(|v| {
                    v["apps"][0].as_object_mut().unwrap().remove("url");
                }),
                "url",
            ),
            (
                "uppercase id",
                Box::new(|v| v["apps"][0]["id"] = "Haus".into()),
                "apps[0].id",
            ),
            (
                "empty id",
                Box::new(|v| v["apps"][0]["id"] = "".into()),
                "apps[0].id",
            ),
            (
                "long id",
                Box::new(|v| v["apps"][0]["id"] = "a".repeat(33).into()),
                "apps[0].id",
            ),
            (
                "duplicate id",
                Box::new(|v| v["apps"][1]["id"] = "haus".into()),
                "apps[1].id",
            ),
            (
                "blank name",
                Box::new(|v| v["apps"][1]["name"] = "  ".into()),
                "apps[1].name",
            ),
            (
                "long name",
                Box::new(move |v| v["apps"][1]["name"] = long.clone().into()),
                "apps[1].name",
            ),
            (
                "long home name",
                Box::new(|v| v["home"]["name"] = "y".repeat(41).into()),
                "home.name",
            ),
            (
                "protocol-relative",
                Box::new(|v| v["apps"][1]["url"] = "//evil.example/".into()),
                "apps[1].url",
            ),
            (
                "http",
                Box::new(|v| v["apps"][1]["url"] = "http://a.example/".into()),
                "apps[1].url",
            ),
            (
                "javascript",
                Box::new(|v| v["apps"][1]["url"] = "javascript:alert(1)".into()),
                "apps[1].url",
            ),
            (
                "relative",
                Box::new(|v| v["apps"][1]["url"] = "photos/".into()),
                "apps[1].url",
            ),
            (
                "space",
                Box::new(|v| v["apps"][1]["url"] = "/a b".into()),
                "apps[1].url",
            ),
            (
                "quote",
                Box::new(|v| v["apps"][1]["url"] = "/a\"b".into()),
                "apps[1].url",
            ),
            (
                "angle",
                Box::new(|v| v["apps"][1]["url"] = "/a<b".into()),
                "apps[1].url",
            ),
            (
                "home url",
                Box::new(|v| v["home"]["url"] = "//x".into()),
                "home.url",
            ),
            (
                "long desc",
                Box::new(|v| v["apps"][0]["desc"] = "d".repeat(121).into()),
                "apps[0].desc",
            ),
            (
                "no apps",
                Box::new(|v| v["apps"] = serde_json::json!([])),
                "apps",
            ),
            (
                "21 apps",
                Box::new(move |v| v["apps"] = many.clone().into()),
                "apps",
            ),
            (
                "self absent",
                Box::new(|v| v["apps"][0]["id"] = "other".into()),
                "self",
            ),
        ];
        assert_eq!(cases.len(), 24);
        check(&example()).unwrap();
        for (what, edit, field) in cases {
            let mut v = example();
            edit(&mut v);
            let err = check(&v).expect_err(what);
            assert!(err.contains(field), "{what}: {err:?} does not name {field}");
        }
        // The limits themselves pass.
        let mut v = example();
        v["apps"][1]["name"] = "n".repeat(40).into();
        v["apps"][1]["id"] = "a".repeat(32).into();
        v["apps"][1]["desc"] = "d".repeat(120).into();
        check(&v).unwrap();
    }

    // The nav v1 rules warte added for v1.7.0 (stated by warte's
    // conformance probe; its refusal cases are not on warte's main yet, so
    // these are written from the contract text, not mirrored).
    // rivet: verifies REQ-UI-009
    #[test]
    fn contract_additions_of_warte_1_7_are_refused_here() {
        let h = r#""home": {"name": "h", "url": "/"}"#;
        let app = r#"{"id": "haus", "name": "haus", "url": "/haus/"}"#;
        let file = |version: &str, home: &str, apps: &str| {
            format!(r#"{{"version": {version}, {home}, "apps": [{apps}]}}"#)
        };
        let cases: Vec<(&str, String, &str)> = vec![
            (
                "backslash path",
                file(
                    "1",
                    h,
                    r#"{"id": "haus", "name": "haus", "url": "/\\evil.example"}"#,
                ),
                "apps[0].url",
            ),
            (
                "backslash https",
                file(
                    "1",
                    h,
                    r#"{"id": "haus", "name": "haus", "url": "https://a.example\\b"}"#,
                ),
                "apps[0].url",
            ),
            (
                "backslash home",
                file("1", r#""home": {"name": "h", "url": "/\\x"}"#, app),
                "home.url",
            ),
            (
                "file as array",
                format!(r#"[1, {{"name": "h", "url": "/"}}, [{app}]]"#),
                "nav file",
            ),
            (
                "home as array",
                file("1", r#""home": ["h", "/"]"#, app),
                "home",
            ),
            (
                "app as array",
                file("1", h, r#"["haus", "haus", "/haus/"]"#),
                "apps[0]",
            ),
            (
                "apps as object",
                format!(r#"{{"version": 1, {h}, "apps": {app}}}"#),
                "apps",
            ),
            ("version 1.0", file("1.0", h, app), "version"),
            ("version string", file(r#""1""#, h, app), "version"),
            (
                "duplicate key",
                file(
                    "1",
                    h,
                    r#"{"id": "haus", "name": "a", "name": "b", "url": "/"}"#,
                ),
                "name",
            ),
            (
                "duplicate top key",
                format!(r#"{{"version": 1, "version": 1, {h}, "apps": [{app}]}}"#),
                "version",
            ),
            (
                "name type",
                file("1", h, r#"{"id": "haus", "name": 5, "url": "/"}"#),
                "apps[0].name",
            ),
            (
                "home url type",
                file("1", r#""home": {"name": "h", "url": 1}"#, app),
                "home.url",
            ),
            (
                "desc type",
                file(
                    "1",
                    h,
                    r#"{"id": "haus", "name": "haus", "url": "/", "desc": 3}"#,
                ),
                "apps[0].desc",
            ),
            (
                "41 code points",
                file(
                    "1",
                    h,
                    &format!(
                        r#"{{"id": "haus", "name": "{}", "url": "/"}}"#,
                        "é".repeat(41)
                    ),
                ),
                "apps[0].name",
            ),
        ];
        for (what, json, field) in cases {
            let err = parse(&json, "haus").expect_err(what);
            assert!(err.contains(field), "{what}: {err:?} does not name {field}");
        }
        // What the contract allows still passes.
        let ok = [
            file(
                "1",
                h,
                r#"{"id": "haus", "name": "haus", "url": "/", "desc": null}"#,
            ),
            file(
                "1",
                h,
                &format!(
                    r#"{{"id": "haus", "name": "{}", "url": "/"}}"#,
                    "é".repeat(40)
                ),
            ),
            file(
                "1",
                h,
                &format!(
                    r#"{{"id": "haus", "name": "h", "url": "/", "desc": "{}"}}"#,
                    "ü".repeat(120)
                ),
            ),
        ];
        for json in ok {
            parse(&json, "haus").unwrap();
        }
    }
}
