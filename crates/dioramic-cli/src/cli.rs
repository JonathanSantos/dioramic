//! Command-line interface definition (clap derive). Behavior lives in [`crate::commands`] and [`crate::init`].

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use dioramic_render::OutputFormat;
use dioramic_spec::{ColorRef, Density, GridStyle, LayoutAlgorithm, RotationSpec, Routing, ThemeName};

const AFTER_HELP: &str = "\
Examples:
  dioramic render diagrams/checkout.diorama.yaml                 # → diagrams/checkout.png
  dioramic render checkout.diorama.yaml -o checkout.gif          # format from the extension
  dioramic render checkout.diorama.yaml --routing rails --grid-style dots
  dioramic watch checkout.diorama.yaml --rotate auto
  dioramic lint diagrams/*.diorama.yaml
  dioramic icons search postgres
  dioramic icons install aws                                     # provider icons: icon: aws/lambda
  dioramic init                                                  # MCP server + skill for this project

Colors follow NO_COLOR / CLICOLOR_FORCE and are disabled when the output is not a terminal.";

/// Isometric system design diagrams from a declarative spec.
#[derive(Debug, Parser)]
#[command(name = "dioramic", version, propagate_version = true, after_help = AFTER_HELP)]
pub struct Cli {
    /// Subcommand.
    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Render a spec to PNG, SVG, animated SVG or GIF.
    #[command(after_help = "\
The format comes from --format, else from the extension of --output, else PNG.
Without --output the file is written next to the spec: checkout.diorama.yaml → checkout.png.
View options (--theme, --rotate, --density, --layout, --routing, --grid-*, --no-icon-animation) override the spec
for this render only; the spec file is not changed.

Examples:
  dioramic render checkout.diorama.yaml                                  # → checkout.png
  dioramic render checkout.diorama.yaml -o checkout.gif                  # animated, icons move
  dioramic render checkout.diorama.yaml --routing rails                  # isometric rails, rounded bends
  dioramic render checkout.diorama.yaml --layout hub                     # busiest nodes in the middle, short edges
  dioramic render web.diorama.yaml --theme schematic --layout flow       # schematic lays out with hub by default
  dioramic render checkout.diorama.yaml --routing rails --grid-size 0.5 --grid-style dots --grid-opacity 0.6
  dioramic render checkout.diorama.yaml -o checkout.gif --no-icon-animation   # only the packets move
  dioramic render checkout.diorama.yaml --theme midnight --grid-style none -o dark.svg")]
    Render(RenderArgs),
    /// Re-render a spec every time it changes (Ctrl+C to stop).
    #[command(after_help = "Spec errors are printed and the watcher keeps waiting for the next save.")]
    Watch(RenderArgs),
    /// Validate specs and print diagnostics, without rasterizing.
    #[command(after_help = "Exit status: 1 when any spec has errors (E_*), 0 when there are only warnings or hints.")]
    Lint(LintArgs),
    /// Browse the icon catalog and manage provider icon packs (AWS, Google Cloud, Azure, Kubernetes).
    #[command(subcommand)]
    Icons(IconsCommand),
    /// Run the MCP server on stdio (used by Claude Code, VS Code, Claude Desktop).
    #[command(
        after_help = "You normally do not run this by hand: `dioramic init` configures your AI clients to start it."
    )]
    Mcp,
    /// Configure the MCP server and the Dioramic skill for Claude Code and VS Code Copilot.
    #[command(after_help = "\
Without --claude/--vscode/--desktop the targets are detected (.claude/, CLAUDE.md or `claude` on PATH;
.vscode/ or `code` on PATH); when nothing is detected both are configured.
Running it again changes nothing. Existing entries that differ are never overwritten without --force,
and other servers, keys and comments in the config files are preserved.

Examples:
  dioramic init                       # this project, detected clients
  dioramic init --claude --vscode     # both, explicitly
  dioramic init --global --claude     # user scope (~/.claude.json, ~/.claude/skills)
  dioramic init --desktop             # Claude Desktop
  dioramic init --dry-run             # show the plan and a diff, write nothing")]
    Init(InitArgs),
}

/// Arguments of `render` and `watch`.
#[derive(Debug, Clone, Args)]
pub struct RenderArgs {
    /// Spec file (`*.diorama.yaml` or `*.diorama.json`).
    pub spec: PathBuf,
    /// Output file [default: next to the spec, extension from the format].
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Output format [default: from the --output extension, else png].
    #[arg(short, long, value_enum)]
    pub format: Option<FormatArg>,
    /// Theme override.
    #[arg(long, value_enum)]
    pub theme: Option<ThemeArg>,
    /// Camera rotation: 0, 90, 180, 270 or auto (fewest collisions).
    #[arg(long = "rotate", value_name = "ANGLE", value_parser = parse_rotation)]
    pub rotation: Option<RotationSpec>,
    /// Layout density override.
    #[arg(long, value_enum)]
    pub density: Option<DensityArg>,
    /// Placement override (`layout.algorithm`) [default: the spec's, else hub for schematic, flow otherwise].
    #[arg(long = "layout", value_enum, value_name = "ALGORITHM")]
    pub layout_algorithm: Option<LayoutArg>,
    /// Edge routing override (`layout.routing`); edges with their own `route` keep it.
    #[arg(long, value_enum, value_name = "ROUTING")]
    pub routing: Option<RoutingArg>,
    /// Background grid cell side in plan tiles, 0.25 to 4 (`view.grid.size`, default 1).
    #[arg(long, value_name = "TILES")]
    pub grid_size: Option<f64>,
    /// Background grid style (`view.grid.style`).
    #[arg(long, value_enum, value_name = "STYLE")]
    pub grid_style: Option<GridStyleArg>,
    /// Background grid opacity, 0 to 1 (`view.grid.opacity`, default 1).
    #[arg(long, value_name = "0..1")]
    pub grid_opacity: Option<f64>,
    /// Background grid color: a palette token (`blue`, `slate`) or `#rrggbb` (`view.grid.color`).
    #[arg(long, value_name = "COLOR", value_parser = parse_color)]
    pub grid_color: Option<ColorRef>,
    /// Keep the icons still in GIF / animated SVG (LEDs, waves and rings); packets still flow.
    #[arg(long)]
    pub no_icon_animation: bool,
    /// PNG pixel density (0 < scale ≤ 4).
    #[arg(long, default_value_t = 2.0)]
    pub scale: f64,
    /// Output width in px (PNG/GIF; GIF default 1600).
    #[arg(long, value_name = "PX")]
    pub width: Option<u32>,
}

fn parse_rotation(text: &str) -> Result<RotationSpec, String> {
    text.parse()
}

/// Same rules as `color:` in the spec: a palette token or a hex literal.
fn parse_color(text: &str) -> Result<ColorRef, String> {
    serde_json::from_value(serde_json::Value::String(text.trim().to_owned()))
        .map_err(|_| format!("`{text}` is not a palette token (blue, orange, slate…) or a #rrggbb color"))
}

impl RenderArgs {
    /// `--no-icon-animation` as an override (`None` = what the spec says).
    pub fn icon_animation(&self) -> Option<bool> {
        self.no_icon_animation.then_some(false)
    }
}

/// `--routing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RoutingArg {
    /// Smooth curves (default).
    Curved,
    /// Grid-following paths with rounded corners.
    Orthogonal,
    /// Straight segments.
    Straight,
    /// Isometric rails: axis-aligned legs, rounded 90° bends, fewest bends (alias: elbow).
    #[value(alias = "elbow")]
    Rails,
}

/// `--layout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LayoutArg {
    /// Zones in flow order, in a row or folded (default of the classic themes).
    Flow,
    /// Busiest nodes in the middle, short aligned edges, packed zones (default of schematic).
    Hub,
}

impl From<LayoutArg> for LayoutAlgorithm {
    fn from(layout: LayoutArg) -> Self {
        match layout {
            LayoutArg::Flow => Self::Flow,
            LayoutArg::Hub => Self::Hub,
        }
    }
}

impl From<RoutingArg> for Routing {
    fn from(routing: RoutingArg) -> Self {
        match routing {
            RoutingArg::Curved => Self::Curved,
            RoutingArg::Orthogonal => Self::Orthogonal,
            RoutingArg::Straight => Self::Straight,
            RoutingArg::Rails => Self::Rails,
        }
    }
}

/// `--grid-style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GridStyleArg {
    /// Lines along the plan axes (default).
    Lines,
    /// A dot at every intersection.
    Dots,
    /// No grid.
    None,
}

impl From<GridStyleArg> for GridStyle {
    fn from(style: GridStyleArg) -> Self {
        match style {
            GridStyleArg::Lines => Self::Lines,
            GridStyleArg::Dots => Self::Dots,
            GridStyleArg::None => Self::None,
        }
    }
}

/// `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatArg {
    /// Raster image.
    Png,
    /// Static vector image.
    Svg,
    /// Vector image with CSS/SMIL animation.
    SvgAnimated,
    /// Looping animation (1600 px, 50 fps).
    Gif,
}

impl From<FormatArg> for OutputFormat {
    fn from(format: FormatArg) -> Self {
        match format {
            FormatArg::Png => Self::Png,
            FormatArg::Svg => Self::Svg,
            FormatArg::SvgAnimated => Self::SvgAnimated,
            FormatArg::Gif => Self::Gif,
        }
    }
}

/// `--theme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ThemeArg {
    /// Light blue-grey background with grid.
    Blueprint,
    /// Dark navy background, neon edges.
    Midnight,
    /// Warm off-white, pastel.
    Paper,
    /// Greyscale, no shadows (print).
    Mono,
    /// Technical drawing: silver icons, thick blue rails, semantic edge colors.
    Schematic,
}

impl From<ThemeArg> for ThemeName {
    fn from(theme: ThemeArg) -> Self {
        match theme {
            ThemeArg::Blueprint => Self::Blueprint,
            ThemeArg::Midnight => Self::Midnight,
            ThemeArg::Paper => Self::Paper,
            ThemeArg::Mono => Self::Mono,
            ThemeArg::Schematic => Self::Schematic,
        }
    }
}

/// `--density`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DensityArg {
    /// Zones in one line along the flow, generous gaps.
    Spaced,
    /// Zones folded in a serpentine block (~58% less area).
    Compact,
}

impl From<DensityArg> for Density {
    fn from(density: DensityArg) -> Self {
        match density {
            DensityArg::Spaced => Self::Spaced,
            DensityArg::Compact => Self::Compact,
        }
    }
}

/// Arguments of `lint`.
#[derive(Debug, Args)]
pub struct LintArgs {
    /// Spec files.
    #[arg(required = true, value_name = "SPEC")]
    pub specs: Vec<PathBuf>,
    /// Print the diagnostics as JSON (one object per file).
    #[arg(long)]
    pub json: bool,
}

/// `icons` subcommands.
#[derive(Debug, Subcommand)]
pub enum IconsCommand {
    /// Search icons by id, name, alias or category (built-in and installed packs).
    Search {
        /// Query (`postgres`, `message queue`, `aws lambda`).
        query: String,
        /// Only this category (compute, network, security, data, messaging, cloud, clients, devices, k8s, observability, generic).
        #[arg(long)]
        category: Option<String>,
        /// Only this pack (`builtin`, `aws`, `gcp`, `azure`, `k8s`, `local`).
        #[arg(long)]
        pack: Option<String>,
        /// Maximum results.
        #[arg(long, default_value_t = 12)]
        limit: usize,
    },
    /// List the catalog (built-in icons and installed packs), or the installed packs with --packs.
    List {
        /// Only this category.
        #[arg(long)]
        category: Option<String>,
        /// Only this pack (`builtin`, `aws`, `gcp`, `azure`, `k8s`, `local`).
        #[arg(long)]
        pack: Option<String>,
        /// List the installed packs (id, icons, version, source, license) instead of icons.
        #[arg(long)]
        packs: bool,
    },
    /// Install a provider icon pack from the provider's official download (or from a local copy).
    #[command(after_help = "\
Packs go to $XDG_DATA_HOME/dioramic/icons/<pack> (default ~/.local/share/dioramic/icons, also on macOS).
Dioramic does not redistribute provider icons: they are downloaded from the provider, under its terms.
Without --from the official archive is downloaded with `curl`. With --from, pass the archive you
downloaded yourself (zip), a directory with the extracted files (or with the zips), or an https URL.

Examples:
  dioramic icons install aws
  dioramic icons install k8s
  dioramic icons install azure --from ~/Downloads/Azure_Public_Service_Icons_V24.zip
  dioramic icons install gcp --from ~/Downloads/gcp-icons/      # folder with the 3 Google Cloud zips")]
    Install {
        /// Pack: aws, gcp, azure or k8s.
        pack: String,
        /// Local archive (zip), directory or https URL to read instead of the official download (repeatable).
        #[arg(long, value_name = "ZIP|DIR|URL")]
        from: Vec<String>,
    },
    /// Remove an installed icon pack.
    Remove {
        /// Pack id (`aws`, `gcp`, `azure`, `k8s`).
        pack: String,
    },
}

/// Arguments of `init`. Flags follow `docs/work/research/clients.md` §4.1.
#[derive(Debug, Clone, Default, Args)]
pub struct InitArgs {
    /// Configure Claude Code (`.mcp.json` + `.claude/skills/dioramic/SKILL.md`).
    #[arg(long)]
    pub claude: bool,
    /// Configure VS Code Copilot (`.vscode/mcp.json` + skill).
    #[arg(long)]
    pub vscode: bool,
    /// Write the user-scope files (home directory) instead of the project files.
    #[arg(long)]
    pub global: bool,
    /// Configure Claude Desktop (always user scope; macOS and Windows).
    #[arg(long)]
    pub desktop: bool,
    /// Also write `.github/instructions/dioramic.instructions.md` (VS Code, applied to *.diorama.* files).
    #[arg(long)]
    pub instructions: bool,
    /// Show the plan and a diff without writing anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Replace existing Dioramic entries that differ (other entries are never touched).
    #[arg(long)]
    pub force: bool,
    /// `command` written in the configs [default: `dioramic` in project files, absolute path in user ones].
    #[arg(long, value_name = "PATH")]
    pub command: Option<String>,
    /// Project directory [default: current directory].
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn rotation_accepts_angles_and_auto() {
        let cli = Cli::try_parse_from(["dioramic", "render", "a.yaml", "--rotate", "auto"]);
        assert!(matches!(
            cli.map(|cli| cli.command),
            Ok(Command::Render(RenderArgs { rotation: Some(RotationSpec::Auto), .. }))
        ));
        assert!(Cli::try_parse_from(["dioramic", "render", "a.yaml", "--rotate", "45"]).is_err());
    }

    fn render_args(arguments: &[&str]) -> RenderArgs {
        let mut argv = vec!["dioramic", "render", "a.yaml"];
        argv.extend_from_slice(arguments);
        match Cli::try_parse_from(argv).map(|cli| cli.command) {
            Ok(Command::Render(args)) => args,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn routing_grid_and_icon_motion_flags_parse() {
        let args = render_args(&[
            "--routing",
            "rails",
            "--grid-size",
            "0.5",
            "--grid-style",
            "dots",
            "--grid-opacity",
            "0.6",
            "--grid-color",
            "#3e50c8",
            "--no-icon-animation",
        ]);
        assert_eq!(args.routing.map(Routing::from), Some(Routing::Rails));
        assert_eq!((args.grid_size, args.grid_opacity), (Some(0.5), Some(0.6)));
        assert_eq!(args.grid_style.map(GridStyle::from), Some(GridStyle::Dots));
        assert!(matches!(args.grid_color, Some(ColorRef::Hex(_))));
        assert_eq!(args.icon_animation(), Some(false));
    }

    #[test]
    fn layout_flag_parses_and_rails_takes_its_alias() {
        assert_eq!(
            render_args(&["--layout", "hub"]).layout_algorithm.map(LayoutAlgorithm::from),
            Some(LayoutAlgorithm::Hub)
        );
        assert_eq!(
            render_args(&["--layout", "flow"]).layout_algorithm.map(LayoutAlgorithm::from),
            Some(LayoutAlgorithm::Flow)
        );
        assert_eq!(render_args(&[]).layout_algorithm, None);
        assert!(Cli::try_parse_from(["dioramic", "render", "a.yaml", "--layout", "grid"]).is_err());
        assert_eq!(render_args(&["--routing", "elbow"]).routing.map(Routing::from), Some(Routing::Rails));
    }

    #[test]
    fn view_flags_default_to_the_spec() {
        let args = render_args(&[]);
        assert_eq!((args.routing, args.grid_size, args.grid_style, args.icon_animation()), (None, None, None, None));
        assert!(matches!(render_args(&["--grid-color", "teal"]).grid_color, Some(ColorRef::Token(_))));
        assert!(Cli::try_parse_from(["dioramic", "render", "a.yaml", "--routing", "zigzag"]).is_err());
    }
}
