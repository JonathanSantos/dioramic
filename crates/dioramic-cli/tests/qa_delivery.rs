//! QA de entrega (T-380): the two flows of the v1 "definition of done" that only had partial coverage.
//!
//! - A full MCP session over stdio against the real binary: `initialize` → `notifications/initialized` →
//!   `tools/list` → `tools/call render_diagram` with an inline spec, checking the result carries a PNG preview
//!   **and** the JSON summary, and that the files land inside the server's working directory.
//! - `init --claude --vscode` produces the three files, is idempotent byte for byte, and the generated
//!   `SKILL.md` follows the strict Agent Skills format (`docs/work/research/clients.md` §1.4, §2.3, §4.5),
//!   with a cheatsheet that itself lints clean.
//!
//! `HOME`, `APPDATA`, `XDG_CONFIG_HOME` and `PATH` are overridden so nothing reaches the user's machine.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

/// Base64 of the 8-byte PNG signature (`\x89PNG\r\n\x1a\n`) plus the start of the IHDR length.
const PNG_BASE64_PREFIX: &str = "iVBORw0KGgo";
const SKILL_NAME_MAX: usize = 64;
const SKILL_DESCRIPTION_MAX: usize = 1024;
const SKILL_LINES_MAX: usize = 500;

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

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dioramic"));
        command
            .current_dir(self.project())
            .env("HOME", self.home())
            .env("USERPROFILE", self.home())
            .env("APPDATA", self.root.path().join("appdata"))
            .env("XDG_CONFIG_HOME", self.home().join(".config"))
            .env("PATH", self.root.path().join("empty-bin"))
            .env("NO_COLOR", "1")
            .env_remove("CLICOLOR_FORCE");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}

fn all_output(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

/// A line-delimited JSON-RPC client over the child's stdio.
struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpSession {
    fn start(sandbox: &Sandbox) -> Self {
        let mut child = sandbox
            .command()
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self { child, stdin, stdout }
    }

    fn notify(&mut self, method: &str) {
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "method": method })).unwrap();
        self.stdin.flush().unwrap();
    }

    /// Sends a request and returns the response with the same id; stdout must carry JSON-RPC only.
    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).unwrap();
            assert!(read > 0, "server closed stdout while waiting for {method}");
            let response: Value =
                serde_json::from_str(&line).unwrap_or_else(|_| panic!("stdout is not JSON-RPC: {line}"));
            if response["id"] == json!(id) {
                return response;
            }
        }
    }

    /// Closing stdin must end the server cleanly.
    fn finish(mut self) -> std::process::ExitStatus {
        drop(self.stdin);
        self.child.wait().unwrap()
    }
}

fn initialize_params() -> Value {
    json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": { "name": "qa-delivery", "version": "0" }
    })
}

fn three_tier_spec() -> Value {
    json!({
        "diorama": 1,
        "title": "QA handshake",
        "nodes": [
            { "id": "users", "icon": "users", "label": "Users" },
            { "id": "api", "icon": "server", "label": "API" },
            { "id": "db", "icon": "postgres" }
        ],
        "edges": ["users -> api: HTTPS", "api -> db: SQL"]
    })
}

#[test]
fn mcp_stdio_session_renders_an_inline_spec_with_image_and_summary() {
    let sandbox = Sandbox::new();
    let mut session = McpSession::start(&sandbox);

    let initialize = session.request(1, "initialize", initialize_params());
    let result = &initialize["result"];
    assert_eq!(result["serverInfo"]["name"], "dioramic", "{initialize}");
    for capability in ["tools", "resources", "prompts"] {
        assert!(result["capabilities"].get(capability).is_some(), "missing capability {capability}: {initialize}");
    }
    session.notify("notifications/initialized");

    let listed = session.request(2, "tools/list", json!({}));
    let mut names: Vec<&str> =
        listed["result"]["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(names, ["get_guide", "render_diagram", "search_icons", "validate_diagram"], "{listed}");

    let call = session.request(
        3,
        "tools/call",
        json!({ "name": "render_diagram", "arguments": { "spec": three_tier_spec() } }),
    );
    let content = call["result"]["content"].as_array().unwrap_or_else(|| panic!("no content: {call}"));
    assert_ne!(call["result"]["isError"], json!(true), "{call}");

    let image = content.iter().find(|item| item["type"] == "image").unwrap_or_else(|| panic!("no image: {call}"));
    assert_eq!(image["mimeType"], "image/png");
    assert!(image["data"].as_str().unwrap().starts_with(PNG_BASE64_PREFIX), "preview is not a PNG");

    let text = content.iter().find(|item| item["type"] == "text").unwrap_or_else(|| panic!("no text: {call}"));
    let summary: Value = serde_json::from_str(text["text"].as_str().unwrap()).unwrap();
    assert_eq!(summary["format"], "png", "{summary}");
    assert_eq!(summary["diagnostics"], json!([]), "{summary}");
    let output_path = summary["output_path"].as_str().unwrap();
    let spec_path = summary["spec_path"].as_str().unwrap();
    assert!(Path::new(output_path).is_relative(), "paths are reported relative to the project: {summary}");
    assert!(sandbox.project().join(output_path).is_file(), "{output_path} was not written in the cwd");
    assert!(sandbox.project().join(spec_path).is_file(), "inline spec was not saved next to the output");

    assert!(session.finish().success(), "server must exit 0 when stdin closes");
}

#[test]
fn mcp_stdio_session_reports_invalid_specs_as_tool_errors_not_protocol_errors() {
    let sandbox = Sandbox::new();
    let mut session = McpSession::start(&sandbox);
    session.request(1, "initialize", initialize_params());
    session.notify("notifications/initialized");

    let spec = json!({ "diorama": 1, "nodes": [{ "id": "a", "icon": "server" }], "edges": ["a -> ghost"] });
    let call = session.request(2, "tools/call", json!({ "name": "render_diagram", "arguments": { "spec": spec } }));
    assert!(call.get("error").is_none(), "a bad spec must not be a JSON-RPC error: {call}");
    assert_eq!(call["result"]["isError"], json!(true), "{call}");
    let text = call["result"].to_string();
    assert!(text.contains("E_UNKNOWN_REF") && text.contains("edges[0].to"), "{text}");
    assert!(!sandbox.project().join("diagrams").exists(), "nothing may be written for an invalid spec");

    assert!(session.finish().success());
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                files.push((path.strip_prefix(root).unwrap().to_path_buf(), fs::read(&path).unwrap(), modified));
            }
        }
    }
    files.sort();
    files
}

fn stdio_server(config: &Value, key: &str) -> Value {
    config[key]["dioramic"].clone()
}

#[test]
fn init_for_claude_and_vscode_writes_three_files_and_a_second_run_changes_nothing() {
    let sandbox = Sandbox::new();
    let first = sandbox.run(&["init", "--claude", "--vscode"]);
    assert!(first.status.success(), "{}", all_output(&first));

    let mcp: Value = serde_json::from_str(&fs::read_to_string(sandbox.project().join(".mcp.json")).unwrap()).unwrap();
    let vscode: Value =
        serde_json::from_str(&fs::read_to_string(sandbox.project().join(".vscode/mcp.json")).unwrap()).unwrap();
    let expected = json!({ "type": "stdio", "command": "dioramic", "args": ["mcp"] });
    assert_eq!(stdio_server(&mcp, "mcpServers"), expected);
    assert_eq!(stdio_server(&vscode, "servers"), expected);
    assert!(sandbox.project().join(".claude/skills/dioramic/SKILL.md").is_file());
    assert!(!sandbox.project().join(".github/skills").exists(), "one skill copy per scope (clients.md §4.2)");
    assert!(fs::read_dir(sandbox.home()).unwrap().next().is_none(), "project init must not touch the home");

    let before = snapshot(&sandbox.project());
    assert_eq!(before.len(), 3, "{:?}", before.iter().map(|file| &file.0).collect::<Vec<_>>());
    let second = sandbox.run(&["init", "--claude", "--vscode"]);
    assert!(second.status.success(), "{}", all_output(&second));
    assert!(all_output(&second).contains("No changes"), "{}", all_output(&second));
    assert_eq!(snapshot(&sandbox.project()), before, "second run must not rewrite any file (bytes or mtime)");
}

/// Frontmatter `key: value` lines (top level only) of a `SKILL.md`.
fn frontmatter(skill: &str) -> Vec<(String, String)> {
    let body = skill.strip_prefix("---\n").expect("frontmatter must start on line 1");
    let end = body.find("\n---\n").expect("frontmatter must be closed");
    body[..end]
        .lines()
        .filter(|line| !line.starts_with(' '))
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_string(), value.trim().trim_matches('"').to_string()))
        .collect()
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> &'a str {
    &fields.iter().find(|(name, _)| name == key).unwrap_or_else(|| panic!("missing `{key}`")).1
}

fn is_agent_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= SKILL_NAME_MAX
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

#[test]
fn generated_skill_follows_the_strict_agent_skills_format() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["init", "--claude"]);
    assert!(output.status.success(), "{}", all_output(&output));
    let skill_dir = sandbox.project().join(".claude/skills/dioramic");
    let skill = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    let fields = frontmatter(&skill);

    let name = field(&fields, "name");
    assert!(is_agent_skill_name(name), "invalid name {name:?}");
    assert_eq!(name, skill_dir.file_name().unwrap().to_str().unwrap(), "name must equal the folder");
    let description = field(&fields, "description");
    assert!((1..=SKILL_DESCRIPTION_MAX).contains(&description.chars().count()), "description length");
    assert!(description.contains("Use when"), "description must say when to use it: {description}");
    assert!(skill.contains("generator: dioramic"), "generated marker missing (clients.md §4.4 item 8)");
    assert!(skill.lines().count() < SKILL_LINES_MAX);
    assert!(skill.ends_with('\n'));
    for tool in ["render_diagram", "search_icons", "get_guide"] {
        assert!(skill.contains(tool), "skill does not mention {tool}");
    }
}

#[test]
fn skill_cheatsheet_is_a_spec_that_lints_clean() {
    let sandbox = Sandbox::new();
    assert!(sandbox.run(&["init", "--claude"]).status.success());
    let skill = fs::read_to_string(sandbox.project().join(".claude/skills/dioramic/SKILL.md")).unwrap();
    let start = skill.find("```yaml\n").expect("cheatsheet block") + "```yaml\n".len();
    let length = skill[start..].find("```").unwrap();
    fs::write(sandbox.project().join("cheat.diorama.yaml"), &skill[start..start + length]).unwrap();

    let lint = sandbox.run(&["lint", "cheat.diorama.yaml"]);
    assert!(lint.status.success(), "{}", all_output(&lint));
    assert!(!all_output(&lint).contains("warning"), "the cheatsheet should be warning-free: {}", all_output(&lint));
}
