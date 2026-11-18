use crate::{
    common::{decomposition::TransformOrder, tag_from_string},
    Anchor, Axis, BabelfontError, Component, Font, Glyph, GlyphCategory, Guide, Layer, LayerType,
    Master, MetricType, Node, NodeType, Path, Shape, Tag,
};
use fontdrasil::{
    coords::{DesignCoord, DesignLocation, Location, UserCoord},
    types::Axes,
};
use indexmap::IndexMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::{collections::HashMap, convert::TryInto, fs, path::PathBuf, sync::LazyLock};

fn to_point(s: String) -> Result<(f64, f64), BabelfontError> {
    let mut i = s.split(' ');
    let x_str = i.next().ok_or(BabelfontError::General(
        "Couldn't read X coordinate".to_string(),
    ))?;
    let x = x_str
        .parse::<f64>()
        .map_err(|_| BabelfontError::General(format!("Couldn't parse X coordinate {:}", x_str)))?;
    let y_str = i.next().ok_or(BabelfontError::General(
        "Couldn't read Y coordinate".to_string(),
    ))?;
    let y = y_str
        .parse::<f64>()
        .map_err(|_| BabelfontError::General(format!("Couldn't parse Y coordinate {:}", y_str)))?;
    Ok((x, y))
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabComponent {
    glyphName: String,
}

impl From<FontlabComponent> for Shape {
    fn from(val: FontlabComponent) -> Self {
        use crate::common::decomposition::DecomposedAffine;
        Shape::Component(Component {
            reference: val.glyphName.into(),
            transform: DecomposedAffine::default(),
            format_specific: Default::default(),
            location: IndexMap::new(),
        })
    }
}
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabContour {
    nodes: Vec<String>,
}
#[allow(clippy::unwrap_used)]
static NODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(-?[\d\.]+) (-?[\d\.]+)( [osc])?").unwrap());

/// Parse a single node string from a Fontlab contour into one or more Node values.
///
/// In cubic contours (where any node string contains double-space-separated
/// coordinate pairs), a string like "146 229  163 271  197 305 s" expands to
/// three nodes: two OffCurve control points and one Curve end point.
///
/// In quadratic/linear contours each string is a single coordinate pair:
///   - "x y s" -> QCurve (smooth on-curve)
///   - "x y o" -> OffCurve
///   - "x y"   -> Line
///
/// The `is_first_string` and `ends_with_c` flags convey contour-level context
/// needed to correctly classify the first node when the contour closes with a
/// cubic "curve back" segment (" c" suffix).
fn parse_node_string(
    s: &str,
    is_cubic: bool,
    is_first_string: bool,
    ends_with_c: bool,
) -> Vec<Node> {
    let pieces: Vec<&str> = s.split("  ").collect();
    let num_pieces = pieces.len();

    pieces
        .iter()
        .enumerate()
        .filter_map(|(ix, piece)| {
            let mat = NODE_RE.captures(piece)?;
            let suffix = mat.get(3).map(|m| m.as_str());
            let is_last_piece = ix == num_pieces - 1;

            #[allow(clippy::unwrap_used)] // Matches regex -> parses
            Some(if is_cubic {
                let (nodetype, smooth) = if num_pieces > 1 && !is_last_piece {
                    // Intermediate piece in a multi-piece cubic string
                    (NodeType::OffCurve, false)
                } else if suffix == Some(" o") {
                    (NodeType::OffCurve, false)
                } else if suffix == Some(" s") {
                    if num_pieces > 1 {
                        // Last piece of a multi-piece cubic string: Curve endpoint
                        (NodeType::Curve, true)
                    } else if is_first_string && ends_with_c {
                        // First node of a cubic contour with closing 'c'
                        (NodeType::Curve, true)
                    } else {
                        // Single-piece cubic string: smooth line/move
                        (NodeType::Line, true)
                    }
                } else if suffix == Some(" c") {
                    if num_pieces > 1 {
                        // "c" on a multi-piece string: both pieces are control
                        // points; the endpoint is the first contour node.
                        (NodeType::OffCurve, false)
                    } else {
                        (NodeType::Curve, false)
                    }
                } else if num_pieces > 1 {
                    // Last piece of a multi-piece cubic string, no suffix
                    (NodeType::Curve, false)
                } else if is_first_string && ends_with_c {
                    // First node of a cubic contour that closes with 'c':
                    // it's a Curve, smooth only if it has the 's' suffix.
                    (NodeType::Curve, false)
                } else {
                    (NodeType::Line, false)
                };
                Node {
                    x: mat[1].parse().unwrap(),
                    y: mat[2].parse().unwrap(),
                    nodetype,
                    smooth,
                    format_specific: Default::default(),
                }
            } else {
                // Quadratic or linear contour.
                // A node with no suffix could be an on-curve point between
                // quadratic segments (QCurve) or a line start (Line). We
                // initially parse as QCurve and fix up Line after the full
                // parse below.
                let (nodetype, smooth) = if suffix == Some(" o") {
                    (NodeType::OffCurve, false)
                } else if suffix == Some(" s") {
                    (NodeType::QCurve, true)
                } else {
                    (NodeType::QCurve, false)
                };
                Node {
                    x: mat[1].parse().unwrap(),
                    y: mat[2].parse().unwrap(),
                    nodetype,
                    smooth,
                    format_specific: Default::default(),
                }
            })
        })
        .collect()
}

impl From<FontlabContour> for Shape {
    fn from(val: FontlabContour) -> Self {
        let is_cubic = val.nodes.iter().any(|s| s.contains("  "));
        let ends_with_c = val.nodes.last().is_some_and(|s| s.trim().ends_with(" c"));

        // `nodes` is built in the same point-pen order FontLab stores them,
        // i.e. starting at the contour's start point.
        let mut path = Path {
            nodes: val
                .nodes
                .into_iter()
                .enumerate()
                .flat_map(|(string_ix, s)| {
                    parse_node_string(&s, is_cubic, string_ix == 0, ends_with_c)
                })
                .collect(),
            format_specific: Default::default(),
            closed: true,
        };
        // Some VFJ contours explicitly repeat the start point at the end
        // as a close marker. Remove it so we don't produce a duplicate node.
        if path.nodes.len() > 1 {
            let first = &path.nodes[0];
            #[allow(clippy::unwrap_used)] // Checked above
            let last = path.nodes.last().unwrap();
            let is_oncurve = |n: &Node| {
                matches!(
                    n.nodetype,
                    NodeType::Line | NodeType::Curve | NodeType::QCurve
                )
            };
            if is_oncurve(first) && is_oncurve(last) && first.x == last.x && first.y == last.y {
                path.nodes.pop();
            }
        }

        if is_cubic && ends_with_c {
            // Cubic contour with closing "c": the last segment's control
            // points are trailing off-curves. Rotate so the start point
            // (first node) moves to the end and the wrapping off-curves
            // come to the front.
            path.nodes.rotate_left(1);
        } else if path.closed && path.nodes.len() > 1 {
            let has_offcurves = path.nodes.iter().any(|n| n.nodetype == NodeType::OffCurve);
            if has_offcurves
                && path
                    .nodes
                    .last()
                    .is_some_and(|n| n.nodetype == NodeType::OffCurve)
            {
                // Quadratic VFJ contour wraps: the last entry is an off-curve
                // whose implied endpoint is the first point. Rotate so the
                // start point moves to the end (Glyphs.app convention).
                path.nodes.rotate_left(1);
            }

            if has_offcurves {
                // In a quadratic contour, a QCurve node is an on-curve point
                // between quadratic segments when preceded by an OffCurve, or
                // a smooth/plain Line when preceded by another on-curve.
                for ix in 0..path.nodes.len() {
                    let prev_ix = if ix == 0 {
                        path.nodes.len() - 1
                    } else {
                        ix - 1
                    };
                    if path.nodes[ix].nodetype == NodeType::QCurve
                        && path.nodes[prev_ix].nodetype != NodeType::OffCurve
                    {
                        path.nodes[ix].nodetype = NodeType::Line;
                    }
                }
            } else {
                // Line-only contour: all no-suffix QCurves should be Line
                for node in path.nodes.iter_mut() {
                    if node.nodetype == NodeType::QCurve {
                        node.nodetype = NodeType::Line;
                    }
                }
            }
        }
        Shape::Path(path)
    }
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabPath {
    contours: Vec<FontlabContour>,
}

impl From<FontlabPath> for Vec<Shape> {
    fn from(val: FontlabPath) -> Self {
        val.contours.into_iter().map(|x| x.into()).collect()
    }
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum FontlabShape {
    Component { component: FontlabComponent },
    Path(FontlabPath),
    Image { image: serde_json::Value },
}
impl From<FontlabShape> for Vec<Shape> {
    fn from(val: FontlabShape) -> Self {
        match val {
            FontlabShape::Component { component } => vec![component.into()],
            FontlabShape::Path(p) => p.into(),
            FontlabShape::Image { image: _ } => vec![], // XXX
        }
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
struct FontlabComponentTransform {
    #[serde(default)]
    xOffset: i32,
    #[serde(default)]
    yOffset: i32,
    #[serde(default)]
    xScale: Option<f64>,
    #[serde(default)]
    yScale: Option<f64>,
}

impl From<FontlabComponentTransform> for crate::common::decomposition::DecomposedAffine {
    fn from(val: FontlabComponentTransform) -> Self {
        crate::common::decomposition::DecomposedAffine {
            translation: (val.xOffset as f64, val.yOffset as f64),
            scale: (val.xScale.unwrap_or(1.0), val.yScale.unwrap_or(1.0)),
            order: TransformOrder::Glyphs,
            ..Default::default()
        }
    }
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum FontlabElement {
    TaggedShape {
        elementData: FontlabShape,
        #[serde(default)]
        transform: Option<FontlabComponentTransform>,
    },
    UntaggedShape {
        component: FontlabComponent,
        #[serde(default)]
        transform: Option<FontlabComponentTransform>,
    },
}

fn apply_transform(shape: &mut Shape, transform: Option<FontlabComponentTransform>) {
    if let Some(t) = transform {
        if let Shape::Component(ref mut c) = shape {
            c.transform = t.into();
        }
    }
}

impl From<FontlabElement> for Vec<Shape> {
    fn from(val: FontlabElement) -> Self {
        match val {
            FontlabElement::TaggedShape {
                elementData,
                transform,
            } => {
                let mut shapes: Vec<Shape> = elementData.into();
                for shape in shapes.iter_mut() {
                    apply_transform(shape, transform.clone());
                }
                shapes
            }
            FontlabElement::UntaggedShape {
                component,
                transform,
            } => {
                let mut shape: Shape = component.into();
                apply_transform(&mut shape, transform);
                vec![shape]
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct FontlabAnchor {
    name: String,
    point: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug, Default)]
struct FontlabGuide {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    horizontal: bool,
    #[serde(default)]
    vertical: bool,
    position: Option<f64>,
    angle: Option<f64>,
    x: Option<f64>,
    y: Option<f64>,
    center: Option<String>,
    vector: Option<String>,
    color: Option<String>,
}

fn parse_point(value: &str) -> Option<(f64, f64)> {
    let mut parts = value.split_whitespace();
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    Some((x, y))
}

/// Parse a FontLab colour value (`#rrggbb` or a small set of named colours).
fn parse_color(value: &str) -> Option<crate::common::Color> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let r = i32::from_str_radix(&hex[0..2], 16).ok()?;
        let g = i32::from_str_radix(&hex[2..4], 16).ok()?;
        let b = i32::from_str_radix(&hex[4..6], 16).ok()?;
        return Some(crate::common::Color { r, g, b, a: 255 });
    }
    let (r, g, b) = match value.trim().to_ascii_lowercase().as_str() {
        "silver" => (192, 192, 192),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "black" => (0, 0, 0),
        _ => return None,
    };
    Some(crate::common::Color { r, g, b, a: 255 })
}

impl From<&FontlabGuide> for Guide {
    fn from(val: &FontlabGuide) -> Self {
        let position = val.position.unwrap_or(0.0) as f32;
        let pos = if val.horizontal {
            crate::common::Position {
                x: 0.0,
                y: position,
                angle: 0.0,
            }
        } else if val.vertical {
            crate::common::Position {
                x: position,
                y: 0.0,
                angle: 90.0,
            }
        } else if let (Some(center), Some(vector)) = (&val.center, &val.vector) {
            let (x, y) = parse_point(center).unwrap_or((0.0, 0.0));
            let (dx, dy) = parse_point(vector).unwrap_or((0.0, 0.0));
            crate::common::Position {
                x: x as f32,
                y: y as f32,
                angle: dy.atan2(dx).to_degrees() as f32,
            }
        } else {
            crate::common::Position {
                x: val.x.unwrap_or(0.0) as f32,
                y: val.y.unwrap_or(0.0) as f32,
                angle: val.angle.unwrap_or(0.0) as f32,
            }
        };
        Guide {
            pos,
            name: val.name.clone(),
            color: val.color.as_deref().and_then(parse_color),
            format_specific: Default::default(),
        }
    }
}

impl TryInto<Option<Anchor>> for FontlabAnchor {
    fn try_into(self) -> Result<Option<Anchor>, BabelfontError> {
        // An anchor without a point is placed at the origin (FontLab's
        // convention for an anchor which has not been positioned yet).
        let (x, y) = match self.point {
            Some(point) => to_point(point)?,
            None => (0.0, 0.0),
        };
        Ok(Some(Anchor {
            x,
            y,
            name: self.name,
            format_specific: Default::default(),
        }))
    }

    type Error = BabelfontError;
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabLayer {
    advanceWidth: i32,
    name: Option<String>,
    #[serde(default)]
    anchors: Vec<FontlabAnchor>,
    #[serde(default)]
    guidelines: Vec<FontlabGuide>,
    #[serde(default)]
    active: Option<bool>,
    color: Option<String>,
    #[serde(default)]
    elements: Vec<FontlabElement>,
}

impl FontlabLayer {
    fn try_into_babel(self, font: &Font) -> Result<Layer, BabelfontError> {
        let anchors: Result<Vec<Option<Anchor>>, BabelfontError> =
            self.anchors.into_iter().map(|x| x.try_into()).collect();
        // FontLab names each non-background layer after a master.
        let master = self
            .name
            .as_ref()
            .and_then(|layer_name| {
                font.masters.iter().find(|master| {
                    master.name.get_default().map(|name| name.as_str()) == Some(layer_name.as_str())
                })
            })
            .map(|master| LayerType::DefaultForMaster(master.id.clone()))
            .unwrap_or(LayerType::FreeFloating);
        Ok(Layer {
            width: self.advanceWidth as f32,
            vertical_advance: None,
            vertical_origin: None,
            name: self.name.clone(),
            id: self.name,
            active: self.active.unwrap_or(true),
            master,
            guides: self.guidelines.iter().map(Guide::from).collect(),
            shapes: self
                .elements
                .into_iter()
                .flat_map(|x| {
                    let v: Vec<Shape> = x.into();
                    v
                })
                .collect(),
            anchors: anchors?.into_iter().flatten().collect(),
            color: self.color.as_deref().and_then(parse_color),
            layer_index: None,
            is_background: false,
            background_layer_id: None,
            location: None,
            smart_component_location: Default::default(),
            format_specific: Default::default(),
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct FontlabGlyph {
    name: String,
    unicode: Option<String>,
    layers: Vec<FontlabLayer>,
}

impl FontlabGlyph {
    fn try_into(self, font: &Font) -> Result<Glyph, BabelfontError> {
        let codepoints = if let Some(unicode) = self.unicode {
            unicode
                .split(',')
                .flat_map(|x| u32::from_str_radix(x, 16))
                .collect()
        } else {
            vec![]
        };
        let mut layers: Vec<Layer> = self
            .layers
            .into_iter()
            .map(|x| x.try_into_babel(font))
            .collect::<Result<_, _>>()?;
        // FontLab writes glyph layers in reverse master order (plus any
        // mask/image layer first). Put the master layers back into master
        // order so they line up with every other Babelfont format.
        layers.sort_by_key(|layer| match &layer.master {
            LayerType::DefaultForMaster(id) => (
                1usize,
                font.masters
                    .iter()
                    .position(|master| &master.id == id)
                    .unwrap_or(usize::MAX),
            ),
            _ => (0, 0),
        });

        Ok(Glyph {
            name: self.name.into(),
            production_name: None,
            category: GlyphCategory::Unknown,
            codepoints,
            layers,
            exported: true,
            direction: None,
            format_specific: Default::default(),
            component_axes: Default::default(),
        })
    }
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabAxis {
    name: String,
    shortName: String,
    tag: String,
    designMinimum: f32,
    designMaximum: f32,
    minimum: Option<f64>,
    maximum: Option<f64>,
    default: Option<f64>,
    axisGraph: Option<HashMap<String, f64>>,
}

impl From<FontlabAxis> for Axis {
    fn from(val: FontlabAxis) -> Self {
        // FontLab stores axis names in a lower-case, tag-like form
        // ("weight"); use the conventional human-readable name where known.
        let name = match val.tag.as_str() {
            "wght" => "Weight".to_string(),
            "wdth" => "Width".to_string(),
            "opsz" => "Optical Size".to_string(),
            "ital" => "Italic".to_string(),
            "slnt" => "Slant".to_string(),
            _ => val.name,
        };
        let mut ax = Axis::new(
            name,
            tag_from_string(&val.tag).unwrap_or(Tag::new(&[0, 0, 0, 0])),
        );
        ax.min = val.minimum.map(UserCoord::new);
        ax.max = val.maximum.map(UserCoord::new);
        ax.default = val.default.map(UserCoord::new);
        if let Some(map) = val.axisGraph {
            let mut axismap = vec![];
            for (left, right) in map.iter() {
                if let Ok(l_f64) = left.parse::<f64>() {
                    axismap.push((UserCoord::new(*right), DesignCoord::new(l_f64)));
                }
            }
            axismap.sort();
            ax.map = Some(axismap);
        }
        ax
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct FontlabInstance {
    name: String,
    tsn: String,
    sgn: String,
    location: HashMap<String, f32>,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabFontInfo {
    tfn: String,
    sgn: String,
    creationDate: String,
    copyright: Option<String>,
    trademark: Option<String>,
    designer: Option<String>,
    designerURL: Option<String>,
    manufacturer: Option<String>,
    manufacturerURL: Option<String>,
    description: Option<String>,
    license: Option<String>,
    licenseURL: Option<String>,
    vendorID: Option<String>,
    versionMajor: Option<u16>,
    versionMinor: Option<u16>,
    version: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabMaster {
    name: String,
    tsn: String,
    sgn: String,
    ffn: String,
    psn: String,
    ascender: i32,
    descender: i32,
    xHeight: Option<i32>,
    capsHeight: Option<i32>,
    lineGap: Option<i32>,
    safeTop: Option<i32>,
    safeBottom: Option<i32>,
    underlineThickness: Option<i32>,
    underlinePosition: Option<i32>,
    #[serde(default)]
    location: HashMap<String, f64>,
    #[serde(default)]
    otherData: HashMap<String, serde_json::Value>, // coward
    #[serde(default)]
    kerning: serde_json::Value,
    #[serde(default)]
    measurements: Option<HashMap<String, String>>,
    #[serde(default)]
    guidelines: Vec<FontlabGuide>,
}

impl FontlabMaster {
    fn into(self, axes: &HashMap<String, Tag>) -> Master {
        let location: DesignLocation = Location::from(
            self.location
                .iter()
                .flat_map(|(short_name, val)| {
                    axes.get(short_name)
                        .map(|axis| (*axis, DesignCoord::new(*val)))
                })
                .collect::<Vec<_>>(),
        );
        let mut master = Master::new(self.name.clone(), self.name, location);

        // Direct vertical metrics from the top-level master fields
        master.metrics.insert(MetricType::Ascender, self.ascender);
        master.metrics.insert(MetricType::Descender, self.descender);
        master
            .metrics
            .insert(MetricType::TypoAscender, self.ascender);
        master
            .metrics
            .insert(MetricType::TypoDescender, self.descender);
        if let Some(xh) = self.xHeight {
            master.metrics.insert(MetricType::XHeight, xh);
        }
        if let Some(ch) = self.capsHeight {
            master.metrics.insert(MetricType::CapHeight, ch);
        }
        if let Some(ut) = self.underlineThickness {
            master.metrics.insert(MetricType::UnderlineThickness, ut);
        }
        if let Some(up) = self.underlinePosition {
            master.metrics.insert(MetricType::UnderlinePosition, up);
        }
        if let Some(lg) = self.lineGap {
            master.metrics.insert(MetricType::TypoLineGap, lg);
        }
        if let Some(top) = self.safeTop {
            master.metrics.insert(MetricType::WinAscent, top);
        }
        if let Some(bottom) = self.safeBottom {
            master.metrics.insert(MetricType::WinDescent, bottom.abs());
        }

        // Helper to extract an i32 from otherData
        let extract = |key: &str| -> Option<i32> {
            self.otherData.get(key).and_then(|v| {
                if let Some(n) = v.as_i64() {
                    Some(n as i32)
                } else {
                    v.as_f64().map(|n| n as i32)
                }
            })
        };

        // hhea metrics from otherData
        if let Some(ha) = extract("hhea_ascender") {
            master.metrics.insert(MetricType::HheaAscender, ha);
        }
        if let Some(hd) = extract("hhea_descender") {
            master.metrics.insert(MetricType::HheaDescender, hd);
        }
        if let Some(hlg) = extract("hhea_line_gap") {
            master.metrics.insert(MetricType::HheaLineGap, hlg);
        }

        // Strikeout metrics from otherData
        if let Some(sp) = extract("strikeout_position") {
            master.metrics.insert(MetricType::StrikeoutPosition, sp);
        }
        if let Some(ss) = extract("strikeout_size") {
            master.metrics.insert(MetricType::StrikeoutSize, ss);
        }

        // Subscript metrics from otherData
        if let Some(v) = extract("subscript_x_size") {
            master.metrics.insert(MetricType::SubscriptXSize, v);
        }
        if let Some(v) = extract("subscript_y_size") {
            master.metrics.insert(MetricType::SubscriptYSize, v);
        }
        if let Some(v) = extract("subscript_x_offset") {
            master.metrics.insert(MetricType::SubscriptXOffset, v);
        }
        if let Some(v) = extract("subscript_y_offset") {
            master.metrics.insert(MetricType::SubscriptYOffset, v);
        }

        // Superscript metrics from otherData
        if let Some(v) = extract("superscript_x_size") {
            master.metrics.insert(MetricType::SuperscriptXSize, v);
        }
        if let Some(v) = extract("superscript_y_size") {
            master.metrics.insert(MetricType::SuperscriptYSize, v);
        }
        if let Some(v) = extract("superscript_x_offset") {
            master.metrics.insert(MetricType::SuperscriptXOffset, v);
        }
        if let Some(v) = extract("superscript_y_offset") {
            master.metrics.insert(MetricType::SuperscriptYOffset, v);
        }

        // Italic angle and overshoots from the measurements dict (values are strings)
        if let Some(ref measurements) = self.measurements {
            let meas = |key: &str| -> Option<i32> {
                measurements.get(key).and_then(|s| s.parse::<i32>().ok())
            };
            if let Some(ia) = meas("ItalicAngle") {
                master.metrics.insert(MetricType::ItalicAngle, ia);
            }
        }

        master.guides = self.guidelines.iter().map(Guide::from).collect();
        master
    }
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabMasterWrapper {
    fontMaster: FontlabMaster,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct FontlabFont {
    glyphsCount: u16,
    upm: u16,
    #[serde(default)]
    glyphs: Vec<FontlabGlyph>,
    #[serde(default)]
    axes: Vec<FontlabAxis>,
    #[serde(default)]
    instances: Vec<FontlabInstance>,
    defaultMaster: Option<String>,
    currentMaster: Option<String>,
    masters: Vec<FontlabMasterWrapper>,
    // classes: Vec<FontlabClass>,
    // openTypeFeatures: XXX,
    // hinting: XXX,
    info: FontlabFontInfo,
}

#[derive(Serialize, Deserialize, Debug)]
struct FontlabFontWrapper {
    version: u8,
    font: FontlabFont,
}

/// Load a Fontlab VFJ font from a file path
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let s = fs::read_to_string(&path)?;
    load_str(&s)
}

fn kerning_value(value: &serde_json::Value) -> Option<i16> {
    if let Some(number) = value.as_i64() {
        Some(number as i16)
    } else if let Some(number) = value.as_f64() {
        Some(number.round() as i16)
    } else if let Some(text) = value.as_str() {
        text.parse::<i16>()
            .ok()
            .or_else(|| text.parse::<f64>().ok().map(|number| number.round() as i16))
    } else {
        None
    }
}

fn normalize_kerning_key(key: &str) -> SmolStr {
    if let Some(name) = key.strip_prefix("@MMK_L_") {
        format!("@{}", name).into()
    } else if let Some(name) = key.strip_prefix("@MMK_R_") {
        format!("@{}", name).into()
    } else {
        key.into()
    }
}

fn merge_kerning_group(
    groups: &mut IndexMap<SmolStr, Vec<SmolStr>>,
    name: &str,
    glyphs: &[String],
) {
    let entry = groups.entry(name.into()).or_default();
    for glyph in glyphs {
        let glyph: SmolStr = glyph.as_str().into();
        if !entry.contains(&glyph) {
            entry.push(glyph);
        }
    }
}

/// Load FontLab VFJ kerning. Each master carries its own copy of the class
/// definitions and kerning pairs.
fn apply_vfj_kerning(font: &mut Font, master_kerning: &[serde_json::Value]) {
    // Collect class definitions from every master, merging duplicates.
    for value in master_kerning {
        let Some(classes) = value.get("kerningClasses").and_then(|v| v.as_array()) else {
            continue;
        };
        for class in classes {
            let Some(name) = class.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let glyphs: Vec<String> = class
                .get("names")
                .and_then(|v| v.as_array())
                .map(|names| {
                    names
                        .iter()
                        .filter_map(|name| name.as_str().map(|name| name.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            if glyphs.is_empty() {
                continue;
            }
            let first = class.get("1st").and_then(|v| v.as_bool()).unwrap_or(false);
            let second = class.get("2nd").and_then(|v| v.as_bool()).unwrap_or(false);
            if first || (!first && !second) {
                merge_kerning_group(&mut font.first_kern_groups, name, &glyphs);
            }
            if second {
                merge_kerning_group(&mut font.second_kern_groups, name, &glyphs);
            }
        }
    }

    // The pair map may either live under `pairs` or be the kerning object itself.
    for (master_index, value) in master_kerning.iter().enumerate() {
        let Some(pairs) = value.get("pairs").and_then(|v| v.as_object()).or_else(|| {
            if value.is_object() && value.get("kerningClasses").is_none() {
                value.as_object()
            } else {
                None
            }
        }) else {
            continue;
        };
        for (left, right_map) in pairs {
            let Some(right_map) = right_map.as_object() else {
                continue;
            };
            for (right, kern) in right_map {
                let Some(kern) = kerning_value(kern) else {
                    continue;
                };
                if let Some(master) = font.masters.get_mut(master_index) {
                    master.kerning.insert(
                        (normalize_kerning_key(left), normalize_kerning_key(right)),
                        kern,
                    );
                }
            }
        }
    }
}

/// Copy the font-level names from the VFJ `info` block into the Babelfont
/// name table.
fn apply_font_info(
    font: &mut Font,
    info: &FontlabFontInfo,
    default_master: Option<&str>,
    postscript_name: Option<&str>,
) {
    fn set_name(target: &mut crate::i18ndictionary::I18NDictionary, value: &str) {
        if !value.is_empty() {
            *target = value.to_string().into();
        }
    }

    // `tfn` is the typographic family name; `sgn` is the style-group family
    // name, which FontLab also uses as the full font name.
    set_name(&mut font.names.family_name, &info.tfn);
    set_name(&mut font.names.typographic_family, &info.tfn);
    set_name(&mut font.names.full_name, &info.sgn);
    if let Some(value) = &info.copyright {
        set_name(&mut font.names.copyright, value);
    }
    if let Some(value) = &info.trademark {
        set_name(&mut font.names.trademark, value);
    }
    if let Some(value) = &info.designer {
        set_name(&mut font.names.designer, value);
    }
    if let Some(value) = &info.designerURL {
        set_name(&mut font.names.designer_url, value);
    }
    if let Some(value) = &info.manufacturer {
        set_name(&mut font.names.manufacturer, value);
    }
    if let Some(value) = &info.manufacturerURL {
        set_name(&mut font.names.manufacturer_url, value);
    }
    if let Some(value) = &info.description {
        set_name(&mut font.names.description, value);
    }
    if let Some(value) = &info.license {
        set_name(&mut font.names.license, value);
    }
    if let Some(value) = &info.licenseURL {
        set_name(&mut font.names.license_url, value);
    }
    if let Some(value) = &info.version {
        set_name(&mut font.names.version, value);
    }
    if let (Some(major), Some(minor)) = (info.versionMajor, info.versionMinor) {
        font.version = (major, minor);
    }
    if let Some(name) = default_master {
        set_name(&mut font.names.preferred_subfamily_name, name);
        set_name(&mut font.names.typographic_subfamily, name);
    }
    if let Some(psn) = postscript_name {
        set_name(&mut font.names.postscript_name, psn);
    }
}

/// Load a Fontlab VFJ font from string contents
pub fn load_str(contents: &str) -> Result<Font, BabelfontError> {
    let mut axes_short_name_to_tag: HashMap<String, _> = HashMap::new();
    log::debug!("Parsing to internal structs");
    let mut font = Font::new();
    let p: FontlabFontWrapper = serde_json::from_str(contents)
        .map_err(|e| BabelfontError::General(format!("Couldn't parse VFJ: {:}", e)))?;
    let fontlab = p.font;
    // log::debug!("{:#?}", fontlab);
    let default_master_name = fontlab.defaultMaster.clone();
    let default_postscript_name = default_master_name.as_ref().and_then(|name| {
        fontlab
            .masters
            .iter()
            .find(|master| &master.fontMaster.name == name)
            .map(|master| master.fontMaster.psn.clone())
    });
    for axis in fontlab.axes {
        let sn = axis.shortName.clone();
        let new_axis: Axis = axis.into();
        axes_short_name_to_tag.insert(sn, new_axis.tag);
        font.axes.push(new_axis);
    }
    let mut master_kerning = Vec::new();
    for master in fontlab.masters {
        master_kerning.push(master.fontMaster.kerning.clone());
        font.masters
            .push(master.fontMaster.into(&axes_short_name_to_tag));
    }
    apply_vfj_kerning(&mut font, &master_kerning);
    apply_font_info(
        &mut font,
        &fontlab.info,
        default_master_name.as_deref(),
        default_postscript_name.as_deref(),
    );
    if let Some(default_master) = default_master_name.and_then(|name| font.master(&name).cloned()) {
        let axes: Result<Vec<fontdrasil::types::Axis>, _> =
            font.axes.iter().map(|a| a.try_into()).collect();
        let new_loc = default_master.location.to_user(&Axes::from(axes?))?;
        for axis in font.axes.iter_mut() {
            if let Some(val) = new_loc.get(axis.tag) {
                axis.default = Some(val);
            }
        }
        assert!(font.default_master_index().is_some())
    }
    for glyph in fontlab.glyphs {
        let new_glyph = glyph.try_into(&font)?;
        font.glyphs.push(new_glyph);
    }

    font.upm = fontlab.upm;
    Ok(font)
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_layer_cubic() {
        let layer = r#"
            {
              "name":"Bold",
              "advanceWidth":641,
              "elements":[
                {
                  "elementData":{
                    "contours":[
                      {
                        "nodes":[
                          "145 180",
                          "146 229  163 271  197 305 s",
                          "231 339  273 356  321 356",
                          "369 357  410 339  444 305 s",
                          "478 271  495 229  495 181 s",
                          "495 133  478 91  444 57 s",
                          "410 22  369 5  320 5 s",
                          "280 5  241 19  209 45 s",
                          "186 64  169 88  158 113 s",
                          "149 134  145 157 c"
                        ]
                      },
                      {
                        "nodes":[
                          "213 1437",
                          "431 1437",
                          "472 1437",
                          "471 1396",
                          "446 488",
                          "445 449",
                          "406 449",
                          "239 449",
                          "200 449",
                          "199 488",
                          "173 1396",
                          "172 1437"
                        ]
                      }
                    ]
                  }
                }
              ]
          }
        "#;
        let fl_layer = serde_json::from_str::<FontlabLayer>(layer).unwrap();
        let bb_layer = fl_layer.try_into_babel(&Font::new()).unwrap();
        assert!(bb_layer.shapes.len() == 2);
        let first_shape = &bb_layer.shapes[0];
        match first_shape {
            Shape::Path(path) => {
                assert_eq!(path.nodes.len(), 27);
                // After rotation, the start point moves to the end.
                assert_eq!(path.nodes[0].x, 146.0);
                assert_eq!(path.nodes[0].y, 229.0);
                assert_eq!(path.nodes[0].nodetype, NodeType::OffCurve);
                assert_eq!(path.nodes[1].nodetype, NodeType::OffCurve);
                // The last node is the original start point
                assert_eq!(path.nodes[26].x, 145.0);
                assert_eq!(path.nodes[26].y, 180.0);
                assert_eq!(path.nodes[26].nodetype, NodeType::Curve);
                // No 's' suffix in VFJ, so smooth is false
                assert!(!path.nodes[26].smooth);
            }
            _ => panic!("Expected path shape"),
        }
    }

    #[test]
    fn test_parse_layer_quadratic() {
        let layer = r#"
              {
                "name":"Bold",
                "advanceWidth":641,
                "elements":[
                  {
                    "elementData":{
                      "contours":[
                        {
                          "nodes":[
                            "145 180 s",
                            "145 253 o",
                            "248 356 o",
                            "393 356 o",
                            "495 253 o",
                            "495 108 o",
                            "393 5 o",
                            "320 5 s",
                            "300 5 o",
                            "261 14 o",
                            "225 32 o",
                            "209 45 s",
                            "192 59 o",
                            "166 94 o",
                            "158 113 s",
                            "145 145 o"
                          ]
                        },
                        {
                          "nodes":[
                            "213 1437",
                            "431 1437",
                            "472 1437",
                            "471 1396",
                            "446 488",
                            "445 449",
                            "406 449",
                            "239 449",
                            "200 449",
                            "199 488",
                            "173 1396",
                            "172 1437"
                          ]
                        }
                      ]
                    }
                  }
                ]
            }
"#;
        let fl_layer = serde_json::from_str::<FontlabLayer>(layer).unwrap();
        let bb_layer = fl_layer.try_into_babel(&Font::new()).unwrap();
        assert!(bb_layer.shapes.len() == 2);
        let first_shape = &bb_layer.shapes[0];
        match first_shape {
            Shape::Path(path) => {
                assert_eq!(path.nodes.len(), 16);
                // After rotation to Glyphs convention, the start point moves to
                // the end and the path begins with the first off-curve.
                assert_eq!(path.nodes[0].x, 145.0);
                assert_eq!(path.nodes[0].y, 253.0);
                assert_eq!(path.nodes[0].nodetype, NodeType::OffCurve);
                assert_eq!(path.nodes[1].nodetype, NodeType::OffCurve);
                // The last node is the original start point (on-curve, smooth)
                assert_eq!(path.nodes[15].x, 145.0);
                assert_eq!(path.nodes[15].y, 180.0);
                assert_eq!(path.nodes[15].nodetype, NodeType::QCurve);
                assert!(path.nodes[15].smooth);
            }
            _ => panic!("Expected path shape"),
        }
    }

    #[test]
    fn test_sepehr_all_glyphs() {
        let vfj_font =
            crate::load("resources/fontlab/Sepehr-Bold.vfj").expect("Failed to load VFJ");
        let ref_font = crate::load("resources/fontlab/Sepehr-Bold.babelfont")
            .expect("Failed to load reference");

        for ref_glyph in ref_font.glyphs.iter() {
            let name = &ref_glyph.name;
            let vfj_glyph = match vfj_font.glyphs.get(name) {
                Some(g) => g,
                None => {
                    panic!("Glyph {} missing from VFJ", name);
                }
            };

            assert_eq!(
                vfj_glyph.layers.len(),
                ref_glyph.layers.len(),
                "{}: layer count differs",
                name
            );

            let vfj_shapes = &vfj_glyph.layers[0].shapes;
            let ref_shapes = &ref_glyph.layers[0].shapes;

            assert_eq!(
                vfj_shapes.len(),
                ref_shapes.len(),
                "{}: shape count differs",
                name
            );

            for (i, (vfj_shape, ref_shape)) in vfj_shapes.iter().zip(ref_shapes.iter()).enumerate()
            {
                match (vfj_shape, ref_shape) {
                    (Shape::Path(vfj_path), Shape::Path(ref_path)) => {
                        assert_eq!(vfj_path, ref_path, "{} shape[{}] path differs", name, i);
                    }
                    (Shape::Component(vfj_comp), Shape::Component(ref_comp)) => {
                        assert_eq!(
                            vfj_comp.reference, ref_comp.reference,
                            "{} shape[{}] component reference differs",
                            name, i
                        );
                        // Translation from VFJ is exact (integer offsets)
                        assert_eq!(
                            vfj_comp.transform.translation, ref_comp.transform.translation,
                            "{} shape[{}] translation differs: {:?} vs {:?}",
                            name, i, vfj_comp.transform.translation, ref_comp.transform.translation
                        );
                        // Scale may differ slightly due to Glyphs round-trip
                        // precision loss in decomposed transforms
                        let (vfj_sx, vfj_sy) = vfj_comp.transform.scale;
                        let (ref_sx, ref_sy) = ref_comp.transform.scale;
                        assert!(
                            (vfj_sx - ref_sx).abs() < 0.001,
                            "{} shape[{}] xScale {:.8} differs from {:.8}",
                            name,
                            i,
                            vfj_sx,
                            ref_sx
                        );
                        assert!(
                            (vfj_sy - ref_sy).abs() < 0.001,
                            "{} shape[{}] yScale {:.8} differs from {:.8}",
                            name,
                            i,
                            vfj_sy,
                            ref_sy
                        );
                        // Rotation and skew should be zero
                        assert_eq!(
                            vfj_comp.transform.rotation, ref_comp.transform.rotation,
                            "{} shape[{}] rotation differs",
                            name, i
                        );
                        assert_eq!(
                            vfj_comp.transform.skew, ref_comp.transform.skew,
                            "{} shape[{}] skew differs",
                            name, i
                        );
                        // Order must match
                        assert_eq!(
                            vfj_comp.transform.order, ref_comp.transform.order,
                            "{} shape[{}] order differs",
                            name, i
                        );
                    }
                    _ => {
                        panic!(
                            "{} shape[{}] type mismatch: {:?} vs {:?}",
                            name, i, vfj_shape, ref_shape
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_sepehr_kerning() {
        let font = crate::load("resources/fontlab/Sepehr-Bold.vfj").expect("Failed to load VFJ");

        // Classes from the first master's `kerningClasses` list.
        assert_eq!(
            font.first_kern_groups.get("A").map(|glyphs| glyphs.len()),
            Some(2)
        );
        assert!(font.second_kern_groups.contains_key("C"));
        assert_eq!(
            font.second_kern_groups.get("C").map(|glyphs| glyphs.len()),
            Some(4)
        );

        let master = &font.masters[0];
        assert_eq!(
            master
                .kerning
                .get(&(SmolStr::new("@A"), SmolStr::new("@C"))),
            Some(&-21)
        );
        assert_eq!(
            master
                .kerning
                .get(&(SmolStr::new("A"), SmolStr::new("question"))),
            Some(&-81)
        );
        assert!(
            master.kerning.len() > 600,
            "expected >600 kerning pairs, got {}",
            master.kerning.len()
        );
    }
}
