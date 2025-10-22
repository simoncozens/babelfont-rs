use crate::{
    common::{decomposition::DecomposedAffine, Node, NodeType},
    BabelfontError,
};

#[derive(Debug, Clone)]
pub struct Component {
    pub reference: String,
    pub transform: kurbo::Affine,
    pub format_specific: crate::common::FormatSpecific,
}

#[derive(Debug, Clone, Default)]
pub struct Path {
    pub nodes: Vec<Node>,
    pub closed: bool,
    pub format_specific: crate::common::FormatSpecific,
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

#[derive(Debug, Clone)]
pub enum Shape {
    Component(Component),
    Path(Path),
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use crate::convertors::glyphs3::KEY_ATTR;

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
            let transform = kurbo::Affine::IDENTITY
                * kurbo::Affine::translate((val.position.0 as f64, val.position.1 as f64))
                * kurbo::Affine::rotate((val.angle as f64).to_radians())
                * kurbo::Affine::scale_non_uniform(val.scale.0 as f64, val.scale.1 as f64);
            // let transform = kurbo::Affine::new([
            //     val.scale.0 as f64,
            //     0.0, // XXX
            //     0.0, // XXX
            //     val.scale.1 as f64,
            //     val.position.0 as f64,
            //     val.position.1 as f64,
            // ]);
            Component {
                reference: val.component_glyph.clone(),
                transform,
                format_specific: Default::default(),
            }
        }
    }

    impl From<&Component> for glyphslib::glyphs3::Component {
        fn from(val: &Component) -> Self {
            let decomposed: DecomposedAffine = val.transform.into();
            glyphslib::glyphs3::Component {
                component_glyph: val.reference.clone(),
                position: (
                    decomposed.translation.0 as f32,
                    decomposed.translation.1 as f32,
                ),
                scale: (decomposed.scale.0 as f32, decomposed.scale.1 as f32),
                angle: decomposed.rotation as f32,
                // For now
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
            let decomposed: DecomposedAffine = val.transform.into();
            fontra::Component {
                name: val.reference.clone(),
                transformation: decomposed.into(),
                location: HashMap::new(),
            }
        }
    }
}
