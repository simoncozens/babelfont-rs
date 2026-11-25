use crate::{
    convertors::ufo::{as_norad, load_kerning, stash_lib, KEY_LIB},
    glyph::GlyphList,
    names::Names,
    I18NDictionary, Instance, Layer, Tag,
};
use fontdrasil::coords::{DesignCoord, DesignLocation, UserCoord};
use norad::{
    designspace::{
        AxisMappings, AxisSubset, DesignSpaceDocument, Instance as DSInstance, RuleProcessing,
        Rules, Source, SubsetValue, VariableFont,
    },
    Plist,
};
use std::collections::{BTreeMap, HashMap};
// use rayon::prelude::*;
use std::path::PathBuf;

use uuid::Uuid;

/// Key to store the designspace format version in the font's format_specific
pub const FORMAT_KEY: &str = "norad.designspace.format";
/// Key to store the master filename in the instance's format_specific
pub const FILENAME_KEY: &str = "norad.designspace.filename";
/// Key to store the master style name in the master's format_specific
pub const STYLENAME_KEY: &str = "norad.designspace.style";
/// Key to store a variable font's axis subsets in the instance's format_specific
///
/// The values are in user space coordinates, as they are in the designspace file.
pub const AXIS_SUBSETS_KEY: &str = "norad.designspace.axisSubsets";
/// Key to store the elided fallback name in the font's format_specific
pub const ELIDED_FALLBACK_NAME_KEY: &str = "norad.designspace.elidedFallbackName";

use crate::{
    convertors::ufo::{load_master_info, norad_glyph_to_babelfont_layer},
    Axis, BabelfontError, Font, Master,
};

/// Load a DesignSpace document and all referenced UFOs into a Babelfont Font
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let ds: DesignSpaceDocument = norad::designspace::DesignSpaceDocument::load(path.clone())?;
    let relative = path.parent();
    let axes: Vec<Axis> = ds
        .axes
        .iter()
        .filter_map(|dsax| dsax.try_into().ok())
        .collect();
    #[allow(clippy::unwrap_used)] // We put a default there
    let axis_name_tag_map: HashMap<String, crate::Tag> = axes
        .iter()
        .map(|ax| (ax.name.get_default().unwrap().clone(), ax.tag))
        .collect();
    let default_master = default_master(&ds, &axes).ok_or(BabelfontError::NoDefaultMaster)?;
    let relative_path_to_default_master = if let Some(r) = relative {
        r.join(default_master.filename.clone())
    } else {
        default_master.filename.clone().into()
    };
    let mut font = crate::convertors::ufo::load(relative_path_to_default_master)?;
    font.axes = axes;

    load_instances(&mut font, &axis_name_tag_map, &ds.instances);
    load_variable_fonts(&mut font, &ds.variable_fonts);
    let res: Vec<(Master, Vec<Vec<Layer>>)> = ds
        .sources
        .iter()
        .filter_map(|source| load_master(&font.glyphs, source, relative, &axis_name_tag_map).ok())
        .collect();
    // Drop the default master loaded from the UFO above
    font.masters.clear();
    // Clear all layers
    for g in font.glyphs.iter_mut() {
        g.layers.clear();
    }
    for (master, mut layerset) in res {
        font.masters.push(master);
        for (g, l) in font.glyphs.iter_mut().zip(layerset.iter_mut()) {
            g.layers.append(l);
        }
    }
    // Stash DS format
    #[allow(clippy::unwrap_used)] // It's a number
    font.format_specific.insert(
        FORMAT_KEY.to_string(),
        serde_json::Value::Number(serde_json::Number::from_f64(ds.format.into()).unwrap()),
    );
    if let Some(elided_fallback_name) = &ds.elided_fallback_name {
        font.format_specific.insert(
            ELIDED_FALLBACK_NAME_KEY.to_string(),
            elided_fallback_name.as_str().into(),
        );
    }
    Ok(font)
}

pub(crate) fn load_instances(
    font: &mut Font,
    axis_name_tag_map: &HashMap<String, Tag>,
    instances: &[DSInstance],
) {
    for instance in instances {
        let mut custom_names = Names::new();
        if let Some(familyname) = &instance.familyname {
            custom_names.family_name = familyname.into();
        }
        if let Some(stylename) = &instance.stylename {
            custom_names.preferred_subfamily_name = stylename.into(); // ?
        }
        if let Some(psname) = &instance.postscriptfontname {
            custom_names.postscript_name = psname.into();
        }
        if let Some(stylemapfamilyname) = &instance.stylemapfamilyname {
            custom_names.typographic_family = stylemapfamilyname.into(); // ???
        }
        if let Some(stylemapstylename) = &instance.stylemapstylename {
            custom_names.typographic_subfamily = stylemapstylename.into(); // ???
        }
        let mut inst = Instance {
            id: instance
                .name
                .as_ref()
                .unwrap_or(&"Unnamed instance".to_string())
                .clone(),
            name: instance
                .name
                .as_ref()
                .unwrap_or(&"Unnamed instance".to_string())
                .into(),
            location: DesignLocation::from(
                instance
                    .location
                    .iter()
                    .map(|dimension| {
                        (
                            *axis_name_tag_map
                                .get(&dimension.name)
                                .unwrap_or(&Tag::new(b"unkn")),
                            DesignCoord::new(dimension.xvalue.unwrap_or_default()),
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
            custom_names,
            variable: false,
            format_specific: stash_lib(Some(&instance.lib)),
            linked_style: None,
            // We should also stash the file name
        };
        if let Some(filename) = &instance.filename {
            inst.format_specific.insert(
                FILENAME_KEY.to_string(),
                serde_json::Value::String(filename.into()),
            );
        }
        font.instances.push(inst);
    }
}

/// Load the variable fonts described by a designspace document as variable instances
pub(crate) fn load_variable_fonts(font: &mut Font, variable_fonts: &[VariableFont]) {
    for vf in variable_fonts {
        let location = DesignLocation::from(
            vf.axis_subsets
                .iter()
                .filter_map(|subset| {
                    let axis = font
                        .axes
                        .iter()
                        .find(|ax| ax.name.get_default() == Some(&subset.name))?;
                    let user_value = match subset.value {
                        SubsetValue::Discrete(value) => value,
                        SubsetValue::Range { default, .. } => default?,
                    };
                    // Axis subset values are in user coordinates.
                    let design = axis
                        .userspace_to_designspace(UserCoord::new(user_value))
                        .unwrap_or_else(|_| DesignCoord::new(user_value));
                    Some((axis.tag, design))
                })
                .collect::<Vec<_>>(),
        );
        let mut format_specific = stash_lib(Some(&vf.lib));
        format_specific.insert(
            AXIS_SUBSETS_KEY.to_string(),
            serde_json::to_value(&vf.axis_subsets).unwrap_or_default(),
        );
        if let Some(filename) = &vf.filename {
            format_specific.insert(
                FILENAME_KEY.to_string(),
                serde_json::Value::String(filename.clone()),
            );
        }
        font.instances.push(Instance {
            id: vf.name.clone(),
            name: vf.name.as_str().into(),
            location,
            custom_names: Names::new(),
            variable: true,
            linked_style: None,
            format_specific,
        });
    }
}

fn load_master(
    glyphs: &GlyphList,
    source: &Source,
    relative: Option<&std::path::Path>,
    axis_name_tag_map: &HashMap<String, Tag>,
) -> Result<(Master, Vec<Vec<Layer>>), BabelfontError> {
    let location = DesignLocation::from(
        source
            .location
            .iter()
            .map(|dimension| {
                // XXX We may have uservalues in DS5 sources
                (
                    *axis_name_tag_map
                        .get(dimension.name.as_str())
                        .unwrap_or_else(|| panic!("Axis name not found: {}", dimension.name)),
                    DesignCoord::new(dimension.xvalue.unwrap_or_default()),
                )
            })
            .collect::<Vec<_>>(),
    );
    let required_layer = &source.layer;
    let uuid = Uuid::new_v4().to_string();

    let mut master = Master::new(
        source.name.as_ref().unwrap_or(&source.filename),
        uuid,
        location,
    );
    // Stash master style name
    if let Some(stylename) = &source.stylename {
        master.format_specific.insert(
            STYLENAME_KEY.to_string(),
            serde_json::Value::String(stylename.into()),
        );
    }
    master.format_specific.insert(
        FILENAME_KEY.to_string(),
        serde_json::Value::String(source.filename.clone()),
    );

    let relative_path_to_master = if let Some(r) = relative {
        r.join(source.filename.clone())
    } else {
        source.filename.clone().into()
    };

    let source_font = norad::Font::load(relative_path_to_master)?;
    let info = &source_font.font_info;
    load_master_info(&mut master, info);
    load_kerning(&mut master, &source_font.kerning);
    let ufo_layer = if let Some(source_layer) = required_layer {
        source_font
            .layers
            .get(source_layer)
            .ok_or_else(|| BabelfontError::MasterNotFound(source_layer.to_string()))?
    } else {
        source_font.default_layer()
    };

    let mut bf_layer_list = vec![];
    for g in glyphs.iter() {
        let mut glyph_layer_list = vec![];
        if let Some(norad_glyph) = ufo_layer.get_glyph(g.name.as_str()) {
            let mut our_layer = norad_glyph_to_babelfont_layer(norad_glyph, ufo_layer, &master);
            // Even if this is non-default in the UFO, it is the default layer for this master,
            // because we have promoted sparse masters to their own babelfont master.
            our_layer.master = crate::LayerType::DefaultForMaster(master.id.to_string());
            glyph_layer_list.push(our_layer);
            if required_layer.is_none() {
                // i.e. the default layer
                // Check for a background layer for this glyph. Load it as a separate layer, and tie it to the layer we just pushed
                // Unwrap is safe because we're going to unwrap something we just pushed to the array
                #[allow(clippy::unwrap_used)]
                if let Some(background_glyph) = source_font
                    .layers
                    .get("public.background")
                    .and_then(|l| l.get_glyph(g.name.as_str()))
                {
                    let mut background_layer =
                        norad_glyph_to_babelfont_layer(background_glyph, ufo_layer, &master);
                    background_layer.master = crate::LayerType::FreeFloating;
                    background_layer.id = Some(Uuid::new_v4().to_string());
                    background_layer.is_background = true;
                    glyph_layer_list.last_mut().unwrap().background_layer_id =
                        background_layer.id.clone();
                    glyph_layer_list.push(background_layer);
                }
            }
        }
        bf_layer_list.push(glyph_layer_list)
    }
    Ok((master, bf_layer_list))
}

fn default_master<'a>(ds: &'a DesignSpaceDocument, axes: &[Axis]) -> Option<&'a Source> {
    #[allow(clippy::unwrap_used)]
    // We know the axes are well defined because we constructed them from the DS
    let defaults: BTreeMap<&String, DesignCoord> = axes
        .iter()
        .map(|ax| {
            (
                ax.name.get_default().unwrap(),
                ax.default
                    .map(|x| ax.userspace_to_designspace(x).unwrap())
                    .unwrap_or_default(),
            )
        })
        .collect();
    for source in ds.sources.iter() {
        let mut maybe = true;
        for loc in source.location.iter() {
            if defaults.get(&loc.name) != loc.xvalue.map(DesignCoord::new).as_ref() {
                maybe = false;
                break;
            }
        }
        if maybe {
            return Some(source);
        }
    }
    None
}

/// Save a Babelfont Font as a DesignSpace document and referenced UFOs
///
/// This is incomplete and may not save all data.
pub fn save_designspace(font: &Font, path: &PathBuf) -> Result<(), BabelfontError> {
    let axis_tag_name_map: HashMap<Tag, String> = font
        .axes
        .iter()
        .map(|ax| {
            (
                ax.tag,
                ax.name
                    .get_default()
                    .unwrap_or(&"Unnamed axis".to_string())
                    .to_string(),
            )
        })
        .collect();
    let master_filenames: Vec<String> = font
        .masters
        .iter()
        .map(|m| {
            m.format_specific
                .get(FILENAME_KEY)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or(format!(
                    "{}-{}.ufo",
                    font.names
                        .family_name
                        .get_default()
                        .unwrap_or(&"Unnamed font".to_string())
                        .replace(" ", ""),
                    m.name
                        .get_default()
                        .map(|x| x.as_str())
                        .unwrap_or(m.id.as_str())
                        .replace(" ", "")
                ))
        })
        .collect();
    let axis_order = font
        .axes
        .iter()
        .flat_map(|ax| ax.name.get_default())
        .map(|x| x.as_str())
        .collect::<Vec<_>>();
    let cross_axis_mappings: Vec<_> = font
        .cross_axis_mappings
        .iter()
        .map(|m| crate::axis::ufo::make_cross_axis_mapping_entry(m, &axis_tag_name_map))
        .collect();
    let (variable_instances, static_instances): (Vec<&Instance>, Vec<&Instance>) = font
        .instances
        .iter()
        .partition(|instance| instance.variable);
    // Variable fonts and cross-axis mappings both require designspace format 5.
    let mut format = font
        .format_specific
        .get(FORMAT_KEY)
        .and_then(|v| v.as_f64())
        .unwrap_or(5.0) as f32;
    if !variable_instances.is_empty() || !cross_axis_mappings.is_empty() {
        format = format.max(5.0);
    }
    let ds = DesignSpaceDocument {
        format,
        axes: font.axes.iter().map(|x| x.into()).collect(),
        rules: Rules {
            processing: RuleProcessing::First,
            rules: vec![],
        },
        sources: font
            .masters
            .iter()
            .zip(master_filenames.iter())
            .map(|(m, f)| to_source(font, m, f, &axis_tag_name_map, &axis_order))
            .collect(),
        instances: static_instances
            .iter()
            .map(|i| to_norad_instance(i, &axis_tag_name_map, &axis_order))
            .collect(),
        lib: Plist::new(),
        axis_mappings: if cross_axis_mappings.is_empty() {
            None
        } else {
            Some(AxisMappings {
                description: None,
                mappings: cross_axis_mappings,
            })
        },
        elided_fallback_name: font
            .format_specific
            .get(ELIDED_FALLBACK_NAME_KEY)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        location_labels: vec![],
        variable_fonts: variable_instances
            .iter()
            .map(|i| to_norad_variable_font(i, &font.axes))
            .collect(),
    };
    // Now save all the UFOs
    for (ix, master_filename) in master_filenames.iter().enumerate() {
        let relative_path = path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(master_filename);
        let ufo = as_norad(font, ix)?;
        ufo.save(&relative_path)?;
    }
    Ok(ds.save(path)?)
}

pub(crate) fn to_norad_instance(
    instance: &Instance,
    axis_tag_name_map: &HashMap<Tag, String>,
    axis_order: &[&str],
) -> norad::designspace::Instance {
    let name_to_option_string = |x: &I18NDictionary| x.get_default().map(|y| y.to_string());
    let mut location: Vec<_> =
        crate::axis::ufo::location_to_dimensions(&instance.location, axis_tag_name_map);
    // Sort them based on the font's axis order
    location.sort_by(|a, b| {
        axis_order
            .iter()
            .position(|x| x == &a.name)
            .unwrap_or(usize::MAX)
            .cmp(
                &axis_order
                    .iter()
                    .position(|x| x == &b.name)
                    .unwrap_or(usize::MAX),
            )
    });
    norad::designspace::Instance {
        familyname: name_to_option_string(&instance.custom_names.family_name),
        stylename: name_to_option_string(&instance.custom_names.preferred_subfamily_name),
        name: name_to_option_string(&instance.name),
        filename: instance
            .format_specific
            .get(FILENAME_KEY)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        postscriptfontname: name_to_option_string(&instance.custom_names.postscript_name),
        stylemapfamilyname: name_to_option_string(&instance.custom_names.typographic_family),
        stylemapstylename: name_to_option_string(&instance.custom_names.typographic_subfamily),
        location,
        lib: serde_json::from_value(
            instance
                .format_specific
                .get(KEY_LIB)
                .cloned()
                .unwrap_or_default(),
        )
        .ok()
        .unwrap_or_default(),
        location_label: None,
        localised_style_names: (&instance.custom_names.preferred_subfamily_name).into(),
        localised_family_names: (&instance.custom_names.family_name).into(),
        localised_style_map_style_names: (&instance.custom_names.typographic_subfamily).into(),
        localised_style_map_family_names: (&instance.custom_names.typographic_family).into(),
    }
}

/// Convert a variable instance to a designspace variable font
pub(crate) fn to_norad_variable_font(instance: &Instance, axes: &[Axis]) -> VariableFont {
    let axis_subsets = instance
        .format_specific
        .get(AXIS_SUBSETS_KEY)
        .and_then(|value| serde_json::from_value::<Vec<AxisSubset>>(value.clone()).ok())
        .unwrap_or_else(|| {
            // No stored subsets (the instance didn't come from a designspace);
            // any axis named in the location is pinned to that value, and all
            // other axes are included in full.
            axes.iter()
                .map(|axis| {
                    let value = instance
                        .location
                        .get(axis.tag)
                        .and_then(|coord| axis.designspace_to_userspace(coord).ok())
                        .map(|user| SubsetValue::Discrete(user.to_f64()))
                        .unwrap_or_default();
                    AxisSubset {
                        name: axis
                            .name
                            .get_default()
                            .map(|x| x.to_string())
                            .unwrap_or_else(|| axis.tag.to_string()),
                        value,
                    }
                })
                .collect()
        });
    VariableFont {
        name: instance
            .name
            .get_default()
            .map(|x| x.to_string())
            .unwrap_or_default(),
        filename: instance
            .format_specific
            .get(FILENAME_KEY)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        axis_subsets,
        lib: serde_json::from_value(
            instance
                .format_specific
                .get(KEY_LIB)
                .cloned()
                .unwrap_or_default(),
        )
        .ok()
        .unwrap_or_default(),
    }
}

fn to_source(
    font: &Font,
    master: &Master,
    filename: &str,
    axis_tag_name_map: &HashMap<Tag, String>,
    axis_order: &[&str],
) -> norad::designspace::Source {
    let mut location: Vec<_> = master
        .location
        .iter()
        .map(|(tag, coord)| norad::designspace::Dimension {
            name: axis_tag_name_map
                .get(tag)
                .cloned()
                .unwrap_or_else(|| tag.to_string()),
            xvalue: Some(coord.to_f64()),
            uservalue: None,
            yvalue: None,
        })
        .collect();
    // Sort them based on the font's axis order
    location.sort_by(|a, b| {
        axis_order
            .iter()
            .position(|x| x == &a.name)
            .unwrap_or(usize::MAX)
            .cmp(
                &axis_order
                    .iter()
                    .position(|x| x == &b.name)
                    .unwrap_or(usize::MAX),
            )
    });

    norad::designspace::Source {
        familyname: font.names.family_name.get_default().map(|x| x.to_string()),
        stylename: master
            .format_specific
            .get(STYLENAME_KEY)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        name: master.name.get_default().map(|x| x.to_string()),
        // We know we have one
        filename: filename.to_string(),
        location,
        layer: None, // XXX
        localised_family_names: (&font.names.family_name).into(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::PathBuf;

    use crate::filters::{FontFilter, RetainGlyphs};

    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    fn test_roundtrip(#[files("resources/*.designspace")] path: PathBuf) {
        let there = crate::load(&path).unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let temp_path = tempdir.path().join("temp.designspace");
        save_designspace(&there, &temp_path).unwrap();
        println!("Saved to temp path: {:?}", temp_path);

        // Perform a norad load on both designspace files and check for equivalence
        let relative = path.parent();
        let ds1 = norad::designspace::DesignSpaceDocument::load(&path).unwrap();
        let ds2 = norad::designspace::DesignSpaceDocument::load(&temp_path).unwrap();
        assert_eq!(ds1, ds2);
        let index_of_default = there.default_master_index().unwrap();

        // Now check each UFO in turn.
        for (ix, source) in ds1.sources.iter().enumerate() {
            use crate::convertors::ufo::tests::ufo_semantic_test;

            let relative_path_to_master = if let Some(r) = relative {
                r.join(source.filename.clone())
            } else {
                source.filename.clone().into()
            };
            let font1 = norad::Font::load(relative_path_to_master).unwrap();

            let source2 = ds2
                .sources
                .iter()
                .find(|s| s.filename == source.filename)
                .unwrap();
            let relative_path_to_master2 = tempdir.path().join(source2.filename.clone());
            println!(
                "Testing master: {}, loading from {:?}",
                source.filename,
                relative_path_to_master2.display()
            );
            let font2 = norad::Font::load(relative_path_to_master2).unwrap();
            ufo_semantic_test(&font1, &font2, ix == index_of_default);
        }
    }

    #[test]
    fn test_variable_fonts_roundtrip() {
        let mut font = crate::load("resources/IbarraRealNova.designspace").unwrap();
        assert_eq!(font.instances.len(), 8);
        let variable: Vec<_> = font.instances.iter().filter(|i| i.variable).collect();
        assert_eq!(variable.len(), 2);
        assert_eq!(
            variable[0].name.get_default().unwrap(),
            "IbarraRealNova[wght]"
        );

        // A font containing variable instances must be saved as format 5,
        // even if it was loaded from an older format.
        font.format_specific
            .insert(FORMAT_KEY.to_string(), serde_json::json!(4.0));
        let tempdir = tempfile::tempdir().unwrap();
        let temp_path = tempdir.path().join("temp.designspace");
        save_designspace(&font, &temp_path).unwrap();

        let ds = norad::designspace::DesignSpaceDocument::load(&temp_path).unwrap();
        assert_eq!(ds.format, 5.0);
        // Variable and static instances are split into separate elements.
        assert_eq!(ds.variable_fonts.len(), 2);
        assert_eq!(ds.instances.len(), 6);
        assert_eq!(ds.variable_fonts[0].name, "IbarraRealNova[wght]");
        assert_eq!(ds.variable_fonts[0].axis_subsets.len(), 2);
    }

    #[test]
    fn test_background() {
        let mut font = crate::load("resources/IbarraRealNova.designspace").unwrap();
        let glyph = font.glyphs.iter().find(|g| g.name == "A").unwrap();
        // Each layer in the glyph should have a background layer, and they should be linked
        assert_eq!(glyph.layers.len(), 2 * font.masters.len());
        // Slim the font just to the A glyph for simplicity
        RetainGlyphs::new(vec!["A".to_string()])
            .apply(&mut font)
            .unwrap();
        // Convert master 1 to UFO
        let ufo = as_norad(&font, 0).unwrap();
        // Check it has two layers
        assert_eq!(ufo.layers.len(), 2);
    }
}
