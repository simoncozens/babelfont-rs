use std::sync::LazyLock;

use crate::{filters::FontFilter, GlyphCategory};
use regex::Regex;

/// A filter that sets the category of conjunct glyphs without ligature anchors from ligature to base
pub struct CorrectConjunctCategory;

#[allow(clippy::unwrap_used)]
static LIGATURE_ANCHOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_\d+$").unwrap());

fn has_ligature_anchor(layer: &crate::Layer) -> bool {
    layer
        .anchors
        .iter()
        .any(|anchor| LIGATURE_ANCHOR.is_match(&anchor.name))
}

impl FontFilter for CorrectConjunctCategory {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            if glyph.category == GlyphCategory::Ligature
                && !glyph.layers.iter().any(has_ligature_anchor)
            {
                log::debug!(
                    "Correcting category of glyph {} from Ligature to Base",
                    glyph.name
                );
                glyph.category = GlyphCategory::Base;
                glyph
                    .format_specific
                    .insert_json_non_null("subcategory", &"Conjunct".to_string());
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(CorrectConjunctCategory)
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("correctconjunctcategory")
            .long("correct-conjunct-category")
            .help("Set the category of conjunct glyphs without ligature anchors from ligature to base")
            .action(clap::ArgAction::SetTrue)
    }
}
