//! Terminal output: styles, user-facing errors with hints, diagnostics and small formatters.
//!
//! Everything is printed through `anstream`, which strips the ANSI styles when the stream is not a
//! terminal and honors `NO_COLOR` / `CLICOLOR_FORCE`, so the code below can always style freely.

use std::fmt;

use anstyle::{AnsiColor, Style};
use dioramic_spec::{Diagnostic, Diagnostics, Severity};

/// Bold text (titles, file names).
pub const BOLD: Style = Style::new().bold();
/// Dimmed text (secondary details).
pub const DIM: Style = Style::new().dimmed();
/// Success marks.
pub const GREEN: Style = AnsiColor::Green.on_default().bold();
/// Warnings.
pub const YELLOW: Style = AnsiColor::Yellow.on_default().bold();
/// Errors.
pub const RED: Style = AnsiColor::Red.on_default().bold();
/// Hints and accents.
pub const CYAN: Style = AnsiColor::Cyan.on_default();
/// Merged / updated entries.
pub const BLUE: Style = AnsiColor::Blue.on_default().bold();

/// Wraps `text` in `style` (reset afterwards).
pub fn paint(style: Style, text: impl fmt::Display) -> String {
    format!("{style}{text}{style:#}")
}

/// An error meant for the user: one sentence plus an optional actionable hint.
#[derive(Debug)]
pub struct UserError {
    /// What went wrong.
    pub message: String,
    /// What to do about it.
    pub hint: Option<String>,
}

impl UserError {
    /// Error without hint.
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), hint: None }
    }

    /// Adds a hint.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl fmt::Display for UserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UserError {}

/// Marker error: the command already printed what failed; only the exit status remains.
#[derive(Debug)]
pub struct AlreadyReported;

impl fmt::Display for AlreadyReported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("command failed")
    }
}

impl std::error::Error for AlreadyReported {}

/// Prints any error of a command on stderr: message, cause chain and hint.
pub fn report_error(error: &anyhow::Error) {
    if error.downcast_ref::<AlreadyReported>().is_some() {
        return;
    }
    anstream::eprintln!("{} {}", paint(RED, "error:"), error);
    for cause in error.chain().skip(1) {
        anstream::eprintln!("  {} {cause}", paint(DIM, "caused by:"));
    }
    if let Some(hint) = error.downcast_ref::<UserError>().and_then(|user| user.hint.as_deref()) {
        print_hint(hint);
    }
}

/// `  hint: …` on stderr.
pub fn print_hint(hint: &str) {
    anstream::eprintln!("  {} {hint}", paint(CYAN, "hint:"));
}

/// `warning: …` on stderr.
pub fn print_warning(message: &str) {
    anstream::eprintln!("{} {message}", paint(YELLOW, "warning:"));
}

/// Prints diagnostics (sorted as given) on stderr, below a line that names the spec file.
pub fn print_diagnostics(diagnostics: &Diagnostics) {
    for diagnostic in diagnostics {
        anstream::eprintln!("{}", format_diagnostic(diagnostic));
    }
}

/// One diagnostic, possibly on two lines (the hint goes below): `error[E_UNKNOWN_REF] 6:5 edges[0] message`.
pub fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    let (label, style) = severity_label(diagnostic.severity);
    let location = diagnostic
        .location
        .map(|position| format!(" {}", paint(DIM, format!("{}:{}", position.line, position.column))))
        .unwrap_or_default();
    let path = if diagnostic.path.is_empty() { String::new() } else { format!(" {}", paint(BOLD, &diagnostic.path)) };
    let mut text = format!(
        "  {}{}{location}{path} {}",
        paint(style, label),
        paint(style, format!("[{}]", diagnostic.code)),
        diagnostic.message
    );
    if let Some(hint) = &diagnostic.hint {
        text.push_str(&format!("\n    {} {hint}", paint(CYAN, "hint:")));
    }
    text
}

fn severity_label(severity: Severity) -> (&'static str, Style) {
    match severity {
        Severity::Error => ("error", RED),
        Severity::Warning => ("warning", YELLOW),
        Severity::Hint => ("hint", CYAN),
    }
}

/// Counts per severity, e.g. `2 errors, 1 warning`; empty string when there is nothing.
pub fn diagnostic_counts(diagnostics: &Diagnostics) -> String {
    let count = |severity| diagnostics.iter().filter(|diagnostic| diagnostic.severity == severity).count();
    [(count(Severity::Error), "error"), (count(Severity::Warning), "warning"), (count(Severity::Hint), "hint")]
        .into_iter()
        .filter(|(amount, _)| *amount > 0)
        .map(|(amount, noun)| plural(amount, noun))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `1 file`, `3 files`.
pub fn plural(amount: usize, noun: &str) -> String {
    if amount == 1 { format!("{amount} {noun}") } else { format!("{amount} {noun}s") }
}

/// Human file size: `912 B`, `14.2 KB`, `3.1 MB`.
pub fn human_bytes(bytes: u64) -> String {
    const KILOBYTE: f64 = 1024.0;
    const MEGABYTE: f64 = KILOBYTE * KILOBYTE;
    let value = bytes as f64;
    if value < KILOBYTE {
        format!("{bytes} B")
    } else if value < MEGABYTE {
        format!("{:.1} KB", value / KILOBYTE)
    } else {
        format!("{:.1} MB", value / MEGABYTE)
    }
}

/// Right-pads `text` to `width` visible characters (styles must be applied after padding).
pub fn pad(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

#[cfg(test)]
mod tests {
    use dioramic_spec::DiagnosticCode;

    use super::*;

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_bytes(912), "912 B");
        assert_eq!(human_bytes(14_540), "14.2 KB");
        assert_eq!(human_bytes(3 * 1024 * 1024 + 100_000), "3.1 MB");
    }

    #[test]
    fn counts_skip_empty_severities() {
        let mut list = Diagnostics::new();
        assert_eq!(diagnostic_counts(&list), "");
        list.push(Diagnostic::new(DiagnosticCode::UnknownRef, "edges[0]", "node 'x' does not exist"));
        list.push(Diagnostic::new(DiagnosticCode::UnknownRef, "edges[1]", "node 'y' does not exist"));
        list.push(Diagnostic::new(DiagnosticCode::LabelLong, "nodes[0].label", "long"));
        assert_eq!(diagnostic_counts(&list), "2 errors, 1 warning");
    }

    #[test]
    fn diagnostic_line_has_code_path_and_hint() {
        let diagnostic = Diagnostic::new(DiagnosticCode::UnknownRef, "edges[0]", "node 'dbb' does not exist")
            .with_hint("did you mean 'db'?");
        let text = format_diagnostic(&diagnostic);
        for expected in ["E_UNKNOWN_REF", "edges[0]", "does not exist", "did you mean 'db'?"] {
            assert!(text.contains(expected), "{expected} missing in {text}");
        }
    }
}
