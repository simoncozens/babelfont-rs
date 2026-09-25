use crate::filters::FontFilter;
use crate::MetricType;

#[derive(Default)]
/// A filter that converts `UnderlinePosition` to `post.underlinePosition` the way
/// FontForge up to tag 20170731 did.
///
/// FontForge's `UnderlinePosition` names the centre of the underline; the `post`
/// table's `underlinePosition` names its top. FontForge's exporter applies the offset
/// itself, so a binary it exported does not carry the source's raw value.
///
/// Which offset it applied depends on the FontForge that exported the binary.
/// `fontforge/tottf.c`, `dumppost()`, computed `upos - uwidth/2` up to tag 20170731
/// and `upos + uwidth/2` from 20190317 on; the sign was reversed in commit `9f667c9c`
/// (fontforge/fontforge#3389, "Fix the direction of the underline position correction
/// for TTF output"). This filter applies the older rule: `UnderlinePosition: -50` with
/// `UnderlineWidth: 50` becomes -75, the underline's bottom edge rather than the top
/// the `post` table defines. The two values are real numbers there, and
/// `putshort` truncates the difference towards zero, so position 30 with width 51
/// gives 4. A binary exported by FontForge 20190317 or later needs the opposite sign,
/// so this filter does not match one.
///
/// It is opt-in for that reason: there is no single correct offset, only the one of
/// the FontForge that exported the binary, whatever the export date.
///
/// FontForge reads both lines as real numbers, but the SFD reader keeps only whole
/// ones and drops a fractional value such as `UnderlinePosition: -51.2`. This filter
/// then leaves a master with a fractional position alone, and counts a fractional
/// width as zero.
pub struct FontForgeUnderlinePosition;

impl FontForgeUnderlinePosition {
    /// Create a new FontForgeUnderlinePosition filter
    pub fn new() -> Self {
        FontForgeUnderlinePosition
    }
}

impl FontFilter for FontForgeUnderlinePosition {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let mut adjusted = 0;
        for master in font.masters.iter_mut() {
            let Some(&position) = master.metrics.get(&MetricType::UnderlinePosition) else {
                continue;
            };
            let thickness = master
                .metrics
                .get(&MetricType::UnderlineThickness)
                .copied()
                .unwrap_or(0);
            // `putshort(at->post, sf->upos - sf->uwidth/2)`: real arithmetic, then
            // truncation towards zero.
            let exported = (f64::from(position) - f64::from(thickness) / 2.0) as i32;
            master.metrics.insert(MetricType::UnderlinePosition, exported);
            adjusted += 1;
        }
        log::info!(
            "Moved underlinePosition down by half the thickness on {adjusted} master(s), \
             as FontForge's TTF exporter did up to tag 20170731 (position - thickness/2)"
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeUnderlinePosition::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgeunderlineposition")
            .long("fontforge-underline-position")
            .help(
                "Apply the offset FontForge's TTF exporter applied up to tag 20170731 to a \
                 FontForge source's UnderlinePosition (the underline's centre): position - \
                 thickness/2, which is the underline's bottom edge rather than the top the post \
                 table defines. FontForge reversed the sign in commit 9f667c9c (2018-12-26, \
                 merged 2018-12-28; tag 20190317 is the first to carry it), so use this only for \
                 a binary exported by an older FontForge, whatever the export date",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Master};

    fn font_with(position: i32, thickness: i32) -> Font {
        let mut master = Master::default();
        master
            .metrics
            .insert(MetricType::UnderlinePosition, position);
        master
            .metrics
            .insert(MetricType::UnderlineThickness, thickness);
        let mut font = Font::new();
        font.masters.push(master);
        font
    }

    fn position_of(font: &Font) -> Option<i32> {
        font.masters[0]
            .metrics
            .get(&MetricType::UnderlinePosition)
            .copied()
    }

    #[test]
    fn test_the_centre_moves_down_by_half_the_thickness() {
        // UnderlinePosition -50 with UnderlineWidth 50 exports as
        // post.underlinePosition -75.
        let mut font = font_with(-50, 50);
        FontForgeUnderlinePosition::new().apply(&mut font).unwrap();
        assert_eq!(position_of(&font), Some(-75));
    }

    #[test]
    fn test_the_difference_is_truncated_towards_zero() {
        // 30 - 51/2 is 4.5 in real arithmetic, and the conversion to a short gives 4.
        // Integer division would give 30 - 25 = 5.
        let mut font = font_with(30, 51);
        FontForgeUnderlinePosition::new().apply(&mut font).unwrap();
        assert_eq!(position_of(&font), Some(4));

        // A negative result truncates towards zero too: -50 - 25.5 is -75.5, so -75.
        let mut font = font_with(-50, 51);
        FontForgeUnderlinePosition::new().apply(&mut font).unwrap();
        assert_eq!(position_of(&font), Some(-75));
    }

    #[test]
    fn test_a_master_with_no_underline_position_is_left_alone() {
        let mut font = Font::new();
        let mut master = Master::default();
        master.metrics.insert(MetricType::UnderlineThickness, 50);
        font.masters.push(master);
        FontForgeUnderlinePosition::new().apply(&mut font).unwrap();
        assert_eq!(position_of(&font), None);
    }

    #[test]
    fn test_a_missing_thickness_counts_as_zero() {
        // The offset is half the thickness, so with no thickness there is no offset.
        let mut font = Font::new();
        let mut master = Master::default();
        master.metrics.insert(MetricType::UnderlinePosition, -50);
        font.masters.push(master);
        FontForgeUnderlinePosition::new().apply(&mut font).unwrap();
        assert_eq!(position_of(&font), Some(-50));
    }

    #[test]
    fn test_applying_it_twice_is_not_the_same_as_once() {
        // It is a conversion, not a normalisation: it has no fixed point, which is
        // why it must be opt-in and applied exactly once.
        let mut font = font_with(-50, 50);
        let filter = FontForgeUnderlinePosition::new();
        filter.apply(&mut font).unwrap();
        filter.apply(&mut font).unwrap();
        assert_eq!(position_of(&font), Some(-100));
    }
}
