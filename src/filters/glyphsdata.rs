use std::sync::LazyLock;

use serde::Deserialize;

use crate::filters::FontFilter;

#[derive(Debug, Deserialize)]
struct GlyphInfo {
    // unicode: Option<u32>,
    // unicode_legacy: Option<String>,
    name: String,
    category: String,
    // sub_category: Option<String>,
    // case: Option<String>,
    // direction: Option<String>,
    // script: Option<String>,
    production: Option<String>,
    // alt_names: Vec<String>,
}

const GLYPHS_DATA_STR: &str = include_str!(concat!(env!("OUT_DIR"), "/glyphsdata.json"));
#[allow(clippy::expect_used)]
static GLYPHS_DATA: LazyLock<Vec<GlyphInfo>> =
    LazyLock::new(|| serde_json::from_str(GLYPHS_DATA_STR).expect("Failed to parse glyphs data"));

/// A filter that adds Glyphs.app glyph metadata to the font
pub struct GlyphsData;

impl FontFilter for GlyphsData {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Applying GlyphsData filter");
        for glyph_info in GLYPHS_DATA.iter() {
            if let Some(glyph) = font.glyphs.get_mut(&glyph_info.name) {
                if let Some(production_name) = &glyph_info.production {
                    if glyph.production_name.is_none() {
                        glyph.production_name = Some(production_name.into());
                    }
                }
                if glyph.category != crate::GlyphCategory::Unknown {
                    continue;
                }
                match glyph_info.category.as_str() {
                    "Mark" => glyph.category = crate::GlyphCategory::Mark,
                    "Base" => glyph.category = crate::GlyphCategory::Base,
                    "Ligature" => glyph.category = crate::GlyphCategory::Ligature,
                    // "Component" => glyph.category = crate::GlyphCategory::Component,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(GlyphsData)
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("glyphsdata")
            .long("glyphs-data")
            .help("Add Glyphs.app glyph metadata to the font")
            .action(clap::ArgAction::SetTrue)
    }
}
