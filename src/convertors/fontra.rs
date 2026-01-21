use fontdrasil::coords::{NormalizedCoord, UserCoord};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
/// A Fontra axis
pub struct Axis {
    /// The name of the axis
    pub name: String,
    /// The label of the axis
    pub label: String,
    /// The tag of the axis
    pub tag: String,
    /// The minimum value of the axis
    #[serde(rename = "minValue")]
    pub min_value: f64,
    /// The maximum value of the axis
    #[serde(rename = "maxValue")]
    pub max_value: f64,
    /// The default value of the axis
    #[serde(rename = "defaultValue")]
    pub default_value: f64,
    /// Whether the axis is hidden
    pub hidden: bool,
    /// The map of user to normalized coordinates
    pub mapping: Vec<(UserCoord, NormalizedCoord)>,
}

/// A Fontra axes definition
#[derive(Serialize, Deserialize, Default)]
pub struct Axes {
    /// The list of axes
    pub axes: Vec<Axis>,
    /// The cross-axis mappings
    pub mappings: Vec<String>, // Should be a cross-axis mapping
    /// The elided fallback name
    #[serde(rename = "elidedFallBackname")]
    pub elided_fall_backname: String,
}

/// A Fontra guideline
#[derive(Serialize, Deserialize, Default)]
pub struct Guideline {
    /// The name of the guideline
    pub name: Option<String>,
    /// The x position of the guideline
    pub x: f32,
    /// The y position of the guideline
    pub y: f32,
    /// The angle of the guideline
    pub angle: f32,
    /// Whether the guideline is locked
    pub locked: bool,
}

/// A Fontra source
#[derive(Serialize, Deserialize, Default)]
pub struct Source {
    /// The name of the source
    pub name: String,
    /// Whether the source is sparse
    #[serde(rename = "isSparse")]
    pub is_sparse: String,
    /// The location of the source in ... design space coordinates?
    pub location: HashMap<String, f64>,
    /// The italic angle of the source
    #[serde(rename = "italicAngle")]
    pub italic_angle: f32,
    /// Master-level guidelines
    pub guidelines: Vec<Guideline>,
    /// Custom data associated with the source
    #[serde(rename = "customData")]
    pub custom_data: HashMap<String, String>,
}

/// A Fontra backend info structure
#[derive(Serialize, Deserialize, Default)]
pub struct BackendInfo {
    /// A list of features supported by the backend
    pub features: Vec<String>,
    /// Project manager specific features
    #[serde(rename = "projectManagerFeatures")]
    pub project_manager_features: Value,
}

/// A Fontra glyph axis definition
///
/// Currently empty, but reserved for future use
#[derive(Serialize, Deserialize, Default)]
pub struct GlyphAxis {}

/// A Fontra glyph source definition

#[derive(Serialize, Deserialize, Default)]
pub struct GlyphSource {
    /// The name of the glyph source
    pub name: String,
    /// The name of the layer
    #[serde(rename = "layerName")]
    pub layer_name: String,
    /// The location of the glyph source in design space coordinates
    pub location: HashMap<String, f64>,
}

/// A Fontra layer definition
#[derive(Serialize, Deserialize, Default)]
pub struct Layer {
    /// The static glyph associated with the layer
    pub glyph: StaticGlyph,
}

/// A Fontra static glyph definition
#[derive(Serialize, Deserialize, Default)]
pub struct StaticGlyph {
    /// The glyph's path
    pub path: PackedPath,
    /// The components of the glyph
    pub components: Vec<Component>,
    /// The horizontal advance of the glyph
    #[serde(rename = "xAdvance")]
    pub x_advance: f32,
    /// The vertical advance of the glyph
    #[serde(rename = "yAdvance")]
    pub y_advance: f32,
    /// The anchors of the glyph
    pub anchors: Vec<Anchor>,
    /// The guides of the glyph
    pub guides: Vec<Guideline>,
}

/// A Fontra component definition
#[derive(Serialize, Deserialize, Default)]
pub struct Component {
    /// The name of the referenced glyph
    pub name: String,
    /// The transformation applied to the component
    pub transformation: DecomposedTransform,
    /// The location of the component in design space coordinates
    pub location: HashMap<String, f32>,
}

/// A Fontra anchor definition
#[derive(Serialize, Deserialize, Default)]
pub struct Anchor {
    /// The name of the anchor
    pub name: String,
    /// The x coordinate of the anchor
    pub x: f32,
    /// The y coordinate of the anchor
    pub y: f32,
}

/// A Fontra decomposed transform
#[derive(Serialize, Deserialize, Default)]
pub struct DecomposedTransform {
    /// The translation in the x direction
    #[serde(rename = "translateX")]
    pub translate_x: f32,
    /// The translation in the y direction
    #[serde(rename = "translateY")]
    pub translate_y: f32,
    /// The rotation angle
    pub rotation: f32,
    /// The scaling in the x direction
    #[serde(rename = "scaleX")]
    pub scale_x: f32,
    /// The scaling in the y direction
    #[serde(rename = "scaleY")]
    pub scale_y: f32,
    /// The skewing in the x direction
    #[serde(rename = "skewX")]
    pub skew_x: f32,
    /// The skewing in the y direction
    #[serde(rename = "skewY")]
    pub skew_y: f32,
    /// The transform center x coordinate
    #[serde(rename = "tCenterX")]
    pub t_center_x: f32,
    /// The transform center y coordinate
    #[serde(rename = "tCenterY")]
    pub t_center_y: f32,
}

/// A Fontra contour info definition
#[derive(Serialize, Deserialize, Default)]
pub struct ContourInfo {
    /// The end point index of the contour
    #[serde(rename = "endPoint")]
    pub end_point: usize,
    /// Whether the contour is closed
    #[serde(rename = "isClosed")]
    pub is_closed: bool,
}

/// A Fontra path structure
#[derive(Serialize, Deserialize, Default)]
pub struct PackedPath {
    /// The coordinates of the points in the path
    pub coordinates: Vec<f32>,
    #[serde(rename = "pointTypes")]
    /// The types of points in the path
    pub point_types: Vec<i32>,
    /// Further contour information
    #[serde(rename = "contourInfo")]
    pub contour_info: Vec<ContourInfo>,
}

impl PackedPath {
    /// Add a Babelfont path to this PackedPath
    pub fn push_path(&mut self, babelfont: &crate::Path) {
        for node in babelfont.nodes.iter() {
            self.coordinates.push(node.x as f32);
            self.coordinates.push(node.y as f32);
            if node.nodetype != crate::NodeType::OffCurve {
                self.point_types.push(0);
            } else {
                self.point_types.push(1);
            }
        }
        self.contour_info.push(ContourInfo {
            end_point: self.coordinates.len() / 2 - 1,
            is_closed: babelfont.closed,
        })
    }
}

/// A Fontra glyph definition
#[derive(Serialize, Deserialize, Default)]
pub struct Glyph {
    /// The name of the glyph
    pub name: String,
    /// Any glyph-specific axes
    pub axes: Vec<GlyphAxis>,
    /// The sources for the glyph
    pub sources: Vec<GlyphSource>,
    /// The layers of the glyph
    pub layers: HashMap<String, Layer>,
}

/// Font-level information in Fontra format
#[derive(Serialize, Deserialize, Default)]
pub struct FontInfo {
    /// The family name of the font
    #[serde(rename = "familyName")]
    pub family_name: Option<String>,
    /// The major version number of the font
    #[serde(rename = "versionMajor")]
    pub version_major: Option<u16>,
    /// The minor version number of the font
    #[serde(rename = "versionMinor")]
    pub version_minor: Option<u16>,
    /// The copyright information of the font (OpenType Name ID 0)
    pub copyright: Option<String>,
    /// The trademark information of the font (OpenType Name ID 7)
    pub trademark: Option<String>,
    /// The description of the font (OpenType Name ID 10)
    pub description: Option<String>,
    /// Sample text for the font (OpenType Name ID 19)
    #[serde(rename = "sampleText")]
    pub sample_text: Option<String>,
    /// The designer of the font (OpenType Name ID 9)
    pub designer: Option<String>,
    /// The designer URL of the font (OpenType Name ID 12)
    #[serde(rename = "designerURL")]
    pub designer_url: Option<String>,
    /// The manufacturer of the font (OpenType Name ID 8)
    pub manufacturer: Option<String>,
    /// The manufacturer URL of the font (OpenType Name ID 11)
    #[serde(rename = "manufacturerURL")]
    pub manufacturer_url: Option<String>,
    /// The license description of the font (OpenType Name ID 13)
    #[serde(rename = "licenseDescription")]
    pub license_description: Option<String>,
    /// The license info URL of the font (OpenType Name ID 14)
    #[serde(rename = "licenseInfoURL")]
    pub license_info_url: Option<String>,
    /// The vendor ID of the font for the OS/2 table
    #[serde(rename = "vendorID")]
    pub vendor_id: Option<String>,
    /// Any custom data associated with the font
    #[serde(rename = "customData")]
    pub custom_data: HashMap<String, String>,
}
