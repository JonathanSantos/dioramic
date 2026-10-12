//! `search_icons`: ranked search over the built-in catalog, the installed provider packs and the project
//! icons, optionally with a contact sheet image.

use dioramic_core::icons::flat::{BUILTIN_PACK, PROVIDER_PACKS};
use dioramic_core::icons::{self, IconCategory, IconEntry, IconFilter, IconRegistry};
use dioramic_core::theme::Theme;
use dioramic_render::PREVIEW_MAX_SIDE_PX;
use dioramic_render::png::encode_png;
use dioramic_render::preview::downscale;
use dioramic_render::raster::{RasterSize, rasterize};
use dioramic_spec::ThemeName;
use rmcp::model::CallToolResult;
use serde::Serialize;
use serde_json::json;

use crate::outcome::{ToolError, png_block, run_blocking, success};
use crate::params::{DEFAULT_ICON_LIMIT, SearchIconsParams};
use crate::resources::IconSummary;

/// Upper bound of `limit`.
const MAX_ICON_LIMIT: u32 = 100;
/// Raster scale of the contact sheet before downscaling to the preview size.
const CONTACT_SHEET_SCALE: f64 = 1.0;
/// Shown when nothing matched.
const NO_MATCH_HINT: &str =
    "No icon matched. Try a broader query or a category, or use icon: block with a color and label.";

/// Pack availability reported with every search, so the model knows which provider ids work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PackStatus {
    id: &'static str,
    name: &'static str,
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    install: Option<String>,
}

/// Runs the tool against `registry` (installed packs + project icons).
pub async fn search_icons(
    params: SearchIconsParams,
    registry: IconRegistry,
    structured: bool,
) -> Result<CallToolResult, ToolError> {
    let category = params.category.as_deref().map(parse_category).transpose()?;
    let pack = params.pack.as_deref().map(str::trim).filter(|pack| !pack.is_empty()).map(str::to_ascii_lowercase);
    let limit = params.limit.unwrap_or(DEFAULT_ICON_LIMIT).clamp(1, MAX_ICON_LIMIT) as usize;
    let query = params.query.trim().to_owned();
    let packs = pack_status(&registry);
    let missing_pack =
        pack.as_deref().filter(|pack| *pack != BUILTIN_PACK && !registry.has_pack(pack)).map(str::to_owned);
    let search_registry = registry.clone();
    let entries = run_blocking(move || {
        let filter = IconFilter { category, pack: pack.as_deref() };
        Ok(find(&search_registry, &query, &filter, limit))
    })
    .await?;
    let mut blocks = Vec::new();
    if params.preview && !entries.is_empty() {
        let sheet_entries = entries.clone();
        let png = run_blocking(move || contact_sheet_png(&sheet_entries)).await?;
        blocks.push(png_block(&png));
    }
    let icons: Vec<IconSummary> = entries.iter().copied().map(IconSummary::from).collect();
    let mut payload = json!({ "icons": icons, "packs": packs });
    if let Some(pack) = missing_pack {
        payload["hint"] = json!(format!(
            "The '{pack}' icon pack is not installed. Ask the user to run `dioramic icons install {pack}`; meanwhile use built-in icons."
        ));
    } else if icons.is_empty() {
        payload["hint"] = json!(NO_MATCH_HINT);
    }
    Ok(success(blocks, payload, structured))
}

fn pack_status(registry: &IconRegistry) -> Vec<PackStatus> {
    PROVIDER_PACKS
        .iter()
        .map(|(id, name)| {
            let installed = registry.has_pack(id);
            PackStatus { id, name, installed, install: (!installed).then(|| format!("dioramic icons install {id}")) }
        })
        .collect()
}

/// Empty query lists the catalog (filtered); otherwise ranked search.
fn find(registry: &IconRegistry, query: &str, filter: &IconFilter<'_>, limit: usize) -> Vec<&'static IconEntry> {
    if query.is_empty() {
        return registry.list(filter).into_iter().take(limit).collect();
    }
    registry.search(query, filter, limit).into_iter().map(|hit| hit.entry).collect()
}

fn parse_category(text: &str) -> Result<IconCategory, ToolError> {
    IconCategory::parse(text).ok_or_else(|| {
        let known: Vec<&str> = IconCategory::ALL.iter().map(|category| category.as_str()).collect();
        ToolError::BadRequest(format!("unknown category '{text}'; use one of: {}", known.join(", ")))
    })
}

fn contact_sheet_png(entries: &[&'static IconEntry]) -> Result<Vec<u8>, ToolError> {
    let svg = icons::contact_sheet_svg(entries, &Theme::builtin(ThemeName::Blueprint));
    let sheet = rasterize(&svg, RasterSize::Scale(CONTACT_SHEET_SCALE))?;
    Ok(encode_png(&downscale(&sheet, PREVIEW_MAX_SIDE_PX)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_lists_the_catalog_by_category() {
        let registry = IconRegistry::new();
        let filter = IconFilter { category: Some(IconCategory::Data), pack: None };
        let data = find(&registry, "", &filter, 50);
        assert!(!data.is_empty());
        assert!(data.iter().all(|entry| entry.category == IconCategory::Data));
        assert_eq!(find(&registry, "", &IconFilter::default(), 3).len(), 3);
    }

    #[test]
    fn unknown_categories_list_the_valid_ones() {
        let error = parse_category("gpu").unwrap_err();
        assert!(error.to_string().contains("observability"), "{error}");
        assert_eq!(parse_category("Kubernetes").unwrap(), IconCategory::Kubernetes);
    }

    #[test]
    fn pack_status_tells_how_to_install() {
        let status = pack_status(&IconRegistry::new());
        assert_eq!(status.len(), 4);
        assert!(status.iter().all(|pack| !pack.installed && pack.install.is_some()));
        assert_eq!(status[0].install.as_deref(), Some("dioramic icons install aws"));
    }
}
