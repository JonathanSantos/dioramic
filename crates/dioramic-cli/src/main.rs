//! `dioramic`: render, watch, lint, icons, mcp, init.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod cli;
mod commands;
mod init;
mod ui;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};

fn main() -> ExitCode {
    install_panic_hook();
    let cli = Cli::parse();
    match run(&cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            ui::report_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn run(command: &Command) -> anyhow::Result<()> {
    match command {
        Command::Render(args) => commands::render::run(args),
        Command::Watch(args) => commands::watch::run(args),
        Command::Lint(args) => commands::lint::run(args),
        Command::Icons(command) => commands::icons::run(command),
        Command::Mcp => commands::mcp::run(),
        Command::Init(args) => init::run(args),
    }
}

/// Turns a panic into a short message instead of a backtrace dump. Parts of the engine are still being
/// written (`todo!`), and a panic is always a bug worth reporting rather than a user mistake.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::env::var_os("RUST_BACKTRACE").is_some() {
            default_hook(info);
            return;
        }
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_owned());
        let place =
            info.location().map(|location| format!(" at {}:{}", location.file(), location.line())).unwrap_or_default();
        anstream::eprintln!("{} internal error: {message}{place}", ui::paint(ui::RED, "error:"));
        ui::print_hint(
            "this is a bug (or a part of dioramic that is not finished yet); run with RUST_BACKTRACE=1 and report it",
        );
    }));
}
