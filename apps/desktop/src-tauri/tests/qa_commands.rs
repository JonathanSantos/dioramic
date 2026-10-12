//! QA (T-203): the pure functions behind the Tauri commands, exercised end to end on the real
//! `examples/` with the real engine: preview, export, folder scan, watch and robustness to invalid
//! specs / engine errors.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use dioramic_desktop::diagnostics::SpecOutline;
use dioramic_desktop::error::CommandError;
use dioramic_desktop::export::{render_export, suggested_path, write_export};
use dioramic_desktop::icons::IconLibrary;
use dioramic_desktop::preview::render_preview;
use dioramic_desktop::watcher::{WatchBatch, watch};
use dioramic_desktop::workspace::{read_spec, save_spec, scan};
use dioramic_desktop_ipc::{
    DiagramTheme, ExportFormat, ExportRequest, FileKind, FileNode, PreviewRequest, PreviewResponse, PreviewStatus,
    Severity, ViewDensity, ViewRotation, ViewState,
};
use dioramic_render::Engine;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples")
}

fn examples() -> Vec<(String, String)> {
    let mut specs: Vec<(String, String)> = fs::read_dir(examples_dir())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".diorama.yaml"))
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((name, fs::read_to_string(&path).ok()?))
        })
        .collect();
    specs.sort();
    specs
}

fn preview(text: &str, view: ViewState) -> PreviewResponse {
    // No pack store: the tests never read the real data folder.
    render_preview(
        Engine::shared(),
        &IconLibrary::default(),
        &PreviewRequest { text: text.to_owned(), view, spec_path: None, motion: false },
    )
}

fn node_ids(text: &str) -> Vec<String> {
    let value: serde_json::Value = serde_saphyr::from_str(text).unwrap_or(serde_json::Value::Null);
    value["nodes"]
        .as_array()
        .map(|nodes| nodes.iter().filter_map(|node| node["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

// ── preview ─────────────────────────────────────────────────────────────────

#[test]
fn every_example_previews_ready_with_hit_testable_nodes_and_notes() {
    let specs = examples();
    assert!(specs.len() >= 6, "examples/ not found from {}", examples_dir().display());
    for (name, text) in specs {
        let response = preview(&text, ViewState::default());
        assert_eq!(response.status, PreviewStatus::Ready, "{name}: {:?}", response.diagnostics);
        let svg = response.svg.as_deref().unwrap_or_default();
        assert!(svg.starts_with("<svg") || svg.contains("<svg"), "{name}: not an SVG");
        assert!(svg.contains("<text"), "{name}: live preview must keep <text> (webview has Inter)");
        for id in node_ids(&text) {
            assert!(svg.contains(&format!("data-id=\"{id}\"")), "{name}: node {id} has no data-id in the SVG");
        }
        assert!(response.view.is_some(), "{name}: resolved view missing");
        for note in &response.notes {
            assert!(svg.contains(&format!("data-id=\"{}\"", note.id)), "{name}: note target {} not in SVG", note.id);
        }
        for diagnostic in &response.diagnostics {
            assert_ne!(diagnostic.severity, Severity::Error, "{name}: Ready with an error {diagnostic:?}");
            assert!(!diagnostic.code.is_empty(), "{name}: diagnostic without code");
        }
    }
}

#[test]
fn preview_is_deterministic() {
    for (name, text) in examples() {
        let view = ViewState { rotation: Some(ViewRotation::Auto), ..ViewState::default() };
        let first = preview(&text, view);
        let second = preview(&text, view);
        assert_eq!(first.svg, second.svg, "{name}: SVG differs between two renders");
        assert_eq!(first.diagnostics, second.diagnostics, "{name}");
        assert_eq!(first.view, second.view, "{name}");
        assert_eq!(first.notes, second.notes, "{name}");
    }
}

#[test]
fn view_overrides_are_resolved_by_the_engine() {
    let text = fs::read_to_string(examples_dir().join("checkout.diorama.yaml")).unwrap_or_default();
    let rotations =
        [(ViewRotation::Deg0, 0), (ViewRotation::Deg90, 90), (ViewRotation::Deg180, 180), (ViewRotation::Deg270, 270)];
    let densities = [ViewDensity::Spaced, ViewDensity::Compact];
    let themes = DiagramTheme::ALL;
    for (rotation, degrees) in rotations {
        for density in densities {
            for theme in themes {
                let view = ViewState {
                    rotation: Some(rotation),
                    density: Some(density),
                    theme: Some(theme),
                    ..ViewState::default()
                };
                let response = preview(&text, view);
                assert_eq!(response.status, PreviewStatus::Ready, "{view:?}");
                let resolved = response.view.unwrap_or_else(|| panic!("no resolved view for {view:?}"));
                assert_eq!(resolved.rotation_degrees, degrees, "{view:?}");
                assert_eq!(resolved.density, density, "{view:?}");
                assert_eq!(resolved.theme, theme, "{view:?}");
            }
        }
    }
    let auto = preview(&text, ViewState { rotation: Some(ViewRotation::Auto), ..ViewState::default() });
    let degrees = auto.view.map(|view| view.rotation_degrees);
    assert!(matches!(degrees, Some(0 | 90 | 180 | 270)), "auto resolved to {degrees:?}");
}

fn many_nodes(count: usize) -> String {
    let mut text = String::from("diorama: 1\nnodes:\n");
    for index in 0..count {
        text.push_str(&format!("  - {{ id: n{index} }}\n"));
    }
    text
}

/// Expected outcome of an invalid or hostile editor text.
enum Expect {
    /// Status `Invalid` with an error diagnostic carrying this code.
    Invalid(&'static str),
    /// Status `Invalid` with at least one error (any code).
    AnyError,
}

#[test]
fn invalid_specs_never_crash_and_always_explain() {
    let deep_nesting = format!("diorama: 1\nnodes: {}{}\n", "[".repeat(2_000), "]".repeat(2_000));
    let huge_label = format!("diorama: 1\nnodes:\n  - {{ id: a, label: \"{}\" }}\n", "x".repeat(20_000));
    let cases: Vec<(&str, String, Expect)> = vec![
        ("empty", String::new(), Expect::AnyError),
        ("blank", "   \n\n".into(), Expect::AnyError),
        ("comment only", "# nothing\n".into(), Expect::AnyError),
        ("control chars", "\u{0}\u{1}\u{7f}garbage".into(), Expect::AnyError),
        ("yaml sequence", "- 1\n- 2\n".into(), Expect::AnyError),
        ("scalar", "42".into(), Expect::AnyError),
        ("unclosed flow", "diorama: 1\nnodes: [\n".into(), Expect::Invalid("E_PARSE")),
        ("tab indentation", "diorama: 1\nnodes:\n\t- { id: a }\n".into(), Expect::AnyError),
        ("broken json", "{\"diorama\": 1, ".into(), Expect::Invalid("E_PARSE")),
        ("wrong version", "diorama: 2\nnodes:\n  - { id: a }\n".into(), Expect::Invalid("E_VERSION")),
        ("no nodes", "diorama: 1\nnodes: []\n".into(), Expect::Invalid("E_NO_NODES")),
        (
            "duplicate id",
            "diorama: 1\nnodes:\n  - { id: a }\n  - { id: a }\n".into(),
            Expect::Invalid("E_DUPLICATE_ID"),
        ),
        (
            "unknown ref",
            "diorama: 1\nnodes:\n  - { id: a }\nedges:\n  - a -> ghost\n".into(),
            Expect::Invalid("E_UNKNOWN_REF"),
        ),
        ("bad edge syntax", "diorama: 1\nnodes:\n  - { id: a }\nedges:\n  - a => \n".into(), Expect::AnyError),
        ("wrong type", "diorama: 1\nnodes: 7\n".into(), Expect::AnyError),
        ("too many nodes", many_nodes(501), Expect::Invalid("E_LIMIT")),
        ("deep nesting", deep_nesting, Expect::AnyError),
        ("huge label", huge_label, Expect::AnyError),
    ];
    for (name, text, expect) in cases {
        // The app renders on a blocking-pool thread; keep the same 2 MiB stack the runtime gives it.
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || preview(&text, ViewState::default()))
            .unwrap_or_else(|error| panic!("{name}: cannot spawn: {error}"));
        let response = handle.join().unwrap_or_else(|_| panic!("{name}: render_preview panicked"));
        let errors: Vec<_> = response.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
        match expect {
            Expect::Invalid(code) => {
                assert_eq!(response.status, PreviewStatus::Invalid, "{name}: {:?}", response.diagnostics);
                assert!(errors.iter().any(|d| d.code == code), "{name}: expected {code}, got {errors:?}");
            }
            Expect::AnyError => {
                if response.status == PreviewStatus::Ready {
                    // Accepted by the engine (e.g. a long label): it must at least be a real SVG.
                    assert!(response.svg.as_deref().is_some_and(|svg| svg.contains("<svg")), "{name}");
                    continue;
                }
                assert!(!errors.is_empty(), "{name}: status {:?} without an error diagnostic", response.status);
            }
        }
        assert!(response.svg.is_none(), "{name}: invalid spec still returned an SVG");
        for diagnostic in &response.diagnostics {
            assert!(
                diagnostic.code.starts_with("E_")
                    || diagnostic.code.starts_with("W_")
                    || diagnostic.code.starts_with("H_"),
                "{name}: unstable code {:?}",
                diagnostic.code
            );
            assert!(!diagnostic.message.is_empty(), "{name}: empty message");
        }
    }
}

#[test]
fn invalid_spec_diagnostics_point_at_the_element_and_the_line() {
    let text = "diorama: 1\nnodes:\n  - { id: api }\n  - { id: api }\n";
    let response = preview(text, ViewState::default());
    assert_eq!(response.status, PreviewStatus::Invalid);
    let duplicate = response.diagnostics.iter().find(|d| d.code == "E_DUPLICATE_ID");
    let duplicate = duplicate.unwrap_or_else(|| panic!("no E_DUPLICATE_ID in {:?}", response.diagnostics));
    assert!(duplicate.path.starts_with("nodes[1]"), "path {:?}", duplicate.path);
    assert_eq!(duplicate.target_id.as_deref(), Some("api"));
}

#[test]
fn parse_errors_carry_a_source_line_for_the_editor() {
    let response = preview("diorama: 1\nnodes:\n  - { id: a\n  - { id: b }\n", ViewState::default());
    assert_eq!(response.status, PreviewStatus::Invalid);
    let parse = response.diagnostics.iter().find(|d| d.code == "E_PARSE");
    let parse = parse.unwrap_or_else(|| panic!("no E_PARSE in {:?}", response.diagnostics));
    assert!(parse.line.is_some(), "E_PARSE without a line: {parse:?}");
}

#[test]
fn outline_tolerates_any_text() {
    for text in ["", "{", "- a", "nodes: 1", "\u{0}", "diorama: 1\nnodes:\n  - { id: a, note: \"  \" }\n"] {
        let outline = SpecOutline::of(text);
        for path in ["nodes[0]", "nodes[x]", "nodes[", "[0]", "zones[99999999999999999999]", "edges[0].from"] {
            let _ = outline.target_id(path);
        }
        assert!(outline.into_notes().is_empty(), "{text:?}: blank notes must be dropped");
    }
}

// ── export ──────────────────────────────────────────────────────────────────

fn export_request(text: &str, format: ExportFormat) -> ExportRequest {
    ExportRequest { job_id: 1, text: text.to_owned(), view: ViewState::default(), format, scale: 1.0, spec_path: None }
}

#[test]
fn export_writes_every_format_to_a_temporary_folder() -> TestResult {
    let dir = tempfile::tempdir()?;
    let spec_path = examples_dir().join("minimal.diorama.yaml");
    let text = fs::read_to_string(&spec_path)?;
    for format in ExportFormat::ALL {
        let (folder, file_name) = suggested_path(spec_path.to_str(), format);
        assert_eq!(folder.as_deref().map(Path::canonicalize).transpose()?, Some(examples_dir().canonicalize()?));
        assert_eq!(file_name, format!("minimal.{}", format.extension()));
        let bytes = render_export(Engine::shared(), &IconLibrary::default(), &export_request(&text, format))?;
        let target = dir.path().join("exports").join(&file_name);
        let written = write_export(&target, &bytes)?;
        assert_eq!(written, fs::metadata(&target)?.len());
        let on_disk = fs::read(&target)?;
        match format {
            ExportFormat::Png => assert!(on_disk.starts_with(b"\x89PNG\r\n\x1a\n"), "PNG magic"),
            ExportFormat::Gif => assert!(on_disk.starts_with(b"GIF89a"), "GIF magic"),
            ExportFormat::Svg => {
                let svg = String::from_utf8(on_disk)?;
                assert!(svg.contains("<svg"), "SVG root");
            }
        }
    }
    Ok(())
}

#[test]
fn export_is_deterministic() -> TestResult {
    let text = fs::read_to_string(examples_dir().join("checkout.diorama.yaml"))?;
    for format in [ExportFormat::Png, ExportFormat::Svg] {
        let first = render_export(Engine::shared(), &IconLibrary::default(), &export_request(&text, format))?;
        let second = render_export(Engine::shared(), &IconLibrary::default(), &export_request(&text, format))?;
        assert_eq!(first, second, "{format:?} export differs between runs");
    }
    Ok(())
}

#[test]
fn export_of_an_invalid_spec_is_an_engine_error_and_writes_nothing() {
    for text in ["", "diorama: 1\nnodes: [\n", "diorama: 1\nnodes:\n  - { id: a }\nedges:\n  - a -> b\n"] {
        for format in ExportFormat::ALL {
            match render_export(Engine::shared(), &IconLibrary::default(), &export_request(text, format)) {
                Err(CommandError::Engine(message)) => assert!(!message.is_empty()),
                other => panic!("{text:?} {format:?}: expected an engine error, got {:?}", other.map(|b| b.len())),
            }
        }
    }
}

#[test]
fn export_to_an_unwritable_place_is_an_io_error() -> TestResult {
    let dir = tempfile::tempdir()?;
    let blocker = dir.path().join("file");
    fs::write(&blocker, "x")?;
    let error = write_export(&blocker.join("out.png"), b"png");
    assert!(matches!(error, Err(CommandError::Io { .. })), "{error:?}");
    Ok(())
}

// ── workspace ───────────────────────────────────────────────────────────────

fn spec_paths(nodes: &[FileNode], out: &mut Vec<String>) {
    for node in nodes {
        match node.kind {
            FileKind::Spec => out.push(node.path.clone()),
            FileKind::Folder => spec_paths(&node.children, out),
        }
    }
}

/// Sorted names of the `*.diorama.yaml` files directly inside `dir`, read independently of `scan`.
fn spec_names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".diorama.yaml"))
        .collect();
    names.sort();
    names
}

fn child_names(node: &FileNode) -> Vec<&str> {
    node.children.iter().map(|child| child.name.as_str()).collect()
}

#[test]
fn scanning_examples_lists_every_spec_sorted_and_readable() -> TestResult {
    const SUBFOLDER: &str = "schematic";
    let workspace = scan(&examples_dir())?;
    let root_specs: Vec<String> = examples().into_iter().map(|(name, _)| name).collect();
    let subfolder_specs = spec_names_in(&examples_dir().join(SUBFOLDER));
    assert_eq!(subfolder_specs.len(), 10, "examples/{SUBFOLDER} should hold the 10 schematic specs");

    // Folders come first, then the root specs, each group sorted.
    let names: Vec<&str> = workspace.tree.iter().map(|node| node.name.as_str()).collect();
    let mut expected = vec![SUBFOLDER.to_owned()];
    expected.extend(root_specs.iter().cloned());
    assert_eq!(names, expected);

    let folder = &workspace.tree[0];
    assert_eq!(folder.kind, FileKind::Folder);
    assert_eq!(child_names(folder), subfolder_specs);
    assert!(folder.children.iter().all(|child| child.kind == FileKind::Spec && child.children.is_empty()));
    assert!(workspace.tree[1..].iter().all(|node| node.kind == FileKind::Spec));

    let total = root_specs.len() + subfolder_specs.len();
    assert_eq!(workspace.spec_count(), total);
    let mut paths = Vec::new();
    spec_paths(&workspace.tree, &mut paths);
    assert_eq!(paths.len(), total);
    for path in paths {
        assert!(!read_spec(Path::new(&path))?.is_empty(), "{path}");
    }
    Ok(())
}

#[test]
fn scanning_subfolders_and_symlink_loops_terminates() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fs::create_dir_all(root.join("a/b/c"))?;
    fs::write(root.join("a/b/c/deep.diorama.yaml"), "diorama: 1\n")?;
    fs::write(root.join("a/top.diorama.json"), "{}")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(root, root.join("a/loop"))?;
    let started = Instant::now();
    let workspace = scan(root)?;
    assert!(started.elapsed() < Duration::from_secs(5), "scan took {:?}", started.elapsed());
    let mut paths = Vec::new();
    spec_paths(&workspace.tree, &mut paths);
    assert!(paths.iter().any(|path| path.ends_with("deep.diorama.yaml")));
    assert!(paths.iter().any(|path| path.ends_with("top.diorama.json")));
    Ok(())
}

#[test]
fn opening_something_that_is_not_a_folder_fails_cleanly() {
    assert!(matches!(scan(Path::new("/definitely/not/here")), Err(CommandError::NotAFolder(_))));
}

#[test]
fn saving_keeps_text_byte_for_byte_and_reports_io_errors() -> TestResult {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("x.diorama.yaml");
    let text = "# top comment\r\ndiorama: 1   # trailing\r\n\r\nnodes:\r\n  - { id: a }  # é ✓\r\n";
    save_spec(&file, text)?;
    assert_eq!(fs::read(&file)?, text.as_bytes());
    assert_eq!(read_spec(&file)?, text);
    let missing = dir.path().join("missing/y.diorama.yaml");
    assert!(matches!(save_spec(&missing, text), Err(CommandError::Io { .. })));
    assert!(matches!(read_spec(&missing), Err(CommandError::Io { .. })));
    Ok(())
}

// ── watch ───────────────────────────────────────────────────────────────────

/// Collects batches until `done` holds or the timeout expires.
fn collect_until(receiver: &mpsc::Receiver<WatchBatch>, done: impl Fn(&WatchBatch) -> bool) -> WatchBatch {
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut merged = WatchBatch::default();
    while Instant::now() < deadline && !done(&merged) {
        if let Ok(batch) = receiver.recv_timeout(Duration::from_millis(200)) {
            merged.changed_specs.extend(batch.changed_specs);
            merged.tree_changed |= batch.tree_changed;
        }
    }
    merged
}

fn ends_with_any(paths: &BTreeSet<PathBuf>, name: &str) -> bool {
    paths.iter().any(|path| path.ends_with(name))
}

#[test]
fn watcher_follows_specs_written_in_subfolders_renamed_and_removed() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path().canonicalize()?;
    fs::create_dir_all(root.join("sub"))?;
    fs::write(root.join("sub/existing.diorama.yaml"), "diorama: 1\n")?;
    let (sender, receiver) = mpsc::channel();
    let watcher = watch(&root, move |batch| {
        let _ = sender.send(batch);
    })?;
    std::thread::sleep(Duration::from_millis(400));

    // The AI rewrites an existing spec in a subfolder: content change only.
    fs::write(root.join("sub/existing.diorama.yaml"), "diorama: 1\ntitle: AI\n")?;
    let batch = collect_until(&receiver, |batch| ends_with_any(&batch.changed_specs, "existing.diorama.yaml"));
    assert!(ends_with_any(&batch.changed_specs, "existing.diorama.yaml"), "{batch:?}");

    // A new spec in a new folder: the tree must be rescanned.
    fs::create_dir_all(root.join("new/deeper"))?;
    fs::write(root.join("new/deeper/fresh.diorama.yaml"), "diorama: 1\n")?;
    let batch = collect_until(&receiver, |batch| batch.tree_changed);
    assert!(batch.tree_changed, "{batch:?}");

    // Rename and removal also restructure the tree.
    fs::rename(root.join("new/deeper/fresh.diorama.yaml"), root.join("new/deeper/renamed.diorama.yaml"))?;
    assert!(collect_until(&receiver, |batch| batch.tree_changed).tree_changed, "rename not reported");
    fs::remove_file(root.join("new/deeper/renamed.diorama.yaml"))?;
    assert!(collect_until(&receiver, |batch| batch.tree_changed).tree_changed, "removal not reported");

    // Non-spec files never wake the app up.
    while receiver.try_recv().is_ok() {}
    fs::write(root.join("notes.md"), "x")?;
    let noise = collect_until(&receiver, |_| false);
    assert!(noise.changed_specs.is_empty(), "{noise:?}");

    // Dropping the watcher stops events (opening another folder replaces it).
    drop(watcher);
    std::thread::sleep(Duration::from_millis(300));
    while receiver.try_recv().is_ok() {}
    fs::write(root.join("sub/existing.diorama.yaml"), "diorama: 1\ntitle: after\n")?;
    assert!(receiver.recv_timeout(Duration::from_secs(1)).is_err(), "events after the watcher was dropped");
    Ok(())
}
