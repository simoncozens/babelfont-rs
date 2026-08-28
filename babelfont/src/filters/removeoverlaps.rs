#![cfg(feature = "linesweeper")]

use kurbo::BezPath;
use linesweeper::{binary_op, BinaryOp, FillRule};
use smol_str::SmolStr;

use crate::{filters::FontFilter, Shape};

/// A filter that converts cubic Bézier curves to quadratic Bézier curves in all glyphs of a font, attempting to keep corresponding paths across layers consistent for better interpolation results. This filter requires the `kurbo` feature to be enabled.
#[derive(Debug, Clone, Default)]
pub struct RemoveOverlaps(Vec<SmolStr>);

impl RemoveOverlaps {
    /// Create a new RemoveOverlaps filter.
    /// If `glyph_names` is empty, all glyphs are processed.
    pub fn new(glyph_names: Vec<String>) -> Self {
        RemoveOverlaps(glyph_names.into_iter().map(SmolStr::from).collect())
    }
}

impl FontFilter for RemoveOverlaps {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            if self.0.is_empty() || self.0.contains(&glyph.name) {
                for layer in glyph.layers.iter_mut() {
                    // Parition into paths and components
                    let mut components: Vec<Shape> = vec![];
                    let mut bezpath_before = BezPath::new();
                    // Build a single BezPath from all paths in the layer, then perform a union operation to remove overlaps
                    let shapes = std::mem::take(&mut layer.shapes);

                    for shape in shapes {
                        match shape {
                            Shape::Component(component) => {
                                components.push(Shape::Component(component))
                            }
                            Shape::Path(path) => bezpath_before.extend(path.to_kurbo()?),
                        }
                    }
                    let contours = binary_op(
                        &bezpath_before,
                        &BezPath::new(),
                        FillRule::NonZero,
                        BinaryOp::Union,
                    )
                    .map_err(|e| {
                        crate::BabelfontError::FilterError(format!(
                            "Failed to remove overlaps: {}",
                            e
                        ))
                    })?;

                    layer.shapes = contours
                        .contours()
                        .map(|x| crate::Shape::Path(x.path.clone().into()))
                        .collect();
                    layer.shapes.extend(components);
                }
            }
        }
        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(RemoveOverlaps(super::parse_glyph_list(s)))
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        super::glyph_filter_arg(
            "removeoverlaps",
            "remove-overlaps",
            "Remove overlapping paths in glyphs",
        )
    }
}
