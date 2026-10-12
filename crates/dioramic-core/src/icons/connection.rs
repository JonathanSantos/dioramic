//! Connection classes ([`EdgeKind`]) inferred from the icons at both ends of an edge, its operator and its
//! label (`docs/06-renderizacao.md#cores-semânticas-de-conexão`). The class picks the edge color when the
//! theme (or `layout.edge_colors: semantic`) colors edges by meaning.
//!
//! Rules, first match wins:
//! 1. `kind:` written on the edge.
//! 2. A class name used as the style (`@data`).
//! 3. A *data-side* end: cache, data (database, storage, NAS), messaging (queue, stream, bus) or
//!    observability. The target is asked first: `api -> db` is data, `db -> metrics` is observability.
//! 4. An async link (`..>`, `..`, `dashed: true`) between ends without such a class: messaging.
//! 5. A keyword of the label (`SQL`, `GET/SET`, `publish`, `metrics`, `HTTPS`…).
//! 6. A client end (users, laptop, mobile…): client.
//! 7. An external end (cloud, internet, CDN, DNS): external.
//! 8. Otherwise: service.

use dioramic_spec::{Edge, EdgeKind, Model};

use super::{IconCategory, IconEntry, IconRegistry, resolve_with};

/// Words of an icon id that mark a cache among the data icons (built-in `cache`, `aws/elasticache`).
const CACHE_WORDS: [&str; 4] = ["cache", "redis", "memcache", "valkey"];
/// Built-in network icons that stand for the outside world.
const EXTERNAL_ICONS: [&str; 2] = ["cdn", "dns"];

/// Label keywords per class, matched as whole words, case-insensitive. Checked in this order.
const LABEL_KEYWORDS: [(EdgeKind, &[&str]); 5] = [
    (EdgeKind::Data, &["sql", "jdbc", "odbc", "query", "queries", "crud", "orm", "replication", "replica"]),
    (EdgeKind::Cache, &["get/set", "redis", "memcached", "cache", "cached", "lookup"]),
    (
        EdgeKind::Messaging,
        &[
            "publish",
            "subscribe",
            "pub/sub",
            "enqueue",
            "dequeue",
            "consume",
            "produce",
            "emit",
            "event",
            "events",
            "async",
        ],
    ),
    (EdgeKind::Observability, &["metrics", "logs", "traces", "telemetry", "scrape", "otlp"]),
    (EdgeKind::External, &["https", "internet", "webhook", "webhooks", "third-party"]),
];

/// The class of every edge of `model`, in edge order, resolving icons against `registry`.
pub fn edge_kinds(model: &Model, registry: &IconRegistry) -> Vec<EdgeKind> {
    let icon_of = |id| model.node(id).and_then(|node| resolve_with(node, registry).ok()).map(|found| found.entry);
    model.edges.iter().map(|edge| classify(edge, icon_of(&edge.from), icon_of(&edge.to))).collect()
}

/// The class of one edge given the icons of its source and target (`None` = unknown node).
pub fn classify(edge: &Edge, source: Option<&IconEntry>, target: Option<&IconEntry>) -> EdgeKind {
    let ends = [target, source];
    edge.kind
        .or_else(|| class_named(&edge.style))
        .or_else(|| ends.iter().flatten().find_map(|entry| data_side(entry)))
        .or_else(|| (edge.dashed == Some(true)).then_some(EdgeKind::Messaging))
        .or_else(|| edge.label.as_deref().and_then(|label| from_label(label, source)))
        .or_else(|| {
            ends.iter().flatten().any(|entry| entry.category == IconCategory::Clients).then_some(EdgeKind::Client)
        })
        .or_else(|| ends.iter().flatten().any(|entry| is_external(entry)).then_some(EdgeKind::External))
        .unwrap_or(EdgeKind::Service)
}

/// `@data`, `@cache`… as a style name.
fn class_named(style: &str) -> Option<EdgeKind> {
    EdgeKind::ALL.into_iter().find(|kind| kind.as_str() == style)
}

/// The class an end imposes on any edge touching it, if it is on the data side of the system.
fn data_side(entry: &IconEntry) -> Option<EdgeKind> {
    match entry.category {
        IconCategory::Data if is_cache(entry) => Some(EdgeKind::Cache),
        IconCategory::Data => Some(EdgeKind::Data),
        IconCategory::Messaging => Some(EdgeKind::Messaging),
        IconCategory::Observability => Some(EdgeKind::Observability),
        _ => None,
    }
}

fn is_cache(entry: &IconEntry) -> bool {
    let id = entry.id.to_ascii_lowercase();
    CACHE_WORDS.iter().any(|word| id.contains(word))
}

fn is_external(entry: &IconEntry) -> bool {
    entry.category == IconCategory::Cloud || EXTERNAL_ICONS.contains(&entry.id)
}

/// Class from the label's keywords. `HTTPS` from a client is the client's own call, not the outside world.
fn from_label(label: &str, source: Option<&IconEntry>) -> Option<EdgeKind> {
    let lowered = label.to_ascii_lowercase();
    let words: Vec<&str> = lowered
        .split(|character: char| character.is_whitespace() || matches!(character, ',' | ';' | '(' | ')' | ':'))
        .filter(|word| !word.is_empty())
        .collect();
    let kind = LABEL_KEYWORDS
        .iter()
        .find(|(_, keywords)| keywords.iter().any(|keyword| words.contains(keyword)))
        .map(|(kind, _)| *kind)?;
    let from_client = source.is_some_and(|entry| entry.category == IconCategory::Clients);
    Some(if kind == EdgeKind::External && from_client { EdgeKind::Client } else { kind })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::lookup;
    use dioramic_spec::{ArrowEnds, EdgeId, FlowMode, NodeId, Routing, ValueSource};

    fn edge(label: Option<&str>) -> Edge {
        Edge {
            id: EdgeId::from("a->b"),
            from: NodeId::from("a"),
            to: NodeId::from("b"),
            label: label.map(str::to_owned),
            style: "primary".to_owned(),
            style_source: ValueSource::Default,
            kind: None,
            color: None,
            width: None,
            dashed: None,
            arrow: ArrowEnds::End,
            route: Routing::Curved,
            route_source: ValueSource::Default,
            flow: FlowMode::Off,
            path: "edges[0]".to_owned(),
        }
    }

    fn kind_between(source: &str, target: &str) -> EdgeKind {
        classify(&edge(None), lookup(source), lookup(target))
    }

    #[test]
    fn ends_on_the_data_side_set_the_class() {
        assert_eq!(kind_between("server", "postgres"), EdgeKind::Data);
        assert_eq!(kind_between("server", "s3"), EdgeKind::Data);
        assert_eq!(kind_between("server", "nas"), EdgeKind::Data);
        assert_eq!(kind_between("server", "redis"), EdgeKind::Cache);
        assert_eq!(kind_between("queue", "function"), EdgeKind::Messaging);
        assert_eq!(kind_between("server", "prometheus"), EdgeKind::Observability);
        // The target is asked first.
        assert_eq!(kind_between("database", "monitoring"), EdgeKind::Observability);
        assert_eq!(kind_between("cache", "database"), EdgeKind::Data);
    }

    #[test]
    fn edge_ends_toward_the_user_are_client_or_external() {
        assert_eq!(kind_between("laptop", "loadbalancer"), EdgeKind::Client);
        assert_eq!(kind_between("mobile", "cdn"), EdgeKind::Client, "a client end wins over an external one");
        assert_eq!(kind_between("internet", "loadbalancer"), EdgeKind::External);
        assert_eq!(kind_between("cdn", "server"), EdgeKind::External);
        assert_eq!(kind_between("api-gateway", "server"), EdgeKind::Service);
    }

    #[test]
    fn explicit_kind_and_class_style_win() {
        let mut written = edge(None);
        written.kind = Some(EdgeKind::External);
        assert_eq!(classify(&written, lookup("server"), lookup("database")), EdgeKind::External);
        let mut styled = edge(None);
        styled.style = "cache".to_owned();
        assert_eq!(classify(&styled, lookup("server"), lookup("server")), EdgeKind::Cache);
    }

    #[test]
    fn async_links_and_labels_refine_service_edges() {
        let mut dashed = edge(None);
        dashed.dashed = Some(true);
        assert_eq!(classify(&dashed, lookup("server"), lookup("function")), EdgeKind::Messaging);
        assert_eq!(classify(&dashed, lookup("server"), lookup("database")), EdgeKind::Data, "the data end is stronger");
        let labeled = |text: &str, source: &str| classify(&edge(Some(text)), lookup(source), lookup("server"));
        assert_eq!(labeled("SQL", "server"), EdgeKind::Data);
        assert_eq!(labeled("GET/SET", "server"), EdgeKind::Cache);
        assert_eq!(labeled("publish OrderPlaced", "server"), EdgeKind::Messaging);
        assert_eq!(labeled("metrics", "server"), EdgeKind::Observability);
        assert_eq!(labeled("HTTPS", "server"), EdgeKind::External);
        assert_eq!(labeled("HTTPS", "mobile"), EdgeKind::Client);
        assert_eq!(labeled("gRPC", "server"), EdgeKind::Service);
        assert_eq!(labeled("preprocessing", "server"), EdgeKind::Service, "whole words only");
    }

    #[test]
    fn unknown_ends_are_service() {
        assert_eq!(classify(&edge(None), None, None), EdgeKind::Service);
    }
}
