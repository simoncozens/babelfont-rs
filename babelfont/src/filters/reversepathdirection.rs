use crate::filters::FontFilter;

#[derive(Default)]
/// A filter that reverses every closed contour in the font.
///
/// This is the counterpart to [`super::correctpathdirection::CorrectPathDirection`]
/// for a font whose contour directions are not uniform. Correcting gives every outer
/// contour the same direction; reversing keeps the source's own mix of directions
/// through a compiler that reverses every contour on output.
///
/// Open contours are left alone, as they are not filled.
pub struct ReversePathDirection;

impl ReversePathDirection {
    /// Create a new ReversePathDirection filter
    pub fn new() -> Self {
        ReversePathDirection
    }
}

impl FontFilter for ReversePathDirection {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let mut reversed = 0;
        for glyph in font.glyphs.iter_mut() {
            for layer in &mut glyph.layers {
                for path in layer.shapes.iter_mut().filter_map(|s| s.as_path_mut()) {
                    if path.closed {
                        path.reverse();
                        reversed += 1;
                    }
                }
            }
        }
        log::info!("Reversed {reversed} closed contour(s)");
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(ReversePathDirection::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("reversepathdirection")
            .long("reverse-path-direction")
            .help(
                "Reverse every closed contour. Use instead of --correct-path-direction \
                 to keep contour directions that are not uniform",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Glyph, Layer, Node, Path, Shape};

    fn triangle(closed: bool) -> Shape {
        Shape::Path(Path {
            nodes: vec![
                Node::new_line(0, 0),
                Node::new_line(100, 0),
                Node::new_line(0, 100),
            ],
            closed,
            format_specific: Default::default(),
        })
    }

    #[test]
    fn test_a_closed_contour_is_reversed_and_an_open_one_is_not() {
        let mut layer = Layer::new(500.0);
        layer.shapes.push(triangle(true));
        layer.shapes.push(triangle(false));
        let mut font = Font::new();
        font.glyphs.0.push(Glyph {
            name: "a".into(),
            layers: vec![layer],
            ..Default::default()
        });
        let original = font.glyphs.0[0].layers[0].shapes.clone();

        ReversePathDirection::new().apply(&mut font).unwrap();

        let shapes = &font.glyphs.0[0].layers[0].shapes;
        let area = |shape: &Shape| shape.as_path().unwrap().signed_area().unwrap();
        assert_eq!(area(&shapes[0]), -area(&original[0]));
        assert_eq!(
            shapes[1].as_path().unwrap().nodes,
            original[1].as_path().unwrap().nodes
        );
    }
}
