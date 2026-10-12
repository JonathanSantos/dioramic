//! Subcommands other than `init` (which has its own module tree).

pub mod icons;
pub mod lint;
pub mod mcp;
pub mod render;
pub mod watch;

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::Result;
use dioramic_core::icons::IconRegistry;
use dioramic_packs::PackStore;
use dioramic_render::TextFormat;
use dioramic_spec::SourceFormat;

use crate::ui::{self, UserError};

/// Reads a spec file with a friendly error when it is missing or unreadable.
pub fn read_spec(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|error| {
        let message = format!("cannot read {}: {error}", path.display());
        let user_error = match error.kind() {
            ErrorKind::NotFound => UserError::new(format!("spec not found: {}", path.display()))
                .hint("check the path; specs are usually named *.diorama.yaml"),
            ErrorKind::InvalidData => UserError::new(message).hint("specs must be UTF-8 text (YAML or JSON)"),
            _ => UserError::new(message),
        };
        user_error.into()
    })
}

/// Engine-side mirror of the spec text format.
pub fn text_format(format: SourceFormat) -> TextFormat {
    match format {
        SourceFormat::Json => TextFormat::Json,
        SourceFormat::Yaml => TextFormat::Yaml,
    }
}

/// Flat icons a spec can use: the installed packs it references (`aws/…`) and the project icons in the
/// `icons/` directory next to it. Unreadable icon files are printed as warnings, never fatal.
pub fn spec_icons(spec: &Path, text: &str) -> IconRegistry {
    let store = PackStore::from_env().ok();
    let directory = spec.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let (registry, problems) = dioramic_packs::registry_for_spec(store.as_ref(), text, Some(directory));
    for problem in problems {
        ui::print_warning(&format!("icon {}: {}", problem.path.display(), problem.message));
    }
    registry
}
