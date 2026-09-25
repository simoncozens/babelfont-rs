use crate::filters::fontforge_standard_height::{exported_heights, CurveMean, MeasuredHeights};
use crate::filters::FontFilter;
use crate::MetricType;

#[derive(Default)]
/// A filter that fills the x-height and cap height the way a FontForge built before
/// 2012-05-14 exported them.
///
/// Until commit 4d34d21ef866, FontForge's `SFStandardHeight` averaged the tops of
/// its reference glyphs, when none of them is flat, by dividing the sum of the
/// distinct heights by the number of glyphs. Every later build divides by the number
/// of distinct heights. A font exported by an older build carries the smaller value,
/// often far below the letters it describes.
///
/// Like `--fontforge-os2-defaults` it fills only a value the source does not state,
/// so it must run before that filter for its values to be the ones kept. That
/// filter's notes on the OS/2 table version, on `pfmset` and on stated values apply
/// here too; these builds predate tag 20150612, so a non-zero `OS2Version:` sets the
/// table version outright, and a build before commit 3536593d (2010-06-11) does not
/// raise it for `OS2_UseTypoMetrics` or `OS2_WeightWidthSlopeOnly`, so it exports a
/// TrueType font as version 1, without these two values, unless `OS2Version:` says
/// otherwise. It does not model the other differences of those builds:
/// `SplineIsLinear` without the `ret &&` guard and with a relative `RealNear`, and a
/// glyph lookup that also matches alternate codepoints.
pub struct FontForgeHeightGlyphCountMean;

impl FontForgeHeightGlyphCountMean {
    /// Create a new FontForgeHeightGlyphCountMean filter
    pub fn new() -> Self {
        FontForgeHeightGlyphCountMean
    }
}

impl FontFilter for FontForgeHeightGlyphCountMean {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let mut filled = 0;
        let heights: Vec<MeasuredHeights> = font
            .masters
            .iter()
            .map(|master| exported_heights(font, master, CurveMean::GlyphCount))
            .collect();
        for (master, measured) in font.masters.iter_mut().zip(heights) {
            for (metric, value) in [
                (MetricType::XHeight, measured.x_height),
                (MetricType::CapHeight, measured.cap_height),
            ] {
                if !master.metrics.contains_key(&metric) {
                    master.metrics.insert(metric, value);
                    filled += 1;
                }
            }
        }
        log::info!(
            "Filled {filled} x-height and cap height value(s) using the SFStandardHeight \
             rule of FontForge before 2012-05-14 (distinct tops over the glyph count)"
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeHeightGlyphCountMean::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgeheightglyphcountmean")
            .long("fontforge-height-glyph-count-mean")
            .help(
                "Fill the x-height and cap height the source does not state as a FontForge \
                 built before 2012-05-14 exported them: with no flat top, the sum of the \
                 distinct tops over the number of glyphs. Run it before \
                 --fontforge-os2-defaults. Use only to reproduce a binary such a FontForge \
                 exported",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Master};

    #[test]
    fn test_a_stated_height_is_never_overwritten() {
        let mut m = Master::default();
        m.metrics.insert(MetricType::Ascender, 800);
        m.metrics.insert(MetricType::Descender, -200);
        m.metrics.insert(MetricType::XHeight, 480);
        let mut f = Font::new();
        f.masters.push(m);
        FontForgeHeightGlyphCountMean::new().apply(&mut f).unwrap();
        assert_eq!(f.masters[0].metrics.get(&MetricType::XHeight), Some(&480));
        // nothing to measure: the exporter writes 0
        assert_eq!(f.masters[0].metrics.get(&MetricType::CapHeight), Some(&0));
    }
}
