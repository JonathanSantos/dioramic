//! `dioramic lint`: compile specs without rasterizing and report diagnostics.

use std::path::{Path, PathBuf};

use anyhow::Result;
use dioramic_render::{Engine, RenderError, RenderRequest, SourceText, SpecSource};
use dioramic_spec::{Diagnostic, DiagnosticCode, Diagnostics, SourceFormat};
use serde::Serialize;

use crate::cli::LintArgs;
use crate::commands::{read_spec, spec_icons, text_format};
use crate::ui::{self, AlreadyReported, BOLD, DIM, GREEN, RED, YELLOW, paint};

/// Outcome of one spec.
#[derive(Debug, Serialize)]
struct LintReport {
    file: PathBuf,
    ok: bool,
    diagnostics: Diagnostics,
}

/// Exit status 1 when any spec has errors.
pub fn run(args: &LintArgs) -> Result<()> {
    let engine = Engine::shared();
    let reports: Vec<LintReport> = args.specs.iter().map(|spec| lint_file(engine, spec)).collect();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        print_reports(&reports);
    }
    if reports.iter().all(|report| report.ok) { Ok(()) } else { Err(AlreadyReported.into()) }
}

fn lint_file(engine: &Engine, spec: &Path) -> LintReport {
    let diagnostics = match read_spec(spec) {
        Ok(text) => compile_diagnostics(engine, spec, text),
        Err(error) => Diagnostics::from(Diagnostic::new(DiagnosticCode::Parse, "", error.to_string())),
    };
    LintReport { file: spec.to_path_buf(), ok: !diagnostics.has_errors(), diagnostics }
}

fn compile_diagnostics(engine: &Engine, spec: &Path, text: String) -> Diagnostics {
    let icons = spec_icons(spec, &text);
    let mut request = RenderRequest::new(SpecSource::Text(SourceText {
        text,
        format: SourceFormat::from_path(spec).map(text_format),
    }));
    request.icons = icons;
    match engine.compile(&request) {
        Ok(diagram) => diagram.diagnostics,
        Err(RenderError::Invalid(diagnostics)) => diagnostics,
        // Not a spec problem (limits, engine stub): still reported per file, as a document-level error.
        Err(other) => Diagnostics::from(Diagnostic::new(DiagnosticCode::Limit, "", other.to_string())),
    }
}

fn print_reports(reports: &[LintReport]) {
    for report in reports {
        let (mark, style) = match (report.ok, report.diagnostics.is_empty()) {
            (false, _) => ("✗", RED),
            (true, false) => ("!", YELLOW),
            (true, true) => ("✓", GREEN),
        };
        let counts = ui::diagnostic_counts(&report.diagnostics);
        let details = if counts.is_empty() { "ok".to_owned() } else { counts };
        anstream::println!("{} {}  {}", paint(style, mark), paint(BOLD, report.file.display()), paint(DIM, details));
        for diagnostic in &report.diagnostics {
            anstream::println!("{}", ui::format_diagnostic(diagnostic));
        }
    }
    if reports.len() > 1 {
        let failed = reports.iter().filter(|report| !report.ok).count();
        let summary = format!("{} checked, {} with errors", ui::plural(reports.len(), "spec"), failed);
        anstream::println!("{}", paint(if failed > 0 { RED } else { GREEN }, summary));
    }
}
