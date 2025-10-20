use crate::{
    common::{FormatSpecific, OTValue},
    i18ndictionary::I18NDictionary,
    names::Names,
    Axis, BabelfontError, Font, GlyphList, Master,
};
use chrono::Local;
use fontdrasil::coords::{DesignCoord, DesignLocation, UserCoord};
use glyphslib::glyphs3;
use smol_str::SmolStr;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    str::FromStr,
};
use write_fonts::types::Tag;

pub(crate) type UserData = BTreeMap<SmolStr, glyphslib::Plist>;

const KEY_PREFIX: &str = "com.schriftgestalt.Glyphs.";

pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    log::debug!("Reading to string");
    let s = fs::read_to_string(&path).map_err(|source| BabelfontError::IO {
        path: path.clone(),
        source,
    })?;
    load_str(&s, path.clone())
}

pub fn load_str(s: &str, path: PathBuf) -> Result<Font, BabelfontError> {
    let mut font = Font::new();
    let glyphs_font =
        glyphslib::Font::load_str(s).map_err(|source| BabelfontError::PlistParse {
            source,
            path: path.clone(),
        })?;
    let glyphs_font = glyphs_font
        .as_glyphs3()
        .ok_or_else(|| BabelfontError::WrongConvertor { path })?;
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

    // Copy masters
    font.masters = glyphs_font
        .masters
        .iter()
        .map(|master| load_master(master, glyphs_font, &font))
        .collect();
    // Copy glyphs
    font.glyphs = GlyphList(glyphs_font.glyphs.iter().map(Into::into).collect());

    // Copy instances
    font.instances = glyphs_font
        .instances
        .iter()
        .map(|i| load_instance(&font, i))
        .collect();
    // Copy metadata
    load_metadata(&mut font, glyphs_font);
    // Copy kern groups
    for glyph in font.glyphs.iter() {
        let left_group = glyph.formatspecific.get_string("kern_left");
        font.second_kern_groups
            .entry(left_group)
            .or_default()
            .push(glyph.name.clone());

        let right_group = glyph.formatspecific.get_string("kern_right");
        font.first_kern_groups
            .entry(right_group)
            .or_default()
            .push(glyph.name.clone());
    }
    // Interpret metrics
    // Interpret axes
    interpret_axes(&mut font);

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
    if let Some(weight_class) = instance.weight_class.as_ref().and_then(|x| x.as_i64()) {
        format_specific.insert("weightClass".into(), weight_class.into());
    }
    if let Some(width_class) = instance.width_class.as_ref().and_then(|x| x.as_i64()) {
        format_specific.insert("widthClass".into(), width_class.into());
    }
    if !instance.exports {
        format_specific.insert("exports".into(), serde_json::Value::Bool(false));
    }
    crate::Instance {
        name: I18NDictionary::from(&instance.name),
        location: designspace_to_location(&instance.axes_values),
        custom_names: Names::new(), // TODO instance.custom_names.clone().into(),
        variable: instance.export_type == glyphslib::glyphs3::ExportType::Variable,
        format_specific,
    }
}

fn load_metadata(font: &mut Font, glyphs_font: &glyphs3::Glyphs3) {
    font.names.family_name = glyphs_font.family_name.clone().into();
    font.upm = glyphs_font.units_per_em as u16;
    font.version = (
        glyphs_font.version.major as u16,
        glyphs_font.version.minor as u16,
    );
    for property in glyphs_font.properties.iter() {
        match property {
            glyphs3::Property::SingularProperty { key, value } => {
                match key {
                    glyphs3::SingularPropertyKey::Designer => {
                        font.names.designer = I18NDictionary::from(value)
                    }
                    glyphs3::SingularPropertyKey::Manufacturer => {
                        font.names.manufacturer = I18NDictionary::from(value)
                    }
                    glyphs3::SingularPropertyKey::DesignerUrl => {
                        font.names.designer_url = I18NDictionary::from(value)
                    }
                    glyphs3::SingularPropertyKey::ManufacturerUrl => {
                        font.names.manufacturer_url = I18NDictionary::from(value);
                    }
                    glyphs3::SingularPropertyKey::LicenseUrl => {
                        font.names.license_url = I18NDictionary::from(value)
                    }
                    glyphs3::SingularPropertyKey::PostscriptFullName => {
                        font.names.postscript_name = I18NDictionary::from(value)
                    }
                    glyphs3::SingularPropertyKey::PostscriptFontName => {
                        //     font.names.postscript_font_name = I18NDictionary::from(value)
                    }
                    glyphs3::SingularPropertyKey::WwsFamilyName => {
                        font.names.wws_family_name = I18NDictionary::from(value)
                    }
                    glyphs3::SingularPropertyKey::VersionString => {
                        font.names.version = I18NDictionary::from(value)
                    }
                    glyphs3::SingularPropertyKey::VendorID => font.custom_ot_values.push(OTValue {
                        table: "OS/2".into(),
                        field: "achVendID".into(),
                        value: crate::OTScalar::StringType(value.clone()),
                    }),
                    glyphs3::SingularPropertyKey::UniqueID => {
                        font.names.unique_id = I18NDictionary::from(value)
                    }
                }
            }
            glyphs3::Property::LocalizedProperty { key, values } => {
                let value = I18NDictionary::new();
                for _localized_value in values.iter() {
                    // value.insert(
                    //     localized_value.language.clone(),
                    //     localized_value.value.clone(),
                    // );
                }
                match key {
                    glyphs3::LocalizedPropertyKey::FamilyNames => font.names.family_name = value,
                    glyphs3::LocalizedPropertyKey::Copyrights => font.names.copyright = value,
                    glyphs3::LocalizedPropertyKey::Designers => font.names.designer = value,
                    glyphs3::LocalizedPropertyKey::Manufacturers => font.names.manufacturer = value,
                    glyphs3::LocalizedPropertyKey::Licenses => font.names.license = value,
                    glyphs3::LocalizedPropertyKey::Trademarks => font.names.trademark = value,
                    glyphs3::LocalizedPropertyKey::Descriptions => font.names.description = value,
                    glyphs3::LocalizedPropertyKey::SampleTexts => font.names.sample_text = value,
                    glyphs3::LocalizedPropertyKey::CompatibleFullNames => {
                        font.names.compatible_full_name = value
                    }
                    glyphs3::LocalizedPropertyKey::StyleNames => {
                        font.names.typographic_subfamily = value;
                    }
                }
            }
            glyphs3::Property::Junk(_plist) => unreachable!(),
        }

        font.note = Some(glyphs_font.note.clone());
        font.date = glyphs_font.date.parse().unwrap_or_else(|_| Local::now());

        // Copy custom parameters
        for cp in glyphs_font.custom_parameters.iter() {
            if let Ok(value) = serde_json::to_value(&cp.value) {
                font.format_specific.insert(cp.name.clone(), value);
            }
        }
        if !glyphs_font.user_data.is_empty() {
            font.format_specific.insert(
                "userData".into(),
                serde_json::to_value(&glyphs_font.user_data).unwrap_or(serde_json::Value::Null),
            );
        }
        if !glyphs_font.stems.is_empty() {
            font.format_specific.insert(
                "stems".into(),
                serde_json::to_value(&glyphs_font.stems).unwrap_or(serde_json::Value::Null),
            );
        }
    }
}

fn load_master(master: &glyphs3::Master, glyphs_font: &glyphs3::Glyphs3, font: &Font) -> Master {
    let designspace_to_location = |numbers: &[f64]| -> DesignLocation {
        numbers
            .iter()
            .zip(font.axes.iter())
            .map(|(number, axis)| (axis.tag, DesignCoord::new(*number)))
            .collect()
    };
    let f64_axes: Vec<f64> = master.axes_values.iter().map(|x| *x as f64).collect();
    let mut m = Master {
        name: master.name.clone().into(),
        id: master.id.clone(),
        location: designspace_to_location(&f64_axes),
        guides: master.guides.iter().map(Into::into).collect(),
        metrics: HashMap::new(),
        kerning: HashMap::new(),
        custom_ot_values: vec![],
    };
    m.kerning = glyphs_font
        .kerning
        .get(&m.id)
        .map(|kerndict| {
            let mut kerns = HashMap::new();
            for (first, items) in kerndict {
                for (second, kern) in items {
                    kerns.insert((first.clone(), second.clone()), *kern as i16);
                }
            }
            kerns
        })
        .unwrap_or_default();
    m
}

fn interpret_axes(font: &mut Font) {
    // This is going to look very wrong, but after much trial and error I can confirm
    // it works. First: load the axes assuming that userspace=designspace. Then
    // work out the axis mappings. Then apply the mappings to the axis locations.

    let origin: &Master = font.masters.first().unwrap_or_else(|| {
        font.format_specific
            .get("Variable Font Origin")
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
    if let Some(mappings) = font
        .format_specific
        .get("Axis Mappings")
        .and_then(|x| x.as_object())
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
        let c = instance
            .format_specific
            .get("Axis Location")
            .and_then(|x| x.as_array());
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
                .get("weightClass")
                .and_then(|x| x.as_f64())
            {
                c_pairs.push(("Weight", weightclass));
            }
            if let Some(widthclass) = instance
                .format_specific
                .get("widthClass")
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
    let axes = font
        .axes
        .iter()
        .map(|ax| glyphs3::Axis {
            hidden: false,
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

    let glyphs_font = glyphs3::Glyphs3 {
        format_version: 3,
        family_name: font
            .names
            .family_name
            .get_default()
            .map(|x| x.to_string())
            .unwrap_or_default(),
        axes,
        metrics: our_metrics.iter().map(Into::into).collect(),
        masters: font
            .masters
            .iter()
            .map(|x| save_master(x, &font.axes, &our_metrics))
            .collect(),
        glyphs: font.glyphs.iter().map(Into::into).collect(),
        instances: font
            .instances
            .iter()
            .map(|x| save_instance(x, &font.axes))
            .collect(),
        date: font.date.format("%Y-%m-%d %H:%M:%S +0000").to_string(),
        keep_alternates_together: false,
        units_per_em: font.upm.into(),
        version: glyphslib::common::Version {
            major: font.version.0.into(),
            minor: font.version.1.into(),
        },
        user_data: font
            .format_specific
            .get("userData")
            .and_then(|x| serde_json::from_value::<UserData>(x.clone()).ok())
            .unwrap_or_default(),
        stems: font
            .format_specific
            .get("stems")
            .and_then(|x| serde_json::from_value::<Vec<glyphs3::Stem>>(x.clone()).ok())
            .unwrap_or_default(),
        ..Default::default() // Stuff we should probably get to one day
    };
    // Save kerning
    // Save custom parameters
    // Save metadata
    // Save features
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
        ..Default::default()
    }
}

fn save_instance(instance: &crate::Instance, axes: &[Axis]) -> glyphs3::Instance {
    let mut axes_values = vec![];
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
    let formatspecific = &instance.format_specific;
    glyphs3::Instance {
        name: instance
            .name
            .get_default()
            .map(|x| x.to_string())
            .unwrap_or_default(),
        axes_values,
        weight_class: formatspecific
            .get("weightClass")
            .and_then(|x| x.as_i64())
            .map(glyphslib::Plist::Integer),
        width_class: formatspecific
            .get("widthClass")
            .and_then(|x| x.as_i64())
            .map(glyphslib::Plist::Integer),
        exports: formatspecific
            .get("exports")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        ..Default::default()
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
        if let Shape::ComponentShape(p) = shape {
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
        let there = load("resources/RadioCanadaDisplay.glyphs".into()).unwrap();
        let backagain = glyphslib::Font::Glyphs3(as_glyphs3(&there));
        let orig = glyphslib::Font::load_str(
            &fs::read_to_string("resources/RadioCanadaDisplay.glyphs").unwrap(),
        )
        .unwrap();
        let old_string = orig.to_string().unwrap();
        let new_string = backagain.to_string().unwrap();
        let diff = TextDiff::from_lines(&old_string, &new_string);
        let text_diff = diff.unified_diff().to_string();
        println!("Diff between original and roundtrip:\n{}", text_diff);
        // for change in diff.iter_all_changes() {
        //     let sign = match change.tag() {
        //         ChangeTag::Delete => "-",
        //         ChangeTag::Insert => "+",
        //         ChangeTag::Equal => " ",
        //     };
        //     print!("{}{}", sign, change);
        // }
        if diff.ratio() < 1.0 {
            panic!("Roundtrip produced different output");
        }
    }

    #[test]
    fn test_load_open_shape() {
        let font = load("resources/GlyphsFileFormatv3.glyphs".into()).unwrap();
        let shape = &font.glyphs.get("A").unwrap().layers[1].shapes[0];
        match shape {
            Shape::PathShape(p) => assert!(!p.closed),
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
}
