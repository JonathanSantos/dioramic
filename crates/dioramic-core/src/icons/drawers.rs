//! One drawer per [`ProceduralIcon`], built only from the primitives, in icon-local coordinates
//! (front = local `+b`). Sizes come from [`shape_of`], so `metrics` and the drawing never drift apart.

use super::ProceduralIcon;
use super::motion::{IconMotion, close_hook, open_hook};
use super::primitives::{
    Face, FaceRegion, IconFrame, LocalBox, PERSON_HEIGHT, PortRow, PortStyle, Prism, RoundSolid, SignPlate,
    TubeSegment, cloud, detail_stroke, dome, floor_arrow, floor_ring, person, prism, screen, side_gradient, sign_plate,
    slab, solid_box, solid_cylinder, sphere, stack, tube,
};
use crate::canvas::{IsoCanvas, Paint, PathData, Stroke, Style};
use crate::color::Color;
use crate::geometry::{Rect, ScreenPoint, ScreenVector};
use crate::theme::{IconFinish, Theme};

/// Local footprint of an icon before scale and rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum LocalFootprint {
    /// Rectangle `half_width × half_depth` (local `a × b`).
    Rect(f64, f64),
    /// Circle of radius.
    Circle(f64),
}

/// Footprint and height of an icon at scale 1, rotation 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LocalShape {
    pub footprint: LocalFootprint,
    pub height: f64,
}

const fn rect(half_width: f64, half_depth: f64, height: f64) -> LocalShape {
    LocalShape { footprint: LocalFootprint::Rect(half_width, half_depth), height }
}

const fn circle(radius: f64, height: f64) -> LocalShape {
    LocalShape { footprint: LocalFootprint::Circle(radius), height }
}

// Heights of composite icons, shared by the table and the drawers.
const SERVER_UNIT: f64 = 0.3;
const SERVER_PITCH: f64 = 0.34;
const DATABASE_DISC: f64 = 0.28;
const DATABASE_PITCH: f64 = 0.33;
const CACHE_LOWER: f64 = 0.3;
const CACHE_UPPER_Z: f64 = 0.36;
const CACHE_UPPER: f64 = 0.24;
const TUBE_RADIUS: f64 = 0.25;
const TUBE_AXIS_Z: f64 = 0.28;
const ROUTER_HEIGHT: f64 = 0.26;
const AP_PUCK: f64 = 0.08;
const AP_DOME_RISE: f64 = 0.14;
const AP_WAVES_TOP: f64 = 0.56;
/// Share of a cycle between two Wi-Fi arcs lighting up.
const AP_WAVE_LAG: f64 = 0.12;
const BUCKET_HEIGHT: f64 = 0.58;
const NAS_HEIGHT: f64 = 0.46;
/// Drive bays across the NAS front.
const NAS_BAYS: usize = 4;
const CDN_BASE: f64 = 0.14;
const CDN_GLOBE_RADIUS: f64 = 0.3;
/// A sphere's silhouette reaches `√1.5·r` above its center (true isometric drawing).
const SPHERE_SILHOUETTE: f64 = 1.224_744_871_391_589;
const DNS_POLE_TOP: f64 = 0.94;
const GATEWAY_TOP: f64 = 0.68;
const CLOUD_TOP: f64 = 0.9;
/// Spokes of the Kubernetes helm: the helm turns one spoke per cycle, so the loop is seamless.
const HELM_SPOKES: usize = 7;
/// Messages travelling on the queue at once, and how far along the tube they go (local `a`, tiles).
const QUEUE_MESSAGES: usize = 3;
const QUEUE_TRIP: (f64, f64) = (-0.46, 0.46);
/// Sway of the cloud (px at the default tile).
const CLOUD_DRIFT_PX: f64 = 1.6;

/// The size table (tiles). Low icons (`height < TALL_HEIGHT`): switch, router, load balancer, VoIP phone,
/// Kubernetes platform, pod.
pub(super) const fn shape_of(icon: ProceduralIcon) -> LocalShape {
    match icon {
        ProceduralIcon::Block => rect(0.4, 0.4, 0.6),
        ProceduralIcon::Server => rect(0.42, 0.42, 2.0 * SERVER_PITCH + SERVER_UNIT),
        ProceduralIcon::Rack => rect(0.4, 0.4, 1.4),
        ProceduralIcon::Switch => rect(0.56, 0.32, 0.2),
        ProceduralIcon::Router => circle(0.44, ROUTER_HEIGHT),
        ProceduralIcon::LoadBalancer => rect(0.55, 0.4, 0.28),
        ProceduralIcon::Firewall => rect(0.54, 0.2, 0.72),
        ProceduralIcon::AccessPoint => circle(0.36, AP_WAVES_TOP),
        ProceduralIcon::Database => circle(0.48, 2.0 * DATABASE_PITCH + DATABASE_DISC),
        ProceduralIcon::Cache => circle(0.48, CACHE_UPPER_Z + CACHE_UPPER),
        ProceduralIcon::Storage => circle(0.46, BUCKET_HEIGHT),
        ProceduralIcon::Nas => rect(0.44, 0.36, NAS_HEIGHT),
        ProceduralIcon::Queue => rect(0.62, TUBE_RADIUS, TUBE_AXIS_Z + TUBE_RADIUS),
        ProceduralIcon::Cloud => circle(0.58, CLOUD_TOP),
        ProceduralIcon::Users => circle(0.46, PERSON_HEIGHT),
        ProceduralIcon::Person => circle(0.2, PERSON_HEIGHT),
        ProceduralIcon::Workstation => rect(0.44, 0.32, 0.7),
        ProceduralIcon::Laptop => rect(0.42, 0.3, 0.53),
        ProceduralIcon::Mobile => rect(0.26, 0.16, 0.8),
        ProceduralIcon::VoipPhone => rect(0.36, 0.3, 0.22),
        ProceduralIcon::Printer => rect(0.42, 0.34, 0.52),
        ProceduralIcon::Atm => rect(0.34, 0.3, 1.0),
        ProceduralIcon::Function => circle(0.42, 0.52),
        ProceduralIcon::Container => rect(0.52, 0.3, 0.5),
        ProceduralIcon::ApiGateway => rect(0.52, 0.26, GATEWAY_TOP),
        ProceduralIcon::Cdn => rect(0.42, 0.42, CDN_BASE + CDN_GLOBE_RADIUS * (1.0 + SPHERE_SILHOUETTE)),
        ProceduralIcon::Dns => rect(0.32, 0.32, DNS_POLE_TOP),
        ProceduralIcon::K8sCluster => circle(0.62, 0.2),
        ProceduralIcon::Pod => circle(0.34, 0.36),
        ProceduralIcon::Monitoring => rect(0.44, 0.16, 0.8),
    }
}

/// Everything a drawer needs: canvas, theme, frame and the optional tint of the main body.
pub(super) struct Painter<'c, 't> {
    pub canvas: &'c mut IsoCanvas,
    pub theme: &'t Theme,
    pub frame: IconFrame,
    pub tint: Option<Color>,
    /// Wrap the moving parts in motion hooks (T-402).
    pub animate: bool,
}

impl Painter<'_, '_> {
    /// The main body color: the tint if any, else the theme's color for the icon.
    fn main(&self, default: Color) -> Color {
        self.tint.unwrap_or(default)
    }

    fn solid(&mut self, solid: &LocalBox, color: Color) {
        solid_box(self.canvas, self.theme, &self.frame, solid, color);
    }

    fn cylinder(&mut self, at: (f64, f64), radius: f64, z: f64, height: f64, color: Color) {
        solid_cylinder(self.canvas, self.theme, &self.frame, &RoundSolid { at, radius, z, height }, color);
    }

    fn fill(&mut self, region: &FaceRegion, u: (f64, f64), v: (f64, f64), color: Color) {
        region.fill_rect(self.canvas, &self.frame, u, v, &Style::fill(color));
    }

    fn line(&mut self, region: &FaceRegion, points: &[(f64, f64)], color: Color, width_px: f64) {
        let stroke = detail_stroke(&self.frame, color, width_px);
        region.polyline(self.canvas, &self.frame, points, &stroke);
    }

    fn dot(&mut self, region: &FaceRegion, at: (f64, f64), radius_px: f64, color: Color) {
        region.dot(self.canvas, &self.frame, at, radius_px, color);
    }

    /// Draws `part` inside a motion hook when the icon animates, as is otherwise.
    fn moving(&mut self, motion: IconMotion, part: impl FnOnce(&mut Self)) {
        if self.animate {
            open_hook(self.canvas, &motion);
            part(self);
            close_hook(self.canvas);
        } else {
            part(self);
        }
    }

    /// Draws `part` (a light, ring or item that only exists while animating) inside a motion hook, and
    /// nothing at all when the icon does not animate.
    fn overlay(&mut self, motion: IconMotion, part: impl FnOnce(&mut Self)) {
        if self.animate {
            self.moving(motion, part);
        }
    }

    /// Projects a local point (tracked).
    fn at(&mut self, a: f64, b: f64, z: f64) -> ScreenPoint {
        self.frame.point(self.canvas, a, b, z)
    }
}

/// Draws the body of `icon` (no shadow, no badge).
pub(super) fn draw_body(painter: &mut Painter<'_, '_>, icon: ProceduralIcon) {
    match icon {
        ProceduralIcon::Block => block(painter),
        ProceduralIcon::Server => server(painter),
        ProceduralIcon::Rack => rack(painter),
        ProceduralIcon::Switch => switch(painter),
        ProceduralIcon::Router => router(painter),
        ProceduralIcon::LoadBalancer => load_balancer(painter),
        ProceduralIcon::Firewall => firewall(painter),
        ProceduralIcon::AccessPoint => access_point(painter),
        ProceduralIcon::Database => database(painter),
        ProceduralIcon::Cache => cache(painter),
        ProceduralIcon::Storage => storage(painter),
        ProceduralIcon::Nas => nas(painter),
        ProceduralIcon::Queue => queue(painter),
        ProceduralIcon::Cloud => cloud_icon(painter),
        ProceduralIcon::Users => users(painter),
        ProceduralIcon::Person => single_person(painter),
        ProceduralIcon::Workstation => workstation(painter),
        ProceduralIcon::Laptop => laptop(painter),
        ProceduralIcon::Mobile => mobile(painter),
        ProceduralIcon::VoipPhone => voip_phone(painter),
        ProceduralIcon::Printer => printer(painter),
        ProceduralIcon::Atm => atm(painter),
        ProceduralIcon::Function => function(painter),
        ProceduralIcon::Container => container(painter),
        ProceduralIcon::ApiGateway => api_gateway(painter),
        ProceduralIcon::Cdn => cdn(painter),
        ProceduralIcon::Dns => dns(painter),
        ProceduralIcon::K8sCluster => k8s_cluster(painter),
        ProceduralIcon::Pod => pod(painter),
        ProceduralIcon::Monitoring => monitoring(painter),
    }
}

fn block(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.block);
    let body = LocalBox::centered(0.4, 0.4, 0.0, 0.6);
    p.solid(&body, color);
    let top = FaceRegion::of(body, Face::Top);
    p.fill(&top, (0.22, 0.78), (0.22, 0.78), color.shade(1.1));
}

fn server(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.server);
    let unit = LocalBox::centered(0.42, 0.42, 0.0, SERVER_UNIT);
    stack(p.canvas, p.theme, &p.frame, &unit, (3, SERVER_PITCH), color, p.animate);
}

fn rack(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.server);
    let cabinet = LocalBox::centered(0.4, 0.4, 0.0, 1.4);
    p.solid(&cabinet, color.shade(0.9));
    let front = FaceRegion::of(cabinet, Face::Front);
    p.fill(&front, (0.08, 0.92), (0.03, 0.97), color.shade(0.42));
    let units = 7;
    for index in 0..units {
        let v0 = 0.06 + index as f64 * 0.128;
        let slot = front.inset(0.13, 0.87, v0, v0 + 0.1);
        p.fill(&slot, (0.0, 1.0), (0.0, 1.0), color.shade(1.08));
        let led = p.theme.nodes.led;
        p.moving(IconMotion::Blink { phase: sequence(index, units) }, |p| p.dot(&slot, (0.12, 0.5), 1.5, led));
        p.line(&slot, &[(0.42, 0.5), (0.86, 0.5)], color.shade(0.55), 1.4);
    }
}

fn switch(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.switch);
    let body = LocalBox::centered(0.56, 0.32, 0.0, 0.2);
    let ports = PortRow { count: 7, color: p.theme.nodes.screen, style: PortStyle::Sockets, span: (0.08, 0.74) };
    let sockets = slab(p.canvas, p.theme, &p.frame, &body, color, &ports);
    let front = FaceRegion::of(body, Face::Front);
    let led = p.theme.nodes.led;
    // Activity lights over the sockets, in a scattered order (traffic is not a marching row).
    let half = (ports.span.1 - ports.span.0) / (ports.count - 1) as f64 * 0.2;
    for (index, u) in sockets.iter().enumerate() {
        let phase = sequence(index * 3 % ports.count, ports.count);
        p.overlay(IconMotion::Glint { phase }, |p| p.fill(&front, (u - half, u + half), (0.36, 0.64), led));
    }
    p.moving(IconMotion::Blink { phase: 0.0 }, |p| p.dot(&front, (0.85, 0.5), 1.6, led));
    p.moving(IconMotion::Blink { phase: 0.5 }, |p| p.dot(&front, (0.92, 0.5), 1.6, led));
    // The classic switch glyph: two opposite arrows on the top.
    let arrow = Style::fill(p.theme.nodes.mark).with_opacity(0.92);
    floor_arrow(p.canvas, &p.frame, (-0.3, -0.1), (0.3, -0.1), 0.2, (0.05, 0.14), &arrow);
    floor_arrow(p.canvas, &p.frame, (0.3, 0.1), (-0.3, 0.1), 0.2, (0.05, 0.14), &arrow);
}

fn router(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.router);
    p.cylinder((0.0, 0.0), 0.44, 0.0, ROUTER_HEIGHT, color);
    // The classic router glyph: arrows out along one axis, in along the other.
    let arrow = Style::fill(p.theme.nodes.mark).with_opacity(0.95);
    let widths = (0.05, 0.13);
    // Outgoing and incoming arrows breathe in turn.
    for (from, to, phase) in [
        ((0.08, 0.0), (0.33, 0.0), 0.0),
        ((-0.08, 0.0), (-0.33, 0.0), 0.0),
        ((0.0, 0.33), (0.0, 0.09), 0.5),
        ((0.0, -0.33), (0.0, -0.09), 0.5),
    ] {
        p.moving(IconMotion::Breathe { phase }, |p| {
            floor_arrow(p.canvas, &p.frame, from, to, ROUTER_HEIGHT, widths, &arrow);
        });
    }
}

fn load_balancer(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.load_balancer);
    let body = LocalBox::centered(0.55, 0.4, 0.0, 0.28);
    // Prototype: 5 ports, 0.13 tiles in from each end.
    let margin = 0.13 / body.width;
    let ports = PortRow { count: 5, color: p.theme.nodes.port, style: PortStyle::Dots, span: (margin, 1.0 - margin) };
    let dots = slab(p.canvas, p.theme, &p.frame, &body, color, &ports);
    // The ports light up one after the other, like requests handed out in turn.
    let front = FaceRegion::of(body, Face::Front);
    let led = p.theme.nodes.led;
    for (index, u) in dots.iter().enumerate() {
        let phase = sequence(index, ports.count);
        p.overlay(IconMotion::Glint { phase }, |p| p.dot(&front, (*u, 0.5), 2.2, led));
    }
}

fn firewall(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.firewall);
    let wall = LocalBox::centered(0.54, 0.2, 0.0, 0.72);
    p.solid(&wall, color);
    let mortar = color.shade(0.55);
    let rows = 5;
    let front = FaceRegion::of(wall, Face::Front);
    let side = FaceRegion::of(wall, Face::Side);
    for row in 1..rows {
        let v = row as f64 / rows as f64;
        p.line(&front, &[(0.0, v), (1.0, v)], mortar, 1.1);
        p.line(&side, &[(0.0, v), (1.0, v)], mortar, 1.1);
    }
    for row in 0..rows {
        let (v0, v1) = (row as f64 / rows as f64, (row + 1) as f64 / rows as f64);
        let offset = if row % 2 == 0 { 0.25 } else { 0.125 };
        let mut u = offset;
        while u < 0.99 {
            p.line(&front, &[(u, v0), (u, v1)], mortar, 1.1);
            u += 0.25;
        }
        if row % 2 == 1 {
            p.line(&side, &[(0.5, v0), (0.5, v1)], mortar, 1.1);
        }
    }
}

fn access_point(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.access_point);
    p.cylinder((0.0, 0.0), 0.36, 0.0, AP_PUCK, color);
    let cap = RoundSolid { at: (0.0, 0.0), radius: 0.28, z: AP_PUCK, height: AP_DOME_RISE };
    dome(p.canvas, p.theme, &p.frame, &cap, color);
    // LED on the puck rim facing the camera.
    let rim = 0.36 * std::f64::consts::FRAC_1_SQRT_2;
    let led = p.at(rim, rim, AP_PUCK / 2.0);
    let led_style = Style::fill(p.theme.nodes.led);
    p.moving(IconMotion::Blink { phase: 0.0 }, |p| p.canvas.circle(led, p.frame.px(1.8), &led_style));
    // Wi-Fi waves, billboarded above the dome.
    let peak = p.canvas.project_quiet(p.frame.plan(0.0, 0.0), p.frame.elevation(AP_PUCK + AP_DOME_RISE));
    let stroke = detail_stroke(&p.frame, p.theme.nodes.screen_glow, 2.2);
    let center = ScreenPoint::new(peak.x, peak.y - p.frame.px(2.0));
    // A wave leaves the dome: the inner arc lights first.
    for (index, radius_px) in [7.0, 12.5, 18.0].into_iter().enumerate() {
        let radius = p.frame.px(radius_px);
        let half_angle = 48f64.to_radians();
        let start = ScreenPoint::new(center.x - radius * half_angle.sin(), center.y - radius * half_angle.cos());
        let end = ScreenPoint::new(center.x + radius * half_angle.sin(), center.y - radius * half_angle.cos());
        let arc = PathData::new().move_to(start).arc_to(radius, radius, false, true, end).include(Rect::new(
            start.x,
            center.y - radius,
            end.x,
            start.y,
        ));
        let style = Style::stroke(stroke.clone());
        let phase = (-(index as f64) * AP_WAVE_LAG).rem_euclid(1.0);
        p.moving(IconMotion::Wave { phase }, |p| p.canvas.path(&arc, &style));
    }
}

fn database(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.database);
    for index in 0..3 {
        p.cylinder((0.0, 0.0), 0.48, index as f64 * DATABASE_PITCH, DATABASE_DISC, color);
    }
    ripple(p, 0.48, 2.0 * DATABASE_PITCH + DATABASE_DISC, 0.0);
}

/// A ring spreading over the top disc of a cylinder of `radius` whose top is at `top` (a write landing).
fn ripple(p: &mut Painter<'_, '_>, radius: f64, top: f64, phase: f64) {
    let center = p.canvas.project_quiet(p.frame.plan(0.0, 0.0), p.frame.elevation(top));
    let (radius_x, radius_y) = p.canvas.projection().circle_radii(p.frame.tiles(radius * 0.82));
    let style = Style::stroke(detail_stroke(&p.frame, p.theme.nodes.mark, 1.6));
    p.overlay(IconMotion::Ripple { center, phase }, |p| p.canvas.ellipse(center, radius_x, radius_y, &style));
}

fn cache(p: &mut Painter<'_, '_>) {
    let (lower, upper) = match p.tint {
        Some(tint) => (tint, tint.shade(1.1)),
        None => (p.theme.nodes.cache, p.theme.nodes.cache_top),
    };
    p.cylinder((0.0, 0.0), 0.48, 0.0, CACHE_LOWER, lower);
    p.cylinder((0.0, 0.0), 0.48, CACHE_UPPER_Z, CACHE_UPPER, upper);
    // Lightning bolt on the side facing the camera.
    let rim = 0.48 * std::f64::consts::FRAC_1_SQRT_2;
    let center = p.canvas.project_quiet(p.frame.plan(rim, rim), p.frame.elevation(CACHE_LOWER / 2.0));
    let bolt = [(1.5, -7.0), (-4.0, 1.0), (-0.3, 1.0), (-1.5, 7.0), (4.0, -1.0), (0.3, -1.0)];
    let points: Vec<ScreenPoint> =
        bolt.iter().map(|&(dx, dy)| ScreenPoint::new(center.x + p.frame.px(dx), center.y + p.frame.px(dy))).collect();
    p.canvas.polygon(&points, &Style::fill(p.theme.nodes.mark));
    let glow = Style::fill(p.theme.nodes.led);
    p.overlay(IconMotion::Shine { phase: 0.0 }, |p| p.canvas.polygon(&points, &glow));
    ripple(p, 0.48, CACHE_UPPER_Z + CACHE_UPPER, 0.5);
}

fn storage(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.storage);
    let (bottom_radius, top_radius) = (0.34, 0.46);
    let bottom = p.canvas.project_quiet(p.frame.plan(0.0, 0.0), p.frame.elevation(0.0));
    let top = p.canvas.project_quiet(p.frame.plan(0.0, 0.0), p.frame.elevation(BUCKET_HEIGHT));
    let projection = p.canvas.projection();
    let (bottom_x, bottom_y) = projection.circle_radii(p.frame.tiles(bottom_radius));
    let (top_x, top_y) = projection.circle_radii(p.frame.tiles(top_radius));
    let side = PathData::new()
        .move_to(ScreenPoint::new(top.x - top_x, top.y))
        .line_to(ScreenPoint::new(bottom.x - bottom_x, bottom.y))
        .arc_to(bottom_x, bottom_y, false, false, ScreenPoint::new(bottom.x + bottom_x, bottom.y))
        .line_to(ScreenPoint::new(top.x + top_x, top.y))
        .close()
        .include(Rect::new(top.x - top_x, top.y - top_y, top.x + top_x, bottom.y + bottom_y));
    let gradient = side_gradient(p.canvas, color);
    let outline = Stroke::new(p.theme.stroke, p.theme.stroke_width);
    p.canvas.path(&side, &Style { fill: Paint::Reference(gradient), ..Style::default() }.with_stroke(outline.clone()));
    // A band around the bucket, front half only.
    let band_z = BUCKET_HEIGHT * 0.45;
    let band_radius = bottom_radius + (top_radius - bottom_radius) * 0.45;
    let band_center = p.canvas.project_quiet(p.frame.plan(0.0, 0.0), p.frame.elevation(band_z));
    let (band_x, band_y) = projection.circle_radii(p.frame.tiles(band_radius));
    let band = PathData::new().move_to(ScreenPoint::new(band_center.x - band_x, band_center.y)).arc_to(
        band_x,
        band_y,
        false,
        false,
        ScreenPoint::new(band_center.x + band_x, band_center.y),
    );
    p.canvas.path(&band, &Style::stroke(detail_stroke(&p.frame, color.shade(0.62), 1.6)));
    // Open top: rim and the dark inside.
    p.canvas.ellipse(top, top_x, top_y, &Style::fill(color.shade(1.12)).with_stroke(outline.clone()));
    let (inner_x, inner_y) = projection.circle_radii(p.frame.tiles(top_radius - 0.05));
    p.canvas.ellipse(top, inner_x, inner_y, &Style::fill(color.shade(0.68)).with_stroke(outline));
}

/// A desktop NAS in the server's visual language: slate chassis, dark bay cage, silver drive trays with
/// a LED each. Reads as an appliance, not as object storage.
fn nas(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.server);
    let chassis = LocalBox::centered(0.44, 0.36, 0.0, NAS_HEIGHT);
    p.solid(&chassis, color);
    let top = FaceRegion::of(chassis, Face::Top);
    p.fill(&top, (0.1, 0.9), (0.12, 0.88), color.shade(1.08));
    let front = FaceRegion::of(chassis, Face::Front);
    p.fill(&front, (0.05, 0.95), (0.1, 0.9), color.shade(0.42));
    let tray = p.theme.nodes.database;
    let (margin, gap) = (0.08, 0.03);
    let bay_width = (1.0 - 2.0 * margin - gap * (NAS_BAYS - 1) as f64) / NAS_BAYS as f64;
    for index in 0..NAS_BAYS {
        let u0 = margin + index as f64 * (bay_width + gap);
        let bay = front.inset(u0, u0 + bay_width, 0.16, 0.84);
        p.fill(&bay, (0.0, 1.0), (0.0, 1.0), tray);
        p.line(&bay, &[(0.5, 0.14), (0.5, 0.6)], tray.shade(0.62), 1.3);
        let led = p.theme.nodes.led;
        p.moving(IconMotion::Blink { phase: sequence(index, NAS_BAYS) }, |p| p.dot(&bay, (0.5, 0.8), 1.4, led));
    }
}

fn queue(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.queue);
    let cradle = p.theme.nodes.server;
    for a in [-0.44, 0.34] {
        p.solid(&LocalBox::new(a, -0.17, 0.0, 0.1, 0.34, 0.12), cradle);
    }
    let segments = 4;
    let gap = 0.04;
    let length = 1.24;
    let segment_length = (length - gap * (segments - 1) as f64) / segments as f64;
    for index in 0..segments {
        let a0 = -length / 2.0 + index as f64 * (segment_length + gap);
        let segment = TubeSegment { a0, a1: a0 + segment_length, axis: (0.0, TUBE_AXIS_Z), radius: TUBE_RADIUS };
        tube(p.canvas, p.theme, &p.frame, &segment, color);
    }
    queue_messages(p);
}

/// Messages riding along the top of the queue, one after the other (only while animating).
fn queue_messages(p: &mut Painter<'_, '_>) {
    let top = TUBE_AXIS_Z + TUBE_RADIUS;
    let start = p.canvas.project_quiet(p.frame.plan(QUEUE_TRIP.0, 0.0), p.frame.elevation(top));
    let end = p.canvas.project_quiet(p.frame.plan(QUEUE_TRIP.1, 0.0), p.frame.elevation(top));
    let trip = ScreenVector::new(end.x - start.x, end.y - start.y);
    let message = LocalBox::new(QUEUE_TRIP.0 - 0.05, -0.07, top - 0.02, 0.1, 0.14, 0.07);
    let paper = p.theme.nodes.paper;
    for index in 0..QUEUE_MESSAGES {
        let phase = sequence(index, QUEUE_MESSAGES);
        p.overlay(IconMotion::Travel { trip, phase }, |p| p.solid(&message, paper));
    }
}

fn cloud_icon(p: &mut Painter<'_, '_>) {
    let (light, shade) = match p.tint {
        Some(tint) => (tint.mix(p.theme.nodes.cloud_light, 0.6), tint),
        None => (p.theme.nodes.cloud_light, p.theme.nodes.cloud_shade),
    };
    let offset = ScreenVector::new(p.frame.px(CLOUD_DRIFT_PX), 0.0);
    p.moving(IconMotion::Drift { offset, phase: 0.0 }, |p| cloud(p.canvas, p.theme, &p.frame, light, shade));
}

fn users(p: &mut Painter<'_, '_>) {
    let nodes = p.theme.nodes;
    let bodies = match (p.tint, p.theme.icon_finish) {
        (Some(tint), _) => [tint.shade(0.85), tint, tint.shade(1.15)],
        // Silver figures differ by tone only: one accent would read as a highlighted user.
        (None, IconFinish::Silver) => [nodes.person.shade(0.9), nodes.person, nodes.person.shade(1.08)],
        (None, IconFinish::Shaded) => [nodes.person, nodes.accent, nodes.router],
    };
    // Back to front (increasing a + b).
    let spots = [(-0.2, -0.22), (0.24, -0.04), (-0.04, 0.24)];
    for (spot, body) in spots.into_iter().zip(bodies) {
        person(p.canvas, p.theme, &p.frame, spot, body, nodes.person_head);
    }
}

fn single_person(p: &mut Painter<'_, '_>) {
    let body = p.main(p.theme.nodes.person);
    person(p.canvas, p.theme, &p.frame, (0.0, 0.0), body, p.theme.nodes.person_head);
}

/// Window-like content on a glass: title bar and two blocks. A silver glass stays clear (its glare is
/// the content), as in a technical drawing.
fn window_content(p: &mut Painter<'_, '_>, glass: &FaceRegion) {
    if p.theme.icon_finish == IconFinish::Silver {
        return;
    }
    let glow = p.theme.nodes.screen_glow;
    p.fill(glass, (0.08, 0.92), (0.78, 0.9), glow);
    p.fill(glass, (0.08, 0.46), (0.14, 0.68), glow.shade(0.72));
    p.fill(glass, (0.54, 0.92), (0.42, 0.68), glow.shade(0.6));
    p.fill(glass, (0.54, 0.92), (0.14, 0.32), glow.shade(0.6));
}

fn workstation(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.device);
    p.solid(&LocalBox::centered(0.44, 0.32, 0.0, 0.05), color.shade(0.92));
    p.solid(&LocalBox::new(-0.06, -0.22, 0.05, 0.12, 0.06, 0.16), color.shade(0.8));
    let panel = LocalBox::new(-0.4, -0.27, 0.2, 0.8, 0.06, 0.5);
    let glass = screen(p.canvas, p.theme, &p.frame, &panel, color, p.theme.nodes.screen, (0.05, 0.07));
    window_content(p, &glass);
    let keyboard = LocalBox::new(-0.28, 0.04, 0.05, 0.48, 0.2, 0.025);
    p.solid(&keyboard, color.shade(0.7));
    let keys = FaceRegion::of(keyboard, Face::Top);
    for row in 0..3 {
        let v = 0.22 + row as f64 * 0.28;
        p.line(&keys, &[(0.08, v), (0.92, v)], color.shade(1.05), 1.2);
    }
    p.solid(&LocalBox::new(0.28, 0.1, 0.05, 0.08, 0.12, 0.025), color.shade(0.7));
}

fn laptop(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.device);
    let base = LocalBox::centered(0.42, 0.3, 0.0, 0.05);
    p.solid(&base, color);
    let top = FaceRegion::of(base, Face::Top);
    p.fill(&top, (0.1, 0.9), (0.14, 0.58), color.shade(0.62));
    p.fill(&top, (0.38, 0.62), (0.68, 0.9), color.shade(0.86));
    let lid = LocalBox::new(-0.42, -0.3, 0.05, 0.84, 0.04, 0.48);
    let glass = screen(p.canvas, p.theme, &p.frame, &lid, color, p.theme.nodes.screen, (0.05, 0.07));
    window_content(p, &glass);
}

fn mobile(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.device);
    p.solid(&LocalBox::centered(0.26, 0.16, 0.0, 0.04), color.shade(0.82));
    let phone = LocalBox::new(-0.2, -0.05, 0.04, 0.4, 0.07, 0.76);
    let glass = screen(p.canvas, p.theme, &p.frame, &phone, p.theme.nodes.glyph, p.theme.nodes.screen, (0.06, 0.04));
    let glow = p.theme.nodes.screen_glow;
    p.fill(&glass, (0.34, 0.66), (0.93, 0.97), p.theme.nodes.glyph);
    p.fill(&glass, (0.1, 0.9), (0.56, 0.86), glow);
    for row in 0..3 {
        let v = 0.12 + row as f64 * 0.13;
        p.fill(&glass, (0.1, 0.24), (v, v + 0.08), p.theme.nodes.accent);
        p.fill(&glass, (0.32, 0.9), (v, v + 0.08), glow.shade(0.7));
    }
}

fn voip_phone(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.device);
    let base = LocalBox::centered(0.36, 0.3, 0.0, 0.14);
    p.solid(&base, color);
    let top = FaceRegion::of(base, Face::Top);
    p.fill(&top, (0.46, 0.92), (0.1, 0.36), p.theme.nodes.screen);
    p.fill(&top, (0.52, 0.86), (0.18, 0.28), p.theme.nodes.screen_glow);
    for row in 0..3 {
        for column in 0..3 {
            let (u, v) = (0.54 + column as f64 * 0.15, 0.52 + row as f64 * 0.15);
            p.fill(&top, (u - 0.05, u + 0.05), (v - 0.045, v + 0.045), p.theme.nodes.glyph.mix(color, 0.35));
        }
    }
    let front = FaceRegion::of(base, Face::Front);
    let led = p.theme.nodes.led;
    p.moving(IconMotion::Blink { phase: 0.0 }, |p| p.dot(&front, (0.88, 0.5), 1.6, led));
    p.solid(&LocalBox::new(-0.32, -0.26, 0.14, 0.2, 0.52, 0.08), p.theme.nodes.server);
}

fn printer(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.device);
    let body = LocalBox::centered(0.42, 0.34, 0.0, 0.34);
    p.solid(&body, color);
    let front = FaceRegion::of(body, Face::Front);
    p.fill(&front, (0.12, 0.88), (0.52, 0.66), p.theme.nodes.screen);
    p.line(&front, &[(0.3, 0.24), (0.7, 0.24)], color.shade(0.55), 1.4);
    let top = FaceRegion::of(body, Face::Top);
    p.fill(&top, (0.74, 0.94), (0.1, 0.3), p.theme.nodes.screen);
    let led = p.theme.nodes.led;
    p.moving(IconMotion::Blink { phase: 0.0 }, |p| p.dot(&top, (0.84, 0.2), 1.4, led));
    let sheet = FaceRegion::of(body, Face::Top).inset(0.18, 0.7, 0.5, 0.92);
    p.fill(&sheet, (0.0, 1.0), (0.0, 1.0), p.theme.nodes.paper);
    for v in [0.3, 0.55, 0.8] {
        p.line(&sheet, &[(0.15, v), (0.85, v)], p.theme.nodes.glyph.mix(p.theme.nodes.paper, 0.6), 1.0);
    }
    p.solid(&LocalBox::new(-0.28, -0.3, 0.34, 0.5, 0.03, 0.18), p.theme.nodes.paper);
}

fn atm(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.server);
    let kiosk = LocalBox::centered(0.34, 0.3, 0.0, 1.0);
    p.solid(&kiosk, color);
    let front = FaceRegion::of(kiosk, Face::Front);
    p.fill(&front, (0.0, 1.0), (0.86, 0.97), p.theme.nodes.accent);
    let glass = front.inset(0.14, 0.86, 0.53, 0.8);
    p.fill(&glass, (0.0, 1.0), (0.0, 1.0), p.theme.nodes.screen);
    p.fill(&glass, (0.12, 0.88), (0.62, 0.8), p.theme.nodes.screen_glow);
    p.fill(&glass, (0.12, 0.6), (0.24, 0.42), p.theme.nodes.screen_glow.shade(0.65));
    for row in 0..3 {
        for column in 0..3 {
            let (u, v) = (0.3 + column as f64 * 0.11, 0.26 + row as f64 * 0.065);
            p.fill(&front, (u - 0.035, u + 0.035), (v - 0.02, v + 0.02), p.theme.nodes.port);
        }
    }
    p.line(&front, &[(0.7, 0.4), (0.84, 0.4)], p.theme.nodes.glyph, 1.8);
    p.fill(&front, (0.2, 0.8), (0.1, 0.15), p.theme.nodes.glyph);
}

fn function(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.function);
    // A face of the hexagon looks straight at the camera (normal at 45°).
    let shape = Prism { sides: 6, radius: 0.42, z: 0.0, height: 0.52, phase_degrees: 15.0 };
    if let Some(face) = prism(p.canvas, p.theme, &p.frame, &shape, color) {
        let lambda = |p: &mut Painter<'_, '_>, stroke: &Stroke| {
            face.polyline(p.canvas, &[(0.24, 0.84), (0.38, 0.84), (0.74, 0.14)], stroke, false);
            face.polyline(p.canvas, &[(0.53, 0.48), (0.28, 0.14)], stroke, false);
        };
        lambda(p, &detail_stroke(&p.frame, p.theme.nodes.glyph, 2.6));
        // The λ lights up now and then, as an invocation runs.
        let glow = detail_stroke(&p.frame, p.theme.nodes.paper, 2.6);
        p.overlay(IconMotion::Shine { phase: 0.0 }, |p| lambda(p, &glow));
    }
}

fn container(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.container);
    let body = LocalBox::centered(0.52, 0.3, 0.0, 0.5);
    p.solid(&body, color);
    let ribs = color.shade(0.6);
    let front = FaceRegion::of(body, Face::Front);
    for index in 1..10 {
        let u = index as f64 / 10.0;
        p.line(&front, &[(u, 0.08), (u, 0.92)], ribs, 1.0);
    }
    let door = FaceRegion::of(body, Face::Side);
    p.line(&door, &[(0.5, 0.04), (0.5, 0.96)], ribs, 1.2);
    for u in [0.3, 0.7] {
        p.line(&door, &[(u, 0.1), (u, 0.9)], color.shade(0.45), 1.8);
    }
}

fn api_gateway(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.api_gateway);
    p.solid(&LocalBox::centered(0.52, 0.26, 0.0, 0.1), color.shade(0.85));
    let pillar_height = GATEWAY_TOP - 0.1 - 0.14;
    p.solid(&LocalBox::new(-0.48, -0.16, 0.1, 0.18, 0.32, pillar_height), color);
    p.solid(&LocalBox::new(0.3, -0.16, 0.1, 0.18, 0.32, pillar_height), color);
    let lintel = LocalBox::new(-0.52, -0.2, GATEWAY_TOP - 0.14, 1.04, 0.4, 0.14);
    p.solid(&lintel, color);
    let front = FaceRegion::of(lintel, Face::Front);
    for u in [0.4, 0.5, 0.6] {
        p.line(&front, &[(u - 0.025, 0.25), (u + 0.025, 0.5), (u - 0.025, 0.75)], p.theme.nodes.mark, 1.8);
    }
}

fn cdn(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.cdn);
    p.solid(&LocalBox::centered(0.42, 0.42, 0.0, CDN_BASE), p.theme.nodes.server);
    let center_z = CDN_BASE + CDN_GLOBE_RADIUS * SPHERE_SILHOUETTE;
    let orbit_radius = 0.42;
    let accent = p.theme.nodes.accent;
    let orbit_center = p.canvas.project_quiet(p.frame.plan(0.0, 0.0), p.frame.elevation(center_z));
    let (orbit_x, orbit_y) = p.canvas.projection().circle_radii(p.frame.tiles(orbit_radius));
    let orbit_stroke = detail_stroke(&p.frame, accent, 1.6);
    let half_orbit = |upper: bool| {
        PathData::new()
            .move_to(ScreenPoint::new(orbit_center.x - orbit_x, orbit_center.y))
            .arc_to(orbit_x, orbit_y, false, upper, ScreenPoint::new(orbit_center.x + orbit_x, orbit_center.y))
            .include(Rect::new(
                orbit_center.x - orbit_x,
                orbit_center.y - orbit_y,
                orbit_center.x + orbit_x,
                orbit_center.y + orbit_y,
            ))
    };
    p.canvas.path(&half_orbit(true), &Style::stroke(orbit_stroke.clone()));
    let (globe, radius) = sphere(p.canvas, p.theme, &p.frame, (0.0, 0.0, center_z), CDN_GLOBE_RADIUS, color);
    let lines = detail_stroke(&p.frame, color.shade(0.62), 1.1);
    let (equator_x, equator_y) = p.canvas.projection().circle_radii(p.frame.tiles(CDN_GLOBE_RADIUS));
    p.canvas.ellipse(globe, equator_x, equator_y, &Style::stroke(lines.clone()));
    p.canvas.ellipse(globe, radius * 0.45, radius, &Style::stroke(lines.clone()));
    p.canvas.line(ScreenPoint::new(globe.x, globe.y - radius), ScreenPoint::new(globe.x, globe.y + radius), &lines);
    p.canvas.path(&half_orbit(false), &Style::stroke(orbit_stroke));
    let edge_style = Style::fill(accent).with_stroke(Stroke::new(p.theme.stroke, p.theme.stroke_width));
    for (index, angle) in [200f64, 270.0, 340.0].into_iter().enumerate() {
        let radians = angle.to_radians();
        let node = ScreenPoint::new(orbit_center.x + orbit_x * radians.cos(), orbit_center.y - orbit_y * radians.sin());
        let radius = p.frame.px(3.0);
        p.moving(IconMotion::Blink { phase: sequence(index, 3) }, |p| p.canvas.circle(node, radius, &edge_style));
    }
}

fn dns(p: &mut Painter<'_, '_>) {
    let pole_color = p.main(p.theme.nodes.device);
    p.solid(&LocalBox::centered(0.32, 0.32, 0.0, 0.08), p.theme.nodes.server);
    p.solid(&LocalBox::centered(0.04, 0.04, 0.08, DNS_POLE_TOP - 0.08), pole_color);
    let marks = p.theme.nodes.mark;
    let plates = [
        (
            SignPlate { along_a: true, across: 0.0, span: (0.04, 0.5), heights: (0.62, 0.8), thickness: 0.04 },
            p.theme.nodes.accent,
        ),
        (
            SignPlate { along_a: false, across: 0.0, span: (0.04, 0.46), heights: (0.38, 0.54), thickness: 0.04 },
            p.theme.nodes.load_balancer,
        ),
    ];
    for (plate, color) in plates {
        let face = sign_plate(p.canvas, p.theme, &p.frame, &plate, color);
        let stroke = detail_stroke(&p.frame, marks, 1.6);
        face.polyline(p.canvas, &[(0.14, 0.5), (0.56, 0.5)], &stroke, false);
    }
}

fn k8s_cluster(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.kubernetes);
    let height = 0.2;
    let shape = Prism { sides: 7, radius: 0.62, z: 0.0, height, phase_degrees: 45.0 };
    prism(p.canvas, p.theme, &p.frame, &shape, color);
    // Helm wheel on the top.
    let paper = p.theme.nodes.mark;
    floor_ring(p.canvas, &p.frame, (0.0, 0.0), 0.28, height, &detail_stroke(&p.frame, paper, 2.4));
    let spoke = detail_stroke(&p.frame, paper, 2.0);
    let hub = p.at(0.0, 0.0, height);
    // The helm turns slowly, one spoke per cycle.
    let spin = IconMotion::Spin { center: hub, step_degrees: 360.0 / HELM_SPOKES as f64, phase: 0.0 };
    p.moving(spin, |p| {
        for index in 0..HELM_SPOKES {
            let angle = (45.0 + 360.0 * index as f64 / HELM_SPOKES as f64).to_radians();
            let from = p.at(0.07 * angle.cos(), 0.07 * angle.sin(), height);
            let to = p.at(0.38 * angle.cos(), 0.38 * angle.sin(), height);
            p.canvas.line(from, to, &spoke);
        }
    });
    let (hub_x, hub_y) = p.canvas.projection().circle_radii(p.frame.tiles(0.08));
    p.canvas.ellipse(hub, hub_x, hub_y, &Style::fill(paper));
}

fn pod(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.pod);
    let shape = Prism { sides: 6, radius: 0.34, z: 0.0, height: 0.36, phase_degrees: 15.0 };
    if let Some(face) = prism(p.canvas, p.theme, &p.frame, &shape, color) {
        // Cube glyph (the Kubernetes pod symbol).
        let stroke = detail_stroke(&p.frame, p.theme.nodes.mark, 1.4);
        let outline = [(0.5, 0.86), (0.8, 0.7), (0.8, 0.3), (0.5, 0.14), (0.2, 0.3), (0.2, 0.7)];
        face.polyline(p.canvas, &outline, &stroke, true);
        face.polyline(p.canvas, &[(0.2, 0.7), (0.5, 0.54), (0.8, 0.7)], &stroke, false);
        face.polyline(p.canvas, &[(0.5, 0.54), (0.5, 0.14)], &stroke, false);
    }
}

fn monitoring(p: &mut Painter<'_, '_>) {
    let color = p.main(p.theme.nodes.server);
    p.solid(&LocalBox::centered(0.22, 0.16, 0.0, 0.04), color.shade(0.85));
    p.solid(&LocalBox::new(-0.04, -0.03, 0.04, 0.08, 0.06, 0.18), color.shade(0.85));
    let panel = LocalBox::new(-0.44, -0.04, 0.22, 0.88, 0.08, 0.58);
    let glass = screen(p.canvas, p.theme, &p.frame, &panel, color, p.theme.nodes.screen, (0.04, 0.06));
    let grid = p.theme.nodes.screen_glow.mix(p.theme.nodes.screen, 0.6);
    for v in [0.35, 0.6, 0.85] {
        p.line(&glass, &[(0.05, v), (0.95, v)], grid, 0.8);
    }
    let bars = [0.22, 0.34, 0.18, 0.4, 0.3, 0.46];
    let bar_color = p.theme.nodes.led.shade(0.85);
    for (index, height) in bars.iter().enumerate() {
        let u = 0.1 + index as f64 * 0.14;
        // Live data: the bars rise and fall in a wave running across the chart.
        let anchor = glass.point(p.canvas, &p.frame, u + 0.035, 0.08);
        let phase = sequence(index, bars.len());
        p.moving(IconMotion::Bob { anchor, phase }, |p| {
            p.fill(&glass, (u, u + 0.07), (0.08, 0.08 + height), bar_color)
        });
    }
    let line = [(0.06, 0.5), (0.24, 0.62), (0.4, 0.55), (0.56, 0.78), (0.72, 0.66), (0.92, 0.88)];
    let accent = p.theme.nodes.accent;
    p.line(&glass, &line, accent, 2.0);
    if let Some(&last) = line.last() {
        p.moving(IconMotion::Blink { phase: 0.0 }, |p| p.dot(&glass, last, 2.4, accent));
    }
}

/// Phase of the `index`-th of `count` parts lighting in sequence.
fn sequence(index: usize, count: usize) -> f64 {
    index as f64 / count.max(1) as f64
}
