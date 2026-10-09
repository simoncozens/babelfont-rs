use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::common::formatspecific::FormatSpecific;

#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Default)]
#[typeshare]
/// Types of nodes in a glyph outline
pub enum NodeType {
    #[default] // arbitrary
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
    /// Draw a quartic Bézier curve to this node
    Quartic,
    /// Hobby curve (used in some advanced outline representations)
    Hobby,
    /// Spiro curve
    Spiro,
    /// Raph Levien's new spiral curve
    RaphNewSpiral,
}

impl NodeType {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            NodeType::Move => "m",
            NodeType::Line => "l",
            NodeType::OffCurve => "o",
            NodeType::Curve => "c",
            NodeType::QCurve => "q",
            NodeType::Quartic => "u",
            NodeType::Hobby => "h",
            NodeType::Spiro => "s",
            NodeType::RaphNewSpiral => "r",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
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

    /// Create a new Move node with default properties
    pub fn new_offcurve(x: impl Into<f64>, y: impl Into<f64>) -> Self {
        Node {
            x: x.into(),
            y: y.into(),
            nodetype: NodeType::OffCurve,
            ..Default::default()
        }
    }

    /// Create a new Curve node with default properties
    pub fn new_curve(x: impl Into<f64>, y: impl Into<f64>) -> Self {
        Node {
            x: x.into(),
            y: y.into(),
            nodetype: NodeType::Curve,
            ..Default::default()
        }
    }

    /// Create a new Move node with default properties
    pub fn new_move(x: impl Into<f64>, y: impl Into<f64>) -> Self {
        Node {
            x: x.into(),
            y: y.into(),
            nodetype: NodeType::Move,
            ..Default::default()
        }
    }

    /// Create a new Line node with default properties
    pub fn new_line(x: impl Into<f64>, y: impl Into<f64>) -> Self {
        Node {
            x: x.into(),
            y: y.into(),
            nodetype: NodeType::Line,
            ..Default::default()
        }
    }

    /// Create a new QCurve node with default properties
    pub fn new_qcurve(x: impl Into<f64>, y: impl Into<f64>) -> Self {
        Node {
            x: x.into(),
            y: y.into(),
            nodetype: NodeType::QCurve,
            smooth: false,
            format_specific: FormatSpecific::default(),
        }
    }
}

#[cfg(feature = "ufo")]
mod ufo {
    use crate::{convertors::ufo::stash_lib, BabelfontError};

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

    impl TryFrom<NodeType> for norad::PointType {
        type Error = BabelfontError;

        fn try_from(p: NodeType) -> Result<Self, Self::Error> {
            Ok(match p {
                NodeType::Move => norad::PointType::Move,
                NodeType::Line => norad::PointType::Line,
                NodeType::OffCurve => norad::PointType::OffCurve,
                NodeType::QCurve => norad::PointType::QCurve,
                NodeType::Curve => norad::PointType::Curve,
                _ => {
                    return Err(BabelfontError::UnrepresentablePointType(
                        p.as_str().to_string(),
                    ))
                }
            })
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

    impl TryFrom<&Node> for norad::ContourPoint {
        type Error = BabelfontError;

        fn try_from(p: &Node) -> Result<Self, Self::Error> {
            Ok(norad::ContourPoint::new(
                p.x,
                p.y,
                p.nodetype.try_into()?,
                p.smooth,
                None,
                None,
            ))
        }
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use crate::{
        convertors::glyphs3::{
            copy_user_data, KEY_NODE_HOI, KEY_NODE_LOCKED, KEY_NODE_ORIENTATION, KEY_NODE_TANGENT,
            KEY_USER_DATA,
        },
        BabelfontError,
    };

    use super::*;
    use glyphslib::{glyphs2::Node as G2Node, glyphs3::Node as G3Node};

    impl From<glyphslib::common::NodeType> for NodeType {
        fn from(p: glyphslib::common::NodeType) -> Self {
            match p {
                glyphslib::common::NodeType::Line | glyphslib::common::NodeType::LineSmooth => {
                    NodeType::Line
                }
                glyphslib::common::NodeType::OffCurve => NodeType::OffCurve,
                glyphslib::common::NodeType::Curve | glyphslib::common::NodeType::CurveSmooth => {
                    NodeType::Curve
                }
                glyphslib::common::NodeType::QCurve | glyphslib::common::NodeType::QCurveSmooth => {
                    NodeType::QCurve
                }
                glyphslib::common::NodeType::Quartic
                | glyphslib::common::NodeType::QuarticSmooth => NodeType::Quartic,
                glyphslib::common::NodeType::Hobby | glyphslib::common::NodeType::HobbySmooth => {
                    NodeType::Hobby
                }
                glyphslib::common::NodeType::RaphNewSpiral
                | glyphslib::common::NodeType::RaphNewSpiralSmooth => NodeType::RaphNewSpiral,
            }
        }
    }

    impl TryFrom<NodeType> for glyphslib::common::NodeType {
        type Error = BabelfontError;

        fn try_from(p: NodeType) -> Result<Self, Self::Error> {
            Ok(match p {
                NodeType::Line => glyphslib::common::NodeType::Line,
                NodeType::OffCurve => glyphslib::common::NodeType::OffCurve,
                NodeType::Curve => glyphslib::common::NodeType::Curve,
                NodeType::QCurve => glyphslib::common::NodeType::QCurve,
                NodeType::Move => glyphslib::common::NodeType::Line, // ?
                NodeType::Quartic => glyphslib::common::NodeType::Quartic,
                NodeType::Hobby => glyphslib::common::NodeType::Hobby,
                NodeType::RaphNewSpiral => glyphslib::common::NodeType::RaphNewSpiral,
                _ => {
                    return Err(BabelfontError::UnrepresentablePointType(
                        p.as_str().to_string(),
                    ))
                }
            })
        }
    }

    impl From<&G3Node> for Node {
        fn from(val: &G3Node) -> Self {
            let mut format_specific = FormatSpecific::default();
            if let Some(user_data) = &val.user_data {
                copy_user_data(&mut format_specific, user_data);
            }
            format_specific.insert_json(KEY_NODE_LOCKED, &val.locked);
            format_specific.insert_json(KEY_NODE_TANGENT, &val.tangent);
            format_specific.insert_json(KEY_NODE_ORIENTATION, &val.orientation);
            format_specific.insert_json(KEY_NODE_HOI, &val.hoi);

            Node {
                x: val.x as f64,
                y: val.y as f64,
                nodetype: val.node_type.into(),
                smooth: matches!(
                    val.node_type,
                    glyphslib::common::NodeType::LineSmooth
                        | glyphslib::common::NodeType::CurveSmooth
                        | glyphslib::common::NodeType::QCurveSmooth
                        | glyphslib::common::NodeType::HobbySmooth
                        | glyphslib::common::NodeType::QuarticSmooth
                        | glyphslib::common::NodeType::RaphNewSpiralSmooth
                ),
                format_specific,
            }
        }
    }

    impl TryFrom<&Node> for G3Node {
        type Error = BabelfontError;

        fn try_from(val: &Node) -> Result<Self, Self::Error> {
            Ok(G3Node {
                x: val.x as f32,
                y: val.y as f32,
                node_type: match (val.nodetype, val.smooth) {
                    (NodeType::Line, true) => glyphslib::common::NodeType::LineSmooth,
                    (NodeType::Curve, true) => glyphslib::common::NodeType::CurveSmooth,
                    (NodeType::QCurve, true) => glyphslib::common::NodeType::QCurveSmooth,
                    (NodeType::Hobby, true) => glyphslib::common::NodeType::HobbySmooth,
                    (NodeType::Quartic, true) => glyphslib::common::NodeType::QuarticSmooth,
                    (NodeType::RaphNewSpiral, true) => {
                        glyphslib::common::NodeType::RaphNewSpiralSmooth
                    }
                    (nt, _) => nt.try_into()?,
                },
                user_data: val.format_specific.get_json(KEY_USER_DATA),
                tangent: val.format_specific.get_json(KEY_NODE_TANGENT),
                locked: val.format_specific.get_json(KEY_NODE_LOCKED),
                orientation: val.format_specific.get_json(KEY_NODE_ORIENTATION),
                hoi: val.format_specific.get_json(KEY_NODE_HOI),
            })
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

    impl TryFrom<&Node> for G2Node {
        type Error = BabelfontError;

        fn try_from(val: &Node) -> Result<Self, Self::Error> {
            Ok(G2Node {
                x: val.x as f32,
                y: val.y as f32,
                node_type: val.nodetype.try_into()?,
            })
        }
    }
}
