use crate::filters::FontFilter;
use crate::glyph::GlyphCategory;

/// A filter that records an uncategorised glyph with a non-zero advance as a base,
/// so a compiler keeps the advance the source states.
///
/// A compiler that finds no category on a glyph may take one from the glyph's name,
/// and a mark's advance is zeroed, even when the source gives the glyph a width
/// (`commaaccent`, for example). FontForge's own TTF export writes every advance as
/// the source states it.
///
/// A glyph with a non-zero advance in every layer is recorded as a base. Glyphs that
/// already carry a category, including marks found by `--infer-mark-category`, which
/// reads anchors rather than names, are left as they are.
///
/// # This filter is order-dependent
///
/// It only skips a glyph that *already* carries a category, so `--infer-mark-category`
/// has to run before it. Passed the other way round, a real advancing mark is
/// recorded as a base here and `--infer-mark-category` then finds a category already
/// set and leaves it.
///
/// # What it costs
///
/// Setting `Base` on a glyph puts it in GDEF class 1, which suppresses the mark class
/// it would otherwise have had. An advancing glyph that should stay a mark has to be
/// categorised before this filter runs.
#[derive(Default)]
pub struct KeepSourceAdvances;

impl KeepSourceAdvances {
    /// Create a new KeepSourceAdvances filter
    pub fn new() -> Self {
        KeepSourceAdvances
    }
}

impl FontFilter for KeepSourceAdvances {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            if glyph.category != GlyphCategory::Unknown {
                continue;
            }
            // A glyph that is zero-width in any layer is left alone.
            let advancing = !glyph.layers.is_empty()
                && glyph.layers.iter().all(|layer| layer.width != 0.0);
            if advancing {
                glyph.category = GlyphCategory::Base;
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(KeepSourceAdvances::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("keepsourceadvances")
            .long("keep-source-advances")
            .help(
                "Record an uncategorised glyph with a non-zero advance as a base, so a \
                 compiler does not zero its advance as a mark by its name (e.g. \
                 commaaccent). A base is not in the GDEF mark class, so pass \
                 --infer-mark-category before this to keep the marks that have anchors",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Anchor, Font, Glyph, Layer};

    fn glyph(name: &str, width: f32, anchors: Vec<&str>) -> Glyph {
        let mut layer = Layer::new(width);
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
    fn test_advancing_glyph_becomes_a_base_and_zero_width_one_does_not() {
        let mut font = Font::new();
        // The source gives commaaccent a width, so it is recorded as a base.
        font.glyphs.0.push(glyph("commaaccent", 730.0, vec![]));
        // A zero-width glyph is left Unknown.
        font.glyphs.0.push(glyph("anusvara", 0.0, vec!["_top"]));
        KeepSourceAdvances::new().apply(&mut font).unwrap();
        assert_eq!(font.glyphs.0[0].category, GlyphCategory::Base);
        assert_eq!(font.glyphs.0[1].category, GlyphCategory::Unknown);
    }

    #[test]
    fn test_a_category_already_set_is_never_overwritten() {
        // This is what makes the filter order-dependent: --infer-mark-category has to
        // run first. An advancing mark it has already found stays a mark; run the
        // other way round it would have been recorded as a base here and
        // --infer-mark-category would then have found a category already set.
        let mut font = Font::new();
        let mut advancing_mark = glyph("dotbelow", 400.0, vec!["_bottom"]);
        advancing_mark.category = GlyphCategory::Mark;
        font.glyphs.0.push(advancing_mark);
        KeepSourceAdvances::new().apply(&mut font).unwrap();
        assert_eq!(
            font.glyphs.0[0].category,
            GlyphCategory::Mark,
            "an advancing glyph already categorised as a mark must stay one"
        );
    }
}
