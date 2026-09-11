use crate::{
    common::FormatSpecific,
    glyph::{self, glyphs::glyph_to_glyphs},
    i18ndictionary::I18NDictionary,
    names::Names,
    Axis, BabelfontError, CustomOTValues, Font, GlyphList, Master, Tag,
};
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

mod customparameters;

use customparameters::{enabled_cp_value, export_font_level_cps, interpret_custom_parameters};

pub(crate) type UserData = BTreeMap<SmolStr, glyphslib::Plist>;

pub(crate) const KEY_ALIGNMENT: &str = "com.schriftgestalt.Glyphs.alignment";
pub(crate) const KEY_ANCHOR_LOCKED: &str = "com.schriftgestalt.Glyphs.anchorLocked";
pub(crate) const KEY_ANCHOR_ORIENTATION: &str = "com.schriftgestalt.Glyphs.anchorOrientation";
pub(crate) const KEY_ANNOTATIONS: &str = "com.schriftgestalt.Glyphs.annotations";
pub(crate) const KEY_APP_VERSION: &str = "com.schriftgestalt.Glyphs.appVersion";
pub(crate) const KEY_ATTR: &str = "com.schriftgestalt.Glyphs.attr";
pub(crate) const KEY_COLOR_LABEL: &str = "com.schriftgestalt.Glyphs.colorLabel";
pub(crate) const KEY_COMPONENT_ANCHOR: &str = "com.schriftgestalt.Glyphs.componentAnchor";
pub(crate) const KEY_COMPONENT_LOCKED: &str = "com.schriftgestalt.Glyphs.componentLocked";
pub(crate) const KEY_CUSTOM_PARAMETERS: &str = "com.schriftgestalt.Glyphs.customParameters.";
pub(crate) const KEY_DISPLAY_STRINGS: &str = "com.schriftgestalt.Glyphs.displayStrings";
pub(crate) const KEY_FORMAT_VERSION: &str = "com.schriftgestalt.Glyphs.formatVersion";
pub(crate) const KEY_ICON_NAME: &str = "com.schriftgestalt.Glyphs.iconName";
pub(crate) const KEY_INSTANCE_EXPORTS: &str = "com.schriftgestalt.Glyphs.exports";
pub(crate) const KEY_IS_BOLD: &str = "com.schriftgestalt.Glyphs.isBold";
pub(crate) const KEY_IS_ITALIC: &str = "com.schriftgestalt.Glyphs.isItalic";
pub(crate) const KEY_KERNING_RTL: &str = "com.schriftgestalt.Glyphs.kerningRTL";
pub(crate) const KEY_KERNING_VERTICAL: &str = "com.schriftgestalt.Glyphs.kerningVertical";
pub(crate) const KEY_LAYER_HINTS: &str = "com.schriftgestalt.Glyphs.layerHints";
pub(crate) const KEY_LAYER_IMAGE: &str = "com.schriftgestalt.Glyphs.layerBackgroundImage";
pub(crate) const KEY_MASTER_VISIBLE: &str = "com.schriftgestalt.Glyphs.visible";
pub(crate) const KEY_METRIC_BOTTOM: &str = "com.schriftgestalt.Glyphs.metricBottom";
pub(crate) const KEY_METRIC_LEFT: &str = "com.schriftgestalt.Glyphs.metricLeft";
pub(crate) const KEY_METRIC_RIGHT: &str = "com.schriftgestalt.Glyphs.metricRight";
pub(crate) const KEY_METRIC_TOP: &str = "com.schriftgestalt.Glyphs.metricTop";
pub(crate) const KEY_METRIC_VERT_ORIGIN: &str = "com.schriftgestalt.Glyphs.metricVertOrigin";
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
pub(crate) const KEY_STYLISTIC_SET_LABEL: &str = "com.schriftgestalt.Glyphs.labels";

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

/// Load a Glyphs package font from in-memory package entries
pub fn load_package_entries(
    path: PathBuf,
    entries: &HashMap<String, String>,
) -> Result<Font, BabelfontError> {
    let glyphs_font = glyphslib::Font::load_package_entries(entries)
        .map_err(|x| BabelfontError::PlistParse(x.to_string()))?;
    _load(&glyphs_font, path)
}

fn _load(glyphs_font: &glyphslib::Font, path: PathBuf) -> Result<Font, BabelfontError> {
    let mut font = Font::new();
    let mut upgraded = glyphs_font.clone();
    let glyphs_font = if glyphs_font.as_glyphs2().is_some() {
        font.format_specific.insert(
            KEY_FORMAT_VERSION.into(),
            serde_json::Value::String("2".into()),
        );
        upgraded.upgrade_in_place();
        upgraded.as_glyphs3()
    } else {
        glyphs_font.as_glyphs3()
    }
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
    font.date = glyphs_font
        .date
        .parse()
        .unwrap_or_else(|_| chrono::Utc::now());
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
            .collect::<Result<Vec<_>, BabelfontError>>()?,
    );
    // Instances
    font.instances = glyphs_font
        .instances
        .iter()
        .map(|i| load_instance(&font, i))
        .collect();
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
        // The *left side* of the glyph is relevant when the glyph is
        // on the *right side* (second item) of a kerning pair.
        let left_group = glyph.format_specific.get_string("kern_left");
        if !left_group.is_empty() {
            font.second_kern_groups
                .entry(left_group.into())
                .or_default()
                .push(glyph.name.clone());
        }

        // The *right side* of the glyph is relevant when the glyph is
        // on the *left side* (first item) of a kerning pair.
        let right_group = glyph.format_specific.get_string("kern_right");
        if !right_group.is_empty() {
            font.first_kern_groups
                .entry(right_group.into())
                .or_default()
                .push(glyph.name.clone());
        }
    }

    // Interpret metrics
    // Interpret axes
    interpret_axes(&mut font)?;
    // Interpret font-level custom parameters
    interpret_custom_parameters(&mut font)?;

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
    format_specific.insert_some_json(KEY_WEIGHT_CLASS, &instance.weight_class.as_ref());
    format_specific.insert_some_json(KEY_WIDTH_CLASS, &instance.width_class.as_ref());
    format_specific.insert_if_ne_json(KEY_INSTANCE_EXPORTS, &instance.exports, &true);
    format_specific.insert_if_ne_json(KEY_IS_BOLD, &instance.is_bold, &false);
    format_specific.insert_if_ne_json(KEY_IS_ITALIC, &instance.is_italic, &false);
    let mut names = Names::new();
    let mut custom_ot_values = CustomOTValues::default();
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

fn save_instance(
    instance: &crate::Instance,
    axes: &[Axis],
    weight_class: Option<i32>,
    width_class: Option<i32>,
) -> glyphs3::Instance {
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
        // An instance's own weightClass/widthClass wins; otherwise the
        // font-level OS/2 classes the source stated (custom_ot_values) are
        // serialized here, because the instance field is where the Glyphs
        // format carries them.
        weight_class: format_specific
            .get(KEY_WEIGHT_CLASS)
            .and_then(|x| x.as_i64())
            .map(|x| x as i32)
            .or(weight_class),
        width_class: format_specific
            .get(KEY_WIDTH_CLASS)
            .and_then(|x| x.as_i64())
            .map(|x| x as i32)
            .or(width_class),
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
        properties: save_properties(&instance.custom_names, &CustomOTValues::default()),
        export_type: if instance.variable {
            glyphslib::glyphs3::ExportType::Variable
        } else {
            glyphslib::glyphs3::ExportType::Static
        },
    }
}

fn load_properties(
    names: &mut Names,
    custom_ot_values: &mut CustomOTValues,
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
                glyphs3::SingularPropertyKey::VendorID => {
                    if let Ok(x) = Tag::from_str(value) {
                        custom_ot_values.os2_vendor_id = Some(x);
                    }
                }
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
                    glyphs3::LocalizedPropertyKey::PostscriptFullNames => {
                        names.postscript_name = value
                    }
                }
            }
            glyphs3::Property::Junk(plist) => {
                log::warn!("Ignoring junk property in Glyphs3 font: {:?}", plist);
            }
        }
    }
}

fn save_properties(names: &Names, custom_ot_values: &CustomOTValues) -> Vec<glyphs3::Property> {
    let mut properties: Vec<glyphs3::Property> = vec![];

    // Macro for singular-only properties (no localized variant). These
    // cannot carry a localization, so a name that exists only under a
    // language tag (an SFD's LangName strings live under ENG) must fall
    // back to it rather than be dropped.
    macro_rules! push_singular {
        ($field:expr, $key:expr) => {
            if let Some(value) = $field.get_default_or_fallback() {
                properties.push(glyphs3::Property::SingularProperty {
                    key: $key,
                    value: value.clone(),
                });
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

    push_localized!(
        names.compatible_full_name,
        glyphs3::LocalizedPropertyKey::CompatibleFullNames
    );

    push_localized!(names.copyright, glyphs3::LocalizedPropertyKey::Copyrights);

    // Glyphs 3 always emits the localized form.
    push_localized!(names.designer, glyphs3::LocalizedPropertyKey::Designers);

    push_singular!(
        names.designer_url,
        glyphs3::SingularPropertyKey::DesignerUrl
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
    push_singular!(names.license_url, glyphs3::SingularPropertyKey::LicenseUrl);

    // Glyphs 3 always emits the localized form.
    push_localized!(
        names.manufacturer,
        glyphs3::LocalizedPropertyKey::Manufacturers
    );

    push_singular!(
        names.manufacturer_url,
        glyphs3::SingularPropertyKey::ManufacturerUrl
    );
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

    // Output vendor ID from custom OT values if present
    if let Some(vendor_id) = custom_ot_values.os2_vendor_id {
        properties.push(glyphs3::Property::SingularProperty {
            key: glyphs3::SingularPropertyKey::VendorID,
            value: vendor_id.to_string(),
        });
    }
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

fn find_base_style(masters: &[Master]) -> String {
    if masters.is_empty() {
        return String::new();
    }
    let first_name = masters[0]
        .name
        .get_default()
        .map(|s| s.as_str())
        .unwrap_or("");
    let mut base_style: Vec<&str> = first_name.split_whitespace().collect();
    for master in masters.iter().skip(1) {
        let name = master.name.get_default().map(|s| s.as_str()).unwrap_or("");
        let style: Vec<&str> = name.split_whitespace().collect();
        base_style.retain(|s| style.contains(s));
    }
    base_style.join(" ")
}

fn get_origin_master(font: &Font) -> Option<&Master> {
    if font.masters.is_empty() {
        return None;
    }
    // The current Glyphs spec uses "Variable Font Origin" (matched by master ID).
    if let Some(id) =
        enabled_cp_value(&font.format_specific, "Variable Font Origin").and_then(|x| x.as_str())
    {
        if let Some(master) = font.masters.iter().find(|m| m.id == id) {
            return Some(master);
        }
    }
    // Older name: "Variation Font Origin" (matched by master name).
    if let Some(name) =
        enabled_cp_value(&font.format_specific, "Variation Font Origin").and_then(|x| x.as_str())
    {
        if let Some(master) = font
            .masters
            .iter()
            .find(|m| m.name.get_default().map(|n| n.as_str()) == Some(name))
        {
            return Some(master);
        }
    }
    // Fall back to the base style shared by all masters (default "Regular").
    let base_style = find_base_style(&font.masters);
    let base_style = if base_style.is_empty() {
        "Regular".to_string()
    } else {
        base_style
    };
    if let Some(master) = font
        .masters
        .iter()
        .find(|m| m.name.get_default().map(|n| n.as_str()) == Some(base_style.as_str()))
    {
        return Some(master);
    }
    // Second attempt: master whose name with "Regular" stripped matches base_style.
    if let Some(master) = font.masters.iter().find(|m| {
        m.name.get_default().is_some_and(|name| {
            let without_regular = name
                .split_whitespace()
                .filter(|&w| w != "Regular")
                .collect::<Vec<_>>()
                .join(" ");
            without_regular == base_style
        })
    }) {
        return Some(master);
    }
    font.masters.first()
}

/// The OS/2 width class is a 1–9 enumeration, not a user-space value; the
/// user-space width Glyphs exposes is a percentage of the normal width. See
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/os2#uswidthclass>.
/// This mirrors glyphsLib's `WIDTH_CLASS_TO_VALUE`.
const WIDTH_CLASS_TO_VALUE: [f64; 9] = [
    50.0,  // Ultra-condensed
    62.5,  // Extra-condensed
    75.0,  // Condensed
    87.5,  // Semi-condensed
    100.0, // Medium (normal)
    112.5, // Semi-expanded
    125.0, // Expanded
    150.0, // Extra-expanded
    200.0, // Ultra-expanded
];

/// Translate an OS/2 width class (1–9) into the user-space percentage Glyphs
/// uses for the width axis.
fn width_class_to_user_loc(class: i64) -> Option<f64> {
    let index = usize::try_from(class).ok()?;
    if index == 0 || index > WIDTH_CLASS_TO_VALUE.len() {
        return None;
    }
    WIDTH_CLASS_TO_VALUE.get(index - 1).copied()
}

/// Which family an axis belongs to. This decides the default user location and
/// whether the OS/2 weight/width classes are used to derive one. Mirrors
/// glyphsLib's `AxisDefinition`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AxisKind {
    Weight,
    Width,
    Other,
}

impl AxisKind {
    fn of(tag: Tag) -> Self {
        if tag == Tag::new(b"wght") {
            AxisKind::Weight
        } else if tag == Tag::new(b"wdth") {
            AxisKind::Width
        } else {
            AxisKind::Other
        }
    }

    /// The user location used when nothing else supplies one.
    fn default_user_loc(self) -> f64 {
        match self {
            AxisKind::Weight => 400.0,
            AxisKind::Width => 100.0,
            AxisKind::Other => 0.0,
        }
    }
}

/// A user → design mapping being assembled for one axis.
#[derive(Default, Clone)]
struct AxisMapping {
    pairs: Vec<(f64, f64)>,
}

impl AxisMapping {
    /// Add a mapping, overwriting (with a warning) any previous mapping for the
    /// same user location — glyphsLib's behaviour, rather than erroring.
    fn insert(&mut self, user: f64, design: f64, axis_name: &str) {
        if let Some(existing) = self.pairs.iter_mut().find(|(u, _)| *u == user) {
            if existing.1 != design {
                log::warn!(
                    "Axis {axis_name}: redefining the mapping for user location {user} \
                     from {} to {design}",
                    existing.1
                );
            }
            existing.1 = design;
        } else {
            self.pairs.push((user, design));
        }
    }

    fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Whether every entry maps a user location to the identical design
    /// location, so there is nothing to record.
    fn is_identity(&self) -> bool {
        self.pairs.iter().all(|(user, design)| user == design)
    }

    /// Mapping entries sorted by user location.
    fn sorted(&self) -> Vec<(f64, f64)> {
        let mut pairs = self.pairs.clone();
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        pairs
    }
}

/// The computed range, default and mapping for one axis.
#[derive(Default)]
struct ComputedAxis {
    min: Option<f64>,
    max: Option<f64>,
    default: Option<f64>,
    map: Option<Vec<(f64, f64)>>,
}

/// The per-object inputs used to derive a user location along one axis.
struct UserLocSource<'a> {
    format_specific: &'a FormatSpecific,
    design_loc: Option<f64>,
    weight_class: Option<f64>,
    width_class: Option<f64>,
}

impl<'a> UserLocSource<'a> {
    /// A master: Glyphs masters have no weight/width class of their own (their
    /// `weight`/`width` keys are legacy name fragments, not locations).
    fn master(format_specific: &'a FormatSpecific, design_loc: Option<f64>) -> Self {
        Self {
            format_specific,
            design_loc,
            weight_class: None,
            width_class: None,
        }
    }

    /// An instance, whose `weightClass`/`widthClass` attributes are OS/2
    /// classes used to derive a user location.
    fn instance(instance: &'a crate::Instance, design_loc: Option<f64>) -> Self {
        Self {
            format_specific: &instance.format_specific,
            design_loc,
            weight_class: instance_weight_class(instance),
            width_class: instance_width_class(instance),
        }
    }
}

/// Resolve the user location of a master or instance along one axis, following
/// glyphsLib's precedence: the default/design location is overridden by the
/// `weightClass`/`widthClass` attributes, and finally by the `Axis Location`
/// custom parameter (which is already in user space).
fn get_user_loc(
    kind: AxisKind,
    default_user_loc: f64,
    tag: Tag,
    axis_name: &str,
    source: UserLocSource<'_>,
) -> Option<f64> {
    let UserLocSource {
        format_specific,
        design_loc,
        weight_class,
        width_class,
    } = source;
    // For weight the user location defaults to 400 regardless of the design
    // location; for every other axis it defaults to the design location.
    let mut user_loc = match kind {
        AxisKind::Weight => Some(default_user_loc),
        _ => design_loc,
    };

    match kind {
        AxisKind::Weight => {
            if let Some(class) = weight_class {
                // An OS/2 weight class *is* a user location.
                user_loc = Some(class);
            }
        }
        AxisKind::Width => {
            if let Some(class) = width_class.and_then(|c| width_class_to_user_loc(c.round() as i64))
            {
                user_loc = Some(class);
            }
        }
        AxisKind::Other => {}
    }

    // The Axis Location custom parameter wins over everything else.
    if let Some(location) = axis_location_value(format_specific, tag, axis_name) {
        user_loc = Some(location);
    }

    user_loc
}

/// The enabled `Axis Location` custom parameter of a master or instance, if
/// any.
fn axis_location_cp(format_specific: &FormatSpecific) -> Option<&serde_json::Value> {
    enabled_cp_value(format_specific, "Axis Location")
}

/// The user-space location for one axis from an `Axis Location` custom
/// parameter. Glyphs stores this as a list of `{Axis, Location}` dicts; some
/// versions use a dict keyed by axis name or tag.
fn axis_location_value(format_specific: &FormatSpecific, tag: Tag, axis_name: &str) -> Option<f64> {
    let value = axis_location_cp(format_specific)?;
    if let Some(list) = value.as_array() {
        for entry in list {
            if let Some(entry) = entry.as_object() {
                if entry.get("Axis").and_then(|axis| axis.as_str()) == Some(axis_name) {
                    return entry.get("Location").and_then(parse_number);
                }
            }
        }
    } else if let Some(map) = value.as_object() {
        let tag_string = tag.to_string();
        for key in [axis_name, tag_string.as_str()] {
            if let Some(location) = map.get(key).and_then(parse_number) {
                return Some(location);
            }
        }
    }
    None
}

/// A JSON number, or a string holding one (Glyphs sometimes writes
/// `Location = "200"`).
fn parse_number(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

/// Instances that contribute to the axis mapping: exported, non-variable ones
/// (glyphsLib's `is_instance_active` plus its `InstanceType.VARIABLE` filter).
fn mapping_instances(font: &Font) -> impl Iterator<Item = &crate::Instance> {
    font.instances.iter().filter(|instance| {
        !instance.variable
            && instance
                .format_specific
                .get(KEY_INSTANCE_EXPORTS)
                .and_then(|exports| exports.as_bool())
                .unwrap_or(true)
    })
}

fn instance_weight_class(instance: &crate::Instance) -> Option<f64> {
    instance
        .format_specific
        .get(KEY_WEIGHT_CLASS)
        .and_then(|class| class.as_f64())
}

fn instance_width_class(instance: &crate::Instance) -> Option<f64> {
    instance
        .format_specific
        .get(KEY_WIDTH_CLASS)
        .and_then(|class| class.as_f64())
}

/// Invert a user → design mapping at `design`, interpolating between known
/// points and extrapolating beyond the ends (as fontTools' `piecewiseLinearMap`
/// does).
fn design_to_user(pairs: &[(f64, f64)], design: f64) -> Option<f64> {
    if pairs.is_empty() {
        return None;
    }
    let mut by_design: Vec<(f64, f64)> = pairs
        .iter()
        .map(|(user, design)| (*design, *user))
        .collect();
    by_design.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    if let Some((_, user)) = by_design.iter().find(|(d, _)| *d == design) {
        return Some(*user);
    }
    let (first_design, first_user) = *by_design.first()?;
    if design < first_design {
        return Some(design + first_user - first_design);
    }
    let (last_design, last_user) = *by_design.last()?;
    if design > last_design {
        return Some(design + last_user - last_design);
    }
    for window in by_design.windows(2) {
        if let [(d0, u0), (d1, u1)] = window {
            if design >= *d0 && design <= *d1 {
                if d1 == d0 {
                    return Some(*u0);
                }
                let t = (design - d0) / (d1 - d0);
                return Some(u0 + t * (u1 - u0));
            }
        }
    }
    None
}

/// The user location in `pairs` closest to `value`. `Axis`'s converter requires
/// the default to be one of the mapping's user coordinates, so an interpolated
/// default is snapped to the nearest entry.
fn nearest_user_loc(pairs: &[(f64, f64)], value: f64) -> Option<f64> {
    pairs.iter().map(|(user, _)| *user).min_by(|a, b| {
        (a - value)
            .abs()
            .partial_cmp(&(b - value).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Turn a mapping into the axis's range/default/map, following glyphsLib: the
/// min/max are the mapping's extremes, the default is derived from the regular
/// master, and an identity mapping is elided.
fn finish_axis(mapping: AxisMapping, regular_design: Option<f64>, axis_name: &str) -> ComputedAxis {
    if mapping.is_empty() {
        return ComputedAxis::default();
    }
    let pairs = mapping.sorted();
    let (Some(min), Some(max)) = (
        pairs.first().map(|(user, _)| *user),
        pairs.last().map(|(user, _)| *user),
    ) else {
        return ComputedAxis::default();
    };

    if mapping.is_identity() {
        // user == design, so there is nothing to map; the default is just the
        // regular master's location, clamped into range.
        let default = regular_design.map(|d| d.clamp(min, max)).unwrap_or(min);
        return ComputedAxis {
            min: Some(min),
            max: Some(max),
            default: Some(default),
            map: None,
        };
    }

    let default = regular_design
        .and_then(|design| design_to_user(&pairs, design))
        .map(|user| user.clamp(min, max))
        .and_then(|user| nearest_user_loc(&pairs, user))
        .or(Some(min));
    if default.is_none() {
        log::warn!("Axis {axis_name}: could not determine a default location");
    }
    ComputedAxis {
        min: Some(min),
        max: Some(max),
        default,
        map: Some(pairs),
    }
}

/// An identity mapping built from the masters' design locations — glyphsLib's
/// `master_mapping`, used when the instances don't provide a usable mapping.
fn mapping_from_masters(master_locs: &[f64], axis_name: &str) -> AxisMapping {
    let mut mapping = AxisMapping::default();
    for design in master_locs {
        mapping.insert(*design, *design, axis_name);
    }
    mapping
}

/// Parse the `Axis Mappings` custom parameter for one axis. The parameter is a
/// dict of tag → mapping, where each mapping is either a list of `[user,
/// design]` pairs or a dict of user → design.
fn explicit_axis_mapping(mappings: &serde_json::Value, tag: Tag) -> Option<Vec<(f64, f64)>> {
    let map = mappings.as_object()?.get(&tag.to_string())?;
    let mut pairs = Vec::new();
    if let Some(array) = map.as_array() {
        for item in array {
            if let Some(pair) = item.as_array() {
                if let (Some(user), Some(design)) = (
                    pair.first().and_then(parse_number),
                    pair.get(1).and_then(parse_number),
                ) {
                    pairs.push((user, design));
                }
            }
        }
    } else if let Some(object) = map.as_object() {
        for (user, design) in object {
            if let (Ok(user), Some(design)) = (user.parse::<f64>(), parse_number(design)) {
                pairs.push((user, design));
            }
        }
    }
    Some(pairs)
}

/// Compute the range, default and mapping for one axis, following glyphsLib's
/// three-way strategy:
///
/// * an explicit `Axis Mappings` custom parameter is authoritative;
/// * otherwise, if every master carries an `Axis Location` parameter, the
///   mapping comes from the masters (instances contributing only their own
///   `Axis Location`);
/// * otherwise the mapping is derived from the instances, falling back to the
///   masters' design locations when that yields nothing useful.
fn compute_axis(
    font: &Font,
    axis: &Axis,
    origin_id: Option<&str>,
    explicit_mappings: Option<&serde_json::Value>,
    uses_axis_locations: bool,
) -> ComputedAxis {
    let tag = axis.tag;
    let kind = AxisKind::of(tag);
    let axis_name = axis.name();
    let default_user_loc = kind.default_user_loc();

    let master_locs: Vec<f64> = font
        .masters
        .iter()
        .filter_map(|master| master.location.get(tag).map(|coord| coord.to_f64()))
        .collect();
    let regular_design = origin_id
        .and_then(|id| font.masters.iter().find(|master| master.id == id))
        .and_then(|master| master.location.get(tag))
        .map(|coord| coord.to_f64())
        .or_else(|| master_locs.first().copied());

    // Strategy A: an explicit "Axis Mappings" custom parameter is authoritative.
    if let Some(mappings) = explicit_mappings {
        if let Some(pairs) = explicit_axis_mapping(mappings, tag) {
            if !pairs.is_empty() {
                let mut mapping = AxisMapping::default();
                for (user, design) in pairs {
                    mapping.insert(user, design, &axis_name);
                }
                return finish_axis(mapping, regular_design, &axis_name);
            }
        }
        log::warn!(
            "Font has an Axis Mappings custom parameter but no mapping for axis {tag}; \
             leaving it unmapped"
        );
        return finish_axis(
            mapping_from_masters(&master_locs, &axis_name),
            regular_design,
            &axis_name,
        );
    }

    // Strategy B: every master states its user location with an "Axis Location"
    // parameter. Instances then only contribute their own "Axis Location".
    if uses_axis_locations {
        let mut mapping = AxisMapping::default();
        for master in font.masters.iter() {
            let Some(design) = master.location.get(tag).map(|coord| coord.to_f64()) else {
                continue;
            };
            if let Some(user) = get_user_loc(
                kind,
                default_user_loc,
                tag,
                &axis_name,
                UserLocSource::master(&master.format_specific, Some(design)),
            ) {
                mapping.insert(user, design, &axis_name);
            }
        }
        for instance in mapping_instances(font) {
            let Some(design) = instance.location.get(tag).map(|coord| coord.to_f64()) else {
                continue;
            };
            if let Some(user) = axis_location_value(&instance.format_specific, tag, &axis_name) {
                mapping.insert(user, design, &axis_name);
            }
        }
        return finish_axis(mapping, regular_design, &axis_name);
    }

    // Strategy C: derive the mapping from the instances; if that yields nothing
    // useful, fall back to the masters' design locations.
    let mut instance_mapping = AxisMapping::default();
    for instance in mapping_instances(font) {
        let Some(design) = instance.location.get(tag).map(|coord| coord.to_f64()) else {
            continue;
        };
        if let Some(user) = get_user_loc(
            kind,
            default_user_loc,
            tag,
            &axis_name,
            UserLocSource::instance(instance, Some(design)),
        ) {
            instance_mapping.insert(user, design, &axis_name);
        }
    }
    let mapping = if !instance_mapping.is_empty() && !instance_mapping.is_identity() {
        instance_mapping
    } else {
        mapping_from_masters(&master_locs, &axis_name)
    };
    finish_axis(mapping, regular_design, &axis_name)
}

fn interpret_axes(font: &mut Font) -> Result<(), BabelfontError> {
    let origin_id = get_origin_master(font).map(|m| m.id.clone());
    let explicit_mappings = enabled_cp_value(&font.format_specific, "Axis Mappings");
    let uses_axis_locations = !font.axes.is_empty()
        && !font.masters.is_empty()
        && font
            .masters
            .iter()
            .all(|master| axis_location_cp(&master.format_specific).is_some());

    let computed: Vec<ComputedAxis> = font
        .axes
        .iter()
        .map(|axis| {
            compute_axis(
                font,
                axis,
                origin_id.as_deref(),
                explicit_mappings,
                uses_axis_locations,
            )
        })
        .collect();

    for (axis, computed) in font.axes.iter_mut().zip(computed) {
        axis.min = computed.min.map(UserCoord::new);
        axis.max = computed.max.map(UserCoord::new);
        axis.default = computed.default.map(UserCoord::new);
        axis.map = computed.map.map(|pairs| {
            pairs
                .into_iter()
                .map(|(user, design)| (UserCoord::new(user), DesignCoord::new(design)))
                .collect()
        });
    }

    Ok(())
}

pub(crate) fn as_glyphs3(font: &Font) -> Result<glyphs3::Glyphs3, BabelfontError> {
    // Do some cleanups.
    let mut font = font.clone();
    // #[allow(clippy::unwrap_used)] // Surely this can't fail
    // DropSparseMasters.apply(&mut font).unwrap();
    // println!("Exporting {} masters", font.masters.len());

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
            // OS/2 + hhea vertical metrics are emitted as custom parameters
            // (append_master_vertical_metrics), not as metric slots.
            if customparameters::is_vertical_metric_cp(key) {
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

    // This serializes the ones we have in .format_specific
    let mut custom_parameters = serialize_custom_parameters(&font.format_specific);
    // Now overlay any font-level custom parameters in the font structure (custom_ot_values etc)
    export_font_level_cps(&mut custom_parameters, &mut font)?;

    let family_name = font
        .names
        .family_name
        .get_default()
        .map(|x| x.to_string())
        .unwrap_or_default();

    let masters: Vec<glyphs3::Master> = font
        .masters
        .iter()
        .map(|x| save_master(x, &font.axes, &our_metrics, &font.format_specific))
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

    let properties = save_properties(&font.names, &font.custom_ot_values);
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
            .map(|x| {
                save_instance(
                    x,
                    &font.axes,
                    font.custom_ot_values.os2_us_weight_class.map(|w| w as i32),
                    font.custom_ot_values.os2_us_width_class.map(|w| w as i32),
                )
            })
            .collect(),
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
    Ok(glyphs_font)
}

fn save_master(
    master: &Master,
    axes: &[Axis],
    metrics: &[crate::MetricType],
    font_format_specific: &FormatSpecific,
) -> glyphs3::Master {
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

    let mut custom_parameters = serialize_custom_parameters(&master.format_specific);
    // OS/2 + hhea vertical metrics live in master-level custom parameters.
    customparameters::append_master_vertical_metrics(
        &mut custom_parameters,
        master,
        font_format_specific,
    );

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
        custom_parameters,
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
        temp_data: Default::default(),
    }
}

#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
#[cfg(test)]
mod tests {
    #[test]
    fn english_only_singular_properties_are_not_dropped() {
        // An SFD's LangName strings populate the names under the ENG tag,
        // not the default; the singular Glyphs properties (the URLs among
        // them) must fall back to that instead of vanishing. Verified
        // against the Google Fonts corpus: 119 of 121 shipped fonts carry a
        // license URL that the converted .glyphs lost this way.
        let mut font = crate::Font::new();
        font.names
            .designer_url
            .insert("ENG".to_string(), "http://example.com/d".to_string());
        font.names
            .license_url
            .insert("ENG".to_string(), "http://scripts.sil.org/OFL".to_string());
        font.names
            .manufacturer_url
            .insert("ENG".to_string(), "http://example.com/m".to_string());
        let props = super::save_properties(&font.names, &font.custom_ot_values);
        let singulars: Vec<_> = props
            .iter()
            .filter_map(|p| match p {
                glyphslib::glyphs3::Property::SingularProperty { key, value } => {
                    Some((format!("{:?}", key), value.clone()))
                }
                _ => None,
            })
            .collect();
        for (key, value) in [
            ("DesignerUrl", "http://example.com/d"),
            ("LicenseUrl", "http://scripts.sil.org/OFL"),
            ("ManufacturerUrl", "http://example.com/m"),
        ] {
            assert!(
                singulars.iter().any(|(k, v)| k == key && v == value),
                "missing singular property {key}: got {singulars:?}"
            );
        }
    }

    #[test]
    fn font_level_os2_classes_reach_the_exported_instance() {
        // The Glyphs format carries usWeightClass/usWidthClass on instances.
        // A font-level value read from another format (custom_ot_values) is
        // serialized there, unless the instance states its own.
        let mut font = crate::Font::new();
        font.custom_ot_values.os2_us_weight_class = Some(700);
        font.custom_ot_values.os2_us_width_class = Some(3);
        let mut inst = crate::Instance::default();
        inst.name.set_default("Bold".to_string());
        font.instances.push(inst);
        let g = super::as_glyphs3(&font).unwrap();
        assert_eq!(g.instances[0].weight_class, Some(700));
        assert_eq!(g.instances[0].width_class, Some(3));

        // An instance's own value wins over the font level.
        let mut font2 = crate::Font::new();
        font2.custom_ot_values.os2_us_weight_class = Some(700);
        let mut inst2 = crate::Instance::default();
        inst2.name.set_default("Own".to_string());
        inst2
            .format_specific
            .insert(super::KEY_WEIGHT_CLASS.to_string(), serde_json::json!(500));
        font2.instances.push(inst2);
        let g2 = super::as_glyphs3(&font2).unwrap();
        assert_eq!(g2.instances[0].weight_class, Some(500));
    }

    use crate::{GlyphCategory, Shape};
    use fontdrasil::coords::Location;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
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
                p.transform.as_affine(),
                kurbo::Affine::new([1.0, 0.0, 0.0, 1.0, 152.0, 0.0])
            );
        } else {
            panic!("Expected a component shape");
        }
    }

    #[test]
    fn test_default_master() {
        let font = load("resources/Nunito3.glyphs".into()).unwrap();
        assert_eq!(font.masters.len(), 6);
        assert_eq!(
            font.masters[0].location,
            Location::from(vec![
                (Tag::new(b"wght"), DesignCoord::new(42.0)),
                (Tag::new(b"ital"), DesignCoord::new(0.0)),
            ])
        );
        assert_eq!(font.axes.len(), 2);
        assert_eq!(font.axes[0].tag, Tag::new(b"wght"));
        assert_eq!(font.axes[0].min.unwrap().to_f64(), 200.0);
        assert_eq!(font.axes[0].max.unwrap().to_f64(), 1000.0);
        assert_eq!(font.axes[0].default.unwrap().to_f64(), 200.0);
        // Every master states an "Axis Location", so the mapping comes from
        // those (with instances contributing their own Axis Location). This
        // matches glyphsLib's designspace output for the same file.
        assert_eq!(
            axis_map(&font.axes[0]),
            vec![
                (200.0, 42.0),
                (300.0, 61.0),
                (400.0, 81.0),
                (600.0, 101.0),
                (700.0, 125.0),
                (800.0, 151.0),
                (900.0, 178.0),
                (1000.0, 208.0),
            ]
        );
        // The italic axis is unmapped (user == design) and runs 0..9.
        assert_eq!(font.axes[1].tag, Tag::new(b"ital"));
        assert_eq!(font.axes[1].min.unwrap().to_f64(), 0.0);
        assert_eq!(font.axes[1].max.unwrap().to_f64(), 9.0);
        assert_eq!(font.axes[1].default.unwrap().to_f64(), 0.0);
        assert!(font.axes[1].map.is_none());
        assert!(font.default_master().is_some());
    }

    #[rstest]
    fn test_roundtrip(
        #[files("resources/*glyphs")]
        // Weird instance configuration means we can't find the default master
        #[exclude("GlyphsFileFormatv3.glyphs")]
        // Small but insignificant difference in property serialization.
        // Glyphs sometimes stores a singular property as a localized property
        // with only one entry, and vice versa.
        #[exclude("RadioCanadaDisplay.glyphs")]
        #[exclude("RadioCanadaDisplay-Italic.glyphs")]
        #[exclude("Nunito3.glyphs")]
        #[exclude("NotoSansGrantha-SmartComponent.glyphs")]
        #[exclude("Fustat.glyphs")]
        // RTL kerning is merged into LTR on load, so roundtrip output differs structurally
        #[exclude("G3RTLKerning.glyphs")]
        path: PathBuf,
    ) {
        let there = load(path.clone()).unwrap();
        let backagain = glyphslib::Font::Glyphs3(as_glyphs3(&there).unwrap());
        let orig = glyphslib::Font::load_str(&fs::read_to_string(path).unwrap()).unwrap();

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
        assert_eq!(font.axes[0].default.unwrap().to_f64(), 100.0);
        // Every master carries an "Axis Location", so the mapping comes from
        // those and not from the instances' weightClass values. Verified
        // against glyphsLib's own output for this file.
        assert_eq!(axis_map(&font.axes[0]), vec![(100.0, 1.0), (600.0, 199.0)]);
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
        // *Left* side of D looks like an H, so is used when D is the *second*
        // glyph in a pair.
        let h_group = font.second_kern_groups.get("H").unwrap();
        assert_eq!(h_group.len(), 2);
        assert!(h_group.contains(&"D".into()));
        assert!(h_group.contains(&"Dcaron".into()));
        // *Right* side of D looks like an O, so is used when D is the *first*
        // glyph in a pair.
        let o_group = font.first_kern_groups.get("O").unwrap();
        assert_eq!(o_group.len(), 3);
        assert!(o_group.contains(&"D".into()));
        assert!(o_group.contains(&"Dcaron".into()));
        assert!(o_group.contains(&"Dcroat".into()));
    }

    #[test]
    fn test_timezone() {
        let font_date = "2024-05-08 05:56:55 +0000";
        let date = font_date.parse().unwrap_or_else(|_| chrono::Utc::now());
        let formatted_date = date.format("%Y-%m-%d %H:%M:%S +0000").to_string();
        assert_eq!(formatted_date, font_date);
    }

    #[test]
    fn test_smart_components() {
        let font = load("resources/NotoSansGrantha-SmartComponent.glyphs".into()).unwrap();
        let glyph = font.glyphs.get("_part.iMatra").unwrap();
        assert_eq!(glyph.component_axes.len(), 5);
        assert_eq!(glyph.component_axes[0].name.get_default().unwrap(), "Width");
        assert_eq!(glyph.component_axes[0].min, Some(UserCoord::new(0.0)));
        assert_eq!(glyph.component_axes[0].max, Some(UserCoord::new(800.0)));
        assert_eq!(glyph.component_axes[1].name.get_default().unwrap(), "Dip");
        assert_eq!(glyph.component_axes[1].min, Some(UserCoord::new(0.0)));
        assert_eq!(glyph.component_axes[1].max, Some(UserCoord::new(100.0)));
        // Each layer should have some location in those axes
        // Layer one is called Wide
        assert_eq!(glyph.layers[1].name.as_ref().unwrap(), "Wide");
        let layer_location = &glyph.layers[1].smart_component_location;
        assert!(!layer_location.is_empty());
        println!("Layer location: {:?}", layer_location);
        assert_eq!(layer_location.len(), 1);
        assert_eq!(
            layer_location.get("Width").unwrap(),
            &DesignCoord::new(800.0)
        );

        // Test the user end
        let layer = font
            .glyphs
            .get("ny_ji_gran")
            .unwrap()
            .layers
            .first()
            .unwrap();
        let shapes = &layer.shapes;
        let Shape::Component(first) = shapes.first().unwrap() else {
            panic!("Expected component shape");
        };
        assert_eq!(first.reference, "ny_ja_gran");
        assert_eq!(first.location, IndexMap::<String, DesignCoord>::default());
        // But the second is a smart component
        let Shape::Component(smart) = &shapes[1] else {
            panic!("Expected component shape");
        };
        assert_eq!(smart.reference, "_part.iMatra");
        assert_eq!(smart.location.len(), 3);
        assert_eq!(
            smart.location.get("Width").unwrap(),
            &DesignCoord::new(520.0)
        );
    }

    #[test]
    fn test_load_marks() {
        let font = load("resources/NotoSansLimbu.glyphs".into()).unwrap();
        /*
        category = Mark;
        glyphname = uni1938;
        subCategory = Spacing;
        */
        let uni1938 = font.glyphs.get("uni1938").unwrap();
        assert_eq!(uni1938.category, GlyphCategory::Base);
        // When we send it back to Glyphs, it should get Mark/Spacing again
        let glyphs_glyph = glyph_to_glyphs(
            uni1938,
            &font.axes.iter().map(|a| a.tag).collect::<Vec<_>>(),
            None,
            None,
        );
        assert_eq!(glyphs_glyph.category, Some("Mark".to_string()));
        assert_eq!(glyphs_glyph.subcategory, Some("Spacing".to_string()));
    }

    #[test]
    fn all_the_axes() {
        let font = load("resources/RadioCanadaDisplay-Italic.glyphs".into()).unwrap();
        assert_eq!(font.axes[0].tag, Tag::new(b"wght"));
        assert_eq!(
            font.masters[0].location,
            DesignLocation::from(vec![(Tag::new(b"wght"), DesignCoord::new(400.0))])
        );
        assert_eq!(font.axes[0].min, Some(UserCoord::new(400.0)));
        // No instance supplies a distinct user location, so the axis is
        // unmapped and spans the masters. Matches glyphsLib's output.
        assert_eq!(font.axes[0].max, Some(UserCoord::new(700.0)));
        assert_eq!(font.axes[0].default, Some(UserCoord::new(400.0)));
        assert!(font.axes[0].map.is_none());
    }

    fn load_test_font(source: &str) -> Font {
        load_str(source, PathBuf::from("test.glyphs")).unwrap()
    }

    fn axis_map(axis: &Axis) -> Vec<(f64, f64)> {
        axis.map
            .as_ref()
            .map(|map| {
                map.iter()
                    .map(|(user, design)| (user.to_f64(), design.to_f64()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn user(axis: &Axis, which: fn(&Axis) -> Option<UserCoord>) -> Option<f64> {
        which(axis).map(|coord| coord.to_f64())
    }

    /// A two-master width-axis font. The design locations are deliberately not
    /// the user-space percentages, so a raw class leaking through would show up.
    const WIDTH_CLASS_FONT: &str = r#"{
.formatVersion = 3;
.appVersion = "3243";
familyName = WidthClassTest;
date = "2024-05-08 05:56:55 +0000";
unitsPerEm = 1000;
versionMajor = 1;
versionMinor = 0;
axes = (
{
name = Width;
tag = wdth;
}
);
metrics = ();
fontMaster = (
{
id = m01;
name = Condensed;
axesValues = (0);
metricValues = ();
},
{
id = m02;
name = Regular;
axesValues = (1000);
metricValues = ();
}
);
glyphs = ();
instances = (
{
name = Condensed;
axesValues = (0);
widthClass = 3;
},
{
name = SemiCondensed;
axesValues = (500);
widthClass = 4;
},
{
name = Regular;
axesValues = (1000);
widthClass = 5;
}
);
}
"#;

    #[test]
    fn width_class_is_a_percentage_not_a_raw_class() {
        // An OS/2 width class of 4 means "semi-condensed", i.e. 87.5% of the
        // normal width — the user-space value 4 is wrong (as is 3, "condensed").
        let font = load_test_font(WIDTH_CLASS_FONT);
        let axis = &font.axes[0];
        assert_eq!(axis.tag, Tag::new(b"wdth"));
        assert_eq!(
            axis_map(axis),
            vec![(75.0, 0.0), (87.5, 500.0), (100.0, 1000.0)]
        );
        assert_eq!(user(axis, |a| a.min), Some(75.0));
        assert_eq!(user(axis, |a| a.max), Some(100.0));
        // Default comes from the "Regular" master, inverted through the mapping.
        assert_eq!(user(axis, |a| a.default), Some(100.0));
    }

    #[test]
    fn instance_axis_location_overrides_the_width_class() {
        // The "Axis Location" custom parameter is already in user space, so it
        // wins over the width class (and, before this was fixed, it was read
        // from the wrong place and ignored entirely).
        let font = load_test_font(&WIDTH_CLASS_FONT.replace(
            r#"name = Condensed;
axesValues = (0);
widthClass = 3;"#,
            r#"name = Condensed;
axesValues = (0);
widthClass = 3;
customParameters = (
{
name = "Axis Location";
value = (
{
Axis = Width;
Location = 80;
}
);
}
);"#,
        ));
        let axis = &font.axes[0];
        assert_eq!(
            axis_map(axis),
            vec![(80.0, 0.0), (87.5, 500.0), (100.0, 1000.0)]
        );
        assert_eq!(user(axis, |a| a.min), Some(80.0));
    }

    #[test]
    fn axis_location_on_masters_builds_the_mapping_from_masters() {
        // When every master has an "Axis Location", that is the mapping; the
        // instances' weight classes must not contribute extra points.
        let font = load_test_font(
            r#"{
.formatVersion = 3;
.appVersion = "3243";
familyName = MasterAxisLocationTest;
date = "2024-05-08 05:56:55 +0000";
unitsPerEm = 1000;
versionMajor = 1;
versionMinor = 0;
axes = (
{
name = Weight;
tag = wght;
}
);
metrics = ();
fontMaster = (
{
id = m01;
name = Thin;
axesValues = (1);
customParameters = (
{
name = "Axis Location";
value = (
{
Axis = Weight;
Location = 100;
}
);
}
);
metricValues = ();
},
{
id = m02;
name = Bold;
axesValues = (199);
customParameters = (
{
name = "Axis Location";
value = (
{
Axis = Weight;
Location = 700;
}
);
}
);
metricValues = ();
}
);
glyphs = ();
instances = (
{
name = Thin;
axesValues = (1);
weightClass = 100;
},
{
name = Regular;
axesValues = (100);
weightClass = 400;
},
{
name = Bold;
axesValues = (199);
weightClass = 700;
}
);
}
"#,
        );
        let axis = &font.axes[0];
        assert_eq!(axis_map(axis), vec![(100.0, 1.0), (700.0, 199.0)]);
        assert_eq!(user(axis, |a| a.min), Some(100.0));
        assert_eq!(user(axis, |a| a.max), Some(700.0));
        assert_eq!(user(axis, |a| a.default), Some(100.0));
    }

    #[test]
    fn explicit_axis_mappings_take_precedence_over_instances() {
        let font = load_test_font(
            r#"{
.formatVersion = 3;
.appVersion = "3243";
familyName = ExplicitMappingTest;
date = "2024-05-08 05:56:55 +0000";
unitsPerEm = 1000;
versionMajor = 1;
versionMinor = 0;
axes = (
{
name = Weight;
tag = wght;
}
);
customParameters = (
{
name = "Axis Mappings";
value = {
wght = {
300 = 1;
900 = 199;
};
};
}
);
metrics = ();
fontMaster = (
{
id = m01;
name = Light;
axesValues = (1);
metricValues = ();
},
{
id = m02;
name = Black;
axesValues = (199);
metricValues = ();
}
);
glyphs = ();
instances = (
{
name = Regular;
axesValues = (100);
weightClass = 400;
}
);
}
"#,
        );
        let axis = &font.axes[0];
        // The instance's weightClass would map 400 -> 100 if it were consulted.
        assert_eq!(axis_map(axis), vec![(300.0, 1.0), (900.0, 199.0)]);
        assert_eq!(user(axis, |a| a.min), Some(300.0));
        assert_eq!(user(axis, |a| a.max), Some(900.0));
    }

    #[test]
    fn identity_mapping_is_elided() {
        let font = load_test_font(
            r#"{
.formatVersion = 3;
.appVersion = "3243";
familyName = IdentityTest;
date = "2024-05-08 05:56:55 +0000";
unitsPerEm = 1000;
versionMajor = 1;
versionMinor = 0;
axes = (
{
name = Weight;
tag = wght;
}
);
metrics = ();
fontMaster = (
{
id = m01;
name = Regular;
axesValues = (400);
metricValues = ();
}
);
glyphs = ();
instances = (
{
name = Regular;
axesValues = (400);
weightClass = 400;
}
);
}
"#,
        );
        let axis = &font.axes[0];
        assert!(axis.map.is_none(), "an identity mapping must be elided");
        assert_eq!(user(axis, |a| a.min), Some(400.0));
        assert_eq!(user(axis, |a| a.max), Some(400.0));
        assert_eq!(user(axis, |a| a.default), Some(400.0));
    }

    #[test]
    fn instance_without_a_weight_class_defaults_to_400() {
        // An instance with no `weightClass` is Glyphs' "Regular", i.e. 400 in
        // user space — not its design location. (This replaces the old
        // name-matching heuristic.)
        let font = load_test_font(
            r#"{
.formatVersion = 3;
.appVersion = "3243";
familyName = DefaultWeightTest;
date = "2024-05-08 05:56:55 +0000";
unitsPerEm = 1000;
versionMajor = 1;
versionMinor = 0;
axes = (
{
name = Weight;
tag = wght;
}
);
metrics = ();
fontMaster = (
{
id = m01;
name = Regular;
axesValues = (94);
metricValues = ();
},
{
id = m02;
name = Bold;
axesValues = (152);
metricValues = ();
}
);
glyphs = ();
instances = (
{
name = Regular;
axesValues = (94);
},
{
name = Bold;
axesValues = (152);
weightClass = 700;
}
);
}
"#,
        );
        let axis = &font.axes[0];
        assert_eq!(axis_map(axis), vec![(400.0, 94.0), (700.0, 152.0)]);
        assert_eq!(user(axis, |a| a.min), Some(400.0));
        assert_eq!(user(axis, |a| a.max), Some(700.0));
        assert_eq!(user(axis, |a| a.default), Some(400.0));
    }
}
