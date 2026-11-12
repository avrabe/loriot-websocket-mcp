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
        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>'));
    if shape && clean {
        Ok(())
    } else {
        Err(format!(
            "{field}: {u:?} must be a path starting with \"/\" (not \"//\") or an https:// URL, without whitespace, quotes, < or >"
        ))
    }
}

/// Parse and check a nav v1 file; `me` is this app's own id. Every error
/// names the field.
pub fn parse(json: &str, me: &str) -> Result<Nav, String> {
    let nav: Nav = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if nav.version != 1 {
        return Err(format!("version: must be 1, got {}", nav.version));
    }
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
}
