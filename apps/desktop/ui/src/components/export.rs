//! Export popover, anchored under the toolbar's Export button: format, one row of options whose
//! height never changes, a summary of exactly what will be written, then the native save dialog.

use dioramic_desktop_ipc::{ExportFormat, ViewLayout, ViewRouting};
use leptos::html;
use leptos::prelude::*;

use crate::components::popover::keep_inside_preview;

use crate::icons::{Icon, IconView};
use crate::state::{AppState, ExportState};

/// PNG pixel densities offered.
const PNG_SCALES: [u8; 4] = [1, 2, 3, 4];
/// Default PNG density (retina).
const DEFAULT_PNG_SCALE: u8 = 2;
/// GIF width the engine renders (`dioramic_render::GIF_DEFAULT_WIDTH_PX`).
const GIF_WIDTH_PX: f64 = 1600.0;
/// GIF frame rate of the high-quality preset.
const GIF_FPS: u32 = 50;

fn format_icon(format: ExportFormat) -> Icon {
    match format {
        ExportFormat::Png => Icon::Image,
        ExportFormat::Svg => Icon::Vector,
        ExportFormat::Gif => Icon::Film,
    }
}

/// Two short lines at most, in every popover width.
fn format_caption(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Png => "Image for docs and slides",
        ExportFormat::Svg => "Vector, sharp at any size",
        ExportFormat::Gif => "Animated loop",
    }
}

/// Output size in px of an export of a diagram whose viewBox is `size`.
pub fn output_size(format: ExportFormat, size: (f64, f64), scale: u8) -> (u32, u32) {
    let (width, height) = match format {
        ExportFormat::Png => (size.0 * f64::from(scale), size.1 * f64::from(scale)),
        ExportFormat::Svg => size,
        ExportFormat::Gif if size.0 > 0.0 => (GIF_WIDTH_PX, GIF_WIDTH_PX * size.1 / size.0),
        ExportFormat::Gif => (GIF_WIDTH_PX, 0.0),
    };
    // Rounded pixel sizes of a diagram are far inside u32.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (width.round().max(0.0) as u32, height.round().max(0.0) as u32)
}

/// `2170 × 1526 px`.
fn size_label(size: (u32, u32)) -> String {
    format!("{} × {} px", size.0, size.1)
}

/// Popover anchored under the toolbar's Export button.
#[component]
pub fn ExportPopover() -> impl IntoView {
    let state = expect_context::<AppState>();
    let format = RwSignal::new(ExportFormat::Png);
    let scale = RwSignal::new(DEFAULT_PNG_SCALE);
    let popover: NodeRef<html::Div> = NodeRef::new();
    keep_inside_preview(popover);
    let summary = move || {
        let mut view = format!(
            "{} · {}° · {}",
            state.diagram_theme().label(),
            state.rotation_degrees(),
            state.density().spec_value()
        );
        if state.layout() == ViewLayout::Hub {
            view.push_str(" · hub");
        }
        if state.routing() == ViewRouting::Rails {
            view.push_str(" · rails");
        }
        match state.diagram_size.get() {
            Some(size) => format!("{} · {view}", size_label(output_size(format.get(), size, scale.get()))),
            None => view,
        }
    };

    move || match state.export.get() {
        ExportState::Closed => None,
        ExportState::Choosing => Some(
            view! {
                <div class="popover export-popover" role="dialog" aria-label="Export" node_ref=popover>
                    <div class="popover-head">
                        <span class="popover-title">"Export"</span>
                        <button
                            class="icon-btn small"
                            aria-label="Close"
                            on:click=move |_| state.export.set(ExportState::Closed)
                        >
                            <IconView icon=Icon::Close />
                        </button>
                    </div>
                    <div class="format-cards" role="radiogroup" aria-label="Format">
                        {ExportFormat::ALL
                            .into_iter()
                            .map(|option| {
                                view! {
                                    <button
                                        class="format-card"
                                        role="radio"
                                        aria-checked=move || (format.get() == option).to_string()
                                        class:active=move || format.get() == option
                                        on:click=move |_| format.set(option)
                                    >
                                        <IconView icon=format_icon(option) class="format-icon" />
                                        <span class="format-name">{option.label()}</span>
                                        <span class="format-caption">{format_caption(option)}</span>
                                    </button>
                                }
                            })
                            .collect_view()}
                    </div>
                    <div class="field">{move || format_options(format.get(), scale)}</div>
                    <p class="export-summary">
                        <IconView icon=Icon::Check class="summary-icon" />
                        <span>{summary}</span>
                    </p>
                    <div class="popover-actions">
                        <button class="btn ghost small" on:click=move |_| state.export.set(ExportState::Closed)>
                            "Cancel"
                        </button>
                        <button
                            class="btn primary small"
                            on:click=move |_| state.export(format.get_untracked(), f64::from(scale.get_untracked()))
                        >
                            {move || format!("Export {}…", format.get().label())}
                        </button>
                    </div>
                </div>
            }
            .into_any(),
        ),
        ExportState::Running { format: running, .. } => Some(
            view! {
                <div class="popover export-popover running" role="status" node_ref=popover>
                    <div class="popover-head">
                        <span class="popover-title">{format!("Exporting {}", running.label())}</span>
                    </div>
                    <div class="progress"><span></span></div>
                    <p class="popover-note">
                        {if running == ExportFormat::Gif {
                            "Rendering every frame with a shared palette. This can take a few seconds."
                        } else {
                            "Choose where to save, then the file is written."
                        }}
                    </p>
                    <div class="popover-actions">
                        <button class="btn ghost small" on:click=move |_| state.cancel_export()>"Cancel"</button>
                    </div>
                </div>
            }
            .into_any(),
        ),
    }
}

/// The one options row: density for PNG, facts for SVG and GIF (same height for all three).
fn format_options(format: ExportFormat, scale: RwSignal<u8>) -> AnyView {
    match format {
        ExportFormat::Png => view! {
            <span class="field-label">"Pixel density"</span>
            <div class="segmented small" role="radiogroup" aria-label="Pixel density">
                {PNG_SCALES
                    .into_iter()
                    .map(|option| {
                        view! {
                            <button
                                class="seg"
                                role="radio"
                                aria-checked=move || (scale.get() == option).to_string()
                                class:active=move || scale.get() == option
                                on:click=move |_| scale.set(option)
                            >
                                {format!("{option}×")}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
        }
        .into_any(),
        ExportFormat::Svg => view! {
            <span class="field-label">"Text"</span>
            <span class="field-value">"Outlined, no fonts needed"</span>
        }
        .into_any(),
        ExportFormat::Gif => view! {
            <span class="field-label">"Animation"</span>
            <span class="field-value">{format!("{GIF_FPS} fps · seamless loop")}</span>
        }
        .into_any(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_sizes_follow_each_format() {
        let size = (1085.1, 762.8);
        assert_eq!(output_size(ExportFormat::Png, size, 2), (2170, 1526));
        assert_eq!(output_size(ExportFormat::Svg, size, 4), (1085, 763));
        assert_eq!(output_size(ExportFormat::Gif, size, 2), (1600, 1125));
        assert_eq!(output_size(ExportFormat::Gif, (0.0, 0.0), 1), (1600, 0));
    }

    #[test]
    fn size_label_reads_naturally() {
        assert_eq!(size_label((2170, 1526)), "2170 × 1526 px");
    }
}
