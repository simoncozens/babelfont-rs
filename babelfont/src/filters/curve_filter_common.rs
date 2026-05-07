use std::collections::HashMap;

use kurbo::{BezPath, PathEl};

use crate::{NodeType, Path};

pub(crate) fn path_el_kind(el: &PathEl) -> &'static str {
    match el {
        PathEl::MoveTo(_) => "MoveTo",
        PathEl::LineTo(_) => "LineTo",
        PathEl::QuadTo(_, _) => "QuadTo",
        PathEl::CurveTo(_, _, _) => "CurveTo",
        PathEl::ClosePath => "ClosePath",
    }
}

pub(crate) fn signature(path: &BezPath) -> String {
    let mut sig = String::new();
    for el in path.elements() {
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

pub(crate) fn mark_closed_and_normalize(new_paths: &mut [Path]) {
    for new_path in new_paths {
        new_path.closed = true;
        // Closed babelfont paths must not keep a leading Move node.
        if let (Some(first), Some(last)) = (new_path.nodes.first(), new_path.nodes.last()) {
            if first.nodetype == NodeType::Move {
                if first.x == last.x && first.y == last.y {
                    new_path.nodes.remove(0);
                } else if let Some(first_mut) = new_path.nodes.first_mut() {
                    first_mut.nodetype = NodeType::Line;
                }
            }
        }
    }
}

pub(crate) fn apply_interpolatable_path_filter(
    font: &mut crate::Font,
    filter_name: &str,
    convert_bezpath_independently: fn(&BezPath) -> Result<Path, crate::BabelfontError>,
    convert_bezpaths_in_parallel: fn(Vec<&BezPath>) -> Result<Vec<Path>, crate::BabelfontError>,
) -> Result<(), crate::BabelfontError> {
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
                log::warn!(
                    "{}: Paths have different signatures for glyph {}, converting separately: {}",
                    filter_name,
                    glyph.name,
                    sigs.join(", ")
                );
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
                for (layer_ix, new_path) in layer_order.iter().copied().zip(converted.into_iter()) {
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
