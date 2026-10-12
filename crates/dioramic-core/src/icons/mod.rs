//! Icon catalog, search and procedural drawing (`docs/06-renderizacao.md#ícones`).
//! Contract: `docs/work/contracts/C-003-icons.md`. Implementation: T-102.
//!
//! Every icon is drawn in the (already rotated) world through [`IsoCanvas`], and returns its
//! [`NodeShape`] (footprint for arrow ports, height for label anchors, screen bbox for collisions).
//! Details (LEDs, ports, logos) always face the camera ("billboard"): the drawer receives the camera
//! [`Rotation`] only to swap non-square footprints and keep details on visible faces.

mod catalog;
pub mod connection;
mod draw;
mod drawers;
pub mod flat;
pub mod motion;
mod primitives;
mod search;

use dioramic_spec::{Diagnostic, DiagnosticCode, Diagnostics, Model, Node, NodeSize, Rotation};

use crate::color::Color;
use crate::geometry::{PlanPoint, Rect};
use crate::theme::Theme;

pub use connection::edge_kinds;
pub(crate) use draw::floating_outline;
pub use draw::{contact_sheet_svg, draw, metrics, replica_offset};
pub use flat::{FlatIcon, FlatIconRef, IconFilter, IconPackInfo, IconRegistry, Mount};
pub use primitives::{badge, cylinder, iso_box, shadow};
pub use search::{infer_from_node_id, search, suggest};

/// Height (tiles) from which a node counts as "tall": arrows cannot end behind it.
pub const TALL_HEIGHT: f64 = 0.5;

/// Icon category (`search_icons.category`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IconCategory {
    /// Servers, functions, containers.
    Compute,
    /// Switches, routers, load balancers, gateways, CDN, DNS.
    Network,
    /// Firewalls.
    Security,
    /// Databases, caches, storage.
    Data,
    /// Queues, streams, brokers.
    Messaging,
    /// Internet / external providers.
    Cloud,
    /// Users and end-user devices.
    Clients,
    /// Office/branch devices (printer, phone, ATM).
    Devices,
    /// Kubernetes.
    Kubernetes,
    /// Monitoring, metrics, logs.
    Observability,
    /// Generic block.
    Generic,
}

impl IconCategory {
    /// All categories in catalog order.
    pub const ALL: [IconCategory; 11] = [
        Self::Compute,
        Self::Network,
        Self::Security,
        Self::Data,
        Self::Messaging,
        Self::Cloud,
        Self::Clients,
        Self::Devices,
        Self::Kubernetes,
        Self::Observability,
        Self::Generic,
    ];

    /// Stable lowercase name (`k8s` for Kubernetes, as in `docs/05-mcp.md`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compute => "compute",
            Self::Network => "network",
            Self::Security => "security",
            Self::Data => "data",
            Self::Messaging => "messaging",
            Self::Cloud => "cloud",
            Self::Clients => "clients",
            Self::Devices => "devices",
            Self::Kubernetes => "k8s",
            Self::Observability => "observability",
            Self::Generic => "generic",
        }
    }

    /// Parses [`IconCategory::as_str`] (case-insensitive; `kubernetes` also accepted).
    pub fn parse(text: &str) -> Option<Self> {
        let lower = text.trim().to_ascii_lowercase();
        if lower == "kubernetes" {
            return Some(Self::Kubernetes);
        }
        Self::ALL.into_iter().find(|category| category.as_str() == lower)
    }
}

/// Procedural icons of v1. Each maps to a drawer built from primitives (box, slab, stack, cylinder,
/// tube, dome, cloud, person, screen, prism).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProceduralIcon {
    /// Generic block (fallback).
    Block,
    /// Stack of 3 boxes with LEDs.
    Server,
    /// Tall cabinet with units.
    Rack,
    /// Slab with ports.
    Switch,
    /// Slab with antennas / arrows glyph.
    Router,
    /// Wide slab with ports (prototype `lb`).
    LoadBalancer,
    /// Brick wall block.
    Firewall,
    /// Dome on a puck.
    AccessPoint,
    /// Stack of 3 cylinders.
    Database,
    /// Two red cylinders.
    Cache,
    /// Object-storage bucket (S3, blob).
    Storage,
    /// Disk appliance (NAS / SAN): low chassis with drive bays.
    Nas,
    /// Lying tube in segments.
    Queue,
    /// Puffs with unified outline.
    Cloud,
    /// Group of persons.
    Users,
    /// Capsule + sphere.
    Person,
    /// Slab + monitor.
    Workstation,
    /// Open laptop.
    Laptop,
    /// Phone standing.
    Mobile,
    /// Desk phone.
    VoipPhone,
    /// Printer.
    Printer,
    /// ATM kiosk.
    Atm,
    /// Hexagonal prism with λ.
    Function,
    /// Box with container ribs.
    Container,
    /// Gate / arch slab.
    ApiGateway,
    /// Globe on slab.
    Cdn,
    /// Signpost / globe with "DNS".
    Dns,
    /// Heptagon platform.
    K8sCluster,
    /// Small hexagonal prism.
    Pod,
    /// Screen with chart.
    Monitoring,
}

/// How an icon is produced: procedural (built-in catalog) or flat (a provider/project SVG projected onto
/// an isometric base, see [`flat`]). Isometric (Isoflow) icons may come later without breaking users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IconKind {
    /// Drawn from primitives.
    Procedural(ProceduralIcon),
    /// A sanitized 2D SVG on a base (`mount: top | stand`).
    Flat(FlatIconRef),
}

impl IconKind {
    /// `"procedural" | "isometric" | "flat"` (MCP `search_icons.kind`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Procedural(_) => "procedural",
            Self::Flat(_) => "flat",
        }
    }
}

/// One catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IconEntry {
    /// Canonical id (`database`, `k8s-cluster`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Aliases (`postgres`, `pg`).
    pub aliases: &'static [&'static str],
    /// Category.
    pub category: IconCategory,
    /// Kind.
    pub kind: IconKind,
}

/// Floor footprint of a node (tiles), around its center.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Footprint {
    /// Axis-aligned rectangle (already swapped for odd rotations).
    Rect {
        /// Half extent along x.
        half_width: f64,
        /// Half extent along y.
        half_depth: f64,
    },
    /// Circle.
    Circle {
        /// Radius.
        radius: f64,
    },
}

impl Footprint {
    /// Half extents `(x, y)` of the bounding square/rect.
    pub fn half_extents(&self) -> (f64, f64) {
        match *self {
            Self::Rect { half_width, half_depth } => (half_width, half_depth),
            Self::Circle { radius } => (radius, radius),
        }
    }
}

/// Footprint and height, known before drawing (used by layout).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconMetrics {
    /// Footprint.
    pub footprint: Footprint,
    /// Height in tiles (top of the body; label leader starts here).
    pub height: f64,
}

impl IconMetrics {
    /// Tall nodes hide what is behind them; arrows must use a visible port.
    pub fn is_tall(&self) -> bool {
        self.height >= TALL_HEIGHT
    }
}

/// What the scene asks the drawer to draw.
#[derive(Debug, Clone, Copy)]
pub struct IconDraw<'a> {
    /// Center on the plan (rotated world).
    pub center: PlanPoint,
    /// Elevation (tiles).
    pub z: f64,
    /// Camera rotation (swap footprints, keep details on visible faces).
    pub rotation: Rotation,
    /// Scale.
    pub size: NodeSize,
    /// Tint from `nodes[].color` (resolved by the theme).
    pub tint: Option<Color>,
    /// Replicas (≥ 1): a deck of copies along a grid line plus a `×N` badge.
    pub replicas: u32,
    /// Theme.
    pub theme: &'a Theme,
    /// Wrap the moving parts (LEDs, waves, rings) in motion hooks ([`motion`], T-402). Hooks are resolved by
    /// the SVG writer: stripped in a static SVG, evaluated at `t` in frames, SMIL in the animated SVG.
    pub animate: bool,
}

/// Result of drawing a node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeShape {
    /// Footprint (rotated world).
    pub footprint: Footprint,
    /// Height in tiles.
    pub height: f64,
    /// Screen bounding box of everything drawn for the node (shadow, replicas, badge included).
    pub screen_bbox: Rect,
}

impl NodeShape {
    /// See [`IconMetrics::is_tall`].
    pub fn is_tall(&self) -> bool {
        self.height >= TALL_HEIGHT
    }
}

/// A search result with its relevance score (higher is better).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchHit {
    /// Entry.
    pub entry: &'static IconEntry,
    /// Score in `0.0..=1.0`.
    pub score: f64,
}

/// The whole catalog.
pub fn catalog() -> &'static [IconEntry] {
    catalog::CATALOG
}

/// The fallback icon (`block`).
pub fn block() -> &'static IconEntry {
    catalog().iter().find(|entry| entry.id == "block").unwrap_or(&catalog::CATALOG[catalog::CATALOG.len() - 1])
}

/// Exact lookup by id or alias, case-insensitive (`Postgres` → `database`).
pub fn lookup(id_or_alias: &str) -> Option<&'static IconEntry> {
    let wanted = id_or_alias.trim().to_ascii_lowercase();
    catalog().iter().find(|entry| entry.id == wanted || entry.aliases.contains(&wanted.as_str()))
}

/// Resolves the icon of a node against the built-in catalog only (see [`resolve_with`]).
pub fn resolve(node: &Node) -> Result<&'static IconEntry, Diagnostic> {
    resolve_with(node, IconRegistry::empty()).map(|resolution| resolution.entry)
}

/// A resolved icon and, when a provider pack is missing, the warning explaining the fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    /// Icon to draw.
    pub entry: &'static IconEntry,
    /// `W_ICON_PACK_MISSING` when `entry` stands in for an icon of a pack that is not installed.
    pub warning: Option<Diagnostic>,
}

/// Resolves the icon of a node: explicit id/alias (flat icons of `registry` first, then the built-in
/// catalog), else inferred from the node id, else `block`.
///
/// An icon of a provider pack that is not installed (`aws/lambda` without the `aws` pack) falls back to
/// the closest built-in icon with a `W_ICON_PACK_MISSING` warning whose hint is the install command.
/// Any other unknown explicit icon yields `E_UNKNOWN_ICON` at `nodes[i].icon` with a suggestion.
pub fn resolve_with(node: &Node, registry: &IconRegistry) -> Result<Resolution, Diagnostic> {
    let Some(icon) = node.icon.as_deref() else {
        return Ok(Resolution { entry: infer_from_node_id(node.id.as_str()).unwrap_or_else(block), warning: None });
    };
    if let Some(entry) = registry.lookup(icon) {
        return Ok(Resolution { entry, warning: None });
    }
    let path = format!("{}.icon", node.path);
    match flat::split_pack_id(icon) {
        Some((pack, name)) if !registry.has_pack(pack) && is_provider_pack(pack) => {
            let entry = fallback_for(name);
            let warning = Diagnostic::new(
                DiagnosticCode::IconPackMissing,
                path,
                format!("icon '{icon}' needs the '{pack}' icon pack, which is not installed; drawn as '{}'", entry.id),
            )
            .with_hint(format!("run `dioramic icons install {pack}`, then render again"));
            Ok(Resolution { entry, warning: Some(warning) })
        }
        Some((pack, _)) if registry.has_pack(pack) => Err(Diagnostic::new(
            DiagnosticCode::UnknownIcon,
            path,
            format!("icon '{icon}' does not exist in the '{pack}' pack"),
        )
        .with_hint(match registry.suggest_in_pack(pack, icon) {
            Some(suggestion) => format!("did you mean '{suggestion}'? Otherwise call search_icons with pack '{pack}'"),
            None => format!("call search_icons (or `dioramic icons search`) with pack '{pack}'"),
        })),
        _ => Err(Diagnostic::new(DiagnosticCode::UnknownIcon, path, format!("icon '{icon}' does not exist"))
            .with_hint(unknown_icon_hint(icon))),
    }
}

fn is_provider_pack(pack: &str) -> bool {
    flat::PROVIDER_PACKS.iter().any(|(id, _)| id.eq_ignore_ascii_case(pack))
}

/// Built-in stand-in for a provider icon: inferred from its name (`lambda` → function,
/// `simple-queue-service` → queue), else the best search hit, else `block`.
fn fallback_for(name: &str) -> &'static IconEntry {
    /// Search score a stand-in must reach (a plain alias/name match).
    const MIN_FALLBACK_SCORE: f64 = 0.5;
    let generic = |entry: &&'static IconEntry| entry.category != IconCategory::Generic;
    // Generic words (`service`, `component`) infer `block`; a more specific word of the name wins.
    let specific_word = || name.rsplit(['-', '_', '/']).filter_map(infer_from_node_id).find(generic);
    infer_from_node_id(name)
        .filter(generic)
        .or_else(specific_word)
        .or_else(|| {
            search(&name.replace(['-', '/'], " "), None, 1)
                .first()
                .filter(|hit| hit.score >= MIN_FALLBACK_SCORE)
                .map(|hit| hit.entry)
        })
        .unwrap_or_else(block)
}

fn unknown_icon_hint(icon: &str) -> String {
    const FALLBACK: &str = "call search_icons, or use icon: block with a color and label";
    match suggest(icon) {
        Some(suggestion) => format!("did you mean '{suggestion}'? Otherwise {FALLBACK}"),
        None => FALLBACK.to_owned(),
    }
}

/// Runs [`resolve`] on every node and collects the `E_UNKNOWN_ICON` errors.
pub fn validate_icons(model: &Model) -> Diagnostics {
    validate_icons_with(model, IconRegistry::empty())
}

/// Runs [`resolve_with`] on every node and collects its errors and warnings.
pub fn validate_icons_with(model: &Model, registry: &IconRegistry) -> Diagnostics {
    model
        .nodes
        .iter()
        .filter_map(|node| match resolve_with(node, registry) {
            Ok(resolution) => resolution.warning,
            Err(error) => Some(error),
        })
        .collect::<Vec<_>>()
        .into()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn icons_used_by_examples_resolve() {
        // Ícones usados em examples/*.diorama.yaml (T-000).
        for name in [
            "storage",
            "nas",
            "server",
            "pod",
            "workstation",
            "users",
            "switch",
            "redis",
            "queue",
            "postgres",
            "monitor",
            "loadbalancer",
            "mobile",
            "function",
            "database",
            "container",
            "cloud",
            "atm",
            "api-gateway",
            "access-point",
            "voip-phone",
            "rack",
            "printer",
            "k8s-cluster",
            "firewall",
            "dns",
            "cdn",
            "k8s/pod",
        ] {
            assert!(lookup(name).is_some(), "icon '{name}' should resolve");
        }
        assert_eq!(lookup("Postgres").map(|entry| entry.id), Some("database"));
        assert_eq!(block().id, "block");
    }

    #[test]
    fn categories_round_trip() {
        for category in IconCategory::ALL {
            assert_eq!(IconCategory::parse(category.as_str()), Some(category));
        }
        assert!(catalog().iter().all(|entry| IconCategory::ALL.contains(&entry.category)));
    }

    fn node(icon: &str) -> Node {
        Node {
            id: dioramic_spec::NodeId::from("fn_1"),
            icon: Some(icon.to_owned()),
            label: None,
            zone: None,
            color: None,
            replicas: 1,
            badge: None,
            at: None,
            z: 0.0,
            size: NodeSize::M,
            note: None,
            path: "nodes[0]".to_owned(),
        }
    }

    #[test]
    fn flat_icons_resolve_through_the_registry() {
        let registry = flat::fixtures::registry();
        let resolution = resolve_with(&node("testaws/lambda"), &registry).unwrap();
        assert_eq!(resolution.entry.id, "testaws/lambda");
        assert!(resolution.warning.is_none());
        // Built-ins keep working with a registry, and without one the flat id is unknown.
        assert_eq!(resolve_with(&node("postgres"), &registry).unwrap().entry.id, "database");
        assert_eq!(resolve(&node("testaws/lambda")).unwrap_err().code, DiagnosticCode::UnknownIcon);
    }

    #[test]
    fn missing_provider_pack_falls_back_with_a_warning() {
        let resolution = resolve_with(&node("aws/lambda"), IconRegistry::empty()).unwrap();
        assert_eq!(resolution.entry.id, "function");
        let warning = resolution.warning.unwrap();
        assert_eq!(warning.code, DiagnosticCode::IconPackMissing);
        assert_eq!(warning.path, "nodes[0].icon");
        assert!(warning.hint.as_deref().is_some_and(|hint| hint.contains("dioramic icons install aws")));
        assert_eq!(resolve_with(&node("aws/simple-queue-service"), IconRegistry::empty()).unwrap().entry.id, "queue");
        assert_eq!(resolve_with(&node("gcp/zzzz"), IconRegistry::empty()).unwrap().entry.id, "block");
        // The built-in alias still wins when the k8s pack is not installed.
        let pod = resolve_with(&node("k8s/pod"), IconRegistry::empty()).unwrap();
        assert_eq!((pod.entry.id, pod.warning), ("pod", None));
    }

    #[test]
    fn unknown_icons_in_installed_or_unknown_packs_are_errors() {
        let registry = flat::fixtures::registry();
        let error = resolve_with(&node("testaws/lamda"), &registry).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::UnknownIcon);
        assert!(error.hint.as_deref().is_some_and(|hint| hint.contains("testaws/lambda")), "{error:?}");
        let error = resolve_with(&node("ibm/db2"), &registry).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::UnknownIcon);
        let spec = "diorama: 1\nnodes:\n  - { id: a, icon: aws/lambda }\n  - { id: b, icon: nope }\n";
        let model = dioramic_spec::load(spec, dioramic_spec::SourceFormat::Yaml).unwrap().model;
        let findings = validate_icons_with(&model, &registry);
        assert_eq!(findings.len(), 2);
        assert!(findings.has_errors());
    }
}
