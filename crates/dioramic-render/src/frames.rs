//! Animation frames rendered in parallel (`rayon`): each frame is `emit_svg(scene, Frame { t })` → raster.

use dioramic_core::Diagram;
use dioramic_core::animation::AnimationPlan;
use dioramic_core::svg::{SvgMode, SvgOptions, emit_svg};
use rayon::prelude::*;
use resvg::tiny_skia::Pixmap;

use crate::MAX_FRAMES;
use crate::error::RenderError;
use crate::raster::{RasterSize, rasterize};

/// Renders every frame of `plan` (in order) at `size`. Errors above [`crate::MAX_FRAMES`].
pub fn render_frames(diagram: &Diagram, plan: &AnimationPlan, size: RasterSize) -> Result<Vec<Pixmap>, RenderError> {
    let count = checked_frame_count(plan)?;
    let times: Vec<f64> = (0..count).map(|index| plan.frame_time(index)).collect();
    render_frames_at(diagram, plan, &times, size)
}

/// Frame count of `plan`, or [`RenderError::Limit`] above [`MAX_FRAMES`].
pub(crate) fn checked_frame_count(plan: &AnimationPlan) -> Result<u32, RenderError> {
    let count = plan.frame_count();
    if count > MAX_FRAMES {
        return Err(RenderError::Limit { what: "frames", actual: u64::from(count), limit: u64::from(MAX_FRAMES) });
    }
    Ok(count)
}

/// Renders the frames at the given normalized times, in parallel, preserving order.
pub(crate) fn render_frames_at(
    diagram: &Diagram,
    plan: &AnimationPlan,
    times: &[f64],
    size: RasterSize,
) -> Result<Vec<Pixmap>, RenderError> {
    times.par_iter().map(|&t| rasterize(&frame_svg(diagram, plan, t), size)).collect()
}

/// Static SVG of the scene at loop time `t` (text as `<text>`: resvg draws it with the embedded fonts;
/// no vignette: it bands with a 256-color palette).
pub(crate) fn frame_svg(diagram: &Diagram, plan: &AnimationPlan, t: f64) -> String {
    let options = SvgOptions { mode: SvgMode::Frame { t }, vignette: false, metadata: false, ..SvgOptions::default() };
    emit_svg(&diagram.scene, Some(plan), &options)
}

/// `count` indices evenly spread over `0..frame_count`, starting at 0, without duplicates
/// (palette samples, filmstrip key frames).
pub(crate) fn evenly_spaced_indices(frame_count: usize, count: usize) -> Vec<usize> {
    let count = count.clamp(1, frame_count.max(1));
    let mut indices: Vec<usize> = (0..count).map(|slot| slot * frame_count / count).collect();
    indices.dedup();
    indices
}

#[cfg(test)]
mod tests {
    use dioramic_spec::AnimationPreset;

    use super::*;

    fn plan(fps: u32, duration_s: f64) -> AnimationPlan {
        AnimationPlan { preset: AnimationPreset::Flow, duration_s, fps, looped: true, flowing_edges: Vec::new() }
    }

    #[test]
    fn frame_limit_is_enforced() {
        assert_eq!(checked_frame_count(&plan(50, 2.0)).unwrap(), 100);
        let error = checked_frame_count(&plan(50, 13.0)).unwrap_err();
        assert!(matches!(error, RenderError::Limit { what: "frames", actual: 650, .. }), "{error}");
    }

    #[test]
    fn indices_are_evenly_spaced() {
        assert_eq!(evenly_spaced_indices(100, 4), vec![0, 25, 50, 75]);
        assert_eq!(evenly_spaced_indices(3, 4), vec![0, 1, 2]);
        assert_eq!(evenly_spaced_indices(1, 0), vec![0]);
    }
}
