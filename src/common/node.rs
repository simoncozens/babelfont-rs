use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::common::formatspecific::FormatSpecific;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
#[typeshare]
/// Types of nodes in a glyph outline
pub enum NodeType {
    /// Move to a new position without drawing (only defined for open contours)
    Move,
    /// Draw a straight line to this node
    Line,
    /// Cubic Bézier curve control node (off-curve)
    OffCurve,
    /// Draw a cubic Bézier curve to this node
    Curve,
    /// Draw a quadratic Bézier curve to this node
    QCurve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[typeshare]
/// A node in a glyph outline
pub struct Node {
    /// The x-coordinate of the node
    pub x: f64,
    /// The y-coordinate of the node
    pub y: f64,
    /// The type of the node
    pub nodetype: NodeType,
    /// Whether the node is smooth
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub smooth: bool, // Not keen on the idea that we can have a smooth OffCurve node, may change
    /// Format-specific data
    #[typeshare(typescript(type = "Record<string, any>"))]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    pub format_specific: FormatSpecific,
}

impl Node {
    /// Convert the Node to a [kurbo::Point]
    pub fn to_kurbo(&self) -> kurbo::Point {
        kurbo::Point::new(self.x, self.y)
    }
}

#[cfg(feature = "ufo")]
mod ufo {
    use crate::convertors::ufo::stash_lib;

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
                x: p.x,
                y: p.y,
                nodetype: (&p.typ).into(),
                smooth: p.smooth,
                format_specific: stash_lib(p.lib()),
            }
        }
    }

    impl From<&Node> for norad::ContourPoint {
        fn from(p: &Node) -> Self {
            norad::ContourPoint::new(p.x, p.y, p.nodetype.into(), p.smooth, None, None)
        }
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use crate::convertors::glyphs3::{copy_user_data, KEY_USER_DATA};

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
            let mut format_specific = FormatSpecific::default();
            if let Some(user_data) = &val.user_data {
                copy_user_data(&mut format_specific, user_data);
            }
            Node {
                x: val.x as f64,
                y: val.y as f64,
                nodetype: val.node_type.into(),
                smooth: matches!(
                    val.node_type,
                    glyphslib::common::NodeType::LineSmooth
                        | glyphslib::common::NodeType::CurveSmooth
                        | glyphslib::common::NodeType::QCurveSmooth
                ),
                format_specific,
            }
        }
    }

    impl From<&Node> for G3Node {
        fn from(val: &Node) -> Self {
            G3Node {
                x: val.x as f32,
                y: val.y as f32,
                node_type: match (val.nodetype, val.smooth) {
                    (NodeType::Line, true) => glyphslib::common::NodeType::LineSmooth,
                    (NodeType::Curve, true) => glyphslib::common::NodeType::CurveSmooth,
                    (NodeType::QCurve, true) => glyphslib::common::NodeType::QCurveSmooth,
                    (nt, _) => nt.into(),
                },
                user_data: val.format_specific.get_json(KEY_USER_DATA),
            }
        }
    }

    impl From<&G2Node> for Node {
        fn from(val: &G2Node) -> Self {
            Node {
                x: val.x as f64,
                y: val.y as f64,
                nodetype: val.node_type.into(),
                smooth: matches!(
                    val.node_type,
                    glyphslib::common::NodeType::LineSmooth
                        | glyphslib::common::NodeType::CurveSmooth
                        | glyphslib::common::NodeType::QCurveSmooth
                ),
                format_specific: FormatSpecific::default(),
            }
        }
    }

    impl From<&Node> for G2Node {
        fn from(val: &Node) -> Self {
            G2Node {
                x: val.x as f32,
                y: val.y as f32,
                node_type: val.nodetype.into(),
            }
        }
    }
}
