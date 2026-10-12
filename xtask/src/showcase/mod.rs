//! `cargo xtask showcase`: the README pictures of the `schematic` theme (T-425), with no Python in the loop.
//!
//! Renders each spec of `examples/schematic/` as the CLI does (`dioramic render <spec>`: the spec's own view,
//! PNG at scale 2), writes its thumbnail to `docs/assets/galeria-schematic/<name>.png` (720 px wide, 256
//! colors), assembles the mosaic `docs/assets/galeria-schematic.png` (two columns, a numbered tile per spec) and
//! renders `examples/web-platform.diorama.yaml` to `docs/assets/tema-schematic.png`.
//!
//! `--keep <names>` leaves the asset of those specs (or `tema`) untouched: its tile in the mosaic is drawn from
//! the thumbnail already on disk. Use it when a new render is not at least as good as the one published
//! (`docs/work/README.md`: assets only move forward). `--out <dir>` writes everything somewhere else (the same
//! layout), to compare before replacing.

mod indexed;
mod mosaic;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use dioramic_core::render_svg;
use dioramic_core::svg::SvgOptions;
use dioramic_render::raster::{RasterSize, rasterize};
use dioramic_render::{Engine, Pixmap, RenderRequest};
use rayon::prelude::*;

use crate::cargo::workspace_root;
use crate::gallery::pixels::fit_within;
use indexed::{decode_png, encode_indexed_png, encode_rgba_png};
use mosaic::{Tile, mosaic};

/// Pixel density of the renders, as `dioramic render` writes PNGs by default.
const RENDER_SCALE: f64 = 2.0;
/// Width of a README thumbnail (px).
const THUMBNAIL_WIDTH: u32 = 720;
/// The theme picture: the web platform as the theme draws it by default.
const THEME_SPEC: &str = "examples/web-platform.diorama.yaml";
/// `--keep` name of the theme picture.
const THEME_NAME: &str = "tema";
/// Where the gallery specs live, under the workspace.
const GALLERY_DIR: &str = "examples/schematic";
/// Suffix of a spec file.
const SPEC_SUFFIX: &str = ".diorama.yaml";

/// Arguments of `cargo xtask showcase`.
#[derive(Debug, Clone, Args)]
pub struct ShowcaseArgs {
    /// Leave these assets as they are (comma-separated spec names, or `tema` for the theme picture).
    #[arg(long, value_delimiter = ',')]
    pub keep: Vec<String>,
    /// Output directory (default `docs/assets`).
    #[arg(long)]
    pub out: Option<PathBuf>,
}

/// Renders and writes the showcase; true when every spec rendered.
pub fn run(args: &ShowcaseArgs) -> Result<bool> {
    let root = workspace_root();
    let out = args.out.clone().unwrap_or_else(|| root.join("docs").join("assets"));
    let thumbnails_dir = out.join("galeria-schematic");
    std::fs::create_dir_all(&thumbnails_dir).with_context(|| format!("creating {}", thumbnails_dir.display()))?;
    let names = gallery_names(&root)?;
    for kept in &args.keep {
        if kept != THEME_NAME && !names.contains(kept) {
            bail!("--keep {kept:?}: no {GALLERY_DIR}/{kept}{SPEC_SUFFIX} (nor `{THEME_NAME}`)");
        }
    }
    let kept = |name: &str| args.keep.iter().any(|kept| kept == name);

    let tiles: Vec<Tile> = names
        .par_iter()
        .map(|name| -> Result<Tile> {
            let thumbnail_path = thumbnails_dir.join(format!("{name}.png"));
            let image = if kept(name) {
                let bytes = std::fs::read(&thumbnail_path)
                    .with_context(|| format!("--keep {name}: reading {}", thumbnail_path.display()))?;
                decode_png(&bytes).with_context(|| format!("decoding {}", thumbnail_path.display()))?
            } else {
                let render = render_spec(&root.join(GALLERY_DIR).join(format!("{name}{SPEC_SUFFIX}")))?;
                let thumbnail = fit_within(&render, THUMBNAIL_WIDTH, u32::MAX)
                    .with_context(|| format!("{name}: empty thumbnail"))?;
                std::fs::write(&thumbnail_path, encode_indexed_png(&thumbnail)?)
                    .with_context(|| format!("writing {}", thumbnail_path.display()))?;
                println!(
                    "showcase: {} ({}×{})",
                    relative(&thumbnail_path, &root),
                    thumbnail.width(),
                    thumbnail.height()
                );
                render
            };
            Ok(Tile { number: number_of(name), image })
        })
        .collect::<Result<_>>()?;

    let mosaic_path = out.join("galeria-schematic.png");
    let picture = mosaic(&tiles)?;
    std::fs::write(&mosaic_path, encode_indexed_png(&picture)?)
        .with_context(|| format!("writing {}", mosaic_path.display()))?;
    println!("showcase: {} ({}×{})", relative(&mosaic_path, &root), picture.width(), picture.height());

    if !kept(THEME_NAME) {
        let theme_path = out.join("tema-schematic.png");
        let render = render_spec(&root.join(THEME_SPEC))?;
        std::fs::write(&theme_path, encode_rgba_png(&render)?)
            .with_context(|| format!("writing {}", theme_path.display()))?;
        println!("showcase: {} ({}×{})", relative(&theme_path, &root), render.width(), render.height());
    }
    Ok(true)
}

/// Names (file stems) of the gallery specs, sorted.
fn gallery_names(root: &Path) -> Result<Vec<String>> {
    let directory = root.join(GALLERY_DIR);
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .with_context(|| format!("listing {}", directory.display()))?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.strip_suffix(SPEC_SUFFIX).map(str::to_owned))
        .collect();
    names.sort();
    if names.is_empty() {
        bail!("no spec in {}", directory.display());
    }
    Ok(names)
}

/// The spec rendered as `dioramic render <spec>` does: its own theme, rotation and density, PNG at scale 2.
fn render_spec(path: &Path) -> Result<Pixmap> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let diagram = Engine::shared()
        .compile(&RenderRequest::from_text(text))
        .with_context(|| format!("compiling {}", path.display()))?;
    let svg = render_svg(&diagram, &SvgOptions { metadata: false, ..SvgOptions::default() });
    rasterize(&svg, RasterSize::Scale(RENDER_SCALE)).with_context(|| format!("rasterizing {}", path.display()))
}

/// The number a gallery spec name starts with (`07-video-upload` → `07`), or the whole name.
fn number_of(name: &str) -> String {
    name.split('-').next().unwrap_or(name).to_owned()
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Command {
        #[command(flatten)]
        args: ShowcaseArgs,
    }

    #[test]
    fn keep_takes_a_comma_separated_list() {
        let command = Command::parse_from(["showcase", "--keep", "04-url-shortener,tema"]);
        assert_eq!(command.args.keep, vec!["04-url-shortener".to_owned(), "tema".to_owned()]);
        assert!(command.args.out.is_none());
    }

    #[test]
    fn tiles_are_numbered_by_the_spec_prefix() {
        assert_eq!(number_of("07-video-upload"), "07");
        assert_eq!(number_of("minimal"), "minimal");
    }

    #[test]
    fn the_gallery_lists_the_schematic_specs_in_order() {
        let names = gallery_names(&workspace_root()).unwrap_or_else(|error| panic!("{error:#}"));
        assert!(names.len() >= 2, "{names:?}");
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]), "{names:?}");
    }
}
