use crate::{Font, Path};

/// The glyphs a font draws with cubic curves, and those it draws with
/// quadratic ones, a few of each
pub(crate) struct CurveKinds {
    pub(crate) cubic: Vec<String>,
    pub(crate) quadratic: Vec<String>,
}

const EXAMPLES: usize = 3;

/// Which glyphs are drawn with which curves, in layers that are compiled
/// (masters and intermediate layers)
pub(crate) fn curve_kinds(font: &Font) -> CurveKinds {
    let mut kinds = CurveKinds {
        cubic: vec![],
        quadratic: vec![],
    };
    for glyph in font.glyphs.iter().filter(|g| g.exported) {
        let paths = || {
            glyph
                .layers
                .iter()
                .filter(|l| l.should_interpolate())
                .flat_map(|l| l.paths())
        };
        if paths().any(Path::is_cubic) {
            kinds.cubic.push(glyph.name.to_string());
        }
        if paths().any(Path::is_quadratic) {
            kinds.quadratic.push(glyph.name.to_string());
        }
    }
    kinds
}

/// A font is meant to be drawn with one kind of curve: a compiled font's glyf
/// table holds quadratic curves only, and converting a font as a whole keeps
/// its curves alike. A font that mixes them (Glyphs allows it, path by path)
/// still compiles, each glyph converted on its own: this says which glyphs, so
/// a designer can convert the font to one curve type.
pub(crate) fn mixed_curve_kinds(font: &Font) -> Option<String> {
    let kinds = curve_kinds(font);
    if kinds.cubic.is_empty() || kinds.quadratic.is_empty() {
        return None;
    }
    let examples = |names: &[String]| {
        let mut shown = names
            .iter()
            .take(EXAMPLES)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if names.len() > EXAMPLES {
            shown.push_str(&format!(" and {} more", names.len() - EXAMPLES));
        }
        shown
    };
    Some(format!(
        "The font mixes cubic and quadratic curves: cubic in {}; quadratic in {}. \
         Convert the font to one curve type",
        examples(&kinds.cubic),
        examples(&kinds.quadratic)
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_mixed_curve_kinds_name_their_glyphs() {
        // Sepehr Bold, as Glyphs has it, draws some glyphs with cubic curves and
        // most with quadratic ones
        let font = crate::load("resources/fontlab/Sepehr-Bold.glyphs").unwrap();
        let message = mixed_curve_kinds(&font).unwrap();
        assert!(
            message.contains("cubic in afii57388, uni0615, uni061B and 92 more"),
            "{message}"
        );
        assert!(
            message.contains("quadratic in exclam, dollar, percent and 112 more"),
            "{message}"
        );
        // Fustat's quadratic curves are in backup layers, which are not compiled
        let font = crate::load("resources/Fustat.glyphs").unwrap();
        assert_eq!(mixed_curve_kinds(&font), None);
        let font = crate::load("resources/ttf/Quadratic.ttf").unwrap();
        assert_eq!(mixed_curve_kinds(&font), None);
        let font = crate::load("resources/Test1.ufo").unwrap();
        assert_eq!(mixed_curve_kinds(&font), None);
        // quadratic; its cubic "Old_Fore" layer is not a master's
        let font = crate::load("resources/fontforge/Glegoo-Regular.sfd").unwrap();
        assert_eq!(mixed_curve_kinds(&font), None);
    }
}
