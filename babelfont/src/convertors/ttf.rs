use fea_rs_ast::LanguageSystemStatement;
use fontdrasil::{
    coords::{
        ConvertSpace, DesignCoord, DesignLocation, NormalizedCoord, NormalizedSpace, UserCoord,
    },
    types::Axes,
};
use icu_properties::{
    props::{DefaultIgnorableCodePoint, GeneralCategory},
    CodePointMapData, CodePointSetData,
};
use indexmap::IndexMap;
use itertools::Itertools;
use ordered_float::OrderedFloat;
use skrifa::{
    outline::DrawSettings,
    prelude::{LocationRef, Size},
    raw::{
        tables::{glyf, gpos::PositionSubtables},
        TableProvider,
    },
    string::StringId,
    GlyphId, MetadataProvider,
};
use smol_str::SmolStr;
use sr_aef::fea_rs_ast::AsFea;
use std::collections::{HashMap, HashSet};
use write_fonts::types::F2Dot14;

use crate::layout::expand::expand_glyph_container;

use crate::{
    features::PossiblyAutomaticCode, Anchor, BabelfontError, Features, Font, FormatSpecific, Glyph,
    GlyphCategory, Instance, Layer, LayerType, MetricType, PathBuilder, Tag,
};

/// Load a TTF font from a file path
pub fn load<T: AsRef<std::path::Path>>(path: T) -> Result<Font, BabelfontError> {
    let mut font = Font::new();
    let binary = std::fs::read(path.as_ref()).map_err(|e| BabelfontError::IO(e.to_string()))?;
    let fontref =
        skrifa::FontRef::new(&binary).map_err(|e| BabelfontError::BinaryFontRead(e.to_string()))?;
    font.upm = fontref.head()?.units_per_em();
    font.version = font_version(&fontref)?;

    load_axes(&fontref, &mut font)?;
    load_axis_mappings(&fontref, &mut font)?;
    load_names(&fontref, &mut font)?;
    load_custom_ot_values(&fontref, &mut font)?;
    load_instances(&fontref, &mut font)?;
    load_masters(&fontref, &mut font)?;
    load_glyphs(&fontref, &mut font)?;
    load_features(&fontref, &mut font)?;
    Ok(font)
}

/// head.fontRevision as major and minor versions, the minor in thousandths as
/// Glyphs and UFO state it: 0x00003333 (0.19999) is 0.200
fn font_version(fontref: &skrifa::FontRef) -> Result<(u16, u16), BabelfontError> {
    let revision = fontref.head()?.font_revision().to_f64();
    let thousandths = (revision * 1000.0).round();
    Ok((
        (thousandths / 1000.0).floor() as u16,
        (thousandths % 1000.0) as u16,
    ))
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
            // skrifa reads a record it cannot decode as empty
            let value = name.to_string();
            if !value.is_empty() {
                dict.insert(lang.to_string(), value);
            }
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

/// The fsSelection bits a source states; a compiler sets ITALIC, BOLD and
/// REGULAR from the style.
const SOURCE_FS_SELECTION_BITS: u16 = !(1 << 0 | 1 << 5 | 1 << 6);

fn load_custom_ot_values(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    let os2 = fontref.os2()?;
    let ot = &mut font.custom_ot_values;
    ot.os2_vendor_id = Some(os2.ach_vend_id());
    ot.os2_fs_type = Some(os2.fs_type());
    ot.os2_family_class = Some(os2.s_family_class() as u16);
    ot.os2_panose = os2.panose_10().try_into().ok();
    ot.os2_fs_selection = Some(os2.fs_selection().bits() & SOURCE_FS_SELECTION_BITS);
    ot.head_lowest_rec_ppem = Some(fontref.head()?.lowest_rec_ppem());
    // A variable font's weight and width classes describe its default instance only
    if font.axes.is_empty() {
        ot.os2_us_weight_class = Some(os2.us_weight_class());
        ot.os2_us_width_class = Some(os2.us_width_class());
    }
    Ok(())
}

/// A static font's style as an application reads it: the typographic
/// subfamily, else the subfamily.
fn style_name(fontref: &skrifa::FontRef) -> crate::I18NDictionary {
    for name_id in [
        StringId::TYPOGRAPHIC_SUBFAMILY_NAME,
        StringId::SUBFAMILY_NAME,
    ] {
        let name = name_id_to_i18n(fontref, name_id);
        if name.get_default().is_some() {
            return name;
        }
    }
    "Regular".into()
}

fn load_instances(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    if font.axes.is_empty() {
        // A static font is the one instance of its one master
        font.instances.push(Instance {
            id: "0".to_string(),
            name: style_name(fontref),
            ..Default::default()
        });
        return Ok(());
    }
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
        name: if font.axes.is_empty() {
            style_name(fontref)
        } else {
            "Default".into()
        },
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
    let coords = fontdrasil_location_to_skrifa_location(loc.clone(), &font.axes)?;
    let location = LocationRef::new(&coords);
    let skrifa_metrics = fontref.metrics(Size::unscaled(), location);
    let mut metrics = IndexMap::new();
    metrics.insert(MetricType::Ascender, skrifa_metrics.ascent as i32);
    metrics.insert(MetricType::Descender, skrifa_metrics.descent as i32);
    // OS/2 before version 2 has no sxHeight or sCapHeight; the specification
    // defines them as the tops of U+0078 and U+0048
    let glyph_metrics = fontref.glyph_metrics(Size::unscaled(), location);
    let outline_top = |c: char| {
        fontref
            .charmap()
            .map(c)
            .and_then(|gid| glyph_metrics.bounds(gid))
            .map(|bounds| bounds.y_max)
    };
    if let Some(x_height) = skrifa_metrics.x_height.or_else(|| outline_top('x')) {
        metrics.insert(MetricType::XHeight, x_height as i32);
    }
    if let Some(cap_height) = skrifa_metrics.cap_height.or_else(|| outline_top('H')) {
        metrics.insert(MetricType::CapHeight, cap_height as i32);
    }
    if let Some(strikeout) = skrifa_metrics.strikeout {
        metrics.insert(MetricType::StrikeoutPosition, strikeout.offset as i32);
        metrics.insert(MetricType::StrikeoutSize, strikeout.thickness as i32);
    }
    if let Some(underline) = skrifa_metrics.underline {
        metrics.insert(MetricType::UnderlinePosition, underline.offset as i32);
        metrics.insert(MetricType::UnderlineThickness, underline.thickness as i32);
    }
    // Some things skrifa doesn't know yet
    macro_rules! copy_os2_metric {
        ($metric_type:ident, $method:ident) => {
            metrics.insert(MetricType::$metric_type, fontref.os2()?.$method() as i32);
        };
    }
    copy_os2_metric!(SuperscriptXSize, y_superscript_x_size);
    copy_os2_metric!(SuperscriptYSize, y_superscript_y_size);
    copy_os2_metric!(SuperscriptXOffset, y_superscript_x_offset);
    copy_os2_metric!(SuperscriptYOffset, y_superscript_y_offset);
    copy_os2_metric!(SubscriptXSize, y_subscript_x_size);
    copy_os2_metric!(SubscriptYSize, y_subscript_y_size);
    copy_os2_metric!(SubscriptXOffset, y_subscript_x_offset);
    copy_os2_metric!(SubscriptYOffset, y_subscript_y_offset);
    copy_os2_metric!(WinAscent, us_win_ascent);
    copy_os2_metric!(WinDescent, us_win_descent);
    copy_os2_metric!(TypoLineGap, s_typo_line_gap);
    copy_os2_metric!(TypoAscender, s_typo_ascender);
    copy_os2_metric!(TypoDescender, s_typo_descender);

    metrics.insert(
        MetricType::HheaAscender,
        fontref.hhea()?.ascender().to_i16() as i32,
    );
    metrics.insert(
        MetricType::HheaDescender,
        fontref.hhea()?.descender().to_i16() as i32,
    );
    metrics.insert(
        MetricType::HheaLineGap,
        fontref.hhea()?.line_gap().to_i16() as i32,
    );
    if let Ok(vhea) = fontref.vhea() {
        metrics.insert(MetricType::VheaAscender, vhea.ascender().to_i16() as i32);
        metrics.insert(MetricType::VheaDescender, vhea.descender().to_i16() as i32);
        metrics.insert(MetricType::VheaLineGap, vhea.line_gap().to_i16() as i32);
    }

    // post stores the angle negative for a right lean; babelfont stores the
    // Glyphs convention, the opposite sign.
    metrics.insert(
        MetricType::ItalicAngle,
        -fontref.post()?.italic_angle().to_f32() as i32,
    );
    Ok(metrics)
}

fn load_glyphs(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    // the names the uncompiled features use
    let names = sr_aef::unique_glyph_names(fontref);
    for name in &names {
        let mut glyph = Glyph::new(name.as_str());
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
    load_categories(fontref, font)
}

/// Each glyph's category from GDEF's glyph classes, so that a compiler does not
/// guess one from its name; a mark with an advance is Spacing Combining. Without
/// glyph classes every glyph is a base, so that the compiled font has none either
/// and a shaper guesses them as it does for the binary. But a font that attaches
/// marks gets glyph classes from a compiler anyway, so there a glyph a shaper
/// takes for a mark is one.
fn load_categories(fontref: &skrifa::FontRef, font: &mut Font) -> Result<(), BabelfontError> {
    let class_def = match fontref.gdef() {
        Ok(gdef) => gdef.glyph_class_def().transpose()?,
        Err(_) => None,
    };
    let marks_by_codepoint = class_def.is_none() && attaches_marks(fontref)?;
    let metrics = fontref.glyph_metrics(Size::unscaled(), LocationRef::default());
    for (gid, glyph) in font.glyphs.iter_mut().enumerate() {
        let gid = GlyphId::new(gid as u32);
        let class = match &class_def {
            Some(class_def) => class_def.get(gid),
            None if marks_by_codepoint && shaper_mark(glyph) => 3,
            None => 1,
        };
        glyph.category = match class {
            2 => GlyphCategory::Ligature,
            3 => GlyphCategory::Mark,
            _ => GlyphCategory::Base,
        };
        let subcategory = match class {
            2 => Some("Ligature"),
            3 if metrics.advance_width(gid).unwrap_or_default() == 0.0 => Some("Nonspacing"),
            3 => Some("Spacing Combining"),
            _ => None,
        };
        if let Some(subcategory) = subcategory {
            glyph.format_specific.insert(
                "subcategory".to_string(),
                serde_json::Value::String(subcategory.to_string()),
            );
        }
    }
    Ok(())
}

/// Whether a shaper without glyph classes takes a glyph for a mark: when one of
/// its codepoints is a nonspacing mark and not default-ignorable.
fn shaper_mark(glyph: &Glyph) -> bool {
    let general_category = CodePointMapData::<GeneralCategory>::new();
    let default_ignorable = CodePointSetData::new::<DefaultIgnorableCodePoint>();
    glyph
        .codepoints
        .iter()
        .filter_map(|&codepoint| char::from_u32(codepoint))
        .any(|c| {
            general_category.get(c) == GeneralCategory::NonspacingMark
                && !default_ignorable.contains(c)
        })
}

/// Whether the font's GPOS attaches marks (to bases, ligatures or marks).
fn attaches_marks(fontref: &skrifa::FontRef) -> Result<bool, BabelfontError> {
    let Ok(gpos) = fontref.gpos() else {
        return Ok(false);
    };
    for lookup in gpos.lookup_list()?.lookups().iter() {
        if matches!(
            lookup?.subtables()?,
            PositionSubtables::MarkToBase(_)
                | PositionSubtables::MarkToLig(_)
                | PositionSubtables::MarkToMark(_)
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

// This is only sound for mark class definitions
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
                            anchor_name
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
                    anchor_name
                );
            }
        }
    }

    // Do the same but for mark classes.
    // The lookups name the mark classes, so they are defined too
    let mark_class_definitions = uncompile_context
        .mark_classes
        .values()
        .flatten()
        .map(|definition| definition.as_fea(""))
        .join("\n");
    for (anchor_name, classdefs) in uncompile_context.mark_classes {
        let anchor_name = format!("_{}", anchor_name);
        for definition in classdefs.iter() {
            let anchor = &definition.anchor;
            let glyphs = &definition.glyphs;
            let glyphset = expand_glyph_container(font, glyphs, &HashMap::new(), None);
            for glyph_name in glyphset {
                if let Some(glyph) = font.glyphs.get_mut(&glyph_name) {
                    for layer in glyph.layers.iter_mut() {
                        let LayerType::DefaultForMaster(mid) = &layer.master else {
                            continue;
                        };
                        let Some(master) = master_ids.get(mid) else {
                            log::warn!(
                                "Master ID {} from anchor {} does not exist in the font",
                                mid,
                                anchor_name
                            );
                            continue;
                        };
                        let location = master.location.clone();
                        let (x, y) = get_x_y_location_for_anchor(location, anchor, &axes)?;
                        layer.anchors.push(Anchor {
                            name: anchor_name.clone(),
                            x,
                            y,
                            format_specific: FormatSpecific::default(),
                        });
                    }
                } else {
                    log::warn!(
                        "Glyph {} from anchor {} does not exist in the font",
                        glyph_name,
                        anchor_name
                    );
                }
            }
        }
    }

    for (class_name, glyphs) in uncompile_context.named_classes.iter() {
        // A class's code is its members; the writers add the brackets
        let members = glyphs.glyphs.iter().map(|g| g.as_fea("")).join(" ");
        features
            .classes
            .insert(class_name.clone(), PossiblyAutomaticCode::new(members));
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
    if let Some(glyph_classes) = gdef_glyph_classes(fontref, font)? {
        features
            .prefixes
            .insert("GDEF".into(), PossiblyAutomaticCode::new(glyph_classes));
    }
    if !mark_class_definitions.is_empty() {
        features.prefixes.insert(
            "MarkClasses".into(),
            PossiblyAutomaticCode::new(mark_class_definitions),
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

/// GDEF's glyph classes as a GlyphClassDef statement, so that a compiler keeps
/// them rather than deriving its own
fn gdef_glyph_classes(
    fontref: &skrifa::FontRef,
    font: &Font,
) -> Result<Option<String>, BabelfontError> {
    let Ok(gdef) = fontref.gdef() else {
        return Ok(None);
    };
    let Some(class_def) = gdef.glyph_class_def().transpose()? else {
        return Ok(None);
    };
    let mut classes: [Vec<&str>; 4] = Default::default();
    for (gid, class) in class_def.iter() {
        let glyph = font.glyphs.get_by_index(gid.to_u32() as usize);
        let members = (class as usize)
            .checked_sub(1)
            .and_then(|index| classes.get_mut(index));
        if let (Some(glyph), Some(members)) = (glyph, members) {
            members.push(glyph.name.as_str());
        }
    }
    let classes = classes
        .iter()
        .map(|members| {
            if members.is_empty() {
                String::new()
            } else {
                fea_rs_ast::GlyphContainer::new_class(members).as_fea("")
            }
        })
        .join(", ");
    Ok(Some(format!(
        "table GDEF {{\n    GlyphClassDef {};\n}} GDEF;",
        classes
    )))
}

fn get_x_y_location_for_anchor(
    location: DesignLocation,
    anchor: &sr_aef::fea_rs_ast::Anchor,
    axes: &Axes,
) -> Result<(f64, f64), BabelfontError> {
    // sr-aef rounds user coordinates to as few digits as normalize to the same values, so
    // compare the locations normalized
    let normalized_loc = location.to_normalized(axes)?;
    let is_location = |loc: &IndexMap<SmolStr, OrderedFloat<f64>>| {
        normalized_loc.iter().all(|(tag, coord)| {
            axes.get(tag).is_some_and(|axis| {
                loc.get(tag.to_string().as_str()).is_some_and(|user_coord| {
                    UserCoord::new(user_coord.0)
                        .to_normalized(&axis.converter)
                        .to_f2dot14()
                        == coord.to_f2dot14()
                })
            })
        })
    };

    let x = match &anchor.x {
        fea_rs_ast::Metric::Scalar(x_scalar) => *x_scalar as f64,
        fea_rs_ast::Metric::Variable(items) => {
            // Anchor must exist at this location, so find the first item that matches the location
            items
                .iter()
                .find_map(|(loc, item)| {
                    if is_location(loc) {
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
                    if is_location(loc) {
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

/// A glyph's vmtx advance, and its vertical origin: the top of its bounding box
/// (0 for an empty glyph) plus its top side bearing.
fn load_vertical_metrics(
    fontref: &skrifa::FontRef<'_>,
    gid: skrifa::GlyphId,
    layer: &mut Layer,
) -> Result<(), BabelfontError> {
    let Ok(vmtx) = fontref.vmtx() else {
        return Ok(());
    };
    let (Some(advance), Some(tsb)) = (vmtx.advance(gid), vmtx.side_bearing(gid)) else {
        return Ok(());
    };
    let y_max = fontref
        .loca(None)?
        .get_glyf(gid, &fontref.glyf()?)?
        .map_or(0, |glyph| glyph.y_max()) as f32;
    layer.vertical_advance = Some(advance as f32);
    layer.vertical_origin = Some(y_max + tsb as f32);
    Ok(())
}

/// How far a TrueType rasterizer shifts a glyph's stored outline when drawing
/// it: by lsb - xMin, or for a composite with a USE_MY_METRICS component, by that
/// component's shift. An empty glyph's xMin is 0. (At the default location, and
/// without component deltas.)
fn drawn_shift(
    fontref: &skrifa::FontRef<'_>,
    glyf: &glyf::Glyf<'_>,
    gid: skrifa::GlyphId,
    depth: usize,
) -> Result<f64, BabelfontError> {
    if depth > COMPOSITE_RECURSION_LIMIT {
        return Err(BabelfontError::BinaryFontRead(format!(
            "Glyph ID {} nests components too deeply",
            gid.to_u32()
        )));
    }
    let glyph = fontref.loca(None)?.get_glyf(gid, glyf)?;
    let x_min = glyph.as_ref().map_or(0, |glyph| glyph.x_min()) as f64;
    let lsb = fontref
        .hmtx()?
        .side_bearing(gid)
        .map_or(x_min, |lsb| lsb as f64);
    let mut shift = lsb - x_min;
    if let Some(glyf::Glyph::Composite(composite)) = glyph {
        for component in composite.components() {
            if component
                .flags
                .contains(glyf::CompositeGlyphFlags::USE_MY_METRICS)
            {
                shift = drawn_shift(fontref, glyf, component.glyph.into(), depth + 1)?;
            }
        }
    }
    Ok(shift)
}

/// The shift a component's outline was read with (an empty one has none)
fn read_shift(
    fontref: &skrifa::FontRef<'_>,
    glyf: &glyf::Glyf<'_>,
    gid: skrifa::GlyphId,
) -> Result<f64, BabelfontError> {
    if fontref.loca(None)?.get_glyf(gid, glyf)?.is_none() {
        return Ok(0.0);
    }
    drawn_shift(fontref, glyf, gid, 0)
}

const COMPOSITE_RECURSION_LIMIT: usize = 32;

fn load_layer(
    axes: &[crate::Axis],
    fontref: &skrifa::FontRef<'_>,
    gid: skrifa::GlyphId,
    master: &crate::Master,
    names: &[SmolStr],
) -> Result<crate::Layer, BabelfontError> {
    let loc = fontdrasil_location_to_skrifa_location(master.location.clone(), axes)?;
    let locationref = LocationRef::new(&loc);
    let width = fontref.glyph_metrics(Size::unscaled(), locationref);
    let mut layer = Layer::new(width.advance_width(gid).unwrap_or_default());
    layer.master = crate::LayerType::DefaultForMaster(master.id.clone());
    load_vertical_metrics(fontref, gid, &mut layer)?;

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
                // glyf contours run clockwise, a source's counter-clockwise
                layer.shapes = pen
                    .build()
                    .into_iter()
                    .map(|mut path| {
                        path.reverse();
                        crate::Shape::Path(path)
                    })
                    .collect();
            }
        }
        glyf::Glyph::Composite(c) => {
            // This gets complex. :-/
            //
            // Simple glyphs are read as drawn (see drawn_shift). A composite is
            // drawn from its components' stored outlines and then shifted, so
            // each offset gains the composite's shift and loses the one its
            // component was read with, transformed as the component is.
            let shift = drawn_shift(fontref, &glyf, gid, 0)?;
            for component in c.components() {
                let gid = component.glyph;
                let anchor = match component.anchor {
                    glyf::Anchor::Offset { x, y } => (x as f64, y as f64),
                    glyf::Anchor::Point {
                        base: _,
                        component: _,
                    } => (0.0, 0.0),
                };
                // x' = xx * x + xy * y, y' = yx * x + yy * y
                let xx = component.transform.xx.to_f32() as f64;
                let yx = component.transform.yx.to_f32() as f64;
                let xy = component.transform.xy.to_f32() as f64;
                let yy = component.transform.yy.to_f32() as f64;
                let component_shift = read_shift(fontref, &glyf, gid.into())?;
                // XXX apply deltas
                // kurbo::Affine::new expects [xx, yx, xy, yy, dx, dy]
                let our_affine = kurbo::Affine::new([
                    xx,
                    yx,
                    xy,
                    yy,
                    anchor.0 + shift - xx * component_shift,
                    anchor.1 - yx * component_shift,
                ]);

                layer.shapes.push(crate::Shape::Component(crate::Component {
                    reference: names
                        .get(gid.to_u32() as usize)
                        .ok_or(BabelfontError::BinaryFontRead(format!(
                            "Component glyph ID {} is not in the font",
                            gid.to_u32()
                        )))?
                        .clone(),
                    transform: our_affine.into(),
                    location: IndexMap::new(), // We don't have per-component locations in TTF, so leave this empty
                    format_specific: FormatSpecific::default(),
                }));
            }
        }
    }

    Ok(layer)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use kurbo::Shape as _;

    const STATIC_LEGACY: &str = "resources/ttf/StaticLegacy.ttf";
    const GDEF_CLASSES: &str = "resources/ttf/GdefClasses.ttf";
    const DUPLICATE_NAMES: &str = "resources/ttf/DuplicateNames.ttf";
    const MARK_NO_GDEF: &str = "resources/ttf/MarkNoGdef.ttf";

    fn assert_category(
        font: &Font,
        glyph: &str,
        category: GlyphCategory,
        subcategory: Option<&str>,
    ) {
        let glyph = font.glyphs.get(glyph).unwrap();
        assert_eq!(glyph.category, category, "{}", glyph.name);
        assert_eq!(
            glyph
                .format_specific
                .get_optionstring("subcategory")
                .as_deref(),
            subcategory,
            "{}",
            glyph.name
        );
    }

    fn component_coeffs(font: &Font, glyph: &str) -> Vec<[f64; 6]> {
        font.glyphs.get(glyph).unwrap().layers[0]
            .components()
            .map(|c| c.transform.as_affine().as_coeffs())
            .collect()
    }

    fn assert_close(a: &[f64], b: &[f64]) {
        assert!(
            a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6),
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn test_component_transform_order() {
        // barrot turns bar 90 degrees: x' = -y, y' = x
        let font = load(STATIC_LEGACY).unwrap();
        let coeffs = component_coeffs(&font, "barrot");
        assert_close(&coeffs[0][..4], &[0.0, 1.0, -1.0, 0.0]);
    }
    #[test]
    fn test_glyphs_read_as_drawn() {
        // every glyph, simple or composite, comes out where skrifa draws it
        let binary = std::fs::read(STATIC_LEGACY).unwrap();
        let fontref = skrifa::FontRef::new(&binary).unwrap();
        let font = load(STATIC_LEGACY).unwrap();
        for (gid, glyph) in font.glyphs.iter().enumerate() {
            let mut pen = PathBuilder::new();
            fontref
                .outline_glyphs()
                .get(GlyphId::new(gid as u32))
                .unwrap()
                .draw(
                    DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                    &mut pen,
                )
                .unwrap();
            let drawn = pen
                .build()
                .iter()
                .map(|p| p.to_kurbo().unwrap().bounding_box())
                .reduce(|a, b| a.union(b))
                .unwrap_or_default();
            let layer = &glyph.layers[0];
            let mut paths: Vec<crate::Path> = layer.paths().cloned().collect();
            for component in layer.components() {
                paths.extend(component.decompose(None, |name, _| {
                    let layer = &font.glyphs.get(name)?.layers[0];
                    Some(crate::shape::ResolvedShapes {
                        paths: layer.paths().cloned().collect(),
                        components: layer.components().cloned().collect(),
                    })
                }));
            }
            let read = paths
                .iter()
                .map(|p| p.to_kurbo().unwrap().bounding_box())
                .reduce(|a, b| a.union(b))
                .unwrap_or_default();
            assert_close(
                &[read.x0, read.y0, read.x1, read.y1],
                &[drawn.x0, drawn.y0, drawn.x1, drawn.y1],
            );
        }
        assert_close(&component_coeffs(&font, "barflip")[0][4..], &[310.0, 600.0]);
        let bars = component_coeffs(&font, "bars");
        assert_close(&[bars[0][4], bars[1][4]], &[-5.0, 245.0]);
    }
    #[test]
    fn test_contours_read_counter_clockwise() {
        // x is a clockwise rectangle in the binary
        let font = load(STATIC_LEGACY).unwrap();
        let path = font.glyphs.get("x").unwrap().layers[0]
            .paths()
            .next()
            .unwrap();
        let n = path.nodes.len();
        let area: f64 = (0..n)
            .map(|i| {
                let (a, b) = (&path.nodes[i], &path.nodes[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum();
        assert!(area > 0.0, "{:?}", path.nodes);
    }
    #[test]
    fn test_static_font_master_and_instance() {
        // named after the style: ID 17, not ID 2
        let font = load(STATIC_LEGACY).unwrap();
        assert_eq!(font.masters.len(), 1);
        assert_eq!(font.masters[0].name.get_default().unwrap(), "Medium");
        assert_eq!(font.instances.len(), 1);
        assert_eq!(font.instances[0].name.get_default().unwrap(), "Medium");
    }
    #[test]
    fn test_os2_values() {
        let ot = load(STATIC_LEGACY).unwrap().custom_ot_values;
        assert_eq!(ot.os2_us_weight_class, Some(500));
        assert_eq!(ot.os2_us_width_class, Some(6));
        assert_eq!(ot.os2_fs_type, Some(0));
        assert_eq!(ot.os2_panose, Some([2, 0, 6, 3, 0, 0, 0, 0, 0, 0]));
        // USE_TYPO_METRICS; REGULAR is left to the compiler
        assert_eq!(ot.os2_fs_selection, Some(1 << 7));
        assert_eq!(ot.head_lowest_rec_ppem, Some(9));
    }
    #[test]
    fn test_font_revision() {
        // head.fontRevision 2.005 (0x00020148)
        assert_eq!(load(STATIC_LEGACY).unwrap().version, (2, 5));
    }
    #[test]
    fn test_heights_without_os2_v2() {
        let font = load(STATIC_LEGACY).unwrap();
        let metrics = &font.masters[0].metrics;
        assert_eq!(metrics.get(&MetricType::XHeight), Some(&480));
        assert_eq!(metrics.get(&MetricType::CapHeight), Some(&700));
    }
    #[test]
    fn test_hhea_line_gap() {
        let font = load(STATIC_LEGACY).unwrap();
        let metrics = &font.masters[0].metrics;
        assert_eq!(metrics.get(&MetricType::HheaLineGap), Some(&25));
    }
    #[test]
    fn test_vertical_metrics() {
        // vhea 900/-100/0; a: advance height 1000, tsb 100, yMax 500
        let font = load(GDEF_CLASSES).unwrap();
        let metrics = &font.masters[0].metrics;
        assert_eq!(metrics.get(&MetricType::VheaAscender), Some(&900));
        assert_eq!(metrics.get(&MetricType::VheaDescender), Some(&-100));
        assert_eq!(metrics.get(&MetricType::VheaLineGap), Some(&0));
        let layer = &font.glyphs.get("a").unwrap().layers[0];
        assert_eq!(layer.vertical_advance, Some(1000.0));
        assert_eq!(layer.vertical_origin, Some(600.0));
    }
    #[test]
    fn test_undecodable_name_left_out() {
        // the MacJapanese record does not decode; the Windows one does
        let font = load(STATIC_LEGACY).unwrap();
        let ja: Vec<&String> = font
            .names
            .family_name
            .0
            .iter()
            .filter(|(lang, _)| lang.starts_with("ja"))
            .map(|(_, name)| name)
            .collect();
        assert_eq!(ja, ["\u{3057}\u{3063}\u{307d}\u{308a}"]);
    }
    #[test]
    fn test_duplicate_glyph_names() {
        // the post table names glyphs 2 and 3 both "dupe"; Acomp uses glyph 3
        let font = load(DUPLICATE_NAMES).unwrap();
        let names: Vec<&str> = font.glyphs.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, [".notdef", "A", "dupe", "dupe.1", "Acomp"]);
        let component = font.glyphs.get("Acomp").unwrap().layers[0]
            .components()
            .next()
            .unwrap();
        assert_eq!(component.reference, "dupe.1");
    }
    #[test]
    fn test_named_class_members() {
        let font = load(GDEF_CLASSES).unwrap();
        let (_, members) = font.features.classes.first().unwrap();
        assert_eq!(members.code, "a gravecomb acutecomb tildecomb f_f");
        assert!(font
            .features
            .to_fea()
            .contains("= [a gravecomb acutecomb tildecomb f_f\n];"));
    }
    #[test]
    fn test_mark_classes_defined() {
        // every mark class a lookup names has a markClass statement
        let fea = load(GDEF_CLASSES).unwrap().features.to_fea();
        let used: Vec<&str> = fea
            .split("mark @")
            .skip(1)
            .map(|rest| {
                rest.split(|c: char| c == ';' || c.is_whitespace())
                    .next()
                    .unwrap()
            })
            .collect();
        assert!(!used.is_empty(), "{fea}");
        for class in used {
            assert!(fea.contains(&format!("> @{class};")), "@{class}: {fea}");
        }
    }
    #[test]
    fn test_categories_from_gdef() {
        let font = load(GDEF_CLASSES).unwrap();
        // a base in GDEF, though U+0300 is a nonspacing mark
        assert_category(&font, "gravecomb", GlyphCategory::Base, None);
        assert_category(&font, "acutecomb", GlyphCategory::Mark, Some("Nonspacing"));
        assert_category(
            &font,
            "tildecomb",
            GlyphCategory::Mark,
            Some("Spacing Combining"),
        );
        assert_category(&font, "f_f", GlyphCategory::Ligature, Some("Ligature"));
    }

    #[test]
    fn test_categories_without_gdef() {
        let font = load(STATIC_LEGACY).unwrap();
        for glyph in ["gravecomb", "commaaccent", "x"] {
            assert_category(&font, glyph, GlyphCategory::Base, None);
        }
    }

    #[test]
    fn test_categories_without_gdef_attaching_marks() {
        let font = load(MARK_NO_GDEF).unwrap();
        assert_category(&font, "gravecomb", GlyphCategory::Mark, Some("Nonspacing"));
        // U+034F is a default-ignorable nonspacing mark
        for glyph in ["a", "commaaccent", "uni034F"] {
            assert_category(&font, glyph, GlyphCategory::Base, None);
        }
    }
    #[test]
    fn test_gdef_glyph_classes() {
        let font = load(GDEF_CLASSES).unwrap();
        assert_eq!(
            font.features.prefixes.get("GDEF").unwrap().code,
            "table GDEF {\n    GlyphClassDef [a gravecomb \\script], [f_f], [acutecomb tildecomb], ;\n} GDEF;"
        );
        let font = load(STATIC_LEGACY).unwrap();
        assert!(font.features.prefixes.get("GDEF").is_none());
    }
}
