//! `dioramic init`: configures the MCP server and the Dioramic skill for Claude Code, VS Code Copilot
//! and Claude Desktop. Design: `docs/work/research/clients.md` §4.
//!
//! ```text
//! select clients ─▶ plan files ─▶ evaluate (read + merge in memory) ─▶ apply (unless --dry-run) ─▶ report
//! ```

mod apply;
mod content;
pub mod environment;
mod json_config;
mod plan;
mod report;

use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::InitArgs;
use crate::ui::{AlreadyReported, UserError};
use environment::Environment;
use plan::{Change, Scope};

/// Runs `init` against the real environment.
pub fn run(args: &InitArgs) -> Result<()> {
    let project_dir = project_dir(args)?;
    let changes = execute(args, &Environment::from_process(project_dir))?;
    if changes.iter().any(|change| change.status.is_problem()) { Err(AlreadyReported.into()) } else { Ok(()) }
}

fn project_dir(args: &InitArgs) -> Result<PathBuf> {
    let directory = match &args.dir {
        Some(directory) => directory.clone(),
        None => env::current_dir().context("cannot read the current directory")?,
    };
    if !directory.is_dir() {
        return Err(UserError::new(format!("project directory not found: {}", directory.display()))
            .hint("pass an existing directory with --dir")
            .into());
    }
    Ok(directory.canonicalize().unwrap_or(directory))
}

/// Plans, applies (unless `--dry-run`) and prints the report. Returns the final changes.
pub fn execute(args: &InitArgs, environment: &Environment) -> Result<Vec<Change>> {
    let selection = plan::select_clients(args, environment);
    let actions = plan::plan_actions(args, environment, &selection.clients)?;
    let mut changes = plan::evaluate(&actions, environment, args.force);
    if !args.dry_run {
        apply::apply(&mut changes);
    }
    let project = environment.display_project();
    let info = report::RunInfo {
        scope: if args.global { Scope::User } else { Scope::Project },
        project: &project,
        clients: &selection.clients,
        detected: selection.detected,
        dry_run: args.dry_run,
    };
    report::print(&info, &changes);
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use serde_json::Value;

    use super::*;
    use environment::Platform;
    use plan::Status;

    fn environment(root: &Path) -> Environment {
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        Environment {
            project_dir: project,
            home_dir: Some(root.join("home")),
            app_data_dir: Some(root.join("appdata")),
            config_dir: Some(root.join("home/.config")),
            executable: Some(PathBuf::from("/opt/dioramic/bin/dioramic")),
            path_dirs: Vec::new(),
            platform: Platform::MacOs,
        }
    }

    fn statuses(changes: &[Change]) -> Vec<(String, Status)> {
        changes.iter().map(|change| (change.label.clone(), change.status)).collect()
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn claude_and_vscode_project_files() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path());
        let args = InitArgs { claude: true, vscode: true, ..InitArgs::default() };
        let changes = execute(&args, &environment).unwrap();
        assert_eq!(
            statuses(&changes),
            vec![
                (".mcp.json".to_owned(), Status::Create),
                (".claude/skills/dioramic/SKILL.md".to_owned(), Status::Create),
                (".vscode/mcp.json".to_owned(), Status::Create),
            ]
        );
        let project = &environment.project_dir;
        assert_snapshot(&fs::read_to_string(project.join(".mcp.json")).unwrap(), SNAPSHOT_CLAUDE);
        assert_snapshot(&fs::read_to_string(project.join(".vscode/mcp.json")).unwrap(), SNAPSHOT_VSCODE);
        assert_eq!(
            fs::read_to_string(project.join(".claude/skills/dioramic/SKILL.md")).unwrap(),
            content::skill_markdown()
        );
        assert!(!project.join(".github").exists(), "one skill per scope");
        assert!(!root.path().join("home").exists(), "project init never touches the home directory");
    }

    const SNAPSHOT_CLAUDE: &str = "{\n  \"mcpServers\": {\n    \"dioramic\": {\n      \"type\": \"stdio\",\n      \"command\": \"dioramic\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}\n";
    const SNAPSHOT_VSCODE: &str = "{\n  \"servers\": {\n    \"dioramic\": {\n      \"type\": \"stdio\",\n      \"command\": \"dioramic\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}\n";

    fn assert_snapshot(actual: &str, expected: &str) {
        assert_eq!(actual, expected, "snapshot mismatch:\n{actual}");
    }

    #[test]
    fn second_run_changes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path());
        let args = InitArgs { claude: true, vscode: true, instructions: true, ..InitArgs::default() };
        execute(&args, &environment).unwrap();
        let before = snapshot_tree(&environment.project_dir);
        let changes = execute(&args, &environment).unwrap();
        assert!(changes.iter().all(|change| change.status == Status::Unchanged), "{:?}", statuses(&changes));
        assert_eq!(snapshot_tree(&environment.project_dir), before);
    }

    fn snapshot_tree(directory: &Path) -> Vec<(PathBuf, String)> {
        let mut files = Vec::new();
        let mut pending = vec![directory.to_path_buf()];
        while let Some(current) = pending.pop() {
            for entry in fs::read_dir(&current).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push((path.clone(), fs::read_to_string(&path).unwrap_or_default()));
                }
            }
        }
        files.sort();
        files
    }

    #[test]
    fn existing_config_keeps_other_servers() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path());
        let config = environment.project_dir.join(".mcp.json");
        fs::write(
            &config,
            "{\n  \"mcpServers\": {\n    \"github\": { \"command\": \"gh\", \"args\": [\"mcp\"] }\n  }\n}\n",
        )
        .unwrap();
        let args = InitArgs { claude: true, ..InitArgs::default() };
        let changes = execute(&args, &environment).unwrap();
        assert_eq!(changes[0].status, Status::Merge);
        let value = read_json(&config);
        assert_eq!(value["mcpServers"]["github"]["command"], "gh");
        assert_eq!(value["mcpServers"]["dioramic"]["command"], "dioramic");
        assert!(environment.project_dir.join(".mcp.json.bak").exists());
    }

    #[test]
    fn dry_run_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path());
        let args = InitArgs { claude: true, vscode: true, desktop: true, dry_run: true, ..InitArgs::default() };
        let changes = execute(&args, &environment).unwrap();
        assert!(changes.iter().all(|change| change.status == Status::Create));
        assert!(fs::read_dir(&environment.project_dir).unwrap().next().is_none());
        assert!(!root.path().join("home").exists());
    }

    #[test]
    fn conflict_needs_force() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path());
        let config = environment.project_dir.join(".vscode/mcp.json");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        let original = "{\n  // mine\n  \"servers\": { \"dioramic\": { \"command\": \"/old/dioramic\", \"args\": [\"serve\"] } }\n}\n";
        fs::write(&config, original).unwrap();
        let args = InitArgs { vscode: true, ..InitArgs::default() };
        let changes = execute(&args, &environment).unwrap();
        assert_eq!(changes[0].status, Status::Conflict);
        assert_eq!(fs::read_to_string(&config).unwrap(), original);

        let changes = execute(&InitArgs { force: true, ..args.clone() }, &environment).unwrap();
        assert_eq!(changes[0].status, Status::Replace);
        let text = fs::read_to_string(&config).unwrap();
        assert!(text.contains("// mine"));
        assert_eq!(execute(&args, &environment).unwrap()[0].status, Status::Unchanged);
    }

    #[test]
    fn invalid_json_fails_only_that_file() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path());
        fs::write(environment.project_dir.join(".mcp.json"), "{ not json").unwrap();
        let args = InitArgs { claude: true, ..InitArgs::default() };
        let changes = execute(&args, &environment).unwrap();
        assert_eq!(changes[0].status, Status::Failed);
        assert_eq!(changes[1].status, Status::Create, "the skill is still written");
        assert_eq!(fs::read_to_string(environment.project_dir.join(".mcp.json")).unwrap(), "{ not json");
    }

    #[test]
    fn global_and_desktop_write_under_the_injected_home() {
        let root = tempfile::tempdir().unwrap();
        let environment = environment(root.path());
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".claude.json"), "{\n  \"numStartups\": 7,\n  \"projects\": {}\n}\n").unwrap();
        let args = InitArgs { claude: true, vscode: true, desktop: true, global: true, ..InitArgs::default() };
        let changes = execute(&args, &environment).unwrap();
        assert!(changes.iter().all(|change| !change.status.is_problem()), "{:?}", statuses(&changes));

        let claude = read_json(&home.join(".claude.json"));
        assert_eq!(claude["numStartups"], 7);
        assert_eq!(claude["mcpServers"]["dioramic"]["command"], "/opt/dioramic/bin/dioramic");
        assert!(home.join(".claude/skills/dioramic/SKILL.md").exists());
        assert!(!home.join(".copilot").exists(), "~/.claude/skills already serves VS Code");
        let vscode = read_json(&home.join("Library/Application Support/Code/User/mcp.json"));
        assert_eq!(vscode["servers"]["dioramic"]["type"], "stdio");
        let desktop = read_json(&home.join("Library/Application Support/Claude/claude_desktop_config.json"));
        assert_eq!(
            desktop["mcpServers"]["dioramic"],
            serde_json::json!({"command": "/opt/dioramic/bin/dioramic", "args": ["mcp"]})
        );
        assert!(fs::read_dir(&environment.project_dir).unwrap().next().is_none(), "--global never writes the project");

        let again = execute(&args, &environment).unwrap();
        assert!(again.iter().all(|change| change.status == Status::Unchanged), "{:?}", statuses(&again));
    }
}
