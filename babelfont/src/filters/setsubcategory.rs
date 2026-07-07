use crate::{filters::FontFilter, GlyphCategory};

/// A filter that sets the subcategory of mark glyphs to Nonspacing for Glyphs export
pub struct SetSubcategory;

fn has_underscore_anchor(layer: &crate::Layer) -> bool {
    layer
        .anchors
        .iter()
        .any(|anchor| anchor.name.starts_with('_'))
}

impl FontFilter for SetSubcategory {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            if glyph.category == GlyphCategory::Mark
                && glyph.layers.iter().any(has_underscore_anchor)
            {
                glyph
                    .format_specific
                    .insert_json_non_null("subcategory", &"Nonspacing".to_string());
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(SetSubcategory)
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("setsubcategory")
            .long("set-subcategory")
            .help("Set the subcategory of mark glyphs to Nonspacing for Glyphs export")
            .action(clap::ArgAction::SetTrue)
    }
}
