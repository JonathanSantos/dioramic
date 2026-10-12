//! Background grid popover of the floating toolbar: style (lines, dots, none), cell size (½, 1, 2 tiles
//! and a fine slider) and opacity, with a small isometric floor that previews the choice. The grid is
//! view state like the camera: it reaches the spec only through "Pin" (`view.grid`).

use dioramic_desktop_ipc::{GridStyle, ViewGrid};
use leptos::html;
use leptos::prelude::*;

use crate::components::popover::{close_on_outside_press, keep_inside_preview};
use crate::icons::{Icon, IconView};
use crate::state::AppState;

/// Cell sizes offered as one-click chips (tiles), with their labels.
const SIZE_PRESETS: [(f64, &str); 3] = [(0.5, "½"), (1.0, "1"), (2.0, "2")];
/// Steps of the size slider. The slider is logarithmic: every step multiplies the size by the same
/// factor, so 0.25–0.5 gets as much travel as 2–4.
const SIZE_SLIDER_STEPS: f64 = 100.0;
/// Opacity slider step (percent).
const OPACITY_STEP_PERCENT: u32 = 5;

/// Slider position (0–[`SIZE_SLIDER_STEPS`]) of a cell size.
pub fn size_to_slider(size: f64) -> f64 {
    let span = (ViewGrid::MAX_SIZE / ViewGrid::MIN_SIZE).ln();
    ((size / ViewGrid::MIN_SIZE).ln() / span * SIZE_SLIDER_STEPS).clamp(0.0, SIZE_SLIDER_STEPS)
}

/// Cell size of a slider position.
pub fn slider_to_size(position: f64) -> f64 {
    let span = ViewGrid::MAX_SIZE / ViewGrid::MIN_SIZE;
    ViewGrid::MIN_SIZE * span.powf(position.clamp(0.0, SIZE_SLIDER_STEPS) / SIZE_SLIDER_STEPS)
}

/// `0.5 tile`, `2 tiles`.
pub fn size_label(size: f64) -> String {
    let unit = if (size - 1.0).abs() < f64::EPSILON || size < 1.0 { "tile" } else { "tiles" };
    format!("{} {unit}", trimmed(size))
}

/// `0.5`, `1`, `1.25`: two decimals at most, no trailing zeros.
fn trimmed(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn style_icon(style: GridStyle) -> Icon {
    match style {
        GridStyle::Lines => Icon::Grid,
        GridStyle::Dots => Icon::GridDots,
        GridStyle::None => Icon::GridNone,
    }
}

/// Side of the swatch floor, in plan tiles.
const SWATCH_TILES: f64 = 4.0;
/// Half the screen width of one tile in the swatch (px); a tile is twice as wide as it is tall.
const SWATCH_HALF_TILE: f64 = 21.0;
/// Top vertex of the floor in the swatch's viewBox.
const SWATCH_ORIGIN: (f64, f64) = (88.0, 8.0);
/// Height of the reference cube (px).
const SWATCH_CUBE_HEIGHT: f64 = 17.0;

/// Screen point of plan point (`u`, `v`) in the swatch.
fn swatch_point(u: f64, v: f64) -> (f64, f64) {
    (SWATCH_ORIGIN.0 + (u - v) * SWATCH_HALF_TILE, SWATCH_ORIGIN.1 + (u + v) * SWATCH_HALF_TILE / 2.0)
}

/// Grid positions `0, size, 2·size … ≤ SWATCH_TILES`.
fn grid_positions(size: f64) -> Vec<f64> {
    let count = (SWATCH_TILES / size + 1e-6).floor();
    // `count` is at most SWATCH_TILES / MIN_SIZE = 16.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (0..=count as u32).map(|index| f64::from(index) * size).collect()
}

/// A 4 × 4 tile isometric floor with the grid drawn on it and a one-tile cube for scale.
pub fn grid_swatch(grid: ViewGrid) -> String {
    let point = |u: f64, v: f64| {
        let (x, y) = swatch_point(u, v);
        format!("{x:.1} {y:.1}")
    };
    let floor = format!(
        "M{} L{} L{} L{} Z",
        point(0.0, 0.0),
        point(SWATCH_TILES, 0.0),
        point(SWATCH_TILES, SWATCH_TILES),
        point(0.0, SWATCH_TILES)
    );
    let positions = grid_positions(grid.size);
    let marks = match grid.style {
        GridStyle::Lines => {
            let mut path = String::new();
            for &k in &positions {
                path.push_str(&format!("M{} L{} ", point(k, 0.0), point(k, SWATCH_TILES)));
                path.push_str(&format!("M{} L{} ", point(0.0, k), point(SWATCH_TILES, k)));
            }
            format!(r#"<path d="{}" class="gs-line"/>"#, path.trim_end())
        }
        GridStyle::Dots => positions
            .iter()
            .flat_map(|&u| positions.iter().map(move |&v| swatch_point(u, v)))
            .map(|(x, y)| format!(r#"<circle cx="{x:.1}" cy="{y:.1}" r="1.4" class="gs-dot"/>"#))
            .collect(),
        GridStyle::None => String::new(),
    };
    format!(
        concat!(
            r#"<svg viewBox="0 0 176 104" width="176" height="104" aria-hidden="true">"#,
            r#"<path d="{floor}" class="gs-floor"/>"#,
            r#"<g opacity="{opacity:.2}">{marks}</g>"#,
            "{cube}",
            r#"</svg>"#
        ),
        floor = floor,
        opacity = grid.opacity,
        marks = marks,
        cube = swatch_cube(),
    )
}

/// One-tile cube standing in the middle of the floor: the size of a node, for scale.
fn swatch_cube() -> String {
    let (low, high) = (1.5, 2.5);
    let lift = |(x, y): (f64, f64)| (x, y - SWATCH_CUBE_HEIGHT);
    let path = |points: &[(f64, f64)]| {
        let body: Vec<String> = points.iter().map(|(x, y)| format!("{x:.1} {y:.1}")).collect();
        format!("M{} Z", body.join(" L"))
    };
    let (north, east, south, west) =
        (swatch_point(low, low), swatch_point(high, low), swatch_point(high, high), swatch_point(low, high));
    format!(
        r#"<path d="{}" class="gs-cube-left"/><path d="{}" class="gs-cube-right"/><path d="{}" class="gs-cube-top"/>"#,
        path(&[lift(west), west, south, lift(south)]),
        path(&[lift(south), south, east, lift(east)]),
        path(&[lift(north), lift(east), lift(south), lift(west)]),
    )
}

/// Grid button and its popover.
#[component]
pub fn GridPicker() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.grid_open;
    let grid = move || state.grid();
    let has_override = move || state.view.with(|view| view.grid.is_some());

    let popover: NodeRef<html::Div> = NodeRef::new();
    // Click anywhere outside the picker closes the popover (sliders need a popover that stays put).
    close_on_outside_press(open, ".grid-picker");
    keep_inside_preview(popover);

    view! {
        <div class="grid-picker">
            <button
                class="tb-btn labeled grid-btn"
                class:active=move || open.get()
                class:changed=has_override
                aria-label="Background grid"
                aria-haspopup="dialog"
                aria-expanded=move || open.get().to_string()
                data-tip="Background grid"
                data-kbd="G"
                on:click=move |_| state.toggle_grid_popover()
            >
                <IconView icon=Icon::Grid />
                <span class="tb-label">"Grid"</span>
            </button>
            <Show when=move || open.get()>
                <div class="popover grid-popover" role="dialog" aria-label="Background grid" node_ref=popover>
                    <div class="popover-head">
                        <span class="popover-title">"Grid"</span>
                        <div class="popover-head-actions">
                            <button
                                class="text-btn"
                                disabled=move || !has_override()
                                data-tip="Back to what the spec says"
                                on:click=move |_| state.reset_grid()
                            >
                                "Reset"
                            </button>
                            <button class="icon-btn small" aria-label="Close" on:click=move |_| open.set(false)>
                                <IconView icon=Icon::Close />
                            </button>
                        </div>
                    </div>
                    <div class="grid-swatch" inner_html=move || grid_swatch(grid())></div>
                    <div class="field">
                        <span class="field-label">"Style"</span>
                        <div class="segmented small" role="radiogroup" aria-label="Grid style">
                            {GridStyle::ALL
                                .into_iter()
                                .map(|style| {
                                    view! {
                                        <button
                                            class="seg"
                                            role="radio"
                                            aria-checked=move || (grid().style == style).to_string()
                                            class:active=move || grid().style == style
                                            on:click=move |_| state.update_grid(|grid| ViewGrid { style, ..grid })
                                        >
                                            <IconView icon=style_icon(style) />
                                            {style.label()}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>
                    <div class="grid-controls" class:muted=move || grid().style == GridStyle::None>
                        <div class="field">
                            <span class="field-label">"Cell"</span>
                            <div class="segmented small" role="radiogroup" aria-label="Cell size">
                                {SIZE_PRESETS
                                    .into_iter()
                                    .map(|(size, label)| {
                                        let active = move || (grid().size - size).abs() < f64::EPSILON;
                                        view! {
                                            <button
                                                class="seg size-chip"
                                                role="radio"
                                                aria-label=size_label(size)
                                                aria-checked=move || active().to_string()
                                                class:active=active
                                                on:click=move |_| state.update_grid(|grid| grid.with_size(size))
                                            >
                                                {label}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </div>
                        <label class="slider-row">
                            <span class="field-label"></span>
                            <input
                                type="range"
                                class="slider"
                                aria-label="Cell size"
                                min="0"
                                max=SIZE_SLIDER_STEPS.to_string()
                                step="1"
                                prop:value=move || format!("{:.0}", size_to_slider(grid().size))
                                on:input=move |event| {
                                    if let Ok(position) = event_target_value(&event).parse::<f64>() {
                                        state.update_grid(|grid| grid.with_size(slider_to_size(position)));
                                    }
                                }
                            />
                            <span class="slider-value">{move || size_label(grid().size)}</span>
                        </label>
                        <label class="slider-row">
                            <span class="field-label">"Opacity"</span>
                            <input
                                type="range"
                                class="slider"
                                aria-label="Grid opacity"
                                min="0"
                                max="100"
                                step=OPACITY_STEP_PERCENT.to_string()
                                prop:value=move || format!("{:.0}", grid().opacity * 100.0)
                                on:input=move |event| {
                                    if let Ok(percent) = event_target_value(&event).parse::<f64>() {
                                        state.update_grid(|grid| grid.with_opacity(percent / 100.0));
                                    }
                                }
                            />
                            <span class="slider-value">{move || format!("{:.0}%", grid().opacity * 100.0)}</span>
                        </label>
                    </div>
                    <p class="popover-note">"Cell side in floor tiles (a node is about one). Pin writes view.grid."</p>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_slider_is_logarithmic_and_round_trips() {
        assert!((slider_to_size(0.0) - ViewGrid::MIN_SIZE).abs() < 1e-9);
        assert!((slider_to_size(SIZE_SLIDER_STEPS) - ViewGrid::MAX_SIZE).abs() < 1e-9);
        assert!((slider_to_size(50.0) - 1.0).abs() < 1e-9, "1 tile sits in the middle");
        for size in [0.25, 0.5, 1.0, 2.0, 3.3, 4.0] {
            assert!((slider_to_size(size_to_slider(size)) - size).abs() < 1e-9);
        }
    }

    #[test]
    fn size_labels_read_naturally() {
        assert_eq!(size_label(0.5), "0.5 tile");
        assert_eq!(size_label(1.0), "1 tile");
        assert_eq!(size_label(2.0), "2 tiles");
        assert_eq!(size_label(1.25), "1.25 tiles");
    }

    #[test]
    fn swatch_draws_the_chosen_style_and_density() {
        let lines = grid_swatch(ViewGrid { size: 1.0, style: GridStyle::Lines, opacity: 1.0 });
        assert!(lines.contains("gs-line") && !lines.contains("gs-dot"));
        let dots = grid_swatch(ViewGrid { size: 0.5, style: GridStyle::Dots, opacity: 0.6 });
        assert_eq!(dots.matches("gs-dot").count(), 81, "9 × 9 intersections on a 4-tile floor");
        assert!(dots.contains(r#"opacity="0.60""#));
        let none = grid_swatch(ViewGrid { style: GridStyle::None, ..ViewGrid::default() });
        assert!(!none.contains("gs-line") && !none.contains("gs-dot") && none.contains("gs-cube-top"));
    }

    #[test]
    fn grid_positions_cover_the_floor() {
        assert_eq!(grid_positions(1.0), [0.0, 1.0, 2.0, 3.0, 4.0]);
        assert_eq!(grid_positions(0.25).len(), 17);
        assert_eq!(grid_positions(3.0), [0.0, 3.0]);
    }
}
