use crate::filters::FontFilter;
use crate::MetricType;

/// The eight sub/superscript metrics and two strikeout metrics FontForge computes
/// at export, all together, when the source states no `OS2SubXSize`.
const SUBSUPER: [MetricType; 8] = [
    MetricType::SubscriptXSize,
    MetricType::SubscriptYSize,
    MetricType::SubscriptXOffset,
    MetricType::SubscriptYOffset,
    MetricType::SuperscriptXSize,
    MetricType::SuperscriptYSize,
    MetricType::SuperscriptXOffset,
    MetricType::SuperscriptYOffset,
];
const STRIKEOUT: [MetricType; 2] = [MetricType::StrikeoutSize, MetricType::StrikeoutPosition];

#[derive(Default)]
/// A filter that computes the ten OS/2 sub/superscript and strikeout metrics the way
/// FontForge's exporter does for a source that states no `OS2SubXSize`, replacing
/// any of them the source does state.
///
/// A FontForge `.sfd` usually carries no `OS2SubXSize` and friends. FontForge then
/// computes them while writing the binary, so the exported font has values that are
/// nowhere in the source, and a compiler with a different fallback writes different
/// ones.
///
/// FontForge treats the ten values as one group. Reading an `OS2SubXSize:` line is
/// what marks them as stated (`subsuper_set` in `sfd.c`), and when that mark is unset
/// the exporter computes all ten, replacing any of them the source does state. This
/// filter does the same, keyed on the subscript x size.
///
/// The rule is stated in FontForge's own source, `fontforge/tottf.c`,
/// `SFDefaultOS2SubSuper`:
///
/// ```text
/// os2_supysize = os2_subysize = .7*emsize;
/// os2_supxsize = os2_subxsize = .65*emsize;
/// os2_subyoff  = .14*emsize;
/// os2_supyoff  = .48*emsize;
/// os2_supxoff  =  s*os2_supyoff;    /* s = sin(italic_angle) */
/// os2_subxoff  = -s*os2_subyoff;
/// os2_strikeysize = 102*emsize/2048;
/// os2_strikeypos  = 530*emsize/2048;
/// ```
///
/// `emsize` is ascender + descender. Every field is a 16-bit integer, so each value
/// is truncated towards zero when it is stored, and the x offsets are computed from
/// the y offsets already truncated. The two strikeout lines are integer arithmetic
/// in C, so a 1000-unit em gives 49 and 258 rather than 49.8 and 258.8.
///
/// FontForge uses the italic angle as the source states it, a real number, but the
/// SFD reader rounds it to a whole degree, so a fractional angle gives other x
/// offsets: on a 2048-unit em, `ItalicAngle: -12.5` gives 61 and -212 in FontForge,
/// and 64 and -221 here, from -13.
///
/// # Opt-in, because it is a statement about who exported the binary
///
/// A binary not exported by FontForge has different values, and this filter would
/// move a conversion further from it. Styles of one family may have been exported by
/// different tools, so the choice is made per source.
///
/// When the source states `OS2SubXSize`, none of the ten is changed.
///
/// # Not modelled: a source that does not set `pfmset`
///
/// Any of the lines `PfmFamily:`, `TTFWeight:`, `PfmWeight:`, `TTFWidth:`, `LineGap:`
/// and `VLineGap:` sets FontForge's `pfmset`. Without one, `SFDefaultOS2Info` resets
/// `pfminfo`, FontForge's record of the OS/2 values, before filling it, so the
/// exporter computes the ten values even when the source states `OS2SubXSize`. This
/// filter keeps them. FontForge's Font Info dialog sets `pfmset` whenever it stores
/// the ten, so only a source edited by hand or written by another tool is affected.
pub struct FontForgeOs2Defaults;

impl FontForgeOs2Defaults {
    /// Create a new FontForgeOs2Defaults filter
    pub fn new() -> Self {
        FontForgeOs2Defaults
    }
}

impl FontFilter for FontForgeOs2Defaults {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let mut filled = 0;
        for master in font.masters.iter_mut() {
            // `subsuper_set` comes from the `OS2SubXSize:` line alone; without it,
            // `SFDefaultOS2SubSuper` writes all ten.
            if master.metrics.contains_key(&MetricType::SubscriptXSize) {
                continue;
            }
            let ascender = master.metrics.get(&MetricType::Ascender).copied();
            let descender = master.metrics.get(&MetricType::Descender).copied();
            let (Some(a), Some(d)) = (ascender, descender) else {
                continue;
            };
            // FontForge's emsize is ascent + descent, and its descent is a positive
            // number below the baseline where babelfont's is negative.
            let em = a - d;
            if em <= 0 {
                continue;
            }
            let em_f = f64::from(em);
            let italic = master
                .metrics
                .get(&MetricType::ItalicAngle)
                .copied()
                .unwrap_or(0);
            // babelfont holds the italic angle in the opposite sense to the `post`
            // table and the SFD, and FontForge's formula is written against its own
            // sense, so the angle is negated here.
            let s = (f64::from(-italic) * std::f64::consts::PI / 180.0).sin();
            // Stored in 16-bit fields before the x offsets are derived from them.
            let subyoff = (0.14 * em_f) as i32;
            let supyoff = (0.48 * em_f) as i32;
            let value = |m: &MetricType| -> i32 {
                match m {
                    MetricType::SubscriptXSize | MetricType::SuperscriptXSize => {
                        (0.65 * em_f) as i32
                    }
                    MetricType::SubscriptYSize | MetricType::SuperscriptYSize => {
                        (0.7 * em_f) as i32
                    }
                    MetricType::SubscriptYOffset => subyoff,
                    MetricType::SuperscriptYOffset => supyoff,
                    MetricType::SubscriptXOffset => (-s * f64::from(subyoff)) as i32,
                    MetricType::SuperscriptXOffset => (s * f64::from(supyoff)) as i32,
                    // Integer arithmetic, as in the C: 102*em/2048, not 0.0498*em.
                    MetricType::StrikeoutSize => (102 * em) / 2048,
                    MetricType::StrikeoutPosition => (530 * em) / 2048,
                    _ => 0,
                }
            };
            for m in SUBSUPER.iter().chain(STRIKEOUT.iter()) {
                master.metrics.insert(m.clone(), value(m));
                filled += 1;
            }
        }
        log::info!(
            "Filled {filled} OS/2 sub/superscript and strikeout metric(s) using \
             FontForge's SFDefaultOS2SubSuper rule"
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeOs2Defaults::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgeos2defaults")
            .long("fontforge-os2-defaults")
            .help(
                "Compute all ten OS/2 sub/superscript and strikeout metrics of a FontForge \
                 source that states no OS2SubXSize, replacing any it states, using the rule \
                 FontForge's exporter uses (SFDefaultOS2SubSuper in tottf.c). Use only to \
                 reproduce a binary FontForge exported; a binary built by another compiler \
                 carries different values",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Master};

    fn font_with(ascender: i32, descender: i32, italic: Option<i32>) -> Font {
        let mut m = Master::default();
        m.metrics.insert(MetricType::Ascender, ascender);
        m.metrics.insert(MetricType::Descender, descender);
        if let Some(i) = italic {
            m.metrics.insert(MetricType::ItalicAngle, i);
        }
        let mut f = Font::new();
        f.masters.push(m);
        f
    }

    fn got(f: &Font, m: MetricType) -> Option<i32> {
        f.masters[0].metrics.get(&m).copied()
    }

    #[test]
    fn test_matches_fontforge_on_a_2048_em() {
        // Ascender 1638, descender -410: a 2048-unit em.
        let mut f = font_with(1638, -410, None);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::SubscriptXSize), Some(1331));
        assert_eq!(got(&f, MetricType::SubscriptYSize), Some(1433));
        assert_eq!(got(&f, MetricType::SubscriptYOffset), Some(286));
        assert_eq!(got(&f, MetricType::SuperscriptYSize), Some(1433));
        assert_eq!(got(&f, MetricType::SuperscriptYOffset), Some(983));
        assert_eq!(got(&f, MetricType::StrikeoutSize), Some(102));
        assert_eq!(got(&f, MetricType::StrikeoutPosition), Some(530));
    }

    #[test]
    fn test_strikeout_is_integer_arithmetic() {
        // 102*1000/2048 is 49.8. FontForge does this in C integer arithmetic and
        // gets 49; computing 0.0498*em and rounding would give 50.
        let mut f = font_with(800, -200, None);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::StrikeoutSize), Some(49));
        assert_eq!(got(&f, MetricType::StrikeoutPosition), Some(258));
    }

    #[test]
    fn test_italic_angle_slants_the_offsets() {
        // supxoff = s*supyoff, subxoff = -s*subyoff with s = sin(italic_angle). A
        // roman gets zero for both; an italic does not.
        let mut roman = font_with(1638, -410, None);
        FontForgeOs2Defaults::new().apply(&mut roman).unwrap();
        assert_eq!(got(&roman, MetricType::SuperscriptXOffset), Some(0));
        assert_eq!(got(&roman, MetricType::SubscriptXOffset), Some(0));

        // post.italicAngle -12 gives subscriptXOffset +59 and superscriptXOffset
        // -204. babelfont's internal angle has the opposite sign to post's, so that
        // is +12 here.
        let mut italic = font_with(1638, -410, Some(12));
        FontForgeOs2Defaults::new().apply(&mut italic).unwrap();
        assert_eq!(got(&italic, MetricType::SubscriptXOffset), Some(59));
        assert_eq!(got(&italic, MetricType::SuperscriptXOffset), Some(-204));
    }

    #[test]
    fn test_the_x_offsets_come_from_the_truncated_y_offsets() {
        // A 2048 em stores subyoff 286 and supyoff 983, not 286.72 and 983.04.
        // sin(20 degrees) * 286 is 97.8, which truncates to 97; from 286.72 it would
        // be 98.06 and give 98.
        let mut f = font_with(1638, -410, Some(20));
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::SubscriptXOffset), Some(97));
        assert_eq!(got(&f, MetricType::SuperscriptXOffset), Some(-336));
    }

    #[test]
    fn test_a_stated_subscript_x_size_keeps_all_ten() {
        let mut f = font_with(1638, -410, None);
        f.masters[0].metrics.insert(MetricType::SubscriptXSize, 1300);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::SubscriptXSize), Some(1300));
        assert_eq!(got(&f, MetricType::SubscriptYSize), None);
        assert_eq!(got(&f, MetricType::StrikeoutPosition), None);
    }

    #[test]
    fn test_without_a_subscript_x_size_all_ten_are_computed() {
        // A strikeout size stated without OS2SubXSize does not set subsuper_set, so
        // the exporter replaces it with the computed one.
        let mut f = font_with(1638, -410, None);
        f.masters[0].metrics.insert(MetricType::StrikeoutSize, 77);
        FontForgeOs2Defaults::new().apply(&mut f).unwrap();
        assert_eq!(got(&f, MetricType::StrikeoutSize), Some(102));
        assert_eq!(got(&f, MetricType::StrikeoutPosition), Some(530));
        assert_eq!(got(&f, MetricType::SubscriptXSize), Some(1331));
    }
}
