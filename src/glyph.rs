use crate::{
    common::{Direction, FormatSpecific},
    layer::Layer,
    serde_helpers::{default_true, is_true},
    Axis,
};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::ops::{Deref, DerefMut};
use typeshare::typeshare;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// A list of glyphs in the font
pub struct GlyphList(pub Vec<Glyph>);
impl GlyphList {
    /// Get a glyph by name
    pub fn get(&self, g: &str) -> Option<&Glyph> {
        self.0.iter().find(|&glyph| glyph.name == g)
    }
    /// Get a glyph by name, mutably
    pub fn get_mut(&mut self, g: &str) -> Option<&mut Glyph> {
        self.0.iter_mut().find(|glyph| glyph.name == g)
    }

    /// Get a glyph by index
    pub fn get_by_index(&self, id: usize) -> Option<&Glyph> {
        self.0.get(id)
    }
    /// Get a glyph by index, mutably
    pub fn get_by_index_mut(&mut self, id: usize) -> Option<&mut Glyph> {
        self.0.get_mut(id)
    }
    /// Get an iterator over the glyphs
    pub fn iter(&self) -> std::slice::Iter<'_, Glyph> {
        self.0.iter()
    }
}

impl Deref for GlyphList {
    type Target = Vec<Glyph>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for GlyphList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(untagged)]
#[typeshare]
/// The category of a glyph
pub enum GlyphCategory {
    /// A base glyph
    Base,
    /// A mark glyph
    Mark,
    /// An unknown / un-set category
    #[default]
    Unknown,
    /// A ligature glyph
    Ligature,
    /// Custom
    Custom(String),
}

impl From<&GlyphCategory> for Option<String> {
    fn from(val: &GlyphCategory) -> Self {
        match val {
            GlyphCategory::Base => Some("Base".to_string()),
            GlyphCategory::Mark => Some("Mark".to_string()),
            GlyphCategory::Ligature => Some("Ligature".to_string()),
            GlyphCategory::Custom(s) => Some(s.to_string()),
            GlyphCategory::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[typeshare]
/// A glyph in the font
pub struct Glyph {
    /// The name of the glyph
    #[typeshare(serialized_as = "String")]
    pub name: SmolStr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[typeshare(serialized_as = "Option<String>")]
    /// The production name of the glyph, if any
    pub production_name: Option<SmolStr>,
    /// The category of the glyph
    pub category: GlyphCategory,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Unicode codepoints assigned to the glyph
    pub codepoints: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// The layers in the glyph
    ///
    /// These include background layers, design-only layers, etc. as well as
    /// the main master and location-specific layers.
    pub layers: Vec<Layer>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    /// Whether the glyph is exported
    pub exported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The writing direction of the glyph, if any
    pub direction: Option<Direction>,

    /// Glyph-specific axes for "smart components" / variable components
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub component_axes: Vec<Axis>,

    /// Format-specific data
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: FormatSpecific,
}

impl Glyph {
    /// Create a new Glyph with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Get a layer by id
    pub fn get_layer(&self, id: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id.as_deref() == Some(id))
    }
    /// Get a mutable layer by id
    pub fn get_layer_mut(&mut self, id: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id.as_deref() == Some(id))
    }

    /// Check if this glyph is a smart component
    pub fn is_smart_component(&self) -> bool {
        !self.component_axes.is_empty() && self.layers.iter().any(|l| l.is_smart_component())
    }
}

#[cfg(feature = "glyphs")]
pub(crate) mod glyphs {
    use std::str::FromStr;

    use crate::{
        convertors::glyphs3::{copy_user_data, UserData, KEY_USER_DATA},
        layer::glyphs::{layer_from_glyphs, layer_to_glyphs},
        BabelfontError,
    };

    use super::*;
    use fontdrasil::{
        coords::{DesignCoord, UserCoord},
        types::Tag,
    };
    use glyphslib::glyphs3::Glyph as G3Glyph;

    pub(crate) fn from_glyphs(val: &G3Glyph, axes_order: &[Tag]) -> Result<Glyph, BabelfontError> {
        let mut format_specific = FormatSpecific::default();
        format_specific.insert_json_non_null("case", &val.case);
        format_specific.insert_json_non_null("color", &val.color);
        format_specific.insert_some_json("kern_bottom", &val.kern_bottom);
        format_specific.insert_some_json("kern_left", &val.kern_left);
        format_specific.insert_some_json("kern_right", &val.kern_right);
        format_specific.insert_some_json("kern_top", &val.kern_top);
        format_specific.insert_json_non_null("last_change", &val.last_change);
        format_specific.insert_json("locked", &val.locked);
        format_specific.insert_json_non_null("metric_bottom", &val.metric_bottom);
        format_specific.insert_json_non_null("metric_left", &val.metric_left);
        format_specific.insert_json_non_null("metric_right", &val.metric_right);
        format_specific.insert_json_non_null("metric_top", &val.metric_top);
        format_specific.insert_json_non_null("metric_vert_width", &val.metric_vert_width);
        format_specific.insert_json_non_null("metric_width", &val.metric_width);
        format_specific.insert_json_non_null("note", &val.note);
        format_specific.insert_json_non_null("script", &val.script);
        format_specific.insert_json_non_null("sort_name", &val.sort_name);
        format_specific.insert_json_non_null("sort_name_keep", &val.sort_name_keep);
        format_specific.insert_json_non_null("subcategory", &val.subcategory);
        format_specific.insert_nonempty_json("tags", &val.tags);
        let category = if let Some(cat) = &val.category {
            match cat.as_str() {
                "Base" => GlyphCategory::Base,
                "Mark" => {
                    if val.subcategory == Some("Nonspacing".to_string()) {
                        GlyphCategory::Mark
                    } else {
                        GlyphCategory::Base
                    }
                }
                "Ligature" => GlyphCategory::Ligature,
                o => GlyphCategory::Custom(o.to_string()),
            }
        } else {
            GlyphCategory::Unknown
        };
        copy_user_data(&mut format_specific, &val.user_data);
        let mut layers = vec![];
        let component_axes: Vec<Axis> = val
            .smart_component_settings
            .iter()
            .map(|x| x.into())
            .collect();
        // Now pre-chew them for easy layer generation
        let sc_axes = glyph_specific_axes(&component_axes);
        for layer in &val.layers {
            let mut bf_layer = layer_from_glyphs(layer, axes_order, &sc_axes)?;
            if let Some(bg_layer) = &layer.background {
                let mut background = layer_from_glyphs(bg_layer.deref(), axes_order, &sc_axes)?;
                background.is_background = true;
                if background.id.is_none() || background.id == Some("".to_string()) {
                    background.id =
                        Some(format!("{}.bg", bf_layer.id.as_deref().unwrap_or("layer")));
                }
                bf_layer.background_layer_id = background.id.clone();
                layers.push(bf_layer);
                layers.push(background);
            } else {
                layers.push(bf_layer);
            }
        }
        Ok(Glyph {
            name: SmolStr::from(&val.name),
            production_name: val.production.as_ref().map(SmolStr::from),
            category,
            codepoints: val.unicode.clone(),
            layers,
            exported: val.export,
            direction: val.direction.as_ref().and_then(|d| {
                if d.is_empty() {
                    None
                } else {
                    Some(Direction::from_str(d).unwrap_or(Direction::LeftToRight))
                }
            }),
            component_axes,
            format_specific,
        })
    }

    fn glyph_specific_axes(component_axes: &[Axis]) -> Vec<(String, DesignCoord, DesignCoord)> {
        component_axes
            .iter()
            .map(|axis| {
                (
                    axis.name
                        .get_default()
                        .unwrap_or(&"Unnamed Axis".to_string())
                        .to_string(),
                    DesignCoord::new(axis.min.unwrap_or(UserCoord::new(0.0)).to_f64()), // There is no mapping
                    DesignCoord::new(axis.max.unwrap_or(UserCoord::new(0.0)).to_f64()),
                )
            })
            .collect()
    }

    pub(crate) fn glyph_to_glyphs(
        val: &Glyph,
        axis_order: &[Tag],
        kern_left: Option<&SmolStr>,
        kern_right: Option<&SmolStr>,
    ) -> glyphslib::glyphs3::Glyph {
        let mut g3_layers = vec![];
        let sc_axes = glyph_specific_axes(&val.component_axes);
        for layer in &val.layers {
            if layer.is_background {
                continue;
            }
            let mut g3_layer = layer_to_glyphs(layer, axis_order, &sc_axes);
            if let Some(bg_id) = &layer.background_layer_id {
                if let Some(bg_layer) = val.get_layer(bg_id) {
                    g3_layer.background =
                        Some(Box::new(layer_to_glyphs(bg_layer, axis_order, &sc_axes)));
                }
            }
            g3_layers.push(g3_layer);
        }

        G3Glyph {
            name: val.name.to_string(),
            production: val.production_name.as_ref().map(|p| p.to_string()),
            unicode: val.codepoints.clone(),
            layers: g3_layers,
            export: val.exported,
            case: val.format_specific.get_string("case"),
            category: (&val.category).into(),
            direction: val.direction.as_ref().map(|d| match d {
                Direction::LeftToRight => "LTR".to_string(),
                Direction::RightToLeft => "RTL".to_string(),
                Direction::TopToBottom => "VTR".to_string(),
                Direction::Bidi => "BIDI".to_string(),
            }),
            kern_bottom: val.format_specific.get_optionstring("kern_bottom"),
            kern_left: kern_left.as_ref().map(|s| s.to_string()),
            kern_right: kern_right.as_ref().map(|s| s.to_string()),
            kern_top: val.format_specific.get_optionstring("kern_top"),
            last_change: val.format_specific.get_optionstring("last_change"),
            locked: val.format_specific.get_bool("locked"),
            metric_bottom: val.format_specific.get_optionstring("metric_bottom"),
            metric_left: val.format_specific.get_optionstring("metric_left"),
            metric_right: val.format_specific.get_optionstring("metric_right"),
            metric_top: val.format_specific.get_optionstring("metric_top"),
            metric_vert_width: val.format_specific.get_optionstring("metric_vert_width"),
            metric_width: val.format_specific.get_optionstring("metric_width"),
            note: val.format_specific.get_string("note"),
            smart_component_settings: val.component_axes.iter().map(|x| x.into()).collect(),
            script: val.format_specific.get_optionstring("script"),
            sort_name: val.format_specific.get_optionstring("sort_name"),
            sort_name_keep: val.format_specific.get_optionstring("sort_name_keep"),
            subcategory: val.format_specific.get_optionstring("subcategory"),
            tags: val
                .format_specific
                .get_parse_or::<Vec<String>>("tags", Vec::new()),
            user_data: val
                .format_specific
                .get(KEY_USER_DATA)
                .and_then(|x| serde_json::from_value::<UserData>(x.clone()).ok())
                .unwrap_or_default(),
            color: val.format_specific.get("color").and_then(|x|
                    // either a tuple -> ColorTuple or an int -> ColorInt
                    if x.is_number() {
                        Some(glyphslib::common::Color::ColorInt(x.as_i64().unwrap_or(0) as u8))
                    } else if x.is_array() {
                        Some(glyphslib::common::Color::ColorTuple(
                            x.as_array()
                                .unwrap_or(&vec![])
                                .iter()
                                .filter_map(|v| v.as_u64())
                                .map(|v| v as u8)
                                .collect(),
                        ))
                    } else { None }
                ),
        }
    }

    impl From<&glyphslib::glyphs3::SmartComponentSetting> for Axis {
        fn from(val: &glyphslib::glyphs3::SmartComponentSetting) -> Self {
            Axis {
                name: crate::I18NDictionary::from(&val.name),
                tag: Tag::new(b"VARC"), // placeholder
                default: Some(UserCoord::new(val.bottom_value as f64)),
                min: Some(UserCoord::new(val.bottom_value as f64)),
                max: Some(UserCoord::new(val.top_value as f64)),
                ..Default::default()
            }
        }
    }

    impl From<&Axis> for glyphslib::glyphs3::SmartComponentSetting {
        fn from(val: &Axis) -> Self {
            glyphslib::glyphs3::SmartComponentSetting {
                bottom_value: val.min.unwrap_or(UserCoord::new(0.0)).to_f64() as i32,
                top_value: val.max.unwrap_or(UserCoord::new(0.0)).to_f64() as i32,
                name: val
                    .name
                    .get_default()
                    .unwrap_or(&"Unnamed Axis".to_string())
                    .to_string(),
            }
        }
    }
}
