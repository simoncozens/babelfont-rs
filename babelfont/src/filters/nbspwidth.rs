//! Make a separate no-break space glyph as wide as the space glyph.
//!
//! This is a **correction**, not a faithful conversion, and it is opt-in for
//! that reason.
//!
//! A source may carry its own U+00A0 glyph with an advance that is not the
//! space's. Font QA rejects that (`whitespace_widths`), and mixing the two in
//! running text is visibly wrong, so normalising it produces a better font.
//!
//! But a source may state a deliberate no-break-space advance -- sometimes an
//! exact multiple of the space -- and the shipped binary preserves it. Applying
//! this changes such a font, which is why it is a separate opt-in pass rather
//! than part of `--add-legacy-duplicate-cmap`: the cmap mapping restores
//! coverage the exporter synthesised, while this changes a stated value.

use crate::filters::FontFilter;

/// Set a separate no-break space's advance to the space's.
#[derive(Default)]
pub struct NbspWidth;

impl NbspWidth {
    /// Create a new NbspWidth filter
    pub fn new() -> Self {
        NbspWidth
    }
}

impl FontFilter for NbspWidth {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let space_widths: Vec<f32> = font
            .glyphs
            .iter()
            .find(|g| g.codepoints.contains(&0x0020))
            .map(|g| g.layers.iter().map(|l| l.width).collect())
            .unwrap_or_default();
        if space_widths.is_empty() {
            return Ok(());
        }

        let Some(nbsp) = font
            .glyphs
            .iter_mut()
            .find(|g| g.codepoints.contains(&0x00A0) && !g.codepoints.contains(&0x0020))
        else {
            // Either there is no separate no-break space, or it is the space
            // glyph itself carrying both codepoints -- in which case there is
            // nothing to reconcile.
            return Ok(());
        };

        for (layer, width) in nbsp.layers.iter_mut().zip(space_widths.iter()) {
            if (layer.width - width).abs() > f32::EPSILON {
                log::info!(
                    "Setting the no-break space advance to the space's ({} -> {width})",
                    layer.width
                );
                layer.width = *width;
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(NbspWidth::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("nbspwidth")
            .long("normalise-nbsp-width")
            .help(
                "Set a separate no-break space glyph's advance to the space's. \
                 A correction, not a faithful conversion: a source may state a \
                 deliberate no-break-space advance.",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Glyph, Layer};

    fn space_and_nbsp(space_w: f32, nbsp_w: f32) -> crate::Font {
        let mut font = crate::Font::new();
        let mut mk = |name: &str, cp: u32, w: f32| {
            let mut g = Glyph::new(name);
            g.codepoints.push(cp);
            let l = Layer {
                width: w,
                ..Default::default()
            };
            g.layers.push(l);
            font.glyphs.push(g);
        };
        mk("space", 0x0020, space_w);
        mk("uni00A0", 0x00A0, nbsp_w);
        font
    }

    fn nbsp_width(font: &crate::Font) -> f32 {
        font.glyphs
            .iter()
            .find(|g| g.codepoints.contains(&0x00A0))
            .and_then(|g| g.layers.first())
            .map(|l| l.width)
            .unwrap()
    }

    #[test]
    fn widens_a_narrow_nbsp_to_the_space() {
        let mut font = space_and_nbsp(683.0, 838.0);
        NbspWidth::new().apply(&mut font).unwrap();
        assert_eq!(nbsp_width(&font), 683.0);
    }

    #[test]
    fn leaves_an_already_matching_nbsp_alone() {
        let mut font = space_and_nbsp(500.0, 500.0);
        NbspWidth::new().apply(&mut font).unwrap();
        assert_eq!(nbsp_width(&font), 500.0);
    }

    #[test]
    fn does_nothing_without_a_space() {
        let mut font = crate::Font::new();
        let mut g = Glyph::new("uni00A0");
        g.codepoints.push(0x00A0);
        let l = Layer {
            width: 300.0,
            ..Default::default()
        };
        g.layers.push(l);
        font.glyphs.push(g);
        NbspWidth::new().apply(&mut font).unwrap();
        assert_eq!(nbsp_width(&font), 300.0);
    }

    #[test]
    fn does_nothing_when_one_glyph_carries_both_codepoints() {
        // The duplicate-cmap case: U+00A0 mapped onto the space itself. There
        // is no separate advance to reconcile.
        let mut font = crate::Font::new();
        let mut g = Glyph::new("space");
        g.codepoints.push(0x0020);
        g.codepoints.push(0x00A0);
        let l = Layer {
            width: 250.0,
            ..Default::default()
        };
        g.layers.push(l);
        font.glyphs.push(g);
        NbspWidth::new().apply(&mut font).unwrap();
        assert_eq!(nbsp_width(&font), 250.0);
    }
}
