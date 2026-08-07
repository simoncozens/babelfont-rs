use crate::filters::FontFilter;

/// A filter that restores the duplicate cmap entries makeotf used to add.
///
/// makeotf mapped a small fixed set of codepoints onto glyphs that already
/// carried a different one -- U+00A0 onto `space`, U+00AD onto `hyphen`, and so
/// on. The glyph is the same; only the extra mapping is missing. FontForge does
/// the same thing at export time for the no-break space.
///
/// A convertor that reads only what the source file states drops those, and the
/// characters render as `.notdef` even though the right glyph is present. U+00A0
/// is the one that matters in practice: it appears in ordinary web text, and a
/// font without it breaks a space that was meant to be unbreakable.
///
/// Deliberately conservative on both sides:
///
///   * the codepoint is only added when it is **not already mapped** anywhere in
///     the font, so a source that assigns it deliberately always wins;
///   * the target glyph must **already exist**, so nothing is invented.
#[derive(Default)]
pub struct LegacyDuplicateCmap;

impl LegacyDuplicateCmap {
    /// Create a new LegacyDuplicateCmap filter
    pub fn new() -> Self {
        LegacyDuplicateCmap
    }
}

/// The duplicate mappings makeotf applied, as (codepoint, target glyph name).
///
/// This is the AFDKO set. It is short on purpose: every entry is a case where
/// the two codepoints are genuinely the same glyph, not merely similar.
const DUPLICATES: &[(u32, &str)] = &[
    (0x00A0, "space"),          // no-break space
    (0x02C9, "macron"),         // modifier letter macron
    (0x03BC, "mu"),             // greek small letter mu
    (0x2126, "Omega"),          // ohm sign
    (0x2206, "Delta"),          // increment
    (0x2219, "periodcentered"), // bullet operator
];

// U+00AD SOFT HYPHEN is deliberately NOT here, though makeotf did add it. It is
// a formatting character rather than a glyph, and font QA rejects encoding it:
// fontspector's `soft_hyphen` warns on any font that has one.


impl FontFilter for LegacyDuplicateCmap {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let already_mapped: std::collections::HashSet<u32> = font
            .glyphs
            .iter()
            .flat_map(|g| g.codepoints.iter().copied())
            .collect();

        for (codepoint, target) in DUPLICATES {
            if already_mapped.contains(codepoint) {
                continue;
            }
            if let Some(glyph) = font.glyphs.iter_mut().find(|g| g.name == *target) {
                glyph.codepoints.push(*codepoint);
                log::info!("Added the legacy duplicate mapping U+{codepoint:04X} -> {target}");
            }
        }

        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(LegacyDuplicateCmap::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("legacyduplicatecmap")
            .long("add-legacy-duplicate-cmap")
            .help(
                "Add the duplicate cmap entries makeotf used to synthesise \
                 (U+00A0 -> space, U+00AD -> hyphen, ...)",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Glyph};

    fn glyph(name: &str, codepoints: Vec<u32>) -> Glyph {
        Glyph {
            name: name.into(),
            codepoints,
            ..Default::default()
        }
    }

    #[test]
    fn the_duplicate_is_added_when_the_glyph_exists() {
        let mut font = Font::new();
        font.glyphs.push(glyph("space", vec![0x0020]));
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();

        let space = font.glyphs.iter().find(|g| g.name == "space").unwrap();
        assert!(space.codepoints.contains(&0x0020), "original kept");
        assert!(space.codepoints.contains(&0x00A0), "duplicate added");
    }

    #[test]
    fn nothing_is_invented_when_the_glyph_is_absent() {
        let mut font = Font::new();
        font.glyphs.push(glyph("A", vec![0x0041]));
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();

        assert_eq!(font.glyphs.len(), 1, "no glyph created");
        let a = font.glyphs.iter().find(|g| g.name == "A").unwrap();
        assert_eq!(a.codepoints, vec![0x0041], "unrelated glyph untouched");
    }

    #[test]
    fn a_codepoint_the_source_already_assigns_is_left_alone() {
        // The source deliberately gives U+00A0 its own glyph. Adding it to
        // `space` as well would map one codepoint to two glyphs.
        let mut font = Font::new();
        font.glyphs.push(glyph("space", vec![0x0020]));
        font.glyphs.push(glyph("uni00A0", vec![0x00A0]));
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();

        let space = font.glyphs.iter().find(|g| g.name == "space").unwrap();
        assert_eq!(space.codepoints, vec![0x0020], "space not given U+00A0");
        let nbsp = font.glyphs.iter().find(|g| g.name == "uni00A0").unwrap();
        assert_eq!(nbsp.codepoints, vec![0x00A0]);
    }

    fn glyph_with_width(name: &str, codepoints: Vec<u32>, width: f32) -> Glyph {
        let mut g = glyph(name, codepoints);
        g.layers.push(crate::Layer::new(width));
        g
    }

    #[test]
    fn a_separate_nbsp_keeps_its_own_width() {
        // This filter restores coverage; it must NOT change advances. A source
        // that states its own no-break-space width means it. Normalising the
        // width is a correction and lives behind --normalise-nbsp-width.
        let mut font = Font::new();
        font.glyphs.push(glyph_with_width("space", vec![0x0020], 616.0));
        font.glyphs
            .push(glyph_with_width("uni00A0", vec![0x00A0], 720.0));
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();

        let nbsp = font.glyphs.iter().find(|g| g.name == "uni00A0").unwrap();
        assert_eq!(
            nbsp.layers[0].width, 720.0,
            "the source's no-break-space advance must survive this filter"
        );
        let space = font.glyphs.iter().find(|g| g.name == "space").unwrap();
        assert_eq!(space.layers[0].width, 616.0, "space must not move");
    }

    #[test]
    fn the_width_is_left_alone_when_there_is_nothing_to_reconcile() {
        // U+00A0 on the space glyph itself: one glyph, one width, nothing to do.
        let mut font = Font::new();
        font.glyphs
            .push(glyph_with_width("space", vec![0x0020, 0x00A0], 616.0));
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();
        let space = font.glyphs.iter().find(|g| g.name == "space").unwrap();
        assert_eq!(space.layers[0].width, 616.0);

        // No space glyph at all: nothing to copy from.
        let mut font2 = Font::new();
        font2
            .glyphs
            .push(glyph_with_width("uni00A0", vec![0x00A0], 720.0));
        LegacyDuplicateCmap::new().apply(&mut font2).unwrap();
        let nbsp = font2.glyphs.iter().find(|g| g.name == "uni00A0").unwrap();
        assert_eq!(nbsp.layers[0].width, 720.0, "unchanged with no reference");
    }

    #[test]
    fn the_soft_hyphen_is_never_added() {
        // U+00AD is a formatting character, not a glyph; font QA warns on any
        // font that encodes one, so it stays out of the table even though
        // makeotf added it.
        let mut font = Font::new();
        font.glyphs.push(glyph("hyphen", vec![0x002D]));
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();

        let hyphen = font.glyphs.iter().find(|g| g.name == "hyphen").unwrap();
        assert_eq!(hyphen.codepoints, vec![0x002D], "U+00AD must not be added");
        assert!(!DUPLICATES.iter().any(|(cp, _)| *cp == 0x00AD));
    }

    #[test]
    fn every_duplicate_in_the_table_is_applied() {
        let mut font = Font::new();
        for (_, target) in DUPLICATES {
            font.glyphs.push(glyph(target, vec![]));
        }
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();

        for (codepoint, target) in DUPLICATES {
            let g = font.glyphs.iter().find(|g| g.name == *target).unwrap();
            assert!(
                g.codepoints.contains(codepoint),
                "U+{codepoint:04X} was not added to {target}"
            );
        }
    }

    #[test]
    fn running_it_twice_does_not_duplicate_the_codepoint() {
        let mut font = Font::new();
        font.glyphs.push(glyph("space", vec![0x0020]));
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();
        LegacyDuplicateCmap::new().apply(&mut font).unwrap();

        let space = font.glyphs.iter().find(|g| g.name == "space").unwrap();
        assert_eq!(
            space.codepoints.iter().filter(|c| **c == 0x00A0).count(),
            1,
            "U+00A0 added twice"
        );
    }
}
