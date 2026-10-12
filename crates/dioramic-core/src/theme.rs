//! Themes: color tokens and typography (`docs/06-renderizacao.md#temas`, `docs/10-marca.md#cores`).
//!
//! `blueprint` and `midnight` carry the prototype's tokens (`docs/spike/scene.mjs`); `paper` (warm, pastel,
//! softer strokes) and `mono` (greys, no shadow, for print) follow the same structure. New icon colors are
//! additive fields of [`NodePalette`].

use dioramic_spec::{ColorRef, ColorToken, EdgeColors, EdgeKind, LayoutAlgorithm, Routing, ThemeName};

use crate::color::Color;

/// Fill and stroke of a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneColors {
    /// Translucent floor fill.
    pub fill: Color,
    /// Dashed border (also the "strong" tone of the token).
    pub stroke: Color,
}

/// Zone palette indexed by [`ColorToken`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZonePalette {
    /// Blue.
    pub blue: ZoneColors,
    /// Orange.
    pub orange: ZoneColors,
    /// Teal.
    pub teal: ZoneColors,
    /// Green.
    pub green: ZoneColors,
    /// Violet.
    pub violet: ZoneColors,
    /// Pink.
    pub pink: ZoneColors,
    /// Red.
    pub red: ZoneColors,
    /// Gray.
    pub gray: ZoneColors,
}

impl ZonePalette {
    /// Colors for a token.
    pub fn get(&self, token: ColorToken) -> ZoneColors {
        match token {
            ColorToken::Blue => self.blue,
            ColorToken::Orange => self.orange,
            ColorToken::Teal => self.teal,
            ColorToken::Green => self.green,
            ColorToken::Violet => self.violet,
            ColorToken::Pink => self.pink,
            ColorToken::Red => self.red,
            ColorToken::Gray => self.gray,
        }
    }
}

/// Look of one connection class ([`EdgeKind`]) under semantic edge colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindStyle {
    /// Stroke (and packet) color.
    pub color: Color,
    /// Stroke pattern, so the class reads without color too (color-blind safe).
    pub line: LinePattern,
}

/// Solid or dashed stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinePattern {
    /// Continuous stroke.
    #[default]
    Solid,
    /// Dashed (or dotted, per [`EdgeDrawing::dash`]).
    Dashed,
}

/// Semantic edge palette (`edge.kinds.<kind>`), one look per [`EdgeKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeKindPalette {
    /// Database, storage, NAS.
    pub data: KindStyle,
    /// Cache.
    pub cache: KindStyle,
    /// Queues, streams, async links.
    pub messaging: KindStyle,
    /// Internet, third parties, CDN, DNS.
    pub external: KindStyle,
    /// End-user clients.
    pub client: KindStyle,
    /// Service to service.
    pub service: KindStyle,
    /// Metrics, logs, traces.
    pub observability: KindStyle,
}

impl EdgeKindPalette {
    /// Look of a class.
    pub fn get(&self, kind: EdgeKind) -> KindStyle {
        match kind {
            EdgeKind::Data => self.data,
            EdgeKind::Cache => self.cache,
            EdgeKind::Messaging => self.messaging,
            EdgeKind::External => self.external,
            EdgeKind::Client => self.client,
            EdgeKind::Service => self.service,
            EdgeKind::Observability => self.observability,
        }
    }

    /// The semantic palette of a classic theme, derived from its zone strokes and edge styles, so every
    /// theme can opt into `edge_colors: semantic` without hand-picked hex.
    fn derived(zones: &ZonePalette, edges_secondary: Color) -> Self {
        let solid = |color: Color| KindStyle { color, line: LinePattern::Solid };
        Self {
            data: solid(zones.red.stroke),
            cache: solid(zones.orange.stroke),
            messaging: KindStyle { color: zones.violet.stroke, line: LinePattern::Dashed },
            external: solid(zones.gray.stroke),
            client: solid(zones.teal.stroke),
            service: solid(edges_secondary),
            observability: solid(zones.green.stroke),
        }
    }
}

/// Built-in edge styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgePalette {
    /// `primary` (amber; default).
    pub primary: Color,
    /// `secondary` (indigo).
    pub secondary: Color,
    /// `muted`.
    pub muted: Color,
    /// `danger`.
    pub danger: Color,
    /// `success`.
    pub success: Color,
    /// Semantic palette by connection class (`layout.edge_colors: semantic`).
    pub kinds: EdgeKindPalette,
}

/// Where the arrow head of an edge sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArrowPosition {
    /// At the target port (classic).
    #[default]
    End,
    /// Halfway along the edge, pointing along the flow (schematic drawings).
    Middle,
}

/// How dashed edges (`..>`, `..`, `dashed: true`) are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashPattern {
    /// Short dashes.
    #[default]
    Dashes,
    /// Round dots spaced along the edge.
    Dots,
}

/// Stroke of the leader line under a node label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeaderStyle {
    /// Continuous.
    #[default]
    Solid,
    /// Dotted.
    Dotted,
}

/// How zone floors are painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoneFill {
    /// Translucent floor plus border.
    #[default]
    Filled,
    /// Border (and label) only: the floor stays the page.
    Outline,
}

/// Finish of the procedural icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconFinish {
    /// Colored bodies, glowing screens with windows (classic).
    #[default]
    Shaded,
    /// Silver bodies, light-blue glass with a reflection, dark glyphs (schematic).
    Silver,
}

/// Edge drawing tokens beyond color and width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeDrawing {
    /// Where the arrow head sits.
    pub arrow_position: ArrowPosition,
    /// Arrow head color; `None` = the edge's color.
    pub arrow_color: Option<Color>,
    /// Dashed edges as dashes or dots.
    pub dash: DashPattern,
    /// Routing when the spec writes none; `None` = the spec default (`curved`).
    pub default_routing: Option<Routing>,
    /// Edge coloring when the spec writes none (`layout.edge_colors`).
    pub colors: EdgeColors,
}

/// The classic edge drawing of `blueprint`, `midnight`, `paper` and `mono`.
const CLASSIC_EDGES: EdgeDrawing = EdgeDrawing {
    arrow_position: ArrowPosition::End,
    arrow_color: None,
    dash: DashPattern::Dashes,
    default_routing: None,
    colors: EdgeColors::Style,
};

/// Node label box colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelColors {
    /// Box fill.
    pub background: Color,
    /// Text.
    pub text: Color,
    /// Box border.
    pub border: Color,
    /// Leader line.
    pub leader: Color,
}

/// Pill colors (edge labels and zone labels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PillColors {
    /// Fill.
    pub background: Color,
    /// Text.
    pub text: Color,
    /// Hairline border; `None` = no border (classic dark pills).
    pub border: Option<Color>,
}

/// Base colors of procedural icons (before face shading). T-102 may add fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodePalette {
    /// Server / rack body.
    pub server: Color,
    /// LEDs ("on" details).
    pub led: Color,
    /// Load balancer / switch slab.
    pub load_balancer: Color,
    /// Ports on slabs.
    pub port: Color,
    /// Database cylinders.
    pub database: Color,
    /// Cache bottom cylinder.
    pub cache: Color,
    /// Cache top cylinder.
    pub cache_top: Color,
    /// Cloud highlight.
    pub cloud_light: Color,
    /// Cloud shade.
    pub cloud_shade: Color,
    /// Generic block / fallback.
    pub block: Color,
    /// Accent (badges, glyphs).
    pub accent: Color,
    /// Switch slab.
    pub switch: Color,
    /// Router puck.
    pub router: Color,
    /// Firewall bricks.
    pub firewall: Color,
    /// Access point puck and dome.
    pub access_point: Color,
    /// Storage bucket.
    pub storage: Color,
    /// Queue tube.
    pub queue: Color,
    /// Function prism (λ).
    pub function: Color,
    /// Container box.
    pub container: Color,
    /// API gateway arch.
    pub api_gateway: Color,
    /// CDN globe.
    pub cdn: Color,
    /// Kubernetes cluster platform.
    pub kubernetes: Color,
    /// Pod prism.
    pub pod: Color,
    /// End-user devices (laptop, phone, printer) body.
    pub device: Color,
    /// Screen glass (off).
    pub screen: Color,
    /// Screen content (windows, charts).
    pub screen_glow: Color,
    /// Person body.
    pub person: Color,
    /// Person head.
    pub person_head: Color,
    /// Paper and light glyphs (printer sheets, arrows on tops).
    pub paper: Color,
    /// Dark glyphs on light bodies (λ, keypads).
    pub glyph: Color,
    /// Base (block top, stand panel) under flat icons without their own backdrop (Azure, GCP, Kubernetes).
    pub plinth: Color,
    /// Glyphs printed on a body (router and switch arrows, gateway chevrons, helm, cache bolt, ripples):
    /// `paper` on colored bodies, a dark tone on silver ones.
    pub mark: Color,
}

/// Soft shadow under nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowStyle {
    /// Color.
    pub color: Color,
    /// Opacity.
    pub opacity: f64,
    /// Gaussian blur std deviation (px).
    pub blur: f64,
}

/// Face shading multipliers (light from the top-left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceShading {
    /// Top face (1.0).
    pub top: f64,
    /// Left face, facing +y (0.8).
    pub left: f64,
    /// Right face, facing +x (0.62).
    pub right: f64,
}

/// Font sizes and weights of the diagram.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typography {
    /// Node label size (px).
    pub node_label_size: f64,
    /// Node label weight.
    pub node_label_weight: u16,
    /// Edge pill size.
    pub pill_size: f64,
    /// Edge pill weight.
    pub pill_weight: u16,
    /// Zone label size.
    pub zone_label_size: f64,
    /// Zone label weight.
    pub zone_label_weight: u16,
    /// Zone label letter spacing.
    pub zone_label_letter_spacing: f64,
    /// Title size.
    pub title_size: f64,
    /// Subtitle size.
    pub subtitle_size: f64,
}

/// A complete theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Name.
    pub name: ThemeName,
    /// Page background.
    pub background: Color,
    /// Grid lines.
    pub grid: Color,
    /// Title and strong text.
    pub ink: Color,
    /// Subtitle text.
    pub subtle: Color,
    /// Outline of every solid.
    pub stroke: Color,
    /// Outline width (px).
    pub stroke_width: f64,
    /// Zone fill opacity.
    pub zone_opacity: f64,
    /// Zone palette.
    pub zones: ZonePalette,
    /// Edge palette.
    pub edges: EdgePalette,
    /// Default edge width (px).
    pub edge_width: f64,
    /// Node labels.
    pub label: LabelColors,
    /// Pills.
    pub pill: PillColors,
    /// Icons.
    pub nodes: NodePalette,
    /// Shadow.
    pub shadow: ShadowStyle,
    /// Packet glow opacity.
    pub glow_opacity: f64,
    /// Vignette color (never used in GIF output).
    pub vignette: Color,
    /// Face shading.
    pub faces: FaceShading,
    /// No large gradients (safe for 256-color GIF).
    pub gif_safe: bool,
    /// Typography.
    pub typography: Typography,
    /// Saturation of flat (provider) icons: 1 keeps brand colors, 0 prints them in grey (`mono`).
    pub icon_saturation: f64,
    /// Arrow position, dash pattern, default routing and coloring of edges.
    pub edge_drawing: EdgeDrawing,
    /// Leader line under node labels.
    pub label_leader: LeaderStyle,
    /// Zone floors filled or outlined.
    pub zone_fill: ZoneFill,
    /// Finish of the procedural icons.
    pub icon_finish: IconFinish,
    /// Placement algorithm when the spec writes no `layout.algorithm`; `None` = the spec default (`flow`).
    pub default_layout: Option<LayoutAlgorithm>,
}

const FACES: FaceShading = FaceShading { top: 1.0, left: 0.8, right: 0.62 };
/// Gentler shading for light silver bodies: the faces stay readable without going grey.
const SOFT_FACES: FaceShading = FaceShading { top: 1.0, left: 0.9, right: 0.78 };

const TYPOGRAPHY: Typography = Typography {
    node_label_size: 14.0,
    node_label_weight: 500,
    pill_size: 11.0,
    pill_weight: 700,
    zone_label_size: 12.0,
    zone_label_weight: 700,
    zone_label_letter_spacing: 0.6,
    title_size: 26.0,
    subtitle_size: 14.0,
};

const fn zone(fill: u32, stroke: u32) -> ZoneColors {
    ZoneColors { fill: Color::from_u32(fill), stroke: Color::from_u32(stroke) }
}

impl Theme {
    /// Returns a built-in theme.
    pub fn builtin(name: ThemeName) -> Theme {
        match name {
            ThemeName::Blueprint => Self::blueprint(),
            ThemeName::Midnight => Self::midnight(),
            ThemeName::Paper => Self::paper(),
            ThemeName::Mono => Self::mono(),
            ThemeName::Schematic => Self::schematic(),
        }
    }

    /// Light theme, faithful to the reference (default).
    pub fn blueprint() -> Theme {
        let zones = ZonePalette {
            blue: zone(0xcdd6f6, 0x5468d4),
            orange: zone(0xfbe2c8, 0xd5843a),
            teal: zone(0xc9e9e4, 0x2c9a8c),
            green: zone(0xd3ecd2, 0x3f9a4f),
            violet: zone(0xded5f6, 0x7a5bd3),
            pink: zone(0xf6d5e6, 0xc8508e),
            red: zone(0xf8d3d1, 0xd04a43),
            gray: zone(0xdfe3ea, 0x7b8496),
        };
        Theme {
            name: ThemeName::Blueprint,
            background: Color::from_u32(0xf2f4f8),
            grid: Color::from_u32(0xdde2ec),
            ink: Color::from_u32(0x1c2130),
            subtle: Color::from_u32(0x5b6478),
            stroke: Color::from_u32(0x252b3b),
            stroke_width: 1.1,
            zone_opacity: 0.75,
            zones,
            edges: EdgePalette {
                primary: Color::from_u32(0xd27a2c),
                secondary: Color::from_u32(0x3e50c8),
                muted: Color::from_u32(0x8a93a8),
                danger: Color::from_u32(0xd0453e),
                success: Color::from_u32(0x2c9a6a),
                kinds: EdgeKindPalette::derived(&zones, Color::from_u32(0x3e50c8)),
            },
            edge_width: 2.6,
            label: LabelColors {
                background: Color::from_u32(0xffffff),
                text: Color::from_u32(0x1c2130),
                border: Color::from_u32(0xc8cfdc),
                leader: Color::from_u32(0x252b3b),
            },
            pill: PillColors { background: Color::from_u32(0x1f2434), text: Color::from_u32(0xffffff), border: None },
            nodes: NodePalette {
                server: Color::from_u32(0x56607a),
                led: Color::from_u32(0x7ef0a8),
                load_balancer: Color::from_u32(0x3d4fd1),
                port: Color::from_u32(0xb9c3ff),
                database: Color::from_u32(0xa3acc0),
                cache: Color::from_u32(0xe0534f),
                cache_top: Color::from_u32(0xef6b5f),
                cloud_light: Color::from_u32(0xffffff),
                cloud_shade: Color::from_u32(0xd9dee8),
                block: Color::from_u32(0x8a93a8),
                accent: Color::from_u32(0xe8833a),
                switch: Color::from_u32(0x2f7fc1),
                router: Color::from_u32(0x2c9a8c),
                firewall: Color::from_u32(0xd8643a),
                access_point: Color::from_u32(0xf4f6fb),
                storage: Color::from_u32(0x4fae7b),
                queue: Color::from_u32(0xe8833a),
                function: Color::from_u32(0xf5b335),
                container: Color::from_u32(0x2496ed),
                api_gateway: Color::from_u32(0x7a5bd3),
                cdn: Color::from_u32(0x38a3d9),
                kubernetes: Color::from_u32(0x326ce5),
                pod: Color::from_u32(0x5b8def),
                device: Color::from_u32(0xd9dee8),
                screen: Color::from_u32(0x1f2434),
                screen_glow: Color::from_u32(0x6f86f5),
                person: Color::from_u32(0x5468d4),
                person_head: Color::from_u32(0xf2c9a5),
                paper: Color::from_u32(0xffffff),
                glyph: Color::from_u32(0x1c2130),
                plinth: Color::from_u32(0xf7f9fc),
                mark: Color::from_u32(0xffffff),
            },
            shadow: ShadowStyle { color: Color::from_u32(0x1b2440), opacity: 0.22, blur: 5.0 },
            glow_opacity: 0.28,
            vignette: Color::from_u32(0xc9d1e0),
            faces: FACES,
            gif_safe: true,
            typography: TYPOGRAPHY,
            icon_saturation: 1.0,
            edge_drawing: CLASSIC_EDGES,
            label_leader: LeaderStyle::Solid,
            zone_fill: ZoneFill::Filled,
            icon_finish: IconFinish::Shaded,
            default_layout: None,
        }
    }

    /// Dark theme: neon edges with glow.
    pub fn midnight() -> Theme {
        let zones = ZonePalette {
            blue: zone(0x1b2652, 0x6f86f5),
            orange: zone(0x3a2818, 0xf2a05a),
            teal: zone(0x123432, 0x38c2ae),
            green: zone(0x15321d, 0x55d27a),
            violet: zone(0x272048, 0x8f73f2),
            pink: zone(0x3a1a2e, 0xf06aae),
            red: zone(0x3a1818, 0xff6a62),
            gray: zone(0x1d2333, 0x8d97ae),
        };
        Theme {
            name: ThemeName::Midnight,
            background: Color::from_u32(0x0c111d),
            grid: Color::from_u32(0x182033),
            ink: Color::from_u32(0xeef1f8),
            subtle: Color::from_u32(0x8d97ae),
            stroke: Color::from_u32(0x333d5c),
            stroke_width: 1.1,
            zone_opacity: 0.8,
            zones,
            edges: EdgePalette {
                primary: Color::from_u32(0xff9f4a),
                secondary: Color::from_u32(0x7d8cff),
                muted: Color::from_u32(0x6b7590),
                danger: Color::from_u32(0xff5a54),
                success: Color::from_u32(0x5cffa1),
                kinds: EdgeKindPalette::derived(&zones, Color::from_u32(0x7d8cff)),
            },
            edge_width: 2.6,
            label: LabelColors {
                background: Color::from_u32(0x141b2c),
                text: Color::from_u32(0xeef1f8),
                border: Color::from_u32(0x3a4870),
                leader: Color::from_u32(0x6b7896),
            },
            pill: PillColors { background: Color::from_u32(0xeef1f8), text: Color::from_u32(0x0c111d), border: None },
            nodes: NodePalette {
                server: Color::from_u32(0x5d6a96),
                led: Color::from_u32(0x5cffa1),
                load_balancer: Color::from_u32(0x5468ff),
                port: Color::from_u32(0xc8d0ff),
                database: Color::from_u32(0x8f9ab4),
                cache: Color::from_u32(0xff5a54),
                cache_top: Color::from_u32(0xff7a66),
                cloud_light: Color::from_u32(0xf4f6fb),
                cloud_shade: Color::from_u32(0xaab4c8),
                block: Color::from_u32(0x6b7590),
                accent: Color::from_u32(0xff9f4a),
                switch: Color::from_u32(0x3d93d6),
                router: Color::from_u32(0x38c2ae),
                firewall: Color::from_u32(0xf0784a),
                access_point: Color::from_u32(0xe6eaf2),
                storage: Color::from_u32(0x55c98a),
                queue: Color::from_u32(0xff9f4a),
                function: Color::from_u32(0xffc145),
                container: Color::from_u32(0x3aa0ff),
                api_gateway: Color::from_u32(0x8f73f2),
                cdn: Color::from_u32(0x4cb8ee),
                kubernetes: Color::from_u32(0x4a80f0),
                pod: Color::from_u32(0x6f9bff),
                device: Color::from_u32(0xb8c1d4),
                screen: Color::from_u32(0x0a0e18),
                screen_glow: Color::from_u32(0x7d8cff),
                person: Color::from_u32(0x6f86f5),
                person_head: Color::from_u32(0xf0c6a0),
                paper: Color::from_u32(0xf4f6fb),
                glyph: Color::from_u32(0x0c111d),
                plinth: Color::from_u32(0xe4e9f4),
                mark: Color::from_u32(0xf4f6fb),
            },
            shadow: ShadowStyle { color: Color::from_u32(0x000000), opacity: 0.45, blur: 5.0 },
            glow_opacity: 0.45,
            vignette: Color::from_u32(0x000000),
            faces: FACES,
            gif_safe: true,
            typography: TYPOGRAPHY,
            icon_saturation: 1.0,
            edge_drawing: CLASSIC_EDGES,
            label_leader: LeaderStyle::Solid,
            zone_fill: ZoneFill::Filled,
            icon_finish: IconFinish::Shaded,
            default_layout: None,
        }
    }

    /// Warm off-white, pastel, softer strokes.
    pub fn paper() -> Theme {
        let zones = ZonePalette {
            blue: zone(0xd8e1f3, 0x6b84c9),
            orange: zone(0xf8e2c6, 0xd08b47),
            teal: zone(0xd2ebe3, 0x4a9f8f),
            green: zone(0xdcebd3, 0x6a9e5a),
            violet: zone(0xe5dcf2, 0x8b72c4),
            pink: zone(0xf5dbe4, 0xc46a8e),
            red: zone(0xf5d8d2, 0xc9625a),
            gray: zone(0xe7e3db, 0x8c857a),
        };
        Theme {
            name: ThemeName::Paper,
            background: Color::from_u32(0xf7f3ea),
            grid: Color::from_u32(0xe9e2d3),
            ink: Color::from_u32(0x2e2a24),
            subtle: Color::from_u32(0x7a7266),
            stroke: Color::from_u32(0x4a4339),
            stroke_width: 1.0,
            zone_opacity: 0.7,
            zones,
            edges: EdgePalette {
                primary: Color::from_u32(0xd9894a),
                secondary: Color::from_u32(0x5a6fc4),
                muted: Color::from_u32(0xa39b8e),
                danger: Color::from_u32(0xc9564e),
                success: Color::from_u32(0x4f9a72),
                kinds: EdgeKindPalette::derived(&zones, Color::from_u32(0x5a6fc4)),
            },
            edge_width: 2.4,
            label: LabelColors {
                background: Color::from_u32(0xfffdf8),
                text: Color::from_u32(0x2e2a24),
                border: Color::from_u32(0xddd4c4),
                leader: Color::from_u32(0x4a4339),
            },
            pill: PillColors { background: Color::from_u32(0x3a342c), text: Color::from_u32(0xfffdf8), border: None },
            nodes: NodePalette {
                server: Color::from_u32(0x7d8599),
                led: Color::from_u32(0x8fe3b0),
                load_balancer: Color::from_u32(0x6f7fd6),
                port: Color::from_u32(0xdfe4ff),
                database: Color::from_u32(0xb8bfcc),
                cache: Color::from_u32(0xe6837a),
                cache_top: Color::from_u32(0xf0998c),
                cloud_light: Color::from_u32(0xffffff),
                cloud_shade: Color::from_u32(0xe6e0d4),
                block: Color::from_u32(0xa8a196),
                accent: Color::from_u32(0xe39a5c),
                switch: Color::from_u32(0x6aa0cf),
                router: Color::from_u32(0x6db5a6),
                firewall: Color::from_u32(0xdf8a66),
                access_point: Color::from_u32(0xfbf8f2),
                storage: Color::from_u32(0x7fbf95),
                queue: Color::from_u32(0xe9a46a),
                function: Color::from_u32(0xf0c56a),
                container: Color::from_u32(0x6aa8e6),
                api_gateway: Color::from_u32(0x9d88d6),
                cdn: Color::from_u32(0x79bfe0),
                kubernetes: Color::from_u32(0x6c8fe0),
                pod: Color::from_u32(0x8eaaf0),
                device: Color::from_u32(0xe4dfd5),
                screen: Color::from_u32(0x3a3f4f),
                screen_glow: Color::from_u32(0x9fb0f0),
                person: Color::from_u32(0x7d8ede),
                person_head: Color::from_u32(0xf3d3b5),
                paper: Color::from_u32(0xfffdf8),
                glyph: Color::from_u32(0x2e2a24),
                plinth: Color::from_u32(0xfffcf5),
                mark: Color::from_u32(0xfffdf8),
            },
            shadow: ShadowStyle { color: Color::from_u32(0x5a4a30), opacity: 0.16, blur: 6.0 },
            glow_opacity: 0.22,
            vignette: Color::from_u32(0xe6dccb),
            faces: FACES,
            gif_safe: true,
            typography: TYPOGRAPHY,
            icon_saturation: 1.0,
            edge_drawing: CLASSIC_EDGES,
            label_leader: LeaderStyle::Solid,
            zone_fill: ZoneFill::Filled,
            icon_finish: IconFinish::Shaded,
            default_layout: None,
        }
    }

    /// Greyscale, no shadows (print).
    pub fn mono() -> Theme {
        let zones = ZonePalette {
            blue: zone(0xeaeaea, 0x4d4d4d),
            orange: zone(0xf0f0f0, 0x6e6e6e),
            teal: zone(0xe3e3e3, 0x5c5c5c),
            green: zone(0xededed, 0x7a7a7a),
            violet: zone(0xe6e6e6, 0x444444),
            pink: zone(0xf2f2f2, 0x808080),
            red: zone(0xdedede, 0x3a3a3a),
            gray: zone(0xf4f4f4, 0x8c8c8c),
        };
        Theme {
            name: ThemeName::Mono,
            background: Color::from_u32(0xffffff),
            grid: Color::from_u32(0xececec),
            ink: Color::from_u32(0x1a1a1a),
            subtle: Color::from_u32(0x666666),
            stroke: Color::from_u32(0x222222),
            stroke_width: 1.0,
            zone_opacity: 0.6,
            zones,
            edges: EdgePalette {
                primary: Color::from_u32(0x333333),
                secondary: Color::from_u32(0x777777),
                muted: Color::from_u32(0xaaaaaa),
                danger: Color::from_u32(0x111111),
                success: Color::from_u32(0x555555),
                kinds: EdgeKindPalette::derived(&zones, Color::from_u32(0x777777)),
            },
            edge_width: 2.2,
            label: LabelColors {
                background: Color::from_u32(0xffffff),
                text: Color::from_u32(0x1a1a1a),
                border: Color::from_u32(0xcccccc),
                leader: Color::from_u32(0x444444),
            },
            pill: PillColors { background: Color::from_u32(0x222222), text: Color::from_u32(0xffffff), border: None },
            nodes: NodePalette {
                server: Color::from_u32(0x6e6e6e),
                led: Color::from_u32(0xf2f2f2),
                load_balancer: Color::from_u32(0x5a5a5a),
                port: Color::from_u32(0xdddddd),
                database: Color::from_u32(0xb4b4b4),
                cache: Color::from_u32(0x8c8c8c),
                cache_top: Color::from_u32(0x9e9e9e),
                cloud_light: Color::from_u32(0xffffff),
                cloud_shade: Color::from_u32(0xdcdcdc),
                block: Color::from_u32(0xa0a0a0),
                accent: Color::from_u32(0x444444),
                switch: Color::from_u32(0x707070),
                router: Color::from_u32(0x7c7c7c),
                firewall: Color::from_u32(0x8a8a8a),
                access_point: Color::from_u32(0xf4f4f4),
                storage: Color::from_u32(0x9a9a9a),
                queue: Color::from_u32(0x858585),
                function: Color::from_u32(0xb0b0b0),
                container: Color::from_u32(0x7a7a7a),
                api_gateway: Color::from_u32(0x686868),
                cdn: Color::from_u32(0xa8a8a8),
                kubernetes: Color::from_u32(0x606060),
                pod: Color::from_u32(0x8c8c8c),
                device: Color::from_u32(0xdedede),
                screen: Color::from_u32(0x2a2a2a),
                screen_glow: Color::from_u32(0xbdbdbd),
                person: Color::from_u32(0x6a6a6a),
                person_head: Color::from_u32(0xd8d8d8),
                paper: Color::from_u32(0xffffff),
                glyph: Color::from_u32(0x1a1a1a),
                plinth: Color::from_u32(0xf6f6f6),
                mark: Color::from_u32(0xffffff),
            },
            shadow: ShadowStyle { color: Color::from_u32(0x000000), opacity: 0.0, blur: 0.0 },
            glow_opacity: 0.0,
            vignette: Color::from_u32(0xffffff),
            faces: FACES,
            gif_safe: true,
            typography: TYPOGRAPHY,
            icon_saturation: 0.0,
            edge_drawing: CLASSIC_EDGES,
            label_leader: LeaderStyle::Solid,
            zone_fill: ZoneFill::Filled,
            icon_finish: IconFinish::Shaded,
            default_layout: None,
        }
    }

    /// Light technical drawing for high-level system design: near-white page with a fine grid, silver
    /// icons, outlined zones, thick blue rails with a black arrow halfway, dotted async links and
    /// semantic edge colors (data red, cache amber, messaging violet…).
    pub fn schematic() -> Theme {
        // Zones are outlines only: soft strokes, the label in the same tone.
        let zones = ZonePalette {
            blue: zone(0xe6ecf8, 0x7d93c9),
            orange: zone(0xf8ede0, 0xc49a6c),
            teal: zone(0xe2f1ee, 0x6fa9a0),
            green: zone(0xe5f1e3, 0x7fa77a),
            violet: zone(0xece7f7, 0x9887c4),
            pink: zone(0xf6e6ee, 0xc0869f),
            red: zone(0xf7e4e3, 0xc58783),
            gray: zone(0xeceef2, 0x99a1b0),
        };
        let solid = |hex: u32| KindStyle { color: Color::from_u32(hex), line: LinePattern::Solid };
        let service_blue = 0x1d5bd8;
        Theme {
            name: ThemeName::Schematic,
            background: Color::from_u32(0xf8f9fb),
            grid: Color::from_u32(0xe2e6ed),
            ink: Color::from_u32(0x1f2533),
            subtle: Color::from_u32(0x667085),
            stroke: Color::from_u32(0x5f6879),
            stroke_width: 1.3,
            zone_opacity: 0.0,
            zones,
            edges: EdgePalette {
                primary: Color::from_u32(service_blue),
                secondary: Color::from_u32(0x2f8fd8),
                muted: Color::from_u32(0x98a1b1),
                danger: Color::from_u32(0xd23a3a),
                success: Color::from_u32(0x1e9468),
                kinds: EdgeKindPalette {
                    data: solid(0xd23a3a),
                    cache: solid(0xc97c0a),
                    messaging: KindStyle { color: Color::from_u32(0x7a4ed6), line: LinePattern::Dashed },
                    external: solid(0x6c7a96),
                    client: solid(0x1a8cc2),
                    service: solid(service_blue),
                    observability: solid(0x12968a),
                },
            },
            edge_width: 4.4,
            label: LabelColors {
                background: Color::from_u32(0xffffff),
                text: Color::from_u32(0x1f2533),
                border: Color::from_u32(0xb8c0cc),
                leader: Color::from_u32(0x4f5869),
            },
            pill: PillColors {
                background: Color::from_u32(0xffffff),
                text: Color::from_u32(0x2a3142),
                border: Some(Color::from_u32(0xc3cad5)),
            },
            nodes: NodePalette {
                server: Color::from_u32(0xeaeef4),
                led: Color::from_u32(0x5a6477),
                load_balancer: Color::from_u32(0xe6eaf1),
                port: Color::from_u32(0x5a6477),
                database: Color::from_u32(0xdbe2ec),
                cache: Color::from_u32(0xe4e9f0),
                cache_top: Color::from_u32(0xeef1f6),
                cloud_light: Color::from_u32(0xffffff),
                cloud_shade: Color::from_u32(0xe6eaf0),
                block: Color::from_u32(0xe6eaf1),
                accent: Color::from_u32(0x2f6fe0),
                switch: Color::from_u32(0xe6eaf1),
                router: Color::from_u32(0xe6eaf1),
                firewall: Color::from_u32(0xeee3dc),
                access_point: Color::from_u32(0xf4f6fa),
                storage: Color::from_u32(0xe1e7ef),
                queue: Color::from_u32(0xe6eaf1),
                function: Color::from_u32(0xeef1f6),
                container: Color::from_u32(0xe1e7ef),
                api_gateway: Color::from_u32(0xe6eaf1),
                cdn: Color::from_u32(0xd9e8f8),
                kubernetes: Color::from_u32(0xe1e8f4),
                pod: Color::from_u32(0xeaeff7),
                device: Color::from_u32(0xeef1f5),
                screen: Color::from_u32(0xc4e2f8),
                screen_glow: Color::from_u32(0x93c8ef),
                person: Color::from_u32(0xccd5e2),
                person_head: Color::from_u32(0xf1f3f6),
                paper: Color::from_u32(0xffffff),
                glyph: Color::from_u32(0x3b4354),
                plinth: Color::from_u32(0xf7f9fc),
                mark: Color::from_u32(0x4a5468),
            },
            shadow: ShadowStyle { color: Color::from_u32(0x2a3550), opacity: 0.12, blur: 2.5 },
            glow_opacity: 0.18,
            vignette: Color::from_u32(0xe9ecf1),
            faces: SOFT_FACES,
            gif_safe: true,
            typography: TYPOGRAPHY,
            icon_saturation: 1.0,
            edge_drawing: EdgeDrawing {
                arrow_position: ArrowPosition::Middle,
                arrow_color: Some(Color::from_u32(0x101318)),
                dash: DashPattern::Dots,
                default_routing: Some(Routing::Rails),
                colors: EdgeColors::Semantic,
            },
            label_leader: LeaderStyle::Dotted,
            zone_fill: ZoneFill::Outline,
            icon_finish: IconFinish::Silver,
            default_layout: Some(LayoutAlgorithm::Hub),
        }
    }

    /// Whether nodes cast a soft shadow (`mono` does not).
    pub fn has_shadow(&self) -> bool {
        self.shadow.opacity > 0.0
    }

    /// Zone colors: the token's colors, a hex (fill derived by mixing with the background), or the
    /// palette rotation [`ColorToken::ALL`] by zone index when unset.
    pub fn zone_colors(&self, color: Option<&ColorRef>, zone_index: usize) -> ZoneColors {
        match color {
            Some(ColorRef::Token(token)) => self.zones.get(*token),
            Some(ColorRef::Hex(hex)) => match hex.rgb() {
                Some([r, g, b]) => {
                    let stroke = Color::rgb(r, g, b);
                    ZoneColors { fill: stroke.mix(self.background, 0.75), stroke }
                }
                None => self.zones.gray,
            },
            None => self.zones.get(ColorToken::ALL[zone_index % ColorToken::ALL.len()]),
        }
    }

    /// Color of a built-in edge style name (`primary`, `secondary`, `muted`, `danger`, `success`).
    pub fn builtin_edge_color(&self, style: &str) -> Option<Color> {
        match style {
            "primary" => Some(self.edges.primary),
            "secondary" => Some(self.edges.secondary),
            "muted" => Some(self.edges.muted),
            "danger" => Some(self.edges.danger),
            "success" => Some(self.edges.success),
            _ => None,
        }
    }

    /// Edge coloring for a spec value: what the spec wrote, else the theme's default.
    pub fn edge_colors(&self, written: Option<EdgeColors>) -> EdgeColors {
        written.unwrap_or(self.edge_drawing.colors)
    }

    /// Whether zone floors are painted (`zone_fill: filled`).
    pub fn fills_zones(&self) -> bool {
        self.zone_fill == ZoneFill::Filled
    }

    /// Strong color for a spec color (edges, icon tints): token → zone stroke tone; hex → itself;
    /// malformed hex → `ink`.
    pub fn resolve(&self, color: &ColorRef) -> Color {
        match color {
            ColorRef::Token(token) => self.zones.get(*token).stroke,
            ColorRef::Hex(hex) => hex.rgb().map_or(self.ink, |[r, g, b]| Color::rgb(r, g, b)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_tokens_are_preserved() {
        let blueprint = Theme::blueprint();
        assert_eq!(blueprint.background.to_hex(), "#f2f4f8");
        assert_eq!(blueprint.zones.violet.stroke.to_hex(), "#7a5bd3");
        assert_eq!(Theme::midnight().edges.primary.to_hex(), "#ff9f4a");
    }

    #[test]
    fn every_builtin_theme_is_complete() {
        for name in ThemeName::ALL {
            let theme = Theme::builtin(name);
            assert_eq!(theme.name, name);
            let expected_faces = if theme.icon_finish == IconFinish::Silver { SOFT_FACES } else { FACES };
            assert_eq!(theme.faces, expected_faces);
            assert!(theme.gif_safe, "{name:?} has no large gradients");
            assert_ne!(theme.background, theme.ink);
            assert_ne!(theme.label.background, theme.label.text);
            assert_ne!(theme.pill.background, theme.pill.text);
            assert!(theme.stroke_width > 0.0 && theme.edge_width > 0.0);
        }
    }

    #[test]
    fn paper_is_warm_and_mono_is_grey_without_shadow() {
        let paper = Theme::paper();
        assert!(paper.background.r > paper.background.b, "warm off-white");
        assert!(paper.has_shadow());
        let mono = Theme::mono();
        assert!(!mono.has_shadow());
        let is_grey = |color: Color| color.r == color.g && color.g == color.b;
        let nodes = mono.nodes;
        for color in
            [nodes.server, nodes.cache, nodes.function, nodes.person, mono.edges.primary, mono.zones.blue.stroke]
        {
            assert!(is_grey(color), "{color} is not grey");
        }
    }

    /// WCAG 2.x relative luminance.
    fn relative_luminance(color: Color) -> f64 {
        let linear = |channel: u8| {
            let value = f64::from(channel) / 255.0;
            if value <= 0.039_28 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    /// WCAG 2.x contrast ratio (1..=21), symmetric.
    fn contrast_ratio(first: Color, second: Color) -> f64 {
        let (first, second) = (relative_luminance(first), relative_luminance(second));
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    /// Zone floor as rendered: the translucent fill over the page background.
    fn rendered_zone_floors(theme: &Theme) -> Vec<Color> {
        ColorToken::ALL
            .iter()
            .map(|token| theme.background.mix(theme.zones.get(*token).fill, theme.zone_opacity))
            .collect()
    }

    #[test]
    fn wcag_contrast_matches_known_pairs() {
        assert!((contrast_ratio(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn midnight_silhouettes_and_guides_have_contrast() {
        // T-319: dark bodies must read on every zone; outline, leader and chip border on the page.
        const MIN_BODY_ON_ZONE: f64 = 2.5;
        const MIN_OUTLINE: f64 = 1.5;
        const MIN_LEADER: f64 = 4.0;
        const MIN_CHIP_BORDER: f64 = 2.0;
        const MIN_TEXT: f64 = 4.5;
        let theme = Theme::midnight();
        let nodes = theme.nodes;
        let bodies = [
            ("server", nodes.server),
            ("block", nodes.block),
            ("database", nodes.database),
            ("load_balancer", nodes.load_balancer),
            ("kubernetes", nodes.kubernetes),
            ("api_gateway", nodes.api_gateway),
            ("device", nodes.device),
            ("person", nodes.person),
        ];
        for floor in rendered_zone_floors(&theme).into_iter().chain([theme.background]) {
            for (name, body) in bodies {
                let ratio = contrast_ratio(body, floor);
                assert!(ratio >= MIN_BODY_ON_ZONE, "{name} {body} on {floor}: {ratio:.2}");
            }
        }
        let outline = contrast_ratio(theme.stroke, theme.background);
        assert!(outline >= MIN_OUTLINE, "outline {outline:.2}");
        let leader = contrast_ratio(theme.label.leader, theme.background);
        assert!(leader >= MIN_LEADER, "leader {leader:.2}");
        let border = contrast_ratio(theme.label.border, theme.background);
        assert!(border >= MIN_CHIP_BORDER, "chip border {border:.2}");
        let text = contrast_ratio(theme.label.text, theme.label.background);
        assert!(text >= MIN_TEXT, "label text {text:.2}");
    }

    #[test]
    fn classic_themes_keep_the_classic_drawing() {
        for name in [ThemeName::Blueprint, ThemeName::Midnight, ThemeName::Paper, ThemeName::Mono] {
            let theme = Theme::builtin(name);
            assert_eq!(theme.edge_drawing, CLASSIC_EDGES, "{name:?}");
            assert_eq!(theme.label_leader, LeaderStyle::Solid);
            assert!(theme.fills_zones());
            assert_eq!(theme.icon_finish, IconFinish::Shaded);
            assert_eq!(theme.default_layout, None, "classic themes keep the spec default (flow)");
            assert_eq!(theme.pill.border, None);
            assert_eq!(theme.nodes.mark, theme.nodes.paper, "glyphs stay paper-white on colored bodies");
            assert_eq!(theme.edge_colors(None), EdgeColors::Style);
        }
    }

    #[test]
    fn schematic_tokens_draw_the_reference() {
        let theme = Theme::schematic();
        let drawing = theme.edge_drawing;
        assert_eq!(drawing.arrow_position, ArrowPosition::Middle);
        assert_eq!(drawing.dash, DashPattern::Dots);
        assert_eq!(drawing.default_routing, Some(Routing::Rails));
        assert_eq!(theme.default_layout, Some(LayoutAlgorithm::Hub));
        assert_eq!(theme.edge_colors(None), EdgeColors::Semantic);
        assert_eq!(theme.edge_colors(Some(EdgeColors::Style)), EdgeColors::Style, "the spec wins");
        assert!(!theme.fills_zones());
        assert_eq!(theme.label_leader, LeaderStyle::Dotted);
        assert!(theme.edge_width >= 4.0, "thick connections");
        let arrow = drawing.arrow_color.unwrap_or(Color::WHITE);
        assert!(relative_luminance(arrow) < 0.02, "black arrow heads");
        // Silver bodies: light, with a darker outline.
        for body in [theme.nodes.server, theme.nodes.database, theme.nodes.device, theme.nodes.load_balancer] {
            assert!(relative_luminance(body) > 0.6, "{body} is not silver");
            assert!(contrast_ratio(body, theme.stroke) >= 2.5, "{body} outline");
        }
    }

    #[test]
    fn semantic_palettes_are_distinct_and_readable() {
        // WCAG 1.4.11 asks 3:1 of graphics; the classic themes reuse their zone strokes (amber ≈ 2.6).
        const MIN_SCHEMATIC: f64 = 3.0;
        const MIN_CLASSIC: f64 = 2.5;
        for name in ThemeName::ALL {
            let theme = Theme::builtin(name);
            let minimum = if name == ThemeName::Schematic { MIN_SCHEMATIC } else { MIN_CLASSIC };
            let looks: Vec<KindStyle> = EdgeKind::ALL.iter().map(|kind| theme.edges.kinds.get(*kind)).collect();
            if name != ThemeName::Mono {
                for (index, look) in looks.iter().enumerate() {
                    assert!(
                        looks[index + 1..].iter().all(|other| other.color != look.color),
                        "{name:?} repeats a color"
                    );
                    let ratio = contrast_ratio(look.color, theme.background);
                    assert!(ratio >= minimum, "{name:?} {} on page: {ratio:.2}", look.color);
                }
            }
            // Messaging also differs by stroke, not just by hue (color-blind safe).
            assert_eq!(theme.edges.kinds.messaging.line, LinePattern::Dashed, "{name:?}");
        }
    }

    #[test]
    fn schematic_pills_and_labels_read() {
        let theme = Theme::schematic();
        assert!(contrast_ratio(theme.pill.text, theme.pill.background) >= 7.0);
        assert!(contrast_ratio(theme.label.text, theme.label.background) >= 7.0);
        assert!(theme.pill.border.is_some(), "a white pill needs a hairline on a light page");
    }

    #[test]
    fn unset_zone_color_rotates_palette() {
        let theme = Theme::blueprint();
        assert_eq!(theme.zone_colors(None, 0), theme.zones.blue);
        assert_eq!(theme.zone_colors(None, 1), theme.zones.orange);
        assert_eq!(theme.zone_colors(None, 8), theme.zones.blue);
    }
}
