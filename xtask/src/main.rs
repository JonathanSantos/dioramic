//! `cargo xtask`: dev mini-engine for UI and QA (`docs/work/README.md#mini-engine-para-ui-e-qa`).
//! Contract: C-007. Implementation: T-106.
//!
//! - `qa <crate|all>`: fmt check + clippy `-D warnings` + tests of one crate, with a short summary.
//! - `gallery`: every example × 2 themes × 4 rotations × 2 densities, contact sheets, crops, `report.json`.
//! - `golden [--update]`: runs the golden tests of the workspace (optionally rewriting `tests/golden/`).
//! - `showcase [--keep <names>] [--out <dir>]`: the `schematic` pictures of the README (gallery thumbnails, mosaic,
//!   theme picture) rendered and assembled in Rust.

mod cargo;
mod gallery;
mod golden;
mod qa;
mod showcase;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Dioramic dev tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// fmt --check + clippy -D warnings + tests of one crate.
    Qa {
        /// Crate name (`dioramic-core`, or just `core`) or `all`.
        krate: String,
    },
    /// Render every example × 2 themes × 4 rotations × 2 densities into target/gallery/ (+ contact sheets, report.json).
    Gallery(gallery::GalleryArgs),
    /// Render examples/schematic/ into the README thumbnails and mosaic (docs/assets/galeria-schematic*) and
    /// the theme picture (docs/assets/tema-schematic.png).
    Showcase(showcase::ShowcaseArgs),
    /// Compare (or with --update, regenerate) tests/golden/.
    Golden {
        /// Rewrite the golden files.
        #[arg(long)]
        update: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match cli.command {
        Command::Qa { krate } => qa::run(&krate),
        Command::Gallery(args) => gallery::run(&args),
        Command::Golden { update } => golden::run(update),
        Command::Showcase(args) => showcase::run(&args),
    };
    match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("xtask: {error:#}");
            ExitCode::from(2)
        }
    }
}
