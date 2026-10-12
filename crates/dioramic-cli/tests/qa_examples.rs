//! QA (T-301): `dioramic` binary end to end over every example (render, lint, mcp).
//!
//! `HOME`, `APPDATA`, `XDG_CONFIG_HOME` and `PATH` are always overridden so no test can reach the real
//! user configuration (`~/.claude.json`, Claude Desktop, VS Code).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const INVALID_SPEC: &str =
    "diorama: 1\nnodes:\n  - { id: api, icon: server }\n  - { id: db, icon: postgres }\nedges:\n  - api -> dbb: SQL\n";

/// Temp project + fake home, isolated from the user's machine.
struct Sandbox {
    root: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let sandbox = Self { root: tempfile::tempdir().unwrap() };
        fs::create_dir_all(sandbox.project()).unwrap();
        fs::create_dir_all(sandbox.home()).unwrap();
        fs::create_dir_all(sandbox.root.path().join("empty-bin")).unwrap();
        sandbox
    }

    fn project(&self) -> PathBuf {
        self.root.path().join("project")
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_dioramic"))
            .args(args)
            .current_dir(self.project())
            .env("HOME", self.home())
            .env("USERPROFILE", self.home())
            .env("APPDATA", self.root.path().join("appdata"))
            .env("XDG_CONFIG_HOME", self.home().join(".config"))
            .env("XDG_DATA_HOME", self.home().join(".local/share"))
            .env("PATH", self.root.path().join("empty-bin"))
            .env("NO_COLOR", "1")
            .env_remove("CLICOLOR_FORCE")
            .output()
            .unwrap()
    }

    fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.project().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

fn all_output(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn example_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(examples_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".diorama.yaml").map(str::to_owned))
        .collect();
    names.sort();
    names
}

fn copy_example(sandbox: &Sandbox, name: &str) -> String {
    let relative = format!("{name}.diorama.yaml");
    fs::copy(examples_dir().join(&relative), sandbox.project().join(&relative)).unwrap();
    relative
}

#[test]
fn render_every_example_to_png_and_svg() {
    let sandbox = Sandbox::new();
    for name in example_names() {
        let spec = copy_example(&sandbox, &name);
        let png = sandbox.run(&["render", &spec]);
        assert!(png.status.success(), "{name}: {}", all_output(&png));
        let bytes = fs::read(sandbox.project().join(format!("{name}.png"))).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"), "{name}: not a PNG");
        let svg = sandbox.run(&["render", &spec, "-o", &format!("{name}.svg")]);
        assert!(svg.status.success(), "{name}: {}", all_output(&svg));
        let text = fs::read_to_string(sandbox.project().join(format!("{name}.svg"))).unwrap();
        assert!(text.contains("<svg"), "{name}: not an SVG");
    }
}

#[test]
fn render_infers_gif_from_the_output_extension() {
    let sandbox = Sandbox::new();
    let spec = copy_example(&sandbox, "minimal");
    let output = sandbox.run(&["render", &spec, "-o", "out.gif", "--width", "320"]);
    assert!(output.status.success(), "{}", all_output(&output));
    let bytes = fs::read(sandbox.project().join("out.gif")).unwrap();
    assert!(bytes.starts_with(b"GIF89a"), "not a GIF");
}

#[test]
fn render_auto_rotation_reports_the_chosen_angle() {
    let sandbox = Sandbox::new();
    let spec = copy_example(&sandbox, "bank-branch");
    let output = sandbox.run(&["render", &spec, "-o", "bank.svg", "--rotate", "auto"]);
    assert!(output.status.success(), "{}", all_output(&output));
    let text = all_output(&output);
    assert!(text.contains("auto") && text.contains('°'), "summary should name the chosen rotation: {text}");
}

#[test]
fn render_of_an_invalid_spec_fails_and_writes_nothing() {
    let sandbox = Sandbox::new();
    sandbox.write("bad.diorama.yaml", INVALID_SPEC);
    let output = sandbox.run(&["render", "bad.diorama.yaml"]);
    assert!(!output.status.success());
    assert!(all_output(&output).contains("E_UNKNOWN_REF"), "{}", all_output(&output));
    assert!(!sandbox.project().join("bad.png").exists());
}

#[test]
fn lint_every_example_exits_0() {
    let sandbox = Sandbox::new();
    let specs: Vec<String> = example_names().iter().map(|name| copy_example(&sandbox, name)).collect();
    let mut args = vec!["lint"];
    args.extend(specs.iter().map(String::as_str));
    let output = sandbox.run(&args);
    assert!(output.status.success(), "{}", all_output(&output));
}

#[test]
fn lint_json_is_machine_readable_with_codes_and_paths() {
    let sandbox = Sandbox::new();
    sandbox.write("bad.diorama.yaml", INVALID_SPEC);
    let output = sandbox.run(&["lint", "--json", "bad.diorama.yaml"]);
    assert_eq!(output.status.code(), Some(1), "{}", all_output(&output));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("lint --json must print JSON on stdout ({error}): {}", all_output(&output)));
    let text = json.to_string();
    assert!(text.contains("E_UNKNOWN_REF"), "{text}");
    assert!(text.contains("edges[0]"), "diagnostic path expected: {text}");
}

#[test]
fn mcp_answers_initialize_on_stdio() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    let sandbox = Sandbox::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dioramic"))
        .arg("mcp")
        .current_dir(sandbox.project())
        .env("HOME", sandbox.home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"qa","version":"0"}}}"#;
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{request}").unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let response: serde_json::Value = serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON-RPC: {line}"));
    assert_eq!(response["id"], 1, "{response}");
    assert_eq!(response["result"]["serverInfo"]["name"], "dioramic", "{response}");
    assert!(response["result"]["instructions"].as_str().is_some_and(|text| text.contains("render_diagram")));
}
