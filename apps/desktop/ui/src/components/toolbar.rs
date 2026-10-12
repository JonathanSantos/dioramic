//! Floating toolbar over the preview: camera (⟲ ⟳ Auto), density, layout (flow ⟷ hub), connections (curves ⟷ rails),
//! background grid, diagram theme, motion, pin, export.
//!
//! The bar fits any preview the workbench allows (≥ `PREVIEW_MIN_PX`) through container-query steps on the preview
//! pane, never by wrapping: first the text labels fold away (icons keep their tooltips and shortcuts), then
//! density, layout and connections fold into one "View" popover where they get their names back. The bar never
//! changes width as state changes, and every popover is kept inside the pane ([`crate::components::popover`]).

use dioramic_desktop_ipc::{DiagramTheme, ViewDensity, ViewLayout, ViewRotation, ViewRouting};
use leptos::html;
use leptos::prelude::*;

use crate::components::export::ExportPopover;
use crate::components::grid::GridPicker;
use crate::components::popover::{close_on_outside_press, keep_inside_preview};
use crate::icons::{Icon, IconView};
use crate::state::{AppState, ExportState};

/// Floating toolbar and the export popover anchored to it.
#[component]
pub fn Toolbar() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_auto = move || state.view.with(|view| view.rotation == Some(ViewRotation::Auto));
    let has_overrides = move || state.has_view_overrides();
    let pin_tip = move || {
        let fields = state.pending_pin_fields();
        if fields.is_empty() {
            "Pin the view into the spec\nNothing to pin: the view matches the spec".to_owned()
        } else {
            format!("Pin the view into the spec\nWrites {}", fields.join(", "))
        }
    };
    let motion_on = move || state.motion_active();
    let motion_tip = move || {
        if state.reduced_motion.get() {
            "Motion preview\nOff: the system asks to reduce motion"
        } else if motion_on() {
            "Motion preview: on\nPackets and icons move, as in the GIF"
        } else {
            "Motion preview\nPackets and icons move, as in the GIF"
        }
    };
    let toggle_export = move |_| {
        let current = state.export.get_untracked();
        state.close_popovers();
        state.export.set(match current {
            ExportState::Closed => ExportState::Choosing,
            ExportState::Choosing => ExportState::Closed,
            running @ ExportState::Running { .. } => running,
        });
    };

    view! {
        <div class="toolbar-dock">
            <div class="toolbar" role="toolbar" aria-label="View">
                <div class="tb-group">
                    <button
                        class="tb-btn"
                        aria-label="Rotate left"
                        data-tip="Rotate left"
                        data-kbd="["
                        on:click=move |_| state.rotate(-1)
                    >
                        <IconView icon=Icon::RotateLeft />
                    </button>
                    <span class="tb-readout" class:auto=is_auto aria-live="polite">
                        {move || format!("{}°", state.rotation_degrees())}
                    </span>
                    <button
                        class="tb-btn"
                        aria-label="Rotate right"
                        data-tip="Rotate right"
                        data-kbd="]"
                        on:click=move |_| state.rotate(1)
                    >
                        <IconView icon=Icon::RotateRight />
                    </button>
                    <button
                        class="tb-btn labeled"
                        class:active=is_auto
                        aria-label="Auto angle"
                        aria-pressed=move || is_auto().to_string()
                        data-tip="Best angle, fewest overlaps"
                        data-kbd="A"
                        on:click=move |_| state.toggle_auto()
                    >
                        <IconView icon=Icon::Auto />
                        <span class="tb-label">"Auto"</span>
                    </button>
                </div>
                <span class="tb-sep"></span>
                // Inline on a wide preview; folded into the View popover on a narrow one (CSS).
                <div class="tb-view-inline">
                    <DensitySegments />
                    <span class="tb-sep"></span>
                    <LayoutSegments />
                    <span class="tb-sep"></span>
                    <RoutingSegments />
                </div>
                <ViewPicker />
                <GridPicker />
                <span class="tb-sep"></span>
                <ThemePicker />
                <button
                    class="tb-btn motion-btn"
                    class:active=motion_on
                    disabled=move || state.reduced_motion.get()
                    aria-label="Motion preview"
                    aria-pressed=move || motion_on().to_string()
                    data-tip=motion_tip
                    data-kbd="M"
                    on:click=move |_| state.toggle_motion()
                >
                    <IconView icon=Icon::Motion />
                </button>
                <button
                    class="tb-btn pin-btn"
                    class:attention=has_overrides
                    disabled=move || !has_overrides()
                    aria-label=move || {
                        if has_overrides() { "Pin view into the spec" } else { "Pin view (nothing to pin)" }
                    }
                    data-tip=pin_tip
                    on:click=move |_| state.pin_view()
                >
                    <IconView icon=Icon::Pin />
                    <span class="pin-dot" aria-hidden="true"></span>
                </button>
                <span class="tb-sep"></span>
                <button
                    class="btn primary small export-btn"
                    aria-haspopup="dialog"
                    aria-expanded=move || (!matches!(state.export.get(), ExportState::Closed)).to_string()
                    data-tip="Export"
                    data-kbd="⌘ E"
                    on:click=toggle_export
                >
                    <IconView icon=Icon::Export />
                    <span class="tb-label keep">"Export"</span>
                </button>
            </div>
            <ExportPopover />
        </div>
    }
}

/// Spaced ⟷ compact (`D`).
#[component]
fn DensitySegments() -> impl IntoView {
    let state = expect_context::<AppState>();
    let density = move || state.density();
    let pick = move |wanted: ViewDensity| {
        if density() != wanted {
            state.toggle_density();
        }
    };
    view! {
        <div class="segmented" role="radiogroup" aria-label="Density">
            <button
                class="seg"
                role="radio"
                class:active=move || density() == ViewDensity::Spaced
                aria-checked=move || (density() == ViewDensity::Spaced).to_string()
                aria-label="Spaced"
                data-tip="Spaced layout"
                data-kbd="D"
                on:click=move |_| pick(ViewDensity::Spaced)
            >
                <IconView icon=Icon::Spaced />
                <span class="tb-label">"Spaced"</span>
            </button>
            <button
                class="seg"
                role="radio"
                class:active=move || density() == ViewDensity::Compact
                aria-checked=move || (density() == ViewDensity::Compact).to_string()
                aria-label="Compact"
                data-tip="Compact layout"
                data-kbd="D"
                on:click=move |_| pick(ViewDensity::Compact)
            >
                <IconView icon=Icon::Compact />
                <span class="tb-label">"Compact"</span>
            </button>
        </div>
    }
}

/// Flow ⟷ hub placement (`L`).
#[component]
fn LayoutSegments() -> impl IntoView {
    let state = expect_context::<AppState>();
    let layout = move || state.layout();
    view! {
        <div class="segmented" role="radiogroup" aria-label="Layout">
            <button
                class="seg"
                role="radio"
                class:active=move || layout() == ViewLayout::Flow
                aria-checked=move || (layout() == ViewLayout::Flow).to_string()
                aria-label="Flow layout"
                data-tip="Flow layout\nZones in flow order, in a row or folded"
                data-kbd="L"
                on:click=move |_| state.set_layout(ViewLayout::Flow)
            >
                <IconView icon=Icon::Flow />
                <span class="tb-label">"Flow"</span>
            </button>
            <button
                class="seg"
                role="radio"
                class:active=move || layout() == ViewLayout::Hub
                aria-checked=move || (layout() == ViewLayout::Hub).to_string()
                aria-label="Hub layout"
                data-tip="Hub layout\nBusiest nodes in the middle, short aligned edges"
                data-kbd="L"
                on:click=move |_| state.set_layout(ViewLayout::Hub)
            >
                <IconView icon=Icon::Hub />
                <span class="tb-label">"Hub"</span>
            </button>
        </div>
    }
}

/// Curves ⟷ rails (`R`).
#[component]
fn RoutingSegments() -> impl IntoView {
    let state = expect_context::<AppState>();
    let routing = move || state.routing();
    view! {
        <div class="segmented" role="radiogroup" aria-label="Connections">
            <button
                class="seg"
                role="radio"
                class:active=move || routing() != ViewRouting::Rails
                aria-checked=move || (routing() != ViewRouting::Rails).to_string()
                aria-label="Curves"
                data-tip=move || match routing() {
                    ViewRouting::Orthogonal => "Connections: orthogonal (from the spec)\nClick for curves",
                    ViewRouting::Straight => "Connections: straight (from the spec)\nClick for curves",
                    _ => "Curved connections",
                }
                data-kbd="R"
                on:click=move |_| state.set_routing(ViewRouting::Curved)
            >
                <IconView icon=Icon::Curves />
                <span class="tb-label">"Curves"</span>
            </button>
            <button
                class="seg"
                role="radio"
                class:active=move || routing() == ViewRouting::Rails
                aria-checked=move || (routing() == ViewRouting::Rails).to_string()
                aria-label="Rails"
                data-tip="Isometric rails\nStraight legs, rounded 90° bends"
                data-kbd="R"
                on:click=move |_| state.set_routing(ViewRouting::Rails)
            >
                <IconView icon=Icon::Rails />
                <span class="tb-label">"Rails"</span>
            </button>
        </div>
    }
}

/// One named row of the View popover: name, shortcut, then the segmented control.
fn view_row(name: &'static str, key: &'static str, control: impl IntoView) -> impl IntoView {
    view! {
        <div class="view-row">
            <span class="field-label">{name}</span>
            <kbd class="view-kbd">{key}</kbd>
            {control}
        </div>
    }
}

/// "View" button of a narrow preview: density, layout and connections, folded into one popover with
/// their names and shortcuts.
#[component]
fn ViewPicker() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.view_open;
    let popover: NodeRef<html::Div> = NodeRef::new();
    close_on_outside_press(open, ".view-picker");
    keep_inside_preview(popover);
    let summary = move || {
        format!(
            "View options\n{} · {} · {}",
            state.density().spec_value(),
            if state.layout() == ViewLayout::Hub { "hub" } else { "flow" },
            if state.routing() == ViewRouting::Rails { "rails" } else { "curves" },
        )
    };

    view! {
        <div class="view-picker">
            <button
                class="tb-btn labeled view-btn"
                class:active=move || open.get()
                aria-label="View options"
                aria-haspopup="dialog"
                aria-expanded=move || open.get().to_string()
                data-tip=summary
                on:click=move |_| state.toggle_popover(open)
            >
                <IconView icon=Icon::View />
                <span class="tb-label">"View"</span>
            </button>
            <Show when=move || open.get()>
                <div class="popover view-popover" role="dialog" aria-label="View options" node_ref=popover>
                    <div class="popover-head">
                        <span class="popover-title">"View"</span>
                        <button class="icon-btn small" aria-label="Close" on:click=move |_| open.set(false)>
                            <IconView icon=Icon::Close />
                        </button>
                    </div>
                    {view_row("Density", "D", view! { <DensitySegments /> })}
                    {view_row("Layout", "L", view! { <LayoutSegments /> })}
                    {view_row("Connections", "R", view! { <RoutingSegments /> })}
                    <p class="popover-note">"Pin writes these choices into the spec."</p>
                </div>
            </Show>
        </div>
    }
}

/// Background, face and accent of a theme's swatch. The base is the theme's page colour, so
/// light and dark themes read apart even in greyscale; the cube carries the theme's node colours.
struct SwatchColors {
    page: &'static str,
    rim: &'static str,
    top: &'static str,
    left: &'static str,
    right: &'static str,
    flow: &'static str,
}

fn swatch_colors(theme: DiagramTheme) -> SwatchColors {
    match theme {
        DiagramTheme::Blueprint => SwatchColors {
            page: "#F2F4F8",
            rim: "#C9D1EA",
            top: "#6F86F5",
            left: "#3E50C8",
            right: "#2C3A9E",
            flow: "#E8833A",
        },
        DiagramTheme::Midnight => SwatchColors {
            page: "#0C111D",
            rim: "#4A5A85",
            top: "#93A6FF",
            left: "#6F86F5",
            right: "#4A5FD6",
            flow: "#FF9F4A",
        },
        DiagramTheme::Paper => SwatchColors {
            page: "#F7F3EA",
            rim: "#DCCFB8",
            top: "#E9C9A0",
            left: "#B8875A",
            right: "#8F6440",
            flow: "#D9894A",
        },
        DiagramTheme::Mono => SwatchColors {
            page: "#FFFFFF",
            rim: "#BDBDBD",
            top: "#E6E6E6",
            left: "#9A9A9A",
            right: "#5C5C5C",
            flow: "#333333",
        },
        DiagramTheme::Schematic => SwatchColors {
            page: "#F8F9FB",
            rim: "#C3CAD5",
            top: "#F4F6FA",
            left: "#DDE3EC",
            right: "#B9C2D0",
            flow: "#1D5BD8",
        },
    }
}

/// Swatch of a theme: a tiny diorama, a page-coloured base with a cube and a flow dot on it.
fn swatch(theme: DiagramTheme) -> String {
    let SwatchColors { page, rim, top, left, right, flow } = swatch_colors(theme);
    format!(
        concat!(
            r#"<svg viewBox="0 0 26 20" width="26" height="20" aria-hidden="true">"#,
            r#"<path d="M13 6.2 24.6 12.6 13 19 1.4 12.6z" fill="{page}" stroke="{rim}" stroke-width="1" stroke-linejoin="round"/>"#,
            r#"<path d="M13 2.2 18 5v5.6l-5 2.8-5-2.8V5z" fill="{left}"/>"#,
            r#"<path d="M13 7.8 18 5v5.6l-5 2.8z" fill="{right}"/>"#,
            r#"<path d="M13 2.2 18 5l-5 2.8L8 5z" fill="{top}"/>"#,
            r#"<circle cx="20.4" cy="13.4" r="1.7" fill="{flow}"/>"#,
            r#"</svg>"#
        ),
        page = page,
        rim = rim,
        top = top,
        left = left,
        right = right,
        flow = flow,
    )
}

#[component]
fn ThemePicker() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.theme_open;
    let current = move || state.diagram_theme();
    let menu: NodeRef<html::Div> = NodeRef::new();
    close_on_outside_press(open, ".theme-picker");
    keep_inside_preview(menu);

    view! {
        <div class="theme-picker" class:open=move || open.get()>
            <button
                class="tb-btn labeled theme-btn"
                aria-label=move || format!("Diagram theme: {}", current().label())
                aria-haspopup="listbox"
                aria-expanded=move || open.get().to_string()
                data-tip="Diagram theme"
                on:click=move |_| state.toggle_popover(open)
            >
                <span class="swatch" inner_html=move || swatch(current())></span>
                <span class="tb-label theme-label">{move || current().label()}</span>
            </button>
            <Show when=move || open.get()>
                <div class="menu" role="listbox" aria-label="Diagram theme" node_ref=menu>
                    {DiagramTheme::ALL
                        .into_iter()
                        .map(|theme| {
                            view! {
                                <button
                                    class="menu-item"
                                    role="option"
                                    aria-selected=move || (current() == theme).to_string()
                                    class:active=move || current() == theme
                                    on:click=move |_| {
                                        state.set_theme(theme);
                                        open.set(false);
                                    }
                                >
                                    <span class="swatch" inner_html=swatch(theme)></span>
                                    <span class="menu-label">{theme.label()}</span>
                                    <Show when=move || current() == theme>
                                        <IconView icon=Icon::Check class="menu-check" />
                                    </Show>
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative luminance of `#rrggbb` (WCAG).
    fn luminance(hex: &str) -> f64 {
        let channel = |index: usize| {
            let value = f64::from(u8::from_str_radix(&hex[1 + index * 2..3 + index * 2], 16).unwrap_or(0)) / 255.0;
            if value <= 0.039_28 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2)
    }

    #[test]
    fn swatches_differ_in_greyscale() {
        // Each swatch's (page, cube top, cube side) luminances must differ from every other's in at least one part.
        let signature = |theme| {
            let colors = swatch_colors(theme);
            [luminance(colors.page), luminance(colors.top), luminance(colors.right)]
        };
        for (index, first) in DiagramTheme::ALL.iter().enumerate() {
            for second in &DiagramTheme::ALL[index + 1..] {
                let (a, b) = (signature(*first), signature(*second));
                let distance = a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max);
                assert!(distance > 0.12, "{first:?} vs {second:?}: {distance}");
            }
        }
    }

    #[test]
    fn swatch_svg_uses_the_theme_page_colour() {
        assert!(swatch(DiagramTheme::Midnight).contains("#0C111D"));
        assert!(swatch(DiagramTheme::Blueprint).contains("#F2F4F8"));
    }
}
