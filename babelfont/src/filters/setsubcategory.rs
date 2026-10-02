use crate::{filters::FontFilter, GlyphCategory};
use smol_str::SmolStr;

/// A filter that sets the subcategory of mark glyphs to Nonspacing, or Spacing
/// Combining for a mark that advances, for Glyphs export
pub struct SetSubcategory(Vec<SmolStr>);

impl SetSubcategory {
    /// Create a new SetSubcategory filter.
    /// If `glyph_names` is empty, all glyphs are processed.
    pub fn new(glyph_names: Vec<String>) -> Self {
        SetSubcategory(glyph_names.into_iter().map(SmolStr::from).collect())
    }
}

fn has_underscore_anchor(layer: &crate::Layer) -> bool {
    layer
        .anchors
        .iter()
        .any(|anchor| anchor.name.starts_with('_'))
}

impl FontFilter for SetSubcategory {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let filter_list = &self.0;
        for glyph in font.glyphs.iter_mut() {
            if !filter_list.is_empty() && !filter_list.contains(&glyph.name) {
                continue;
            }
            if glyph.category == GlyphCategory::Mark
                && glyph.layers.iter().any(has_underscore_anchor)
            {
                // A mark that advances is a spacing combining mark: a Glyphs
                // compiler gives a Nonspacing mark no advance.
                let subcategory = if glyph.layers.iter().any(|layer| layer.width != 0.0) {
                    "Spacing Combining"
                } else {
                    "Nonspacing"
                };
                glyph
                    .format_specific
                    .insert_json_non_null("subcategory", &subcategory.to_string());
            }
        }
        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(SetSubcategory(super::parse_glyph_list(s)))
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        super::glyph_filter_arg(
            "setsubcategory",
            "set-subcategory",
            "Set the subcategory of mark glyphs to Nonspacing (Spacing Combining when the \
             mark advances) for Glyphs export",
        )
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Anchor, Font, Glyph, Layer};

    fn mark(name: &str, width: f32) -> Glyph {
        let mut layer = Layer::new(width);
        layer.anchors.push(Anchor {
            name: "_top".to_string(),
            ..Default::default()
        });
        Glyph {
            name: name.into(),
            category: GlyphCategory::Mark,
            layers: vec![layer],
            ..Default::default()
        }
    }

    #[test]
    fn test_an_advancing_mark_is_spacing_combining() {
        let mut font = Font::new();
        font.glyphs.0.push(mark("acutecomb", 0.0));
        font.glyphs.0.push(mark("uni2E0F", 2443.0));
        SetSubcategory::new(vec![]).apply(&mut font).unwrap();
        let subcategory = |i: usize| {
            font.glyphs.0[i]
                .format_specific
                .get("subcategory")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        assert_eq!(subcategory(0).as_deref(), Some("Nonspacing"));
        assert_eq!(subcategory(1).as_deref(), Some("Spacing Combining"));
    }
}
