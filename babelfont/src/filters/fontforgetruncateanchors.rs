use crate::filters::FontFilter;

/// A filter that truncates anchor coordinates toward zero.
///
/// FontForge's GPOS export writes an anchor point's coordinates truncated toward
/// zero: an anchor at 129.5 is exported at 129, and one at -46.7 at -46. A compiler
/// that rounds puts the mark attached through it a unit away.
#[derive(Default)]
pub struct FontForgeTruncateAnchors;

impl FontForgeTruncateAnchors {
    /// Create a new FontForgeTruncateAnchors filter
    pub fn new() -> Self {
        FontForgeTruncateAnchors
    }
}

impl FontFilter for FontForgeTruncateAnchors {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                for anchor in layer.anchors.iter_mut() {
                    anchor.x = anchor.x.trunc();
                    anchor.y = anchor.y.trunc();
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeTruncateAnchors::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgetruncateanchors")
            .long("fontforge-truncate-anchors")
            .help(
                "Truncate anchor coordinates toward zero, as FontForge's GPOS export \
                 does",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Anchor, Font, Glyph, Layer};

    #[test]
    fn test_anchors_are_truncated_toward_zero() {
        let mut layer = Layer::new(500.0);
        for (x, y) in [(129.5, -46.7), (-0.5, 600.9)] {
            layer.anchors.push(Anchor {
                name: "top".to_string(),
                x,
                y,
                ..Default::default()
            });
        }
        let mut font = Font::new();
        font.glyphs.0.push(Glyph {
            name: "a".into(),
            layers: vec![layer],
            ..Default::default()
        });
        FontForgeTruncateAnchors::new().apply(&mut font).unwrap();
        let positions: Vec<(f64, f64)> = font.glyphs.0[0].layers[0]
            .anchors
            .iter()
            .map(|a| (a.x, a.y))
            .collect();
        assert_eq!(positions, vec![(129.0, -46.0), (0.0, 600.0)]);
    }
}
