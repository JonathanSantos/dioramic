//! QA (T-203): "Fixar" writes the view into the spec text with surgical edits. Proves on every
//! example and on edge-case layouts that comments and formatting survive, that the result is
//! deterministic and idempotent, and that the engine then renders exactly the pinned view.

use std::fs;
use std::path::Path;

use dioramic_desktop::icons::IconLibrary;
use dioramic_desktop::pin::pin_view;
use dioramic_desktop::preview::render_preview;
use dioramic_desktop_ipc::{
    DiagramTheme, GridStyle, PreviewRequest, PreviewStatus, ResolvedView, ViewDensity, ViewGrid, ViewLayout,
    ViewRotation, ViewRouting, ViewState,
};
use dioramic_render::Engine;

fn examples() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples");
    let mut specs: Vec<(String, String)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".diorama.yaml"))
        .filter_map(|path| Some((path.file_name()?.to_string_lossy().into_owned(), fs::read_to_string(&path).ok()?)))
        .collect();
    specs.sort();
    specs
}

fn views() -> Vec<ViewState> {
    vec![
        ViewState { rotation: Some(ViewRotation::Deg90), ..ViewState::default() },
        ViewState { density: Some(ViewDensity::Compact), ..ViewState::default() },
        ViewState { theme: Some(DiagramTheme::Mono), ..ViewState::default() },
        ViewState {
            rotation: Some(ViewRotation::Deg270),
            density: Some(ViewDensity::Spaced),
            theme: Some(DiagramTheme::Paper),
            ..ViewState::default()
        },
        ViewState {
            rotation: Some(ViewRotation::Deg180),
            density: Some(ViewDensity::Compact),
            theme: Some(DiagramTheme::Midnight),
            ..ViewState::default()
        },
        ViewState { routing: Some(ViewRouting::Rails), ..ViewState::default() },
        ViewState { layout: Some(ViewLayout::Hub), ..ViewState::default() },
        ViewState { layout: Some(ViewLayout::Flow), theme: Some(DiagramTheme::Schematic), ..ViewState::default() },
        ViewState {
            routing: Some(ViewRouting::Rails),
            grid: Some(ViewGrid { size: 0.5, style: GridStyle::Dots, opacity: 0.6 }),
            ..ViewState::default()
        },
        ViewState {
            density: Some(ViewDensity::Spaced),
            grid: Some(ViewGrid { size: 2.0, style: GridStyle::None, opacity: 1.0 }),
            ..ViewState::default()
        },
    ]
}

/// Resolved view of a text rendered with no overrides (the spec alone decides).
fn spec_view(text: &str) -> Option<ResolvedView> {
    // No pack store: the tests never read the real data folder.
    let request = PreviewRequest { text: text.to_owned(), view: ViewState::default(), spec_path: None, motion: false };
    let response = render_preview(Engine::shared(), &IconLibrary::default(), &request);
    assert_eq!(response.status, PreviewStatus::Ready, "{:?}\n{text}", response.diagnostics);
    response.view
}

fn degrees(rotation: ViewRotation) -> Option<u16> {
    match rotation {
        ViewRotation::Deg0 => Some(0),
        ViewRotation::Deg90 => Some(90),
        ViewRotation::Deg180 => Some(180),
        ViewRotation::Deg270 => Some(270),
        ViewRotation::Auto => None,
    }
}

fn assert_view_applied(name: &str, pinned: &str, view: &ViewState) {
    let resolved = spec_view(pinned).unwrap_or_else(|| panic!("{name}: no resolved view"));
    if let Some(expected) = view.rotation.and_then(degrees) {
        assert_eq!(resolved.rotation_degrees, expected, "{name} {view:?}\n{pinned}");
    }
    if let Some(density) = view.density {
        assert_eq!(resolved.density, density, "{name} {view:?}\n{pinned}");
    }
    if let Some(theme) = view.theme {
        assert_eq!(resolved.theme, theme, "{name} {view:?}\n{pinned}");
    }
    if let Some(routing) = view.routing {
        assert_eq!(resolved.routing, routing, "{name} {view:?}\n{pinned}");
    }
    if let Some(layout) = view.layout {
        assert_eq!(resolved.layout, layout, "{name} {view:?}\n{pinned}");
    }
    if let Some(grid) = view.grid {
        assert_eq!(resolved.grid, grid, "{name} {view:?}\n{pinned}");
    }
}

/// Every original line survives (in order) except lines that are a `key: value` edit of the same key.
fn assert_surgical(name: &str, original: &str, pinned: &str) {
    let before: Vec<&str> = original.lines().collect();
    let after: Vec<&str> = pinned.lines().collect();
    assert!(after.len() <= before.len() + 3, "{name}: more than 3 lines inserted");
    let mut cursor = 0;
    let mut edited = 0;
    for line in &before {
        let found = after[cursor..].iter().position(|candidate| candidate == line);
        match found {
            Some(offset) => cursor += offset + 1,
            None => {
                edited += 1;
                assert!(
                    !line.trim_start().starts_with('#'),
                    "{name}: comment line lost or changed: {line:?}\n--- pinned ---\n{pinned}"
                );
                if let Some((_, comment)) = line.split_once(" #") {
                    assert!(pinned.contains(&format!("#{comment}")), "{name}: inline comment lost: {line:?}");
                }
            }
        }
    }
    assert!(edited <= 3, "{name}: {edited} original lines changed (only rotation/density/theme may)");
}

#[test]
fn pin_on_every_example_is_surgical_idempotent_deterministic_and_rendered() {
    for (name, original) in examples() {
        for view in views() {
            let pinned = pin_view(&original, &view);
            assert_eq!(pinned, pin_view(&original, &view), "{name}: pin is not deterministic");
            assert_eq!(pin_view(&pinned, &view), pinned, "{name}: pin is not idempotent for {view:?}");
            assert_surgical(&name, &original, &pinned);
            assert_view_applied(&name, &pinned, &view);
            assert_eq!(pinned.ends_with('\n'), original.ends_with('\n'), "{name}: trailing newline changed");
        }
    }
}

#[test]
fn pin_keeps_every_comment_of_every_example() {
    for (name, original) in examples() {
        let view = ViewState {
            rotation: Some(ViewRotation::Auto),
            density: Some(ViewDensity::Compact),
            theme: Some(DiagramTheme::Midnight),
            ..ViewState::default()
        };
        let pinned = pin_view(&original, &view);
        let comments = |text: &str| -> Vec<String> {
            text.lines().filter_map(|line| line.find('#').map(|at| line[at..].to_owned())).collect()
        };
        assert_eq!(comments(&original), comments(&pinned), "{name}: comments differ after pin");
    }
}

#[test]
fn repinning_a_different_view_converges() {
    for (name, original) in examples() {
        let first = ViewState { rotation: Some(ViewRotation::Deg90), ..ViewState::default() };
        let second = ViewState { rotation: Some(ViewRotation::Deg180), theme: Some(DiagramTheme::Paper), ..first };
        let twice = pin_view(&pin_view(&original, &first), &second);
        let once = pin_view(&original, &second);
        assert_eq!(twice, once, "{name}: pin history leaks into the text");
    }
}

/// Edge-case layouts a human (or the AI) may write. Each pinned text must still be a valid spec
/// that renders with the pinned view, keeping the marker comment.
#[test]
fn pin_edge_cases_table() {
    let view = ViewState {
        rotation: Some(ViewRotation::Deg90),
        density: Some(ViewDensity::Compact),
        theme: Some(DiagramTheme::Midnight),
        ..ViewState::default()
    };
    let cases: [(&str, &str); 10] = [
        ("no header sections", "# keep\ndiorama: 1\nnodes:\n  - { id: a }\n"),
        (
            "block sections with comments",
            "diorama: 1\nview:   # camera keep\n  # keep\n  rotation: 0 # keep2\nlayout:\n  density: spaced\ntheme: blueprint\nnodes:\n  - { id: a }\n",
        ),
        ("flow sections", "diorama: 1\nview: { rotation: 0 } # keep\nlayout: {}\nnodes:\n  - { id: a }\n"),
        (
            "block scalar subtitle",
            "# keep\ndiorama: 1\ntitle: T\nsubtitle: >\n  long text\n  # not a comment, part of the scalar\nnodes:\n  - { id: a }\n",
        ),
        ("quoted theme", "diorama: 1\ntheme: \"blueprint\" # keep\nnodes:\n  - { id: a }\n"),
        ("crlf", "# keep\r\ndiorama: 1\r\nview:\r\n  rotation: 0\r\nnodes:\r\n  - { id: a }\r\n"),
        ("nested same-name keys", "diorama: 1\nnodes:\n  - id: a # keep\n    note: >\n      theme: x\n      view: y\n"),
        ("empty block section", "diorama: 1\nview:\n# keep\nnodes:\n  - { id: a }\n"),
        ("document marker", "# keep\n---\ndiorama: 1\nnodes:\n  - { id: a }\n"),
        ("no trailing newline", "# keep\ndiorama: 1\nnodes:\n  - { id: a }"),
    ];
    for (name, text) in cases {
        let pinned = pin_view(text, &view);
        assert!(pinned.contains("keep"), "{name}: marker comment lost\n{pinned}");
        assert_eq!(pin_view(&pinned, &view), pinned, "{name}: not idempotent\n{pinned}");
        assert_view_applied(name, &pinned, &view);
    }
}

/// A flow map split over several lines is valid YAML; pinning must not leave it half-edited.
#[test]
fn pin_handles_a_multiline_flow_map() {
    let text = "diorama: 1\nview: {\n  rotation: 0\n}\nnodes:\n  - { id: a }\n";
    let view = ViewState { rotation: Some(ViewRotation::Deg90), ..ViewState::default() };
    let pinned = pin_view(text, &view);
    assert_view_applied("multiline flow", &pinned, &view);
}

/// The tree lists `*.diorama.json` and the editor opens them; pinning must keep JSON valid.
#[test]
fn pin_keeps_json_specs_valid() {
    let text = "{\n  \"diorama\": 1,\n  \"nodes\": [{ \"id\": \"a\" }]\n}\n";
    let view =
        ViewState { rotation: Some(ViewRotation::Deg90), theme: Some(DiagramTheme::Paper), ..ViewState::default() };
    let pinned = pin_view(text, &view);
    assert!(serde_json::from_str::<serde_json::Value>(&pinned).is_ok(), "pinned JSON is not JSON:\n{pinned}");
    assert_view_applied("json", &pinned, &view);
}
