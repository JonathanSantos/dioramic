//! Font families of the text-mode SVG (T-314): every `font-family` ends with a generic family, and every
//! `<text>` in weight 500/600 sits inside a family that names the weight's static instance ("Inter Medium",
//! "Inter SemiBold"), since `"Inter"` + 500 renders Regular and + 600 renders Bold in resvg's font database.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use dioramic_core::svg::{SvgMode, SvgOptions, TextRendering};
use dioramic_core::{CompileOptions, compile_source, render_svg};
use dioramic_spec::SourceFormat;

const GENERIC_FAMILIES: [&str; 3] = ["sans-serif", "serif", "monospace"];

fn example_svgs() -> Vec<(String, String)> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.to_string_lossy().ends_with(".diorama.yaml"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).unwrap();
            let diagram = compile_source(&source, SourceFormat::Yaml, &CompileOptions::default()).unwrap();
            let options = SvgOptions { mode: SvgMode::Static, text: TextRendering::Text, ..SvgOptions::default() };
            (path.file_name().unwrap().to_string_lossy().into_owned(), render_svg(&diagram, &options))
        })
        .collect()
}

/// Value of `name="…"` inside one tag.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = start + tag[start..].find('"')?;
    Some(&tag[start..end])
}

/// Every tag of the document, in order.
fn tags(svg: &str) -> Vec<&str> {
    svg.split('<').skip(1).map(|chunk| chunk.split('>').next().unwrap_or_default()).collect()
}

#[test]
fn every_font_family_ends_with_a_generic_family() {
    for (name, svg) in example_svgs() {
        for tag in tags(&svg) {
            let Some(family) = attribute(tag, "font-family") else { continue };
            let last = family.rsplit(',').next().unwrap_or_default().trim();
            assert!(GENERIC_FAMILIES.contains(&last), "{name}: font-family=\"{family}\" has no generic fallback");
        }
    }
}

#[test]
fn medium_and_semibold_text_names_the_weight_instance() {
    for (name, svg) in example_svgs() {
        // Stack of the family in effect, one entry per open <g> (None = inherited).
        let mut families: Vec<Option<String>> = Vec::new();
        let mut root: Option<String> = None;
        for tag in tags(&svg) {
            if tag.starts_with("svg ") {
                root = attribute(tag, "font-family").map(str::to_owned);
            } else if tag.starts_with("g ") || tag == "g" {
                if !tag.ends_with('/') {
                    families.push(attribute(tag, "font-family").map(str::to_owned));
                }
            } else if tag == "/g" {
                families.pop();
            } else if tag.starts_with("text ") {
                let weight = attribute(tag, "font-weight").unwrap_or("400");
                let family = attribute(tag, "font-family")
                    .map(str::to_owned)
                    .or_else(|| families.iter().rev().flatten().next().cloned())
                    .or_else(|| root.clone())
                    .unwrap_or_default();
                let first = family.split(',').next().unwrap_or_default().trim().to_owned();
                match weight {
                    "500" => assert!(first.ends_with(" Medium"), "{name}: weight 500 text in \"{family}\""),
                    "600" => assert!(first.ends_with(" SemiBold"), "{name}: weight 600 text in \"{family}\""),
                    _ => {}
                }
            }
        }
    }
}
