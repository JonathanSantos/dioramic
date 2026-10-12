//! What `init` will do: target selection, the list of files per target (`research/clients.md` §4.2)
//! and, for each file, the change it needs. Reading only; nothing here writes.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::cli::InitArgs;
use crate::init::content::{self, GENERATOR_VERSION, SKILL_NAME};
use crate::init::environment::Environment;
use crate::init::json_config::{self, MergeOutcome, ServerEntry};
use crate::ui::UserError;

/// Command written in project files: portable, resolved through `PATH` by the client.
const PROJECT_COMMAND: &str = "dioramic";

/// An AI client `init` can configure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Client {
    /// Claude Code (CLI and IDE extensions).
    ClaudeCode,
    /// VS Code with GitHub Copilot.
    VsCode,
    /// Claude Desktop.
    ClaudeDesktop,
}

impl Client {
    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::VsCode => "VS Code Copilot",
            Self::ClaudeDesktop => "Claude Desktop",
        }
    }
}

/// Project files or the user's home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Files inside the project (versioned, shared with the team).
    Project,
    /// User-wide files.
    User,
}

/// Clients to configure and whether they were detected rather than asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Clients, in a stable order.
    pub clients: Vec<Client>,
    /// True when no target flag was given.
    pub detected: bool,
}

/// Flags win; otherwise detect Claude Code (`.claude/`, `CLAUDE.md`, `claude` on PATH) and VS Code
/// (`.vscode/`, `code` on PATH), and configure both when nothing is found.
pub fn select_clients(args: &InitArgs, environment: &Environment) -> Selection {
    if args.claude || args.vscode || args.desktop {
        let flagged =
            [(args.claude, Client::ClaudeCode), (args.vscode, Client::VsCode), (args.desktop, Client::ClaudeDesktop)];
        let clients = flagged.into_iter().filter(|(on, _)| *on).map(|(_, client)| client).collect();
        return Selection { clients, detected: false };
    }
    let has = |relative: &str| environment.project_path(relative).exists();
    let claude = has(".claude") || has("CLAUDE.md") || environment.find_program("claude").is_some();
    let vscode = has(".vscode") || environment.find_program("code").is_some();
    let clients = match (claude, vscode) {
        (true, false) => vec![Client::ClaudeCode],
        (false, true) => vec![Client::VsCode],
        _ => vec![Client::ClaudeCode, Client::VsCode],
    };
    Selection { clients, detected: true }
}

/// One file `init` manages.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// MCP server entry in a JSON config.
    Server {
        /// Config file.
        path: PathBuf,
        /// Entry and its root key.
        entry: ServerEntry,
        /// Client it configures.
        client: Client,
        /// `claude` executable to prefer over editing `~/.claude.json` by hand.
        claude_cli: Option<PathBuf>,
    },
    /// The Dioramic skill.
    Skill {
        /// `…/skills/dioramic/SKILL.md`.
        path: PathBuf,
        /// Client whose folder it is.
        client: Client,
    },
    /// VS Code instructions for `*.diorama.*` files.
    Instructions {
        /// `.github/instructions/dioramic.instructions.md`.
        path: PathBuf,
    },
    /// Deliberately not done.
    Skip {
        /// What would have been written.
        label: String,
        /// Why not.
        reason: String,
    },
}

/// Files per selected client and scope.
pub fn plan_actions(args: &InitArgs, environment: &Environment, clients: &[Client]) -> Result<Vec<Action>, UserError> {
    let scope = if args.global { Scope::User } else { Scope::Project };
    let wants = |client| clients.contains(&client);
    let mut actions = Vec::new();
    if wants(Client::ClaudeCode) || wants(Client::VsCode) {
        match scope {
            Scope::Project => plan_project(args, environment, clients, &mut actions),
            Scope::User => plan_user(args, environment, clients, &mut actions)?,
        }
    } else if args.instructions {
        actions.push(Action::Skip {
            label: "dioramic.instructions.md".into(),
            reason: "--instructions applies to VS Code only".into(),
        });
    }
    if wants(Client::ClaudeDesktop) {
        actions.push(plan_desktop(args, environment));
    }
    Ok(actions)
}

fn plan_project(args: &InitArgs, environment: &Environment, clients: &[Client], actions: &mut Vec<Action>) {
    let command = args.command.clone().unwrap_or_else(|| PROJECT_COMMAND.to_owned());
    let claude = clients.contains(&Client::ClaudeCode);
    let skill_dir = |root: &str| environment.project_path(&format!("{root}/skills/{SKILL_NAME}"));
    if claude {
        actions.push(Action::Server {
            path: environment.project_path(".mcp.json"),
            entry: ServerEntry::stdio("mcpServers", &command, true, false),
            client: Client::ClaudeCode,
            claude_cli: None,
        });
        actions.push(Action::Skill { path: skill_dir(".claude").join("SKILL.md"), client: Client::ClaudeCode });
    }
    if clients.contains(&Client::VsCode) {
        actions.push(Action::Server {
            path: environment.project_path(".vscode/mcp.json"),
            entry: ServerEntry::stdio("servers", &command, true, false),
            client: Client::VsCode,
            claude_cli: None,
        });
        if !claude {
            // One skill per scope: VS Code also reads `.claude/skills`, so never duplicate it.
            actions.push(single_skill(skill_dir(".github"), skill_dir(".claude"), environment, Client::VsCode));
        }
        if args.instructions {
            actions.push(Action::Instructions {
                path: environment.project_path(".github/instructions/dioramic.instructions.md"),
            });
        }
    } else if args.instructions {
        actions.push(Action::Skip {
            label: ".github/instructions/dioramic.instructions.md".into(),
            reason: "--instructions applies to VS Code only".into(),
        });
    }
}

fn plan_user(
    args: &InitArgs,
    environment: &Environment,
    clients: &[Client],
    actions: &mut Vec<Action>,
) -> Result<(), UserError> {
    let home = environment.home_dir.clone().ok_or_else(|| {
        UserError::new("cannot find your home directory").hint("set HOME (USERPROFILE on Windows), or drop --global")
    })?;
    let command = user_command(args, environment);
    let claude = clients.contains(&Client::ClaudeCode);
    let skill_dir = |root: &str| home.join(root).join("skills").join(SKILL_NAME);
    if claude {
        actions.push(Action::Server {
            path: home.join(".claude.json"),
            // Same shape `claude mcp add --scope user` writes, so both paths converge.
            entry: ServerEntry::stdio("mcpServers", &command, true, true),
            client: Client::ClaudeCode,
            claude_cli: environment.find_program("claude"),
        });
        actions.push(Action::Skill { path: skill_dir(".claude").join("SKILL.md"), client: Client::ClaudeCode });
    }
    if clients.contains(&Client::VsCode) {
        match environment.vscode_user_mcp() {
            Some(path) => actions.push(Action::Server {
                path,
                entry: ServerEntry::stdio("servers", &command, true, false),
                client: Client::VsCode,
                claude_cli: None,
            }),
            None => actions.push(Action::Skip {
                label: "VS Code user mcp.json".into(),
                reason: "cannot locate the VS Code user folder (set APPDATA / XDG_CONFIG_HOME)".into(),
            }),
        }
        if !claude {
            actions.push(single_skill(skill_dir(".copilot"), skill_dir(".claude"), environment, Client::VsCode));
        }
    }
    if args.instructions {
        actions.push(Action::Skip {
            label: "dioramic.instructions.md".into(),
            reason: "instructions are a project file; run without --global".into(),
        });
    }
    Ok(())
}

/// The skill in `preferred`, unless the Claude folder already has it (VS Code reads that one too).
fn single_skill(preferred: PathBuf, claude_dir: PathBuf, environment: &Environment, client: Client) -> Action {
    if claude_dir.join("SKILL.md").exists() {
        return Action::Skip {
            label: environment.display(&preferred.join("SKILL.md")),
            reason: format!("{} already provides the skill (VS Code reads it)", environment.display(&claude_dir)),
        };
    }
    Action::Skill { path: preferred.join("SKILL.md"), client }
}

fn plan_desktop(args: &InitArgs, environment: &Environment) -> Action {
    match environment.claude_desktop_config() {
        Some(path) => Action::Server {
            path,
            // Claude Desktop's documented format has no `type`.
            entry: ServerEntry::stdio("mcpServers", &user_command(args, environment), false, false),
            client: Client::ClaudeDesktop,
            claude_cli: None,
        },
        None => Action::Skip {
            label: "claude_desktop_config.json".into(),
            reason: "Claude Desktop is only available on macOS and Windows".into(),
        },
    }
}

/// GUI apps do not inherit the shell `PATH`, so user-scope configs get the absolute binary path.
fn user_command(args: &InitArgs, environment: &Environment) -> String {
    args.command.clone().unwrap_or_else(|| {
        environment.executable.as_ref().map_or_else(|| PROJECT_COMMAND.to_owned(), |path| path.display().to_string())
    })
}

/// Outcome of one action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// New file.
    Create,
    /// Entry added to an existing file.
    Merge,
    /// Generated file refreshed to this version.
    Update,
    /// Different content replaced (`--force`).
    Replace,
    /// Already as wanted.
    Unchanged,
    /// Different content kept; needs `--force`.
    Conflict,
    /// Deliberately not done.
    Skipped,
    /// Could not be read, parsed or written.
    Failed,
}

impl Status {
    /// True when the file must be written.
    pub fn writes(self) -> bool {
        matches!(self, Self::Create | Self::Merge | Self::Update | Self::Replace)
    }

    /// True when the run must exit non-zero.
    pub fn is_problem(self) -> bool {
        matches!(self, Self::Conflict | Self::Failed)
    }
}

/// Before/after text shown by `--dry-run` and for conflicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    /// Current text (empty for new files).
    pub before: String,
    /// Wanted text.
    pub after: String,
}

/// One planned (then applied) change.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// Short path.
    pub label: String,
    /// What inside the file (`servers.dioramic`) or why (skips, conflicts, failures).
    pub detail: String,
    /// Outcome.
    pub status: Status,
    /// File to write.
    pub path: Option<PathBuf>,
    /// Full new text when [`Status::writes`].
    pub contents: Option<String>,
    /// What changes (or what conflicts).
    pub diff: Option<Diff>,
    /// Client served by this file.
    pub client: Option<Client>,
    /// Use `claude mcp add` instead of writing the file.
    pub claude_cli: Option<ClaudeCliCall>,
    /// Extra advice shown under the line.
    pub note: Option<String>,
}

/// `claude mcp add --scope user …` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCliCall {
    /// `claude` executable.
    pub program: PathBuf,
    /// Server command.
    pub command: String,
    /// Remove the existing entry first (`--force`).
    pub replace: bool,
}

impl Change {
    fn new(label: String, status: Status, detail: impl Into<String>) -> Self {
        Self {
            label,
            detail: detail.into(),
            status,
            path: None,
            contents: None,
            diff: None,
            client: None,
            claude_cli: None,
            note: None,
        }
    }
}

/// Evaluates every action against the files on disk.
pub fn evaluate(actions: &[Action], environment: &Environment, force: bool) -> Vec<Change> {
    actions.iter().map(|action| evaluate_action(action, environment, force)).collect()
}

fn evaluate_action(action: &Action, environment: &Environment, force: bool) -> Change {
    match action {
        Action::Server { path, entry, client, claude_cli } => {
            let mut change = evaluate_server(path, entry, environment, force);
            change.client = Some(*client);
            if let Some(program) = claude_cli.as_ref().filter(|_| change.status.writes()) {
                let command = entry.command();
                change.claude_cli = Some(ClaudeCliCall {
                    program: program.clone(),
                    command: command.to_owned(),
                    replace: change.status == Status::Replace,
                });
                change.note = Some("via `claude mcp add --scope user`".into());
            } else if path.file_name().is_some_and(|name| name == ".claude.json") && change.status.writes() {
                change.note = Some("Claude Code rewrites this file: close it before running init".into());
            }
            change
        }
        Action::Skill { path, client } => {
            let mut change = evaluate_skill(path, environment, force);
            change.client = Some(*client);
            change
        }
        Action::Instructions { path } => {
            let wanted = content::INSTRUCTIONS.to_owned();
            evaluate_text(path, environment, wanted, force, "VS Code instructions", |_| None)
        }
        Action::Skip { label, reason } => Change::new(label.clone(), Status::Skipped, reason.clone()),
    }
}

fn evaluate_server(path: &Path, entry: &ServerEntry, environment: &Environment, force: bool) -> Change {
    let label = environment.display(path);
    let existing = match read_optional(path) {
        Ok(existing) => existing,
        Err(reason) => return Change::new(label, Status::Failed, reason),
    };
    let (status, contents) = match json_config::merge(existing.as_deref(), entry, force) {
        Ok(MergeOutcome::Create(text)) => (Status::Create, Some(text)),
        Ok(MergeOutcome::Insert(text)) => (Status::Merge, Some(text)),
        Ok(MergeOutcome::Replace { text, .. }) => (Status::Replace, Some(text)),
        Ok(MergeOutcome::Unchanged) => (Status::Unchanged, None),
        Ok(MergeOutcome::Alias(name)) => {
            return Change::new(label, Status::Unchanged, format!("already configured as '{name}'"));
        }
        Ok(MergeOutcome::Conflict { current }) => {
            let mut change =
                Change::new(label, Status::Conflict, format!("{} differs (use --force)", entry.key_path()));
            change.diff = Some(Diff { before: pretty(&current), after: pretty(&entry.value()) });
            return change;
        }
        Err(error) => return Change::new(label, Status::Failed, format!("{error}; the file was left untouched")),
    };
    let mut change = Change::new(label, status, entry.key_path());
    change.diff = contents.as_ref().map(|after| Diff { before: existing.unwrap_or_default(), after: after.clone() });
    change.path = Some(path.to_path_buf());
    change.contents = contents;
    change
}

fn evaluate_skill(path: &Path, environment: &Environment, force: bool) -> Change {
    evaluate_text(
        path,
        environment,
        content::skill_markdown(),
        force,
        &format!("skill v{GENERATOR_VERSION}"),
        |existing| {
            let marker = content::generator_marker(existing)?;
            let ours = content::version_key(GENERATOR_VERSION);
            let theirs = content::version_key(&marker.version);
            Some(if theirs < ours {
                (Status::Update, format!("skill v{} → v{GENERATOR_VERSION}", marker.version))
            } else if theirs > ours {
                (Status::Skipped, format!("a newer skill (v{}) is already installed", marker.version))
            } else {
                (Status::Conflict, "skill edited locally (use --force)".to_owned())
            })
        },
    )
}

/// Shared logic of generated text files. `classify` decides what to do with a different existing file
/// (`None` → it was not generated by us: conflict).
fn evaluate_text(
    path: &Path,
    environment: &Environment,
    wanted: String,
    force: bool,
    detail: &str,
    classify: impl Fn(&str) -> Option<(Status, String)>,
) -> Change {
    let label = environment.display(path);
    let existing = match read_optional(path) {
        Ok(existing) => existing,
        Err(reason) => return Change::new(label, Status::Failed, reason),
    };
    let (status, detail) = match existing.as_deref() {
        None => (Status::Create, detail.to_owned()),
        Some(text) if text == wanted => (Status::Unchanged, detail.to_owned()),
        Some(text) => match classify(text) {
            Some((Status::Conflict, _)) | None if force => (Status::Replace, format!("{detail} (--force)")),
            Some(classified) => classified,
            None => (Status::Conflict, "not written by dioramic (use --force)".to_owned()),
        },
    };
    let mut change = Change::new(label, status, detail);
    if status.writes() || status == Status::Conflict {
        change.diff = Some(Diff { before: existing.unwrap_or_default(), after: wanted.clone() });
    }
    if status.writes() {
        change.path = Some(path.to_path_buf());
        change.contents = Some(wanted);
    }
    change
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read: {error}")),
    }
}

fn pretty(value: &serde_json::Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::environment::Platform;

    fn environment(project: &Path) -> Environment {
        Environment {
            project_dir: project.to_path_buf(),
            home_dir: Some(project.join("home")),
            app_data_dir: None,
            config_dir: Some(project.join("home/.config")),
            executable: Some(PathBuf::from("/opt/dioramic/bin/dioramic")),
            path_dirs: Vec::new(),
            platform: Platform::Linux,
        }
    }

    fn args() -> InitArgs {
        InitArgs::default()
    }

    #[test]
    fn nothing_detected_means_both_clients() {
        let directory = tempfile::tempdir().unwrap();
        let selection = select_clients(&args(), &environment(directory.path()));
        assert_eq!(selection, Selection { clients: vec![Client::ClaudeCode, Client::VsCode], detected: true });
    }

    #[test]
    fn detection_follows_project_markers() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join(".vscode")).unwrap();
        assert_eq!(select_clients(&args(), &environment(directory.path())).clients, vec![Client::VsCode]);
        fs::write(directory.path().join("CLAUDE.md"), "# notes").unwrap();
        assert_eq!(select_clients(&args(), &environment(directory.path())).clients.len(), 2);
    }

    #[test]
    fn detection_uses_path_programs() {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(bin.join("claude"), "").unwrap();
        let mut environment = environment(directory.path());
        environment.path_dirs = vec![bin];
        assert_eq!(select_clients(&args(), &environment).clients, vec![Client::ClaudeCode]);
    }

    #[test]
    fn flags_win_over_detection() {
        let directory = tempfile::tempdir().unwrap();
        let args = InitArgs { desktop: true, ..args() };
        assert_eq!(
            select_clients(&args, &environment(directory.path())),
            Selection { clients: vec![Client::ClaudeDesktop], detected: false }
        );
    }

    #[test]
    fn vscode_alone_writes_the_github_skill_unless_claude_has_it() {
        let directory = tempfile::tempdir().unwrap();
        let environment = environment(directory.path());
        let actions = plan_actions(&args(), &environment, &[Client::VsCode]).unwrap();
        assert!(actions.iter().any(
            |action| matches!(action, Action::Skill { path, .. } if path.ends_with(".github/skills/dioramic/SKILL.md"))
        ));

        let claude_skill = directory.path().join(".claude/skills/dioramic");
        fs::create_dir_all(&claude_skill).unwrap();
        fs::write(claude_skill.join("SKILL.md"), "x").unwrap();
        let actions = plan_actions(&args(), &environment, &[Client::VsCode]).unwrap();
        assert!(actions.iter().any(|action| matches!(action, Action::Skip { .. })));
        assert!(!actions.iter().any(|action| matches!(action, Action::Skill { .. })));
    }

    #[test]
    fn user_scope_uses_the_absolute_binary() {
        let directory = tempfile::tempdir().unwrap();
        let args = InitArgs { global: true, ..args() };
        let actions = plan_actions(&args, &environment(directory.path()), &[Client::ClaudeCode]).unwrap();
        let Some(Action::Server { path, entry, .. }) = actions.first() else { panic!("expected a server action") };
        assert!(path.ends_with("home/.claude.json"));
        assert_eq!(entry.value()["command"], "/opt/dioramic/bin/dioramic");
        assert_eq!(entry.value()["env"], serde_json::json!({}));
    }

    #[test]
    fn desktop_is_skipped_on_linux() {
        let directory = tempfile::tempdir().unwrap();
        let actions = plan_actions(&args(), &environment(directory.path()), &[Client::ClaudeDesktop]).unwrap();
        assert!(matches!(actions.as_slice(), [Action::Skip { .. }]));
    }

    #[test]
    fn user_skill_is_handled_by_version() {
        let directory = tempfile::tempdir().unwrap();
        let environment = environment(directory.path());
        let path = directory.path().join("SKILL.md");
        let check = |force| evaluate_skill(&path, &environment, force).status;
        assert_eq!(check(false), Status::Create);
        fs::write(&path, content::skill_markdown()).unwrap();
        assert_eq!(check(false), Status::Unchanged);
        fs::write(&path, content::skill_markdown().replace(GENERATOR_VERSION, "0.0.1")).unwrap();
        assert_eq!(check(false), Status::Update);
        fs::write(&path, content::skill_markdown().replace(GENERATOR_VERSION, "99.0.0")).unwrap();
        assert_eq!(check(false), Status::Skipped);
        fs::write(&path, "---\nname: dioramic\ndescription: mine\n---\n").unwrap();
        assert_eq!(check(false), Status::Conflict);
        assert_eq!(check(true), Status::Replace);
    }
}
