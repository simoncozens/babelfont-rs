use std::sync::LazyLock;

use serde::Deserialize;

use crate::filters::FontFilter;

#[derive(Debug, Deserialize)]
pub(crate) struct GlyphInfo {
    /// Other names the glyph went by, comma-separated
    alt_names: Option<String>,
    pub(crate) name: String,
    category: String,
    sub_category: Option<String>,
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

struct GlyphsDataIndex {
    by_name: std::collections::HashMap<&'static str, &'static GlyphInfo>,
    by_alt_name: std::collections::HashMap<&'static str, &'static GlyphInfo>,
}

static GLYPHS_DATA_INDEX: LazyLock<GlyphsDataIndex> = LazyLock::new(|| {
    let mut index = GlyphsDataIndex {
        by_name: Default::default(),
        by_alt_name: Default::default(),
    };
    for info in GLYPHS_DATA.iter() {
        index.by_name.insert(info.name.as_str(), info);
        let alt_names = info
            .alt_names
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty());
        for alt_name in alt_names {
            index.by_alt_name.entry(alt_name).or_insert(info);
        }
    }
    index
});

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
                    // a mark only if it attaches: a spacing accent or a
                    // modifier letter (acute, apostrophemod) is a base, as
                    // glyphsLib and fontc have it (category_for_glyph_preliminary)
                    "Mark" => {
                        glyph.category = if matches!(
                            glyph_info.sub_category.as_deref(),
                            Some("Nonspacing" | "Spacing Combining")
                        ) {
                            crate::GlyphCategory::Mark
                        } else {
                            crate::GlyphCategory::Base
                        }
                    }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{Font, Glyph, GlyphCategory};

    #[test]
    fn test_only_attaching_marks_are_marks() {
        // Glyphs' data has all three as Mark: Modifier, Spacing and
        // Nonspacing. A shaper zeroes a mark's advance, so glyphsLib and fontc
        // take only a nonspacing or spacing combining one for a mark.
        let mut font = Font::new();
        for name in ["apostrophemod", "acute", "acutecomb"] {
            font.glyphs.push(Glyph::new(name));
        }
        GlyphsData.apply(&mut font).unwrap();
        let category = |name: &str| font.glyphs.get(name).unwrap().category.clone();
        assert_eq!(category("apostrophemod"), GlyphCategory::Base);
        assert_eq!(category("acute"), GlyphCategory::Base);
        assert_eq!(category("acutecomb"), GlyphCategory::Mark);
    }
}
