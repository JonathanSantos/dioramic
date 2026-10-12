//! `cargo xtask gallery`: renders every example × themes (default {blueprint, midnight}, `--themes all`
//! for the 4) × {0, 90, 180, 270} × {spaced, compact} into
//! `target/gallery/<example>/<theme>-r<rotation>-<density>.{svg,png}`, one contact sheet per example
//! (`contact-<example>.png`), optional zoomed crops (`--crop x,y,w,h`, one view with `--view`), and
//! `target/gallery/report.json`.
//!
//! Spec overrides for visual checks (`--routing rails`, `--layout hub`, `--grid-size 0.5`, `--grid-style dots`) rewrite
//! every example before rendering and send the output to `target/gallery-<overrides>/` instead.
//!
//! Stages that are not implemented yet (`todo!`, `RenderError::NotImplemented`) become one-line errors
//! in the report, the sheet and the console; the rest of the gallery still runs.

mod job;
mod panics;
pub(crate) mod pixels;
mod report;
mod sheet;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Args;
use dioramic_render::Engine;
use dioramic_spec::{
    Density, GridSpec, GridStyle, LayoutAlgorithm, LayoutSpec, Rotation, Routing, SourceFormat, ThemeName, ViewSpec,
};
use rayon::prelude::*;
use test_support::{Fixture, GALLERY_THEMES, View};

use crate::cargo::workspace_root;
use job::{AutoRotation, JobSettings, ViewOutcome, auto_rotation, render_view};
use pixels::Region;
use report::{AutoRotationReport, ExampleReport, RenderReport, Report, Totals};
use sheet::{SheetCell, contact_sheet};

/// Largest accepted `--zoom`.
const MAX_ZOOM: u32 = 16;
/// Distinct error lines printed at the end (the report has all of them).
const MAX_PRINTED_ERRORS: usize = 12;

/// Arguments of `cargo xtask gallery`.
#[derive(Debug, Args)]
pub struct GalleryArgs {
    /// Only examples whose name contains this text.
    #[arg(long)]
    filter: Option<String>,
    /// Also write a zoomed crop of each rendered PNG: `x,y,w,h` in PNG pixels. Every view has its own
    /// framing, so combine it with `--view` to look at one spot of one view.
    #[arg(long, value_name = "x,y,w,h", value_parser = parse_region)]
    crop: Option<Region>,
    /// Enlarge crops N× with nearest-neighbour (crisp pixels).
    #[arg(long, default_value_t = 1)]
    zoom: u32,
    /// PNG pixel density (1 = viewBox size).
    #[arg(long, default_value_t = 1.0)]
    scale: f64,
    /// Themes, comma separated (blueprint, midnight, paper, mono), or `all`. Default: blueprint,midnight.
    #[arg(long, alias = "theme", value_delimiter = ',')]
    themes: Vec<String>,
    /// Only this rotation (0, 90, 180, 270).
    #[arg(long)]
    rotation: Option<u16>,
    /// Only this density (spaced, compact).
    #[arg(long)]
    density: Option<String>,
    /// Only these views, by file stem (`midnight-r90-compact`, comma separated). Replaces the theme,
    /// rotation and density filters.
    #[arg(long, value_delimiter = ',', conflicts_with_all = ["themes", "rotation", "density"])]
    view: Vec<String>,
    /// Override `layout.routing` of every example (curved, orthogonal, straight, rails).
    #[arg(long, value_parser = parse_routing)]
    routing: Option<Routing>,
    /// Override `layout.algorithm` of every example (flow, hub).
    #[arg(long = "layout", value_parser = parse_layout_algorithm)]
    layout_algorithm: Option<LayoutAlgorithm>,
    /// Override `view.grid.size` of every example (tiles, 0.25 to 4).
    #[arg(long)]
    grid_size: Option<f64>,
    /// Override `view.grid.style` of every example (lines, dots, none).
    #[arg(long, value_parser = parse_grid_style)]
    grid_style: Option<GridStyle>,
}

/// A lowercase spec enum value (`rails`, `dots`) through its serde form.
fn parse_spec_enum<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(|_| format!("unknown {what} '{text}'"))
}

fn parse_routing(text: &str) -> Result<Routing, String> {
    parse_spec_enum(text, "routing")
}

fn parse_layout_algorithm(text: &str) -> Result<LayoutAlgorithm, String> {
    parse_spec_enum(text, "layout algorithm")
}

fn parse_grid_style(text: &str) -> Result<GridStyle, String> {
    parse_spec_enum(text, "grid style")
}

/// `--themes` value that selects every built-in theme.
const ALL_THEMES: &str = "all";

/// `--crop x,y,w,h` (the one documented syntax; w and h > 0).
fn parse_region(text: &str) -> Result<Region, String> {
    let numbers: Vec<u32> = text
        .split(',')
        .map(|part| part.trim().parse::<u32>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("expected x,y,w,h in PNG pixels (e.g. --crop 400,200,300,200), got '{text}'"))?;
    match numbers[..] {
        [x, y, width, height] if width > 0 && height > 0 => Ok(Region { x, y, width, height }),
        _ => Err(format!("expected x,y,w,h with w and h > 0 (e.g. --crop 400,200,300,200), got '{text}'")),
    }
}

impl GalleryArgs {
    /// `rails-grid0.5` for the overrides given (empty without overrides).
    fn overrides_tag(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(routing) = self.routing {
            parts.push(
                serde_json::to_value(routing)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default(),
            );
        }
        if let Some(algorithm) = self.layout_algorithm {
            parts.push(algorithm.as_str().to_owned());
        }
        if let Some(size) = self.grid_size {
            parts.push(format!("grid{size}"));
        }
        if let Some(style) = self.grid_style {
            parts.push(
                serde_json::to_value(style)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default(),
            );
        }
        parts.join("-")
    }

    /// Rewrites `fixture` with the spec overrides (as JSON, which the engine sniffs).
    fn apply_overrides(&self, mut fixture: Fixture) -> Result<Fixture> {
        if self.routing.is_none()
            && self.layout_algorithm.is_none()
            && self.grid_size.is_none()
            && self.grid_style.is_none()
        {
            return Ok(fixture);
        }
        let mut spec = dioramic_spec::parse(&fixture.source, SourceFormat::Yaml)
            .map_err(|diagnostics| anyhow::anyhow!("{}: {diagnostics}", fixture.name))?;
        if let Some(routing) = self.routing {
            spec.layout.get_or_insert_with(LayoutSpec::default).routing = Some(routing);
        }
        if let Some(algorithm) = self.layout_algorithm {
            spec.layout.get_or_insert_with(LayoutSpec::default).algorithm = Some(algorithm);
        }
        if self.grid_size.is_some() || self.grid_style.is_some() {
            let view = spec.view.get_or_insert_with(ViewSpec::default);
            let grid = view.grid.get_or_insert_with(GridSpec::default);
            grid.size = self.grid_size.or(grid.size);
            grid.style = self.grid_style.or(grid.style);
        }
        fixture.source = serde_json::to_string(&spec)?;
        Ok(fixture)
    }

    fn crop(&self) -> Result<Option<(Region, u32)>> {
        if !(1..=MAX_ZOOM).contains(&self.zoom) {
            bail!("--zoom must be between 1 and {MAX_ZOOM}");
        }
        Ok(self.crop.map(|region| (region, self.zoom)))
    }

    fn themes(&self) -> Result<Vec<ThemeName>> {
        if self.themes.is_empty() {
            return Ok(GALLERY_THEMES.to_vec());
        }
        if self.themes.iter().any(|name| name == ALL_THEMES) {
            return Ok(ThemeName::ALL.to_vec());
        }
        let mut themes: Vec<ThemeName> = Vec::new();
        for name in &self.themes {
            let theme = parse_named(name, &ThemeName::ALL, |theme| theme.as_str(), "theme")?;
            if !themes.contains(&theme) {
                themes.push(theme);
            }
        }
        Ok(themes)
    }

    fn views(&self) -> Result<Vec<View>> {
        if !self.view.is_empty() {
            return self.named_views();
        }
        let rotation = match self.rotation {
            Some(degrees) => Some(
                Rotation::ALL
                    .into_iter()
                    .find(|rotation| rotation.degrees() == degrees)
                    .with_context(|| format!("--rotation must be 0, 90, 180 or 270 (got {degrees})"))?,
            ),
            None => None,
        };
        let density = match &self.density {
            Some(name) => Some(parse_named(name, &Density::ALL, |density| density.as_str(), "density")?),
            None => None,
        };
        let views = View::matrix(&self.themes()?)
            .into_iter()
            .filter(|view| rotation.is_none_or(|rotation| view.rotation == rotation))
            .filter(|view| density.is_none_or(|density| view.density == density))
            .collect();
        Ok(views)
    }

    fn named_views(&self) -> Result<Vec<View>> {
        let mut views: Vec<View> = Vec::new();
        for label in &self.view {
            let view = View::parse_label(label).with_context(|| {
                format!("--view expects <theme>-r<rotation>-<density> (e.g. midnight-r90-compact), got '{label}'")
            })?;
            if !views.contains(&view) {
                views.push(view);
            }
        }
        Ok(views)
    }
}

fn parse_named<T: Copy>(name: &str, all: &[T], as_str: fn(T) -> &'static str, what: &str) -> Result<T> {
    all.iter().copied().find(|value| as_str(*value) == name).with_context(|| {
        let known: Vec<&str> = all.iter().map(|value| as_str(*value)).collect();
        format!("unknown {what} '{name}'; known: {}", known.join(", "))
    })
}

/// Everything rendered for one example.
struct ExampleRun {
    fixture: Fixture,
    outcomes: Vec<(View, ViewOutcome)>,
    auto: Vec<AutoRotation>,
    contact_sheet: Result<PathBuf, String>,
}

/// Runs the gallery; `Ok(false)` when a render stage failed.
pub fn run(args: &GalleryArgs) -> Result<bool> {
    let started = Instant::now();
    let crop = args.crop()?;
    let views = args.views()?;
    let fixtures = selected_fixtures(args.filter.as_deref())?
        .into_iter()
        .map(|fixture| args.apply_overrides(fixture))
        .collect::<Result<Vec<_>>>()?;
    let root = workspace_root();
    let tag = args.overrides_tag();
    let output = root.join("target").join(if tag.is_empty() { "gallery".to_owned() } else { format!("gallery-{tag}") });
    println!(
        "gallery: {} example(s) × {} view(s) = {} renders → {}",
        fixtures.len(),
        views.len(),
        fixtures.len() * views.len(),
        relative(&output, &root)
    );

    panics::install_quiet_hook();
    let runs = render_all(fixtures, &views, &output, args.scale, crop);
    panics::restore_default_hook();
    let runs = runs?;

    let mut totals = Totals::default();
    let examples: Vec<ExampleReport> = runs.iter().map(|run| example_report(run, &root, &mut totals)).collect();
    totals.wall_ms = report::round(started.elapsed().as_secs_f64() * 1000.0);
    print_console_summary(&runs, &root);

    let themes = themes_of(&views);
    let report = Report { filter: args.filter.clone(), scale: args.scale, themes, totals, examples };
    let report_path = output.join("report.json");
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)
        .with_context(|| format!("writing {}", report_path.display()))?;
    let totals = &report.totals;
    println!(
        "report: {} — {}/{} complete, {} with collisions, {} with arrow problems, {} edge crossings \
         ({} between edges sharing a node), bends mean {:.2} max {}, detour max {:.2} (center {:.2}), stray max {:.2}, \
         {} with excess bends, glued {:.2} tiles in {}, {} edges under node labels, {} double crossings ({:.1}s)",
        relative(&report_path, &root),
        totals.complete,
        totals.renders,
        totals.with_collisions,
        totals.with_arrow_problems,
        totals.crossings,
        totals.crossings_shared_node,
        totals.bends_mean,
        totals.bends_max,
        totals.detour_max,
        totals.detour_center_max,
        totals.stray_max,
        totals.with_excess_bends,
        totals.overlap_len,
        totals.with_overlap,
        totals.under_node_label,
        totals.double_crossings,
        totals.wall_ms / 1000.0
    );
    Ok(totals.failed == 0)
}

/// Theme names present in `views`, in order.
fn themes_of(views: &[View]) -> Vec<String> {
    let mut themes: Vec<String> = Vec::new();
    for view in views {
        let name = view.theme.as_str().to_owned();
        if !themes.contains(&name) {
            themes.push(name);
        }
    }
    themes
}

fn selected_fixtures(filter: Option<&str>) -> Result<Vec<Fixture>> {
    let names: Vec<String> = test_support::fixture_names()
        .into_iter()
        .filter(|name| filter.is_none_or(|filter| name.contains(filter)))
        .collect();
    if names.is_empty() {
        bail!("no example matches {:?} in examples/*.diorama.yaml", filter.unwrap_or(""));
    }
    Ok(names.iter().map(|name| test_support::fixture(name)).collect())
}

fn render_all(
    fixtures: Vec<Fixture>,
    views: &[View],
    output: &Path,
    scale: f64,
    crop: Option<(Region, u32)>,
) -> Result<Vec<ExampleRun>> {
    let engine = Engine::shared();
    for fixture in &fixtures {
        let directory = output.join(&fixture.name);
        std::fs::create_dir_all(&directory).with_context(|| format!("creating {}", directory.display()))?;
    }
    let jobs: Vec<(usize, View)> =
        (0..fixtures.len()).flat_map(|index| views.iter().map(move |view| (index, *view))).collect();
    let mut outcomes: Vec<(usize, View, ViewOutcome)> = jobs
        .par_iter()
        .map(|&(index, view)| {
            let fixture = &fixtures[index];
            let directory = output.join(&fixture.name);
            let settings = JobSettings { directory: &directory, scale, crop };
            (index, view, render_view(engine, fixture, view, settings))
        })
        .collect();
    let groups = auto_rotation_groups(views);
    let autos: Vec<(usize, AutoRotation)> = (0..fixtures.len())
        .flat_map(|index| groups.iter().map(move |group| (index, *group)))
        .collect::<Vec<_>>()
        .par_iter()
        .map(|&(index, (theme, density))| (index, auto_rotation(engine, &fixtures[index], theme, density)))
        .collect();

    let runs = fixtures
        .into_iter()
        .enumerate()
        .map(|(index, fixture)| {
            let mine: Vec<(View, ViewOutcome)> = take_where(&mut outcomes, |(owner, _, _)| *owner == index)
                .into_iter()
                .map(|(_, view, outcome)| (view, outcome))
                .collect();
            let auto = autos.iter().filter(|(owner, _)| *owner == index).map(|(_, auto)| auto.clone()).collect();
            let contact_sheet = write_contact_sheet(&fixture, &mine, &output.join(&fixture.name));
            ExampleRun { fixture, outcomes: mine, auto, contact_sheet }
        })
        .collect();
    Ok(runs)
}

fn auto_rotation_groups(views: &[View]) -> Vec<(ThemeName, Density)> {
    let mut groups: Vec<(ThemeName, Density)> = Vec::new();
    for view in views {
        if !groups.contains(&(view.theme, view.density)) {
            groups.push((view.theme, view.density));
        }
    }
    groups
}

/// Removes and returns the items matching `predicate`, keeping the order of both parts.
fn take_where<T>(items: &mut Vec<T>, predicate: impl Fn(&T) -> bool) -> Vec<T> {
    let (taken, kept): (Vec<T>, Vec<T>) = std::mem::take(items).into_iter().partition(|item| predicate(item));
    *items = kept;
    taken
}

fn write_contact_sheet(
    fixture: &Fixture,
    outcomes: &[(View, ViewOutcome)],
    directory: &Path,
) -> Result<PathBuf, String> {
    let cells: Vec<SheetCell<'_>> = outcomes.iter().map(|(view, outcome)| sheet_cell(*view, outcome)).collect();
    let complete = outcomes.iter().filter(|(_, outcome)| outcome.errors.is_empty()).count();
    let subtitle = format!(
        "{} views · {complete} complete · caption: collisions (px²) · worst arrow error · edges through nodes · \
         edge crossings (shared node) · longest label leader · label size / width · compile / render time",
        outcomes.len()
    );
    let path = directory.join(format!("contact-{}.png", fixture.name));
    let sheet = contact_sheet(&fixture.name, &subtitle, &cells).map_err(|error| format!("contact sheet: {error:#}"))?;
    let bytes = sheet.encode_png().map_err(|error| format!("contact sheet: {error}"))?;
    std::fs::write(&path, bytes).map_err(|error| format!("contact sheet: {}: {error}", path.display()))?;
    Ok(path)
}

fn sheet_cell(view: View, outcome: &ViewOutcome) -> SheetCell<'_> {
    let metrics = outcome.metrics.as_ref();
    let quality = match metrics {
        Some(metrics) => format!(
            "collisions {:.0} px² · arrows {:.2}°{}",
            metrics.collision_score,
            metrics.max_arrow_error_deg(),
            if metrics.arrow_problems.is_empty() {
                String::new()
            } else {
                format!(" ({} bad)", metrics.arrow_problems.len())
            }
        ),
        None => "no scene".to_owned(),
    };
    let legibility = metrics.map_or_else(String::new, |metrics| {
        let quality = &metrics.quality;
        format!(
            "through-node {} · cross {}/{} shared · bends {:.1}/{} · detour {:.2} · glued {:.2} · leader {:.0} px · text {:.2}%",
            quality.edge_through_node,
            quality.crossings,
            quality.crossings_shared_node,
            quality.bends_mean,
            quality.bends_max,
            quality.detour_max,
            quality.overlap_len,
            quality.max_leader_px,
            quality.text_to_width_percent
        )
    });
    let timing = format!(
        "compile {} · render {}{}",
        format_ms(outcome.compile_ms),
        format_ms(outcome.render_ms),
        outcome.png_size.map(|(width, height)| format!(" · {width}×{height}")).unwrap_or_default()
    );
    let alarm = !outcome.errors.is_empty() || metrics.is_some_and(job::SceneMetrics::has_problems);
    let legibility_alarm = metrics
        .is_some_and(|metrics| metrics.quality.edge_through_node > 0 || metrics.quality.crossings_shared_node > 0);
    let error = (!outcome.errors.is_empty()).then(|| outcome.errors.join(" · "));
    SheetCell {
        view,
        thumbnail: outcome.thumbnail.as_ref(),
        caption: [quality, legibility, timing],
        alarm,
        legibility_alarm,
        error,
    }
}

fn format_ms(milliseconds: Option<f64>) -> String {
    milliseconds.map_or_else(|| "—".to_owned(), |milliseconds| format!("{milliseconds:.0} ms"))
}

fn example_report(run: &ExampleRun, root: &Path, totals: &mut Totals) -> ExampleReport {
    for (_, outcome) in &run.outcomes {
        totals.add(outcome);
    }
    ExampleReport {
        name: run.fixture.name.clone(),
        contact_sheet: run.contact_sheet.as_ref().ok().map(|path| relative(path, root)),
        auto_rotation: run.auto.iter().map(AutoRotationReport::of).collect(),
        renders: run.outcomes.iter().map(|(view, outcome)| RenderReport::of(*view, outcome, root)).collect(),
    }
}

fn print_console_summary(runs: &[ExampleRun], root: &Path) {
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    for run in runs {
        let outcomes: Vec<&ViewOutcome> = run.outcomes.iter().map(|(_, outcome)| outcome).collect();
        let complete = outcomes.iter().filter(|outcome| outcome.errors.is_empty()).count();
        let metrics: Vec<&job::SceneMetrics> = outcomes.iter().filter_map(|outcome| outcome.metrics.as_ref()).collect();
        let worst_collision = metrics.iter().map(|metrics| metrics.collision_score).fold(0.0, f64::max);
        let worst_arrow = metrics.iter().map(|metrics| metrics.max_arrow_error_deg()).fold(0.0, f64::max);
        let render_times: Vec<f64> = outcomes.iter().filter_map(|outcome| outcome.render_ms).collect();
        let average = (!render_times.is_empty()).then(|| render_times.iter().sum::<f64>() / render_times.len() as f64);
        let sheet = match &run.contact_sheet {
            Ok(path) => relative(path, root),
            Err(error) => error.clone(),
        };
        let measured = if metrics.is_empty() {
            "no scene".to_owned()
        } else {
            format!("collisions max {worst_collision:.0} px²  arrows max {worst_arrow:.2}°")
        };
        println!(
            "  {:<22} {complete:>2}/{:<2} ok  {measured}  png avg {}  {sheet}",
            run.fixture.name,
            outcomes.len(),
            format_ms(average)
        );
        for outcome in outcomes {
            for error in &outcome.errors {
                *errors.entry(error.clone()).or_default() += 1;
            }
        }
    }
    print_errors(&errors);
}

fn print_errors(errors: &BTreeMap<String, usize>) {
    if errors.is_empty() {
        return;
    }
    let mut sorted: Vec<(&String, &usize)> = errors.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("errors ({} distinct; all of them are in report.json):", errors.len());
    for (error, count) in sorted.iter().take(MAX_PRINTED_ERRORS) {
        println!("  [{count:>3}×] {error}");
    }
    if sorted.len() > MAX_PRINTED_ERRORS {
        println!("  … {} more", sorted.len() - MAX_PRINTED_ERRORS);
    }
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        args: GalleryArgs,
    }

    fn parse(arguments: &[&str]) -> GalleryArgs {
        let mut all = vec!["gallery"];
        all.extend(arguments);
        Harness::try_parse_from(all).unwrap_or_else(|error| panic!("{error}")).args
    }

    fn try_parse(arguments: &[&str]) -> bool {
        let mut all = vec!["gallery"];
        all.extend(arguments);
        Harness::try_parse_from(all).is_ok()
    }

    #[test]
    fn crop_takes_one_comma_separated_region() {
        let expected = Some((Region { x: 10, y: 20, width: 300, height: 200 }, 4));
        assert_eq!(parse(&["--crop", "10,20,300,200", "--zoom", "4"]).crop().ok().flatten(), expected);
        assert_eq!(parse(&[]).crop().ok().flatten(), None);
    }

    #[test]
    fn malformed_crop_or_bad_zoom_is_rejected() {
        assert!(!try_parse(&["--crop", "10,20"]));
        assert!(!try_parse(&["--crop", "10", "20", "300", "200"]));
        assert!(!try_parse(&["--crop", "checkout", "1", "2", "3", "4"]));
        assert!(!try_parse(&["--crop", "1,2,0,4"]));
        assert!(parse(&["--crop", "1,2,3,4", "--zoom", "0"]).crop().is_err());
    }

    #[test]
    fn default_views_are_the_gallery_matrix() {
        assert_eq!(parse(&[]).views().ok(), Some(View::gallery_matrix()));
    }

    #[test]
    fn themes_accept_lists_and_all() {
        assert_eq!(parse(&["--themes", "all"]).views().ok(), Some(View::matrix(&ThemeName::ALL)));
        let themes = parse(&["--themes", "paper,mono,paper"]).themes().ok();
        assert_eq!(themes, Some(vec![ThemeName::Paper, ThemeName::Mono]));
        assert_eq!(parse(&["--theme", "mono"]).themes().ok(), Some(vec![ThemeName::Mono]));
        assert!(parse(&["--themes", "neon"]).views().is_err());
    }

    #[test]
    fn view_filters_narrow_the_matrix() {
        let views = parse(&["--themes", "midnight", "--rotation", "90", "--density", "compact"]).views().ok();
        assert_eq!(views, Some(vec![View::new(ThemeName::Midnight, Rotation::R90, Density::Compact)]));
        assert!(parse(&["--rotation", "45"]).views().is_err());
    }

    #[test]
    fn named_views_select_single_views() {
        let views = parse(&["--view", "paper-r270-compact,blueprint-r0-spaced"]).views().ok();
        assert_eq!(
            views,
            Some(vec![
                View::new(ThemeName::Paper, Rotation::R270, Density::Compact),
                View::new(ThemeName::Blueprint, Rotation::R0, Density::Spaced),
            ])
        );
        assert!(parse(&["--view", "midnight-r45-compact"]).views().is_err());
        assert!(!try_parse(&["--view", "paper-r0-spaced", "--density", "compact"]));
    }

    #[test]
    fn report_lists_the_themes_in_order() {
        assert_eq!(themes_of(&View::matrix(&[ThemeName::Mono, ThemeName::Paper])), ["mono", "paper"]);
    }

    #[test]
    fn take_where_splits_preserving_order() {
        let mut items = vec![1, 2, 3, 4, 5];
        assert_eq!(take_where(&mut items, |item| item % 2 == 0), [2, 4]);
        assert_eq!(items, [1, 3, 5]);
    }
}
