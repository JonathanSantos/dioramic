//! Embedded fonts loaded once per process (ADR-008: scanning system fonts cost 275 ms/frame in the spike).
//!
//! The 500/600 files are static instances that only carry the legacy family name (name ID 1 =
//! "Inter Medium", "Inter SemiBold"; no typographic ID 16), so fontdb would file them as separate
//! families and `font-family="Inter" font-weight="500"` would resolve to Regular (600 to Bold).
//! Every face is re-registered under its typographic family (T-322), keeping the legacy name as an
//! alias so SVGs that name the instance (`"Inter Medium, Inter"`) still match.

use std::sync::{Arc, OnceLock};

use dioramic_core::text::{EmbeddedFont, LABEL_FAMILY, embedded_fonts};
use resvg::usvg::{self, fontdb};

/// Shared font database with only the embedded fonts (never system fonts). Built on first use.
pub fn font_database() -> Arc<fontdb::Database> {
    static DATABASE: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    DATABASE
        .get_or_init(|| {
            let mut database = fontdb::Database::new();
            for font in embedded_fonts() {
                load_embedded_font(&mut database, font);
            }
            database.set_sans_serif_family(LABEL_FAMILY);
            Arc::new(database)
        })
        .clone()
}

/// `usvg` options wired to [`font_database`] with Inter as the default family.
pub fn usvg_options() -> usvg::Options<'static> {
    usvg::Options { fontdb: font_database(), font_family: LABEL_FAMILY.to_owned(), ..usvg::Options::default() }
}

/// Loads `font` and files each of its faces under the typographic family declared for it.
fn load_embedded_font(database: &mut fontdb::Database, font: &EmbeddedFont) {
    let source = fontdb::Source::Binary(Arc::new(font.data));
    for id in database.load_font_source(source) {
        let Some(face) = database.face(id).cloned() else {
            continue;
        };
        database.remove_face(id);
        database.push_face_info(under_typographic_family(face, font.family.css_name()));
    }
}

/// `face` with `family` first (fontdb treats the first family as the English name), legacy names kept.
fn under_typographic_family(mut face: fontdb::FaceInfo, family: &str) -> fontdb::FaceInfo {
    if !face.families.iter().any(|(name, _)| name == family) {
        face.families.insert(0, (family.to_owned(), fontdb::Language::English_UnitedStates));
    }
    face.id = fontdb::ID::dummy();
    face
}

#[cfg(test)]
mod tests {
    use dioramic_core::text::FontFamily;

    use super::*;
    use crate::raster::{RasterSize, rasterize};

    #[test]
    fn database_contains_only_embedded_families() {
        let database = font_database();
        let families: std::collections::BTreeSet<String> =
            database.faces().flat_map(|face| face.families.iter().map(|(name, _)| name.clone())).collect();
        assert!(families.contains("Inter"), "{families:?}");
        assert!(families.contains("Space Grotesk"), "{families:?}");
        assert_eq!(database.faces().count(), embedded_fonts().len());
    }

    #[test]
    fn every_embedded_weight_is_reachable_through_its_typographic_family() {
        let database = font_database();
        for font in embedded_fonts() {
            let family = font.family.css_name();
            let query = fontdb::Query {
                families: &[fontdb::Family::Name(family)],
                weight: fontdb::Weight(font.weight),
                ..fontdb::Query::default()
            };
            let face = database.query(&query).and_then(|id| database.face(id)).unwrap();
            assert_eq!(face.weight.0, font.weight, "{family} {}", font.weight);
        }
    }

    #[test]
    fn legacy_instance_names_stay_as_aliases() {
        let families: Vec<String> =
            font_database().faces().flat_map(|face| face.families.iter().map(|(name, _)| name.clone())).collect();
        assert!(families.iter().any(|name| name == "Inter Medium"), "{families:?}");
        assert!(families.iter().any(|name| name == "Inter SemiBold"), "{families:?}");
    }

    /// Dark pixels of "Load Balancer" drawn with `font-family` and `font-weight`.
    fn ink(family: &str, weight: u16) -> usize {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="40" viewBox="0 0 240 40">
            <rect width="240" height="40" fill="white"/>
            <text x="8" y="28" font-family="{family}" font-weight="{weight}" font-size="22" fill="black">Load Balancer</text>
            </svg>"#
        );
        let pixmap = rasterize(&svg, RasterSize::Scale(1.0)).unwrap();
        pixmap.pixels().iter().filter(|pixel| pixel.red() < 128).count()
    }

    fn assert_strictly_increasing(label: &str, inks: &[usize]) {
        assert!(inks.windows(2).all(|pair| pair[0] < pair[1]), "{label}: ink {inks:?}");
    }

    #[test]
    fn plain_inter_ink_grows_with_every_weight() {
        let inks: Vec<usize> = [400, 500, 600, 700].map(|weight| ink("Inter", weight)).to_vec();
        assert_strictly_increasing("Inter", &inks);
    }

    #[test]
    fn instance_names_render_the_same_face_as_the_typographic_family() {
        // What the SVG writer emits for 500/600 (`weight_family`): instance name first, then the family.
        let inks =
            [ink("Inter", 400), ink("Inter Medium, Inter", 500), ink("Inter SemiBold, Inter", 600), ink("Inter", 700)];
        assert_strictly_increasing("Inter Medium, Inter", &inks);
        assert_eq!(inks[1], ink("Inter", 500));
        assert_eq!(inks[2], ink("Inter", 600));
    }

    #[test]
    fn plain_space_grotesk_ink_grows_with_every_weight() {
        let family = FontFamily::SpaceGrotesk.css_name();
        let inks: Vec<usize> = [500, 600, 700].map(|weight| ink(family, weight)).to_vec();
        assert_strictly_increasing(family, &inks);
    }
}
