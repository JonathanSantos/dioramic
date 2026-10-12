//! `dioramic icons search|list|install|remove`: the icon catalog (built-in + installed packs) and the packs.

mod install;

use anyhow::Result;
use dioramic_core::icons::flat::{BUILTIN_PACK, entry_pack};
use dioramic_core::icons::{IconCategory, IconEntry, IconFilter, IconRegistry};
use dioramic_packs::{PackStore, Provider};

use crate::cli::IconsCommand;
use crate::ui::{self, BOLD, CYAN, DIM, GREEN, UserError, paint};

const HEADERS: [&str; 4] = ["ID", "NAME", "CATEGORY", "ALIASES"];
/// Aliases shown per row before `…` (provider icons carry many).
const MAX_ALIASES_SHOWN: usize = 4;

/// Runs an `icons` subcommand.
pub fn run(command: &IconsCommand) -> Result<()> {
    match command {
        IconsCommand::Search { query, category, pack, limit } => {
            let registry = installed_registry();
            let filter = IconFilter { category: parse_category(category.as_deref())?, pack: pack.as_deref() };
            search(&registry, query, &filter, *limit)
        }
        IconsCommand::List { packs: true, .. } => list_packs(),
        IconsCommand::List { category, pack, packs: false } => {
            let registry = installed_registry();
            let filter = IconFilter { category: parse_category(category.as_deref())?, pack: pack.as_deref() };
            list(&registry, &filter)
        }
        IconsCommand::Install { pack, from } => install::run(&parse_provider(pack)?, from),
        IconsCommand::Remove { pack } => remove(pack),
    }
}

/// The pack store, or a user error when no data directory can be found.
pub fn store() -> Result<PackStore> {
    PackStore::from_env().map_err(|error| UserError::new(error.to_string()).into())
}

/// Every installed pack (problems are printed as warnings).
fn installed_registry() -> IconRegistry {
    let mut registry = IconRegistry::new();
    if let Ok(store) = PackStore::from_env() {
        for problem in store.load_into(&mut registry, None) {
            ui::print_warning(&format!("{}: {}", problem.path.display(), problem.message));
        }
    }
    registry
}

fn search(registry: &IconRegistry, query: &str, filter: &IconFilter<'_>, limit: usize) -> Result<()> {
    let hits = registry.search(query, filter, limit);
    if hits.is_empty() {
        anstream::println!("no icon matches '{query}'");
        ui::print_hint(&no_match_hint(registry, filter));
        return Ok(());
    }
    let entries: Vec<&IconEntry> = hits.iter().map(|hit| hit.entry).collect();
    print_table(&entries);
    Ok(())
}

fn no_match_hint(registry: &IconRegistry, filter: &IconFilter<'_>) -> String {
    let missing: Vec<&str> =
        Provider::ALL.iter().map(|provider| provider.id()).filter(|id| !registry.has_pack(id)).collect();
    match filter.pack {
        Some(pack) if pack != BUILTIN_PACK && !registry.has_pack(pack) => {
            format!("the '{pack}' pack is not installed: run `dioramic icons install {pack}`")
        }
        _ if !missing.is_empty() => format!(
            "use `icon: block` with a label and a color, or install provider icons: `dioramic icons install {}`",
            missing.join("|")
        ),
        _ => "use `icon: block` with a label and a color, or browse with `dioramic icons list`".to_owned(),
    }
}

fn list(registry: &IconRegistry, filter: &IconFilter<'_>) -> Result<()> {
    let entries = registry.list(filter);
    print_table(&entries);
    anstream::println!("{}", paint(DIM, ui::plural(entries.len(), "icon")));
    Ok(())
}

fn list_packs() -> Result<()> {
    let store = store()?;
    let packs = store.installed();
    anstream::println!("{}", paint(DIM, format!("store: {}", store.root().display())));
    if packs.is_empty() {
        anstream::println!("no icon pack installed");
        ui::print_hint("run `dioramic icons install aws` (or gcp, azure, k8s)");
        return Ok(());
    }
    for pack in &packs {
        let manifest = &pack.manifest;
        anstream::println!(
            "{} {}  {}",
            paint(BOLD, &manifest.id),
            manifest.name,
            paint(DIM, format!("{} · version {}", ui::plural(manifest.icons.len(), "icon"), manifest.version))
        );
        anstream::println!("    {} {}", paint(DIM, "source: "), manifest.source);
        anstream::println!("    {} {}", paint(DIM, "license:"), manifest.license);
    }
    let missing: Vec<&str> = Provider::ALL
        .iter()
        .map(|provider| provider.id())
        .filter(|id| !packs.iter().any(|pack| pack.manifest.id == *id))
        .collect();
    if !missing.is_empty() {
        ui::print_hint(&format!("also available: dioramic icons install {}", missing.join("|")));
    }
    Ok(())
}

fn remove(pack: &str) -> Result<()> {
    let store = store()?;
    let removed = store.remove(pack).map_err(|error| {
        let installed: Vec<String> = store.installed().into_iter().map(|pack| pack.manifest.id).collect();
        let hint = if installed.is_empty() {
            "no pack is installed".to_owned()
        } else {
            format!("installed: {}", installed.join(", "))
        };
        UserError::new(error.to_string()).hint(hint)
    })?;
    anstream::println!(
        "{} removed {} {}",
        paint(GREEN, "✓"),
        paint(BOLD, &removed.manifest.id),
        paint(DIM, removed.dir.display())
    );
    Ok(())
}

fn parse_provider(text: &str) -> Result<Provider> {
    text.parse::<Provider>().map_err(|error| UserError::new(error.to_string()).into())
}

fn parse_category(text: Option<&str>) -> Result<Option<IconCategory>> {
    let Some(text) = text else { return Ok(None) };
    IconCategory::parse(text).map(Some).ok_or_else(|| {
        let valid: Vec<&str> = IconCategory::ALL.iter().map(|category| category.as_str()).collect();
        UserError::new(format!("unknown icon category '{text}'"))
            .hint(format!("use one of: {}", valid.join(", ")))
            .into()
    })
}

fn print_table(entries: &[&IconEntry]) {
    let rows: Vec<[String; 4]> = entries.iter().map(|entry| row(entry)).collect();
    let mut widths = HEADERS.map(str::len);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let header: Vec<String> = HEADERS.iter().zip(widths).map(|(title, width)| ui::pad(title, width)).collect();
    anstream::println!("{}", paint(DIM, header.join("  ").trim_end()));
    for [id, name, category, aliases] in rows {
        anstream::println!(
            "{}  {}  {}  {}",
            paint(BOLD, ui::pad(&id, widths[0])),
            ui::pad(&name, widths[1]),
            paint(CYAN, ui::pad(&category, widths[2])),
            paint(DIM, aliases)
        );
    }
}

fn row(entry: &IconEntry) -> [String; 4] {
    let mut aliases: Vec<&str> = entry.aliases.iter().take(MAX_ALIASES_SHOWN).copied().collect();
    if entry.aliases.len() > MAX_ALIASES_SHOWN {
        aliases.push("…");
    }
    let category = match entry_pack(entry) {
        Some(pack) => format!("{} ({pack})", entry.category.as_str()),
        None => entry.category.as_str().to_owned(),
    };
    [entry.id.to_owned(), entry.name.to_owned(), category, aliases.join(", ")]
}

#[cfg(test)]
mod tests {
    use dioramic_core::icons;

    use super::*;

    #[test]
    fn categories_parse_with_friendly_error() {
        assert!(matches!(parse_category(Some("k8s")), Ok(Some(IconCategory::Kubernetes))));
        assert!(matches!(parse_category(None), Ok(None)));
        let error = parse_category(Some("databases")).err().map(|error| error.to_string()).unwrap_or_default();
        assert!(error.contains("databases"));
    }

    #[test]
    fn rows_join_aliases() {
        let entry = icons::lookup("postgres");
        let cells = entry.map(row);
        assert!(cells.is_some_and(|[id, _, category, aliases]| id == "database"
            && category == "data"
            && aliases.contains("postgres")
            && aliases.ends_with('…')));
    }

    #[test]
    fn providers_parse_with_friendly_error() {
        assert!(matches!(parse_provider("Kubernetes"), Ok(Provider::Kubernetes)));
        let error = parse_provider("ibm").err().map(|error| error.to_string()).unwrap_or_default();
        assert!(error.contains("aws, gcp, azure or k8s"), "{error}");
    }

    #[test]
    fn hints_name_the_missing_packs() {
        let registry = IconRegistry::new();
        let hint = no_match_hint(&registry, &IconFilter { pack: Some("aws"), ..IconFilter::default() });
        assert!(hint.contains("dioramic icons install aws"));
        let hint = no_match_hint(&registry, &IconFilter::default());
        assert!(hint.contains("aws|gcp|azure|k8s"));
    }
}
