use crate::filters::fontforge_one_width::one_width;
use crate::filters::FontFilter;

#[derive(Default)]
/// A filter that marks a font fixed pitch (`post.isFixedPitch`) when FontForge's
/// exporter would: when every glyph that counts shares one advance.
///
/// A FontForge `.sfd` states no pitch. FontForge's exporter derives it from the
/// advances, so the exported binary says the font is fixed pitch although nothing in
/// the source does, and a compiler that is not told writes 0.
///
/// The glyphs that count are those the export writes, leaving out `.null`,
/// `nonmarkingreturn` and a `.notdef` with no contours; the advances are read from the
/// default master. This is the rule of FontForge releases up to tag 20230101; from
/// tag 20251009 on, zero-width glyphs are left out too.
///
/// A font with more than one advance is left as it is: 0 is what every compiler
/// writes when nothing is stated.
///
/// # Opt-in, because it is a statement about who exported the binary
///
/// Other tools leave `isFixedPitch` to what the source states, so this filter only
/// makes sense when reproducing a binary FontForge exported.
pub struct FontForgeFixedPitch;

impl FontForgeFixedPitch {
    /// Create a new FontForgeFixedPitch filter
    pub fn new() -> Self {
        FontForgeFixedPitch
    }
}

impl FontFilter for FontForgeFixedPitch {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let width = font
            .default_master()
            .or(font.masters.first())
            .and_then(|master| one_width(font, master))
            .filter(|width| *width > 0);
        if let Some(width) = width {
            font.custom_ot_values.post_is_fixed_pitch = Some(true);
            log::info!(
                "Marked the font fixed pitch: every glyph FontForge exports advances {width}"
            );
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeFixedPitch::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgefixedpitch")
            .long("fontforge-fixed-pitch")
            .help(
                "Set post.isFixedPitch when every glyph FontForge's exporter writes shares \
                 one advance, as FontForge does. Use only to reproduce a binary FontForge \
                 exported",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Glyph, Layer, LayerType, Master, Node, NodeType, Path, Shape};

    fn font() -> Font {
        let mut f = Font::new();
        f.masters.push(Master::default());
        f
    }

    /// A glyph with one triangular contour (or none) and the given advance.
    fn add_glyph(f: &mut Font, name: &str, advance: f32, contour: bool) {
        let mut layer = Layer::new(advance);
        layer.master = LayerType::DefaultForMaster(f.masters[0].id.clone());
        if contour {
            let node = |x: f64, y: f64| Node {
                x,
                y,
                nodetype: NodeType::Line,
                ..Default::default()
            };
            layer.shapes.push(Shape::Path(Path {
                nodes: vec![node(0.0, 0.0), node(100.0, 0.0), node(50.0, 100.0)],
                closed: true,
                ..Default::default()
            }));
        }
        let mut glyph = Glyph::new(name);
        glyph.layers.push(layer);
        f.glyphs.push(glyph);
    }

    #[test]
    fn test_one_shared_advance_marks_the_font_fixed_pitch() {
        let mut f = font();
        add_glyph(&mut f, "a", 1150.0, true);
        add_glyph(&mut f, "b", 1150.0, true);
        // Left out: an empty .notdef of another advance.
        add_glyph(&mut f, ".notdef", 500.0, false);
        FontForgeFixedPitch::new().apply(&mut f).unwrap();
        assert_eq!(f.custom_ot_values.post_is_fixed_pitch, Some(true));
    }

    #[test]
    fn test_two_advances_leave_the_font_alone() {
        let mut f = font();
        add_glyph(&mut f, "a", 1150.0, true);
        add_glyph(&mut f, "b", 600.0, true);
        FontForgeFixedPitch::new().apply(&mut f).unwrap();
        assert_eq!(f.custom_ot_values.post_is_fixed_pitch, None);

        // A .notdef with contours counts.
        let mut f = font();
        add_glyph(&mut f, "a", 1150.0, true);
        add_glyph(&mut f, ".notdef", 500.0, true);
        FontForgeFixedPitch::new().apply(&mut f).unwrap();
        assert_eq!(f.custom_ot_values.post_is_fixed_pitch, None);
    }

    #[test]
    fn test_the_glyphs_source_carries_is_fixed_pitch() {
        let mut f = font();
        add_glyph(&mut f, "a", 1150.0, true);
        FontForgeFixedPitch::new().apply(&mut f).unwrap();
        let glyphs = crate::convertors::glyphs3::as_glyphs3(&f).unwrap();
        let cp = glyphs
            .custom_parameters
            .iter()
            .find(|cp| cp.name == "isFixedPitch")
            .expect("an isFixedPitch custom parameter");
        assert_eq!(cp.value, glyphslib::Plist::Integer(1));
    }
}
