//! Host test: the stylesheet's colour tokens keep text readable (WCAG contrast) in both themes.
//! It reads `styles/app.css` itself, so a token change that drops below the minimum fails `cargo test`.

use std::collections::BTreeMap;

const STYLESHEET: &str = include_str!("../styles/app.css");

/// Text that must be read: WCAG AA for normal text.
const TEXT: f64 = 4.5;
/// Large or non-essential text, icons and focus indicators: WCAG 1.4.11 / 2.4.11.
const GRAPHIC: f64 = 3.0;

/// `--name: #rrggbb;` declarations of the block that starts with `selector {`.
fn tokens(selector: &str) -> BTreeMap<String, String> {
    let start = STYLESHEET.find(&format!("{selector} {{")).unwrap_or_else(|| panic!("no block {selector}"));
    let body = &STYLESHEET[start..];
    let body = &body[..body.find("\n}").unwrap_or(body.len())];
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, value) = line.strip_prefix("--")?.split_once(':')?;
            let value = value.split(';').next()?.trim();
            Some((name.trim().to_owned(), value.to_owned()))
        })
        .collect()
}

/// Resolves `var(--x)` through the theme block, then the light defaults.
fn colour(theme: &BTreeMap<String, String>, defaults: &BTreeMap<String, String>, name: &str) -> String {
    let value = theme.get(name).or_else(|| defaults.get(name)).unwrap_or_else(|| panic!("no token --{name}"));
    match value.strip_prefix("var(--").and_then(|rest| rest.strip_suffix(')')) {
        Some(inner) => colour(theme, defaults, inner),
        None => value.clone(),
    }
}

fn luminance(hex: &str) -> f64 {
    let hex = hex.trim_start_matches('#');
    assert_eq!(hex.len(), 6, "expected #rrggbb, got {hex}");
    let channel = |index: usize| {
        let value = f64::from(u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap_or(0)) / 255.0;
        if value <= 0.039_28 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2)
}

fn ratio(first: &str, second: &str) -> f64 {
    let (a, b) = (luminance(first), luminance(second));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// (foreground token, background token, minimum).
const PAIRS: [(&str, &str, f64); 16] = [
    ("ink", "surface", TEXT),
    ("ink-2", "chrome", TEXT),
    ("ink-3", "chrome", TEXT),
    ("ink-3", "surface", TEXT),
    ("ink-3", "sunken", TEXT),
    ("ink-4", "chrome", GRAPHIC),
    ("ink-4", "surface", GRAPHIC),
    ("t-comment", "surface", TEXT),
    ("t-punct", "surface", GRAPHIC),
    ("t-key", "surface", TEXT),
    ("error", "surface", TEXT),
    ("warning", "surface", TEXT),
    ("accent", "chrome", GRAPHIC),
    ("accent", "surface", GRAPHIC),
    ("accent", "stage", GRAPHIC),
    ("sheet-ring", "stage", 1.15),
];

fn check(theme_name: &str, theme: &BTreeMap<String, String>, defaults: &BTreeMap<String, String>) -> Vec<String> {
    PAIRS
        .iter()
        .filter_map(|&(foreground, background, minimum)| {
            let value = ratio(&colour(theme, defaults, foreground), &colour(theme, defaults, background));
            (value < minimum)
                .then(|| format!("{theme_name}: --{foreground} on --{background} is {value:.2}:1 < {minimum}:1"))
        })
        .collect()
}

#[test]
fn light_tokens_meet_contrast_minimums() {
    let light = tokens(":root");
    let failures = check("light", &light, &light);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn dark_tokens_meet_contrast_minimums() {
    let light = tokens(":root");
    let dark = tokens(":root[data-theme=\"dark\"]");
    let failures = check("dark", &dark, &light);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn both_dark_blocks_agree() {
    // The OS-dark block (inside the media query) must mirror the forced-dark one.
    let forced = tokens(":root[data-theme=\"dark\"]");
    let os = tokens("  :root:not([data-theme=\"light\"])");
    for (name, value) in &forced {
        assert_eq!(os.get(name), Some(value), "--{name} differs between the two dark blocks");
    }
}

#[test]
fn hierarchy_stays_visible() {
    for theme in [tokens(":root"), tokens(":root[data-theme=\"dark\"]")] {
        let light = tokens(":root");
        let on = |name| ratio(&colour(&theme, &light, name), &colour(&theme, &light, "surface"));
        assert!(on("ink") > on("ink-2") && on("ink-2") > on("ink-3") && on("ink-3") > on("ink-4"));
    }
}
