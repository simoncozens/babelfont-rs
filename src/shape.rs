use crate::{
    common::{decomposition::DecomposedAffine, FormatSpecific, Node, NodeType},
    BabelfontError,
};
use fontdrasil::coords::DesignCoord;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use typeshare::typeshare;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[typeshare]
/// A component in a glyph
pub struct Component {
    /// The referenced glyph name
    #[typeshare(serialized_as = "String")]
    pub reference: SmolStr,
    /// The transformation applied to the component
    #[serde(
        default = "DecomposedAffine::default",
        skip_serializing_if = "crate::serde_helpers::decomposed_is_identity"
    )]
    pub transform: DecomposedAffine,
    /// A location for a variable component
    // We don't use a DesignLocation here because we want to allow axis names
    // rather than tags
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        default,
        serialize_with = "crate::serde_helpers::string_design_location_to_map",
        deserialize_with = "crate::serde_helpers::string_design_location_from_map"
    )]
    #[typeshare(typescript(
        type = "Record<string, import('@simoncozens/fonttypes').DesignspaceCoordinate>"
    ))]
    #[typeshare(python(type = "Dict[str, float]"))]
    pub location: IndexMap<String, DesignCoord>,
    /// Format-specific data
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: FormatSpecific,
}

impl Component {
    // component_layer?
    // pos / angle / scale
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[typeshare]
/// A path in a glyph
pub struct Path {
    #[serde(
        serialize_with = "crate::serde_helpers::serialize_nodes",
        deserialize_with = "crate::serde_helpers::deserialize_nodes"
    )]
    /// A list of nodes in the path
    pub nodes: Vec<Node>,
    /// Whether the path is closed
    pub closed: bool,
    /// Format-specific data
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: FormatSpecific,
}

impl Path {
    /// Converts the `Path` to a [`kurbo::BezPath`].
    // Stolen completely from norad
    pub fn to_kurbo(&self) -> Result<kurbo::BezPath, BabelfontError> {
        let mut path = kurbo::BezPath::new();
        let mut offs = std::collections::VecDeque::new();
        let rotate = if self.closed {
            self.nodes
                .iter()
                .rev()
                .position(|pt| pt.nodetype != NodeType::OffCurve)
                .map(|idx| self.nodes.len() - 1 - idx)
                .unwrap_or(0)
        } else {
            0
        };
        let mut nodes = self
            .nodes
            .iter()
            .cycle()
            .skip(rotate)
            .take(self.nodes.len());
        // We do this because all kurbo paths (even closed ones)
        // must start with a move_to (otherwise get_segs doesn't work)
        if let Some(start) = nodes.next() {
            path.move_to(start.to_kurbo());
        }
        for pt in nodes {
            let kurbo_point = pt.to_kurbo();
            match pt.nodetype {
                NodeType::Move => path.move_to(kurbo_point),
                NodeType::Line => path.line_to(kurbo_point),
                NodeType::OffCurve => offs.push_back(kurbo_point),
                NodeType::Curve => {
                    match offs.make_contiguous() {
                        [] => return Err(BabelfontError::BadPath),
                        [p1] => path.quad_to(*p1, kurbo_point),
                        [p1, p2] => path.curve_to(*p1, *p2, kurbo_point),
                        _ => return Err(BabelfontError::BadPath),
                    };
                    offs.clear();
                }
                NodeType::QCurve => {
                    while let Some(pt) = offs.pop_front() {
                        if let Some(next) = offs.front() {
                            let implied_point = pt.midpoint(*next);
                            path.quad_to(pt, implied_point);
                        } else {
                            path.quad_to(pt, kurbo_point);
                        }
                    }
                    offs.clear();
                }
            }
        }
        if self.closed {
            path.close_path()
        }
        Ok(path)
    }
}

/// A shape in a glyph, either a component or a path
#[derive(Debug, Clone, Serialize, Deserialize)]
#[typeshare]
#[serde(untagged)]
pub enum Shape {
    /// A component in a glyph
    Component(Component),
    /// A path in a glyph
    Path(Path),
}

impl Shape {
    pub(crate) fn is_smart_component(&self) -> bool {
        match self {
            Shape::Component(c) => !c.location.is_empty(),
            Shape::Path(_) => false,
        }
    }

    /// Apply a DecomposedAffine transform to the shape
    pub fn apply_transform(&self, transform: DecomposedAffine) -> Self {
        match self {
            Shape::Component(c) => {
                let new_transform = transform.as_affine() * c.transform.as_affine();
                let mut new_component = c.clone();
                new_component.transform = new_transform.into();
                Shape::Component(new_component)
            }
            Shape::Path(p) => {
                let mut contour = Path::default();
                for node in &p.nodes {
                    let new_point = transform.as_affine() * kurbo::Point::new(node.x, node.y);
                    contour.nodes.push(Node {
                        x: new_point.x,
                        y: new_point.y,
                        nodetype: node.nodetype,
                        smooth: node.smooth,
                        format_specific: node.format_specific.clone(),
                    })
                }
                contour.closed = p.closed;

                Shape::Path(contour)
            }
        }
    }
}

// This code stolen from Skrifa.
/// Interface for accepting a sequence of path commands.
pub trait OutlinePen {
    /// Emit a command to begin a new subpath at (x, y).
    fn move_to(&mut self, x: f32, y: f32);

    /// Emit a line segment from the current point to (x, y).
    fn line_to(&mut self, x: f32, y: f32);

    /// Emit a quadratic bezier segment from the current point with a control
    /// point at (cx0, cy0) and ending at (x, y).
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32);

    /// Emit a cubic bezier segment from the current point with control
    /// points at (cx0, cy0) and (cx1, cy1) and ending at (x, y).
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32);

    /// Emit a command to close the current subpath.
    fn close(&mut self);
}

/// A pen which builds paths
///
/// ```rust
/// use babelfont::{OutlinePen, PathBuilder};
/// let mut pen = PathBuilder::new();
/// pen.move_to(0.0, 0.0);
/// pen.line_to(100.0, 0.0);
/// pen.line_to(100.0, 100.0);
/// pen.close();
/// let paths = pen.build();
/// assert_eq!(paths.len(), 1);
/// assert_eq!(paths[0].nodes.len(), 3); // move, line, line
/// assert!(paths[0].closed);
/// ```
#[derive(Debug, Clone, Default)]
pub struct PathBuilder {
    paths: Vec<Path>,
    current_path: Option<Path>,
}
impl PathBuilder {
    /// Create a new PathBuilder
    pub fn new() -> Self {
        Self {
            paths: Vec::new(),
            current_path: None,
        }
    }
    /// Build and return the paths
    pub fn build(self) -> Vec<Path> {
        if let Some(path) = self.current_path {
            let mut paths = self.paths;
            paths.push(path);
            return paths;
        }
        self.paths
    }

    fn current_path_mut(&mut self) -> &mut Path {
        if self.current_path.is_none() {
            self.current_path = Some(Path::default());
        }
        #[allow(clippy::unwrap_used)] // we just checked it's Some
        self.current_path.as_mut().unwrap()
    }
}

impl OutlinePen for PathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        // Start a new path if we have an existing one
        if let Some(path) = self.current_path.take() {
            self.paths.push(path);
        }
        self.current_path_mut().nodes.push(Node::new_move(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.current_path_mut().nodes.push(Node::new_line(x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.current_path_mut()
            .nodes
            .push(Node::new_offcurve(cx0, cy0));
        self.current_path_mut().nodes.push(Node::new_qcurve(x, y));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.current_path_mut()
            .nodes
            .push(Node::new_offcurve(cx0, cy0));
        self.current_path_mut()
            .nodes
            .push(Node::new_offcurve(cx1, cy1));
        self.current_path_mut().nodes.push(Node::new_curve(x, y));
    }

    fn close(&mut self) {
        if let Some(path) = self.current_path.as_mut() {
            // End-of-path fixups; first, close the current path
            path.closed = true;
            // The first node is currently a move, but we don't want a move
            // in a closed path. If the final node brought us back to the start
            // point, then drop the first node. Otherwise, convert it to a line.
            if let Some(first_node) = path.nodes.first() {
                if let Some(last_node) = path.nodes.last() {
                    if first_node.x == last_node.x && first_node.y == last_node.y {
                        // Drop the first node
                        path.nodes.remove(0);
                    } else {
                        // Convert the first node to a line
                        if let Some(first_node) = path.nodes.first_mut() {
                            first_node.nodetype = NodeType::Line;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use fontdrasil::coords::DesignCoord;
    use indexmap::IndexMap;

    use crate::convertors::glyphs3::{
        KEY_ALIGNMENT, KEY_ATTR, KEY_COMPONENT_ANCHOR, KEY_COMPONENT_LOCKED,
    };

    use super::*;

    impl From<&glyphslib::glyphs3::Shape> for Shape {
        fn from(val: &glyphslib::glyphs3::Shape) -> Self {
            match val {
                glyphslib::glyphs3::Shape::Component(c) => Shape::Component(c.into()),
                glyphslib::glyphs3::Shape::Path(p) => Shape::Path(p.into()),
            }
        }
    }

    impl From<&Shape> for glyphslib::glyphs3::Shape {
        fn from(val: &Shape) -> Self {
            match val {
                Shape::Component(c) => glyphslib::glyphs3::Shape::Component(c.into()),
                Shape::Path(p) => glyphslib::glyphs3::Shape::Path(p.into()),
            }
        }
    }

    impl From<&glyphslib::glyphs3::Component> for Component {
        fn from(val: &glyphslib::glyphs3::Component) -> Self {
            use crate::common::decomposition::TransformOrder;
            // Glyphs uses: translate → skew → rotate → scale
            let transform = DecomposedAffine {
                translation: (val.position.0 as f64, val.position.1 as f64),
                skew: (val.slant.0 as f64, val.slant.1 as f64),
                rotation: (val.angle as f64).to_radians(),
                scale: (val.scale.0 as f64, val.scale.1 as f64),
                order: TransformOrder::Glyphs,
            };
            let mut format_specific = FormatSpecific::default();
            format_specific.insert_if_ne_json(
                KEY_ALIGNMENT,
                &val.alignment,
                &-1, // default value
            );
            format_specific.insert_nonempty_json(KEY_ATTR, &val.attr);
            format_specific.insert_some_json(KEY_COMPONENT_ANCHOR, &val.anchor);
            format_specific.insert_if_ne_json(
                KEY_COMPONENT_LOCKED,
                &val.locked,
                &false, // default value
            );
            let mut location = IndexMap::new();
            for (k, v) in &val.smart_component_location {
                location.insert(k.clone(), DesignCoord::new(*v as f64));
            }

            Component {
                reference: SmolStr::from(&val.component_glyph),
                transform,
                location,
                format_specific,
            }
        }
    }

    impl From<&Component> for glyphslib::glyphs3::Component {
        fn from(val: &Component) -> Self {
            // val.transform is already a DecomposedAffine, use it directly
            glyphslib::glyphs3::Component {
                component_glyph: val.reference.to_string(),
                position: (
                    val.transform.translation.0 as f32,
                    val.transform.translation.1 as f32,
                ),
                scale: (val.transform.scale.0 as f32, val.transform.scale.1 as f32),
                angle: (val.transform.rotation as f32).to_degrees(),
                slant: (val.transform.skew.0 as f32, val.transform.skew.1 as f32),
                alignment: val
                    .format_specific
                    .get(KEY_ALIGNMENT)
                    .and_then(|v| v.as_i64())
                    .map(|s| s as i8)
                    .unwrap_or(-1),
                anchor: val.format_specific.get_optionstring(KEY_COMPONENT_ANCHOR),
                attr: val.format_specific.get_json(KEY_ATTR),
                smart_component_location: val
                    .location
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_f64() as f32))
                    .collect(),
                locked: val.format_specific.get_bool(KEY_COMPONENT_LOCKED),
                ..Default::default()
            }
        }
    }

    impl From<&glyphslib::glyphs3::Path> for Path {
        fn from(val: &glyphslib::glyphs3::Path) -> Self {
            let mut nodes = vec![];
            for node in &val.nodes {
                nodes.push(node.into());
            }
            let mut format_specific = crate::common::FormatSpecific::default();
            if !val.attr.is_empty() {
                format_specific.insert(
                    KEY_ATTR.into(),
                    serde_json::to_value(&val.attr).unwrap_or_default(),
                );
            }
            Path {
                nodes,
                closed: val.closed,
                format_specific,
            }
        }
    }

    impl From<&Path> for glyphslib::glyphs3::Path {
        fn from(val: &Path) -> Self {
            let mut nodes = vec![];
            for node in &val.nodes {
                nodes.push(node.into());
            }
            glyphslib::glyphs3::Path {
                nodes,
                closed: val.closed,
                attr: val
                    .format_specific
                    .get(KEY_ATTR)
                    .and_then(|x| serde_json::from_value(x.clone()).ok())
                    .unwrap_or_default(),
            }
        }
    }
}

#[cfg(feature = "fontra")]
mod fontra {
    use std::collections::HashMap;

    use super::*;
    use crate::convertors::fontra;

    impl From<&Component> for fontra::Component {
        fn from(val: &Component) -> Self {
            fontra::Component {
                name: val.reference.to_string(),
                transformation: val.transform.into(),
                location: HashMap::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_path_serde_roundtrip() {
        let path = Path {
            nodes: vec![
                Node {
                    x: 744.0,
                    y: 1249.0,
                    nodetype: NodeType::Line,
                    smooth: true,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: 744.0,
                    y: 1249.0,
                    nodetype: NodeType::OffCurve,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: 744.0,
                    y: 1249.0,
                    nodetype: NodeType::OffCurve,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: 744.0,
                    y: 1249.0,
                    nodetype: NodeType::QCurve,
                    smooth: true,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: 538.0,
                    y: 1470.0,
                    nodetype: NodeType::Line,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: 538.0,
                    y: 1470.0,
                    nodetype: NodeType::OffCurve,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: 538.0,
                    y: 1470.0,
                    nodetype: NodeType::OffCurve,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: 538.0,
                    y: 1470.0,
                    nodetype: NodeType::QCurve,
                    smooth: true,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: -744.0,
                    y: 181.0,
                    nodetype: NodeType::Line,
                    smooth: true,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: -744.0,
                    y: 181.0,
                    nodetype: NodeType::OffCurve,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: -744.0,
                    y: 181.0,
                    nodetype: NodeType::OffCurve,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: -744.0,
                    y: 181.0,
                    nodetype: NodeType::QCurve,
                    smooth: true,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: -538.0,
                    y: -40.0,
                    nodetype: NodeType::Line,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: -538.0,
                    y: -40.0,
                    nodetype: NodeType::OffCurve,
                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: -538.0,
                    y: -40.0,
                    nodetype: NodeType::OffCurve,

                    smooth: false,
                    format_specific: FormatSpecific::default(),
                },
                Node {
                    x: -538.0,
                    y: -40.0,
                    nodetype: NodeType::QCurve,
                    smooth: true,
                    format_specific: FormatSpecific::default(),
                },
            ],
            closed: false,
            format_specific: FormatSpecific::default(),
        };
        let serialized = serde_json::to_string(&path).unwrap();
        let deserialized: Path = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.nodes.len(), path.nodes.len());
        for (a, b) in deserialized.nodes.iter().zip(path.nodes.iter()) {
            assert_eq!(a.x, b.x);
            assert_eq!(a.y, b.y);
            assert_eq!(a.nodetype, b.nodetype);
            assert_eq!(a.smooth, b.smooth);
        }
    }
}
