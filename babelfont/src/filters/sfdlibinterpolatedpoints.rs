use crate::filters::FontFilter;
use crate::{Node, NodeType, Shape};

/// The SFD point flag FontForge sets on an on-curve point it may leave implied
/// (`SFD_PTFLAG_INTERPOLATE`).
const SFD_PTFLAG_INTERPOLATE: u32 = 0x80;

/// Turns each quadratic on-curve point that an SFD flags as interpolated into an
/// off-curve point at the same position, the way sfdLib's `sfd2ufo` reads it.
///
/// In an SFD spline line, `<points> c <flags>[x<hintmask>],<ttf index>,<next cp>`, the
/// flags are a decimal number. In a quadratic layer, sfdLib emits the end point of a
/// `c` segment whose flags carry `SFD_PTFLAG_INTERPOLATE` (0x80) as an off-curve point
/// instead of an on-curve one. On a closed contour the last segment's end point is
/// also the start point, so a flagged closing segment turns the start point into an
/// off-curve point too.
///
/// The point keeps its position: it is not dropped, so the contour gains a control
/// point where FontForge had an on-curve one. Use only to reproduce a binary built
/// from sfdLib's conversion of the source. It needs the `sfd.point_flags` the SFD
/// reader records on each on-curve node, and changes no other node.
#[derive(Default)]
pub struct SfdLibInterpolatedPoints;

impl SfdLibInterpolatedPoints {
    /// Create a new SfdLibInterpolatedPoints filter
    pub fn new() -> Self {
        SfdLibInterpolatedPoints
    }
}

/// The flags of an SFD spline point: the decimal number before an optional
/// `x<hintmask>` and before the first comma.
fn sfd_point_flags(node: &Node) -> Option<u32> {
    let raw = node.format_specific.get("sfd.point_flags")?.as_str()?;
    let flags = raw.split(',').next()?.split('x').next()?.trim();
    flags.parse::<u32>().ok()
}

fn is_interpolated_quadratic_point(node: &Node) -> bool {
    node.nodetype == NodeType::QCurve
        && sfd_point_flags(node).is_some_and(|flags| flags & SFD_PTFLAG_INTERPOLATE != 0)
}

impl FontFilter for SfdLibInterpolatedPoints {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let mut converted = 0;
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                for shape in layer.shapes.iter_mut() {
                    let Shape::Path(path) = shape else {
                        continue;
                    };
                    for node in path.nodes.iter_mut() {
                        if is_interpolated_quadratic_point(node) {
                            node.nodetype = NodeType::OffCurve;
                            node.smooth = false;
                            converted += 1;
                        }
                    }
                }
            }
        }
        log::info!("Turned {converted} interpolated on-curve point(s) into off-curve points");
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(SfdLibInterpolatedPoints::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("sfdlibinterpolatedpoints")
            .long("sfdlib-interpolated-points")
            .help(
                "Turn each quadratic on-curve point a FontForge source flags as interpolated \
                 (SFD point flag 0x80) into an off-curve point at the same position, as \
                 sfdLib's sfd2ufo reads it. Use only to reproduce a binary built from sfdLib's \
                 conversion of the source",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(all(test, feature = "fontforge"))]
mod tests {
    use super::*;
    use crate::convertors::fontforge;
    use crate::Font;

    // A quadratic contour: a line, then two `c` segments; the first ends on a point
    // flagged 128 (with a hint mask), the closing one ends unflagged on the start.
    const SFD: &str = "SplineFontDB: 3.2
FontName: Test
FullName: Test
FamilyName: Test
Weight: Regular
Ascent: 800
Descent: 200
LayerCount: 2
Layer: 0 1 \"Back\" 1
Layer: 1 1 \"Fore\" 0
BeginChars: 1 1

StartChar: a
Encoding: 97 97 0
Width: 500
LayerCount: 2
Fore
SplineSet
0 0 m 1,0,-1
100 0 l 1,1,-1
200 0 200 0 200 100 c 128x80,2,3
200 200 200 200 100 200 c 0,4,5
0 200 0 200 0 0 c 1,6,7
EndSplineSet
EndChar
EndChars
EndSplineFont
";

    fn font() -> Font {
        fontforge::load_str(SFD).expect("parse")
    }

    fn node_types(font: &Font) -> Vec<NodeType> {
        let Shape::Path(path) = &font.glyphs[0].layers[0].shapes[0] else {
            panic!("expected a path");
        };
        path.nodes.iter().map(|n| n.nodetype).collect()
    }

    #[test]
    fn test_a_flagged_point_becomes_an_off_curve_point() {
        let mut font = font();
        SfdLibInterpolatedPoints::new().apply(&mut font).unwrap();
        let Shape::Path(path) = &font.glyphs[0].layers[0].shapes[0] else {
            panic!("expected a path");
        };
        let flagged = path
            .nodes
            .iter()
            .find(|n| n.x == 200.0 && n.y == 100.0)
            .expect("the flagged point keeps its position");
        assert_eq!(flagged.nodetype, NodeType::OffCurve);
    }

    #[test]
    fn test_unflagged_points_are_left_alone() {
        let mut font = font();
        let before = node_types(&font);
        SfdLibInterpolatedPoints::new().apply(&mut font).unwrap();
        let after = node_types(&font);
        let changed = before
            .iter()
            .zip(after.iter())
            .filter(|(b, a)| b != a)
            .count();
        assert_eq!(changed, 1);
    }

    #[test]
    fn test_the_hint_mask_is_not_part_of_the_flags() {
        // "0x80" is flags 0 with hint mask 0x80, not flags 0x80.
        let mut font = fontforge::load_str(&SFD.replace("c 128x80,2,3", "c 0x80,2,3"))
            .expect("parse");
        let before = node_types(&font);
        SfdLibInterpolatedPoints::new().apply(&mut font).unwrap();
        assert_eq!(node_types(&font), before);
    }
}
