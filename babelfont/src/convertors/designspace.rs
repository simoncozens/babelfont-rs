use crate::{
    convertors::ufo::{load_kerning, stash_lib},
    glyph::GlyphList,
    names::Names,
    Instance, Layer,
};
use fontdrasil::coords::{DesignCoord, DesignLocation};
use norad::{
    designspace::{DesignSpaceDocument, Instance as DSInstance, RuleProcessing, Rules, Source},
    Plist,
};
use std::collections::{BTreeMap, HashMap};
use write_fonts::types::Tag;
// use rayon::prelude::*;
use std::path::{Component, Path as FsPath, PathBuf};

use uuid::Uuid;

/// Key to store the designspace format version in the font's format_specific
pub const FORMAT_KEY: &str = "norad.designspace.format";
/// Key to store the master filename in the instance's format_specific
pub const FILENAME_KEY: &str = "norad.designspace.filename";

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
    let axis_name_tag_map = axes
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
    Ok(font)
}

/// Load a DesignSpace document and referenced UFOs from in-memory entries.
///
/// `path` is the virtual designspace path (for example `My.designspace` or `sources/My.designspace`).
pub fn load_entries(path: PathBuf, entries: &HashMap<String, String>) -> Result<Font, BabelfontError> {
    let normalized_entries: HashMap<String, String> = entries
        .iter()
        .map(|(k, v)| (normalize_virtual_path(k).unwrap_or_else(|| k.clone()), v.clone()))
        .collect();

    let ds_path = normalize_virtual_path(&path.to_string_lossy()).ok_or_else(|| {
        BabelfontError::General(format!("Invalid designspace path: {}", path.display()))
    })?;
    let ds_contents = normalized_entries
        .get(&ds_path)
        .or_else(|| {
            FsPath::new(&ds_path)
                .file_name()
                .and_then(|name| normalized_entries.get(&name.to_string_lossy().to_string()))
        })
        .ok_or_else(|| {
            BabelfontError::General(format!("Designspace file not found in entries: {}", ds_path))
        })?;

    let ds: DesignSpaceDocument = norad::designspace::DesignSpaceDocument::load_str(ds_contents)?;
    let ds_base = FsPath::new(&ds_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let axes: Vec<Axis> = ds
        .axes
        .iter()
        .filter_map(|dsax| dsax.try_into().ok())
        .collect();
    #[allow(clippy::unwrap_used)]
    let axis_name_tag_map = axes
        .iter()
        .map(|ax| (ax.name.get_default().unwrap().clone(), ax.tag))
        .collect();

    let default_master = default_master(&ds, &axes).ok_or(BabelfontError::NoDefaultMaster)?;
    let default_ufo_path = resolve_virtual_reference(&ds_base, &default_master.filename)?;
    let mut font = crate::convertors::ufo::load_entries(PathBuf::from(&default_ufo_path), &normalized_entries)?;
    font.axes = axes;

    load_instances(&mut font, &axis_name_tag_map, &ds.instances);
    let res: Vec<(Master, Vec<Vec<Layer>>)> = ds
        .sources
        .iter()
        .map(|source| {
            load_master_from_entries(
                &font.glyphs,
                source,
                &ds_base,
                &axis_name_tag_map,
                &normalized_entries,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    font.masters.clear();
    for g in font.glyphs.iter_mut() {
        g.layers.clear();
    }
    for (master, mut layerset) in res {
        font.masters.push(master);
        for (g, l) in font.glyphs.iter_mut().zip(layerset.iter_mut()) {
            g.layers.append(l);
        }
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
                            DesignCoord::new(
                                dimension.xvalue.map(|x| x as f64).unwrap_or_default(),
                            ),
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
                    DesignCoord::new(dimension.xvalue.map(|x| x as f64).unwrap_or_default()),
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
            let mut our_layer = norad_glyph_to_babelfont_layer(norad_glyph, ufo_layer, &master.id);
            // Even if this is non-default in the UFO, it is the default layer for this master,
            // because we have promoted sparse masters to their own babelfont master.
            our_layer.master = crate::LayerType::DefaultForMaster(master.id.to_string());
            glyph_layer_list.push(our_layer);
        }
        bf_layer_list.push(glyph_layer_list)
    }
    Ok((master, bf_layer_list))
}

fn load_master_from_entries(
    glyphs: &GlyphList,
    source: &Source,
    ds_base: &str,
    axis_name_tag_map: &HashMap<String, Tag>,
    entries: &HashMap<String, String>,
) -> Result<(Master, Vec<Vec<Layer>>), BabelfontError> {
    let location = DesignLocation::from(
        source
            .location
            .iter()
            .map(|dimension| {
                (
                    *axis_name_tag_map
                        .get(dimension.name.as_str())
                        .unwrap_or_else(|| panic!("Axis name not found: {}", dimension.name)),
                    DesignCoord::new(dimension.xvalue.map(|x| x as f64).unwrap_or_default()),
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

    let source_ufo_path = resolve_virtual_reference(ds_base, &source.filename)?;
    let source_font = crate::convertors::ufo::load_norad_from_entries(FsPath::new(&source_ufo_path), entries)?;

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
            let mut our_layer = norad_glyph_to_babelfont_layer(norad_glyph, ufo_layer, &master.id);
            our_layer.master = crate::LayerType::DefaultForMaster(master.id.to_string());
            glyph_layer_list.push(our_layer);
        }
        bf_layer_list.push(glyph_layer_list)
    }
    Ok((master, bf_layer_list))
}

fn resolve_virtual_reference(base_dir: &str, relative: &str) -> Result<String, BabelfontError> {
    let combined = if base_dir.is_empty() {
        relative.to_string()
    } else {
        format!("{}/{}", base_dir.trim_end_matches('/'), relative)
    };
    normalize_virtual_path(&combined).ok_or_else(|| {
        BabelfontError::General(format!(
            "Designspace source path escapes project root or is invalid: {}",
            relative
        ))
    })
}

fn normalize_virtual_path(path: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for component in FsPath::new(path).components() {
        match component {
            Component::Normal(p) => parts.push(p.to_string_lossy().to_string()),
            Component::CurDir | Component::RootDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return None;
                }
            }
            Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
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
            if defaults.get(&loc.name)
                != loc.xvalue.map(|x| x as f64).map(DesignCoord::new).as_ref()
            {
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
                        .unwrap_or(&"Unnamed font".to_string()),
                    m.name
                        .get_default()
                        .map(|x| x.as_str())
                        .unwrap_or(m.id.as_str())
                ))
        })
        .collect();
    let ds = DesignSpaceDocument {
        format: font
            .format_specific
            .get(FORMAT_KEY)
            .and_then(|v| v.as_f64())
            .unwrap_or(3.0) as f32,
        axes: font.axes.iter().map(|x| x.into()).collect(),
        axis_mappings: None,
        rules: Rules {
            processing: RuleProcessing::First,
            rules: vec![],
        },
        sources: font
            .masters
            .iter()
            .zip(master_filenames)
            .map(|(m, f)| to_source(m, f, &axis_tag_name_map))
            .collect(),
        instances: font.instances.iter().map(|i| i.into()).collect(),
        lib: Plist::new(),
    };
    Ok(ds.save(path)?)
}

fn to_source(
    master: &Master,
    filename: String,
    axis_tag_name_map: &HashMap<Tag, String>,
) -> norad::designspace::Source {
    norad::designspace::Source {
        familyname: None, // Maybe we want custom names for masters?
        stylename: None,  // ???
        name: master.name.get_default().map(|x| x.to_string()),
        // We know we have one
        filename,
        location: master
            .location
            .iter()
            .map(|(tag, coord)| norad::designspace::Dimension {
                name: axis_tag_name_map
                    .get(tag)
                    .cloned()
                    .unwrap_or_else(|| tag.to_string()),
                uservalue: Some(coord.to_f64() as f32),
                xvalue: None,
                yvalue: None,
            })
            .collect(),
        layer: None, // XXX
    }
}

#[cfg(test)]
mod tests {
        #![allow(clippy::unwrap_used)]

        use super::*;

        fn basic_ufo_entries(prefix: &str) -> HashMap<String, String> {
                let mut entries = HashMap::new();
                let p = |suffix: &str| {
                        if prefix.is_empty() {
                                suffix.to_string()
                        } else {
                                format!("{}/{}", prefix, suffix)
                        }
                };

                entries.insert(
                        p("metainfo.plist"),
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>creator</key><string>org.test</string><key>formatVersion</key><integer>3</integer></dict></plist>"#
                                .to_string(),
                );
                entries.insert(
                        p("fontinfo.plist"),
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>familyName</key><string>TestFamily</string><key>styleName</key><string>Regular</string></dict></plist>"#
                                .to_string(),
                );
                entries.insert(
                        p("layercontents.plist"),
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><array><array><string>public.default</string><string>glyphs</string></array></array></plist>"#
                                .to_string(),
                );
                entries.insert(
                        p("glyphs/contents.plist"),
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>A</key><string>A_.glif</string></dict></plist>"#
                                .to_string(),
                );
                entries.insert(
                        p("glyphs/A_.glif"),
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<glyph name="A" format="2">
    <advance width="600"/>
</glyph>"#
                                .to_string(),
                );
                entries
        }

        #[test]
        fn test_load_entries_designspace() {
                let mut entries = basic_ufo_entries("Master.ufo");
                entries.insert(
                        "Test.designspace".to_string(),
                        r#"<?xml version='1.0' encoding='UTF-8'?>
<designspace format="5.0">
    <axes>
        <axis name="Weight" tag="wght" minimum="100" maximum="900" default="400"/>
    </axes>
    <sources>
        <source filename="Master.ufo" name="Regular">
            <location>
                <dimension name="Weight" xvalue="400"/>
            </location>
        </source>
    </sources>
</designspace>"#
                                .to_string(),
                );

                let font = load_entries(PathBuf::from("Test.designspace"), &entries).unwrap();
                assert_eq!(font.glyphs.len(), 1);
                assert_eq!(font.masters.len(), 1);
        }

        #[test]
        fn test_designspace_rejects_outside_root_reference() {
                let mut entries = HashMap::new();
                entries.insert(
                        "Test.designspace".to_string(),
                        r#"<?xml version='1.0' encoding='UTF-8'?>
<designspace format="5.0">
    <axes>
        <axis name="Weight" tag="wght" minimum="100" maximum="900" default="400"/>
    </axes>
    <sources>
        <source filename="../Outside.ufo" name="Regular">
            <location>
                <dimension name="Weight" xvalue="400"/>
            </location>
        </source>
    </sources>
</designspace>"#
                                .to_string(),
                );

                let result = load_entries(PathBuf::from("Test.designspace"), &entries);
                assert!(result.is_err());
        }
}
