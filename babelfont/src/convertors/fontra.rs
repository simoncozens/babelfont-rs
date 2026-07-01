#![allow(missing_docs)] // Fontra structs aren't documented yet because we don't know what they mean
use fontdrasil::coords::{DesignCoord, Location, UserCoord};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf};

use crate::{
    BabelfontError, CustomOTValues, Font, FormatSpecific, Glyph, GlyphList, LayerType, Master,
    Position,
};

fn custom_data_to_format_specific(cd: &HashMap<String, Value>) -> FormatSpecific {
    let mut fs = FormatSpecific::default();
    for (key, value) in cd {
        fs.insert(key.clone(), value.clone());
    }
    fs
}

pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let mut font_data: FontraFont = serde_json::from_str(
        &std::fs::read_to_string(path.join("font-data.json"))
            .map_err(|e| BabelfontError::IO(e.to_string()))?,
    )?;
    let kerning = std::mem::take(&mut font_data.kerning);
    let mut our_font = Font::try_from(font_data)?;

    // Move kerning into master structures
    for kerning in kerning.values() {
        // We don't use the feature name at this point
        for (left_glyph, map) in &kerning.values {
            for (right_glyph, values) in map {
                for (master, value) in our_font.masters.iter_mut().zip(values) {
                    if let Some(v) = value {
                        master
                            .kerning
                            .insert((left_glyph.into(), right_glyph.into()), *v as i16);
                    }
                }
            }
        }
    }

    // Set up basic glyphs structures
    let mut rdr = csv::ReaderBuilder::new().delimiter(b';').from_reader(
        std::fs::File::open(path.join("glyph-info.csv"))
            .map_err(|e| BabelfontError::IO(e.to_string()))?,
    );
    let header_names = rdr
        .headers()
        .map_err(|e| BabelfontError::IO(e.to_string()))?
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>();
    for results in rdr.records() {
        let record = results.map_err(|e| BabelfontError::IO(e.to_string()))?;
        let glyph_name = record.get(0).unwrap_or_default().to_string();
        let codepoints: Vec<u32> = record
            .get(1)
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|s| u32::from_str_radix(&s.replace("U+", ""), 16).ok())
            .collect();
        let mut other_info = HashMap::new();
        for (i, field) in record.iter().enumerate().skip(2) {
            let header = &header_names[i];
            other_info.insert(header.clone(), field.to_string());
        }
        let mut fs = FormatSpecific::default();
        fs.insert(
            "fontra.glyph_info".into(),
            serde_json::to_value(other_info).unwrap_or_default(),
        );
        our_font.glyphs.push(Glyph {
            name: glyph_name.into(),
            production_name: None,
            category: crate::GlyphCategory::Unknown,
            codepoints,
            layers: vec![],
            exported: true,
            direction: None,
            component_axes: vec![],
            format_specific: fs,
        });
    }

    // Now load in the glyphs. Just read all the JSON files in the glyphs directory and deserialize them into VariableGlyphs.
    let glyphs_dir = path.join("glyphs");
    if glyphs_dir.is_dir() {
        for entry in std::fs::read_dir(glyphs_dir).map_err(|e| BabelfontError::IO(e.to_string()))? {
            let entry = entry.map_err(|e| BabelfontError::IO(e.to_string()))?;
            let path = entry.path();
            if path.is_file() && path.extension().map(|s| s == "json").unwrap_or(false) {
                let glyph_data: VariableGlyph = serde_json::from_str(
                    &std::fs::read_to_string(&path)
                        .map_err(|e| BabelfontError::IO(e.to_string()))?,
                )?;
                // Find the corresponding glyph in our_font.glyphs and update its format_specific with the VariableGlyph data
                if let Some(glyph) = our_font
                    .glyphs
                    .iter_mut()
                    .find(|g| g.name == glyph_data.name)
                {
                    // Transfer variable glyph data to glyph
                    // Layer sources give us names and locations.
                    let source_map = glyph_data
                        .sources
                        .iter()
                        .map(|s| (s.name.clone(), s))
                        .collect::<HashMap<_, _>>();
                    glyph.component_axes = glyph_data.axes.iter().map(|a| a.into()).collect();
                    glyph.layers = glyph_data
                        .layers
                        .iter()
                        .map(|(source_id, layer)| load_layer(layer, source_map.get(source_id)))
                        .collect();
                }
            }
        }
    }

    Ok(our_font)
}

#[allow(dead_code)]
pub struct GlyphInfo {
    glyph_name: String,
    code_points: Vec<u32>,
    other_info: HashMap<String, String>,
}

fn default_upm() -> u16 {
    1000
}

/// Top-level Fontra font structure — corresponds to Python `Font` class.
#[derive(Serialize, Deserialize, Debug)]
pub struct FontraFont {
    #[serde(rename = "unitsPerEm", default = "default_upm")]
    pub units_per_em: u16,
    #[serde(rename = "fontInfo", default)]
    pub font_info: FontInfo,
    #[serde(default)]
    pub axes: Axes,
    #[serde(default)]
    pub sources: HashMap<String, Source>,
    #[serde(default)]
    pub glyphs: HashMap<String, VariableGlyph>,
    #[serde(rename = "glyphMap", default)]
    pub glyph_map: HashMap<String, Vec<u32>>,
    #[serde(rename = "glyphInfos", default)]
    pub glyph_infos: HashMap<String, Value>,
    #[serde(default)]
    // Opentype feature -> kerning object
    pub kerning: HashMap<String, Kerning>,
    #[serde(default)]
    pub features: OpenTypeFeatures,
    #[serde(rename = "conditionalSubstitutions", default)]
    pub conditional_substitutions: ConditionalSubstitutions,
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

impl TryFrom<FontraFont> for Font {
    type Error = BabelfontError;

    fn try_from(fontra: FontraFont) -> Result<Self, BabelfontError> {
        let axes: Vec<crate::Axis> = fontra
            .axes
            .axes
            .iter()
            .map(|x| x.try_into())
            .collect::<Result<_, _>>()?;
        let cross_axis_mappings = fontra
            .axes
            .mappings
            .iter()
            .map(|x| crate::axis::fontra::cross_axis_mapping_from_fontra(x, &axes))
            .collect::<Result<Vec<crate::axis::CrossAxisMapping>, _>>()?;
        let mut font = Font {
            upm: fontra.units_per_em,
            version: (
                fontra.font_info.version_major.unwrap_or(1),
                fontra.font_info.version_minor.unwrap_or(0),
            ),
            axes,
            cross_axis_mappings,
            instances: vec![],
            masters: fontra
                .sources
                .iter()
                .map(|(id, source)| load_master(id, source))
                .collect::<Result<Vec<_>, _>>()?,
            glyphs: GlyphList::default(),
            note: fontra
                .font_info
                .custom_data
                .get("note")
                .and_then(|v| v.as_str().map(|s| s.to_string())),
            date: chrono::Utc::now(), // Does fontra care?
            names: (&fontra.font_info).into(),
            custom_ot_values: crate::CustomOTValues::default(),
            variation_sequences: Default::default(),
            features: fontra.features.into(),
            first_kern_groups: Default::default(), // Not sure.
            second_kern_groups: Default::default(),
            format_specific: Default::default(),
            source: Default::default(),
        };

        // Populate the glyphs vector from the glyphs HashMap
        for (glyph_name, variable_glyph) in fontra.glyphs {
            let mut fs = FormatSpecific::default();
            fs.insert(
                "fontra.variable_glyph".into(),
                serde_json::to_value(variable_glyph).unwrap_or_default(),
            );
            font.glyphs.push(Glyph {
                name: glyph_name.clone().into(),
                production_name: None,
                category: crate::GlyphCategory::Unknown,
                codepoints: fontra
                    .glyph_map
                    .get(&glyph_name)
                    .cloned()
                    .unwrap_or_default(),
                layers: vec![],
                exported: true,
                direction: None,
                component_axes: vec![],
                format_specific: fs,
            });
        }

        Ok(font)
    }
}
/// A Fontra continuous axis — corresponds to Python `FontAxis` class.
#[derive(Serialize, Deserialize, Debug)]
pub struct FontAxis {
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
    #[serde(default)]
    pub hidden: bool,
    /// The map of user to normalized coordinates
    #[serde(default)]
    pub mapping: Vec<[f64; 2]>,
    /// Axis value labels
    #[serde(rename = "valueLabels", default)]
    pub value_labels: Vec<AxisValueLabel>,
    /// Custom data associated with the axis
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// A Fontra discrete axis — corresponds to Python `DiscreteFontAxis` class.
#[derive(Serialize, Deserialize, Debug)]
pub struct DiscreteFontAxis {
    /// The name of the axis
    pub name: String,
    /// The label of the axis
    pub label: String,
    /// The tag of the axis
    pub tag: String,
    /// The discrete values of the axis
    #[serde(default)]
    pub values: Vec<f64>,
    /// The default value of the axis
    #[serde(rename = "defaultValue")]
    pub default_value: f64,
    /// Whether the axis is hidden
    #[serde(default)]
    pub hidden: bool,
    /// The map of user to normalized coordinates
    #[serde(default)]
    pub mapping: Vec<[f64; 2]>,
    /// Axis value labels
    #[serde(rename = "valueLabels", default)]
    pub value_labels: Vec<AxisValueLabel>,
    /// Custom data associated with the axis
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// Enum to represent either a continuous or discrete Fontra axis.
#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum AnyAxis {
    /// A continuous axis with min/max
    Continuous(FontAxis),
    /// A discrete axis with a list of values
    Discrete(DiscreteFontAxis),
}

impl TryInto<crate::Axis> for &AnyAxis {
    type Error = BabelfontError;

    fn try_into(self) -> Result<crate::Axis, Self::Error> {
        Ok(match self {
            AnyAxis::Continuous(font_axis) => font_axis.try_into()?,
            AnyAxis::Discrete(discrete_font_axis) => {
                let map: Vec<(UserCoord, DesignCoord)> = discrete_font_axis
                    .mapping
                    .iter()
                    .map(|v| (UserCoord::new(v[0]), DesignCoord::new(v[1])))
                    .collect();
                crate::Axis {
                    name: discrete_font_axis.name.clone().into(),
                    tag: crate::Tag::new_checked(discrete_font_axis.tag.as_bytes())
                        .map_err(|e| BabelfontError::AxisConversion(e.to_string()))?,
                    min: None,
                    max: None,
                    default: None,
                    map: (!map.is_empty()).then_some(map),
                    hidden: discrete_font_axis.hidden,
                    values: discrete_font_axis
                        .values
                        .iter()
                        .map(|v| UserCoord::new(*v))
                        .collect(),
                    format_specific: FormatSpecific::default(),
                }
            }
        })
    }
}

/// A Fontra axis value label — corresponds to Python `AxisValueLabel` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct AxisValueLabel {
    /// The name of the label
    pub name: String,
    /// The value of the label
    pub value: f64,
    /// Minimum value for a range label
    #[serde(rename = "minValue", default, skip_serializing_if = "Option::is_none")]
    pub min_value: Option<f64>,
    /// Maximum value for a range label
    #[serde(rename = "maxValue", default, skip_serializing_if = "Option::is_none")]
    pub max_value: Option<f64>,
    /// Linked value for the label
    #[serde(
        rename = "linkedValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub linked_value: Option<f64>,
    /// Whether the label is elidable
    #[serde(default)]
    pub elidable: bool,
    /// Whether the label is an older sibling
    #[serde(rename = "olderSibling", default)]
    pub older_sibling: bool,
}

/// A Fontra single-axis mapping — corresponds to Python `SingleAxisMapping` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct SingleAxisMapping {
    /// The input user value
    #[serde(rename = "inputUserValue")]
    pub input_user_value: f64,
    /// The output user value
    #[serde(rename = "outputUserValue")]
    pub output_user_value: f64,
}

/// A Fontra axes definition — corresponds to Python `Axes` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Axes {
    /// The list of axes
    #[serde(default)]
    pub axes: Vec<AnyAxis>,
    /// The cross-axis mappings
    #[serde(default)]
    pub mappings: Vec<CrossAxisMapping>,
    /// The elided fallback name
    #[serde(
        rename = "elidedFallBackname",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub elided_fall_backname: Option<String>,
    /// Custom data associated with the axes
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// A Fontra cross-axis mapping definition — corresponds to Python `CrossAxisMapping` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct CrossAxisMapping {
    /// Optional description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional group description
    #[serde(
        rename = "groupDescription",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub group_description: Option<String>,
    /// The input location
    ///
    /// A mapping between axis names and design coordinates.
    #[serde(rename = "inputLocation")]
    pub input_location: HashMap<String, f64>,
    /// The output location
    #[serde(rename = "outputLocation")]
    pub output_location: HashMap<String, f64>,
    /// Whether the mapping is inactive
    #[serde(default)]
    pub inactive: bool,
}

/// A Fontra guideline — corresponds to Python `Guideline` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Guideline {
    /// The name of the guideline
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The x position of the guideline
    #[serde(default)]
    pub x: f64,
    /// The y position of the guideline
    #[serde(default)]
    pub y: f64,
    /// The angle of the guideline
    #[serde(default)]
    pub angle: f64,
    /// Whether the guideline is locked
    #[serde(default)]
    pub locked: bool,
    /// Custom data associated with the guideline
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

impl From<&Guideline> for crate::Guide {
    fn from(val: &Guideline) -> Self {
        crate::Guide {
            pos: Position {
                x: val.x as f32,
                y: val.y as f32,
                angle: val.angle as f32,
            },
            name: val.name.clone(),
            color: None,
            format_specific: custom_data_to_format_specific(&val.custom_data),
        }
    }
}

/// A Fontra line metric — corresponds to Python `LineMetric` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct LineMetric {
    /// The value of the metric
    pub value: f64,
    /// The zone of the metric
    #[serde(default)]
    pub zone: f64,
    /// Custom data associated with the metric
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// A Fontra source (master) — corresponds to Python `FontSource` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Source {
    /// The name of the source
    pub name: String,
    /// Whether the source is sparse
    #[serde(rename = "isSparse", default)]
    pub is_sparse: bool,
    /// The location of the source in design space coordinates
    #[serde(default)]
    pub location: HashMap<String, f64>,
    /// Horizontal line metrics
    #[serde(rename = "lineMetricsHorizontalLayout", default)]
    pub line_metrics_horizontal_layout: HashMap<String, LineMetric>,
    /// Vertical line metrics
    #[serde(rename = "lineMetricsVerticalLayout", default)]
    pub line_metrics_vertical_layout: HashMap<String, LineMetric>,
    /// The italic angle of the source
    #[serde(rename = "italicAngle", default)]
    pub italic_angle: f64,
    /// Master-level guidelines
    #[serde(default)]
    pub guidelines: Vec<Guideline>,
    /// Custom data associated with the source
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

fn load_master(id: &str, source: &Source) -> Result<Master, BabelfontError> {
    let location = source
        .location
        .iter()
        .map(|(axis, value)| {
            crate::Tag::new_checked(axis.as_bytes()).map(|t| (t, DesignCoord::new(*value)))
        })
        .collect::<Result<Vec<(_, _)>, _>>()
        .map_err(|e| BabelfontError::AxisConversion(e.to_string()))?
        .into_iter()
        .collect::<Location<_>>();
    Ok(Master {
        name: source.name.clone().into(),
        id: id.to_string(),
        location,
        guides: source
            .guidelines
            .iter()
            .map(|x| x.into())
            .collect::<Vec<_>>(),
        metrics: IndexMap::new(),
        kerning: IndexMap::new(),
        custom_ot_values: CustomOTValues::default(),
        format_specific: FormatSpecific::default(),
    })
}

/// A Fontra backend info structure
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct BackendInfo {
    /// A list of features supported by the backend
    pub features: Vec<String>,
    /// Project manager specific features
    #[serde(rename = "projectManagerFeatures")]
    pub project_manager_features: Value,
}

/// A Fontra OpenType features definition — corresponds to Python `OpenTypeFeatures` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct OpenTypeFeatures {
    /// The feature code language
    #[serde(default = "default_fea_language")]
    pub language: String,
    /// The feature code text
    #[serde(default)]
    pub text: String,
    /// Custom data associated with the features
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

fn default_fea_language() -> String {
    "fea".to_string()
}

/// A Fontra substitution condition — corresponds to Python `SubstitutionCondition` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct SubstitutionCondition {
    /// The axis name
    pub name: String,
    /// Minimum value
    #[serde(rename = "minValue", default, skip_serializing_if = "Option::is_none")]
    pub min_value: Option<f64>,
    /// Maximum value
    #[serde(rename = "maxValue", default, skip_serializing_if = "Option::is_none")]
    pub max_value: Option<f64>,
}

/// A Fontra substitution condition set — corresponds to Python `SubstitutionConditionSet` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct SubstitutionConditionSet {
    /// The list of conditions
    #[serde(default)]
    pub conditions: Vec<SubstitutionCondition>,
}

/// A Fontra substitution rule — corresponds to Python `SubstitionRule` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct SubstitionRule {
    /// Optional name of the rule
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The condition sets
    #[serde(rename = "conditionSets")]
    pub condition_sets: Vec<SubstitutionConditionSet>,
    /// The substitutions map (old name -> new name)
    #[serde(default)]
    pub substitutions: HashMap<String, String>,
}

/// A Fontra conditional substitutions definition — corresponds to Python `ConditionalSubstitutions` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct ConditionalSubstitutions {
    /// The feature tags to apply these substitutions to
    #[serde(rename = "featureTags", default = "default_rclt_feature")]
    pub feature_tags: Vec<String>,
    /// The substitution rules
    #[serde(default)]
    pub rules: Vec<SubstitionRule>,
}

fn default_rclt_feature() -> Vec<String> {
    vec!["rclt".to_string()]
}

/// A Fontra kerning definition — corresponds to Python `Kerning` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Kerning {
    /// First kerning groups (side 1)
    #[serde(rename = "groupsSide1", default)]
    pub groups_side1: HashMap<String, Vec<String>>,
    /// Second kerning groups (side 2)
    #[serde(rename = "groupsSide2", default)]
    pub groups_side2: HashMap<String, Vec<String>>,
    /// Source identifiers for the kerning values
    #[serde(rename = "sourceIdentifiers", default)]
    pub source_identifiers: Vec<String>,
    /// Kerning values: left glyph/group -> right glyph/group -> source index -> value
    #[serde(default)]
    pub values: HashMap<String, HashMap<String, Vec<Option<f64>>>>,
}

/// A Fontra glyph axis definition — corresponds to Python `GlyphAxis` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct GlyphAxis {
    /// The name of the axis
    pub name: String,
    /// The minimum value of the axis
    #[serde(rename = "minValue")]
    pub min_value: f64,
    /// The default value of the axis
    #[serde(rename = "defaultValue")]
    pub default_value: f64,
    /// The maximum value of the axis
    #[serde(rename = "maxValue")]
    pub max_value: f64,
    /// Custom data associated with the glyph axis
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

impl From<&GlyphAxis> for crate::Axis {
    fn from(val: &GlyphAxis) -> Self {
        crate::Axis {
            name: val.name.clone().into(),
            tag: crate::Tag::new(b"VARC"),
            min: Some(UserCoord::new(val.min_value)),
            default: Some(UserCoord::new(val.default_value)),
            max: Some(UserCoord::new(val.max_value)),
            map: None,
            hidden: true,
            values: vec![],
            format_specific: custom_data_to_format_specific(&val.custom_data),
        }
    }
}

/// A Fontra glyph source definition — corresponds to Python `GlyphSource` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct GlyphSource {
    /// The name of the glyph source
    pub name: String,
    /// The name of the layer
    #[serde(rename = "layerName")]
    pub layer_name: String,
    /// The location of the glyph source in design space coordinates
    #[serde(default)]
    pub location: HashMap<String, f64>,
    /// Optional base location name
    #[serde(
        rename = "locationBase",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub location_base: Option<String>,
    /// Whether the source is inactive
    #[serde(default)]
    pub inactive: bool,
    /// Custom data associated with the glyph source
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// Enum to represent either a packed or unpacked Fontra path.
#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum AnyPath {
    /// A packed path (with pointTypes and contourInfo)
    Packed(PackedPath),
    /// An unpacked path (stored as raw JSON)
    Other(Value),
}

impl Default for AnyPath {
    fn default() -> Self {
        AnyPath::Packed(PackedPath::default())
    }
}

/// A Fontra layer definition — corresponds to Python `Layer` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Layer {
    /// The static glyph associated with the layer
    pub glyph: StaticGlyph,
    /// Custom data associated with the layer
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

fn load_layer(layer: &Layer, glyph_source: Option<&&GlyphSource>) -> crate::Layer {
    let shapes = vec![];
    let source_name = glyph_source.map(|s| s.name.clone());
    let _source_location = glyph_source.map(|s| s.location.iter());

    // If the glyph sources has a "location base" this refers to another master by ID. The layer is default for that master.
    // Otherwise, they have an explicit location
    let layer_type = if let Some(master_id) = glyph_source.and_then(|s| s.location_base.as_deref())
    {
        LayerType::DefaultForMaster(master_id.to_string())
    } else {
        LayerType::FreeFloating
    };

    crate::Layer {
        width: layer.glyph.x_advance.unwrap_or_default() as f32,
        name: None,
        id: source_name,
        master: layer_type,
        guides: layer.glyph.guides.iter().map(|g| g.into()).collect(),
        shapes,
        anchors: layer.glyph.anchors.iter().map(|a| a.into()).collect(),
        color: None,
        layer_index: None,
        is_background: false,
        background_layer_id: None,
        location: None,
        smart_component_location: IndexMap::new(),
        format_specific: custom_data_to_format_specific(&layer.custom_data),
    }
}

/// A Fontra RGBAColor — corresponds to Python `RGBAColor` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct RGBAColor {
    /// Red component
    pub red: f64,
    /// Green component
    pub green: f64,
    /// Blue component
    pub blue: f64,
    /// Alpha component
    #[serde(default)]
    pub alpha: f64,
}

/// A Fontra background image — corresponds to Python `BackgroundImage` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct BackgroundImage {
    /// The identifier of the background image
    pub identifier: String,
    /// The transformation applied to the image
    #[serde(default)]
    pub transformation: DecomposedTransform,
    /// The opacity of the image
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    /// Optional color
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<RGBAColor>,
    /// Custom data associated with the background image
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

fn default_opacity() -> f64 {
    1.0
}

/// A Fontra static glyph definition — corresponds to Python `StaticGlyph` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct StaticGlyph {
    /// The glyph's path (either PackedPath or unpacked Path)
    #[serde(default)]
    pub path: AnyPath,
    /// The components of the glyph
    #[serde(default)]
    pub components: Vec<Component>,
    /// The horizontal advance of the glyph
    #[serde(rename = "xAdvance", default, skip_serializing_if = "Option::is_none")]
    pub x_advance: Option<f64>,
    /// The vertical advance of the glyph
    #[serde(rename = "yAdvance", default, skip_serializing_if = "Option::is_none")]
    pub y_advance: Option<f64>,
    /// The vertical origin of the glyph
    #[serde(
        rename = "verticalOrigin",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub vertical_origin: Option<f64>,
    /// The anchors of the glyph
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    /// The guides of the glyph
    #[serde(default)]
    pub guides: Vec<Guideline>,
    /// Optional background image
    #[serde(
        rename = "backgroundImage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub background_image: Option<BackgroundImage>,
    /// Custom data associated with the static glyph
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// A Fontra component definition — corresponds to Python `Component` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Component {
    /// The name of the referenced glyph
    pub name: String,
    /// The transformation applied to the component
    #[serde(default)]
    pub transformation: DecomposedTransform,
    /// The location of the component in design space coordinates
    #[serde(default)]
    pub location: HashMap<String, f64>,
    /// Custom data associated with the component
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// A Fontra anchor definition — corresponds to Python `Anchor` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Anchor {
    /// The name of the anchor
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The x coordinate of the anchor
    #[serde(default)]
    pub x: f64,
    /// The y coordinate of the anchor
    #[serde(default)]
    pub y: f64,
    /// Custom data associated with the anchor
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// A Fontra decomposed transform
#[derive(Serialize, Deserialize, Default, Debug)]
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
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct ContourInfo {
    /// The end point index of the contour
    #[serde(rename = "endPoint")]
    pub end_point: usize,
    /// Whether the contour is closed
    #[serde(rename = "isClosed")]
    pub is_closed: bool,
}

/// A Fontra path structure
#[derive(Serialize, Deserialize, Default, Debug)]
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

/// A Fontra variable glyph definition — corresponds to Python `VariableGlyph` class.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct VariableGlyph {
    /// The name of the glyph
    pub name: String,
    /// Any glyph-specific axes
    #[serde(default)]
    pub axes: Vec<GlyphAxis>,
    /// The sources for the glyph
    #[serde(default)]
    pub sources: Vec<GlyphSource>,
    /// The layers of the glyph
    #[serde(default)]
    pub layers: HashMap<String, Layer>,
    /// Custom data associated with the glyph
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

/// Font-level information in Fontra format
#[derive(Serialize, Deserialize, Default, Debug)]
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
    #[serde(rename = "customData", default)]
    pub custom_data: HashMap<String, Value>,
}

impl From<&FontInfo> for crate::Names {
    fn from(val: &FontInfo) -> Self {
        crate::Names {
            copyright: val.copyright.as_ref().into(),
            family_name: val.family_name.as_ref().into(),
            trademark: val.trademark.as_ref().into(),
            manufacturer: val.manufacturer.as_ref().into(),
            designer: val.designer.as_ref().into(),
            description: val.description.as_ref().into(),
            manufacturer_url: val.manufacturer_url.as_ref().into(),
            designer_url: val.designer_url.as_ref().into(),
            license: val.license_description.as_ref().into(),
            license_url: val.license_info_url.as_ref().into(),
            ..Default::default()
        }
    }
}
