//! Resolves the motion hooks of pre-drawn node markup (T-402, see `icons::motion`).
//!
//! Node bodies are drawn once, with each moving part wrapped in `<g data-icon-motion="…">`. Per output mode:
//! a static SVG strips the hooks (and drops the parts that only exist while animating), so it is
//! byte-identical to a drawing without hooks; a raster frame replaces each hook by the part's opacity and
//! transform at `t`; the animated SVG adds SMIL `<animate>`/`<animateTransform>` built from the same keyframes.

use std::borrow::Cow;
use std::fmt::Write as _;

use crate::canvas::{fmt_matrix, fmt_ratio};
use crate::geometry::{Affine, ScreenPoint};
use crate::icons::motion::{FLOOR_BASIS, IconMotion, Keyframes, MOTION_ATTRIBUTE, TransformTrack, floor_basis_inverse};

/// Opacity under which a frame skips a part altogether.
const INVISIBLE_OPACITY: f64 = 0.001;

/// How hooks are written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HookRender {
    /// No movement: hooks stripped, overlays dropped.
    Static,
    /// Raster frame at loop time `t` of a loop lasting `duration_s`.
    Frame {
        /// Normalized time.
        t: f64,
        /// Loop duration (s).
        duration_s: f64,
    },
    /// SMIL animation for a loop lasting `duration_s`.
    Animated {
        /// Loop duration (s).
        duration_s: f64,
    },
}

/// `markup` with every motion hook resolved for `render`. Borrowed when there is no hook.
pub fn resolve_hooks(markup: &str, render: HookRender) -> Cow<'_, str> {
    if !markup.contains(MOTION_ATTRIBUTE) {
        return Cow::Borrowed(markup);
    }
    let mut output = String::with_capacity(markup.len() + 256);
    write_resolved(markup, render, &mut output);
    Cow::Owned(output)
}

fn hook_prefix() -> String {
    format!("<g {MOTION_ATTRIBUTE}=\"")
}

fn write_resolved(markup: &str, render: HookRender, output: &mut String) {
    let prefix = hook_prefix();
    let mut rest = markup;
    while let Some(start) = rest.find(&prefix) {
        output.push_str(&rest[..start]);
        let Some(hook) = Hook::at(&rest[start..], prefix.len()) else {
            // Malformed: keep the remaining markup untouched rather than lose a part of the drawing.
            output.push_str(&rest[start..]);
            return;
        };
        hook.write(render, output);
        rest = &rest[start + hook.length..];
    }
    output.push_str(rest);
}

/// One hook found in markup: its motion (None if unreadable), inner markup and total byte length.
struct Hook<'a> {
    motion: Option<IconMotion>,
    content: &'a str,
    length: usize,
}

impl<'a> Hook<'a> {
    /// Parses the hook starting at the beginning of `markup` (`prefix_length` = length of `<g data-…="`).
    fn at(markup: &'a str, prefix_length: usize) -> Option<Self> {
        let after = &markup[prefix_length..];
        let quote = after.find('"')?;
        let tag_end = prefix_length + quote + after[quote..].find('>')? + 1;
        let close = matching_close(&markup[tag_end..])?;
        Some(Self {
            motion: IconMotion::parse(&after[..quote]),
            content: &markup[tag_end..tag_end + close],
            length: tag_end + close + CLOSE.len(),
        })
    }

    fn write(&self, render: HookRender, output: &mut String) {
        let Some(motion) = self.motion else {
            write_resolved(self.content, render, output);
            return;
        };
        match render {
            HookRender::Static => {
                if !motion.is_overlay() {
                    write_resolved(self.content, render, output);
                }
            }
            HookRender::Frame { t, duration_s } => {
                let state = motion.state_at(t, duration_s);
                if motion.is_overlay() && state.opacity < INVISIBLE_OPACITY {
                    return;
                }
                let moved = state.transform != Affine::IDENTITY;
                let faded = state.opacity < 1.0;
                if !moved && !faded {
                    write_resolved(self.content, render, output);
                    return;
                }
                output.push_str("<g");
                if moved {
                    let _ = write!(output, " transform=\"{}\"", fmt_matrix(&state.transform));
                }
                if faded {
                    let _ = write!(output, " opacity=\"{}\"", fmt_ratio(state.opacity));
                }
                output.push('>');
                write_resolved(self.content, render, output);
                output.push_str(CLOSE);
            }
            HookRender::Animated { duration_s } => self.write_animated(motion, duration_s, output),
        }
    }

    fn write_animated(&self, motion: IconMotion, duration_s: f64, output: &mut String) {
        let cycle = duration_s / f64::from(motion.cycles(duration_s));
        let timing = format!("dur=\"{}s\" repeatCount=\"indefinite\"", fmt_ratio(cycle));
        output.push_str("<g");
        if motion.is_overlay() {
            // Hidden until SMIL runs (and in viewers without SMIL), as in the static SVG.
            output.push_str(" opacity=\"0\"");
        }
        output.push('>');
        if let Some(keys) = motion.opacity_track() {
            let _ = write!(
                output,
                "<animate attributeName=\"opacity\" {timing} values=\"{}\" keyTimes=\"{}\"/>",
                values(&keys, fmt_ratio),
                key_times(&keys)
            );
        }
        let closes = match motion.phased_transform_track() {
            None => 0,
            Some(TransformTrack::Translate { direction, keys }) => {
                let _ = write!(
                    output,
                    "<animateTransform attributeName=\"transform\" type=\"translate\" {timing} values=\"{}\" keyTimes=\"{}\"/>",
                    values(&keys, |factor| format!(
                        "{},{}",
                        fmt_ratio(direction.x * factor),
                        fmt_ratio(direction.y * factor)
                    )),
                    key_times(&keys)
                );
                0
            }
            Some(TransformTrack::Scale { center, keys }) => {
                open_around(output, center, None, "scale", &values(&keys, fmt_ratio), &keys, &timing);
                AROUND_GROUPS
            }
            Some(TransformTrack::ScaleY { anchor, keys }) => {
                let scales = values(&keys, |scale| format!("1 {}", fmt_ratio(scale)));
                open_around(output, anchor, None, "scale", &scales, &keys, &timing);
                AROUND_GROUPS
            }
            Some(TransformTrack::FloorRotate { center, keys }) => {
                let angles = values(&keys, fmt_ratio);
                open_around(
                    output,
                    center,
                    Some((FLOOR_BASIS, floor_basis_inverse())),
                    "rotate",
                    &angles,
                    &keys,
                    &timing,
                );
                AROUND_GROUPS
            }
        };
        write_resolved(self.content, HookRender::Animated { duration_s }, output);
        for _ in 0..closes {
            output.push_str(CLOSE);
        }
        output.push_str(CLOSE);
    }
}

/// Opens `translate(p) [basis] · <animated> · [basis⁻¹] translate(−p)` groups: SMIL scales and rotates around
/// the origin, so the animated group sits between a move to `point` and back ([`AROUND_GROUPS`] for the caller
/// to close).
fn open_around(
    output: &mut String,
    point: ScreenPoint,
    basis: Option<(Affine, Affine)>,
    kind: &str,
    values: &str,
    keys: &Keyframes,
    timing: &str,
) {
    let (x, y) = (fmt_ratio(point.x), fmt_ratio(point.y));
    let (outer, inner) = match basis {
        Some((basis, inverse)) => (format!(" {}", fmt_matrix(&basis)), format!("{} ", fmt_matrix(&inverse))),
        None => (String::new(), String::new()),
    };
    let _ = write!(
        output,
        "<g transform=\"translate({x},{y}){outer}\"><g><animateTransform attributeName=\"transform\" type=\"{kind}\" {timing} values=\"{values}\" keyTimes=\"{}\"/><g transform=\"{inner}translate(-{x},-{y})\">",
        key_times(keys)
    );
}

fn values(keys: &Keyframes, format: impl Fn(f64) -> String) -> String {
    keys.points().iter().map(|&(_, value)| format(value)).collect::<Vec<_>>().join(";")
}

fn key_times(keys: &Keyframes) -> String {
    keys.points().iter().map(|&(time, _)| fmt_ratio(time)).collect::<Vec<_>>().join(";")
}

const CLOSE: &str = "</g>";
/// Groups [`open_around`] opens.
const AROUND_GROUPS: usize = 3;

/// Byte index of the `</g>` closing the group whose content starts `markup` (nested groups skipped).
fn matching_close(markup: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = 0;
    while let Some(offset) = markup[index..].find('<') {
        let at = index + offset;
        let tail = &markup[at..];
        if tail.starts_with(CLOSE) {
            if depth == 0 {
                return Some(at);
            }
            depth -= 1;
        } else if tail.starts_with("<g ") || tail.starts_with("<g>") {
            let tag_end = tail.find('>')?;
            if !tail[..tag_end].ends_with('/') {
                depth += 1;
            }
        }
        index = at + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::IsoCanvas;
    use crate::geometry::{Projection, ScreenVector};
    use crate::icons::motion::{close_hook, open_hook};

    fn hooked(motion: IconMotion) -> String {
        let mut canvas = IsoCanvas::new(Projection::default());
        canvas.raw("<rect id=\"before\"/>", crate::geometry::Rect::EMPTY);
        open_hook(&mut canvas, &motion);
        canvas.raw("<g data-role=\"inner\"><circle r=\"2\"/></g>", crate::geometry::Rect::EMPTY);
        close_hook(&mut canvas);
        canvas.raw("<rect id=\"after\"/>", crate::geometry::Rect::EMPTY);
        canvas.finish().body
    }

    const PLAIN: &str = "<rect id=\"before\"/><g data-role=\"inner\"><circle r=\"2\"/></g><rect id=\"after\"/>";

    #[test]
    fn static_strips_hooks_and_drops_overlays() {
        let blink = hooked(IconMotion::Blink { phase: 0.2 });
        assert_eq!(resolve_hooks(&blink, HookRender::Static), PLAIN);
        let glint = hooked(IconMotion::Glint { phase: 0.2 });
        assert_eq!(resolve_hooks(&glint, HookRender::Static), "<rect id=\"before\"/><rect id=\"after\"/>");
        assert!(matches!(resolve_hooks(PLAIN, HookRender::Static), Cow::Borrowed(_)));
    }

    #[test]
    fn frames_write_the_state_at_t() {
        let blink = hooked(IconMotion::Blink { phase: 0.0 });
        // At t = 0 the LED is lit: no wrapper at all.
        assert_eq!(resolve_hooks(&blink, HookRender::Frame { t: 0.0, duration_s: 2.0 }), PLAIN);
        // Blinking twice per 2 s loop: dimmed at 0.65 of a cycle.
        let dim = resolve_hooks(&blink, HookRender::Frame { t: 0.325, duration_s: 2.0 });
        assert!(dim.contains("<g opacity=\"0.3\"><g data-role=\"inner\">"), "{dim}");
        let drift = hooked(IconMotion::Drift { offset: ScreenVector::new(2.0, 0.0), phase: 0.0 });
        let moved = resolve_hooks(&drift, HookRender::Frame { t: 0.0, duration_s: 2.0 });
        assert!(moved.contains("transform=\"matrix(1,0,0,1,2,0)\""), "{moved}");
        let glint = hooked(IconMotion::Glint { phase: 0.0 });
        assert_eq!(
            resolve_hooks(&glint, HookRender::Frame { t: 0.0, duration_s: 2.0 }),
            "<rect id=\"before\"/><rect id=\"after\"/>"
        );
    }

    #[test]
    fn animated_svg_uses_smil_with_balanced_groups() {
        let center = ScreenPoint::new(4.0, 5.0);
        for motion in [
            IconMotion::Blink { phase: 0.5 },
            IconMotion::Ripple { center, phase: 0.0 },
            IconMotion::Spin { center, step_degrees: 360.0 / 7.0, phase: 0.3 },
            IconMotion::Bob { anchor: center, phase: 0.25 },
            IconMotion::Travel { trip: ScreenVector::new(20.0, 10.0), phase: 0.5 },
        ] {
            let markup = hooked(motion);
            let svg = resolve_hooks(&markup, HookRender::Animated { duration_s: 2.0 });
            assert!(svg.contains("<animate") && svg.contains("repeatCount=\"indefinite\""), "{svg}");
            assert!(!svg.contains(MOTION_ATTRIBUTE));
            assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count(), "{svg}");
            assert!(svg.ends_with("<rect id=\"after\"/>"), "{svg}");
        }
        let markup = hooked(IconMotion::Spin { center, step_degrees: 90.0, phase: 0.0 });
        let spin = resolve_hooks(&markup, HookRender::Animated { duration_s: 2.0 });
        assert!(spin.contains("type=\"rotate\"") && spin.contains("values=\"0;90\""), "{spin}");
    }

    #[test]
    fn nested_groups_find_their_own_close() {
        assert_eq!(matching_close("<g a><g/></g></g>tail"), Some(13));
        assert_eq!(matching_close("<path/></g>"), Some(7));
        assert_eq!(matching_close("<g>"), None);
    }
}
