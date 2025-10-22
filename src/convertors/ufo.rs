use crate::glyph::GlyphCategory;
use crate::{
    features::Features, BabelfontError, Component, Font, Glyph, Layer, Master, MetricType, Node,
    OTScalar, Path, Shape,
};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use fontdrasil::coords::Location;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::SystemTime;

pub const KEY_LIB: &str = "norad_lib";

fn stash_lib(lib: Option<&norad::Plist>) -> crate::common::FormatSpecific {
    let mut fs = crate::common::FormatSpecific::default();
    if let Some(lib) = lib {
        fs.insert(
            KEY_LIB.into(),
            serde_json::to_value(lib).unwrap_or_default(),
        );
    }
    fs
}

pub(crate) fn stat(path: &std::path::Path) -> Option<DateTime<chrono::Local>> {
    fs::metadata(path)
        .and_then(|x| x.created())
        .ok()
        .and_then(|x| {
            DateTime::from_timestamp(
                x.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or(std::time::Duration::new(0, 0))
                    .as_secs() as i64,
                0,
            )
        })
        .map(DateTime::<chrono::Local>::from)
}

pub fn load<T: AsRef<std::path::Path>>(path: T) -> Result<Font, BabelfontError> {
    let mut font = Font::new();
    let created_time: Option<DateTime<Local>> = stat(path.as_ref());
    let ufo = norad::Font::load(&path)?;
    font.format_specific = stash_lib(Some(&ufo.lib));
    load_glyphs(&mut font, &ufo);
    let info = &ufo.font_info;
    load_font_info(&mut font, info, created_time);
    let mut master = Master::new(
        info.family_name
            .as_ref()
            .unwrap_or(&"Unnamed master".to_string()),
        info.family_name
            .as_ref()
            .unwrap_or(&"Unnamed master".to_string()),
        Location::new(),
    );
    load_master_info(&mut master, info);
    load_kerning(&mut master, &ufo.kerning);
    (font.first_kern_groups, font.second_kern_groups) = load_kern_groups(&ufo.groups);

    for layer in ufo.iter_layers() {
        for g in font.glyphs.iter_mut() {
            if let Some(norad_glyph) = layer.get_glyph(g.name.as_str()) {
                let layer_id = if layer.is_default() {
                    master.id.as_str()
                } else {
                    layer.name()
                };
                g.layers.push(norad_glyph_to_babelfont_layer(
                    norad_glyph,
                    layer_id,
                    &master.id,
                ))
            }
        }
    }
    font.features = Features::from_fea(&ufo.features);
    for (group_name, group) in ufo.groups.iter() {
        font.features.classes.insert(
            group_name.to_string().into(),
            group.iter().map(|x| x.to_string()).collect(),
        );
    }
    font.masters.push(master);

    Ok(font)
}

pub fn as_norad(font: &Font) -> Result<norad::Font, BabelfontError> {
    let mut ufo = norad::Font::new();
    ufo.lib = font
        .format_specific
        .get(KEY_LIB)
        .and_then(|x| serde_json::from_value::<norad::Plist>(x.clone()).ok())
        .unwrap_or_default();
    let first_master = font
        .masters
        .first()
        .ok_or_else(|| BabelfontError::NoDefaultMaster { path: "UFO".into() })?;
    for g in font.glyphs.iter() {
        for layer in g.layers.iter() {
            let norad_layer = babelfont_layer_to_norad_glyph(g, layer)?;
            // If the layer ID is the master ID, it's the default layer
            let layer = if layer.name.as_ref() == Some(&first_master.id) {
                ufo.default_layer_mut()
            } else {
                ufo.layers
                    .get_or_create_layer(layer.name.as_deref().unwrap_or("public.default"))?
            };
            layer.insert_glyph(norad_layer);
        }
    }
    for (class_name, class) in font.features.classes.iter() {
        let class_ufo: Result<Vec<norad::Name>, norad::error::NamingError> =
            class.iter().map(|x| norad::Name::new(x)).collect();
        ufo.groups.insert(norad::Name::new(class_name)?, class_ufo?);
    }
    save_kerning(&mut ufo.kerning, &first_master.kerning)?;
    save_info(&mut ufo.font_info, font);
    ufo.features = font.features.to_fea();
    Ok(ufo)
}

fn babelfont_layer_to_norad_glyph(
    glyph: &Glyph,
    layer: &Layer,
) -> Result<norad::Glyph, BabelfontError> {
    let mut norad_glyph = norad::Glyph::new(glyph.name.as_str());
    norad_glyph.width = layer.width as f64;
    norad_glyph.codepoints =
        norad::Codepoints::new(glyph.codepoints.iter().flat_map(|&x| char::from_u32(x)));

    for shape in &layer.shapes {
        match shape {
            Shape::Path(p) => {
                norad_glyph.contours.push(save_path(p));
            }
            Shape::Component(c) => {
                norad_glyph.components.push(save_component(c)?);
            }
        }
    }
    norad_glyph.anchors = layer
        .anchors
        .iter()
        .map(norad::Anchor::try_from)
        .collect::<Result<Vec<_>, BabelfontError>>()?;
    if !layer.guides.is_empty() {
        norad_glyph.guidelines = layer
            .guides
            .iter()
            .map(norad::Guideline::try_from)
            .collect::<Result<Vec<_>, BabelfontError>>()?;
    }
    Ok(norad_glyph)
}

pub(crate) fn norad_glyph_to_babelfont_layer(
    glyph: &norad::Glyph,
    layer_name: &str,
    master_id: &str,
) -> Layer {
    let mut l = Layer::new(glyph.width as f32);
    l.master_id = Some(master_id.to_string());
    l.name = Some(layer_name.to_string());

    l.guides = glyph.guidelines.iter().map(|x| x.into()).collect();
    l.anchors = glyph.anchors.iter().map(|x| x.into()).collect();
    for comp in &glyph.components {
        l.shapes.push(Shape::Component(load_component(comp)));
    }
    for contour in &glyph.contours {
        l.shapes.push(Shape::Path(load_path(contour)));
    }
    l
}

pub(crate) fn load_component(c: &norad::Component) -> Component {
    let t = c.transform;
    Component {
        reference: c.base.to_string(),
        transform: kurbo::Affine::new([
            t.x_scale, t.xy_scale, t.yx_scale, t.y_scale, t.x_offset, t.y_offset,
        ]),
        format_specific: stash_lib(c.lib()),
    }
}

pub(crate) fn save_component(c: &Component) -> Result<norad::Component, BabelfontError> {
    let t = c.transform.as_coeffs();
    Ok(norad::Component::new(
        norad::Name::new(c.reference.as_str())?,
        norad::AffineTransform {
            x_scale: t[0],
            xy_scale: t[1],
            yx_scale: t[2],
            y_scale: t[3],
            x_offset: t[4],
            y_offset: t[5],
        },
        None,
        c.format_specific
            .get(KEY_LIB)
            .and_then(|x| serde_json::from_value(x.clone()).ok()),
    ))
}

pub(crate) fn load_path(c: &norad::Contour) -> Path {
    let mut nodes: Vec<Node> = c.points.iter().map(|p| p.into()).collect();
    // See https://github.com/simoncozens/rust-font-tools/issues/3
    nodes.rotate_left(1);
    Path {
        nodes,
        closed: c
            .points
            .first()
            .is_none_or(|v| v.typ != norad::PointType::Move),
        format_specific: stash_lib(c.lib()),
    }
}

pub(crate) fn save_path(p: &Path) -> norad::Contour {
    let mut points: Vec<norad::ContourPoint> = p.nodes.iter().map(|n| n.into()).collect();
    // See https://github.com/simoncozens/rust-font-tools/issues/3
    points.rotate_right(1);
    norad::Contour::new(
        points,
        None,
        p.format_specific
            .get(KEY_LIB)
            .and_then(|x| serde_json::from_value(x.clone()).ok()),
    )
}

pub(crate) fn save_kerning(
    norad_kerning: &mut norad::Kerning,
    babelfont_kerning: &HashMap<(String, String), i16>,
) -> Result<(), BabelfontError> {
    for ((left, right), value) in babelfont_kerning.iter() {
        let left_key = if left.starts_with('@') {
            left.trim_start_matches('@').to_string()
        } else {
            left.to_string()
        };
        let right_key = if right.starts_with('@') {
            right.trim_start_matches('@').to_string()
        } else {
            right.to_string()
        };
        norad_kerning
            .entry(norad::Name::new(left_key.as_str())?)
            .or_default()
            .insert(norad::Name::new(right_key.as_str())?, *value as f64);
    }
    Ok(())
}

pub(crate) fn save_info(info: &mut norad::FontInfo, font: &Font) {
    let get_metric = |mt: MetricType| {
        font.masters
            .first()
            .and_then(|m| m.metrics.get(&mt))
            .map(|&v| v as f64)
    };
    info.ascender = get_metric(MetricType::Ascender);
    info.cap_height = get_metric(MetricType::CapHeight);
    info.copyright = font.names.copyright.get_default().map(|x| x.to_string());
    info.descender = get_metric(MetricType::Descender);
    info.family_name = font.names.family_name.get_default().map(|x| x.to_string());
    let guides: Vec<_> = font
        .masters
        .first()
        .map(|m| m.guides.iter().flat_map(|g| g.try_into()).collect())
        .unwrap_or_default();
    info.guidelines = (!guides.is_empty()).then_some(guides);
    info.italic_angle = get_metric(MetricType::ItalicAngle);
    // macintoshFONDName, yey
    info.note = font.note.clone();
    // gasp range records
    info.open_type_head_created = font.date.format("%Y/%m/%d %H:%M:%S").to_string().into();
    info.open_type_head_flags = font.ot_value("head", "flags", true).and_then(|x| match x {
        OTScalar::BitField(v) => Some(v.clone()),
        _ => None,
    });
    // lowest rec ppem
    info.open_type_hhea_ascender = get_metric(MetricType::HheaAscender).map(|x| x as i32);
    info.open_type_hhea_caret_offset = get_metric(MetricType::HheaCaretOffset).map(|x| x as i32);
    info.open_type_hhea_caret_slope_rise =
        get_metric(MetricType::HheaCaretSlopeRise).map(|x| x as i32);
    info.open_type_hhea_caret_slope_run =
        get_metric(MetricType::HheaCaretSlopeRun).map(|x| x as i32);
    info.open_type_hhea_descender = get_metric(MetricType::HheaDescender).map(|x| x as i32);
    info.open_type_hhea_line_gap = get_metric(MetricType::HheaLineGap).map(|x| x as i32);
    // opentype name compatible full name
    info.open_type_name_description = font.names.description.get_default().map(|x| x.to_string());
    info.open_type_name_designer_url = font.names.designer_url.get_default().map(|x| x.to_string());
    info.open_type_name_designer = font.names.designer.get_default().map(|x| x.to_string());
    info.open_type_name_license = font.names.license.get_default().map(|x| x.to_string());
    info.open_type_name_license_url = font.names.license_url.get_default().map(|x| x.to_string());
    info.open_type_name_manufacturer = font.names.manufacturer.get_default().map(|x| x.to_string());
    info.open_type_name_manufacturer_url = font
        .names
        .manufacturer_url
        .get_default()
        .map(|x| x.to_string());
    info.open_type_name_preferred_family_name =
        font.names.family_name.get_default().map(|x| x.to_string());
    info.open_type_name_preferred_subfamily_name = font
        .names
        .preferred_subfamily_name
        .get_default()
        .map(|x| x.to_string());
    // XXX lots more
    info.postscript_font_name = font
        .names
        .postscript_name
        .get_default()
        .map(|x| x.to_string());
    // and more
    info.style_map_family_name = font
        .names
        .preferred_subfamily_name
        .get_default()
        .map(|x| x.to_string());
    // Style map style name
    info.trademark = font.names.trademark.get_default().map(|x| x.to_string());
    info.units_per_em = Some((font.upm as u32).into());
    info.version_major = Some(font.version.0 as i32);
    info.version_minor = Some(font.version.1 as u32);
    // XXX WOFF
    info.x_height = get_metric(MetricType::XHeight);
}

pub(crate) fn load_master_info(master: &mut Master, info: &norad::FontInfo) {
    let metrics = &mut master.metrics;
    if let Some(v) = info.ascender {
        metrics.insert(MetricType::Ascender, v as i32);
    }
    if let Some(v) = info.cap_height {
        metrics.insert(MetricType::CapHeight, v as i32);
    }
    if let Some(v) = info.descender {
        metrics.insert(MetricType::Descender, v as i32);
    }
    if let Some(v) = &info.guidelines {
        for g in v.iter() {
            master.guides.push(g.into())
        }
    }
    if let Some(v) = info.italic_angle {
        metrics.insert(MetricType::ItalicAngle, v as i32); // XXX i32 won't cut it
    }
    if let Some(v) = info.x_height {
        metrics.insert(MetricType::XHeight, v as i32);
    }
}

pub(crate) fn load_font_info(
    font: &mut Font,
    info: &norad::FontInfo,
    created: Option<DateTime<Local>>,
) {
    if let Some(v) = &info.copyright {
        font.names.copyright = v.into();
    }
    if let Some(v) = &info.family_name {
        font.names.family_name = v.into();
    }
    if let Some(v) = &info.note {
        font.note = Some(v.clone());
    }
    if let Some(v) = &info.open_type_head_created {
        if let Ok(Some(date)) = NaiveDateTime::parse_from_str(v, "%Y/%m/%d %H:%M:%S")
            .map(|x| chrono::Local.from_local_datetime(&x).single())
        {
            font.date = date;
        } else {
            font.date = created.unwrap_or_else(chrono::Local::now);
        }
    }
    if let Some(v) = &info.open_type_head_flags {
        font.set_ot_value("head", "flags", OTScalar::BitField(v.to_vec()))
    }
    if let Some(v) = info.open_type_head_lowest_rec_ppem {
        font.set_ot_value("head", "lowestRecPPEM", OTScalar::Unsigned(v))
    }
    if let Some(v) = &info.open_type_os2_type {
        font.set_ot_value("OS/2", "fsType", OTScalar::BitField(v.to_vec()))
    }
    if let Some(v) = &info.postscript_underline_position {
        font.set_ot_value("post", "underlinePosition", OTScalar::Signed(*v as i32))
    }
    // XXX and much more
    if let Some(v) = &info.trademark {
        font.names.trademark = v.into();
    }

    if let Some(v) = info.units_per_em {
        font.upm = v.as_f64() as u16;
    }
    if let Some(v) = info.version_major {
        font.version.0 = v as u16;
    }
    if let Some(v) = info.version_minor {
        font.version.1 = v as u16;
    }
    if let Some(p) = &info.postscript_font_name {
        font.names.postscript_name = p.into();
    }
}

pub(crate) fn load_kerning(master: &mut Master, kerning: &norad::Kerning) {
    for (left, right_dict) in kerning.iter() {
        for (right, value) in right_dict.iter() {
            let left_maybe_group = if left.starts_with("public.kern") {
                format!("@{:}", left)
            } else {
                left.to_string()
            };
            let right_maybe_group = if right.starts_with("public.kern") {
                format!("@{:}", right)
            } else {
                right.to_string()
            };
            master
                .kerning
                .insert((left_maybe_group, right_maybe_group), *value as i16);
        }
    }
}

pub(crate) fn load_kern_groups(
    _groups: &norad::Groups,
) -> (HashMap<String, Vec<String>>, HashMap<String, Vec<String>>) {
    let first: HashMap<String, Vec<String>> = HashMap::new();
    let second: HashMap<String, Vec<String>> = HashMap::new();
    // for (name, members) in groups.iter() {
    //     hm.insert(
    //         name.to_string(),
    //         members.iter().map(|x| x.to_string()).collect(),
    //     );
    // }
    (first, second)
}

pub(crate) fn load_glyphs(font: &mut Font, ufo: &norad::Font) {
    let categories = ufo
        .lib
        .get("public.openTypeCategories")
        .and_then(|x| x.as_dictionary());
    let psnames = ufo
        .lib
        .get("public.postscriptNames")
        .and_then(|x| x.as_dictionary());
    let skipped: HashSet<String> = ufo
        .lib
        .get("public.skipExportGlyphs")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .flat_map(|x| x.as_string())
        .map(|x| x.to_string())
        .collect();
    let glyphorder: Vec<String> = ufo
        .lib
        .get("public.glyphOrder")
        .and_then(|x| x.as_array())
        .unwrap_or(&vec![])
        .iter()
        .flat_map(|x| x.as_string())
        .map(|x| x.to_string())
        .collect();
    let mut order: Vec<String> = vec![];
    let mut ufo_names: Vec<String> = ufo.iter_names().map(|x| x.to_string()).collect();
    if ufo_names.contains(&".notdef".to_string()) {
        order.push(".notdef".to_string());
        ufo_names.retain(|x| x != ".notdef");
    }
    for name in glyphorder {
        if !ufo_names.contains(&name) {
            continue;
        }
        ufo_names.retain(|x| x != &name);
        order.push(name);
    }
    order.append(&mut ufo_names);

    for glyphname in order {
        if let Some(glyph) = ufo.get_glyph(glyphname.as_str()) {
            let cat = if let Some(cats) = categories {
                match cats.get(&glyphname).and_then(|x| x.as_string()) {
                    Some("base") => GlyphCategory::Base,
                    Some("mark") => GlyphCategory::Mark,
                    Some("ligature") => GlyphCategory::Ligature,
                    _ => GlyphCategory::Base,
                }
            } else {
                GlyphCategory::Base
            };
            let production_name = psnames
                .and_then(|x| x.get(&glyphname))
                .and_then(|x| x.as_string())
                .map(|x| x.to_string());
            font.glyphs.push(Glyph {
                name: glyphname.to_string(),
                category: cat,
                production_name,
                codepoints: glyph.codepoints.iter().map(|x| x as u32).collect(),
                layers: vec![],
                exported: !skipped.contains(&glyphname),
                direction: None,
                formatspecific: Default::default(),
            })
        }
    }
    add_uvs_sequences(font, ufo);
}

fn add_uvs_sequences(font: &mut Font, ufo: &norad::Font) {
    if let Some(uvs) = ufo
        .lib
        .get("public.unicodeVariationSequences")
        .and_then(|x| x.as_dictionary())
    {
        // Lasciate ogne speranza, voi ch'intrate
        for (selector_s, records_plist) in uvs.iter() {
            if let Ok(selector) = u32::from_str_radix(selector_s, 16) {
                if let Some(records) = records_plist.as_dictionary() {
                    for (codepoint_s, glyphname_plist) in records {
                        if let Ok(codepoint) = u32::from_str_radix(codepoint_s, 16) {
                            if let Some(glyphname) = glyphname_plist.as_string() {
                                font.variation_sequences
                                    .insert((selector, codepoint), glyphname.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_roundtrip() {
        let there = crate::load("resources/Test1.ufo").unwrap();
        assert!(there.masters.len() == 1);
        let backagain = as_norad(&there).unwrap();
        let once_more = norad::Font::load("resources/Test1.ufo").unwrap();
        assert_eq!(there.glyphs.len(), backagain.default_layer().len());
        assert_eq!(
            backagain.default_layer().len(),
            once_more.default_layer().len()
        );
        assert_eq!(backagain.layers, once_more.layers);
        assert_eq!(backagain.lib, once_more.lib);
        assert_eq!(backagain.groups, once_more.groups);
        assert_eq!(backagain.kerning, once_more.kerning);
        // assert_eq!(backagain.font_info, once_more.font_info);
        // assert_eq!(backagain.features, once_more.features);
        assert_eq!(backagain.data, once_more.data);
        assert_eq!(backagain.images, once_more.images);
    }
}
