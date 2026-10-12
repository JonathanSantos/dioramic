//! QA (T-301): table-driven checks that every diagnostic has a stable code and a path (or a source location),
//! and that unknown references suggest a fix for each reference kind.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use dioramic_spec::{Diagnostic, Diagnostics, SourceFormat};

fn diagnostics_of(yaml: &str) -> Diagnostics {
    match dioramic_spec::load(yaml, SourceFormat::Yaml) {
        Ok(validated) => validated.warnings,
        Err(diagnostics) => diagnostics,
    }
}

fn find<'a>(diagnostics: &'a Diagnostics, code: &str) -> Option<&'a Diagnostic> {
    diagnostics.iter().find(|diagnostic| diagnostic.code.as_str() == code)
}

/// `(spec, expected code, expected path prefix)`; `""` means "only a source location is required" (E_PARSE).
const CASES: &[(&str, &str, &str)] = &[
    ("diorama: 1\nnodes: [ { id: a, bogus: 1 } ]\n", "E_PARSE", ""),
    ("nodes: [ { id: a } ]\n", "E_VERSION", "diorama"),
    ("diorama: 2\nnodes: [ { id: a } ]\n", "E_VERSION", "diorama"),
    ("diorama: 1\nnodes: []\n", "E_NO_NODES", "nodes"),
    ("diorama: 1\nnodes: [ { id: a }, { id: b } ]\nedges: [ 'a => b' ]\n", "E_EDGE_SYNTAX", "edges[0]"),
    ("diorama: 1\nnodes: [ { id: 'Bad Id' } ]\n", "E_BAD_ID", "nodes[0]"),
    ("diorama: 1\nnodes: [ { id: a }, { id: a } ]\n", "E_DUPLICATE_ID", "nodes["),
    ("diorama: 1\nnodes: [ { id: api }, { id: db } ]\nedges: [ 'api -> dbb' ]\n", "E_UNKNOWN_REF", "edges[0]"),
    (
        "diorama: 1\nzones: [ { id: a, parent: b }, { id: b, parent: a } ]\nnodes: [ { id: n, zone: a } ]\n",
        "E_ZONE_CYCLE",
        "zones[",
    ),
    ("diorama: 1\nnodes: [ { id: a, color: '#zzzzzz' } ]\n", "E_BAD_COLOR", "nodes[0]"),
    ("diorama: 1\nnodes: [ { id: a, replicas: 0 } ]\n", "E_BAD_VALUE", "nodes[0]"),
    ("diorama: 1\nlayout: { wrap: 0 }\nnodes: [ { id: a } ]\n", "E_BAD_VALUE", "layout"),
    ("diorama: 1\nnodes: [ { id: a }, { id: b } ]\nedges: [ 'a -> a' ]\n", "W_SELF_LOOP", "edges[0]"),
    (
        "diorama: 1\nnodes: [ { id: a, label: 'A label that is clearly longer than twenty eight' } ]\n",
        "W_LABEL_LONG",
        "nodes[0]",
    ),
    ("diorama: 1\nnodes: [ { id: a }, { id: b }, { id: c } ]\nedges: [ 'a -> b' ]\n", "W_ISOLATED", "nodes[2]"),
];

#[test]
fn every_case_reports_its_code_with_a_path_or_location() {
    for (spec, code, path) in CASES {
        let diagnostics = diagnostics_of(spec);
        let diagnostic =
            find(&diagnostics, code).unwrap_or_else(|| panic!("{code} expected for:\n{spec}\ngot: {diagnostics}"));
        assert!(!diagnostic.message.is_empty(), "{code}: empty message");
        if path.is_empty() {
            assert!(diagnostic.location.is_some(), "{code}: no source location");
        } else {
            assert!(diagnostic.path.starts_with(path), "{code}: path '{}' should start with '{path}'", diagnostic.path);
        }
    }
}

#[test]
fn every_diagnostic_of_every_case_is_addressable() {
    for (spec, _, _) in CASES {
        for diagnostic in diagnostics_of(spec).iter() {
            let addressable = !diagnostic.path.is_empty() || diagnostic.location.is_some();
            assert!(addressable, "diagnostic without path or location for:\n{spec}\n{diagnostic:?}");
        }
    }
}

#[test]
fn too_many_nodes_warns_and_more_than_500_is_a_limit_error() {
    let spec = |count: usize| {
        let nodes: Vec<String> = (0..count).map(|index| format!("  - {{ id: n{index} }}")).collect();
        format!("diorama: 1\nnodes:\n{}\n", nodes.join("\n"))
    };
    assert!(find(&diagnostics_of(&spec(41)), "W_TOO_MANY_NODES").is_some());
    assert!(find(&diagnostics_of(&spec(501)), "E_LIMIT").is_some());
    assert!(find(&diagnostics_of(&spec(40)), "W_TOO_MANY_NODES").is_none());
}

/// Each reference kind of the T-101 acceptance criteria gets a "did you mean" hint.
const UNKNOWN_REFS: &[(&str, &str)] = &[
    ("diorama: 1\nnodes: [ { id: api }, { id: db } ]\nedges: [ 'appi -> db' ]\n", "api"),
    ("diorama: 1\nnodes: [ { id: api }, { id: db } ]\nedges: [ 'api -> dbb' ]\n", "db"),
    ("diorama: 1\nzones: [ { id: backend } ]\nnodes: [ { id: api, zone: backend2 } ]\n", "backend"),
    ("diorama: 1\nzones: [ { id: cloud }, { id: vpc, parent: clowd } ]\nnodes: [ { id: api, zone: vpc } ]\n", "cloud"),
    (
        "diorama: 1\nstyles: { trunk: { color: '#ff0000' } }\nnodes: [ { id: a }, { id: b } ]\nedges: [ 'a -> b @trunc' ]\n",
        "trunk",
    ),
    (
        "diorama: 1\nnodes: [ { id: api }, { id: db } ]\nedges: [ 'api -> db' ]\nanimation: { preset: story, steps: [ { path: [api, dbx] } ] }\n",
        "db",
    ),
];

#[test]
fn unknown_references_suggest_the_closest_id() {
    for (spec, suggestion) in UNKNOWN_REFS {
        let diagnostics = diagnostics_of(spec);
        let diagnostic = find(&diagnostics, "E_UNKNOWN_REF")
            .unwrap_or_else(|| panic!("E_UNKNOWN_REF expected for:\n{spec}\n{diagnostics}"));
        let hint = diagnostic.hint.clone().unwrap_or_default();
        assert!(
            hint.contains(&format!("'{suggestion}'")) || hint.contains(suggestion),
            "hint for:\n{spec}\nshould suggest '{suggestion}', got '{hint}'"
        );
    }
}
