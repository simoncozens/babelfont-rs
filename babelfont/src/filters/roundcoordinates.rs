use crate::filters::FontFilter;
use crate::{Node, NodeType, Path, Shape};

/// Rounds node positions and component offsets to integers, ties to even. An on-curve
/// point midway between two off-curve points stays at their midpoint, so it stays implied.
#[derive(Default)]
pub struct RoundCoordinates;

impl RoundCoordinates {
    /// Create a new RoundCoordinates filter
    pub fn new() -> Self {
        RoundCoordinates
    }
}

fn round(value: f64) -> f64 {
    let rounded = value.round_ties_even();
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

fn is_off_curve(node: &Node) -> bool {
    node.nodetype == NodeType::OffCurve
}

fn round_path(path: &mut Path) {
    let n = path.nodes.len();
    let neighbours = |i: usize| -> Option<(usize, usize)> {
        if path.closed {
            Some(((i + n - 1) % n, (i + 1) % n))
        } else if i > 0 && i + 1 < n {
            Some((i - 1, i + 1))
        } else {
            None
        }
    };
    let implied: Vec<Option<(usize, usize)>> = (0..n)
        .map(|i| {
            let (p, q) = neighbours(i)?;
            let (node, prev, next) = (&path.nodes[i], &path.nodes[p], &path.nodes[q]);
            (n > 2
                && !is_off_curve(node)
                && is_off_curve(prev)
                && is_off_curve(next)
                && node.x == (prev.x + next.x) / 2.0
                && node.y == (prev.y + next.y) / 2.0)
                .then_some((p, q))
        })
        .collect();
    for (i, node) in path.nodes.iter_mut().enumerate() {
        if implied[i].is_none() {
            node.x = round(node.x);
            node.y = round(node.y);
        }
    }
    for (i, pair) in implied.iter().enumerate() {
        if let Some((p, q)) = *pair {
            let x = (path.nodes[p].x + path.nodes[q].x) / 2.0;
            let y = (path.nodes[p].y + path.nodes[q].y) / 2.0;
            path.nodes[i].x = x;
            path.nodes[i].y = y;
        }
    }
}

impl FontFilter for RoundCoordinates {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                for shape in layer.shapes.iter_mut() {
                    match shape {
                        Shape::Path(path) => round_path(path),
                        Shape::Component(component) => {
                            let t = &mut component.transform.translation;
                            *t = (round(t.0), round(t.1));
                        }
                        // Nothing to round in an opaque shape.
                        Shape::FormatSpecific(_) => {}
                    }
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(RoundCoordinates::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("roundcoordinates")
            .long("round-coordinates")
            .help(
                "Round node positions and component offsets to integers, ties to even \
                 (44.5 to 44, 45.5 to 46). An on-curve point midway between two off-curve \
                 points stays at their midpoint",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Font, Glyph, Layer, Node, NodeType, Path};

    #[test]
    fn test_rounds_nodes_and_component_offsets_ties_to_even() {
        let mut layer = Layer::new(500.0);
        layer.shapes.push(Shape::Path(Path {
            nodes: vec![
                Node { x: 10.4, y: -0.4, nodetype: NodeType::Line, ..Default::default() },
                Node { x: 44.5, y: 45.5, nodetype: NodeType::Line, ..Default::default() },
            ],
            closed: true,
            ..Default::default()
        }));
        let mut component = Component {
            reference: "a".into(),
            transform: Default::default(),
            location: Default::default(),
            format_specific: Default::default(),
        };
        component.transform.translation = (855.948, -0.5);
        component.transform.scale = (-1.0, 1.0);
        layer.shapes.push(Shape::Component(component));
        let mut font = Font::new();
        font.glyphs.0.push(Glyph { name: "g".into(), layers: vec![layer], ..Default::default() });

        RoundCoordinates::new().apply(&mut font).unwrap();

        let shapes = &font.glyphs.0[0].layers[0].shapes;
        let Shape::Path(path) = &shapes[0] else { panic!() };
        let points: Vec<(f64, f64)> = path.nodes.iter().map(|n| (n.x, n.y)).collect();
        assert_eq!(points, vec![(10.0, 0.0), (44.0, 46.0)]);
        assert!(points[0].1.is_sign_positive());
        let Shape::Component(c) = &shapes[1] else { panic!() };
        assert_eq!(c.transform.translation, (856.0, 0.0));
        assert!(c.transform.translation.1.is_sign_positive());
        assert_eq!(c.transform.scale, (-1.0, 1.0));
    }

    #[test]
    fn test_implied_on_curve_point_stays_implied() {
        let node = |x, y, nodetype| Node { x, y, nodetype, ..Default::default() };
        let mut path = Path {
            nodes: vec![
                node(10.4, 0.4, NodeType::OffCurve),
                node(15.2, 10.2, NodeType::QCurve),
                node(20.0, 20.0, NodeType::OffCurve),
                node(30.5, 0.5, NodeType::QCurve),
            ],
            closed: true,
            ..Default::default()
        };
        round_path(&mut path);
        let points: Vec<(f64, f64)> = path.nodes.iter().map(|n| (n.x, n.y)).collect();
        assert_eq!(points, vec![(10.0, 0.0), (15.0, 10.0), (20.0, 20.0), (30.0, 0.0)]);
    }
}
