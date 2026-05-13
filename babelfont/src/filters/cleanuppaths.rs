use std::collections::HashSet;

use kurbo::{fit_to_cubic, CubicBez, ParamCurveCurvature};

use crate::{filters::FontFilter, BabelfontError, Node, NodeType, Path};

/// A filter that cleans up paths by removing redundant points and setting smooth flags
#[derive(Default)]
pub struct CleanupPaths;

impl CleanupPaths {
    /// Create a new CleanupPaths filter
    pub fn new() -> Self {
        CleanupPaths
    }
}

fn is_orthogonal(prev: &Node, node: &Node, next: &Node) -> bool {
    (prev.x - node.x).abs() < 1e-6 && (node.x - next.x).abs() < 1e-6
        || (prev.y - node.y).abs() < 1e-6 && (node.y - next.y).abs() < 1e-6
}

fn is_corner(prev: &Node, node: &Node, next: &Node) -> bool {
    node.nodetype == NodeType::Curve
        && (prev.nodetype != NodeType::OffCurve || next.nodetype != NodeType::OffCurve)
}

fn bezes_around(path: &Path, ix: usize) -> Option<(CubicBez, CubicBez)> {
    let circular_lookup = |ix| &path.nodes[(ix + path.nodes.len() - 1) % path.nodes.len()];
    let cur_node = &path.nodes[ix];
    if cur_node.nodetype != NodeType::Curve {
        return None;
    }
    let prev_node = circular_lookup(ix - 1);
    if prev_node.nodetype != NodeType::OffCurve {
        return None;
    }
    let prev_prev_node = circular_lookup(ix - 2);
    if prev_prev_node.nodetype != NodeType::OffCurve {
        return None;
    }
    let prev_prev_prev_node = circular_lookup(ix - 3);
    if prev_prev_prev_node.nodetype != NodeType::Curve {
        return None;
    }
    let bez1 = CubicBez::new(
        kurbo::Point::new(prev_prev_prev_node.x, prev_prev_prev_node.y),
        kurbo::Point::new(prev_prev_node.x, prev_prev_node.y),
        kurbo::Point::new(prev_node.x, prev_node.y),
        kurbo::Point::new(cur_node.x, cur_node.y),
    );
    let next_node = circular_lookup(ix + 1);
    if next_node.nodetype != NodeType::OffCurve {
        return None;
    }
    let next_next_node = circular_lookup(ix + 2);
    if next_next_node.nodetype != NodeType::OffCurve {
        return None;
    }
    let next_next_next_node = circular_lookup(ix + 3);
    if next_next_next_node.nodetype != NodeType::Curve {
        return None;
    }
    let bez2 = CubicBez::new(
        kurbo::Point::new(cur_node.x, cur_node.y),
        kurbo::Point::new(next_node.x, next_node.y),
        kurbo::Point::new(next_next_node.x, next_next_node.y),
        kurbo::Point::new(next_next_next_node.x, next_next_next_node.y),
    );
    Some((bez1, bez2))
}

fn is_inflection(path: &Path, ix: usize) -> bool {
    if let Some((bez1, bez2)) = bezes_around(path, ix) {
        // Point is an inflection if the incoming curve has opposite
        // curvature sign compared to the outgoing curve.
        bez1.curvature(0.9).signum() != bez2.curvature(0.1).signum()
    } else {
        false
    }
}

fn cleanup_path(path: &mut Path) -> Result<Path, BabelfontError> {
    // We're going to take sequences of smooth curves between orthogonal / corner / inflection points,
    // and see if we can replace them with a single curve.
    // If the path is closed, we also consider the sequence that wraps around from the end to the start.
    let mut new_path = path.clone();
    let mut to_combine = vec![];
    let next_node = |ix| &path.nodes[(ix + 1) % path.nodes.len()];
    let prev_node = |ix| &path.nodes[(ix + path.nodes.len() - 1) % path.nodes.len()];
    let mut start_ix = 0;
    let mut seen_ix = HashSet::new();
    while start_ix < path.nodes.len() && !seen_ix.contains(&start_ix) {
        let node = &path.nodes[start_ix];
        seen_ix.insert(start_ix);
        // Is this the start of a sequence? Consider two consecutive offcurves, then take the node before
        if node.nodetype != NodeType::OffCurve || next_node(start_ix).nodetype != NodeType::OffCurve
        {
            start_ix += 1;
            continue;
        }
        start_ix = if start_ix == 0 {
            path.nodes.len() - 1
        } else {
            start_ix - 1
        };
        let mut end_ix = (start_ix + 2) % path.nodes.len();
        while end_ix != start_ix {
            if path.nodes[end_ix].nodetype == NodeType::OffCurve {
                end_ix = (end_ix + 1) % path.nodes.len();
                continue;
            }
            // Stop at orthogonal / inflection / corner
            if is_orthogonal(prev_node(end_ix), &path.nodes[end_ix], next_node(end_ix))
                || is_corner(prev_node(end_ix), &path.nodes[end_ix], next_node(end_ix))
                || is_inflection(path, end_ix)
            {
                break;
            }
            end_ix = (end_ix + 1) % path.nodes.len();
        }
        to_combine.push((start_ix, end_ix));
        start_ix = end_ix;
    }

    // Now check if a single curve can fit these segments without too much error
    let mut replace = vec![];
    for (start_ix, end_ix) in to_combine {
        // Extract the points to fit into a new path. Careful of wraparound!
        let mut points_slice = if end_ix > start_ix {
            new_path.nodes[start_ix..=end_ix].to_vec()
        } else {
            let mut slice = new_path.nodes[start_ix..].to_vec();
            slice.extend_from_slice(&new_path.nodes[..=end_ix]);
            slice
        };
        points_slice[0].nodetype = NodeType::Move;
        let candidate_path = Path {
            nodes: points_slice,
            closed: false,
            ..Default::default()
        };
        let as_bez = candidate_path.to_kurbo()?;
        let simplifier = kurbo::simplify::SimplifyBezPath::new(as_bez);
        if let Some((cubic, _)) = fit_to_cubic(&simplifier, 0f64..1f64, 1.0) {
            // We know the start and end; we want to replace the range with a single cubic so we need to know the two control points.

            let c1 = Node::new_offcurve(cubic.p1.x, cubic.p1.y);
            let c2 = Node::new_offcurve(cubic.p2.x, cubic.p2.y);
            replace.push((start_ix, end_ix, c1, c2));
        }
    }
    // Rebuild the node list instead of mutating in place: this avoids index
    // invalidation and handles wrap-around ranges naturally.
    let node_count = new_path.nodes.len();
    if node_count == 0 || replace.is_empty() {
        return Ok(new_path);
    }

    let mut remove_ix = vec![false; node_count];
    let mut inserts_after_start: Vec<Option<(Node, Node)>> = vec![None; node_count];

    for (start_ix, end_ix, c1, c2) in replace {
        if start_ix >= node_count || end_ix >= node_count {
            continue;
        }

        inserts_after_start[start_ix] = Some((c1, c2));

        // Remove nodes strictly between start and end, walking circularly.
        let mut ix = (start_ix + 1) % node_count;
        while ix != end_ix {
            remove_ix[ix] = true;
            ix = (ix + 1) % node_count;
        }
    }

    let mut rebuilt = Vec::with_capacity(node_count);
    for ix in 0..node_count {
        if remove_ix[ix] {
            continue;
        }

        rebuilt.push(new_path.nodes[ix].clone());
        if let Some((c1, c2)) = &inserts_after_start[ix] {
            rebuilt.push(c1.clone());
            rebuilt.push(c2.clone());
        }
    }

    new_path.nodes = rebuilt;
    new_path.rotate_to_preferred_representation();
    Ok(new_path)
}

impl FontFilter for CleanupPaths {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in &mut font.glyphs.iter_mut() {
            for layer in &mut glyph.layers {
                for path in &mut layer.shapes.iter_mut().filter_map(|s| s.as_path_mut()) {
                    path.set_smooth();
                    *path = cleanup_path(path)?;
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(CleanupPaths::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("cleanuppaths")
            .long("cleanup-paths")
            .help("Clean up paths by removing redundant points and setting smooth flags")
            .action(clap::ArgAction::SetTrue)
    }
}
