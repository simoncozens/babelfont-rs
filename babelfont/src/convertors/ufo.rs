use crate::{
    common::decomposition::DecomposedAffine, features::Features, glyph::GlyphCategory,
    BabelfontError, Component, Font, Glyph, Layer, LayerType, Master, MetricType, Node, Path,
    Shape,
};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use fontdrasil::{coords::Location, types::Tag};
use indexmap::IndexMap;
use paste::paste;
use smol_str::{SmolStr, ToSmolStr};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path as FsPath, PathBuf},
    str::FromStr,
    time::SystemTime,
};

/// Key for storing norad lib data in FormatSpecific
pub const KEY_LIB: &str = "norad.lib";
/// Key for storing norad groups in FormatSpecific
pub const KEY_GROUPS: &str = "norad.groups";
// These aren't pub because we just use them to get stuff out of the ufo's lib, we don't store them ourselves
const KEY_CATEGORIES: &str = "public.openTypeCategories";
const KEY_PSNAMES: &str = "public.postscriptNames";
const KEY_SKIP_EXPORT: &str = "public.skipExportGlyphs";
// Format-specific names
/// Key for storing style map family name in FormatSpecific
pub const KEY_STYLE_MAP_FAMILY_NAME: &str = "ufo.styleMapFamilyName";
/// Key for storing style map style name in FormatSpecific
pub const KEY_STYLE_MAP_STYLE_NAME: &str = "ufo.styleMapStyleName";
/// Key for storing style name in FormatSpecific
pub const KEY_STYLE_NAME: &str = "ufo.styleName";

pub(crate) fn stash_lib(lib: Option<&norad::Plist>) -> crate::common::FormatSpecific {
    let mut fs = crate::common::FormatSpecific::default();
    if let Some(lib) = lib {
        if lib.is_empty() {
            return fs;
        }
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

/// Load a UFO font from a file path
pub fn load<T: AsRef<std::path::Path>>(path: T) -> Result<Font, BabelfontError> {
    let created_time: Option<DateTime<Utc>> = stat(path.as_ref()).map(DateTime::<Utc>::from);
    let ufo = norad::Font::load(&path)?;
    font_from_norad(path.as_ref(), created_time, ufo)
}

/// Load a UFO font from in-memory entries keyed by relative path.
///
/// `path` is the virtual UFO root path (for example `MyFont.ufo` or `sources/MyFont.ufo`).
pub fn load_entries(path: PathBuf, entries: &HashMap<String, String>) -> Result<Font, BabelfontError> {
    let ufo = load_norad_from_entries(&path, entries)?;
    font_from_norad(&path, None, ufo)
}

pub(crate) fn load_norad_from_entries(
    path: &FsPath,
    entries: &HashMap<String, String>,
) -> Result<norad::Font, BabelfontError> {
    norad::Font::load_entries(path, entries).map_err(Into::into)
}

fn font_from_norad(
    path: &FsPath,
    created_time: Option<DateTime<Utc>>,
    ufo: norad::Font,
) -> Result<Font, BabelfontError> {
    let mut font = Font::new();
    font.format_specific = stash_lib(Some(&ufo.lib));
    load_glyphs(&mut font, &ufo);
    let info = &ufo.font_info;
    load_font_info(&mut font, info, created_time);
    let mut master = Master::new(
        info.family_name
            .as_ref()
            .unwrap_or(&"Unnamed master".to_string()),
        uuid::Uuid::new_v4().to_string(),
        Location::new(),
    );
    load_master_info(&mut master, info);
    load_kerning(&mut master, &ufo.kerning);
    (font.first_kern_groups, font.second_kern_groups) = load_kern_groups(&ufo.groups);

    for layer in ufo.iter_layers() {
        for g in font.glyphs.iter_mut() {
            if let Some(norad_glyph) = layer.get_glyph(g.name.as_str()) {
                g.layers.push(norad_glyph_to_babelfont_layer(
                    norad_glyph,
                    layer,
                    &master.id,
                ))
            }
        }
    }
    font.features = Features::from_fea(&ufo.features);
    font.features.include_paths.push(
        path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf(),
    );
    // Potentially also within the UFO itself? I think this unclear.

    font.masters.push(master);

    Ok(font)
}

/// Convert a Babelfont Font to a norad UFO font
///
/// This is currently unfinished and may not preserve all data.
pub fn as_norad(font: &Font) -> Result<norad::Font, BabelfontError> {
    let mut ufo = norad::Font::new();
    // Move some things into lib key before serializing it:
    // exports
    // categories
    ufo.lib = font
        .format_specific
        .get(KEY_LIB)
        .and_then(|x| serde_json::from_value::<norad::Plist>(x.clone()).ok())
        .unwrap_or_default();
    let first_master = font
        .masters
        .first()
        .ok_or(BabelfontError::NoDefaultMaster)?;
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

    save_kerning(&mut ufo.kerning, &first_master.kerning)?;
    save_info(&mut ufo.font_info, font);
    save_kern_groups(
        &mut ufo.groups,
        &font.first_kern_groups,
        &font.second_kern_groups,
    )?;

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
    if let Some(lib) = layer.format_specific.get(KEY_LIB) {
        norad_glyph.lib = serde_json::from_value::<norad::Plist>(lib.clone()).unwrap_or_default();
    }
    Ok(norad_glyph)
}

pub(crate) fn norad_glyph_to_babelfont_layer(
    glyph: &norad::Glyph,
    layer: &norad::Layer,
    master_id: &str,
) -> Layer {
    let mut l = Layer::new(glyph.width as f32);
    if layer.is_default() {
        l.name = None;
        l.master = LayerType::DefaultForMaster(master_id.to_string());
    } else {
        l.name = Some(layer.name().to_string());
        l.master = LayerType::AssociatedWithMaster(layer.name().to_string());
    }
    l.id = Some(master_id.to_string());
    if !glyph.lib.is_empty() {
        l.format_specific = stash_lib(Some(&glyph.lib));
    }

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
    // norad uses the convention:
    //   x' = x_scale * x + xy_scale * y + x_offset
    //   y' = yx_scale * x + y_scale * y + y_offset
    // kurbo::Affine::new expects [xx, yx, xy, yy, dx, dy]
    let affine = kurbo::Affine::new([
        t.x_scale, t.yx_scale, t.xy_scale, t.y_scale, t.x_offset, t.y_offset,
    ]);
    // UFO uses the same transform representation as Fontra
    let decomposed: DecomposedAffine = affine.into();
    Component {
        reference: c.base.to_smolstr(),
        transform: decomposed,
        format_specific: stash_lib(c.lib()),
        location: IndexMap::new(),
    }
}

pub(crate) fn save_component(c: &Component) -> Result<norad::Component, BabelfontError> {
    let affine = c.transform.as_affine();
    let mut t = affine.as_coeffs();
    // Clamp floating point noise to zero to avoid epsilon drift in roundtrips
    for v in t.iter_mut() {
        if v.abs() < 1e-7 {
            *v = 0.0;
        }
    }
    let mut comp = norad::Component::new(
        norad::Name::new(c.reference.as_str())?,
        norad::AffineTransform {
            // Map back from kurbo coefficients [xx, yx, xy, yy, dx, dy]
            x_scale: t[0],
            xy_scale: t[2],
            yx_scale: t[1],
            y_scale: t[3],
            x_offset: t[4],
            y_offset: t[5],
        },
        None,
    );
    if let Some(lib) = c
        .format_specific
        .get(KEY_LIB)
        .and_then(|x| serde_json::from_value(x.clone()).ok())
    {
        comp.replace_lib(lib);
    }
    Ok(comp)
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
    let mut c = norad::Contour::new(points, None);
    if let Some(lib) = p
        .format_specific
        .get(KEY_LIB)
        .and_then(|x| serde_json::from_value(x.clone()).ok())
    {
        c.replace_lib(lib);
    }
    c
}

pub(crate) fn save_kerning(
    norad_kerning: &mut norad::Kerning,
    babelfont_kerning: &IndexMap<(SmolStr, SmolStr), i16>,
) -> Result<(), BabelfontError> {
    for ((left, right), value) in babelfont_kerning.iter() {
        let left_key = if left.starts_with('@') {
            "public.kern1.".to_string() + left.trim_start_matches('@')
        } else {
            left.to_string()
        };
        let right_key = if right.starts_with('@') {
            "public.kern2.".to_string() + right.trim_start_matches('@')
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

pub(crate) fn save_kern_groups(
    norad_groups: &mut norad::Groups,
    first: &IndexMap<SmolStr, Vec<SmolStr>>,
    second: &IndexMap<SmolStr, Vec<SmolStr>>,
) -> Result<(), BabelfontError> {
    // Prefix with public.kern1. and public.kern2.
    for (group_name, glyphs) in first.iter() {
        let norad_group_name = norad::Name::new(&format!("public.kern1.{}", group_name))?;
        let norad_glyph_names: Vec<norad::Name> = glyphs
            .iter()
            .map(|g| norad::Name::new(g.as_str()))
            .collect::<Result<_, _>>()?;
        norad_groups.insert(norad_group_name, norad_glyph_names);
    }
    for (group_name, glyphs) in second.iter() {
        let norad_group_name = norad::Name::new(&format!("public.kern2.{}", group_name))?;
        let norad_glyph_names: Vec<norad::Name> = glyphs
            .iter()
            .map(|g| norad::Name::new(g.as_str()))
            .collect::<Result<_, _>>()?;
        norad_groups.insert(norad_group_name, norad_glyph_names);
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
    info.open_type_head_flags = font.custom_ot_values.head_flags.map(to_bitarray);
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
    info.open_type_name_preferred_family_name = font
        .names
        .wws_family_name
        .get_default()
        .map(|x| x.to_string());
    info.open_type_name_preferred_subfamily_name = font
        .names
        .wws_subfamily_name
        .get_default()
        .map(|x| x.to_string());
    info.open_type_name_sample_text = font.names.sample_text.get_default().map(|x| x.to_string());
    info.open_type_name_unique_id = font.names.unique_id.get_default().map(|x| x.to_string());
    info.open_type_name_version = font.names.version.get_default().map(|x| x.to_string());
    // XXX lots more
    info.postscript_font_name = font
        .names
        .postscript_name
        .get_default()
        .map(|x| x.to_string());
    // and more
    let codepage_ranges: Vec<u8> =
        to_bitarray(font.custom_ot_values.os2_code_page_range1.unwrap_or(0))
            .iter()
            .copied()
            .chain(
                to_bitarray(font.custom_ot_values.os2_code_page_range2.unwrap_or(0))
                    .iter()
                    .map(|&bit| bit + 32),
            )
            .collect();
    if !codepage_ranges.is_empty() {
        info.open_type_os2_code_page_ranges = Some(codepage_ranges);
    }
    info.open_type_os2_selection = font.custom_ot_values.os2_fs_selection.map(to_bitarray);
    info.open_type_os2_type = font.custom_ot_values.os2_fs_type.map(to_bitarray);
    info.open_type_os2_typo_ascender = get_metric(MetricType::TypoAscender).map(|x| x as i32);
    info.open_type_os2_typo_descender = get_metric(MetricType::TypoDescender).map(|x| x as i32);
    info.open_type_os2_typo_line_gap = get_metric(MetricType::TypoLineGap).map(|x| x as i32);
    let mut bit_array = vec![];
    let unicode_range_1 = font.custom_ot_values.os2_unicode_range1.unwrap_or(0) as u64;
    bit_array.extend(to_bitarray(unicode_range_1));
    let unicode_range_2 = font.custom_ot_values.os2_unicode_range2.unwrap_or(0) as u64;
    bit_array.extend(to_bitarray(unicode_range_2).iter().map(|&bit| bit + 32));
    let unicode_range_3 = font.custom_ot_values.os2_unicode_range3.unwrap_or(0) as u64;
    bit_array.extend(to_bitarray(unicode_range_3).iter().map(|&bit| bit + 64));
    let unicode_range_4 = font.custom_ot_values.os2_unicode_range4.unwrap_or(0) as u64;
    bit_array.extend(to_bitarray(unicode_range_4).iter().map(|&bit| bit + 96));
    if !bit_array.is_empty() {
        info.open_type_os2_unicode_ranges = Some(bit_array);
    }
    info.open_type_os2_panose =
        font.custom_ot_values
            .os2_panose
            .map(|x| norad::fontinfo::Os2Panose {
                family_type: x[0].into(),
                serif_style: x[1].into(),
                weight: x[2].into(),
                proportion: x[3].into(),
                contrast: x[4].into(),
                stroke_variation: x[5].into(),
                arm_style: x[6].into(),
                letterform: x[7].into(),
                midline: x[8].into(),
                x_height: x[9].into(),
            });
    info.open_type_os2_vendor_id = font.custom_ot_values.os2_vendor_id.map(|x| x.to_string());
    info.open_type_os2_win_ascent = get_metric(MetricType::WinAscent).map(|x| x as u32);
    info.open_type_os2_win_descent = get_metric(MetricType::WinDescent).map(|x| x as u32);
    info.postscript_underline_position = get_metric(MetricType::UnderlinePosition);
    info.postscript_underline_thickness = get_metric(MetricType::UnderlineThickness);
    info.postscript_other_blues = font.custom_ot_values.cff_other_blues.clone();
    info.postscript_blue_values = font.custom_ot_values.cff_blue_values.clone();
    info.postscript_family_blues = font.custom_ot_values.cff_family_blues.clone();
    info.postscript_family_other_blues = font.custom_ot_values.cff_family_other_blues.clone();
    info.postscript_stem_snap_h = font.custom_ot_values.cff_stem_snap_h.clone();
    info.postscript_stem_snap_v = font.custom_ot_values.cff_stem_snap_v.clone();
    info.style_map_family_name = font
        .format_specific
        .get(KEY_STYLE_MAP_FAMILY_NAME)
        .and_then(|x| x.as_str())
        .map(|x| x.to_string());
    info.style_map_style_name = font
        .format_specific
        .get(KEY_STYLE_MAP_STYLE_NAME)
        .and_then(|x| x.as_str())
        .map(|x| match x {
            "Regular" => norad::fontinfo::StyleMapStyle::Regular,
            "Bold" => norad::fontinfo::StyleMapStyle::Bold,
            "Italic" => norad::fontinfo::StyleMapStyle::Italic,
            "Bold Italic" => norad::fontinfo::StyleMapStyle::BoldItalic,
            _ => norad::fontinfo::StyleMapStyle::Regular,
        });
    info.style_name = font
        .format_specific
        .get(KEY_STYLE_NAME)
        .and_then(|x| x.as_str())
        .map(|x| x.to_string());
    info.trademark = font.names.trademark.get_default().map(|x| x.to_string());
    info.units_per_em = Some((font.upm as u32).into());
    info.version_major = Some(font.version.0 as i32);
    info.version_minor = Some(font.version.1 as u32);
    // XXX WOFF
    info.x_height = get_metric(MetricType::XHeight);
}

macro_rules! load_metric {
    ($info:ident, $metrics:ident, $field:ident, $metric_type:expr) => {
        if let Some(v) = $info.$field {
            $metrics.insert($metric_type, v as i32);
        }
    };
}

pub(crate) fn load_master_info(master: &mut Master, info: &norad::FontInfo) {
    let metrics = &mut master.metrics;
    load_metric!(info, metrics, ascender, MetricType::Ascender);
    load_metric!(info, metrics, cap_height, MetricType::CapHeight);
    load_metric!(info, metrics, descender, MetricType::Descender);
    load_metric!(info, metrics, italic_angle, MetricType::ItalicAngle);
    load_metric!(info, metrics, x_height, MetricType::XHeight);
    load_metric!(
        info,
        metrics,
        open_type_hhea_ascender,
        MetricType::HheaAscender
    );
    load_metric!(
        info,
        metrics,
        open_type_hhea_descender,
        MetricType::HheaDescender
    );
    load_metric!(
        info,
        metrics,
        open_type_hhea_line_gap,
        MetricType::HheaLineGap
    );
    load_metric!(
        info,
        metrics,
        open_type_hhea_caret_offset,
        MetricType::HheaCaretOffset
    );
    load_metric!(
        info,
        metrics,
        open_type_os2_typo_ascender,
        MetricType::TypoAscender
    );
    load_metric!(
        info,
        metrics,
        open_type_os2_typo_descender,
        MetricType::TypoDescender
    );
    load_metric!(
        info,
        metrics,
        open_type_os2_typo_line_gap,
        MetricType::TypoLineGap
    );
    load_metric!(
        info,
        metrics,
        open_type_os2_win_ascent,
        MetricType::WinAscent
    );
    load_metric!(
        info,
        metrics,
        open_type_os2_win_descent,
        MetricType::WinDescent
    );
    load_metric!(
        info,
        metrics,
        postscript_underline_position,
        MetricType::UnderlinePosition
    );
    load_metric!(
        info,
        metrics,
        postscript_underline_thickness,
        MetricType::UnderlineThickness
    );
    if let Some(v) = &info.guidelines {
        for g in v.iter() {
            master.guides.push(g.into())
        }
    }
}

macro_rules! copy_name {
    ($font:ident, $ufo:ident, $field:ident) => {
        paste! {
            if let Some(v) = &$ufo.[<open_type_name_ $field>] {
                $font.names.$field = v.into();
            }
        }
    };
}

// The distinction between this and load_master_info is that this is font-wide info;
// in a .designspace loader, this would be called once per font, while load_master_info
// would be called once per source.
pub(crate) fn load_font_info(
    font: &mut Font,
    info: &norad::FontInfo,
    created: Option<DateTime<Utc>>,
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
            .map(|x| chrono::Utc.from_local_datetime(&x).single())
        {
            font.date = date;
        } else {
            font.date = created.unwrap_or_else(chrono::Utc::now);
        }
    }
    font.custom_ot_values.head_flags = info.open_type_head_flags.as_ref().map(|x| from_bitarray(x));
    font.custom_ot_values.head_lowest_rec_ppem =
        info.open_type_head_lowest_rec_ppem.map(|x| x as u16);
    font.custom_ot_values.os2_fs_selection = info
        .open_type_os2_selection
        .as_ref()
        .map(|x| from_bitarray(x));
    font.custom_ot_values.os2_fs_type = info.open_type_os2_type.as_ref().map(|x| from_bitarray(x));
    if let Some(v) = &info
        .open_type_os2_code_page_ranges
        .as_ref()
        .map(|x| from_bitarray(x))
    {
        let v: u64 = *v;
        // Split into top and bottom 32 bits
        font.custom_ot_values.os2_code_page_range1 = Some(v as u32);
        font.custom_ot_values.os2_code_page_range2 = Some((v >> 32) as u32);
    }
    if let Some(v) = &info.open_type_os2_unicode_ranges {
        // This one's a bit trickier since there are 128 bits split over four u32s.
        let mut ur1 = 0;
        let mut ur2 = 0;
        let mut ur3 = 0;
        let mut ur4 = 0;
        for bit in v.iter() {
            match bit {
                0..=31 => ur1 |= 1 << bit,
                32..=63 => ur2 |= 1 << (bit - 32),
                64..=95 => ur3 |= 1 << (bit - 64),
                96..=127 => ur4 |= 1 << (bit - 96),
                _ => {}
            }
        }
        font.custom_ot_values.os2_unicode_range1 = Some(ur1);
        font.custom_ot_values.os2_unicode_range2 = Some(ur2);
        font.custom_ot_values.os2_unicode_range3 = Some(ur3);
        font.custom_ot_values.os2_unicode_range4 = Some(ur4);
    }
    font.custom_ot_values.cff_blue_values = info.postscript_blue_values.clone();
    font.custom_ot_values.cff_other_blues = info.postscript_other_blues.clone();
    font.custom_ot_values.cff_family_blues = info.postscript_family_blues.clone();
    font.custom_ot_values.cff_family_other_blues = info.postscript_family_other_blues.clone();
    font.custom_ot_values.cff_stem_snap_h = info.postscript_stem_snap_h.clone();
    font.custom_ot_values.cff_stem_snap_v = info.postscript_stem_snap_v.clone();
    font.custom_ot_values.os2_vendor_id = info
        .open_type_os2_vendor_id
        .as_ref()
        .and_then(|x| Tag::from_str(x).ok());
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
    if let Some(p) = &info.style_map_family_name {
        font.names.preferred_subfamily_name = p.into();
    }
    copy_name!(font, info, description);
    copy_name!(font, info, designer_url);
    copy_name!(font, info, designer);
    copy_name!(font, info, license);
    copy_name!(font, info, license_url);
    copy_name!(font, info, manufacturer);
    copy_name!(font, info, manufacturer_url);
    copy_name!(font, info, sample_text);
    copy_name!(font, info, unique_id);
    copy_name!(font, info, version);
    if let Some(smfn) = &info.style_map_family_name {
        font.format_specific.insert(
            KEY_STYLE_MAP_FAMILY_NAME.into(),
            serde_json::to_value(smfn).unwrap_or_default(),
        );
    }
    if let Some(smsn) = &info.style_map_style_name {
        font.format_specific.insert(
            KEY_STYLE_MAP_STYLE_NAME.into(),
            match smsn {
                norad::fontinfo::StyleMapStyle::Regular => "Regular",
                norad::fontinfo::StyleMapStyle::Bold => "Bold",
                norad::fontinfo::StyleMapStyle::Italic => "Italic",
                norad::fontinfo::StyleMapStyle::BoldItalic => "Bold Italic",
            }
            .into(),
        );
    }
    if let Some(stylename) = &info.style_name {
        font.format_specific
            .insert(KEY_STYLE_NAME.into(), stylename.clone().into());
    }
    if let Some(v) = info.open_type_name_preferred_family_name.as_ref() {
        font.names.wws_family_name = v.into(); // Is this the right place?
    }
    if let Some(v) = info.open_type_name_preferred_subfamily_name.as_ref() {
        font.names.wws_subfamily_name = v.into();
    }
    if let Some(panose) = &info.open_type_os2_panose {
        font.custom_ot_values.os2_panose = Some([
            panose.family_type.try_into().unwrap_or(0),
            panose.serif_style.try_into().unwrap_or(0),
            panose.weight.try_into().unwrap_or(0),
            panose.proportion.try_into().unwrap_or(0),
            panose.contrast.try_into().unwrap_or(0),
            panose.stroke_variation.try_into().unwrap_or(0),
            panose.arm_style.try_into().unwrap_or(0),
            panose.letterform.try_into().unwrap_or(0),
            panose.midline.try_into().unwrap_or(0),
            panose.x_height.try_into().unwrap_or(0),
        ]);
    }
}

pub(crate) fn load_kerning(master: &mut Master, kerning: &norad::Kerning) {
    for (left, right_dict) in kerning.iter() {
        for (right, value) in right_dict.iter() {
            let left_maybe_group = if let Some(group) = left.strip_prefix("public.kern1.") {
                format!("@{:}", group)
            } else {
                left.to_string()
            };
            let right_maybe_group = if let Some(group) = right.strip_prefix("public.kern2.") {
                format!("@{:}", group)
            } else {
                right.to_string()
            };
            master.kerning.insert(
                (left_maybe_group.into(), right_maybe_group.into()),
                *value as i16,
            );
        }
    }
}

fn from_bitarray<T>(v: &[u8]) -> T
where
    T: num_traits::PrimInt + num_traits::FromPrimitive,
{
    let mut result = T::zero();
    for bit in v.iter() {
        result = result | (T::one() << usize::from(*bit));
    }
    result
}

fn to_bitarray<T>(v: T) -> Vec<u8>
where
    T: num_traits::PrimInt,
{
    let mut bits = vec![];
    let mut bit_index = 0;
    let mut value = v;
    while !value.is_zero() {
        if (value & T::one()) == T::one() {
            bits.push(bit_index);
        }
        value = value >> 1;
        bit_index += 1;
    }
    bits
}
pub(crate) fn load_kern_groups(
    groups: &norad::Groups,
) -> (
    IndexMap<SmolStr, Vec<SmolStr>>,
    IndexMap<SmolStr, Vec<SmolStr>>,
) {
    let mut first: IndexMap<SmolStr, Vec<SmolStr>> = IndexMap::new();
    let mut second: IndexMap<SmolStr, Vec<SmolStr>> = IndexMap::new();
    for (name, members) in groups.iter() {
        if let Some(first_name) = name.strip_prefix("public.kern1.") {
            first.insert(
                SmolStr::from(first_name),
                members.iter().map(|x| SmolStr::from(x.as_str())).collect(),
            );
        } else if let Some(second_name) = name.strip_prefix("public.kern2.") {
            second.insert(
                SmolStr::from(second_name),
                members.iter().map(|x| SmolStr::from(x.as_str())).collect(),
            );
        }
    }
    (first, second)
}

pub(crate) fn load_glyphs(font: &mut Font, ufo: &norad::Font) {
    let categories = ufo.lib.get(KEY_CATEGORIES).and_then(|x| x.as_dictionary());
    let psnames = ufo.lib.get(KEY_PSNAMES).and_then(|x| x.as_dictionary());
    let skipped: HashSet<String> = ufo
        .lib
        .get(KEY_SKIP_EXPORT)
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
                .map(|x| x.into());
            font.glyphs.push(Glyph {
                name: SmolStr::from(glyphname.as_str()),
                category: cat,
                production_name,
                codepoints: glyph.codepoints.iter().map(|x| x as u32).collect(),
                layers: vec![],
                exported: !skipped.contains(&glyphname),
                direction: None,
                format_specific: Default::default(),
                component_axes: Default::default(),
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
                                    .insert((selector, codepoint), glyphname.into());
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
        let there = crate::load("resources/NotoSans-LightItalic.ufo").unwrap();
        assert!(there.masters.len() == 1);
        let backagain = as_norad(&there).unwrap();
        let once_more = norad::Font::load("resources/NotoSans-LightItalic.ufo").unwrap();
        assert_eq!(there.glyphs.len(), backagain.default_layer().len());
        assert_eq!(
            backagain.default_layer().len(),
            once_more.default_layer().len()
        );
        let backagain_layer = backagain.default_layer();
        let once_more_layer = once_more.default_layer();
        for name in there.glyphs.iter().map(|x| x.name.as_str()) {
            let g1 = backagain_layer.get_glyph(name).unwrap();
            let g2 = once_more_layer.get_glyph(name).unwrap();
            assert_eq!(g1, g2, "Glyph {} differs", name);
        }

        assert_eq!(
            there.custom_ot_values.os2_unicode_range2,
            Some(
                // 0,1,2,3,4,13,30 should be set
                1 << 0 | 1 << 1 | 1 << 2 | 1 << 3 | 1 << 4 | 1 << 13 | 1 << 30
            )
        );

        assert_eq!(backagain.lib, once_more.lib);
        assert_eq!(backagain.groups, once_more.groups);
        assert_eq!(backagain.kerning, once_more.kerning);
        assert_eq!(backagain.features.trim(), once_more.features.trim());
        assert_eq!(backagain.data, once_more.data);
        assert_eq!(backagain.images, once_more.images);
        assert_eq!(backagain.font_info, once_more.font_info);
    }

    #[test]
    fn test_load_entries_ufo() {
        let mut entries = HashMap::new();
        entries.insert(
            "metainfo.plist".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>creator</key><string>org.test</string><key>formatVersion</key><integer>3</integer></dict></plist>"#
                .to_string(),
        );
        entries.insert(
            "fontinfo.plist".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>familyName</key><string>TestFamily</string><key>styleName</key><string>Regular</string></dict></plist>"#
                .to_string(),
        );
        entries.insert(
            "layercontents.plist".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><array><array><string>public.default</string><string>glyphs</string></array></array></plist>"#
                .to_string(),
        );
        entries.insert(
            "glyphs/contents.plist".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>A</key><string>A_.glif</string></dict></plist>"#
                .to_string(),
        );
        entries.insert(
            "glyphs/A_.glif".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<glyph name="A" format="2">
  <advance width="600"/>
</glyph>"#
                .to_string(),
        );

        let font = load_entries(PathBuf::from("Test.ufo"), &entries).unwrap();
        assert_eq!(font.glyphs.len(), 1);
        assert_eq!(font.glyphs[0].name, "A");
    }

    #[test]
    fn test_load_entries_ufo_negative_os2_win_descent() {
        let mut entries = HashMap::new();
        entries.insert(
            "metainfo.plist".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>creator</key><string>org.test</string><key>formatVersion</key><integer>3</integer></dict></plist>"#
                .to_string(),
        );
        entries.insert(
            "fontinfo.plist".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>familyName</key><string>TestFamily</string><key>styleName</key><string>Regular</string><key>openTypeOS2WinDescent</key><integer>-279</integer></dict></plist>"#
                .to_string(),
        );
        entries.insert(
            "layercontents.plist".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><array><array><string>public.default</string><string>glyphs</string></array></array></plist>"#
                .to_string(),
        );
        entries.insert(
            "glyphs/contents.plist".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>A</key><string>A_.glif</string></dict></plist>"#
                .to_string(),
        );
        entries.insert(
            "glyphs/A_.glif".to_string(),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<glyph name="A" format="2">
  <advance width="600"/>
</glyph>"#
                .to_string(),
        );

        let ufo = load_norad_from_entries(std::path::Path::new("Test.ufo"), &entries).unwrap();
        assert_eq!(ufo.font_info.open_type_os2_win_descent, Some(279));
    }
}
