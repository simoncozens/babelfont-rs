use crate::filters::FontFilter;
use crate::shape::{Component, Shape};
use crate::{Layer, LayerType};

/// One step of the F2Dot14 grid TrueType stores a component scale on.
const F2DOT14_STEP: f64 = 1.0 / 16384.0;
/// How far from an integer a scale may sit and still be read as that integer: one
/// and a half F2Dot14 steps, tight enough that a deliberate 0.999 scale is left alone.
const SCALE_EPSILON: f64 = F2DOT14_STEP * 1.5;
/// The range FontForge's exporter accepts for each entry of a component's 2x2 matrix
/// before it writes a glyph as a TrueType composite (`IsTTFRefable` in `tottf.c`).
const COMPOSITE_MATRIX_MIN: f64 = -2.0;
const COMPOSITE_MATRIX_MAX: f64 = 1.999939;
/// Room for the floating-point error of rebuilding a matrix from its decomposed form.
/// It is far below the last digit of a matrix entry in an SFD file, which FontForge
/// writes to six significant digits.
const MATRIX_SLACK: f64 = 1e-9;

/// A filter that snaps a near-integer component scale to that integer and rounds
/// each component offset to an integer, in a glyph made only of components.
///
/// # Which layers it touches
///
/// FontForge's exporter writes one layer of each glyph (`gi->layer` in `tottf.c`, the
/// foreground by default), which the SFD reader loads as the master's own layer. The
/// filter touches only a master's own layer; a background layer and any other layer
/// are left as they are.
///
/// Of those, it touches only a layer that FontForge's exporter writes as a TrueType
/// composite (`IsTTFRefable` in `tottf.c`): one with at least one component and no
/// contours, where every entry of every component's 2x2 matrix lies in
/// [-2, 1.999939]. For any other glyph FontForge applies each component's transform
/// as it stands and rounds the resulting points, as a compiler that decomposes the
/// component does, so such a layer is left as it is.
///
/// # Scales
///
/// A FontForge source may carry a mirror as -0.999939 (-16383/16384 to six digits),
/// as in `Refer: 101 40 N -0.999939 0 0 1 855.948 0 2`. FontForge's exporter floors a
/// scale onto the F2Dot14 grid (`put2d14` in `tottf.c`), which exports this value as
/// -1. The filter reads a scale within one and a half F2Dot14 steps of an integer as
/// that integer and leaves any other scale as it is. That is not `put2d14`'s rule,
/// which exports +0.999939 as 16383/16384 where this filter reads 1. A scale near 2 is
/// left as it is, since F2Dot14 cannot hold 2.
///
/// # Offsets
///
/// Each offset is rounded as `dumpcomposite` in `tottf.c` rounds it, with C's `rint`:
/// to the nearest integer, and a tie to the even one under the default rounding mode.
/// 855.948 becomes 856, 44.5 becomes 44 and 45.5 becomes 46. `dumpcomposite` keeps
/// the result in an integer, so an offset from -0.5 up to 0 becomes 0, not -0.
#[derive(Default)]
pub struct SnapComponentTransforms;

impl SnapComponentTransforms {
    /// Create a new SnapComponentTransforms filter
    pub fn new() -> Self {
        SnapComponentTransforms
    }
}

fn snap_scale(value: f64) -> f64 {
    let nearest = value.round();
    if nearest < 2.0 && (value - nearest).abs() <= SCALE_EPSILON {
        nearest
    } else {
        value
    }
}

/// Round an offset as `dumpcomposite` does: C's `rint`, stored in an integer, which
/// has no negative zero.
fn round_offset(value: f64) -> f64 {
    let rounded = value.round_ties_even();
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

/// Whether the layer is the one FontForge's exporter writes: a master's own layer,
/// not a background or any other layer.
fn is_exported_layer(layer: &Layer) -> bool {
    !layer.is_background && matches!(layer.master, LayerType::DefaultForMaster(_))
}

/// Whether every entry of the component's 2x2 matrix fits a TrueType composite as
/// FontForge's exporter judges it.
fn fits_a_composite(component: &Component) -> bool {
    let [a, b, c, d, _, _] = component.transform.as_affine().as_coeffs();
    [a, b, c, d].iter().all(|entry| {
        (COMPOSITE_MATRIX_MIN - MATRIX_SLACK..=COMPOSITE_MATRIX_MAX + MATRIX_SLACK)
            .contains(entry)
    })
}

/// Whether FontForge's exporter writes this layer as a TrueType composite.
fn is_written_as_a_composite(layer: &Layer) -> bool {
    !layer.shapes.is_empty()
        && layer.shapes.iter().all(|shape| match shape {
            Shape::Component(component) => fits_a_composite(component),
            Shape::Path(_) => false,
        })
}

impl FontFilter for SnapComponentTransforms {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                if !is_exported_layer(layer) || !is_written_as_a_composite(layer) {
                    continue;
                }
                for shape in layer.shapes.iter_mut() {
                    let Shape::Component(component) = shape else {
                        continue;
                    };
                    let t = &mut component.transform;
                    t.scale = (snap_scale(t.scale.0), snap_scale(t.scale.1));
                    t.translation = (
                        round_offset(t.translation.0),
                        round_offset(t.translation.1),
                    );
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(SnapComponentTransforms::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("snapcomponenttransforms")
            .long("snap-component-transforms")
            .help(
                "In a master's own layer made only of components, which FontForge \
                 exports as a TrueType composite: read a component scale within one and \
                 a half F2Dot14 steps of an integer as that integer (a FontForge source \
                 may carry a mirror as -0.999939, which FontForge exports as -1), and \
                 round each component offset with rint, ties to even, as FontForge's \
                 dumpcomposite() does. Background and other layers, which FontForge \
                 does not export, are left alone, as is a layer with contours or with a \
                 component matrix entry outside [-2, 1.999939]",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::decomposition::DecomposedAffine;
    use crate::{Font, Glyph, Node, Path};

    fn component(transform: DecomposedAffine) -> Shape {
        Shape::Component(Component {
            reference: "parenleft".into(),
            transform,
            location: Default::default(),
            format_specific: Default::default(),
        })
    }

    fn scaled(scale: (f64, f64), translation: (f64, f64)) -> Shape {
        component(DecomposedAffine {
            scale,
            translation,
            ..Default::default()
        })
    }

    fn contour() -> Shape {
        Shape::Path(Path {
            nodes: vec![
                Node::new_line(0, 0),
                Node::new_line(100, 0),
                Node::new_line(0, 100),
            ],
            closed: true,
            format_specific: Default::default(),
        })
    }

    fn masters_own_layer(shapes: Vec<Shape>) -> Layer {
        let mut layer = Layer::new(500.0);
        layer.master = LayerType::DefaultForMaster("m01".into());
        layer.shapes = shapes;
        layer
    }

    fn filtered(shapes: Vec<Shape>) -> Vec<Shape> {
        let layer = masters_own_layer(shapes);
        let mut font = Font::new();
        font.glyphs.0.push(Glyph {
            name: "parenright".into(),
            layers: vec![layer],
            ..Default::default()
        });
        SnapComponentTransforms::new().apply(&mut font).unwrap();
        font.glyphs.0[0].layers[0].shapes.clone()
    }

    fn transform_of(shape: &Shape) -> ((f64, f64), (f64, f64)) {
        let Shape::Component(c) = shape else {
            panic!("expected a component");
        };
        (c.transform.scale, c.transform.translation)
    }

    #[test]
    fn test_a_mirror_written_as_minus_0_999939_reads_as_minus_one() {
        // FontForge's exporter floors -0.999939 onto the F2Dot14 grid at -1 and
        // writes the offset 855.948 as 856.
        let shapes = filtered(vec![scaled((-0.999939, 1.0), (855.948, 0.0))]);
        assert_eq!(transform_of(&shapes[0]), ((-1.0, 1.0), (856.0, 0.0)));
    }

    #[test]
    fn test_a_real_scale_further_out_is_left_alone() {
        // The epsilon is one and a half F2Dot14 steps. A deliberate 0.999 scale is
        // eleven times that far out and is a design decision, not grid noise.
        let shapes = filtered(vec![scaled((0.999, 0.999), (10.0, 10.0))]);
        assert_eq!(transform_of(&shapes[0]), ((0.999, 0.999), (10.0, 10.0)));
    }

    #[test]
    fn test_a_half_unit_offset_breaks_toward_even_the_way_rint_does() {
        // FontForge's exporter rounds an offset with rint, which under the default
        // rounding mode sends a tie to the even neighbour: 44.5 becomes 44. It keeps
        // the result in an integer, so an offset from -0.5 up to 0 becomes 0, not -0.
        for (input, expected) in [
            (44.5, 44.0),
            (45.5, 46.0),
            (-44.5, -44.0),
            (-45.5, -46.0),
            (44.4, 44.0),
            (44.6, 45.0),
            (-0.4, 0.0),
            (-0.5, 0.0),
        ] {
            let shapes = filtered(vec![scaled((1.0, 1.0), (input, input))]);
            let (x, y) = transform_of(&shapes[0]).1;
            for offset in [x, y] {
                assert_eq!(offset, expected, "rint({input}) should be {expected}");
                assert_eq!(
                    offset.is_sign_positive(),
                    expected.is_sign_positive(),
                    "rint({input}) should be {expected}, not {offset}"
                );
            }
        }
    }

    #[test]
    fn test_a_glyph_with_a_contour_keeps_its_component_transforms() {
        // FontForge writes a glyph that mixes contours and components as a simple
        // glyph: it applies the transform as it stands and rounds each point, so a
        // point at 101 lands on rint(101.5) = 102, not on 101 + rint(0.5) = 101.
        let shapes = filtered(vec![
            contour(),
            scaled((1.0, 1.0), (0.5, 0.0)),
            scaled((-0.999939, 1.0), (855.948, 0.5)),
        ]);
        assert_eq!(transform_of(&shapes[1]), ((1.0, 1.0), (0.5, 0.0)));
        assert_eq!(transform_of(&shapes[2]), ((-0.999939, 1.0), (855.948, 0.5)));
    }

    #[test]
    fn test_a_matrix_entry_outside_f2dot14_leaves_the_whole_glyph_alone() {
        // One component FontForge cannot write into a composite makes it write the
        // whole glyph as a simple glyph.
        let shapes = filtered(vec![
            scaled((2.5, 1.0), (0.5, 0.0)),
            scaled((-0.999939, 1.0), (855.948, 0.0)),
        ]);
        assert_eq!(transform_of(&shapes[0]), ((2.5, 1.0), (0.5, 0.0)));
        assert_eq!(transform_of(&shapes[1]), ((-0.999939, 1.0), (855.948, 0.0)));
    }

    #[test]
    fn test_the_ends_of_the_f2dot14_range_still_make_a_composite() {
        // Built from the raw matrix, as the SFD reader builds it, so the entries
        // come back through the decomposition. A scale of 1.999939 stays below 2,
        // the value F2Dot14 cannot hold; -2 is already an integer.
        let transform = DecomposedAffine::from(kurbo::Affine::new([
            -2.0, 0.0, 0.0, 1.999939, 10.5, -0.5,
        ]));
        let shapes = filtered(vec![component(transform)]);
        let ((sx, sy), translation) = transform_of(&shapes[0]);
        assert!((sx + 2.0).abs() < 1e-12, "{sx}");
        assert!((sy - 1.999939).abs() < 1e-12, "{sy}");
        assert_eq!(translation, (10.0, 0.0));
    }

    #[test]
    fn test_only_a_masters_own_layer_is_snapped() {
        // FontForge exports one layer of each glyph, the foreground by default. A
        // background layer and any other layer are not written, so a composite
        // there keeps its transform.
        let mirror = || scaled((-0.999939, 1.0), (855.948, 0.5));
        let own = masters_own_layer(vec![mirror()]);
        let mut background = masters_own_layer(vec![mirror()]);
        background.master = LayerType::AssociatedWithMaster("m01".into());
        background.is_background = true;
        let mut background_of_own = masters_own_layer(vec![mirror()]);
        background_of_own.is_background = true;
        let mut extra = masters_own_layer(vec![mirror()]);
        extra.master = LayerType::AssociatedWithMaster("m01".into());
        let mut free_floating = masters_own_layer(vec![mirror()]);
        free_floating.master = LayerType::FreeFloating;
        let mut font = Font::new();
        font.glyphs.0.push(Glyph {
            name: "parenright".into(),
            layers: vec![own, background, background_of_own, extra, free_floating],
            ..Default::default()
        });
        SnapComponentTransforms::new().apply(&mut font).unwrap();
        let layers = &font.glyphs.0[0].layers;
        assert_eq!(
            transform_of(&layers[0].shapes[0]),
            ((-1.0, 1.0), (856.0, 0.0))
        );
        for layer in &layers[1..] {
            assert_eq!(
                transform_of(&layer.shapes[0]),
                ((-0.999939, 1.0), (855.948, 0.5)),
                "{:?} (background: {})",
                layer.master,
                layer.is_background
            );
        }
    }

    #[cfg(feature = "fontforge")]
    #[test]
    fn test_only_the_sfd_glyph_made_of_components_is_snapped() {
        let data = concat!(
            "SplineFontDB: 3.0\n",
            "LayerCount: 2\n",
            "Layer: 0 0 \"Back\" 1\n",
            "Layer: 1 0 \"Fore\" 0\n",
            "BeginChars: 3 3\n",
            "StartChar: bar\n",
            "Encoding: 65 65 0\n",
            "Width: 400\n",
            "Fore\n",
            "SplineSet\n",
            "101 0 m 1\n",
            " 301 0 l 1\n",
            " 301 100 l 1\n",
            " 101 100 l 1\n",
            " 101 0 l 1\n",
            "EndSplineSet\n",
            "EndChar\n",
            "StartChar: mixed\n",
            "Encoding: 66 66 1\n",
            "Width: 400\n",
            "Fore\n",
            "SplineSet\n",
            "0 0 m 1\n",
            " 10 0 l 1\n",
            " 10 10 l 1\n",
            " 0 0 l 1\n",
            "EndSplineSet\n",
            "Refer: 0 65 N 1 0 0 1 0.5 0 2\n",
            "EndChar\n",
            "StartChar: composite\n",
            "Encoding: 67 67 2\n",
            "Width: 400\n",
            "Fore\n",
            "Refer: 0 65 N -0.999939 0 0 1 855.948 0.5 2\n",
            "EndChar\n",
            "EndChars\n",
            "EndSplineFont\n"
        );
        let mut font = crate::convertors::fontforge::load_str(data).unwrap();
        SnapComponentTransforms::new().apply(&mut font).unwrap();
        let component_of = |name: &str| {
            let glyph = font.glyphs.get(name).unwrap();
            let shape = glyph
                .layers
                .iter()
                .flat_map(|layer| layer.shapes.iter())
                .find(|shape| shape.as_component().is_some())
                .unwrap();
            transform_of(shape)
        };
        assert_eq!(component_of("mixed"), ((1.0, 1.0), (0.5, 0.0)));
        let ((sx, sy), translation) = component_of("composite");
        assert_eq!((sx, sy), (-1.0, 1.0));
        assert_eq!(translation, (856.0, 0.0));
    }

    #[cfg(feature = "fontforge")]
    #[test]
    fn test_the_sfd_back_and_extra_layers_keep_their_component_transforms() {
        // FontForge exports the Fore layer by default, so a Refer in the Back layer
        // or in an extra layer is not written, whatever the Fore layer holds.
        let data = concat!(
            "SplineFontDB: 3.0\n",
            "LayerCount: 3\n",
            "Layer: 0 0 \"Back\" 1\n",
            "Layer: 1 0 \"Fore\" 0\n",
            "Layer: 2 0 \"Sketch\" 0\n",
            "BeginChars: 4 4\n",
            "StartChar: bar\n",
            "Encoding: 65 65 0\n",
            "Width: 400\n",
            "Fore\n",
            "SplineSet\n",
            "101 0 m 1\n",
            " 301 0 l 1\n",
            " 301 100 l 1\n",
            " 101 100 l 1\n",
            " 101 0 l 1\n",
            "EndSplineSet\n",
            "EndChar\n",
            "StartChar: withback\n",
            "Encoding: 66 66 1\n",
            "Width: 400\n",
            "Back\n",
            "Refer: 0 65 N -0.999939 0 0 1 855.948 0.5 2\n",
            "Fore\n",
            "SplineSet\n",
            "0 0 m 1\n",
            " 10 0 l 1\n",
            " 10 10 l 1\n",
            " 0 0 l 1\n",
            "EndSplineSet\n",
            "EndChar\n",
            "StartChar: withsketch\n",
            "Encoding: 67 67 2\n",
            "Width: 400\n",
            "Layer: 2\n",
            "Refer: 0 65 N -0.999939 0 0 1 855.948 0.5 2\n",
            "Fore\n",
            "SplineSet\n",
            "0 0 m 1\n",
            " 10 0 l 1\n",
            " 10 10 l 1\n",
            " 0 0 l 1\n",
            "EndSplineSet\n",
            "EndChar\n",
            "StartChar: composite\n",
            "Encoding: 68 68 3\n",
            "Width: 400\n",
            "Back\n",
            "Refer: 0 65 N -0.999939 0 0 1 855.948 0.5 2\n",
            "Fore\n",
            "Refer: 0 65 N -0.999939 0 0 1 855.948 0.5 2\n",
            "EndChar\n",
            "EndChars\n",
            "EndSplineFont\n"
        );
        let mut font = crate::convertors::fontforge::load_str(data).unwrap();
        SnapComponentTransforms::new().apply(&mut font).unwrap();
        let untouched = ((-0.999939, 1.0), (855.948, 0.5));
        let components_of = |name: &str| {
            let glyph = font.glyphs.get(name).unwrap();
            let mut components = glyph
                .layers
                .iter()
                .flat_map(|layer| {
                    let layer_name = layer.name.clone().unwrap_or_default();
                    layer
                        .shapes
                        .iter()
                        .filter(|shape| shape.as_component().is_some())
                        .map(move |shape| (layer_name.clone(), transform_of(shape)))
                })
                .collect::<Vec<_>>();
            components.sort_by(|a, b| a.0.cmp(&b.0));
            components
        };
        assert_eq!(
            components_of("withback"),
            vec![("Back".to_string(), untouched)]
        );
        assert_eq!(
            components_of("withsketch"),
            vec![("Sketch".to_string(), untouched)]
        );
        assert_eq!(
            components_of("composite"),
            vec![
                ("Back".to_string(), untouched),
                ("Fore".to_string(), ((-1.0, 1.0), (856.0, 0.0))),
            ]
        );
    }
}
