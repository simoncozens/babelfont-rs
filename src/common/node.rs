use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum NodeType {
    Move,
    Line,
    OffCurve,
    Curve,
    QCurve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub x: f32,
    pub y: f32,
    pub nodetype: NodeType,
    pub smooth: bool,
    // userData: XXX
}

impl Node {
    pub fn to_kurbo(&self) -> kurbo::Point {
        kurbo::Point::new(self.x as f64, self.y as f64)
    }
}

#[cfg(feature = "ufo")]
mod ufo {
    use super::*;

    impl From<&norad::PointType> for NodeType {
        fn from(p: &norad::PointType) -> Self {
            match p {
                norad::PointType::Move => NodeType::Move,
                norad::PointType::Line => NodeType::Line,
                norad::PointType::OffCurve => NodeType::OffCurve,
                norad::PointType::QCurve => NodeType::QCurve,
                _ => NodeType::Curve,
            }
        }
    }

    impl From<NodeType> for norad::PointType {
        fn from(p: NodeType) -> Self {
            match p {
                NodeType::Move => norad::PointType::Move,
                NodeType::Line => norad::PointType::Line,
                NodeType::OffCurve => norad::PointType::OffCurve,
                NodeType::QCurve => norad::PointType::QCurve,
                NodeType::Curve => norad::PointType::Curve,
            }
        }
    }

    impl From<&norad::ContourPoint> for Node {
        fn from(p: &norad::ContourPoint) -> Self {
            Node {
                x: p.x as f32,
                y: p.y as f32,
                nodetype: (&p.typ).into(),
                smooth: p.smooth,
            }
        }
    }

    impl From<&Node> for norad::ContourPoint {
        fn from(p: &Node) -> Self {
            norad::ContourPoint::new(
                p.x as f64,
                p.y as f64,
                p.nodetype.into(),
                p.smooth,
                None,
                None,
                None,
            )
        }
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use super::*;
    use glyphslib::{glyphs2::Node as G2Node, glyphs3::Node as G3Node};

    impl From<glyphslib::common::NodeType> for NodeType {
        fn from(p: glyphslib::common::NodeType) -> Self {
            match p {
                glyphslib::common::NodeType::Line => NodeType::Line,
                glyphslib::common::NodeType::OffCurve => NodeType::OffCurve,
                glyphslib::common::NodeType::Curve => NodeType::Curve,
                glyphslib::common::NodeType::QCurve => NodeType::QCurve,
                glyphslib::common::NodeType::LineSmooth => NodeType::Line,
                glyphslib::common::NodeType::CurveSmooth => NodeType::Curve,
                glyphslib::common::NodeType::QCurveSmooth => NodeType::QCurve,
            }
        }
    }

    impl From<NodeType> for glyphslib::common::NodeType {
        fn from(p: NodeType) -> Self {
            match p {
                NodeType::Line => glyphslib::common::NodeType::Line,
                NodeType::OffCurve => glyphslib::common::NodeType::OffCurve,
                NodeType::Curve => glyphslib::common::NodeType::Curve,
                NodeType::QCurve => glyphslib::common::NodeType::QCurve,
                NodeType::Move => glyphslib::common::NodeType::Line, // ?
            }
        }
    }

    impl From<&G3Node> for Node {
        fn from(val: &G3Node) -> Self {
            Node {
                x: val.x,
                y: val.y,
                nodetype: val.node_type.into(),
                smooth: matches!(
                    val.node_type,
                    glyphslib::common::NodeType::LineSmooth
                        | glyphslib::common::NodeType::CurveSmooth
                        | glyphslib::common::NodeType::QCurveSmooth
                ),
            }
        }
    }

    impl From<&Node> for G3Node {
        fn from(val: &Node) -> Self {
            G3Node {
                x: val.x,
                y: val.y,
                node_type: match (val.nodetype, val.smooth) {
                    (NodeType::Line, true) => glyphslib::common::NodeType::LineSmooth,
                    (NodeType::Curve, true) => glyphslib::common::NodeType::CurveSmooth,
                    (NodeType::QCurve, true) => glyphslib::common::NodeType::QCurveSmooth,
                    (nt, _) => nt.into(),
                },
                user_data: None,
            }
        }
    }

    impl From<&G2Node> for Node {
        fn from(val: &G2Node) -> Self {
            Node {
                x: val.x,
                y: val.y,
                nodetype: val.node_type.into(),
                smooth: matches!(
                    val.node_type,
                    glyphslib::common::NodeType::LineSmooth
                        | glyphslib::common::NodeType::CurveSmooth
                        | glyphslib::common::NodeType::QCurveSmooth
                ),
            }
        }
    }

    impl From<&Node> for G2Node {
        fn from(val: &Node) -> Self {
            G2Node {
                x: val.x,
                y: val.y,
                node_type: val.nodetype.into(),
            }
        }
    }
}
