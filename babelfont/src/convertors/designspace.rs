use crate::{
    convertors::ufo::{as_norad, load_kerning, stash_lib, KEY_LIB},
    glyph::GlyphList,
    names::Names,
    I18NDictionary, Instance, Layer, Tag,
};
use fontdrasil::coords::{DesignCoord, DesignLocation};
use norad::{
    designspace::{DesignSpaceDocument, Instance as DSInstance, RuleProcessing, Rules, Source},
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

use crate::{
    convertors::ufo::{load_master_info, norad_glyph_to_babelfont_layer},
    Axis, BabelfontError, Font, LoadOptions, LoadResult, Master, SourceLoadFailure,
};

/// Load a DesignSpace document and all referenced UFOs into a Babelfont Font
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    load_with_options(path, &LoadOptions::default()).map(|result| result.font)
}

/// Like [`load`], but load only the parts of the font requested by `options`,
/// and report the `<source>` elements that failed to load instead of silently
/// dropping them: each failure leaves the returned font without the
/// corresponding master.
///
/// Font-level data (names, features, lib, kerning groups) and the glyph list
/// live in the default source, so it must always load — unless neither
/// masters nor glyphs are requested, in which case no source UFO is opened at
/// all and the returned font holds only what the designspace document itself
/// declares: axes, instances, and bare master records.
pub fn load_with_options(
    path: PathBuf,
    options: &LoadOptions,
) -> Result<LoadResult, BabelfontError> {
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
    let mut font = if options.load_masters || options.load_glyphs {
        let relative_path_to_default_master = if let Some(r) = relative {
            r.join(default_master.filename.clone())
        } else {
            default_master.filename.clone().into()
        };
        crate::convertors::ufo::load_with_options(relative_path_to_default_master, options)?
    } else {
        Font::new()
    };
    if options.load_axes {
        font.axes = axes;
    }

    load_instances(&mut font, &axis_name_tag_map, &ds.instances);
    // Drop the master loaded from the default UFO above, and its layers;
    // both are rebuilt from the designspace's sources.
    font.masters.clear();
    for g in font.glyphs.iter_mut() {
        g.layers.clear();
    }
    let mut failures: Vec<SourceLoadFailure> = Vec::new();
    if options.load_masters {
        let load_layers = options.load_layers && options.load_glyphs;
        let res: Vec<(Master, Vec<Vec<Layer>>)> = ds
            .sources
            .iter()
            .filter_map(|source| {
                match load_master(
                    &font.glyphs,
                    source,
                    relative,
                    &axis_name_tag_map,
                    load_layers,
                ) {
                    Ok(master) => Some(master),
                    Err(e) => {
                        failures.push(SourceLoadFailure {
                            filename: source.filename.clone(),
                            error: e.to_string(),
                        });
                        None
                    }
                }
            })
            .collect();
        for (master, mut layerset) in res {
            font.masters.push(master);
            for (g, l) in font.glyphs.iter_mut().zip(layerset.iter_mut()) {
                g.layers.append(l);
            }
        }
    } else {
        // Bare records of what the document declares; no source UFO is opened
        for source in ds.sources.iter() {
            font.masters.push(master_record(source, &axis_name_tag_map));
        }
    }
    // Stash DS format
    #[allow(clippy::unwrap_used)] // It's a number
    font.format_specific.insert(
        FORMAT_KEY.to_string(),
        serde_json::Value::Number(serde_json::Number::from_f64(ds.format as f64).unwrap()),
    );
    Ok(LoadResult {
        font,
        source_failures: failures,
    })
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

/// Build a [`Master`] from what the designspace document itself declares
/// about a `<source>` — its name, location, filename and style name —
/// without opening the source UFO.
fn master_record(source: &Source, axis_name_tag_map: &HashMap<String, Tag>) -> Master {
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
    master
}

fn load_master(
    glyphs: &GlyphList,
    source: &Source,
    relative: Option<&std::path::Path>,
    axis_name_tag_map: &HashMap<String, Tag>,
    load_layers: bool,
) -> Result<(Master, Vec<Vec<Layer>>), BabelfontError> {
    let mut master = master_record(source, axis_name_tag_map);
    let required_layer = &source.layer;

    let relative_path_to_master = if let Some(r) = relative {
        r.join(source.filename.clone())
    } else {
        source.filename.clone().into()
    };

    let request = if load_layers {
        norad::DataRequest::all()
    } else {
        // Everything except the glyph layers, so no .glif file is parsed
        norad::DataRequest::default().layers(false)
    };
    let source_font = norad::Font::load_requested_data(relative_path_to_master, request)?;
    let info = &source_font.font_info;
    load_master_info(&mut master, info);
    load_kerning(&mut master, &source_font.kerning);
    if !load_layers {
        return Ok((master, vec![]));
    }
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
                        norad_glyph_to_babelfont_layer(background_glyph, ufo_layer, &master.id);
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
    let ds = DesignSpaceDocument {
        format: font
            .format_specific
            .get(FORMAT_KEY)
            .and_then(|v| v.as_f64())
            .unwrap_or(5.0) as f32,
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
        instances: font
            .instances
            .iter()
            .map(|i| to_norad_instance(i, &axis_tag_name_map, &axis_order))
            .collect(),
        lib: Plist::new(),
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
    let mut location: Vec<_> = instance
        .location
        .iter()
        .map(|(tag, coord)| norad::designspace::Dimension {
            name: axis_tag_name_map
                .get(tag)
                .cloned()
                .unwrap_or_else(|| tag.to_string()),
            xvalue: Some(coord.to_f64() as f32),
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
            xvalue: Some(coord.to_f64() as f32),
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

    const PLIST_HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
"#;

    /// A minimal single-glyph UFO, enough for the designspace loader.
    fn write_minimal_ufo(dir: &std::path::Path, name: &str) {
        let ufo = dir.join(name);
        let glyphs = ufo.join("glyphs");
        std::fs::create_dir_all(&glyphs).unwrap();
        std::fs::write(
            ufo.join("metainfo.plist"),
            format!("{PLIST_HEADER}<dict><key>creator</key><string>test</string><key>formatVersion</key><integer>3</integer></dict>\n</plist>\n"),
        )
        .unwrap();
        std::fs::write(
            ufo.join("fontinfo.plist"),
            format!("{PLIST_HEADER}<dict><key>familyName</key><string>Test</string><key>styleName</key><string>Regular</string><key>unitsPerEm</key><integer>1000</integer><key>ascender</key><integer>800</integer></dict>\n</plist>\n"),
        )
        .unwrap();
        std::fs::write(
            ufo.join("layercontents.plist"),
            format!("{PLIST_HEADER}<array><array><string>public.default</string><string>glyphs</string></array></array>\n</plist>\n"),
        )
        .unwrap();
        std::fs::write(
            glyphs.join("contents.plist"),
            format!("{PLIST_HEADER}<dict><key>A</key><string>A_.glif</string></dict>\n</plist>\n"),
        )
        .unwrap();
        std::fs::write(
            glyphs.join("A_.glif"),
            r#"<?xml version="1.0"?>
<glyph name="A" format="2"><advance width="500"/><unicode hex="0041"/><outline><contour><point x="0" y="0" type="line"/><point x="400" y="0" type="line"/><point x="400" y="700" type="line"/><point x="0" y="700" type="line"/></contour></outline></glyph>
"#,
        )
        .unwrap();
    }

    fn write_test_designspace(dir: &std::path::Path, sources: &[&str]) -> PathBuf {
        let source_elements: String = sources
            .iter()
            .enumerate()
            .map(|(i, filename)| {
                format!(
                    r#"<source filename="{filename}"><location><dimension name="Weight" xvalue="{}"/></location></source>"#,
                    100 + i * 100
                )
            })
            .collect();
        let ds = dir.join("Test.designspace");
        std::fs::write(
            &ds,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<designspace format="4.1">
  <axes><axis tag="wght" name="Weight" minimum="100" maximum="900" default="100"/></axes>
  <sources>{source_elements}</sources>
</designspace>
"#
            ),
        )
        .unwrap();
        ds
    }

    #[test]
    fn reports_unloadable_sources() {
        let tempdir = tempfile::tempdir().unwrap();
        write_minimal_ufo(tempdir.path(), "Regular.ufo");
        write_minimal_ufo(tempdir.path(), "Bold.ufo");
        // Corrupt the non-default source so it cannot load.
        std::fs::write(
            tempdir.path().join("Bold.ufo/metainfo.plist"),
            "not a plist",
        )
        .unwrap();
        let ds_path = write_test_designspace(tempdir.path(), &["Regular.ufo", "Bold.ufo"]);

        let result = load_with_options(ds_path.clone(), &LoadOptions::default()).unwrap();
        assert_eq!(result.font.masters.len(), 1);
        assert_eq!(result.source_failures.len(), 1);
        assert_eq!(result.source_failures[0].filename, "Bold.ufo");
        assert!(!result.source_failures[0].error.is_empty());

        // `load` keeps its existing behavior: the loadable master, no error.
        let font = load(ds_path).unwrap();
        assert_eq!(font.masters.len(), 1);
    }

    #[test]
    fn report_is_empty_when_all_sources_load() {
        let tempdir = tempfile::tempdir().unwrap();
        write_minimal_ufo(tempdir.path(), "Regular.ufo");
        write_minimal_ufo(tempdir.path(), "Bold.ufo");
        let ds_path = write_test_designspace(tempdir.path(), &["Regular.ufo", "Bold.ufo"]);

        let result = load_with_options(ds_path, &LoadOptions::default()).unwrap();
        assert_eq!(result.font.masters.len(), 2);
        assert!(result.source_failures.is_empty());
    }

    #[test]
    fn skip_layers_keeps_masters_and_glyph_stubs() {
        let tempdir = tempfile::tempdir().unwrap();
        write_minimal_ufo(tempdir.path(), "Regular.ufo");
        write_minimal_ufo(tempdir.path(), "Bold.ufo");
        let ds_path = write_test_designspace(tempdir.path(), &["Regular.ufo", "Bold.ufo"]);

        let result = load_with_options(
            ds_path,
            &LoadOptions {
                load_layers: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.source_failures.is_empty());
        let font = result.font;
        assert_eq!(font.masters.len(), 2);
        // Master content is loaded from the source UFOs
        assert!(!font.masters[0].metrics.is_empty());
        // Font-level data is loaded from the default source
        assert_eq!(
            font.names.family_name.get_default(),
            Some(&"Test".to_string())
        );
        // The glyph list holds layer-less stubs
        let glyph = font.glyphs.get("A").unwrap();
        assert!(glyph.layers.is_empty());
    }

    #[test]
    fn skip_glyphs_keeps_font_and_master_data() {
        let tempdir = tempfile::tempdir().unwrap();
        write_minimal_ufo(tempdir.path(), "Regular.ufo");
        write_minimal_ufo(tempdir.path(), "Bold.ufo");
        let ds_path = write_test_designspace(tempdir.path(), &["Regular.ufo", "Bold.ufo"]);

        let result = load_with_options(
            ds_path,
            &LoadOptions {
                load_glyphs: false,
                ..Default::default()
            },
        )
        .unwrap();
        let font = result.font;
        assert!(font.glyphs.is_empty());
        assert_eq!(font.masters.len(), 2);
        assert_eq!(
            font.names.family_name.get_default(),
            Some(&"Test".to_string())
        );
    }

    #[test]
    fn skip_masters_does_not_open_sources() {
        let tempdir = tempfile::tempdir().unwrap();
        // The source UFOs are never written: a partial parse of the document
        // alone must not try to open them.
        let ds_path = write_test_designspace(tempdir.path(), &["Regular.ufo", "Bold.ufo"]);

        let result = load_with_options(
            ds_path,
            &LoadOptions {
                load_masters: false,
                load_glyphs: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.source_failures.is_empty());
        let font = result.font;
        assert_eq!(font.axes.len(), 1);
        assert!(font.glyphs.is_empty());
        // Bare master records, straight from the document
        assert_eq!(font.masters.len(), 2);
        assert_eq!(
            font.masters[1]
                .location
                .get(Tag::new(b"wght"))
                .map(|x| x.to_f64()),
            Some(200.0)
        );
        assert_eq!(
            font.masters[1]
                .format_specific
                .get(FILENAME_KEY)
                .and_then(|v| v.as_str()),
            Some("Bold.ufo")
        );
        assert!(font.masters[1].metrics.is_empty());
    }

    #[test]
    fn skip_axes() {
        let tempdir = tempfile::tempdir().unwrap();
        write_minimal_ufo(tempdir.path(), "Regular.ufo");
        let ds_path = write_test_designspace(tempdir.path(), &["Regular.ufo"]);

        let result = load_with_options(
            ds_path,
            &LoadOptions {
                load_axes: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.font.axes.is_empty());
        assert_eq!(result.font.masters.len(), 1);
    }
}
