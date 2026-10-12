//! The `init` summary: one line per file, totals, diffs (dry run and conflicts) and next steps.

use std::collections::BTreeSet;

use similar::TextDiff;

use crate::init::plan::{Change, Client, Diff, Scope, Status};
use crate::ui::{self, BLUE, BOLD, CYAN, DIM, GREEN, RED, YELLOW, paint};

/// Diff lines shown per file before eliding the rest.
const MAX_DIFF_LINES: usize = 60;
/// Context lines around each diff hunk.
const DIFF_CONTEXT: usize = 2;

/// What the header says about the run.
pub struct RunInfo<'a> {
    /// Project directory or user scope.
    pub scope: Scope,
    /// Short project path.
    pub project: &'a str,
    /// Clients configured.
    pub clients: &'a [Client],
    /// Clients were detected (no flag).
    pub detected: bool,
    /// Nothing is written.
    pub dry_run: bool,
}

/// Prints the whole report on stdout.
pub fn print(info: &RunInfo<'_>, changes: &[Change]) {
    print_header(info);
    print_table(changes, info.dry_run);
    print_totals(changes, info.dry_run);
    print_diffs(changes, info.dry_run);
    print_next_steps(info, changes);
}

fn print_header(info: &RunInfo<'_>) {
    let clients: Vec<&str> = info.clients.iter().map(|client| client.name()).collect();
    let detected = if info.detected { paint(DIM, " (detected)") } else { String::new() };
    let scope = match info.scope {
        Scope::Project => format!("project {}", info.project),
        Scope::User => "user scope".to_owned(),
    };
    anstream::println!("{} · {scope} · {}{detected}", paint(BOLD, "dioramic init"), clients.join(" + "));
    if info.dry_run {
        anstream::println!("{}", paint(YELLOW, "dry run: nothing will be written"));
    }
    anstream::println!();
}

fn print_table(changes: &[Change], dry_run: bool) {
    let label_width = changes.iter().map(|change| change.label.chars().count()).max().unwrap_or(0);
    let status_width = changes.iter().map(|change| status_word(change.status, dry_run).len()).max().unwrap_or(0);
    for change in changes {
        let word = ui::pad(status_word(change.status, dry_run), status_width);
        let (symbol, style) = status_style(change.status);
        anstream::println!(
            "  {} {}  {}  {}",
            paint(style, symbol),
            paint(style, word),
            ui::pad(&change.label, label_width),
            paint(DIM, &change.detail)
        );
        if let Some(note) = &change.note {
            anstream::println!("  {}  {}", " ".repeat(status_width + 1), paint(DIM, format!("↳ {note}")));
        }
    }
    anstream::println!();
}

fn status_word(status: Status, dry_run: bool) -> &'static str {
    match (status, dry_run) {
        (Status::Create, false) => "created",
        (Status::Create, true) => "would create",
        (Status::Merge, false) => "merged",
        (Status::Merge, true) => "would merge",
        (Status::Update, false) => "updated",
        (Status::Update, true) => "would update",
        (Status::Replace, false) => "replaced",
        (Status::Replace, true) => "would replace",
        (Status::Unchanged, _) => "unchanged",
        (Status::Conflict, _) => "conflict",
        (Status::Skipped, _) => "skipped",
        (Status::Failed, _) => "failed",
    }
}

fn status_style(status: Status) -> (&'static str, anstyle::Style) {
    match status {
        Status::Create => ("+", GREEN),
        Status::Merge | Status::Update | Status::Replace => ("~", BLUE),
        Status::Unchanged => ("=", DIM),
        Status::Skipped => ("-", DIM),
        Status::Conflict => ("!", YELLOW),
        Status::Failed => ("✗", RED),
    }
}

fn print_totals(changes: &[Change], dry_run: bool) {
    if changes.iter().all(|change| matches!(change.status, Status::Unchanged | Status::Skipped)) {
        anstream::println!("{} {}", paint(GREEN, "✓"), "No changes: Dioramic is already configured.");
        return;
    }
    let order = [
        Status::Create,
        Status::Merge,
        Status::Update,
        Status::Replace,
        Status::Unchanged,
        Status::Skipped,
        Status::Conflict,
        Status::Failed,
    ];
    let totals: Vec<String> = order
        .into_iter()
        .filter_map(|status| {
            let count = changes.iter().filter(|change| change.status == status).count();
            (count > 0).then(|| format!("{count} {}", status_word(status, dry_run)))
        })
        .collect();
    anstream::println!("{}", totals.join(" · "));
}

fn print_diffs(changes: &[Change], dry_run: bool) {
    for change in changes {
        let show = change.status == Status::Conflict || (dry_run && change.status.writes());
        if let (true, Some(diff)) = (show, &change.diff) {
            anstream::println!();
            let title =
                if change.status == Status::Conflict { "current entry vs dioramic's" } else { "planned change" };
            anstream::println!("{} {}", paint(BOLD, &change.label), paint(DIM, title));
            print_diff(diff);
        }
    }
}

fn print_diff(diff: &Diff) {
    let text_diff = TextDiff::from_lines(&diff.before, &diff.after);
    let unified = text_diff.unified_diff().context_radius(DIFF_CONTEXT).to_string();
    let lines: Vec<&str> = unified.lines().collect();
    for line in lines.iter().take(MAX_DIFF_LINES) {
        let style = match line.chars().next() {
            Some('+') => GREEN,
            Some('-') => RED,
            Some('@') => CYAN,
            _ => DIM,
        };
        anstream::println!("  {}", paint(style, line));
    }
    if lines.len() > MAX_DIFF_LINES {
        anstream::println!("  {}", paint(DIM, format!("… {} more lines", lines.len() - MAX_DIFF_LINES)));
    }
}

fn print_next_steps(info: &RunInfo<'_>, changes: &[Change]) {
    let problems = changes.iter().filter(|change| change.status.is_problem()).count();
    if problems > 0 {
        anstream::println!();
        if changes.iter().any(|change| change.status == Status::Conflict) {
            ui::print_hint(
                "re-run with --force to replace the Dioramic entries above (other entries are never touched)",
            );
        }
        if changes.iter().any(|change| change.status == Status::Failed) {
            ui::print_hint("fix the files marked as failed (they were left untouched) and run init again");
        }
    }
    let written: BTreeSet<Client> =
        changes.iter().filter(|change| change.status.writes()).filter_map(|change| change.client).collect();
    if written.is_empty() || info.dry_run {
        if info.dry_run && changes.iter().any(|change| change.status.writes()) {
            anstream::println!();
            anstream::println!("{}", paint(DIM, "Run again without --dry-run to apply."));
        }
        return;
    }
    anstream::println!();
    anstream::println!("{}", paint(BOLD, "Next steps"));
    for client in &written {
        anstream::println!("  {} {}", paint(CYAN, "›"), next_step(*client, info.scope));
    }
    anstream::println!("  {} {}", paint(CYAN, "›"), "Ask for an architecture diagram, or type /dioramic.");
}

fn next_step(client: Client, scope: Scope) -> &'static str {
    match (client, scope) {
        (Client::ClaudeCode, Scope::Project) => {
            "Claude Code: open this project and approve the `dioramic` MCP server when asked (check with /mcp)."
        }
        (Client::ClaudeCode, Scope::User) => {
            "Claude Code: restart it; `dioramic` is now available in every project (check with /mcp)."
        }
        (Client::VsCode, Scope::Project) => {
            "VS Code: reload the window, trust the workspace and start `dioramic` (MCP: List Servers); use Copilot Chat in Agent mode."
        }
        (Client::VsCode, Scope::User) => {
            "VS Code: reload the window and start `dioramic` (MCP: List Servers); use Copilot Chat in Agent mode."
        }
        (Client::ClaudeDesktop, _) => "Claude Desktop: quit and reopen the app to load the server.",
    }
}
