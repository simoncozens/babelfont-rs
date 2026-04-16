use fea_rs_ast::LanguageSystemStatement;
use fontdrasil::{
    coords::{
        ConvertSpace, DesignCoord, DesignLocation, NormalizedCoord, NormalizedSpace, UserCoord,
    },
    types::Axes,
};
use indexmap::IndexMap;
use itertools::Itertools;
use skrifa::{
    outline::DrawSettings,
    prelude::{LocationRef, Size},
    raw::{tables::glyf, TableProvider},
    string::StringId,
    GlyphId, GlyphNames, MetadataProvider,
};
use smol_str::SmolStr;
use sr_aef::fea_rs_ast::AsFea;
use std::collections::{HashMap, HashSet};
use write_fonts::types::F2Dot14;

use crate::{
    features::PossiblyAutomaticCode, Anchor, BabelfontError, Features, Font, FormatSpecific, Glyph,
    Instance, Layer, LayerType, MetricType, PathBuilder, Tag,
};

/// Load a TTF font from a file path
pub fn load<T: AsRef<std::path::Path>>(path: T) -> Result<Font, BabelfontError> {
    let mut font = Font::new();
    let binary = std::fs::read(path.as_ref()).map_err(|e| BabelfontError::IO(e.to_string()))?;
    let fontref =
        skrifa::FontRef::new(&binary).map_err(|e| BabelfontError::BinaryFontRead(e.to_string()))?;
    font.upm = fontref.head()?.units_per_em();

    load_axes(&fontref, &mut font)?;
    load_axis_mappings(&fontref, &mut font)?;
    load_names(&fontref, &mut font)?;
    load_instances(&fontref, &mut font)?;
    load_masters(&fontref, &mut font)?;
    load_glyphs(&fontref, &mut font)?;
    load_features(&fontref, &mut font)?;
    Ok(font)
}

fn name_id_to_i18n(fontref: &skrifa::FontRef, name_id: StringId) -> crate::I18NDictionary {
    let names = fontref.localized_strings(name_id);
    let mut dict = crate::I18NDictionary::new();
    for name in names {
        if let Some(lang) = name.language() {
            let lang = if lang == "en-US" {
                crate::i18ndictionary::DFLT
            } else {
                lang
            };
            dict.insert(lang.to_string(), name.to_string());
        }
    }
    dict
}

fn load_axes(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    for axis in fontref.axes().iter() {
        let name = name_id_to_i18n(fontref, axis.name_id());
        let tag = axis.tag();
        let min = axis.min_value();
        let default = axis.default_value();
        let max = axis.max_value();
        font.axes.push(crate::Axis {
            name,
            tag: crate::Tag::from_be_bytes(tag.to_be_bytes()), // hateful
            min: Some(UserCoord::new(min as f64)),
            default: Some(UserCoord::new(default as f64)),
            max: Some(UserCoord::new(max as f64)),
            hidden: axis.is_hidden(),
            ..Default::default()
        });
    }
    Ok(())
}

fn load_axis_mappings(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    // Skrifa doesn't support axis mappings, handle it ourselves
    if let Ok(avar) = fontref.avar() {
        for (axis, segmap) in font.axes.iter_mut().zip(avar.axis_segment_maps().iter()) {
            let converter = axis._converter()?;
            let segmap = segmap.map_err(|e| BabelfontError::BinaryFontRead(e.to_string()))?;
            if segmap.axis_value_maps().len() == 3 {
                // -1/0/1, default
                continue;
            }
            let mut map_vec = vec![];
            for map in segmap.axis_value_maps() {
                let from = NormalizedCoord::new(map.from_coordinate().to_f32() as f64);
                let to = NormalizedCoord::new(map.to_coordinate().to_f32() as f64);
                map_vec.push((from.to_user(&converter), to.to_design(&converter)));
            }
            axis.map = Some(map_vec);
        }
    }
    Ok(())
}

fn load_names(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    for name_id in fontref
        .name()?
        .name_record()
        .iter()
        .map(|record| record.name_id())
        .filter(|id| id.to_u16() < 256)
        .collect::<Vec<_>>()
    {
        if let Some(record) = font.names.get_mut(name_id) {
            *record = name_id_to_i18n(fontref, name_id);
        }
    }
    Ok(())
}

fn load_instances(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    for (id, instance) in fontref.named_instances().iter().enumerate() {
        let name = name_id_to_i18n(fontref, instance.subfamily_name_id());
        let coordinates = instance.location();
        let location: Vec<(crate::Tag, DesignCoord)> = font
            .axes
            .iter()
            .zip(coordinates.coords().iter())
            .flat_map(|(axis, coord)| {
                let normalized = NormalizedCoord::new(coord.to_f32() as f64);
                let design_coord = normalized.to_design(&axis._converter().ok()?);
                Some((axis.tag, design_coord))
            })
            .collect();
        font.instances.push(Instance {
            id: id.to_string(),
            name,
            location: location.into(),
            ..Default::default()
        })
    }
    Ok(())
}

fn load_masters(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    let mut master_locations: HashSet<DesignLocation> = HashSet::new();
    if let Ok(gvar) = fontref.gvar() {
        if let Ok(shared_tuples) = gvar.shared_tuples() {
            for tuple in shared_tuples.tuples().iter().flatten() {
                let loc = tuple
                    .values()
                    .iter()
                    .zip(font.axes.iter())
                    .flat_map(|(coord, axis)| {
                        let normalized = NormalizedCoord::new(coord.get().to_f32() as f64);
                        let design_coord = normalized.to_design(&axis._converter().ok()?);
                        Some((axis.tag, design_coord))
                    })
                    .collect::<Vec<(Tag, DesignCoord)>>();
                master_locations.insert(DesignLocation::from(loc));
            }
        }
    }
    for loc in master_locations {
        let name = loc
            .iter()
            .map(|(tag, coord)| format!("{}={}", tag, coord.to_f64()))
            .collect::<Vec<String>>()
            .join(", ");

        let metrics = load_metrics(fontref, font, &loc)?;
        // More here, check use_typo_metrics
        font.masters.push(crate::Master {
            name: name.into(),
            id: uuid::Uuid::new_v4().to_string(),
            location: loc,
            metrics,
            ..Default::default()
        });
    }
    // Those were the variations (or there were none), add a default one
    let metrics = load_metrics(fontref, font, &fontdrasil::coords::Location::default())?;
    let mut default_location = DesignLocation::new();
    for axis in &font.axes {
        default_location.insert(
            axis.tag,
            axis.default
                .unwrap_or(UserCoord::new(0.0))
                .to_design(&axis._converter()?),
        );
    }

    font.masters.push(crate::Master {
        name: "Default".into(),
        id: uuid::Uuid::new_v4().to_string(),
        location: default_location,
        metrics,
        ..Default::default()
    });
    Ok(())
}

fn load_metrics(
    fontref: &skrifa::FontRef<'_>,
    font: &mut Font,
    loc: &fontdrasil::coords::Location<fontdrasil::coords::DesignSpace>,
) -> Result<IndexMap<MetricType, i32>, BabelfontError> {
    let skrifa_metrics = fontref.metrics(
        Size::unscaled(),
        LocationRef::new(&fontdrasil_location_to_skrifa_location(
            loc.clone(),
            &font.axes,
        )?),
    );
    let mut metrics = IndexMap::new();
    metrics.insert(MetricType::Ascender, skrifa_metrics.ascent as i32);
    metrics.insert(MetricType::Descender, skrifa_metrics.descent as i32);
    if let Some(x_height) = skrifa_metrics.x_height {
        metrics.insert(MetricType::XHeight, x_height as i32);
    }
    metrics.insert(
        MetricType::ItalicAngle,
        fontref.post()?.italic_angle().to_f32() as i32,
    );
    Ok(metrics)
}

fn load_glyphs(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    let glyph_count = fontref.maxp()?.num_glyphs() as u32;
    let names = GlyphNames::new(fontref);
    for glyph_id_u32 in 0..glyph_count {
        let gid = skrifa::GlyphId::new(glyph_id_u32);
        let proposed_name = names
            .get(gid)
            .ok_or(BabelfontError::BinaryFontRead(format!(
                "Glyph ID {} does not have a name in the font's 'post' table",
                glyph_id_u32
            )))?;
        let mut glyph = Glyph::new(proposed_name.as_str());
        glyph.exported = true;
        font.glyphs.push(glyph);
    }

    // Now do it again and load layers
    for (gid, glyph) in font.glyphs.iter_mut().enumerate() {
        for master in &font.masters {
            glyph.layers.push(load_layer(
                &font.axes,
                fontref,
                GlyphId::new(gid as u32),
                master,
                &names,
            )?);
        }
    }
    //cmap
    for (unicode, glyphid) in fontref.charmap().mappings() {
        let glyph = font
            .glyphs
            .get_by_index_mut(glyphid.to_u32() as usize)
            .ok_or(BabelfontError::BinaryFontRead(format!(
                "Glyph ID {} from charmap does not exist in the font",
                glyphid.to_u32()
            )))?;
        glyph.codepoints.push(unicode);
    }
    Ok(())
}

fn load_features(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    let axes = font.fontdrasil_axes()?;
    let uncompile_context = sr_aef::uncompile_context(fontref)
        .map_err(|e| BabelfontError::BinaryFontRead(e.to_string()))?;
    let master_ids = font
        .masters
        .iter()
        .map(|m| (m.id.clone(), m))
        .collect::<HashMap<_, _>>();
    let mut features = Features::default();
    for (anchor_name, anchors) in uncompile_context.anchors {
        for (glyph_name, anchor) in anchors {
            if let Some(glyph) = font.glyphs.get_mut(&glyph_name) {
                for layer in glyph.layers.iter_mut() {
                    let LayerType::DefaultForMaster(mid) = &layer.master else {
                        continue;
                    };
                    let Some(master) = master_ids.get(mid) else {
                        log::warn!(
                            "Master ID {} from anchor {} does not exist in the font",
                            mid,
                            &anchor_name
                        );
                        continue;
                    };
                    let location = master.location.clone();
                    let (x, y) = get_x_y_location_for_anchor(location, &anchor, &axes)?;
                    layer.anchors.push(Anchor {
                        name: anchor_name.clone().into(),
                        x,
                        y,
                        format_specific: FormatSpecific::default(),
                    });
                }
            } else {
                log::warn!(
                    "Glyph {} from anchor {} does not exist in the font",
                    glyph_name,
                    &anchor_name
                );
            }
        }
    }

    for (class_name, glyphs) in uncompile_context.named_classes.iter() {
        features.classes.insert(
            class_name.clone(),
            PossiblyAutomaticCode::new(glyphs.as_fea("")),
        );
    }
    let mut language_systems = vec![];
    for (script_tag, lang_sys_tags) in &uncompile_context.language_systems {
        for lang_sys_tag in lang_sys_tags {
            let lss =
                LanguageSystemStatement::new(script_tag.to_string(), lang_sys_tag.to_string());
            language_systems.push(lss.as_fea(""));
        }
    }
    let language_systems_block = language_systems.join("\n");
    if !language_systems_block.is_empty() {
        features.prefixes.insert(
            "LanguageSystems".into(),
            PossiblyAutomaticCode::new(language_systems_block),
        );
    }
    for (lookup_name, statements) in uncompile_context.lookups.iter() {
        features.prefixes.insert(
            lookup_name.clone(),
            PossiblyAutomaticCode::new(statements.as_fea("")),
        );
    }
    for (feature_name, lookups) in uncompile_context.features.iter() {
        features.features.push((
            feature_name.clone(),
            PossiblyAutomaticCode::new(lookups.iter().map(|l| l.as_fea("")).join("\n")),
        ));
    }
    font.features = features;
    Ok(())
}

fn get_x_y_location_for_anchor(
    location: DesignLocation,
    anchor: &sr_aef::fea_rs_ast::Anchor,
    axes: &Axes,
) -> Result<(f64, f64), BabelfontError> {
    let user_loc = location.to_user(&axes)?;
    let simple_user_loc = user_loc
        .iter()
        .map(|(tag, coord)| (SmolStr::from(tag.to_string()), coord.to_f64() as i16))
        .collect::<IndexMap<SmolStr, i16>>();

    let x = match &anchor.x {
        fea_rs_ast::Metric::Scalar(x_scalar) => *x_scalar as f64,
        fea_rs_ast::Metric::Variable(items) => {
            // Anchor must exist at this location, so find the first item that matches the location
            items
                .iter()
                .find_map(|(loc, item)| {
                    if loc == &simple_user_loc {
                        Some(*item as f64)
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        }
        fea_rs_ast::Metric::GlyphsAppNumber(_) => unreachable!(),
    };

    let y = match &anchor.y {
        fea_rs_ast::Metric::Scalar(y_scalar) => *y_scalar as f64,
        fea_rs_ast::Metric::Variable(items) => {
            // Anchor must exist at this location, so find the first item that matches the location
            items
                .iter()
                .find_map(|(loc, item)| {
                    if loc == &simple_user_loc {
                        Some(*item as f64)
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        }
        fea_rs_ast::Metric::GlyphsAppNumber(_) => unreachable!(),
    };

    Ok((x, y))
}

fn fontdrasil_location_to_skrifa_location<Space: ConvertSpace<NormalizedSpace>>(
    loc: fontdrasil::coords::Location<Space>,
    axis_order: &[crate::Axis],
) -> Result<Vec<F2Dot14>, BabelfontError> {
    let mut coords = vec![];

    for axis in axis_order {
        let converter = axis._converter()?;
        let coord = loc
            .get(axis.tag)
            .map(|c| F2Dot14::from_f32(c.to_normalized(&converter).to_f64() as f32))
            .unwrap_or(F2Dot14::from_f32(0.0));
        coords.push(coord);
    }

    Ok(coords)
}

fn load_layer(
    axes: &[crate::Axis],
    fontref: &skrifa::FontRef<'_>,
    gid: skrifa::GlyphId,
    master: &crate::Master,
    names: &skrifa::GlyphNames<'_>,
) -> Result<crate::Layer, BabelfontError> {
    let loc = fontdrasil_location_to_skrifa_location(master.location.clone(), axes)?;
    let locationref = LocationRef::new(&loc);
    let width = fontref.glyph_metrics(Size::unscaled(), locationref);
    let mut layer = Layer::new(width.advance_width(gid).unwrap_or_default());
    layer.master = crate::LayerType::DefaultForMaster(master.id.clone());

    // Skrifa pens don't support components. Parse component glyphs manually.
    let glyf = fontref.glyf()?;
    let Some(glyph) = fontref.loca(None)?.get_glyf(gid, &glyf)? else {
        // We're done
        return Ok(layer);
    };

    match glyph {
        glyf::Glyph::Simple(_) => {
            if let Some(outline) = fontref.outline_glyphs().get(gid) {
                let mut pen = PathBuilder::new();
                outline
                    .draw(
                        DrawSettings::unhinted(Size::unscaled(), locationref),
                        &mut pen,
                    )
                    .map_err(|e| BabelfontError::BinaryFontRead(e.to_string()))?;
                layer.shapes = pen.build().into_iter().map(crate::Shape::Path).collect();
            }
        }
        glyf::Glyph::Composite(c) => {
            // This gets complex. :-/
            for component in c.components() {
                let gid = component.glyph;
                let anchor = match component.anchor {
                    glyf::Anchor::Offset { x, y } => (x as f64, y as f64),
                    glyf::Anchor::Point {
                        base: _,
                        component: _,
                    } => (0.0, 0.0),
                };
                // XXX apply deltas
                let our_affine = kurbo::Affine::new([
                    component.transform.xx.to_f32() as f64,
                    component.transform.xy.to_f32() as f64,
                    component.transform.yx.to_f32() as f64,
                    component.transform.yy.to_f32() as f64,
                    anchor.0,
                    anchor.1,
                ]);

                layer.shapes.push(crate::Shape::Component(crate::Component {
                    reference: names
                        .get(gid.into())
                        .ok_or(BabelfontError::BinaryFontRead(format!(
                            "Component glyph ID {} does not have a name in the font's 'post' table",
                            gid.to_u32()
                        )))?
                        .to_string()
                        .into(),
                    transform: our_affine.into(),
                    location: IndexMap::new(), // We don't have per-component locations in TTF, so leave this empty
                    format_specific: FormatSpecific::default(),
                }));
            }
        }
    }

    Ok(layer)
}
