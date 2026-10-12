//! GIF budget measurement (T-105 acceptance): size < 2 MB, mean PSNR ≥ 45 dB against the RGBA frames,
//! seamless loop, timing. T-321: no banding stairs in the cloud shadow, no shimmer on static pixels.
//! Ignored by default (release build recommended):
//!
//! ```text
//! cargo test -p dioramic-render --release --test gif_quality -- --ignored --nocapture
//! ```
//!
//! Frames come from `examples/checkout.diorama.yaml` through the engine. Set `DIORAMIC_FRAMES_DIR` to a
//! folder with `<theme>/000.svg … 100.svg` (frame 100 = t 1.0) to measure pre-rendered frames instead, e.g.
//! the ones produced by the spike (`docs/spike/scene.mjs`). Decoded frames and the GIF are written to
//! `DIORAMIC_GIF_OUT` (default: the system temp dir) for visual inspection.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::path::PathBuf;
use std::time::Instant;

use dioramic_core::CompileOptions;
use dioramic_core::animation::{AnimationOverrides, AnimationPlan};
use dioramic_core::svg::{SvgMode, SvgOptions, emit_svg};
use dioramic_render::gif::{GifOptions, encode_gif};
use dioramic_render::png::encode_png;
use dioramic_render::raster::{RasterSize, rasterize};
use dioramic_render::{GIF_DEFAULT_WIDTH_PX, Pixmap};
use dioramic_spec::{AnimationPreset, SourceFormat, ThemeName};
use rayon::prelude::*;

const FRAMES: usize = 100;
const MAX_GIF_BYTES: usize = 2 * 1024 * 1024;
const MIN_MEAN_PSNR_DB: f64 = 45.0;
/// Highest luminance step (0–255 levels) a smooth shadow ramp may collapse into (T-321).
const MAX_STAIR_LEVELS: f64 = 3.0;
/// Side of the box that averages the dither pattern away (the Bayer matrix period).
const DITHER_PERIOD_PX: usize = 4;
/// Rays from the shadow center are this long (the shadow radius at 1600 px is ≈ 90 px).
const RAY_LENGTH_PX: usize = 110;
/// Smoothed decoded luminance changing less than this between ray samples counts as a flat band.
const FLAT_BAND_LEVELS: f64 = 0.25;
/// Node whose soft shadow and radial gradients showed rings before T-321.
const SHADOW_NODE: &str = "users";

fn output_dir() -> PathBuf {
    let dir =
        std::env::var_os("DIORAMIC_GIF_OUT").map_or_else(|| std::env::temp_dir().join("dioramic-gif"), PathBuf::from);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Pixel (at the GIF width) under the floor center of [`SHADOW_NODE`], where its blurred shadow is centered.
type ShadowCenter = (usize, usize);

/// `FRAMES + 1` SVGs: loop times 0, 1/N, …, (N-1)/N and 1.0 (to check the seam), plus the shadow center
/// (unknown for pre-rendered frames).
fn frame_svgs(theme: ThemeName, theme_dir: &str) -> (Vec<String>, Option<ShadowCenter>) {
    if let Some(dir) = std::env::var_os("DIORAMIC_FRAMES_DIR") {
        let dir = PathBuf::from(dir).join(theme_dir);
        let svgs =
            (0..=FRAMES).map(|index| std::fs::read_to_string(dir.join(format!("{index:03}.svg"))).unwrap()).collect();
        return (svgs, None);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/checkout.diorama.yaml");
    let source = std::fs::read_to_string(root).unwrap();
    let options = CompileOptions { theme: Some(theme), ..CompileOptions::default() };
    let diagram = dioramic_core::compile_source(&source, SourceFormat::Yaml, &options).unwrap();
    let plan = diagram.animation.clone().unwrap_or_else(|| {
        let flow = AnimationOverrides { preset: Some(AnimationPreset::Flow), ..AnimationOverrides::default() };
        AnimationPlan::from_model(&diagram.model, &flow).unwrap()
    });
    assert_eq!(plan.frame_count() as usize, FRAMES);
    let svgs = (0..=FRAMES)
        .map(|index| {
            let t = index as f64 / FRAMES as f64;
            let options = SvgOptions { mode: SvgMode::Frame { t }, metadata: false, ..SvgOptions::default() };
            emit_svg(&diagram.scene, Some(&plan), &options)
        })
        .collect();
    (svgs, Some(shadow_center(&diagram)))
}

fn shadow_center(diagram: &dioramic_core::Diagram) -> ShadowCenter {
    let node = diagram.scene.nodes.iter().find(|node| node.id.as_str() == SHADOW_NODE).unwrap();
    let viewbox = diagram.scene.viewbox;
    let scale = f64::from(GIF_DEFAULT_WIDTH_PX) / (viewbox.x1 - viewbox.x0);
    let pixel = |value: f64, origin: f64| ((value - origin) * scale).round() as usize;
    (pixel(node.floor_center.x, viewbox.x0), pixel(node.floor_center.y, viewbox.y0))
}

fn luminance(color: [u8; 3]) -> f64 {
    0.2126 * f64::from(color[0]) + 0.7152 * f64::from(color[1]) + 0.0722 * f64::from(color[2])
}

/// Mean luminance of the [`DITHER_PERIOD_PX`]² box around `(x, y)`.
fn box_luminance(image: &[[u8; 3]], width: usize, (x, y): (usize, usize)) -> f64 {
    let half = DITHER_PERIOD_PX / 2;
    let rows = y - half..y + half;
    let sum: f64 = rows
        .flat_map(|row| (x - half..x + half).map(move |column| row * width + column))
        .map(|at| luminance(image[at]))
        .sum();
    sum / (DITHER_PERIOD_PX * DITHER_PERIOD_PX) as f64
}

/// Tallest "stair" along rays out of the shadow: the source luminance change over a run where the decoded
/// image (dither averaged out) stays flat. Banding shows as long flat runs over a moving source.
fn largest_shadow_stair(source: &[[u8; 3]], decoded: &[[u8; 3]], width: usize, center: ShadowCenter) -> f64 {
    let directions: [(isize, isize); 5] = [(-1, 0), (0, 1), (-1, 1), (1, 0), (1, 1)];
    directions
        .into_iter()
        .map(|(dx, dy)| {
            let ray: Vec<(usize, usize)> = (0..RAY_LENGTH_PX as isize)
                .map(|step| ((center.0 as isize + dx * step) as usize, (center.1 as isize + dy * step) as usize))
                .collect();
            let wanted: Vec<f64> = ray.iter().map(|&point| box_luminance(source, width, point)).collect();
            let shown: Vec<f64> = ray.iter().map(|&point| box_luminance(decoded, width, point)).collect();
            tallest_flat_run(&wanted, &shown)
        })
        .fold(0.0, f64::max)
}

fn tallest_flat_run(wanted: &[f64], shown: &[f64]) -> f64 {
    let mut tallest = 0.0_f64;
    let mut start = 0;
    for end in 1..=shown.len() {
        if end == shown.len() || (shown[end] - shown[end - 1]).abs() >= FLAT_BAND_LEVELS {
            let run = &wanted[start..end];
            let height = run.iter().copied().fold(f64::MIN, f64::max) - run.iter().copied().fold(f64::MAX, f64::min);
            tallest = tallest.max(height);
            start = end;
        }
    }
    tallest
}

/// Pixels whose source did not change between consecutive frames but whose decoded color did (shimmer).
fn shimmering_pixels(frames: &[Pixmap], canvases: &[Vec<[u8; 3]>]) -> usize {
    (1..frames.len())
        .map(|index| {
            let source = frames[index - 1].pixels().iter().zip(frames[index].pixels());
            let shown = canvases[index - 1].iter().zip(&canvases[index]);
            source.zip(shown).filter(|((before, now), (was, is))| before == now && was != is).count()
        })
        .sum()
}

fn rgb(pixmap: &Pixmap) -> Vec<[u8; 3]> {
    pixmap
        .pixels()
        .iter()
        .map(|pixel| {
            let uncovered = 255 - pixel.alpha();
            [pixel.red() + uncovered, pixel.green() + uncovered, pixel.blue() + uncovered]
        })
        .collect()
}

/// Composites every GIF frame (dispose keep) and returns the canvases as RGB.
fn decode_canvases(bytes: &[u8]) -> (usize, Vec<Vec<[u8; 3]>>) {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(std::io::Cursor::new(bytes)).unwrap();
    let width = usize::from(decoder.width());
    let mut canvas = vec![[0_u8; 3]; width * usize::from(decoder.height())];
    let mut canvases = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        for row in 0..usize::from(frame.height) {
            for column in 0..usize::from(frame.width) {
                let source = (row * usize::from(frame.width) + column) * 4;
                if frame.buffer[source + 3] == 0 {
                    continue;
                }
                let target = (usize::from(frame.top) + row) * width + usize::from(frame.left) + column;
                canvas[target] = [frame.buffer[source], frame.buffer[source + 1], frame.buffer[source + 2]];
            }
        }
        canvases.push(canvas.clone());
    }
    (width, canvases)
}

fn psnr(expected: &[[u8; 3]], actual: &[[u8; 3]]) -> f64 {
    let squared: f64 = expected
        .iter()
        .zip(actual)
        .flat_map(|(left, right)| left.iter().zip(right).map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2)))
        .sum();
    let mse = squared / (expected.len() * 3) as f64;
    if mse == 0.0 { f64::INFINITY } else { 10.0 * (255.0_f64 * 255.0 / mse).log10() }
}

fn save_canvas(width: usize, canvas: &[[u8; 3]], path: PathBuf) {
    let height = canvas.len() / width;
    let mut pixmap = Pixmap::new(width as u32, height as u32).unwrap();
    for (pixel, color) in pixmap.pixels_mut().iter_mut().zip(canvas) {
        *pixel = resvg::tiny_skia::PremultipliedColorU8::from_rgba(color[0], color[1], color[2], 255).unwrap();
    }
    std::fs::write(path, encode_png(&pixmap).unwrap()).unwrap();
}

fn measure(theme: ThemeName, name: &str) {
    let (svgs, shadow) = frame_svgs(theme, name);
    let started = Instant::now();
    let mut frames: Vec<Pixmap> =
        svgs.par_iter().map(|svg| rasterize(svg, RasterSize::Width(GIF_DEFAULT_WIDTH_PX)).unwrap()).collect();
    let raster_ms = started.elapsed().as_millis();
    let seam = frames.pop().unwrap();
    let seam_diff = frames[0].pixels().iter().zip(seam.pixels()).filter(|(a, b)| a != b).count();

    let encode_started = Instant::now();
    let bytes = encode_gif(&frames, &GifOptions::default()).unwrap();
    let encode_ms = encode_started.elapsed().as_millis();

    let (width, canvases) = decode_canvases(&bytes);
    assert_eq!(canvases.len(), FRAMES);
    let mean_psnr =
        frames.iter().zip(&canvases).map(|(frame, canvas)| psnr(&rgb(frame), canvas)).sum::<f64>() / FRAMES as f64;

    let stair = shadow.map(|center| largest_shadow_stair(&rgb(&frames[0]), &canvases[0], width, center));
    let shimmer = shimmering_pixels(&frames, &canvases);
    let stair_report = match (shadow, stair) {
        (Some(center), Some(stair)) => format!("shadow stair {stair:.2} levels at {center:?}"),
        _ => "shadow stair n/a".to_owned(),
    };

    let out = output_dir();
    std::fs::write(out.join(format!("{name}.gif")), &bytes).unwrap();
    std::fs::write(out.join(format!("{name}-source-000.png")), encode_png(&frames[0]).unwrap()).unwrap();
    for index in [0, 37, 99] {
        save_canvas(width, &canvases[index], out.join(format!("{name}-decoded-{index:03}.png")));
    }
    println!(
        "{name}: {}×{} {FRAMES} frames, {:.0} KB, mean PSNR {mean_psnr:.1} dB, seam diff {seam_diff} px, \
         {stair_report}, shimmer {shimmer} px, raster {raster_ms} ms (101 frames), \
         encode {encode_ms} ms → {}",
        frames[0].width(),
        frames[0].height(),
        bytes.len() as f64 / 1024.0,
        out.display()
    );
    assert!(bytes.len() < MAX_GIF_BYTES, "{name}: {} bytes", bytes.len());
    assert!(mean_psnr >= MIN_MEAN_PSNR_DB, "{name}: PSNR {mean_psnr:.1} dB");
    assert_eq!(seam_diff, 0, "{name}: frame 0 and frame {FRAMES} differ");
    assert_eq!(shimmer, 0, "{name}: static pixels change between frames");
    if let Some(stair) = stair {
        assert!(stair <= MAX_STAIR_LEVELS, "{name}: shadow bands into {stair:.2}-level stairs");
    }
}

#[test]
#[ignore = "measurement: run with --release -- --ignored --nocapture"]
fn checkout_gif_meets_budget_blueprint() {
    measure(ThemeName::Blueprint, "blueprint");
}

#[test]
#[ignore = "measurement: run with --release -- --ignored --nocapture"]
fn checkout_gif_meets_budget_midnight() {
    measure(ThemeName::Midnight, "midnight");
}

/// Acceptance timings (release, Mac arm64): PNG @2x < 300 ms, GIF 100 frames < 3 s, through `Engine::render`.
#[test]
#[ignore = "measurement: run with --release -- --ignored --nocapture"]
fn engine_render_timings_on_checkout() {
    use dioramic_render::{Engine, OutputFormat, PreviewMode, RenderRequest};

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/checkout.diorama.yaml");
    let source = std::fs::read_to_string(path).unwrap();
    let engine = Engine::shared();
    for format in [OutputFormat::Png, OutputFormat::Gif] {
        let mut request = RenderRequest::from_text(source.clone());
        request.format = format;
        request.preview = PreviewMode::None;
        engine.render(&request).unwrap(); // warm-up: fonts, thread pool
        let started = Instant::now();
        let result = engine.render(&request).unwrap();
        let elapsed = started.elapsed();
        let summary = &result.summary;
        println!("{format:?}: {}×{}, {} KB in {elapsed:?}", summary.width, summary.height, summary.bytes / 1024);
        let budget_ms = if format == OutputFormat::Png { 300 } else { 3000 };
        assert!(elapsed.as_millis() < budget_ms, "{format:?} took {elapsed:?}");
    }
}
