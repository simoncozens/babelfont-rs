use crate::{
    common::{FormatSpecific, OTValue},
    filters::{DropSparseMasters, FontFilter as _},
    glyph::{self, glyphs::glyph_to_glyphs},
    i18ndictionary::I18NDictionary,
    names::Names,
    Axis, BabelfontError, Font, GlyphList, Master,
};
use chrono::Local;
use fontdrasil::coords::{DesignCoord, DesignLocation, UserCoord};
use glyphslib::glyphs3::{self, Property};
use indexmap::IndexMap;
use serde_json::json;
use smol_str::SmolStr;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    str::FromStr,
};
use write_fonts::types::Tag;

pub(crate) type UserData = BTreeMap<SmolStr, glyphslib::Plist>;

pub(crate) const KEY_ALIGNMENT: &str = "com.schriftgestalt.Glyphs.alignment";
pub(crate) const KEY_ANCHOR_LOCKED: &str = "com.schriftgestalt.Glyphs.anchorLocked";
pub(crate) const KEY_ANCHOR_ORIENTATION: &str = "com.schriftgestalt.Glyphs.anchorOrientation";
pub(crate) const KEY_ANNOTATIONS: &str = "com.schriftgestalt.Glyphs.annotations";
pub(crate) const KEY_APP_VERSION: &str = "com.schriftgestalt.Glyphs.appVersion";
pub(crate) const KEY_ATTR: &str = "com.schriftgestalt.Glyphs.attr";
pub(crate) const KEY_COMPONENT_ANCHOR: &str = "com.schriftgestalt.Glyphs.componentAnchor";
pub(crate) const KEY_COMPONENT_LOCKED: &str = "com.schriftgestalt.Glyphs.componentLocked";
pub(crate) const KEY_CUSTOM_PARAMETERS: &str = "com.schriftgestalt.Glyphs.customParameters.";
pub(crate) const KEY_DISPLAY_STRINGS: &str = "com.schriftgestalt.Glyphs.displayStrings";
pub(crate) const KEY_ICON_NAME: &str = "com.schriftgestalt.Glyphs.iconName";
pub(crate) const KEY_INSTANCE_EXPORTS: &str = "com.schriftgestalt.Glyphs.exports";
pub(crate) const KEY_IS_BOLD: &str = "com.schriftgestalt.Glyphs.isBold";
pub(crate) const KEY_IS_ITALIC: &str = "com.schriftgestalt.Glyphs.isItalic";
pub(crate) const KEY_KEEP_ALTERNATES_TOGETHER: &str =
    "com.schriftgestalt.Glyphs.keepAlternatesTogether";
pub(crate) const KEY_KERNING_RTL: &str = "com.schriftgestalt.Glyphs.kerningRTL";
pub(crate) const KEY_KERNING_VERTICAL: &str = "com.schriftgestalt.Glyphs.kerningVertical";
pub(crate) const KEY_LAYER_HINTS: &str = "com.schriftgestalt.Glyphs.layerHints";
pub(crate) const KEY_LAYER_IMAGE: &str = "com.schriftgestalt.Glyphs.layerBackgroundImage";
pub(crate) const KEY_MASTER_VISIBLE: &str = "com.schriftgestalt.Glyphs.visible";
pub(crate) const KEY_METRIC_BOTTOM: &str = "com.schriftgestalt.Glyphs.metricBottom";
pub(crate) const KEY_METRIC_LEFT: &str = "com.schriftgestalt.Glyphs.metricLeft";
pub(crate) const KEY_METRIC_RIGHT: &str = "com.schriftgestalt.Glyphs.metricRight";
pub(crate) const KEY_METRIC_TOP: &str = "com.schriftgestalt.Glyphs.metricTop";
pub(crate) const KEY_METRIC_VERT_WIDTH: &str = "com.schriftgestalt.Glyphs.metricVertWidth";
pub(crate) const KEY_METRIC_WIDTH: &str = "com.schriftgestalt.Glyphs.metricWidth";
pub(crate) const KEY_NUMBER_NAMES: &str = "com.schriftgestalt.Glyphs.numberNames";
pub(crate) const KEY_NUMBER_VALUES: &str = "com.schriftgestalt.Glyphs.numberValues";
pub(crate) const KEY_SETTINGS: &str = "com.schriftgestalt.Glyphs.settings";
pub(crate) const KEY_STEM_VALUES: &str = "com.schriftgestalt.Glyphs.stemValues";
pub(crate) const KEY_STEMS: &str = "com.schriftgestalt.Glyphs.stems";
pub(crate) const KEY_USER_DATA: &str = "com.schriftgestalt.Glyphs.userData";
pub(crate) const KEY_VERT_WIDTH: &str = "com.schriftgestalt.Glyphs.vertWidth";
pub(crate) const KEY_VERT_ORIGIN: &str = "com.schriftgestalt.Glyphs.vertOrigin";
pub(crate) const KEY_WEIGHT_CLASS: &str = "com.schriftgestalt.Glyphs.weightClass";
pub(crate) const KEY_WIDTH_CLASS: &str = "com.schriftgestalt.Glyphs.widthClass";

fn copy_custom_parameters(
    format_specific: &mut FormatSpecific,
    custom_parameters: &[glyphslib::common::CustomParameter],
) {
    for cp in custom_parameters.iter() {
        if let Ok(value) = serde_json::to_value(&cp.value) {
            format_specific.insert(
                format!("{}{}", KEY_CUSTOM_PARAMETERS, cp.name.clone()),
                json!({
                            "value": value,
                            "disabled": cp.disabled,
                }),
            );
        }
    }
}

fn get_cp<'a>(format_specific: &'a FormatSpecific, name: &str) -> Option<&'a serde_json::Value> {
    format_specific.get(&format!("{}{}", KEY_CUSTOM_PARAMETERS, name))
}

fn serialize_custom_parameters(
    format_specific: &FormatSpecific,
) -> Vec<glyphslib::common::CustomParameter> {
    format_specific
        .iter()
        .filter_map(|(key, value)| {
            if key.starts_with(KEY_CUSTOM_PARAMETERS) {
                let name = key.trim_start_matches(KEY_CUSTOM_PARAMETERS).to_string();
                Some(glyphslib::common::CustomParameter {
                    name,
                    value: value
                        .as_object()
                        .and_then(|d| d.get("value").cloned())
                        .and_then(|v| serde_json::from_value(v).ok())
                        .unwrap_or_default(),
                    disabled: value
                        .as_object()
                        .and_then(|d| d.get("disabled"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                })
            } else {
                None
            }
        })
        .collect()
}
pub(crate) fn copy_user_data(
    format_specific: &mut FormatSpecific,
    user_data: &BTreeMap<SmolStr, glyphslib::Plist>,
) {
    if !user_data.is_empty() {
        if let Ok(value) = serde_json::to_value(user_data) {
            format_specific.insert(KEY_USER_DATA.into(), value);
        }
    }
}

/// Load a Glyphs font from a file path
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    if path.extension().and_then(|x| x.to_str()) == Some("glyphspackage") {
        return _load(
            &glyphslib::Font::load(&path).map_err(|x| BabelfontError::PlistParse(x.to_string()))?,
            path,
        );
    }
    let s = fs::read_to_string(&path)?;
    load_str(&s, path.clone())
}

/// Load a Glyphs font from a string
pub fn load_str(s: &str, path: PathBuf) -> Result<Font, BabelfontError> {
    let glyphs_font =
        glyphslib::Font::load_str(s).map_err(|x| BabelfontError::PlistParse(x.to_string()))?;
    _load(&glyphs_font, path)
}

fn _load(glyphs_font: &glyphslib::Font, path: PathBuf) -> Result<Font, BabelfontError> {
    let mut font = Font::new();
    let glyphs_font = glyphs_font
        .as_glyphs3()
        .ok_or(BabelfontError::WrongConvertor { path })?;
    // App version
    font.format_specific.insert(
        KEY_APP_VERSION.into(),
        serde_json::Value::String(glyphs_font.app_version.clone()),
    );
    // Display strings
    font.format_specific.insert(
        KEY_DISPLAY_STRINGS.into(),
        serde_json::to_value(&glyphs_font.display_strings).unwrap_or(serde_json::Value::Null),
    );
    // Copy axes
    font.axes = glyphs_font
        .axes
        .iter()
        .map(|axis| Axis {
            tag: Tag::from_str(&axis.tag).unwrap_or_else(|_| Tag::new(b"????")),
            name: axis.name.clone().into(),
            hidden: axis.hidden,
            ..Default::default()
        })
        .collect();
    let axes_order = font.axes.iter().map(|a| a.tag).collect::<Vec<Tag>>();
    // Classes
    font.features.classes = glyphs_font
        .classes
        .iter()
        .map(|c| (SmolStr::new(&c.name), c.into()))
        .collect();
    // Custom parameters
    copy_custom_parameters(&mut font.format_specific, &glyphs_font.custom_parameters);
    // Date
    font.date = glyphs_font.date.parse().unwrap_or_else(|_| Local::now());
    // Family name
    font.names.family_name = glyphs_font.family_name.clone().into();
    // Feature prefixes
    for prefix in glyphs_font.feature_prefixes.iter() {
        font.features
            .prefixes
            .insert(SmolStr::new(&prefix.name), prefix.into());
    }
    // Features
    for feature in glyphs_font.features.iter() {
        font.features
            .features
            .push((SmolStr::new(&feature.tag), feature.into()));
    }
    // Masters
    font.masters = glyphs_font
        .masters
        .iter()
        .map(|master| load_master(master, glyphs_font, &font))
        .collect();
    // Glyphs
    font.glyphs = GlyphList(
        glyphs_font
            .glyphs
            .iter()
            .map(|g| glyph::glyphs::from_glyphs(g, &axes_order))
            .collect(),
    );
    // Instances
    font.instances = glyphs_font
        .instances
        .iter()
        .map(|i| load_instance(&font, i))
        .collect();
    // Keep alternates together
    if glyphs_font.keep_alternates_together {
        font.format_specific.insert(
            KEY_KEEP_ALTERNATES_TOGETHER.into(),
            serde_json::Value::Bool(true),
        );
    }
    // Handle kerning when we do masters
    // Metrics
    // Note
    font.note = Some(glyphs_font.note.clone());
    // Numbers
    font.format_specific.insert(
        KEY_NUMBER_NAMES.into(),
        glyphs_font.numbers.iter().map(|n| n.name.clone()).collect(),
    );
    // Properties
    load_properties(
        &mut font.names,
        &mut font.custom_ot_values,
        &glyphs_font.properties,
    );
    // Settings
    font.format_specific.insert(
        KEY_SETTINGS.into(),
        serde_json::to_value(&glyphs_font.settings).unwrap_or(serde_json::Value::Null),
    );

    // Stems
    if !glyphs_font.stems.is_empty() {
        font.format_specific.insert(
            KEY_STEMS.into(),
            serde_json::to_value(&glyphs_font.stems).unwrap_or(serde_json::Value::Null),
        );
    }
    // UPM
    font.upm = glyphs_font.units_per_em as u16;

    // User data
    copy_user_data(&mut font.format_specific, &glyphs_font.user_data);
    // Version
    font.version = (
        glyphs_font.version.major as u16,
        glyphs_font.version.minor as u16,
    );

    // RTL and vertical kerning
    font.format_specific
        .insert_json_non_null(KEY_KERNING_VERTICAL, &glyphs_font.kerning_vertical);
    font.format_specific
        .insert_json_non_null(KEY_KERNING_RTL, &glyphs_font.kerning_rtl);

    // Copy masters
    // Copy instances
    // Copy kern groups
    for glyph in font.glyphs.iter() {
        let left_group = glyph.format_specific.get_string("kern_left");
        if !left_group.is_empty() {
            font.first_kern_groups
                .entry(left_group.into())
                .or_default()
                .push(glyph.name.clone());
        }

        let right_group = glyph.format_specific.get_string("kern_right");
        if !right_group.is_empty() {
            font.second_kern_groups
                .entry(right_group.into())
                .or_default()
                .push(glyph.name.clone());
        }
    }

    // Interpret metrics
    // Interpret axes
    interpret_axes(&mut font);

    // Bake in Glyphs data ??? When is best to do this?
    // GlyphsData.apply(&mut font)?;

    Ok(font)
}

fn load_instance(font: &Font, instance: &glyphs3::Instance) -> crate::Instance {
    let designspace_to_location = |numbers: &[f32]| -> DesignLocation {
        numbers
            .iter()
            .zip(font.axes.iter())
            .map(|(number, axis)| (axis.tag, DesignCoord::new(*number as f64)))
            .collect()
    };
    let mut format_specific = FormatSpecific::default();
    copy_custom_parameters(&mut format_specific, &instance.custom_parameters);
    copy_user_data(&mut format_specific, &instance.user_data);
    format_specific.insert_some_json(
        KEY_WEIGHT_CLASS,
        &instance.weight_class.as_ref().and_then(|x| x.as_i64()),
    );
    format_specific.insert_some_json(
        KEY_WIDTH_CLASS,
        &instance.width_class.as_ref().and_then(|x| x.as_i64()),
    );
    format_specific.insert_if_ne_json(KEY_INSTANCE_EXPORTS, &instance.exports, &true);
    format_specific.insert_if_ne_json(KEY_IS_BOLD, &instance.is_bold, &false);
    format_specific.insert_if_ne_json(KEY_IS_ITALIC, &instance.is_italic, &false);
    let mut names = Names::new();
    let mut custom_ot_values = vec![];
    load_properties(
        &mut names,
        &mut custom_ot_values, // XXX
        &instance.properties,
    );
    crate::Instance {
        id: instance.name.clone(),
        name: I18NDictionary::from(&instance.name),
        location: designspace_to_location(&instance.axes_values),
        custom_names: names,
        variable: instance.export_type == glyphslib::glyphs3::ExportType::Variable,
        linked_style: instance.link_style.clone(),
        format_specific,
    }
}

fn save_instance(instance: &crate::Instance, axes: &[Axis]) -> glyphs3::Instance {
    let mut axes_values = vec![];
    if !instance.variable {
        for axis in axes {
            axes_values.push(
                instance
                    .location
                    .get(axis.tag)
                    .map(|x| x.to_f64())
                    .map(|x| x as f32)
                    .unwrap_or(0.0),
            );
        }
    }
    let format_specific = &instance.format_specific;
    glyphs3::Instance {
        name: instance
            .name
            .get_default()
            .map(|x| x.to_string())
            .unwrap_or_default(),
        axes_values,
        weight_class: format_specific
            .get(KEY_WEIGHT_CLASS)
            .and_then(|x| x.as_i64())
            .map(glyphslib::Plist::Integer),
        width_class: format_specific
            .get(KEY_WIDTH_CLASS)
            .and_then(|x| x.as_i64())
            .map(glyphslib::Plist::Integer),
        exports: format_specific
            .get(KEY_INSTANCE_EXPORTS)
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        custom_parameters: serialize_custom_parameters(&instance.format_specific),
        user_data: format_specific
            .get(KEY_USER_DATA)
            .and_then(|x| serde_json::from_value::<UserData>(x.clone()).ok())
            .unwrap_or_default(),
        link_style: instance.linked_style.clone(),
        instance_interpolations: Default::default(),
        is_bold: instance.format_specific.get_bool_or(KEY_IS_BOLD, false),
        is_italic: instance.format_specific.get_bool_or(KEY_IS_ITALIC, false),
        manual_interpolation: Default::default(),
        properties: save_properties(&instance.custom_names),
        export_type: if instance.variable {
            glyphslib::glyphs3::ExportType::Variable
        } else {
            glyphslib::glyphs3::ExportType::Static
        },
    }
}

fn load_properties(
    names: &mut Names,
    custom_ot_values: &mut Vec<OTValue>,
    glyphs_properties: &[Property],
) {
    for property in glyphs_properties.iter() {
        match property {
            glyphs3::Property::SingularProperty { key, value } => match key {
                glyphs3::SingularPropertyKey::Designer => {
                    names.designer = I18NDictionary::from(value)
                }
                glyphs3::SingularPropertyKey::Manufacturer => {
                    names.manufacturer = I18NDictionary::from(value)
                }
                glyphs3::SingularPropertyKey::DesignerUrl => {
                    names.designer_url = I18NDictionary::from(value)
                }
                glyphs3::SingularPropertyKey::ManufacturerUrl => {
                    names.manufacturer_url = I18NDictionary::from(value);
                }
                glyphs3::SingularPropertyKey::LicenseUrl => {
                    names.license_url = I18NDictionary::from(value)
                }
                glyphs3::SingularPropertyKey::PostscriptFullName => {
                    names.postscript_name = I18NDictionary::from(value)
                }
                glyphs3::SingularPropertyKey::PostscriptFontName => {
                    names.postscript_cid_name = I18NDictionary::from(value)
                }
                glyphs3::SingularPropertyKey::WwsFamilyName => {
                    names.wws_family_name = I18NDictionary::from(value)
                }
                glyphs3::SingularPropertyKey::VersionString => {
                    names.version = I18NDictionary::from(value)
                }
                glyphs3::SingularPropertyKey::VendorID => custom_ot_values.push(OTValue {
                    table: "OS/2".into(),
                    field: "achVendID".into(),
                    value: crate::OTScalar::StringType(value.clone()),
                }),
                glyphs3::SingularPropertyKey::UniqueID => {
                    names.unique_id = I18NDictionary::from(value)
                }
            },
            glyphs3::Property::LocalizedProperty { key, values } => {
                let mut value = I18NDictionary::new();
                for localized_value in values.iter() {
                    value.insert(
                        localized_value.language.clone(),
                        localized_value.value.clone(),
                    );
                }
                match key {
                    glyphs3::LocalizedPropertyKey::FamilyNames => names.family_name = value,
                    glyphs3::LocalizedPropertyKey::Copyrights => names.copyright = value,
                    glyphs3::LocalizedPropertyKey::Designers => names.designer = value,
                    glyphs3::LocalizedPropertyKey::Manufacturers => names.manufacturer = value,
                    glyphs3::LocalizedPropertyKey::Licenses => names.license = value,
                    glyphs3::LocalizedPropertyKey::Trademarks => names.trademark = value,
                    glyphs3::LocalizedPropertyKey::Descriptions => names.description = value,
                    glyphs3::LocalizedPropertyKey::SampleTexts => names.sample_text = value,
                    glyphs3::LocalizedPropertyKey::CompatibleFullNames => {
                        names.compatible_full_name = value
                    }
                    glyphs3::LocalizedPropertyKey::StyleNames => {
                        names.typographic_subfamily = value;
                    }
                }
            }
            glyphs3::Property::Junk(_plist) => unreachable!(),
        }
    }
}

fn save_properties(names: &Names) -> Vec<glyphs3::Property> {
    let mut properties: Vec<glyphs3::Property> = vec![];

    // Macro for singular-only properties (no localized variant)
    macro_rules! push_singular {
        ($field:expr, $key:expr) => {
            if let Some(value) = $field.get_default() {
                properties.push(glyphs3::Property::SingularProperty {
                    key: $key,
                    value: value.clone(),
                });
            }
        };
    }

    // Macro for properties that can be singular or localized
    macro_rules! push_property {
        ($field:expr, $singular_key:expr, $localized_key:expr) => {
            if !$field.is_empty() {
                if $field.is_single() {
                    if let Some(value) = $field.get_default() {
                        properties.push(glyphs3::Property::SingularProperty {
                            key: $singular_key,
                            value: value.clone(),
                        });
                    }
                } else {
                    let values: Vec<glyphslib::glyphs3::LocalizedValue> = $field
                        .0
                        .iter()
                        .map(|(language, value)| glyphslib::glyphs3::LocalizedValue {
                            language: language.clone(),
                            value: value.clone(),
                        })
                        .collect();
                    properties.push(glyphs3::Property::LocalizedProperty {
                        key: $localized_key,
                        values,
                    });
                }
            }
        };
    }

    // Macro for localized-only properties (no singular variant)
    macro_rules! push_localized {
        ($field:expr, $key:expr) => {
            if !$field.is_empty() {
                let values: Vec<glyphslib::glyphs3::LocalizedValue> = $field
                    .0
                    .iter()
                    .map(|(language, value)| glyphslib::glyphs3::LocalizedValue {
                        language: language.clone(),
                        value: value.clone(),
                    })
                    .collect();
                properties.push(glyphs3::Property::LocalizedProperty { key: $key, values });
            }
        };
    }

    // Singular-only properties
    push_singular!(
        names.designer_url,
        glyphs3::SingularPropertyKey::DesignerUrl
    );
    push_singular!(
        names.manufacturer_url,
        glyphs3::SingularPropertyKey::ManufacturerUrl
    );
    push_singular!(names.license_url, glyphs3::SingularPropertyKey::LicenseUrl);
    push_singular!(
        names.postscript_name,
        glyphs3::SingularPropertyKey::PostscriptFullName
    );
    push_singular!(
        names.postscript_cid_name,
        glyphs3::SingularPropertyKey::PostscriptFontName
    );
    push_singular!(
        names.wws_family_name,
        glyphs3::SingularPropertyKey::WwsFamilyName
    );

    push_localized!(
        names.compatible_full_name,
        glyphs3::LocalizedPropertyKey::CompatibleFullNames
    );

    push_localized!(names.copyright, glyphs3::LocalizedPropertyKey::Copyrights);

    // Properties that can be singular or localized
    push_property!(
        names.designer,
        glyphs3::SingularPropertyKey::Designer,
        glyphs3::LocalizedPropertyKey::Designers
    );

    // Localized-only properties
    // Only do family name if there's more than one language
    if !names.family_name.is_single() {
        push_localized!(
            names.family_name,
            glyphs3::LocalizedPropertyKey::FamilyNames
        );
    }

    push_localized!(names.license, glyphs3::LocalizedPropertyKey::Licenses);

    push_property!(
        names.manufacturer,
        glyphs3::SingularPropertyKey::Manufacturer,
        glyphs3::LocalizedPropertyKey::Manufacturers
    );

    push_localized!(names.trademark, glyphs3::LocalizedPropertyKey::Trademarks);
    push_localized!(
        names.description,
        glyphs3::LocalizedPropertyKey::Descriptions
    );
    push_localized!(
        names.sample_text,
        glyphs3::LocalizedPropertyKey::SampleTexts
    );
    push_localized!(
        names.typographic_subfamily,
        glyphs3::LocalizedPropertyKey::StyleNames
    );
    push_singular!(names.unique_id, glyphs3::SingularPropertyKey::UniqueID);
    push_singular!(names.version, glyphs3::SingularPropertyKey::VersionString);

    properties
}

fn load_master(master: &glyphs3::Master, glyphs_font: &glyphs3::Glyphs3, font: &Font) -> Master {
    let designspace_to_location = |numbers: &[f32]| -> DesignLocation {
        numbers
            .iter()
            .zip(font.axes.iter())
            .map(|(number, axis)| (axis.tag, DesignCoord::new(*number as f64)))
            .collect()
    };
    let mut m = Master::new(
        master.name.clone(),
        master.id.clone(),
        designspace_to_location(&master.axes_values),
    );
    m.guides = master.guides.iter().map(Into::into).collect();
    for (i, metric_value) in master.metric_values.iter().enumerate() {
        let metric_name = if i < glyphs_font.metrics.len() {
            let glyphs_metric = &glyphs_font.metrics[i];
            if let Some(known_type) = glyphs_font.metrics[i].metric_type {
                let typ = crate::MetricType::from(&known_type);
                if let Some(filter) = &glyphs_metric.filter {
                    crate::MetricType::Custom(format!("{} (filter {})", typ.as_str(), filter))
                } else {
                    typ
                }
            } else {
                crate::MetricType::Custom(glyphs_font.metrics[i].name.clone())
            }
        } else {
            crate::MetricType::Custom(format!("Metric {}", i))
        };
        m.metrics
            .insert(metric_name.clone(), metric_value.pos as i32);
        let overshoot_metric_name =
            crate::MetricType::Custom(format!("{} overshoot", metric_name.as_str()));
        m.metrics
            .insert(overshoot_metric_name, metric_value.over as i32);
    }
    copy_custom_parameters(&mut m.format_specific, &master.custom_parameters);
    m.kerning = glyphs_font
        .kerning
        .get(&m.id)
        .map(|kerndict| {
            let mut kerns = IndexMap::new();
            for (first, items) in kerndict {
                // Replace "@MMK_L_"/"@MMK_R_" prefix in group names with "@"
                let first = if let Some(stripped) = first.strip_prefix("@MMK_L_") {
                    format!("@{}", stripped)
                } else {
                    first.clone()
                };
                for (second, kern) in items {
                    let second = if let Some(stripped) = second.strip_prefix("@MMK_R_") {
                        format!("@{}", stripped)
                    } else {
                        second.clone()
                    };

                    kerns.insert(
                        (SmolStr::from(&first), SmolStr::from(&second)),
                        *kern as i16,
                    );
                }
            }
            kerns
        })
        .unwrap_or_default();
    m.format_specific.insert(
        KEY_STEM_VALUES.into(),
        serde_json::to_value(&master.stem_values).unwrap_or(serde_json::Value::Null),
    );
    m.format_specific.insert(
        KEY_NUMBER_VALUES.into(),
        serde_json::to_value(&master.number_values).unwrap_or(serde_json::Value::Null),
    );
    m.format_specific.insert(
        KEY_ICON_NAME.into(),
        serde_json::Value::String(master.icon_name.clone()),
    );
    m.format_specific.insert(
        KEY_USER_DATA.into(),
        serde_json::to_value(&master.user_data).unwrap_or(serde_json::Value::Null),
    );
    m.format_specific.insert(
        KEY_MASTER_VISIBLE.into(),
        serde_json::Value::Bool(master.visible),
    );

    m
}

fn interpret_axes(font: &mut Font) {
    // This is going to look very wrong, but after much trial and error I can confirm
    // it works. First: load the axes assuming that userspace=designspace. Then
    // work out the axis mappings. Then apply the mappings to the axis locations.

    let origin: &Master = font.masters.first().unwrap_or_else(|| {
        get_cp(&font.format_specific, "Variable Font Origin")
            .and_then(|x| x.as_str())
            .and_then(|id| font.masters.iter().find(|m| m.id == id))
            .unwrap_or(&font.masters[0])
    });
    for master in font.masters.iter() {
        for axis in font.axes.iter_mut() {
            let loc = master
                .location
                .get(axis.tag)
                .unwrap_or(DesignCoord::default());
            axis.min = if axis.min.is_none() {
                Some(UserCoord::new(loc.to_f64()))
            } else {
                axis.min.map(|v| v.min(UserCoord::new(loc.to_f64())))
            };
            axis.max = if axis.max.is_none() {
                Some(UserCoord::new(loc.to_f64()))
            } else {
                axis.max.map(|v| v.max(UserCoord::new(loc.to_f64())))
            };
            if master.id == origin.id {
                axis.default = Some(UserCoord::new(loc.to_f64()));
            }
        }
    }
    interpret_axis_mappings(font);

    // Now treat as designspace and to userspace
    for axis in font.axes.iter_mut() {
        if let Some(map) = &axis.map {
            axis.default = map
                .iter()
                .find(|(_, design)| Some(design.to_f64()) == axis.default.map(|x| x.to_f64()))
                .map(|(user, _)| *user);
            axis.min = map.iter().map(|(user, _)| *user).min();
            axis.max = map.iter().map(|(user, _)| *user).max();
        }
    }
}

fn interpret_axis_mappings(font: &mut Font) {
    if let Some(mappings) =
        get_cp(&font.format_specific, "Axis Mappings").and_then(|x| x.as_object())
    {
        for (tagstr, map) in mappings {
            if let Ok(tag) = Tag::from_str(tagstr) {
                if let Some(axis) = font.axes.iter_mut().find(|a| a.tag == tag) {
                    if let Some(map) = map.as_array() {
                        let mut axis_map: Vec<(UserCoord, DesignCoord)> = vec![];
                        for pair in map {
                            if let Some(pair) = pair.as_array() {
                                if pair.len() == 2 {
                                    if let (Some(user), Some(design)) =
                                        (pair[0].as_f64(), pair[1].as_f64())
                                    {
                                        axis_map
                                            .push((UserCoord::new(user), DesignCoord::new(design)));
                                    }
                                }
                            }
                        }
                        axis.map = Some(axis_map);
                    }
                }
            }
        }
    }
    for instance in font.instances.iter() {
        // The Axis Location custom parameter is in userspace, use this to make the map
        let c = get_cp(&instance.format_specific, "Axis Location").and_then(|x| x.as_array());
        let empty = &vec![];
        let c = c.unwrap_or(empty);
        let mut c_pairs: Vec<(&str, f64)> = vec![];
        for pair in c {
            if let Some(pair) = pair.as_object() {
                if let (Some(axis), Some(location)) = (
                    pair.get("Axis").and_then(|x| x.as_str()),
                    pair.get("Location").and_then(|x| x.as_f64()),
                ) {
                    c_pairs.push((axis, location));
                }
            }
        }
        if c_pairs.is_empty() {
            if let Some(weightclass) = instance
                .format_specific
                .get(KEY_WEIGHT_CLASS)
                .and_then(|x| x.as_f64())
            {
                c_pairs.push(("Weight", weightclass));
            }
            if let Some(widthclass) = instance
                .format_specific
                .get(KEY_WIDTH_CLASS)
                .and_then(|x| x.as_f64())
            {
                c_pairs.push(("Width", widthclass));
            }
        }
        if c_pairs.is_empty() {
            if instance.name.get_default() == Some(&"Regular".to_string()) {
                c_pairs.push(("Weight", 400.0));
                c_pairs.push(("Width", 100.0));
            } else if instance.name.get_default() == Some(&"Bold".to_string()) {
                c_pairs.push(("Weight", 700.0));
                c_pairs.push(("Width", 100.0));
            }
        }

        for (axis_name, user_location) in c_pairs {
            if let Some(axis) = font
                .axes
                .iter_mut()
                .find(|a| a.name.get_default().map(|x| x.as_str()) == Some(axis_name))
            {
                if let Some(design_location) = instance.location.get(axis.tag) {
                    if axis.map.is_none() {
                        axis.map = Some(vec![]);
                    }
                    if let Some(axis_map) = &mut axis.map {
                        axis_map.push((UserCoord::new(user_location), design_location));
                    }
                }
            }
        }
    }
}

pub(crate) fn as_glyphs3(font: &Font) -> glyphs3::Glyphs3 {
    // Do some cleanups.
    let mut font = font.clone();
    #[allow(clippy::unwrap_used)] // Surely this can't fail
    DropSparseMasters.apply(&mut font).unwrap();

    let axes = font
        .axes
        .iter()
        .map(|ax| glyphs3::Axis {
            hidden: ax.hidden,
            name: ax.name(),
            tag: ax.tag.to_string(),
        })
        .collect();

    let mut our_metrics: Vec<crate::MetricType> = vec![];
    for master in font.masters.iter() {
        for key in master.metrics.keys() {
            if key.as_str().ends_with(" overshoot") {
                continue;
            }
            if !our_metrics.contains(key) {
                our_metrics.push(key.clone());
            }
        }
    }
    let app_version = font
        .format_specific
        .get(KEY_APP_VERSION)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let display_strings: Vec<String> = font
        .format_specific
        .get_parse_or(KEY_DISPLAY_STRINGS, Vec::new());
    let classes = font
        .features
        .classes
        .iter()
        .map(|(name, members)| members.to_featureclass(name))
        .collect();
    let feature_prefixes = font
        .features
        .prefixes
        .iter()
        .map(|(name, code)| code.to_featureprefix(name))
        .collect();
    let features = font
        .features
        .features
        .iter()
        .map(|(tag, code)| code.to_feature(tag))
        .collect();

    let custom_parameters = serialize_custom_parameters(&font.format_specific);

    let family_name = font
        .names
        .family_name
        .get_default()
        .map(|x| x.to_string())
        .unwrap_or_default();

    let masters = font
        .masters
        .iter()
        .map(|x| save_master(x, &font.axes, &our_metrics))
        .collect();

    let settings: glyphs3::Settings = font
        .format_specific
        .get_parse_or(KEY_SETTINGS, glyphs3::Settings::default());
    let stems: Vec<glyphs3::Stem> = font.format_specific.get_parse_or(KEY_STEMS, Vec::new());

    let numbers: Vec<glyphslib::glyphs3::Number> = font
        .format_specific
        .get_parse_or::<Vec<String>>(KEY_NUMBER_NAMES, Vec::new())
        .into_iter()
        .map(|x| glyphslib::glyphs3::Number { name: x })
        .collect();

    let kerning = font
        .masters
        .iter()
        .filter(|m| !m.is_sparse(&font))
        .flat_map(|m| {
            let expanded_kerning: BTreeMap<String, BTreeMap<String, f32>> =
                m.kerning
                    .iter()
                    .fold(BTreeMap::new(), |mut acc, ((first, second), value)| {
                        let first = if let Some(stripped) = first.strip_prefix("@") {
                            SmolStr::from(format!("@MMK_L_{}", stripped))
                        } else {
                            first.clone()
                        };
                        let second = if let Some(stripped) = second.strip_prefix("@") {
                            SmolStr::from(format!("@MMK_R_{}", stripped))
                        } else {
                            second.clone()
                        };
                        acc.entry(first.to_string())
                            .or_default()
                            .insert(second.to_string(), *value as f32);
                        acc
                    });

            (!expanded_kerning.is_empty()).then(|| (m.id.clone(), expanded_kerning))
        })
        .collect();

    let axes_order = font.axes.iter().map(|a| a.tag).collect::<Vec<_>>();

    let properties = save_properties(&font.names);
    let mut glyph_to_first_kern_group = HashMap::new();
    let mut glyph_to_second_kern_group = HashMap::new();
    for (group, glyph) in font.first_kern_groups.iter() {
        for glyph_name in glyph.iter() {
            if glyph_to_first_kern_group.contains_key(glyph_name) {
                log::warn!(
                    "Glyph {} is in multiple first kerning groups, skipping assignment",
                    glyph_name
                );
                continue;
            }
            glyph_to_first_kern_group.insert(glyph_name.clone(), group.clone());
        }
    }
    for (group, glyph) in font.second_kern_groups.iter() {
        for glyph_name in glyph.iter() {
            if glyph_to_second_kern_group.contains_key(glyph_name) {
                log::warn!(
                    "Glyph {} is in multiple second kerning groups, skipping assignment",
                    glyph_name
                );
                continue;
            }
            glyph_to_second_kern_group.insert(glyph_name.clone(), group.clone());
        }
    }
    let glyphs_font = glyphs3::Glyphs3 {
        app_version,
        format_version: 3,
        display_strings,
        axes,
        classes,
        custom_parameters,
        date: font.date.format("%Y-%m-%d %H:%M:%S +0000").to_string(),

        family_name,
        feature_prefixes,
        features,
        glyphs: font
            .glyphs
            .iter()
            .map(|g| {
                glyph_to_glyphs(
                    g,
                    &axes_order,
                    glyph_to_second_kern_group.get(&g.name), // Glyph groups are backwards in Glyphs
                    glyph_to_first_kern_group.get(&g.name),
                )
            })
            .collect(),
        instances: font
            .instances
            .iter()
            .map(|x| save_instance(x, &font.axes))
            .collect(),
        keep_alternates_together: false,
        kerning,
        kerning_rtl: font
            .format_specific
            .get_parse_or(KEY_KERNING_RTL, BTreeMap::new()),
        kerning_vertical: font
            .format_specific
            .get_parse_or(KEY_KERNING_VERTICAL, BTreeMap::new()),
        masters,
        metrics: our_metrics.iter().map(Into::into).collect(),
        note: font.note.clone().unwrap_or_default(),
        numbers,
        properties,
        settings,
        stems,
        units_per_em: font.upm.into(),
        version: glyphslib::common::Version {
            major: font.version.0.into(),
            minor: font.version.1.into(),
        },
        user_data: font
            .format_specific
            .get(KEY_USER_DATA)
            .and_then(|x| serde_json::from_value::<UserData>(x.clone()).ok())
            .unwrap_or_default(),
    };
    glyphs_font
}

fn save_master(master: &Master, axes: &[Axis], metrics: &[crate::MetricType]) -> glyphs3::Master {
    let mut axes_values = vec![];
    for axis in axes {
        axes_values.push(
            master
                .location
                .get(axis.tag)
                .map(|x| x.to_f64())
                .map(|x| x as f32)
                .unwrap_or(0.0),
        );
    }

    let mut metric_values: Vec<glyphs3::MetricValue> = vec![];
    for metric in metrics {
        let position = master.metrics.get(metric).copied().unwrap_or(0);
        let over = master
            .metrics
            .get(&crate::MetricType::Custom(format!(
                "{} overshoot",
                metric.as_str()
            )))
            .copied()
            .unwrap_or(0);
        metric_values.push(glyphs3::MetricValue {
            over: over as f32,
            pos: position as f32,
        });
    }

    glyphs3::Master {
        id: master.id.clone(),
        name: master
            .name
            .get_default()
            .map(|x| x.to_string())
            .unwrap_or_default(),
        axes_values,
        guides: master.guides.iter().map(Into::into).collect(),
        metric_values,
        custom_parameters: serialize_custom_parameters(&master.format_specific),
        stem_values: master
            .format_specific
            .get_parse_or::<Vec<f32>>(KEY_STEM_VALUES, Vec::new()),
        number_values: master
            .format_specific
            .get_parse_or::<Vec<f32>>(KEY_NUMBER_VALUES, Vec::new()),
        icon_name: master
            .format_specific
            .get(KEY_ICON_NAME)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        user_data: master
            .format_specific
            .get(KEY_USER_DATA)
            .and_then(|x| serde_json::from_value::<UserData>(x.clone()).ok())
            .unwrap_or_default(),
        visible: master
            .format_specific
            .get(KEY_MASTER_VISIBLE)
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        properties: vec![], // Wait what?
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::Shape;
    use pretty_assertions::assert_eq;
    use similar::TextDiff;

    use super::*;

    #[test]
    fn test_transform() {
        let f = load("resources/RadioCanadaDisplay.glyphs".into()).unwrap();
        let shape = f
            .glyphs
            .iter()
            .find(|g| g.name == "eacute")
            .unwrap()
            .layers
            .first()
            .unwrap()
            .shapes
            .get(1)
            .unwrap();
        if let Shape::Component(p) = shape {
            assert_eq!(p.reference, "acutecomb");
            assert_eq!(
                p.transform,
                kurbo::Affine::new([1.0, 0.0, 0.0, 1.0, 152.0, 0.0])
            );
        } else {
            panic!("Expected a component shape");
        }
    }

    #[test]
    fn test_roundtrip() {
        let there = load("resources/GlyphsFileFormatv3.glyphs".into()).unwrap();
        let backagain = glyphslib::Font::Glyphs3(as_glyphs3(&there));
        let orig = glyphslib::Font::load_str(
            &fs::read_to_string("resources/GlyphsFileFormatv3.glyphs").unwrap(),
        )
        .unwrap();

        assert!(there.format_specific.get(KEY_STEMS).is_some());
        println!("Original stems: {:?}", there.format_specific.get(KEY_STEMS));
        assert!(!backagain.as_glyphs3().unwrap().stems.is_empty());

        let old_string = orig.to_string().unwrap();
        let new_string = backagain.to_string().unwrap();
        let diff = TextDiff::from_lines(&old_string, &new_string);
        let text_diff = diff.unified_diff().to_string();
        println!("Diff between original and roundtrip:\n{}", text_diff);
        if diff.ratio() < 1.0 {
            panic!("Roundtrip produced different output");
        }
    }

    #[test]
    fn test_load_open_shape() {
        let font = load("resources/GlyphsFileFormatv3.glyphs".into()).unwrap();
        let shape = &font.glyphs.get("A").unwrap().layers[1].shapes[0];
        match shape {
            Shape::Path(p) => assert!(!p.closed),
            _ => panic!("Expected a path shape"),
        }
    }

    #[test]
    fn test_designspace() {
        let font = load("resources/Designspace.glyphs".into()).unwrap();
        assert_eq!(font.axes.len(), 1);
        assert_eq!(font.axes[0].name.get_default().unwrap(), "Weight");
        assert_eq!(font.axes[0].tag, Tag::new(b"wght"));

        assert!(font.axes[0].map.is_some());
        // Axes values are in userspace units
        assert_eq!(font.axes[0].min.unwrap().to_f64(), 100.0);
        assert_eq!(font.axes[0].max.unwrap().to_f64(), 600.0);
        // Master locations are in designspace units
        assert_eq!(
            font.masters[0]
                .location
                .get(Tag::new(b"wght"))
                .unwrap()
                .to_f64(),
            1.0
        );
        assert_eq!(
            font.masters[1]
                .location
                .get(Tag::new(b"wght"))
                .unwrap()
                .to_f64(),
            199.0
        );
        // Instance locations are in designspace units
        assert_eq!(
            font.instances[0]
                .location
                .get(Tag::new(b"wght"))
                .unwrap()
                .to_f64(),
            1.0
        );
        assert_eq!(
            font.instances[1]
                .location
                .get(Tag::new(b"wght"))
                .unwrap()
                .to_f64(),
            7.0
        );
    }

    #[test]
    fn test_kern_groups() {
        let font = load("resources/KernGroupTest.glyphs".into()).unwrap();
        println!("First kern groups: {:?}", font.first_kern_groups);
        println!("Second kern groups: {:?}", font.second_kern_groups);
        let h_group = font.first_kern_groups.get("H").unwrap();
        let o_group = font.second_kern_groups.get("O").unwrap();
        assert_eq!(h_group.len(), 2);
        assert!(h_group.contains(&"D".into()));
        assert!(h_group.contains(&"Dcaron".into()));
        assert_eq!(o_group.len(), 3);
        assert!(o_group.contains(&"D".into()));
        assert!(o_group.contains(&"Dcaron".into()));
        assert!(o_group.contains(&"Dcroat".into()));
    }
}
