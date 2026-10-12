//! Zones in screen space, content bounds, auto-fit `viewBox` and title block
//! (`docs/06-renderizacao.md#enquadramento`).

use dioramic_spec::Model;

use crate::geometry::{PlanPoint, Projection, Rect, ScreenPoint};
use crate::layout::PlanLayout;
use crate::text::{self, FontFamily, FontSpec};
use crate::theme::Theme;

use super::{SceneEdge, SceneNode, SceneOptions, SceneZone, TitleBlock};

/// Title position inside the reserved title space (px from the viewBox top-left; prototype values).
pub const TITLE_INSET_X: f64 = 48.0;
/// Baseline of the title below the viewBox top (px).
pub const TITLE_BASELINE_Y: f64 = 66.0;
/// Baseline of the subtitle below the title baseline (px).
pub const SUBTITLE_STEP_Y: f64 = 26.0;
/// Title and subtitle weights.
pub const TITLE_WEIGHT: u16 = 700;
/// Subtitle weight.
pub const SUBTITLE_WEIGHT: u16 = 400;

/// Font of the title.
pub fn title_font(theme: &Theme) -> FontSpec {
    FontSpec { family: FontFamily::Inter, weight: TITLE_WEIGHT, size: theme.typography.title_size, letter_spacing: 0.0 }
}

/// Font of the subtitle.
pub fn subtitle_font(theme: &Theme) -> FontSpec {
    FontSpec {
        family: FontFamily::Inter,
        weight: SUBTITLE_WEIGHT,
        size: theme.typography.subtitle_size,
        letter_spacing: 0.0,
    }
}

/// Zones of the rotated layout projected to the floor (parents first, as the layout orders them).
pub fn scene_zones(
    model: &Model,
    base: &PlanLayout,
    turned: &PlanLayout,
    theme: &Theme,
    projection: &Projection,
) -> Vec<SceneZone> {
    turned
        .zones
        .iter()
        .map(|zone| {
            let rect = zone.rect;
            let corner = |x: f64, y: f64| projection.project(PlanPoint::new(x, y), 0.0);
            let spec = model.zones.get(zone.index);
            SceneZone {
                id: zone.id.clone(),
                polygon: [
                    corner(rect.x0, rect.y0),
                    corner(rect.x1, rect.y0),
                    corner(rect.x1, rect.y1),
                    corner(rect.x0, rect.y1),
                ],
                plan_rect: base
                    .zones
                    .iter()
                    .find(|original| original.id == zone.id)
                    .map_or(rect, |original| original.rect),
                colors: theme.zone_colors(spec.and_then(|zone| zone.color.as_ref()), zone.index),
                border: zone.border,
                depth: zone.depth,
                label: None,
            }
        })
        .collect()
}

/// Bounding box of everything drawn: zones and their labels, nodes, leaders and labels, edges, heads, pills.
pub fn content_bounds(zones: &[SceneZone], nodes: &[SceneNode], edges: &[SceneEdge]) -> Rect {
    let mut bounds = Rect::EMPTY;
    for zone in zones {
        bounds = bounds.union(Rect::from_points(zone.polygon));
        if let Some(label) = &zone.label {
            bounds = bounds.union(label.rect);
        }
    }
    for node in nodes {
        bounds = bounds.union(node.shape.screen_bbox);
        if let Some(label) = &node.label {
            bounds = bounds.union(label.rect);
        }
    }
    for edge in edges {
        bounds = bounds.union(edge.path.bounds().inflate(edge.width / 2.0));
        for head in edge.heads.iter().chain(&edge.midway) {
            bounds = bounds.union(Rect::from_points([head.tip, head.base[0], head.base[1]]));
        }
        if let Some(label) = &edge.label {
            bounds = bounds.union(label.rect);
        }
    }
    if bounds.is_empty() { Rect::new(0.0, 0.0, 0.0, 0.0) } else { bounds }
}

/// `viewBox` = content + padding (+ title space on top when there is a title, wide enough for it).
pub fn frame(model: &Model, content: Rect, theme: &Theme, options: &SceneOptions) -> (Rect, Option<TitleBlock>) {
    let title = model.title.as_deref().filter(|title| !title.trim().is_empty());
    let top = if title.is_some() { options.title_space_px } else { options.padding_px };
    let mut viewbox = Rect::new(
        content.x0 - options.padding_px,
        content.y0 - top,
        content.x1 + options.padding_px,
        content.y1 + options.padding_px,
    );
    let Some(title) = title else {
        return (viewbox, None);
    };
    let subtitle = model.subtitle.clone().filter(|subtitle| !subtitle.trim().is_empty());
    let title_width = text::measure(title, &title_font(theme)).width;
    let subtitle_width =
        subtitle.as_deref().map_or(0.0, |subtitle| text::measure(subtitle, &subtitle_font(theme)).width);
    let needed = viewbox.x0 + TITLE_INSET_X + title_width.max(subtitle_width) + options.padding_px;
    viewbox.x1 = viewbox.x1.max(needed);
    let position = ScreenPoint::new(viewbox.x0 + TITLE_INSET_X, viewbox.y0 + TITLE_BASELINE_Y);
    (viewbox, Some(TitleBlock { title: title.to_owned(), subtitle, position }))
}
