//! Serialization of the `Font` model back to FontForge SFD.
//!
//! Functions are grouped in the same order the model is walked, mirroring
//! `super`: the font as a whole (header, names, OS/2 values), masters and their
//! metrics, glyphs, layers, anchors and shapes, paths and components, kerning,
//! and finally OpenType features.

use std::collections::HashMap;

use smol_str::SmolStr;

use crate::{
    convertors::fontforge::{
        layer_is_quadratic,
        layerregistry::LayerRegistry,
        offsetmetrics::compute_offset_delta,
        stringhelpers::{escape_quoted, escape_sfd_line, fmt_num, sanitize_unquoted},
    },
    BabelfontError, Component, Font, Glyph, GlyphCategory, Layer, MetricType, Node, NodeType, Path,
    Shape,
};

// ===========================================================================
// The font as a whole
// ===========================================================================

/// Serialize a Babelfont Font into a FontForge SFD text representation.
pub fn to_str(font: &Font) -> Result<String, BabelfontError> {
    let mut out: Vec<String> = Vec::new();
    let default_master_id = font
        .masters
        .first()
        .map(|m| m.id.as_str())
        .unwrap_or("default");

    let layer_registry = LayerRegistry::from_font(font, default_master_id);
    let glyph_order: Vec<String> = font.glyphs.iter().map(|g| g.name.to_string()).collect();
    let glyph_index: HashMap<SmolStr, usize> = font
        .glyphs
        .iter()
        .enumerate()
        .map(|(ix, g)| (g.name.clone(), ix))
        .collect();
    let explicit_kerns = collect_explicit_kerns(font, &glyph_index);

    emit_font_header(&mut out, font, &layer_registry)?;
    emit_font_level_kerning(&mut out, font, &glyph_order, &glyph_index);
    emit_features(&mut out, font);

    out.push(format!(
        "BeginChars: {} {}",
        begin_chars_encoding_slots(font, &glyph_order),
        begin_chars_glyph_count(font, &glyph_order)
    ));
    if !font.glyphs.is_empty()
        && font
            .format_specific
            .get("sfd.beginchars_blank_line")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    {
        out.push(String::new());
    }
    for (gid, glyph) in font.glyphs.iter().enumerate() {
        emit_glyph(
            &mut out,
            glyph,
            gid,
            &layer_registry,
            default_master_id,
            &glyph_index,
            explicit_kerns.get(&glyph.name),
        )?;
        if gid + 1 < font.glyphs.len() {
            out.push(String::new());
        }
    }
    out.push("EndChars".to_string());
    out.push("EndSplineFont".to_string());

    Ok(out.join("\n") + "\n")
}

fn emit_font_header(
    out: &mut Vec<String>,
    font: &Font,
    layer_registry: &LayerRegistry,
) -> Result<(), BabelfontError> {
    let mut state = HeaderEmitState::new(font, layer_registry);
    let emit_layer_header = font
        .format_specific
        .get("sfd.has_header_layers")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || layer_registry.layer_count > 2;

    // Follow FontForge's current metadata dump ordering from sfd.cpp.
    for key in [
        "SplineFontDB",
        "FontName",
        "FullName",
        "FamilyName",
        "Weight",
        "Copyright",
        "UComments",
        "Comments",
        "FontLog",
        "Version",
        "FONDName",
        "DefaultBaseFilename",
        "StrokeWidth",
        "ItalicAngle",
        "UnderlinePosition",
        "UnderlineWidth",
        "Ascent",
        "Descent",
        "InvalidEm",
        "sfntRevision",
        "woffMajor",
        "woffMinor",
        "woffMetadata",
        "UFOAscent",
        "UFODescent",
        "LayerCount",
        "Layer",
        "PreferredKerning",
        "StrokedFont",
        "MultiLayer",
        "HasVMetrics",
        "NeedsXUIDChange",
        "XUID",
        "UniqueID",
        "UseXUID",
        "UseUniqueID",
        "BaseHoriz",
        "BaseVert",
        "StyleMap",
        "FSType",
        "OS2Version",
        "OS2_WeightWidthSlopeOnly",
        "OS2_UseTypoMetrics",
        "CreationTime",
        "ModificationTime",
        "PfmFamily",
        "TTFWeight",
        "TTFWidth",
        "LineGap",
        "VLineGap",
        "Panose",
        "OS2TypoAscent",
        "OS2TypoAOffset",
        "OS2TypoDescent",
        "OS2TypoDOffset",
        "OS2TypoLinegap",
        "OS2WinAscent",
        "OS2WinAOffset",
        "OS2WinDescent",
        "OS2WinDOffset",
        "HheadAscent",
        "HheadAOffset",
        "HheadDescent",
        "HheadDOffset",
        "OS2SubXSize",
        "OS2SubYSize",
        "OS2SubXOff",
        "OS2SubYOff",
        "OS2SupXSize",
        "OS2SupYSize",
        "OS2SupXOff",
        "OS2SupYOff",
        "OS2StrikeYSize",
        "OS2StrikeYPos",
        "OS2CapHeight",
        "OS2XHeight",
        "OS2FamilyClass",
        "OS2Vendor",
        "MarkAttachClasses",
        "DEI",
        "LangName",
        "Encoding",
        "UnicodeInterp",
        "NameList",
        "DisplaySize",
        "AntiAlias",
        "FitToEm",
        "WinInfo",
        "BeginPrivate",
        "Grid",
    ] {
        if (key == "LayerCount" || key == "Layer") && !emit_layer_header {
            continue;
        }
        emit_header_key(out, font, layer_registry, key, &mut state)?;
    }

    while state.comment_index < comment_entries(font).len() {
        let entry = &comment_entries(font)[state.comment_index];
        state.comment_index += 1;
        out.push(format!("{}:{}", entry.0, entry.1));
    }

    while emit_layer_header && state.layer_index < layer_registry.defs.len() {
        emit_header_key(out, font, layer_registry, "Layer", &mut state)?;
    }

    emit_font_passthrough_keys_remaining(out, font, &mut state);
    Ok(())
}

struct HeaderEmitState {
    emitted: HashMap<String, bool>,
    layer_index: usize,
    comment_index: usize,
}

impl HeaderEmitState {
    fn new(_font: &Font, _layer_registry: &LayerRegistry) -> Self {
        Self {
            emitted: HashMap::new(),
            layer_index: 0,
            comment_index: 0,
        }
    }

    fn is_emitted(&self, key: &str) -> bool {
        self.emitted.get(key).copied().unwrap_or(false)
    }

    fn mark_emitted(&mut self, key: &str) {
        self.emitted.insert(key.to_string(), true);
    }
}

fn emit_header_key(
    out: &mut Vec<String>,
    font: &Font,
    layer_registry: &LayerRegistry,
    key: &str,
    state: &mut HeaderEmitState,
) -> Result<(), BabelfontError> {
    match key {
        "SplineFontDB" if !state.is_emitted(key) => {
            out.push(format!(
                "SplineFontDB: {}",
                font.format_specific
                    .get(super::HEADER_VERSION_KEY)
                    .and_then(|v| v.as_str())
                    .unwrap_or("3.0")
            ));
            state.mark_emitted(key);
        }
        "FontName" if !state.is_emitted(key) => {
            if let Some(line) = font_name_line(font) {
                out.push(line);
            }
            state.mark_emitted(key);
        }
        "FullName" if !state.is_emitted(key) => {
            if let Some(line) = full_name_line(font) {
                out.push(line);
            }
            state.mark_emitted(key);
        }
        "FamilyName" if !state.is_emitted(key) => {
            if let Some(line) = family_name_line(font) {
                out.push(line);
            }
            state.mark_emitted(key);
        }
        "Weight" if !state.is_emitted(key) => {
            if let Some(line) = weight_line(font) {
                out.push(line);
            }
            state.mark_emitted(key);
        }
        "Copyright" if !state.is_emitted(key) => {
            if let Some(line) = copyright_line(font) {
                out.push(line);
            }
            state.mark_emitted(key);
        }
        "Comments" | "UComments" | "FontLog" => {
            if let Some(line) = next_comment_line(font, key, state) {
                out.push(line);
            }
        }
        "Version" if !state.is_emitted(key) => {
            out.push(version_line(font));
            state.mark_emitted(key);
        }
        "UniqueID" if !state.is_emitted(key) => {
            if let Some(line) = unique_id_line(font) {
                out.push(line);
            }
            state.mark_emitted(key);
        }
        "LayerCount" if !state.is_emitted(key) => {
            out.push(format!("LayerCount: {}", layer_registry.layer_count));
            state.mark_emitted(key);
        }
        "Layer" => {
            while let Some((idx, quadratic, name, flags)) =
                layer_registry.defs.get(state.layer_index)
            {
                out.push(format!(
                    "Layer: {} {} \"{}\" {}",
                    idx,
                    if *quadratic { 1 } else { 0 },
                    escape_quoted(name),
                    flags
                ));
                state.layer_index += 1;
            }
            state.mark_emitted(key);
        }
        "CreationTime" if !state.is_emitted(key) => {
            if font.format_specific.contains_key(key) {
                out.push(format!("CreationTime: {}", font.date.timestamp()));
                state.mark_emitted(key);
            }
        }
        "LangName" => {
            if let Some(serde_json::Value::Array(lines)) =
                font.format_specific.get("sfd.lang_names")
            {
                if !state.is_emitted(key) {
                    for line in lines.iter().filter_map(|v| v.as_str()) {
                        out.push(format!("LangName: {}", line));
                    }
                    state.mark_emitted(key);
                }
            }
        }
        "BeginPrivate" if !state.is_emitted(key) => {
            if let Some(serde_json::Value::Array(lines)) =
                font.format_specific.get("sfd.private_section")
            {
                let first = lines.first().and_then(|v| v.as_str()).unwrap_or("0");
                out.push(format!("BeginPrivate: {}", first));
                for line in lines.iter().skip(1).filter_map(|v| v.as_str()) {
                    out.push(line.to_string());
                }
                out.push("EndPrivate".to_string());
                state.mark_emitted(key);
            }
        }
        "Grid" if !state.is_emitted(key) => {
            emit_guides(out, font);
            state.mark_emitted(key);
        }
        _ => {
            if emit_metric_key(out, font, key, state)?
                || emit_ot_key(out, font, key, state)
                || emit_passthrough_key(out, font, key, state)
            {}
        }
    }
    Ok(())
}

fn comment_entries(font: &Font) -> Vec<(String, String)> {
    font.format_specific
        .get(super::COMMENT_ENTRIES_KEY)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let object = entry.as_object()?;
                    let key = object.get("key")?.as_str()?.to_string();
                    let raw = object.get("raw")?.as_str()?.to_string();
                    Some((key, raw))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn next_comment_line(font: &Font, key: &str, state: &mut HeaderEmitState) -> Option<String> {
    let entries = comment_entries(font);
    if let Some((entry_key, raw)) = entries.get(state.comment_index) {
        if entry_key == key {
            state.comment_index += 1;
            return Some(format!("{}:{}", key, raw));
        }
    }
    None
}

fn font_name_line(font: &Font) -> Option<String> {
    let value = font
        .names
        .postscript_name
        .get_default()
        .or_else(|| font.names.full_name.get_default())
        .or_else(|| font.names.family_name.get_default())?;
    Some(format!("FontName: {}", sanitize_unquoted(value)))
}

fn full_name_line(font: &Font) -> Option<String> {
    let fallback = font
        .names
        .postscript_name
        .get_default()
        .or_else(|| font.names.family_name.get_default())?;
    let value = font.names.full_name.get_default().unwrap_or(fallback);
    Some(format!("FullName: {}", sanitize_unquoted(value)))
}

fn family_name_line(font: &Font) -> Option<String> {
    let fallback = font
        .names
        .full_name
        .get_default()
        .or_else(|| font.names.postscript_name.get_default())?;
    let value = font.names.family_name.get_default().unwrap_or(fallback);
    Some(format!("FamilyName: {}", sanitize_unquoted(value)))
}

fn weight_line(font: &Font) -> Option<String> {
    font.format_specific
        .get("postscript_weight_name")
        .and_then(|v| v.as_str())
        .map(|s| format!("Weight: {}", sanitize_unquoted(s)))
}

fn copyright_line(font: &Font) -> Option<String> {
    // Escaped rather than sanitize_unquoted's space-flattening, so the line
    // breaks FontForge escapes survive an SFD -> SFD round trip.
    font.names
        .copyright
        .get_default()
        .map(|s| format!("Copyright: {}", escape_sfd_line(s)))
}

fn version_line(font: &Font) -> String {
    let version_str = font
        .names
        .version
        .get_default()
        .cloned()
        .unwrap_or_else(|| format!("{}.{}", font.version.0, font.version.1));
    // SFD stores a bare number here. The "Version " prefix belongs to name ID 5,
    // where the OpenType spec asks for it, and writing it back into the SFD
    // would produce "Version: Version 1.002" and break a round-trip.
    let version_str = version_str
        .strip_prefix("Version ")
        .unwrap_or(&version_str)
        .to_string();
    format!("Version: {}", sanitize_unquoted(&version_str))
}

fn unique_id_line(font: &Font) -> Option<String> {
    font.names
        .unique_id
        .get_default()
        .map(|s| format!("UniqueID: {}", sanitize_unquoted(s)))
}

fn emit_ot_key(out: &mut Vec<String>, font: &Font, key: &str, state: &mut HeaderEmitState) -> bool {
    if state.is_emitted(key) {
        return false;
    }
    let Some(line) = ot_line_for_key(font, key) else {
        return false;
    };
    out.push(line);
    state.mark_emitted(key);
    true
}

fn ot_line_for_key(font: &Font, key: &str) -> Option<String> {
    let ot = &font.custom_ot_values;
    match key {
        "FSType" => font
            .format_specific
            .get("sfd.has_fstype")
            .and_then(|v| v.as_bool())
            .filter(|v| *v)
            .and(ot.os2_fs_type)
            .map(|v| format!("FSType: {}", v)),
        "OS2_UseTypoMetrics" => {
            if let Some(raw) = font.format_specific.get(key).and_then(|v| v.as_str()) {
                Some(format!("{}: {}", key, sanitize_unquoted(raw)))
            } else if ot
                .os2_fs_selection
                .map(|v| (v & (1 << 7)) != 0)
                .unwrap_or(false)
            {
                Some("OS2_UseTypoMetrics: 1".to_string())
            } else {
                None
            }
        }
        "OS2_WeightWidthSlopeOnly" => {
            if let Some(raw) = font.format_specific.get(key).and_then(|v| v.as_str()) {
                Some(format!("{}: {}", key, sanitize_unquoted(raw)))
            } else if ot
                .os2_fs_selection
                .map(|v| (v & (1 << 8)) != 0)
                .unwrap_or(false)
            {
                Some("OS2_WeightWidthSlopeOnly: 1".to_string())
            } else {
                None
            }
        }
        "TTFWeight" => ot.os2_us_weight_class.map(|v| format!("TTFWeight: {}", v)),
        "TTFWidth" => ot.os2_us_width_class.map(|v| format!("TTFWidth: {}", v)),
        "OS2FamilyClass" => ot
            .os2_family_class
            .map(|v| format!("OS2FamilyClass: {}", v)),
        "Panose" => ot.os2_panose.map(|panose| {
            format!(
                "Panose: {}",
                panose
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }),
        "OS2Vendor" => ot.os2_vendor_id.map(|v| format!("OS2Vendor: '{}'", v)),
        "OS2UnicodeRanges" => match (
            ot.os2_unicode_range1,
            ot.os2_unicode_range2,
            ot.os2_unicode_range3,
            ot.os2_unicode_range4,
        ) {
            (Some(r1), Some(r2), Some(r3), Some(r4)) => Some(format!(
                "OS2UnicodeRanges: {:08x}.{:08x}.{:08x}.{:08x}",
                r1, r2, r3, r4
            )),
            _ => None,
        },
        "OS2CodePages" => match (ot.os2_code_page_range1, ot.os2_code_page_range2) {
            (Some(c1), Some(c2)) => Some(format!("OS2CodePages: {:08x}.{:08x}", c1, c2)),
            _ => None,
        },
        _ => None,
    }
}

#[allow(dead_code)]
fn emit_ot_values(out: &mut Vec<String>, font: &Font) {
    let ot = &font.custom_ot_values;
    if let Some(v) = ot.os2_fs_type {
        if font
            .format_specific
            .get("sfd.has_fstype")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            out.push(format!("FSType: {}", v));
        }
        let has_typometrics_line = font.format_specific.contains_key("OS2_UseTypoMetrics");
        let has_wws_line = font
            .format_specific
            .contains_key("OS2_WeightWidthSlopeOnly");

        if !has_typometrics_line && (v & (1 << 7)) != 0 {
            out.push("OS2_UseTypoMetrics: 1".to_string());
        }
        if !has_wws_line && (v & (1 << 8)) != 0 {
            out.push("OS2_WeightWidthSlopeOnly: 1".to_string());
        }
    }
    if let Some(v) = ot.os2_us_weight_class {
        out.push(format!("TTFWeight: {}", v));
    }
    if let Some(v) = ot.os2_us_width_class {
        out.push(format!("TTFWidth: {}", v));
    }
    if let Some(v) = ot.os2_family_class {
        out.push(format!("OS2FamilyClass: {}", v));
    }
    if let Some(panose) = ot.os2_panose {
        let vals: Vec<String> = panose.iter().map(u8::to_string).collect();
        out.push(format!("Panose: {}", vals.join(" ")));
    }
    if let Some(vendor) = ot.os2_vendor_id {
        out.push(format!("OS2Vendor: '{}'", vendor));
    }
    if let (Some(r1), Some(r2), Some(r3), Some(r4)) = (
        ot.os2_unicode_range1,
        ot.os2_unicode_range2,
        ot.os2_unicode_range3,
        ot.os2_unicode_range4,
    ) {
        out.push(format!(
            "OS2UnicodeRanges: {:08x}.{:08x}.{:08x}.{:08x}",
            r1, r2, r3, r4
        ));
    }
    if let (Some(c1), Some(c2)) = (ot.os2_code_page_range1, ot.os2_code_page_range2) {
        out.push(format!("OS2CodePages: {:08x}.{:08x}", c1, c2));
    }
}

fn emit_passthrough_key(
    out: &mut Vec<String>,
    font: &Font,
    key: &str,
    state: &mut HeaderEmitState,
) -> bool {
    if state.is_emitted(key) {
        return false;
    }
    let Some(value) = font.format_specific.get(key).and_then(|v| v.as_str()) else {
        return false;
    };
    out.push(format!("{}: {}", key, sanitize_unquoted(value)));
    state.mark_emitted(key);
    true
}

fn emit_font_passthrough_keys_remaining(
    out: &mut Vec<String>,
    font: &Font,
    state: &mut HeaderEmitState,
) {
    for key in [
        "NeedsXUIDChange",
        "XUID",
        "OS2Version",
        "OS2TypoAOffset",
        "OS2TypoDOffset",
        "OS2WinAOffset",
        "OS2WinDOffset",
        "HheadAOffset",
        "HheadDOffset",
        "MarkAttachClasses",
        "DEI",
        "Encoding",
        "UnicodeInterp",
        "NameList",
        "DisplaySize",
        "AntiAlias",
        "FitToEm",
        "WinInfo",
        "ModificationTime",
    ] {
        let _ = emit_passthrough_key(out, font, key, state);
    }
}

fn emit_guides(out: &mut Vec<String>, font: &Font) {
    let Some(master) = font.masters.first() else {
        return;
    };
    if master.guides.is_empty() {
        return;
    }

    out.push("Grid".to_string());
    for g in &master.guides {
        let x1 = g.pos.x as f64;
        let y1 = g.pos.y as f64;
        let angle = (g.pos.angle as f64).to_radians();
        let x2 = x1 + angle.cos() * 1000.0;
        let y2 = y1 + angle.sin() * 1000.0;
        out.push(format!("{} {} m 0", fmt_num(x1), fmt_num(y1),));
        out.push(format!("{} {} l 0", fmt_num(x2), fmt_num(y2),));
    }
    out.push("EndSplineSet".to_string());
}

fn begin_chars_encoding_slots(font: &Font, glyph_order: &[String]) -> usize {
    if let Some(slots) = font
        .format_specific
        .get("sfd.beginchars_slots")
        .and_then(|v| v.as_u64())
    {
        return slots as usize;
    }

    let unencoded_count = font
        .glyphs
        .iter()
        .filter(|g| g.codepoints.is_empty())
        .count();

    if let Some(enc) = font
        .format_specific
        .get("Encoding")
        .and_then(|v| v.as_str())
    {
        if enc.eq_ignore_ascii_case("UnicodeBmp") {
            return 65_536 + unencoded_count;
        }
        if enc.eq_ignore_ascii_case("UnicodeFull") {
            return 1_114_112 + unencoded_count;
        }
    }

    let max_cp = font
        .glyphs
        .iter()
        .flat_map(|g| g.codepoints.iter().copied())
        .max()
        .map(|v| v as usize + 1)
        .unwrap_or(0);

    max_cp.max(glyph_order.len())
}

fn begin_chars_glyph_count(font: &Font, glyph_order: &[String]) -> usize {
    font.format_specific
        .get("sfd.beginchars_count")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(glyph_order.len())
}

// ===========================================================================
// Masters, axes and instances
// ===========================================================================

fn emit_metric_key(
    out: &mut Vec<String>,
    font: &Font,
    key: &str,
    state: &mut HeaderEmitState,
) -> Result<bool, BabelfontError> {
    if state.is_emitted(key) {
        return Ok(false);
    }
    let Some(master) = font.masters.first() else {
        return Ok(false);
    };
    let metric = match key {
        "ItalicAngle" => MetricType::ItalicAngle,
        "UnderlinePosition" => MetricType::UnderlinePosition,
        "UnderlineWidth" => MetricType::UnderlineThickness,
        "Ascent" => MetricType::Ascender,
        "Descent" => MetricType::Descender,
        "LineGap" => MetricType::HheaLineGap,
        "HheadAscent" => MetricType::HheaAscender,
        "HheadDescent" => MetricType::HheaDescender,
        "OS2TypoLinegap" => MetricType::TypoLineGap,
        "OS2TypoAscent" => MetricType::TypoAscender,
        "OS2TypoDescent" => MetricType::TypoDescender,
        "OS2WinAscent" => MetricType::WinAscent,
        "OS2WinDescent" => MetricType::WinDescent,
        "OS2SubXSize" => MetricType::SubscriptXSize,
        "OS2SubYSize" => MetricType::SubscriptYSize,
        "OS2SubXOff" => MetricType::SubscriptXOffset,
        "OS2SubYOff" => MetricType::SubscriptYOffset,
        "OS2SupXSize" => MetricType::SuperscriptXSize,
        "OS2SupYSize" => MetricType::SuperscriptYSize,
        "OS2SupXOff" => MetricType::SuperscriptXOffset,
        "OS2SupYOff" => MetricType::SuperscriptYOffset,
        "OS2StrikeYSize" => MetricType::StrikeoutSize,
        "OS2StrikeYPos" => MetricType::StrikeoutPosition,
        "OS2CapHeight" => MetricType::CapHeight,
        "OS2XHeight" => MetricType::XHeight,
        _ => return Ok(false),
    };
    if master.metrics.contains_key(&metric) {
        // Offset-mode metrics were resolved to absolutes at parse time;
        // reconstruct the delta from the current (possibly user-modified)
        // absolute metric and the appropriate base value.
        let offset_key = format!("sfd.offset_mode.{}", key);
        let is_offset = font
            .format_specific
            .get(&offset_key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if is_offset {
            let absolute = *master.metrics.get(&metric).unwrap_or(&0);
            let delta = compute_offset_delta(font, key, absolute)?;
            out.push(format!("{}: {}", key, delta));
        } else if metric == MetricType::ItalicAngle {
            // ItalicAngle is stored as a counter-clockwise value in FontForge;
            // we store as clockwise, so negate when writing out.
            let value = *master.metrics.get(&metric).unwrap_or(&0);
            out.push(format!("{}: {}", key, -value));
        } else {
            emit_metric(out, master, metric, key);
        }
        state.mark_emitted(key);
    }
    Ok(true)
}

fn emit_metric(out: &mut Vec<String>, master: &crate::Master, metric: MetricType, key: &str) {
    if let Some(v) = master.metrics.get(&metric) {
        if metric == MetricType::Descender {
            out.push(format!("{}: {}", key, -v));
        } else {
            out.push(format!("{}: {}", key, v));
        }
    }
}

// ===========================================================================
// Glyphs
// ===========================================================================

fn emit_glyph(
    out: &mut Vec<String>,
    glyph: &Glyph,
    gid: usize,
    layer_registry: &LayerRegistry,
    default_master_id: &str,
    glyph_index: &HashMap<SmolStr, usize>,
    kerns: Option<&Vec<(usize, i16)>>,
) -> Result<(), BabelfontError> {
    out.push(format!("StartChar: {}", sanitize_unquoted(&glyph.name)));

    let encoding_slot = glyph
        .format_specific
        .get("sfd.encoding_slot")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| {
            glyph
                .codepoints
                .first()
                .copied()
                .map(|cp| cp as i64)
                .unwrap_or(-1)
        });
    let unicode_value = glyph
        .format_specific
        .get("sfd.encoding_unicode")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| {
            glyph
                .codepoints
                .first()
                .copied()
                .map(|cp| cp as i64)
                .unwrap_or(-1)
        });
    let has_gid = glyph
        .format_specific
        .get("sfd.encoding_has_gid")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if has_gid {
        let encoding_gid = glyph
            .format_specific
            .get("sfd.encoding_gid")
            .and_then(|v| v.as_i64())
            .unwrap_or(gid as i64);
        out.push(format!(
            "Encoding: {} {} {}",
            encoding_slot, unicode_value, encoding_gid
        ));
    } else {
        out.push(format!("Encoding: {} {}", encoding_slot, unicode_value));
    }

    let width = pick_foreground_width(glyph, default_master_id);
    out.push(format!("Width: {}", fmt_num(width as f64)));

    if let Some(vwidth) = glyph.format_specific.get("vwidth").and_then(|v| v.as_str()) {
        out.push(format!("VWidth: {}", sanitize_unquoted(vwidth)));
    }

    let class_num = match glyph.category {
        GlyphCategory::Base => 2,
        GlyphCategory::Ligature => 3,
        GlyphCategory::Mark => 4,
        _ => 0,
    };
    if class_num != 0 {
        out.push(format!("GlyphClass: {}", class_num));
    }

    if let Some(flags) = glyph_flags_for_emit(glyph) {
        out.push(format!("Flags: {}", sanitize_unquoted(&flags)));
    }

    if let Some(layer) = glyph_foreground_layer(glyph, default_master_id) {
        if let Some(hstem) = layer
            .format_specific
            .get(super::HSTEM_KEY)
            .and_then(|v| v.as_str())
        {
            out.push(format!("HStem: {}", sanitize_unquoted(hstem)));
        }
        if let Some(vstem) = layer
            .format_specific
            .get(super::VSTEM_KEY)
            .and_then(|v| v.as_str())
        {
            out.push(format!("VStem: {}", sanitize_unquoted(vstem)));
        }
    }

    let emit_layer_count = glyph
        .format_specific
        .get("sfd.has_layer_count")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || glyph.layers.len() > 1;

    if emit_layer_count {
        out.push(format!("LayerCount: {}", layer_registry.layer_count));
    }

    let mut indexed_layers: Vec<(usize, &Layer)> = glyph
        .layers
        .iter()
        .map(|l| (layer_registry.index_for(l, default_master_id), l))
        .collect();
    indexed_layers.sort_by_key(|(ix, _)| *ix);

    for (ix, layer) in indexed_layers {
        emit_layer(out, glyph, layer, ix, glyph_index)?;
    }

    if let Some(comment) = glyph
        .format_specific
        .get("sfd.comment")
        .and_then(|v| v.as_str())
    {
        out.push(format!("Comment: {}", sanitize_unquoted(comment)));
    }

    if let Some(entries) = kerns {
        if !entries.is_empty() {
            let payload = entries
                .iter()
                .map(|(right_gid, value)| {
                    format!(
                        "{} {} \"{}\"",
                        right_gid,
                        value,
                        super::GENERATED_KERN_SUBTABLE
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            out.push(format!("Kerns2: {}", payload));
        }
    }

    out.push("EndChar".to_string());
    Ok(())
}

fn glyph_flags_for_emit(glyph: &Glyph) -> Option<String> {
    if let Some(flags) = glyph
        .format_specific
        .get("sfd.flags")
        .and_then(|v| v.as_str())
    {
        return Some(flags.to_string());
    }

    let mut out = String::new();
    if glyph
        .format_specific
        .get("sfd.changed_since_last_hinted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        out.push('H');
    }
    if glyph
        .format_specific
        .get("sfd.manual_hints")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        out.push('M');
    }
    if glyph
        .format_specific
        .get("sfd.width_set")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        out.push('W');
    }
    if glyph
        .format_specific
        .get("sfd.editor_state_saved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        out.push('O');
    }
    if glyph
        .format_specific
        .get("sfd.instructions_out_of_date")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        out.push('I');
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn pick_foreground_width(glyph: &Glyph, default_master_id: &str) -> f32 {
    glyph_foreground_layer(glyph, default_master_id)
        .map(|l| l.width)
        .unwrap_or(0.0)
}

// ===========================================================================
// Layers
// ===========================================================================

fn emit_layer(
    out: &mut Vec<String>,
    glyph: &Glyph,
    layer: &Layer,
    layer_idx: usize,
    glyph_index: &HashMap<SmolStr, usize>,
) -> Result<(), BabelfontError> {
    match layer_idx {
        0 => out.push("Back".to_string()),
        1 => out.push("Fore".to_string()),
        _ => out.push(format!("Layer: {}", layer_idx)),
    }

    if let Some(color) = layer.color {
        let r = (color.r.clamp(0, 255) as u32) << 16;
        let g = (color.g.clamp(0, 255) as u32) << 8;
        let b = color.b.clamp(0, 255) as u32;
        out.push(format!("Colour: {:06x}", r | g | b));
    }

    emit_layer_shapes(out, glyph, layer, layer_idx, glyph_index)?;
    emit_layer_anchors(out, layer);
    Ok(())
}

pub(crate) fn glyph_foreground_layer<'a>(
    glyph: &'a Glyph,
    default_master_id: &str,
) -> Option<&'a Layer> {
    glyph
        .layers
        .iter()
        .find(|l| LayerRegistry::is_foreground_layer(l, default_master_id))
        .or_else(|| {
            glyph
                .layers
                .iter()
                .find(|l| !LayerRegistry::is_background_layer(l))
        })
        .or_else(|| glyph.layers.first())
}

// ===========================================================================
// Anchors and shapes
// ===========================================================================

fn emit_layer_anchors(out: &mut Vec<String>, layer: &Layer) {
    for anchor in &layer.anchors {
        let kind = anchor
            .format_specific
            .get("sfd.kind")
            .and_then(|v| v.as_str())
            .unwrap_or("base");
        let index = anchor
            .format_specific
            .get("sfd.index")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let name = if kind == "mark" && anchor.name.starts_with('_') {
            anchor.name[1..].to_string()
        } else {
            anchor.name.clone()
        };

        out.push(format!(
            "AnchorPoint: \"{}\" {} {} {} {}",
            escape_quoted(&name),
            fmt_num(anchor.x),
            fmt_num(anchor.y),
            kind,
            index
        ));
    }
}

fn emit_layer_shapes(
    out: &mut Vec<String>,
    glyph: &Glyph,
    layer: &Layer,
    layer_idx: usize,
    glyph_index: &HashMap<SmolStr, usize>,
) -> Result<(), BabelfontError> {
    let has_path = layer.shapes.iter().any(|s| matches!(s, Shape::Path(_)));
    if has_path {
        let explicit_splineset = layer
            .format_specific
            .get("sfd.explicit_splineset")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if explicit_splineset {
            out.push("SplineSet".to_string());
        }
        for (shape_index, shape) in layer.shapes.iter().enumerate() {
            if let Shape::Path(path) = shape {
                let path_str = save_path(path, layer_is_quadratic(layer)).map_err(|error| {
                    BabelfontError::General(format!(
                        "Failed to save path for glyph '{}' layer {} shape {}: {}",
                        glyph.name, layer_idx, shape_index, error
                    ))
                })?;
                out.push(path_str);
            }
        }
        out.push("EndSplineSet".to_string());
    }

    for shape in &layer.shapes {
        if let Shape::Component(component) = shape {
            let component_str = save_component(component, glyph_index)?;
            out.push(component_str);
        }
    }
    Ok(())
}

// ===========================================================================
// Paths and components
// ===========================================================================

fn save_component(
    component: &Component,
    glyph_index: &HashMap<SmolStr, usize>,
) -> Result<String, BabelfontError> {
    let Some(gid) = glyph_index.get(&component.reference) else {
        return Err(BabelfontError::MissingGlyphReference(
            component.reference.to_string(),
        ));
    };

    let coeffs = component.transform.as_affine().as_coeffs();
    // SFD Refer matrix order is [xx, xy, yx, yy, tx, ty].
    let xx = coeffs[0];
    let yx = coeffs[1];
    let xy = coeffs[2];
    let yy = coeffs[3];
    let tx = coeffs[4];
    let ty = coeffs[5];

    let unicodeenc = component
        .format_specific
        .get("sfd.refer.unicodeenc")
        .and_then(|v| v.as_str())
        .unwrap_or("0");
    let selected = component
        .format_specific
        .get("sfd.refer.selected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let flags = component
        .format_specific
        .get("sfd.refer.flags")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or_else(|| {
            let mut bits = 0u32;
            if component
                .format_specific
                .get("sfd.refer.use_my_metrics")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                bits |= 0x1;
            }
            if component
                .format_specific
                .get("sfd.refer.round_translation_to_grid")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                bits |= 0x2;
            }
            if component
                .format_specific
                .get("sfd.refer.point_match")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                bits |= 0x4;
            }
            bits
        });

    let mut line = format!(
        "Refer: {} {} {} {} {} {} {} {} {} {}",
        gid,
        unicodeenc,
        if selected { "S" } else { "N" },
        fmt_num(xx),
        fmt_num(xy),
        fmt_num(yx),
        fmt_num(yy),
        fmt_num(tx),
        fmt_num(ty),
        flags
    );

    if (flags & 0x4) != 0 {
        let base_pt = component
            .format_specific
            .get("sfd.refer.match_pt_base")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let ref_pt = component
            .format_specific
            .get("sfd.refer.match_pt_ref")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        line.push_str(&format!(" {} {}", base_pt, ref_pt));
        if component
            .format_specific
            .get("sfd.refer.point_match_out_of_date")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            line.push_str(" O");
        }
    }

    Ok(line)
}

fn save_path(path: &Path, is_quadratic: bool) -> Result<String, BabelfontError> {
    let oncurve_indices: Vec<usize> = path
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            if matches!(n.nodetype, NodeType::OffCurve) {
                None
            } else {
                Some(i)
            }
        })
        .collect();

    let implicit_move_closed = path.closed
        && path
            .nodes
            .first()
            .map(|node| node.nodetype != NodeType::Move)
            .unwrap_or(false);
    let start_ix = if implicit_move_closed {
        oncurve_indices.last().copied()
    } else {
        oncurve_indices.first().copied()
    };

    let Some(start_ix) = start_ix else {
        return Err(BabelfontError::General(format!(
            "Path has no on-curve points ({} nodes: {})",
            path.nodes.len(),
            path.nodes
                .iter()
                .map(|node| format!(
                    "{:?}@{},{}",
                    node.nodetype,
                    fmt_num(node.x),
                    fmt_num(node.y)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    };
    let start = &path.nodes[start_ix];
    let mut out: Vec<String> = Vec::new();
    out.push(format!(
        "{} {} m {}",
        fmt_num(start.x),
        fmt_num(start.y),
        point_flags_for_node(start)
    ));

    let mut current = start;
    let mut offcurves: Vec<&Node> = Vec::new();
    let remaining_nodes: Vec<&Node> = if path.closed {
        path.nodes[start_ix + 1..]
            .iter()
            .chain(path.nodes[..start_ix].iter())
            .collect()
    } else {
        path.nodes[start_ix + 1..].iter().collect()
    };

    for node in remaining_nodes {
        match node.nodetype {
            NodeType::OffCurve => offcurves.push(node),
            NodeType::Curve => {
                if offcurves.len() >= 2 {
                    let c1 = offcurves[offcurves.len() - 2];
                    let c2 = offcurves[offcurves.len() - 1];
                    out.push(format!(
                        " {} {} {} {} {} {} c {}",
                        fmt_num(c1.x),
                        fmt_num(c1.y),
                        fmt_num(c2.x),
                        fmt_num(c2.y),
                        fmt_num(node.x),
                        fmt_num(node.y),
                        point_flags_for_node(node)
                    ));
                } else {
                    out.push(format!(
                        " {} {} l {}",
                        fmt_num(node.x),
                        fmt_num(node.y),
                        point_flags_for_node(node)
                    ));
                }
                current = node;
                offcurves.clear();
            }
            NodeType::QCurve => {
                if let Some(control) = offcurves.last() {
                    out.push(format!(
                        " {} {} {} {} {} {} c {}",
                        fmt_num(control.x),
                        fmt_num(control.y),
                        fmt_num(control.x),
                        fmt_num(control.y),
                        fmt_num(node.x),
                        fmt_num(node.y),
                        point_flags_for_node(node)
                    ));
                } else if is_quadratic {
                    out.push(format!(
                        " {} {} {} {} {} {} c {}",
                        fmt_num(current.x),
                        fmt_num(current.y),
                        fmt_num(current.x),
                        fmt_num(current.y),
                        fmt_num(node.x),
                        fmt_num(node.y),
                        point_flags_for_node(node)
                    ));
                } else {
                    out.push(format!(
                        " {} {} l {}",
                        fmt_num(node.x),
                        fmt_num(node.y),
                        point_flags_for_node(node)
                    ));
                }
                current = node;
                offcurves.clear();
            }
            NodeType::Line | NodeType::Move => {
                out.push(format!(
                    " {} {} l {}",
                    fmt_num(node.x),
                    fmt_num(node.y),
                    point_flags_for_node(node)
                ));
                current = node;
                offcurves.clear();
            }
        }
    }

    if path.closed
        && (implicit_move_closed
            || current.x != start.x
            || current.y != start.y
            || !offcurves.is_empty())
    {
        if is_quadratic && !offcurves.is_empty() {
            let control = offcurves[offcurves.len() - 1];
            out.push(format!(
                " {} {} {} {} {} {} c {}",
                fmt_num(control.x),
                fmt_num(control.y),
                fmt_num(control.x),
                fmt_num(control.y),
                fmt_num(start.x),
                fmt_num(start.y),
                point_flags_for_node(start)
            ));
        } else if offcurves.len() >= 2 {
            let c1 = offcurves[offcurves.len() - 2];
            let c2 = offcurves[offcurves.len() - 1];
            out.push(format!(
                " {} {} {} {} {} {} c {}",
                fmt_num(c1.x),
                fmt_num(c1.y),
                fmt_num(c2.x),
                fmt_num(c2.y),
                fmt_num(start.x),
                fmt_num(start.y),
                point_flags_for_node(start)
            ));
        } else {
            out.push(format!(
                " {} {} l {}",
                fmt_num(start.x),
                fmt_num(start.y),
                point_flags_for_node(start)
            ));
        }
    }

    Ok(out.join("\n"))
}

fn point_flags_for_node(node: &Node) -> String {
    if let Some(flags) = node
        .format_specific
        .get("sfd.point_flags")
        .and_then(|v| v.as_str())
    {
        return flags.to_string();
    }

    if node.smooth {
        "0x100".to_string()
    } else {
        "0".to_string()
    }
}

// ===========================================================================
// Kerning
// ===========================================================================

fn collect_explicit_kerns(
    font: &Font,
    glyph_index: &HashMap<SmolStr, usize>,
) -> HashMap<SmolStr, Vec<(usize, i16)>> {
    let mut by_left: HashMap<SmolStr, Vec<(usize, i16)>> = HashMap::new();
    let Some(master) = font.masters.first() else {
        return by_left;
    };

    for ((left, right), value) in &master.kerning {
        if left.starts_with('@') || right.starts_with('@') {
            continue;
        }
        if let Some(&right_ix) = glyph_index.get(right) {
            by_left
                .entry(left.clone())
                .or_default()
                .push((right_ix, *value));
        }
    }

    by_left
}

fn emit_font_level_kerning(
    _out: &mut Vec<String>,
    _font: &Font,
    _glyph_order: &[String],
    _glyph_index: &HashMap<SmolStr, usize>,
) {
    // Placeholder for class-based kerning (KernClass2) emission.
}

// ===========================================================================
// OpenType features
// ===========================================================================

fn emit_features(_out: &mut Vec<String>, _font: &Font) {
    // Placeholder for Lookup/feature table emission.
}
