use crate::{filters::FontFilter, Layer, NodeType, Path, Shape};

/// A filter that drops on-curve points a quadratic outline implies: those
/// exactly halfway between two off-curves of one quadratic run, in every layer.
///
/// A TrueType outline leaves them out; a reader that draws through a pen (as
/// the TTF loader does) gets them back as explicit points.
#[derive(Default)]
pub struct DropImpliedOnCurves;

/// How far, in font units, an on-curve may sit from the midpoint and still be implied
const TOLERANCE: f64 = 1e-3;

/// The node that follows `index` on the path, if any
fn next_index(path: &Path, index: usize) -> Option<usize> {
    if index + 1 < path.nodes.len() {
        Some(index + 1)
    } else if path.closed {
        Some(0)
    } else {
        None
    }
}

fn previous_index(path: &Path, index: usize) -> Option<usize> {
    if index > 0 {
        Some(index - 1)
    } else if path.closed {
        Some(path.nodes.len() - 1)
    } else {
        None
    }
}

/// Whether the on-curve at `index` is implied in this path
fn is_implied(path: &Path, index: usize) -> bool {
    let nodes = &path.nodes;
    let Some(node) = nodes.get(index) else {
        return false;
    };
    if node.nodetype != NodeType::QCurve {
        return false;
    }
    let (Some(prev), Some(next)) = (previous_index(path, index), next_index(path, index)) else {
        return false;
    };
    let (Some(prev), Some(next)) = (nodes.get(prev), nodes.get(next)) else {
        return false;
    };
    if prev.nodetype != NodeType::OffCurve || next.nodetype != NodeType::OffCurve {
        return false;
    }
    // The run must go on to a quadratic on-curve, not a cubic one
    let mut cursor = index;
    for _ in 0..nodes.len() {
        let Some(following) = next_index(path, cursor) else {
            return false;
        };
        cursor = following;
        match nodes.get(cursor).map(|n| n.nodetype) {
            Some(NodeType::OffCurve) => continue,
            Some(NodeType::QCurve) => break,
            _ => return false,
        }
    }
    (node.x - (prev.x + next.x) / 2.0).abs() < TOLERANCE
        && (node.y - (prev.y + next.y) / 2.0).abs() < TOLERANCE
}

/// Whether every layer has the same paths, node for node
fn compatible(layers: &[&mut Layer]) -> bool {
    let signature = |layer: &Layer| -> Vec<(bool, Vec<NodeType>)> {
        layer
            .paths()
            .map(|p| (p.closed, p.nodes.iter().map(|n| n.nodetype).collect()))
            .collect()
    };
    let mut layers = layers.iter();
    let Some(first) = layers.next() else {
        return true;
    };
    let first = signature(first);
    layers.all(|layer| signature(layer) == first)
}

/// Keeps a closed path starting at an on-curve, stored last, as the model expects
fn end_on_curve(path: &mut Path) {
    if !path.closed || path.nodes.iter().all(|n| n.nodetype == NodeType::OffCurve) {
        return;
    }
    let trailing = path
        .nodes
        .iter()
        .rev()
        .take_while(|n| n.nodetype == NodeType::OffCurve)
        .count();
    path.nodes.rotate_right(trailing);
}

impl FontFilter for DropImpliedOnCurves {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            let mut layers: Vec<&mut Layer> = glyph
                .layers
                .iter_mut()
                .filter(|l| !l.is_background)
                .collect();
            if layers.is_empty() || !compatible(&layers) {
                continue;
            }
            let path_count = layers[0].paths().count();
            for path_index in 0..path_count {
                // each layer's path once, not once per node
                let paths: Option<Vec<_>> = layers
                    .iter()
                    .map(|layer| layer.paths().nth(path_index).cloned())
                    .collect();
                let Some(paths) = paths else {
                    continue;
                };
                let Some(first) = paths.first() else {
                    continue;
                };
                let implied: Vec<usize> = (0..first.nodes.len())
                    .filter(|&i| paths.iter().all(|p| is_implied(p, i)))
                    .collect();
                if implied.is_empty() {
                    continue;
                }
                for layer in layers.iter_mut() {
                    let Some(path) = layer
                        .shapes
                        .iter_mut()
                        .filter_map(|s| match s {
                            Shape::Path(p) => Some(p),
                            _ => None,
                        })
                        .nth(path_index)
                    else {
                        continue;
                    };
                    for &i in implied.iter().rev() {
                        path.nodes.remove(i);
                    }
                    end_on_curve(path);
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(DropImpliedOnCurves)
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("dropimpliedoncurves")
            .long("drop-implied-oncurves")
            .help("Drop on-curve points a quadratic outline implies")
            .action(clap::ArgAction::SetTrue)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::{Font, Glyph, Node};

    fn path(nodes: &[(f64, f64, NodeType)]) -> Path {
        Path {
            nodes: nodes
                .iter()
                .map(|&(x, y, nodetype)| Node {
                    x,
                    y,
                    nodetype,
                    ..Default::default()
                })
                .collect(),
            closed: true,
            ..Default::default()
        }
    }

    fn font_with(layers: Vec<Vec<(f64, f64, NodeType)>>) -> Font {
        let mut font = Font::new();
        let mut glyph = Glyph::new("o");
        for nodes in layers {
            let mut layer = Layer::new(500.0);
            layer.shapes.push(Shape::Path(path(&nodes)));
            glyph.layers.push(layer);
        }
        font.glyphs.push(glyph);
        font
    }

    fn node_types(font: &Font, layer: usize) -> Vec<NodeType> {
        font.glyphs.get("o").unwrap().layers[layer]
            .paths()
            .next()
            .unwrap()
            .nodes
            .iter()
            .map(|n| n.nodetype)
            .collect()
    }

    use NodeType::{Curve, Line, OffCurve as Off, QCurve as Q};

    #[test]
    fn test_midpoints_go_and_the_path_ends_on_curve() {
        // (100, 0) is implied; (0, 0) is not, (200, 100) is not on a run
        let mut font = font_with(vec![vec![
            (50.0, 0.0, Off),
            (100.0, 0.0, Q),
            (150.0, 0.0, Off),
            (200.0, 100.0, Q),
            (200.0, 200.0, Line),
            (0.0, 0.0, Line),
        ]]);
        DropImpliedOnCurves.apply(&mut font).unwrap();
        assert_eq!(node_types(&font, 0), vec![Off, Off, Q, Line, Line]);
    }

    #[test]
    fn test_all_on_curves_implied_leaves_only_off_curves() {
        let mut font = font_with(vec![vec![
            (0.0, 50.0, Off),
            (25.0, 75.0, Q),
            (50.0, 100.0, Off),
            (75.0, 75.0, Q),
            (100.0, 50.0, Off),
            (75.0, 25.0, Q),
            (50.0, 0.0, Off),
            (25.0, 25.0, Q),
        ]]);
        DropImpliedOnCurves.apply(&mut font).unwrap();
        assert_eq!(node_types(&font, 0), vec![Off; 4]);
    }

    #[test]
    fn test_kept_unless_implied_in_every_layer() {
        let nodes = |mid: f64| {
            vec![
                (0.0, 0.0, Off),
                (mid, 0.0, Q),
                (100.0, 0.0, Off),
                (100.0, 100.0, Q),
                (0.0, 100.0, Line),
            ]
        };
        let mut font = font_with(vec![nodes(50.0), nodes(60.0)]);
        DropImpliedOnCurves.apply(&mut font).unwrap();
        assert_eq!(node_types(&font, 0), vec![Off, Q, Off, Q, Line]);
        assert_eq!(node_types(&font, 1), vec![Off, Q, Off, Q, Line]);
    }

    #[test]
    fn test_kept_before_a_cubic_segment() {
        // a smooth quadratic on-curve whose next segment is cubic stays
        let mut font = font_with(vec![vec![
            (0.0, 0.0, Off),
            (50.0, 0.0, Q),
            (100.0, 0.0, Off),
            (150.0, 50.0, Off),
            (150.0, 100.0, Curve),
            (0.0, 100.0, Line),
        ]]);
        DropImpliedOnCurves.apply(&mut font).unwrap();
        assert_eq!(node_types(&font, 0), vec![Off, Q, Off, Off, Curve, Line]);
    }
}
