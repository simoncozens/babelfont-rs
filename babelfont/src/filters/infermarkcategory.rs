use crate::{filters::FontFilter, GlyphCategory};

/// A filter that classifies uncategorized glyphs whose anchors are exclusively
/// mark-side (underscore-prefixed) as Nonspacing marks.
///
/// Some sources carry no glyph-class information at all (e.g. FontForge SFDs
/// without `GlyphClass` lines, such as Glegoo Bold or Ranga), so their mark
/// glyphs arrive with category Unknown and filters keyed on `Mark` (such as
/// `--set-subcategory`) never fire. The anchors are still authoritative: a
/// glyph whose anchors are all mark-attachment anchors participates in mark
/// positioning as a mark, so give it category Mark and subCategory Nonspacing
/// (mirroring what an explicit mark glyph class would have produced).
pub struct InferMarkCategory;

fn is_mark_only(layer: &crate::Layer) -> (bool, bool) {
    let mut has_mark_anchor = false;
    let mut has_base_anchor = false;
    for anchor in layer.anchors.iter() {
        if anchor.name.starts_with('_') {
            has_mark_anchor = true;
        } else {
            has_base_anchor = true;
        }
    }
    (has_mark_anchor, has_base_anchor)
}

impl FontFilter for InferMarkCategory {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            if glyph.category != GlyphCategory::Unknown {
                continue;
            }
            let mut has_mark_anchor = false;
            let mut has_base_anchor = false;
            for layer in glyph.layers.iter() {
                let (m, b) = is_mark_only(layer);
                has_mark_anchor |= m;
                has_base_anchor |= b;
            }
            if has_mark_anchor && !has_base_anchor {
                glyph.category = GlyphCategory::Mark;
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
        Ok(InferMarkCategory)
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("infermarkcategory")
            .long("infer-mark-category")
            .help("Classify uncategorized glyphs with only mark-side (underscore) anchors as Nonspacing marks")
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Anchor, Font, Glyph, Layer};

    fn glyph_with_anchors(name: &str, anchors: Vec<&str>) -> Glyph {
        let mut layer = Layer::new(500.0);
        for a in anchors {
            layer.anchors.push(Anchor {
                name: a.to_string(),
                x: 0.0,
                y: 0.0,
                ..Default::default()
            });
        }
        Glyph {
            name: name.into(),
            layers: vec![layer],
            ..Default::default()
        }
    }

    #[test]
    fn test_infer_mark_category() {
        let mut font = Font::new();
        font.glyphs.0.push(glyph_with_anchors("anusvara", vec!["_top"]));
        font.glyphs.0.push(glyph_with_anchors("ka", vec!["top", "bottom"]));
        // A mark that can also carry other marks (mkmk) still counts as a base
        // carrier, so it is left alone for explicit classification.
        font.glyphs.0.push(glyph_with_anchors("candra", vec!["_top", "top"]));
        InferMarkCategory.apply(&mut font).expect("filter failed");
        assert_eq!(
            font.glyphs.get("anusvara").expect("anusvara").category,
            GlyphCategory::Mark
        );
        assert_eq!(
            font.glyphs
                .get("anusvara")
                .expect("anusvara")
                .format_specific
                .get("subcategory")
                .and_then(|v| v.as_str()),
            Some("Nonspacing")
        );
        assert_eq!(
            font.glyphs.get("ka").expect("ka").category,
            GlyphCategory::Unknown
        );
        assert_eq!(
            font.glyphs.get("candra").expect("candra").category,
            GlyphCategory::Unknown
        );
    }
}
