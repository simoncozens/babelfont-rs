use std::collections::HashMap;

use kurbo::{cubics_to_quadratic_splines, BezPath, CubicBez, PathEl};

use crate::{filters::FontFilter, Node, Path};

/// A filter that renames glyphs in the font
#[derive(Debug, Clone, Default)]
pub struct CubicToQuadratic;

const TOLERANCE: f64 = 0.5;

fn path_el_kind(el: &PathEl) -> &'static str {
    match el {
        PathEl::MoveTo(_) => "MoveTo",
        PathEl::LineTo(_) => "LineTo",
        PathEl::QuadTo(_, _) => "QuadTo",
        PathEl::CurveTo(_, _, _) => "CurveTo",
        PathEl::ClosePath => "ClosePath",
    }
}

fn convert_bezpaths_in_parallel(paths: Vec<&BezPath>) -> Result<Vec<Path>, crate::BabelfontError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut new_paths = vec![Path::default(); paths.len()];
    let mut all_elements = Vec::with_capacity(paths.len());

    for path in &paths {
        all_elements.push(path.elements());
    }

    if all_elements.iter().all(|elements| elements.is_empty()) {
        return Ok(new_paths);
    }
    if all_elements.iter().any(|elements| elements.is_empty()) {
        return Err(crate::BabelfontError::FilterError(
            "Parallel conversion requires all paths to be either empty or non-empty".to_string(),
        ));
    }

    let mut last_points = Vec::with_capacity(paths.len());
    for elements in &all_elements {
        let last_point = if let Some(PathEl::MoveTo(p)) = elements.first() {
            Some(*p)
        } else {
            // Closed contours in kurbo may not start with a move command.
            elements.last().and_then(|el| el.end_point())
        };
        let Some(last_point) = last_point else {
            return Err(crate::BabelfontError::FilterError(
                "Cannot determine starting point for path during parallel conversion".to_string(),
            ));
        };
        last_points.push(last_point);
    }

    let first_len = all_elements[0].len();
    if all_elements
        .iter()
        .any(|elements| elements.len() != first_len)
    {
        return Err(crate::BabelfontError::FilterError(
            "Parallel conversion requires all paths to have the same number of elements"
                .to_string(),
        ));
    }

    for el_ix in 0..first_len {
        let expected_kind = path_el_kind(&all_elements[0][el_ix]);
        if all_elements
            .iter()
            .any(|elements| path_el_kind(&elements[el_ix]) != expected_kind)
        {
            return Err(crate::BabelfontError::FilterError(format!(
                "Parallel conversion requires matching element kinds at index {}",
                el_ix
            )));
        }

        match &all_elements[0][el_ix] {
            PathEl::MoveTo(_) => {
                for (path_ix, elements) in all_elements.iter().enumerate() {
                    let PathEl::MoveTo(p) = &elements[el_ix] else {
                        unreachable!("element kind checked above")
                    };
                    new_paths[path_ix].nodes.push(Node::new_move(p.x, p.y));
                    last_points[path_ix] = *p;
                }
            }
            PathEl::LineTo(_) => {
                for (path_ix, elements) in all_elements.iter().enumerate() {
                    let PathEl::LineTo(p) = &elements[el_ix] else {
                        unreachable!("element kind checked above")
                    };
                    new_paths[path_ix].nodes.push(Node::new_line(p.x, p.y));
                    last_points[path_ix] = *p;
                }
            }
            PathEl::QuadTo(_, _) => {
                for (path_ix, elements) in all_elements.iter().enumerate() {
                    let PathEl::QuadTo(p1, p2) = &elements[el_ix] else {
                        unreachable!("element kind checked above")
                    };
                    new_paths[path_ix]
                        .nodes
                        .push(Node::new_offcurve(p1.x, p1.y));
                    new_paths[path_ix].nodes.push(Node::new_qcurve(p2.x, p2.y));
                    last_points[path_ix] = *p2;
                }
            }
            PathEl::CurveTo(_, _, _) => {
                let mut cubics = Vec::with_capacity(all_elements.len());
                let mut end_points = Vec::with_capacity(all_elements.len());
                for (path_ix, elements) in all_elements.iter().enumerate() {
                    let PathEl::CurveTo(p1, p2, p3) = &elements[el_ix] else {
                        unreachable!("element kind checked above")
                    };
                    cubics.push(CubicBez::new(last_points[path_ix], *p1, *p2, *p3));
                    end_points.push(*p3);
                }

                let Some(quadsplines) = cubics_to_quadratic_splines(&cubics, TOLERANCE) else {
                    return Err(crate::BabelfontError::FilterError(format!(
                        "Failed to convert cubic segments to quadratic splines at element index {}",
                        el_ix
                    )));
                };
                if quadsplines.len() != new_paths.len() {
                    return Err(crate::BabelfontError::FilterError(format!(
                        "Parallel conversion produced {} splines for {} input paths",
                        quadsplines.len(),
                        new_paths.len()
                    )));
                }

                for (path_ix, spline) in quadsplines.iter().enumerate() {
                    // spline.points is [start, offcurves..., end]
                    let points = spline.points();
                    for (i, point) in points.iter().enumerate() {
                        if i == 0 {
                            // Start point equals the previous segment endpoint.
                        } else if i == points.len() - 1 {
                            new_paths[path_ix]
                                .nodes
                                .push(Node::new_qcurve(point.x, point.y));
                        } else {
                            new_paths[path_ix]
                                .nodes
                                .push(Node::new_offcurve(point.x, point.y));
                        }
                    }
                }

                for (path_ix, end_point) in end_points.into_iter().enumerate() {
                    last_points[path_ix] = end_point;
                }
            }
            PathEl::ClosePath => {
                for new_path in &mut new_paths {
                    new_path.closed = true;
                }
            }
        }
    }

    Ok(new_paths)
}

fn convert_bezpath_independently(path: &BezPath) -> Result<Path, crate::BabelfontError> {
    let converted = convert_bezpaths_in_parallel(vec![path])?;
    Ok(converted.into_iter().next().unwrap_or_default())
}

impl CubicToQuadratic {
    /// Create a new CubicToQuadratic filter
    pub fn new() -> Self {
        CubicToQuadratic
    }
}

fn signature(p: &BezPath) -> String {
    let mut sig = String::new();
    for el in p.elements() {
        match el {
            PathEl::MoveTo(_) => sig.push('M'),
            PathEl::LineTo(_) => sig.push('L'),
            PathEl::QuadTo(_, _) => sig.push('Q'),
            PathEl::CurveTo(_, _, _) => sig.push('C'),
            PathEl::ClosePath => sig.push('Z'),
        }
    }
    sig
}

impl FontFilter for CubicToQuadratic {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            // Collect the layers which should interpolate
            let interpolatable = glyph
                .layers
                .iter()
                .enumerate()
                .filter(|(_ix, l)| l.should_interpolate())
                .collect::<Vec<_>>();
            let pathsets = interpolatable
                .iter()
                .map(|(ix, l)| {
                    let shapes = l
                        .shapes
                        .iter()
                        .filter_map(|s| s.as_path())
                        .map(|p| p.to_kurbo())
                        .collect::<Result<Vec<_>, _>>();
                    shapes.map(|s| (*ix, s))
                })
                .collect::<Result<HashMap<usize, _>, _>>()?;
            let mut should_convert_together = pathsets.len() > 1;
            let first_length = pathsets
                .values()
                .next()
                .map(|paths| paths.len())
                .unwrap_or(0);
            if pathsets.values().any(|v| v.len() != first_length) {
                log::warn!(
                    "Different number of paths in glyph {}, converting separately",
                    glyph.name
                );
                should_convert_together = false;
            }
            for path_ix in 0..first_length {
                let sigs = pathsets
                    .values()
                    .map(|paths| signature(&paths[path_ix]))
                    .collect::<Vec<_>>();
                if sigs.iter().any(|s| s != &sigs[0]) {
                    log::warn!("CubicToQuadratic: Paths have different signatures for glyph {}, converting separately: {}", glyph.name, sigs.join(", "));
                    should_convert_together = false;
                    break;
                }
            }
            if !should_convert_together {
                // Easy case - convert independently and put back into layer
                for (layer_ix, paths) in pathsets {
                    let mut new_paths = paths
                        .iter()
                        .map(convert_bezpath_independently)
                        .collect::<Result<Vec<_>, _>>()?;
                    let layer = &mut glyph.layers[layer_ix];
                    let mut new_shapes = Vec::new();
                    for shape in &layer.shapes {
                        if let Some(_p) = shape.as_path() {
                            new_shapes.push(crate::Shape::Path(new_paths.remove(0)));
                        } else {
                            new_shapes.push(shape.clone());
                        }
                    }
                    layer.shapes = new_shapes;
                }
            } else {
                let layer_order = interpolatable.iter().map(|(ix, _)| *ix).collect::<Vec<_>>();
                let mut converted_by_layer = layer_order
                    .iter()
                    .map(|ix| (*ix, Vec::with_capacity(first_length)))
                    .collect::<HashMap<usize, Vec<Path>>>();

                #[allow(clippy::needless_range_loop)]
                for path_ix in 0..first_length {
                    let correlated_paths = layer_order
                        .iter()
                        .map(|layer_ix| &pathsets[layer_ix][path_ix])
                        .collect::<Vec<_>>();
                    let converted = convert_bezpaths_in_parallel(correlated_paths)?;
                    for (layer_ix, new_path) in
                        layer_order.iter().copied().zip(converted.into_iter())
                    {
                        if let Some(paths) = converted_by_layer.get_mut(&layer_ix) {
                            paths.push(new_path);
                        }
                    }
                }

                for layer_ix in layer_order {
                    let mut new_paths = converted_by_layer.remove(&layer_ix).unwrap_or_default();
                    let layer = &mut glyph.layers[layer_ix];
                    let mut new_shapes = Vec::new();
                    for shape in &layer.shapes {
                        if let Some(_p) = shape.as_path() {
                            new_shapes.push(crate::Shape::Path(new_paths.remove(0)));
                        } else {
                            new_shapes.push(shape.clone());
                        }
                    }
                    layer.shapes = new_shapes;
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(CubicToQuadratic::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("cubic2quadratic")
            .long("cubic2quadratic")
            .help("Convert cubic Bézier curves to quadratic Bézier curves")
            .action(clap::ArgAction::SetTrue)
    }
}
