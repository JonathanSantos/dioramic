//! Flat icons (ADR-006, layer 3): any 2D SVG projected onto an isometric base (`docs/06-renderizacao.md#ícones`).
//!
//! The core never touches the disk: callers (CLI, MCP, desktop) load provider packs and project-local icons
//! and hand the SVG text to an [`IconRegistry`], which sanitizes it and turns each icon into an
//! [`IconEntry`](super::IconEntry) with [`IconKind::Flat`](super::IconKind::Flat). The registry travels in
//! [`CompileOptions`](crate::CompileOptions).
//!
//! Entries are interned for the life of the process (see [`intern`]): the rest of the engine keeps using
//! `&'static IconEntry`, and loading the same icon twice (an MCP server re-reading a pack) reuses the
//! first allocation, so memory grows only with the number of *distinct* icons ever seen.

mod draw;
mod sanitize;

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use draw::{draw_flat, flat_metrics};
pub use sanitize::{MAX_SVG_BYTES, SanitizedSvg, ViewBox, sanitize_svg};

use super::{IconCategory, IconEntry, IconKind, SearchHit};

/// Packs the CLI knows how to install (`dioramic icons install <id>`): `(id, display name)`.
pub const PROVIDER_PACKS: [(&str, &str); 4] =
    [("aws", "AWS"), ("gcp", "Google Cloud"), ("azure", "Azure"), ("k8s", "Kubernetes")];

/// Pack id of icons found in the project's `icons/` directory.
pub const LOCAL_PACK: &str = "local";

/// How a flat icon sits on its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mount {
    /// Lying on the top of a low block (floor matrix). Default.
    #[default]
    Top,
    /// Standing upright on the front visible face of a thin panel.
    Stand,
}

impl Mount {
    /// `"top"` / `"stand"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Stand => "stand",
        }
    }
}

/// Why an icon could not be registered.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FlatIconError {
    /// Not well-formed XML.
    #[error("invalid SVG: {0}")]
    Xml(String),
    /// The root element is not `<svg>`.
    #[error("the root element is not <svg>")]
    NotSvg,
    /// Neither `viewBox` nor `width`/`height`.
    #[error("the SVG has no viewBox (nor width/height)")]
    NoViewBox,
    /// Too large to be an icon.
    #[error("the SVG has {bytes} bytes, the maximum is {limit}")]
    TooLarge {
        /// Size of the source.
        bytes: usize,
        /// Limit.
        limit: usize,
    },
    /// Pack or icon id is not `[a-z0-9-]` (icons may also use `/`).
    #[error("invalid id '{0}': use lowercase letters, digits and '-'")]
    BadId(String),
}

/// A pack as described by its manifest (`manifest.json`, see `crates/dioramic-packs`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IconPackInfo {
    /// Id and id prefix (`aws` → `aws/lambda`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// License or terms of use, as stated by the provider.
    pub license: String,
    /// Where the files came from (URL or local path).
    pub source: String,
    /// Provider release (`2026-07-31`, `V24`).
    pub version: String,
}

/// One icon to register: already-loaded SVG text plus its metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatIconSource {
    /// Pack id (`aws`).
    pub pack: String,
    /// Id inside the pack (`lambda`); the full id is `aws/lambda`.
    pub id: String,
    /// Display name (`AWS Lambda`).
    pub name: String,
    /// Extra ids inside the pack (`aws-lambda`) → `aws/aws-lambda`.
    pub aliases: Vec<String>,
    /// Provider category (`Compute`, `Databases`).
    pub group: Option<String>,
    /// Mount.
    pub mount: Mount,
    /// SVG source.
    pub svg: String,
}

/// A registered flat icon.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatIcon {
    /// Pack id.
    pub pack: &'static str,
    /// Provider category, if any.
    pub group: Option<&'static str>,
    /// Mount.
    pub mount: Mount,
    /// Sanitized SVG.
    pub svg: SanitizedSvg,
}

/// Reference to an interned flat icon. Equality and hashing are by identity: interning guarantees one
/// allocation per distinct icon.
#[derive(Debug, Clone, Copy)]
pub struct FlatIconRef(pub &'static FlatIcon);

impl PartialEq for FlatIconRef {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}

impl Eq for FlatIconRef {}

impl Hash for FlatIconRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(self.0, state);
    }
}

impl std::ops::Deref for FlatIconRef {
    type Target = FlatIcon;

    fn deref(&self) -> &FlatIcon {
        self.0
    }
}

/// Flat icons available to a compile: packs plus their entries. Cheap to clone (shared).
#[derive(Debug, Clone, Default)]
pub struct IconRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Debug, Clone, Default)]
struct RegistryInner {
    packs: Vec<IconPackInfo>,
    entries: Vec<&'static IconEntry>,
    /// Lowercase full id or `pack/alias` → index in `entries`.
    index: BTreeMap<String, usize>,
}

impl PartialEq for IconRegistry {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
            || (self.inner.packs == other.inner.packs
                && self.inner.entries.len() == other.inner.entries.len()
                && self.inner.entries.iter().zip(&other.inner.entries).all(|(a, b)| std::ptr::eq(*a, *b)))
    }
}

impl IconRegistry {
    /// Empty registry (built-in icons only).
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared empty registry.
    pub fn empty() -> &'static IconRegistry {
        static EMPTY: OnceLock<IconRegistry> = OnceLock::new();
        EMPTY.get_or_init(IconRegistry::new)
    }

    /// Adds (or replaces, by id) a pack description.
    pub fn add_pack(&mut self, pack: IconPackInfo) {
        let inner = Arc::make_mut(&mut self.inner);
        match inner.packs.iter_mut().find(|known| known.id == pack.id) {
            Some(known) => *known = pack,
            None => inner.packs.push(pack),
        }
    }

    /// Sanitizes and registers an icon. A later icon with the same full id replaces the earlier one
    /// (project-local packs override installed ones). Aliases never steal an existing id.
    pub fn add_icon(&mut self, source: FlatIconSource) -> Result<&'static IconEntry, FlatIconError> {
        check_id(&source.pack, false)?;
        check_id(&source.id, true)?;
        let svg = sanitize_svg(&source.svg)?;
        let entry = intern(&source, svg);
        let inner = Arc::make_mut(&mut self.inner);
        let position = match inner.index.get(entry.id) {
            Some(&existing) if inner.entries[existing].id == entry.id => {
                inner.entries[existing] = entry;
                existing
            }
            _ => {
                inner.entries.push(entry);
                inner.entries.len() - 1
            }
        };
        inner.index.insert(entry.id.to_owned(), position);
        for alias in entry.aliases {
            let taken = inner.index.get(*alias).is_some_and(|&other| inner.entries[other].id == *alias);
            if !taken {
                inner.index.insert((*alias).to_owned(), position);
            }
        }
        Ok(entry)
    }

    /// Packs in registration order.
    pub fn packs(&self) -> &[IconPackInfo] {
        &self.inner.packs
    }

    /// Whether a pack id is registered.
    pub fn has_pack(&self, pack: &str) -> bool {
        self.inner.packs.iter().any(|known| known.id.eq_ignore_ascii_case(pack))
    }

    /// Flat entries in registration order.
    pub fn entries(&self) -> &[&'static IconEntry] {
        &self.inner.entries
    }

    /// Whether no flat icon is registered.
    pub fn is_empty(&self) -> bool {
        self.inner.entries.is_empty()
    }

    /// Exact lookup by full id or `pack/alias`, case-insensitive. Built-in icons are not searched here.
    pub fn lookup_flat(&self, id: &str) -> Option<&'static IconEntry> {
        let wanted = id.trim().to_ascii_lowercase();
        self.inner.index.get(&wanted).map(|&position| self.inner.entries[position])
    }

    /// Exact lookup: flat icons first (`k8s/pod` from an installed pack), then the built-in catalog.
    pub fn lookup(&self, id_or_alias: &str) -> Option<&'static IconEntry> {
        self.lookup_flat(id_or_alias).or_else(|| super::lookup(id_or_alias))
    }

    /// Ranked search over the built-in catalog and the flat icons (`"aws lambda"`, `"bigquery"`).
    /// `filter` narrows by category and/or pack (`pack: "builtin"` keeps only the built-in catalog).
    pub fn search(&self, query: &str, filter: &IconFilter<'_>, limit: usize) -> Vec<SearchHit> {
        let builtin = filter.pack.is_none_or(|pack| pack.eq_ignore_ascii_case(BUILTIN_PACK));
        let mut hits = if builtin { super::search(query, filter.category, usize::MAX) } else { Vec::new() };
        let flat = self.inner.entries.iter().filter(|entry| filter.accepts(entry));
        hits.extend(super::search::rank(query, flat.copied()));
        // Stable: built-in catalog order first on ties, then registration order.
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit);
        hits
    }

    /// Every entry (built-in catalog, then flat icons) accepted by `filter`.
    pub fn list(&self, filter: &IconFilter<'_>) -> Vec<&'static IconEntry> {
        let builtin = filter.pack.is_none_or(|pack| pack.eq_ignore_ascii_case(BUILTIN_PACK));
        let catalog =
            super::catalog().iter().filter(|entry| builtin && filter.category.is_none_or(|c| c == entry.category));
        catalog.chain(self.inner.entries.iter().copied().filter(|entry| filter.accepts(entry))).collect()
    }

    /// "Did you mean" among the icons of one pack (`aws/lamda` → `aws/lambda`).
    pub fn suggest_in_pack(&self, pack: &str, unknown: &str) -> Option<&'static str> {
        let wanted = unknown.trim().to_ascii_lowercase();
        let budget = (wanted.chars().count() / 3).max(1);
        let prefix = format!("{}/", pack.to_ascii_lowercase());
        self.inner
            .index
            .keys()
            .filter(|name| name.starts_with(&prefix))
            .map(|name| (strsim::damerau_levenshtein(&wanted, name), name))
            .filter(|(distance, _)| *distance <= budget)
            .min_by_key(|(distance, _)| *distance)
            .and_then(|(_, name)| self.lookup_flat(name).map(|entry| entry.id))
    }
}

/// Pack name of the built-in catalog in filters (`search_icons pack: builtin`).
pub const BUILTIN_PACK: &str = "builtin";

/// Search/list filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IconFilter<'a> {
    /// Only this category.
    pub category: Option<IconCategory>,
    /// Only this pack (`aws`, `local`, or [`BUILTIN_PACK`]).
    pub pack: Option<&'a str>,
}

impl IconFilter<'_> {
    fn accepts(&self, entry: &IconEntry) -> bool {
        let pack_ok = self.pack.is_none_or(|pack| entry_pack(entry).is_some_and(|own| own.eq_ignore_ascii_case(pack)));
        pack_ok && self.category.is_none_or(|category| category == entry.category)
    }
}

/// Pack of an entry (`None` for built-in icons).
pub fn entry_pack(entry: &IconEntry) -> Option<&'static str> {
    match entry.kind {
        IconKind::Flat(flat) => Some(flat.0.pack),
        IconKind::Procedural(_) => None,
    }
}

/// Splits `aws/lambda` into `("aws", "lambda")`.
pub fn split_pack_id(id: &str) -> Option<(&str, &str)> {
    id.split_once('/').filter(|(pack, rest)| !pack.is_empty() && !rest.is_empty())
}

/// Category of a provider group (`Databases` → data, `Networking-Content-Delivery` → network).
pub fn category_of_group(pack: &str, group: Option<&str>) -> IconCategory {
    if pack.eq_ignore_ascii_case("k8s") {
        return IconCategory::Kubernetes;
    }
    let group = group.unwrap_or_default().to_ascii_lowercase();
    const RULES: [(&[&str], IconCategory); 9] = [
        (&["security", "identity", "firewall"], IconCategory::Security),
        (&["network", "content-delivery", "cdn", "dns"], IconCategory::Network),
        (&["database", "storage", "analytics", "data"], IconCategory::Data),
        (&["integration", "messaging", "queue", "event"], IconCategory::Messaging),
        (&["container", "kubernetes"], IconCategory::Kubernetes),
        (&["compute", "serverless", "app service", "functions"], IconCategory::Compute),
        (&["management", "monitor", "operations", "observability", "devops"], IconCategory::Observability),
        (&["end-user", "mobile", "web", "iot", "desktop", "identity"], IconCategory::Clients),
        (&["cloud", "hybrid", "general"], IconCategory::Cloud),
    ];
    RULES
        .iter()
        .find(|(keywords, _)| keywords.iter().any(|keyword| group.contains(keyword)))
        .map_or(IconCategory::Generic, |(_, category)| *category)
}

fn check_id(id: &str, allow_slash: bool) -> Result<(), FlatIconError> {
    let valid = !id.is_empty()
        && !id.starts_with(['-', '/'])
        && !id.ends_with(['-', '/'])
        && id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || (allow_slash && character == '/')
        });
    if valid { Ok(()) } else { Err(FlatIconError::BadId(id.to_owned())) }
}

/// Interning key: everything that defines an entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct InternKey {
    pack: String,
    id: String,
    name: String,
    aliases: Vec<String>,
    group: Option<String>,
    mount: Mount,
    symbol_id: String,
}

/// Returns the process-wide entry for this icon, allocating (and leaking) it only the first time.
///
/// Leaking is deliberate: the engine's layout and scene hold `&'static IconEntry`, and the set of distinct
/// icons a process can see is bounded by the packs on disk (a few thousand small entries at most).
fn intern(source: &FlatIconSource, svg: SanitizedSvg) -> &'static IconEntry {
    static POOL: OnceLock<Mutex<BTreeMap<InternKey, &'static IconEntry>>> = OnceLock::new();
    let key = InternKey {
        pack: source.pack.clone(),
        id: source.id.clone(),
        name: source.name.clone(),
        aliases: source.aliases.clone(),
        group: source.group.clone(),
        mount: source.mount,
        symbol_id: svg.symbol_id.clone(),
    };
    let mut pool = POOL.get_or_init(Mutex::default).lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(entry) = pool.get(&key) {
        return entry;
    }
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    let pack = leak(source.pack.clone());
    let group = source.group.clone().map(leak);
    let flat: &'static FlatIcon = Box::leak(Box::new(FlatIcon { pack, group, mount: source.mount, svg }));
    let aliases: Vec<&'static str> = source
        .aliases
        .iter()
        .filter(|alias| check_id(alias, true).is_ok() && **alias != source.id)
        .map(|alias| leak(format!("{}/{alias}", source.pack)))
        .collect();
    let entry: &'static IconEntry = Box::leak(Box::new(IconEntry {
        id: leak(format!("{}/{}", source.pack, source.id)),
        name: leak(source.name.clone()),
        aliases: Box::leak(aliases.into_boxed_slice()),
        category: category_of_group(&source.pack, source.group.as_deref()),
        kind: IconKind::Flat(FlatIconRef(flat)),
    }));
    pool.insert(key, entry);
    entry
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// A square, AWS-like icon with an orange backdrop.
    pub const SQUARE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 80"><g fill="#ED7100"><rect width="80" height="80"/></g><path d="M20 60 L40 20 L60 60 Z" fill="#fff"/></svg>"##;
    /// A transparent, Azure-like icon.
    pub const GLYPH_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 18 18"><circle cx="9" cy="9" r="8" fill="#0078d4"/></svg>"##;

    pub fn source(pack: &str, id: &str, svg: &str, mount: Mount) -> FlatIconSource {
        FlatIconSource {
            pack: pack.to_owned(),
            id: id.to_owned(),
            name: format!("Test {id}"),
            aliases: vec![format!("{id}-alias")],
            group: Some("Compute".to_owned()),
            mount,
            svg: svg.to_owned(),
        }
    }

    pub fn pack(id: &str) -> IconPackInfo {
        IconPackInfo {
            id: id.to_owned(),
            name: id.to_uppercase(),
            license: "test".to_owned(),
            source: "fixture".to_owned(),
            version: "1".to_owned(),
        }
    }

    /// Registry with `testaws/lambda` (top) and `testaws/panel` (stand).
    pub fn registry() -> IconRegistry {
        let mut registry = IconRegistry::new();
        registry.add_pack(pack("testaws"));
        let _ = registry.add_icon(source("testaws", "lambda", SQUARE_SVG, Mount::Top));
        let _ = registry.add_icon(source("testaws", "panel", GLYPH_SVG, Mount::Stand));
        registry
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn registers_looks_up_and_interns() {
        let registry = registry();
        let entry = registry.lookup("TestAWS/Lambda").unwrap();
        assert_eq!(entry.id, "testaws/lambda");
        assert_eq!(entry.aliases, ["testaws/lambda-alias"]);
        assert_eq!(registry.lookup("testaws/lambda-alias").map(|entry| entry.id), Some("testaws/lambda"));
        assert_eq!(entry.category, IconCategory::Compute);
        assert_eq!(entry.kind.as_str(), "flat");
        // Same content → same allocation.
        let again = super::fixtures::registry();
        assert!(std::ptr::eq(entry, again.lookup("testaws/lambda").unwrap()));
        assert_eq!(registry, again);
        // Built-ins still resolve through the registry.
        assert_eq!(registry.lookup("postgres").map(|entry| entry.id), Some("database"));
    }

    #[test]
    fn later_icons_replace_and_aliases_never_steal_ids() {
        let mut registry = registry();
        let mut replacement = source("testaws", "lambda", GLYPH_SVG, Mount::Stand);
        replacement.aliases = vec!["panel".to_owned()];
        registry.add_icon(replacement).unwrap();
        assert_eq!(registry.entries().len(), 2);
        let lambda = registry.lookup("testaws/lambda").unwrap();
        assert!(matches!(lambda.kind, IconKind::Flat(flat) if flat.mount == Mount::Stand));
        assert_eq!(registry.lookup("testaws/panel").map(|entry| entry.id), Some("testaws/panel"));
    }

    #[test]
    fn rejects_bad_ids_and_bad_svg() {
        let mut registry = IconRegistry::new();
        assert!(matches!(registry.add_icon(source("AWS", "x", SQUARE_SVG, Mount::Top)), Err(FlatIconError::BadId(_))));
        assert!(matches!(
            registry.add_icon(source("aws", "../x", SQUARE_SVG, Mount::Top)),
            Err(FlatIconError::BadId(_))
        ));
        assert!(matches!(registry.add_icon(source("aws", "x", "<nope/>", Mount::Top)), Err(FlatIconError::NotSvg)));
        assert!(registry.is_empty());
    }

    #[test]
    fn search_mixes_builtin_and_flat_icons() {
        let registry = registry();
        let top =
            |query: &str, filter: IconFilter<'_>| registry.search(query, &filter, 5).first().map(|hit| hit.entry.id);
        assert_eq!(top("testaws lambda", IconFilter::default()), Some("testaws/lambda"));
        assert_eq!(top("testaws/panel", IconFilter::default()), Some("testaws/panel"));
        assert_eq!(top("postgres", IconFilter::default()), Some("database"));
        assert_eq!(top("lambda", IconFilter { pack: Some(BUILTIN_PACK), ..IconFilter::default() }), Some("function"));
        assert_eq!(
            top("lambda", IconFilter { pack: Some("testaws"), ..IconFilter::default() }),
            Some("testaws/lambda")
        );
        let listed = registry.list(&IconFilter { pack: Some("testaws"), ..IconFilter::default() });
        assert_eq!(listed.len(), 2);
        assert_eq!(registry.suggest_in_pack("testaws", "testaws/lamda"), Some("testaws/lambda"));
    }

    #[test]
    fn provider_groups_map_to_categories() {
        assert_eq!(category_of_group("aws", Some("Arch_Databases")), IconCategory::Data);
        assert_eq!(category_of_group("aws", Some("Networking-Content-Delivery")), IconCategory::Network);
        assert_eq!(category_of_group("azure", Some("security")), IconCategory::Security);
        assert_eq!(category_of_group("k8s", Some("resources")), IconCategory::Kubernetes);
        assert_eq!(category_of_group("gcp", None), IconCategory::Generic);
        assert_eq!(split_pack_id("aws/lambda"), Some(("aws", "lambda")));
        assert_eq!(split_pack_id("lambda"), None);
    }
}
