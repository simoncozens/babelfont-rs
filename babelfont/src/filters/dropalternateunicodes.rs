use crate::filters::FontFilter;

/// The sentinel FontForge writes in an `AltUni2` triple's variation-selector field
/// when the entry is a plain alternate encoding rather than a Unicode Variation
/// Sequence.
const NO_VARIATION_SELECTOR: u32 = 0xffff_ffff;

#[derive(Default)]
/// A filter that drops the alternate Unicode mappings FontForge records in `AltUni2`.
///
/// A FontForge source may map one glyph at several codepoints: `Omega` (U+03A9) also
/// at U+2126 OHM SIGN, `periodcentered` (U+00B7) also at U+2219 BULLET OPERATOR, `mu`
/// (U+00B5 MICRO SIGN) also at U+03BC GREEK SMALL LETTER MU. The `Encoding` line
/// states the glyph's primary codepoint and `AltUni2` the alternates. This filter
/// keeps only each glyph's `Encoding` codepoint. A glyph with no `Encoding` codepoint
/// keeps its first one, so that no glyph is left unmapped.
///
/// FontForge's own TTF export maps the alternates too, so this filter does not
/// reproduce it. It is for a target whose cmap does not carry them, such as a binary
/// built without them that the conversion has to match. It works the other way from
/// [`super::LegacyDuplicateCmap`] (`--add-legacy-duplicate-cmap`), which maps a fixed
/// set of such codepoints, U+03BC, U+2126 and U+2219 among them, onto glyphs found by
/// name; this filter removes only what the source's `AltUni2` lines add.
///
/// Variation-sequence entries (a real variation selector rather than the
/// `0xffffffff` sentinel) are left alone: they describe cmap format 14 records, not
/// plain codepoints, and are not added to `codepoints` in the first place.
pub struct DropAlternateUnicodes;

impl DropAlternateUnicodes {
    /// Create a new DropAlternateUnicodes filter
    pub fn new() -> Self {
        DropAlternateUnicodes
    }
}

/// The plain alternate codepoints recorded in one `AltUni2` value.
fn alternates(raw: &str) -> Vec<u32> {
    raw.split_whitespace()
        .filter_map(|entry| {
            let mut fields = entry.split('.');
            let codepoint = fields.next().and_then(|s| u32::from_str_radix(s, 16).ok())?;
            let selector = fields.next().and_then(|s| u32::from_str_radix(s, 16).ok())?;
            (selector == NO_VARIATION_SELECTOR).then_some(codepoint)
        })
        .collect()
}

impl FontFilter for DropAlternateUnicodes {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let mut dropped = 0;
        for glyph in font.glyphs.iter_mut() {
            let Some(raw) = glyph.format_specific.get("sfd.altuni").and_then(|v| v.as_str())
            else {
                continue;
            };
            let alternates = alternates(raw);
            if alternates.is_empty() {
                continue;
            }
            // The primary codepoint is the one the `Encoding` line states; it is kept
            // even when `AltUni2` repeats it.
            let primary = glyph
                .format_specific
                .get("sfd.encoding_unicode")
                .and_then(|v| v.as_i64())
                .and_then(|cp| u32::try_from(cp).ok());
            let before = glyph.codepoints.len();
            let first = glyph.codepoints.first().copied();
            glyph
                .codepoints
                .retain(|c| Some(*c) == primary || !alternates.contains(c));
            // A glyph with no `Encoding` codepoint keeps its first one.
            if glyph.codepoints.is_empty() {
                glyph.codepoints.extend(first);
            }
            dropped += before - glyph.codepoints.len();
        }
        log::info!("Dropped {dropped} alternate Unicode mapping(s) recorded in AltUni2");
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(DropAlternateUnicodes::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("dropalternateunicodes")
            .long("drop-alternate-unicodes")
            .help(
                "Drop the alternate cmap entries a FontForge source records in AltUni2 \
                 (U+2126 ohm, U+2219 bullet operator, U+03BC Greek mu, ...) and keep \
                 each glyph's Encoding codepoint. FontForge's own TTF export maps the \
                 alternates; use this for a cmap that does not carry them \
                 (--add-legacy-duplicate-cmap adds a fixed set of such entries)",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Glyph};
    use serde_json::json;

    fn glyph(encoding: i64, codepoints: Vec<u32>, altuni: &str) -> Glyph {
        let mut glyph = Glyph {
            name: "g".into(),
            codepoints,
            ..Default::default()
        };
        glyph
            .format_specific
            .insert("sfd.encoding_unicode".into(), json!(encoding));
        glyph
            .format_specific
            .insert("sfd.altuni".into(), json!(altuni));
        glyph
    }

    fn filtered(glyph: Glyph) -> Vec<u32> {
        let mut font = Font::new();
        font.glyphs.0.push(glyph);
        DropAlternateUnicodes::new().apply(&mut font).unwrap();
        font.glyphs.0[0].codepoints.clone()
    }

    #[test]
    fn test_the_encoding_codepoint_is_kept_wherever_it_sits() {
        assert_eq!(
            filtered(glyph(
                0xB5,
                vec![0x03BC, 0xB5],
                "0003bc.ffffffff.0 0000b5.ffffffff.0"
            )),
            vec![0xB5]
        );
    }

    #[test]
    fn test_an_encoding_codepoint_repeated_in_altuni_is_kept() {
        assert_eq!(
            filtered(glyph(
                0x20,
                vec![0x20, 0xA0],
                "000020.ffffffff.0 0000a0.ffffffff.0"
            )),
            vec![0x20]
        );
    }

    #[test]
    fn test_a_glyph_with_no_encoding_codepoint_keeps_its_first() {
        assert_eq!(
            filtered(glyph(
                -1,
                vec![0x03BC, 0x2126],
                "0003bc.ffffffff.0 002126.ffffffff.0"
            )),
            vec![0x03BC]
        );
    }
}
