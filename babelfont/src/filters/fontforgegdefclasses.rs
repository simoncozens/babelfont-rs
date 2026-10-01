use crate::features::PossiblyAutomaticCode;
use crate::filters::FontFilter;

/// A filter that writes a FontForge source's GDEF glyph classes as an explicit
/// `table GDEF { GlyphClassDef ... } GDEF;`, the way FontForge's exporter classed them.
///
/// FontForge classes every glyph: an explicit `GlyphClass` wins; otherwise `.notdef`
/// has no class, a glyph whose first non-cursive anchor is a mark anchor is a mark,
/// a ligature substitution's result is a ligature, and anything else is a base. A
/// compiler that infers classes from categories, anchors or names does not give the
/// same table: it has no way to say "base" for a glyph without anchors, and may make
/// an unclassed accent a mark by its name.
///
/// FontForge writes no class table when no glyph has an explicit class and every glyph
/// is a base (a font without `.notdef`); neither does this filter. Glyphs whose names
/// FEA cannot spell are left out.
///
/// The classes come from the SFD reader, so this does nothing for other sources.
/// It is opt-in because the table is fixed: glyphs added later must be added to it.
#[derive(Default)]
pub struct FontForgeGdefClasses;

impl FontForgeGdefClasses {
    /// Create a new FontForgeGdefClasses filter
    pub fn new() -> Self {
        FontForgeGdefClasses
    }
}

impl FontFilter for FontForgeGdefClasses {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let mut base = vec![];
        let mut ligature = vec![];
        let mut mark = vec![];
        let mut component = vec![];
        let mut recorded = false;
        let mut needs_class = false;
        for glyph in font.glyphs.iter().filter(|g| g.exported) {
            let Some(class) = glyph
                .format_specific
                .get("sfd.gdef_class")
                .and_then(|v| v.as_u64())
            else {
                continue;
            };
            recorded = true;
            if class != 1 || glyph.format_specific.contains_key("sfd.gdef_class_explicit") {
                needs_class = true;
            }
            let name = glyph.name.as_str();
            if !fea_can_name(name) {
                continue;
            }
            match class {
                1 => base.push(name),
                2 => ligature.push(name),
                3 => mark.push(name),
                4 => component.push(name),
                _ => {}
            }
        }
        if !recorded {
            log::warn!("No FontForge glyph classes were recorded; not writing a GDEF table");
            return Ok(());
        }
        if !needs_class {
            return Ok(());
        }
        let class = |names: &[&str]| {
            if names.is_empty() {
                String::new()
            } else {
                format!("[{}]", names.join(" "))
            }
        };
        let code = format!(
            "table GDEF {{\n    GlyphClassDef {}, {}, {}, {};\n}} GDEF;",
            class(&base),
            class(&ligature),
            class(&mark),
            class(&component)
        );
        font.features.prefixes.insert(
            "FontForgeGlyphClasses".into(),
            PossiblyAutomaticCode {
                code,
                ..Default::default()
            },
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeGdefClasses::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgegdefclasses")
            .long("fontforge-gdef-classes")
            .help(
                "Write a FontForge source's GDEF glyph classes as FEA, as FontForge's \
                 exporter classed them (explicit GlyphClass, else mark by anchor, \
                 ligature by substitution, base otherwise)",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

/// Whether a feature file can spell the glyph name (the OpenType feature file syntax,
/// plus `.null`).
fn fea_can_name(name: &str) -> bool {
    if name == ".notdef" || name == ".null" {
        return true;
    }
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b"._*+:^|~-".contains(&b))
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(all(test, feature = "fontforge"))]
mod tests {
    use super::*;

    fn glyph(name: &str, gid: usize, lines: &str) -> String {
        format!("StartChar: {name}\nEncoding: {gid} -1 {gid}\nWidth: 500\n{lines}EndChar\n")
    }

    #[test]
    fn test_classes_follow_fontforge() {
        let sfd = [
            "SplineFontDB: 3.0\nFontName: T\nAscent: 800\nDescent: 200\n".to_string(),
            "Lookup: 4 0 0 \"liga\" {\"liga-1\"} ['liga' ('DFLT' <'dflt' > ) ]\n".to_string(),
            "Lookup: 260 0 0 \"mark\" {\"mark-1\"} ['mark' ('DFLT' <'dflt' > ) ]\n".to_string(),
            "Lookup: 259 0 0 \"curs\" {\"curs-1\"} ['curs' ('DFLT' <'dflt' > ) ]\n".to_string(),
            "AnchorClass2: \"top\" \"mark-1\" \"cursive\" \"curs-1\"\n".to_string(),
            "BeginChars: 8 8\n".to_string(),
            glyph(".notdef", 0, ""),
            // No anchors and no class: a base, whatever its name says.
            glyph("dieresiscomb", 1, ""),
            glyph("a", 2, "AnchorPoint: \"top\" 250 500 basechar 0\n"),
            glyph("acutecomb", 3, "AnchorPoint: \"top\" 0 500 mark 0\n"),
            // A cursive anchor before the mark anchor does not count.
            glyph(
                "tickcomb",
                4,
                "AnchorPoint: \"cursive\" 0 0 entry 0\nAnchorPoint: \"top\" 0 500 mark 0\n",
            ),
            glyph("f_i", 5, "Ligature2: \"liga-1\" f i\n"),
            // An explicit class wins over the anchor.
            glyph(
                "gravecomb",
                6,
                "GlyphClass: 2\nAnchorPoint: \"top\" 0 500 mark 0\n",
            ),
            glyph("ring", 7, "GlyphClass: 4\n"),
            "EndChars\nEndSplineFont\n".to_string(),
        ]
        .concat();
        let mut font = crate::convertors::fontforge::load_str(&sfd).unwrap();
        FontForgeGdefClasses::new().apply(&mut font).unwrap();
        assert_eq!(
            font.features.prefixes["FontForgeGlyphClasses"].code,
            "table GDEF {\n    GlyphClassDef [dieresiscomb a gravecomb], [f_i], \
             [acutecomb tickcomb ring], ;\n} GDEF;"
        );
    }

    #[test]
    fn test_no_table_when_everything_is_a_base() {
        let sfd = [
            "SplineFontDB: 3.0\nFontName: T\nAscent: 800\nDescent: 200\n".to_string(),
            "BeginChars: 2 2\n".to_string(),
            glyph("a", 0, ""),
            glyph("dieresiscomb", 1, ""),
            "EndChars\nEndSplineFont\n".to_string(),
        ]
        .concat();
        let mut font = crate::convertors::fontforge::load_str(&sfd).unwrap();
        FontForgeGdefClasses::new().apply(&mut font).unwrap();
        assert!(!font.features.prefixes.contains_key("FontForgeGlyphClasses"));
    }

    #[test]
    fn test_names_fea_cannot_spell_are_left_out() {
        let sfd = [
            "SplineFontDB: 3.0\nFontName: T\nAscent: 800\nDescent: 200\n".to_string(),
            "BeginChars: 3 3\n".to_string(),
            glyph(".notdef", 0, ""),
            glyph("a", 1, ""),
            glyph(".ttfautohint", 2, ""),
            "EndChars\nEndSplineFont\n".to_string(),
        ]
        .concat();
        let mut font = crate::convertors::fontforge::load_str(&sfd).unwrap();
        FontForgeGdefClasses::new().apply(&mut font).unwrap();
        assert_eq!(
            font.features.prefixes["FontForgeGlyphClasses"].code,
            "table GDEF {\n    GlyphClassDef [a], , , ;\n} GDEF;"
        );
    }
}
