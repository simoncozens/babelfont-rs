use std::{collections::HashMap, path::PathBuf};

use fontdrasil::{
    coords::{DesignCoord, DesignLocation, Location, UserCoord},
    types::Tag,
};
use indexmap::IndexMap;
use smol_str::SmolStr;
use vfbreader::{
    read_vfb, Anchor as VfbAnchor, GlyphEntry, GlyphGDEF, GuideProperty, Guides as VfbGuides,
    Node as VFBNode, PostScriptInfo, TrueTypeValue, Vfb, VfbEntry,
};

use crate::{
    common::decomposition::{DecomposedAffine, TransformOrder},
    features::PossiblyAutomaticCode,
    Anchor, Axis, BabelfontError, Features, Font, FormatSpecific, Glyph, Guide, Layer, LayerType,
    Master, MetricType, Node, NodeType, Path, Shape,
};

/// Map a VFB axis name to its OpenType axis tag, using FontLab's conventions
/// for the common registered axes.
fn axis_tag_for_name(name: &str) -> Tag {
    match name.trim() {
        "Weight" => Tag::from_be_bytes(*b"wght"),
        "Width" => Tag::from_be_bytes(*b"wdth"),
        "Optical Size" => Tag::from_be_bytes(*b"opsz"),
        "Serif" => Tag::from_be_bytes(*b"SERF"),
        "Italic" => Tag::from_be_bytes(*b"ital"),
        "Slant" => Tag::from_be_bytes(*b"slnt"),
        other => {
            let mut bytes = [b' '; 4];
            for (slot, ch) in bytes.iter_mut().zip(other.to_uppercase().chars()) {
                *slot = ch as u8;
            }
            Tag::new(&bytes)
        }
    }
}

/// Store a name only when it is non-empty, matching the behaviour of the
/// other loaders (an absent name is better than an empty one).
fn set_optional_name(target: &mut crate::i18ndictionary::I18NDictionary, value: String) {
    if !value.is_empty() {
        *target = value.into();
    }
}

/// Set a metric on every master.
/// Make an OpenType vendor tag from a VFB vendor string.
///
/// `achVendID` is four printable ASCII bytes, and once a space appears the rest
/// must be spaces too. FontLab pads short strings with NULs; a shorter vendor
/// is padded with spaces. Returns `None` for a string that cannot be a tag.
fn vendor_tag(vendor: &str) -> Option<Tag> {
    let trimmed = vendor.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    if trimmed.is_empty() || trimmed.len() > 4 || !trimmed.chars().all(|c| c.is_ascii_graphic()) {
        return None;
    }
    Tag::new_checked(format!("{trimmed:<4}").as_bytes()).ok()
}

/// The top of an underline FontLab stores by its middle: FontLab's export adds
/// half the thickness, halved with integer division.
fn underline_top(position: i32, thickness: i32) -> i32 {
    position + thickness / 2
}

fn set_metric_all_masters(font: &mut Font, metric: MetricType, value: i32) {
    for master in font.masters.iter_mut() {
        master.metrics.insert(metric.clone(), value);
    }
}

/// VFB convertor
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let vfb: Vfb = read_vfb(&path).map_err(|e| BabelfontError::VfbLoad(e.to_string()))?;

    // First pass: collect metadata about axes and masters
    let mut axis_count = 0;
    let mut axis_names = vec![];
    let mut master_count = 1; // Default to 1 master
    let mut master_names = vec![];
    let mut style_name: Option<String> = None;
    let mut axis_mappings_count = [0u32; 4];
    let mut axis_mappings: Vec<(f64, f64)> = vec![];
    // Master locations, indexed by the (1-based) master index stored in the
    // VFB, in per-mille design coordinates per axis.
    let mut master_locations: Vec<[f64; 4]> = vec![];
    let mut kerning_class_flags: HashMap<String, (i32, i32)> = HashMap::new();
    let mut open_type_classes: Vec<String> = vec![];

    for entry in &vfb.entries {
        match entry {
            VfbEntry::AxisCount(count) => axis_count = *count as usize,
            VfbEntry::AxisName(name) => axis_names.push(name.clone()),
            VfbEntry::MasterCount(count) => master_count = *count as usize,
            VfbEntry::MasterName(name) => master_names.push(name.clone()),
            VfbEntry::StyleName(name) => style_name = Some(name.clone()),
            VfbEntry::OpenTypeClass(class) => open_type_classes.push(class.clone()),
            VfbEntry::KerningClassFlags(flags) => {
                for flag in flags {
                    kerning_class_flags.insert(flag.name.clone(), flag.flags);
                }
            }
            VfbEntry::AxisMappingsCount(counts) => axis_mappings_count = *counts,
            VfbEntry::AxisMappings(raw) => {
                axis_mappings = raw
                    .0
                    .chunks_exact(16)
                    .map(|chunk| {
                        let user = f64::from_le_bytes(chunk[0..8].try_into().unwrap_or([0; 8]));
                        let design = f64::from_le_bytes(chunk[8..16].try_into().unwrap_or([0; 8]));
                        (user, design)
                    })
                    .collect();
            }
            VfbEntry::MasterLocation(raw) if raw.0.len() >= 36 => {
                let index = u32::from_le_bytes(raw.0[0..4].try_into().unwrap_or([0; 4])) as usize;
                let mut location = [0.0f64; 4];
                for (axis, coord) in location.iter_mut().enumerate() {
                    let start = 4 + axis * 8;
                    *coord =
                        f64::from_le_bytes(raw.0[start..start + 8].try_into().unwrap_or([0; 8]));
                }
                if index > 0 {
                    if master_locations.len() < index {
                        master_locations.resize(index, [0.0; 4]);
                    }
                    master_locations[index - 1] = location;
                }
            }
            _ => {}
        }
    }

    // Convert Vfb to Babelfont Font
    let mut font = Font::new();

    // Set up axes from the VFB's axis names and mapping tables.
    for i in 0..axis_count {
        let name = axis_names
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("Axis {}", i + 1));
        let mut axis = Axis::new(name.as_str(), axis_tag_for_name(&name));

        // Each axis uses up to 10 (user, design) mapping pairs. The count
        // entry tells us how many of those pairs are significant.
        let count = axis_mappings_count.get(i).copied().unwrap_or(0) as usize;
        let start = i * 10;
        let mut map = Vec::new();
        for (user, design) in axis_mappings.iter().skip(start).take(count) {
            map.push((UserCoord::new(*user), DesignCoord::new(design * 1000.0)));
        }
        if !map.is_empty() {
            map.sort_by_key(|a| a.0);
            let min = map
                .iter()
                .map(|(user, _)| user.to_f64())
                .fold(f64::INFINITY, f64::min);
            let max = map
                .iter()
                .map(|(user, _)| user.to_f64())
                .fold(f64::NEG_INFINITY, f64::max);
            axis.min = Some(UserCoord::new(min));
            axis.default = Some(UserCoord::new(min));
            axis.max = Some(UserCoord::new(max));
            axis.map = Some(map);
        } else {
            // Without a mapping table, derive user-space bounds from the
            // master locations (which are stored in design coordinates).
            let mut values: Vec<f64> = master_locations
                .iter()
                .take(master_count)
                .map(|location| location[i] * 1000.0)
                .collect();
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            if let (Some(min), Some(max)) = (values.first(), values.last()) {
                axis.min = Some(UserCoord::new(*min));
                axis.default = Some(UserCoord::new(*min));
                axis.max = Some(UserCoord::new(*max));
            }
        }
        font.axes.push(axis);
    }

    // Create masters
    for i in 0..master_count {
        let raw_name = master_names.get(i).map(|name| name.trim()).unwrap_or("");
        // A single-master VFB often stores "Untitled" as the master name; the
        // human-readable layer name is the style name in that case.
        let master_name = if master_count == 1 && !style_name.as_deref().unwrap_or("").is_empty() {
            style_name.clone().unwrap_or_default()
        } else if raw_name.is_empty() {
            format!("Master {}", i + 1)
        } else {
            raw_name.to_string()
        };

        let location: DesignLocation = master_locations
            .get(i)
            .map(|values| {
                font.axes
                    .iter()
                    .enumerate()
                    .map(|(axis_index, axis)| {
                        (axis.tag, DesignCoord::new(values[axis_index] * 1000.0))
                    })
                    .collect::<Vec<_>>()
            })
            .map(Location::from)
            .unwrap_or_default();

        let id = master_name.clone();
        font.masters.push(Master::new(master_name, id, location));
    }

    // OpenType classes are also used for kerning groups. Populate the feature
    // classes and the first/second kerning groups before reading glyphs.
    apply_class_definitions(&mut font, &open_type_classes, &kerning_class_flags);

    // Second pass: collect anchor, guide, mask and image data
    let mut raw_kerning: Vec<(String, HashMap<i32, Vec<i32>>)> = vec![];
    let mut mm_kern_pairs: Vec<[i16; 5]> = vec![];
    let mut pending_anchors_mm: Option<Vec<Vec<VfbAnchor>>> = None;
    let mut pending_gdef: Option<GlyphGDEF> = None;
    let mut pending_global_guides: Option<VfbGuides> = None;
    let mut pending_global_guide_props: Vec<GuideProperty> = vec![];
    let mut pending_guide_props: Vec<GuideProperty> = vec![];
    let mut pending_mask: Option<Vec<VFBNode>> = None;
    let mut pending_image: bool = false;
    let mut postscript_infos: Vec<PostScriptInfo> = vec![];

    // Third pass: process entries, applying anchors when we know the glyph
    for entry in vfb.entries {
        match entry {
            VfbEntry::FlVersion(_) => {}               // => todo!(),
            VfbEntry::FontOptions(_) => {}             // => todo!(),
            VfbEntry::EncodingDefault(_encoding) => {} // => todo!(),
            VfbEntry::Encoding(_encoding) => {}        // => todo!(),
            VfbEntry::MMEncodingType(_) => {}          // => todo!(),
            VfbEntry::BlockNamesEnd(_) => {}
            VfbEntry::BlockFontInfoStart(_) => {}
            VfbEntry::FontName(s) => {
                font.names.postscript_name = s.into();
            }
            VfbEntry::MasterCount(_) => {
                // Already handled in first pass
            }
            VfbEntry::Version(v) => {
                set_optional_name(&mut font.names.version, v);
            }
            VfbEntry::Notice(s) => set_optional_name(&mut font.names.description, s),
            VfbEntry::FullName(s) => set_optional_name(&mut font.names.full_name, s),
            VfbEntry::FamilyName(s) => set_optional_name(&mut font.names.family_name, s),
            VfbEntry::PrefFamilyName(s) => set_optional_name(&mut font.names.typographic_family, s),
            VfbEntry::MenuName(_) => {} // Derived from family/subfamily names
            VfbEntry::AppleName(_) => {} // => todo!(),
            VfbEntry::Weight(_) => {}   // => todo!(),
            VfbEntry::Width(_) => {}    // => todo!(),
            VfbEntry::License(s) => set_optional_name(&mut font.names.license, s),
            VfbEntry::LicenseUrl(s) => set_optional_name(&mut font.names.license_url, s),
            VfbEntry::Copyright(s) => set_optional_name(&mut font.names.copyright, s),
            VfbEntry::Trademark(serde) => set_optional_name(&mut font.names.trademark, serde),
            VfbEntry::Designer(s) => set_optional_name(&mut font.names.designer, s),
            VfbEntry::DesignerUrl(s) => set_optional_name(&mut font.names.designer_url, s),
            VfbEntry::VendorUrl(s) => set_optional_name(&mut font.names.manufacturer_url, s),
            VfbEntry::Source(s) => set_optional_name(&mut font.names.manufacturer, s),
            VfbEntry::IsFixedPitch(_) => {} // => todo!(),
            VfbEntry::UnderlineThickness(ut) => {
                for master in font.masters.iter_mut() {
                    master
                        .metrics
                        .insert(crate::MetricType::UnderlineThickness, ut as i32);
                }
            }
            VfbEntry::MsCharset(_) => {} // => todo!(),
            // The PANOSE the export writes is this font-level entry, not the
            // one in the TrueType block.
            VfbEntry::Panose(p) => {
                font.custom_ot_values.os2_panose = Some(p.map(|b| b as u8));
            }
            VfbEntry::TtVersion(s) => set_optional_name(&mut font.names.version, s),
            VfbEntry::TtUId(_) => {} // => todo!(),
            VfbEntry::StyleName(s) => {
                set_optional_name(&mut font.names.preferred_subfamily_name, s)
            }
            VfbEntry::PrefStyleName(s) => {
                set_optional_name(&mut font.names.typographic_subfamily, s)
            }
            VfbEntry::MacCompatible(_) => {} // => todo!(),
            VfbEntry::Vendor(s) => match vendor_tag(&s) {
                Some(tag) => font.custom_ot_values.os2_vendor_id = Some(tag),
                None if !s
                    .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                    .is_empty() =>
                {
                    log::warn!("Ignoring vendor ID {s:?}: not a legal OpenType tag")
                }
                None => {}
            },
            VfbEntry::Year(_) => {} // => todo!(),
            VfbEntry::VersionMajor(v) => {
                font.version = (v, font.version.1);
            }
            VfbEntry::VersionMinor(v) => {
                font.version = (font.version.0, v);
            }
            VfbEntry::Upm(u) => {
                font.upm = u;
            }
            VfbEntry::FondId(_id) => {} // This is a Mac FondId, *not* an OpenType vendor ID
            VfbEntry::BlueValuesNum(_) => {} // => todo!(),
            VfbEntry::OtherBluesNum(_) => {} // => todo!(),
            VfbEntry::FamilyBluesNum(_) => {} // => todo!(),
            VfbEntry::FamilyOtherBluesNum(_) => {} // => todo!(),
            VfbEntry::StemSnapHNum(_) => {} // => todo!(),
            VfbEntry::StemSnapVNum(_) => {} // => todo!(),
            VfbEntry::FontStyle(_) => {} // => todo!(),
            VfbEntry::PclId(_) => {}    // => todo!(),
            VfbEntry::VpId(_) => {}     // => todo!(),
            VfbEntry::MsId(_) => {}     // => todo!(),
            VfbEntry::PclCharsSet(_) => {} // => todo!(),
            VfbEntry::HheaLineGap(gap) => {
                for master in font.masters.iter_mut() {
                    master
                        .metrics
                        .insert(crate::MetricType::HheaLineGap, gap as i32);
                }
            }
            VfbEntry::StemSnapLimit(_) => {}   // => todo!(),
            VfbEntry::ZonePpm(_) => {}         // => todo!(),
            VfbEntry::CodePpm(_) => {}         // => todo!(),
            VfbEntry::DropoutPpm(_) => {}      // => todo!(),
            VfbEntry::MeasurementLine(_) => {} // => todo!(),
            VfbEntry::ExportPcltTable(_) => {} // => todo!(),
            VfbEntry::Note(_) => {}            // => todo!(),
            VfbEntry::CustomData(_) => {}      // => todo!(),
            VfbEntry::OpenTypeClass(_class) => {
                // Already handled after the first pass so that kerning class
                // flags are available.
            }
            VfbEntry::AxisCount(_) => {
                // Already handled in first pass
            }
            VfbEntry::AxisName(_) => {
                // Already handled in first pass
            }
            VfbEntry::MasterName(_) => {
                // Already handled in first pass
            }
            VfbEntry::DefaultCharacter(_) => {}  // => todo!(),
            VfbEntry::CustomDict(_) => {}        // => todo!(),
            VfbEntry::Mark(_) => {}              // => todo!(),
            VfbEntry::GlyphCustomData(_) => {}   // => todo!(),
            VfbEntry::GlyphNote(_) => {}         // => todo!(),
            VfbEntry::WeightVector(_items) => {} // => todo!(),
            VfbEntry::UniqueId(_) => {}          // => todo!(),
            // The export writes the weight code as usWeightClass, not the
            // TrueType block's value.
            VfbEntry::WeightCode(w) if w > 0 => {
                font.custom_ot_values.os2_us_weight_class = Some(w as u16);
            }
            VfbEntry::WeightCode(_) => {}
            VfbEntry::ItalicAngle(angle) => {
                for master in font.masters.iter_mut() {
                    master
                        .metrics
                        .insert(crate::MetricType::ItalicAngle, angle as i32);
                }
            }
            VfbEntry::SlantAngle(_) => {} // => todo!(),
            VfbEntry::UnderlinePosition(position) => {
                for master in font.masters.iter_mut() {
                    master
                        .metrics
                        .insert(crate::MetricType::UnderlinePosition, position as i32);
                }
            }
            VfbEntry::SampleText(s) => {
                set_optional_name(&mut font.names.sample_text, s);
            }
            VfbEntry::Xuid(_items) => {} // => todo!(),
            VfbEntry::XuidNum(_) => {}   // => todo!(),
            VfbEntry::PostScriptHintingOptions(_post_script_global_hinting_options) => {} // => todo!(),
            VfbEntry::Collection(_items) => {} // => todo!(_),
            VfbEntry::TtInfo(values) => {
                for value in values {
                    match value {
                        TrueTypeValue::Os2UsWidthClass(v) => {
                            font.custom_ot_values.os2_us_width_class = Some(v as u16);
                            continue;
                        }
                        TrueTypeValue::Os2FsType(v) => {
                            font.custom_ot_values.os2_fs_type = Some(v as u16);
                            continue;
                        }
                        TrueTypeValue::HeadLowestRecPpem(v) => {
                            font.custom_ot_values.head_lowest_rec_ppem = Some(v as u16);
                            continue;
                        }
                        _ => {}
                    }
                    let (metric, number) = match value {
                        TrueTypeValue::Os2YSubscriptXSize(v) => (MetricType::SubscriptXSize, v),
                        TrueTypeValue::Os2YSubscriptYSize(v) => (MetricType::SubscriptYSize, v),
                        TrueTypeValue::Os2YSubscriptXOffset(v) => (MetricType::SubscriptXOffset, v),
                        TrueTypeValue::Os2YSubscriptYOffset(v) => (MetricType::SubscriptYOffset, v),
                        TrueTypeValue::Os2YSuperscriptXSize(v) => (MetricType::SuperscriptXSize, v),
                        TrueTypeValue::Os2YSuperscriptYSize(v) => (MetricType::SuperscriptYSize, v),
                        TrueTypeValue::Os2YSuperscriptXOffset(v) => {
                            (MetricType::SuperscriptXOffset, v)
                        }
                        TrueTypeValue::Os2YSuperscriptYOffset(v) => {
                            (MetricType::SuperscriptYOffset, v)
                        }
                        TrueTypeValue::Os2YStrikeoutSize(v) => (MetricType::StrikeoutSize, v),
                        TrueTypeValue::Os2YStrikeoutPosition(v) => {
                            (MetricType::StrikeoutPosition, v)
                        }
                        TrueTypeValue::Os2TypoAscender(v) => (MetricType::TypoAscender, v),
                        TrueTypeValue::Os2TypoDescender(v) => (MetricType::TypoDescender, v),
                        TrueTypeValue::Os2TypoLineGap(v) => (MetricType::TypoLineGap, v),
                        TrueTypeValue::Os2UsWinAscent(v) => (MetricType::WinAscent, v),
                        TrueTypeValue::Os2UsWinDescent(v) => (MetricType::WinDescent, v),
                        _ => continue,
                    };
                    set_metric_all_masters(&mut font, metric, number);
                }
            }
            VfbEntry::UnicodeRanges(_items) => {} // => todo!(),
            VfbEntry::FontNames(_name_records) => {} // => todo!(),
            VfbEntry::CustomCmaps(_raw_data) => {} // => todo!(),
            VfbEntry::PcltTable(_raw_data) => {}  // => todo!(),
            VfbEntry::FontFlags(_raw_data) => {}  // => todo!(),
            VfbEntry::MetricsClassFlags(_raw_data) => {} // => todo!(),
            VfbEntry::KerningClassFlags(_flags) => {
                // Already handled in first pass
            }
            VfbEntry::TrueTypeTable(_binary_table) => {} // => todo!(),
            VfbEntry::Features(code) => {
                let features = Features::from_fea(&code);
                font.features.features = features.features;
                font.features.prefixes = features.prefixes;
            }
            VfbEntry::BlockFontInfoEnd(_raw_data) => {}
            VfbEntry::BlockMMFontInfoStart(_raw_data) => {}
            VfbEntry::AnisotropicInterpolationMappings(_raw_data) => {} // => todo!(),
            VfbEntry::AxisMappingsCount(_) => {}                        // => todo!(),
            VfbEntry::AxisMappings(_raw_data) => {}                     // => todo!(),
            VfbEntry::PrimaryInstanceLocations(_items) => {}            // => todo!(),
            VfbEntry::PrimaryInstances(_raw_data) => {}                 // => todo!(),
            VfbEntry::BlockMMFontInfoEnd(_raw_data) => {}
            VfbEntry::GlobalGuides(guides) => {
                pending_global_guides = Some(guides);
            }
            VfbEntry::GlobalGuideProperties(props) => {
                pending_global_guide_props = props;
            }
            VfbEntry::GlobalMask(_raw_data) => {} // => todo!(),
            // VfbEntry::OpenTypeExportOptions(_raw_data) => {} // => todo!(),
            VfbEntry::ExportOptions(_export_options) => {} // => todo!(),
            VfbEntry::MappingMode(_raw_data) => {}         // => todo!(),
            VfbEntry::BlockMMKerningStart(_raw_data) => {}
            VfbEntry::MMKernPair(pair) => {
                mm_kern_pairs.push(pair);
            }
            VfbEntry::BlockMMKerningEnd(_raw_data) => {}
            VfbEntry::MasterLocation(_raw_data) => {
                // Already handled in first pass
            }
            VfbEntry::PostScriptInfo(info) => {
                postscript_infos.push(info);
            }
            VfbEntry::Cvt(_raw_data) => {}  // => todo!(),
            VfbEntry::Prep(_raw_data) => {} // => todo!(),
            VfbEntry::Fpgm(_raw_data) => {} // => todo!(),
            VfbEntry::Gasp(_raw_data) => {} // => todo!(),
            VfbEntry::Vdmx(_raw_data) => {} // => todo!(),
            VfbEntry::HheaAscender(asc) => {
                for master in font.masters.iter_mut() {
                    master
                        .metrics
                        .insert(crate::MetricType::Ascender, asc.into());
                    master
                        .metrics
                        .insert(crate::MetricType::HheaAscender, asc.into());
                }
            }

            VfbEntry::HheaDescender(desc) => {
                for master in font.masters.iter_mut() {
                    master
                        .metrics
                        .insert(crate::MetricType::Descender, desc.into());
                    master
                        .metrics
                        .insert(crate::MetricType::HheaDescender, desc.into());
                }
            }
            VfbEntry::TrueTypeStemPpems2And3(_raw_data) => {} // => todo!(),
            VfbEntry::TrueTypeStemPpems(_raw_data) => {}      // => todo!(),
            VfbEntry::TrueTypeStems(_raw_data) => {}          // => todo!(),
            VfbEntry::TrueTypeStemPpems1(_raw_data) => {}     // => todo!(),
            VfbEntry::TrueTypeZones(_raw_data) => {}          // => todo!(),
            VfbEntry::TrueTypeZoneDeltas(_raw_data) => {}     // => todo!(),
            VfbEntry::Glyph(items) => {
                // Before processing new glyph, apply any pending metadata to the previous glyph
                if let Some(glyph) = font.glyphs.last_mut() {
                    apply_glyph_metadata(
                        glyph,
                        pending_gdef.take(),
                        pending_anchors_mm.take(),
                        std::mem::take(&mut pending_guide_props),
                        pending_mask.take(),
                        pending_image,
                    )?;
                    pending_image = false;
                }
                load_glyph(&mut font, items, &mut raw_kerning)?;
            }
            VfbEntry::Links(_links) => {} // => todo!(),
            VfbEntry::Image(_raw_data) => {
                pending_image = true;
            }
            VfbEntry::Bitmaps(_raw_data) => {} // => todo!(),
            VfbEntry::VSB(_raw_data) => {}     // => todo!(),
            VfbEntry::Sketch(_raw_data) => {}  // => todo!(),
            VfbEntry::HintingOptions(_post_script_glyph_hinting_options) => {} // => todo!(),
            VfbEntry::Mask(nodes) => {
                pending_mask = Some(nodes);
            }
            VfbEntry::MaskMetrics(_raw_data) => {} // => todo!(),
            VfbEntry::MaskMetricsMm(_raw_data) => {} // => todo!(),
            VfbEntry::Origin(_) => {}              // => todo!(),
            VfbEntry::Unicodes(codepoints) => {
                // Set codepoints for last glyph
                if let Some(glyph) = font.glyphs.last_mut() {
                    glyph.codepoints = codepoints.iter().map(|&c| c as u32).collect();
                }
            }
            VfbEntry::UnicodesNonBmp(items) => {
                // Extend the codepoints
                if let Some(glyph) = font.glyphs.last_mut() {
                    glyph.codepoints.extend(items.iter().copied());
                }
            } // => todo!(),
            VfbEntry::GdefData(gdef) => {
                pending_gdef = Some(gdef);
            }
            VfbEntry::AnchorsProperties(_anchors_supplementals) => {} // Skip for now
            VfbEntry::AnchorsMm(items) => {
                pending_anchors_mm = Some(items);
            }
            VfbEntry::GuideProperties(props) => {
                pending_guide_props = props;
            }
            VfbEntry::BlockFileDataStart(_) => {} // todo!(),
            VfbEntry::BlockFontStart(_) => {}     // todo!(),
            VfbEntry::BlockNamesStart(_) => {}    // todo!(),
            VfbEntry::BlockFontEnd(_) | VfbEntry::BlockFileDataEnd(_) => {}
        }
    }

    // Apply any remaining anchors and layers to the last glyph
    if let Some(glyph) = font.glyphs.last_mut() {
        apply_glyph_metadata(
            glyph,
            pending_gdef.take(),
            pending_anchors_mm.take(),
            std::mem::take(&mut pending_guide_props),
            pending_mask.take(),
            pending_image,
        )?;
    }

    // Apply the per-master PostScript metrics (ascender, descender, x-height
    // and cap-height) to the matching masters.
    for (index, info) in postscript_infos.into_iter().enumerate() {
        if let Some(master) = font.masters.get_mut(index) {
            master
                .metrics
                .insert(crate::MetricType::Ascender, info.ascender);
            master
                .metrics
                .insert(crate::MetricType::Descender, info.descender);
            master
                .metrics
                .insert(crate::MetricType::XHeight, info.x_height);
            master
                .metrics
                .insert(crate::MetricType::CapHeight, info.cap_height);
        }
    }

    // VFB stores the *middle* of the underline, whereas OpenType stores its
    // top. Adjust the position by half the thickness, as FontLab's export does.
    for master in font.masters.iter_mut() {
        let position = master
            .metrics
            .get(&crate::MetricType::UnderlinePosition)
            .copied();
        let thickness = master
            .metrics
            .get(&crate::MetricType::UnderlineThickness)
            .copied();
        if let (Some(position), Some(thickness)) = (position, thickness) {
            master.metrics.insert(
                crate::MetricType::UnderlinePosition,
                underline_top(position, thickness),
            );
        }
    }

    // Apply global guides to masters
    if let Some(guides) = pending_global_guides {
        apply_global_guides_to_masters(&mut font, guides, &pending_global_guide_props)?;
    }

    // Fix up component ID->name references
    let names = font
        .glyphs
        .iter()
        .map(|g| g.name.clone())
        .collect::<Vec<_>>();
    for glyph in font.glyphs.iter_mut() {
        for layer in glyph.layers.iter_mut() {
            for shape in layer.shapes.iter_mut() {
                if let Shape::Component(component) = shape {
                    #[allow(clippy::unwrap_used)] // We put it there
                    let ref_id: usize = component.reference.parse().unwrap();
                    if let Some(ref_glyph) = names.get(ref_id) {
                        component.reference = ref_glyph.clone();
                    }
                }
            }
        }
    }

    apply_kerning(&mut font, &raw_kerning, &mm_kern_pairs, &names);
    Ok(font)
}

fn merge_kerning_group(
    groups: &mut IndexMap<SmolStr, Vec<SmolStr>>,
    name: &str,
    glyphs: &[String],
) {
    let entry = groups.entry(name.into()).or_default();
    for glyph in glyphs {
        let glyph: SmolStr = glyph.as_str().into();
        if !entry.contains(&glyph) {
            entry.push(glyph);
        }
    }
}

/// Populate feature classes and kerning groups from the VFB's OpenType class
/// definitions.
fn apply_class_definitions(
    font: &mut Font,
    classes: &[String],
    flags: &HashMap<String, (i32, i32)>,
) {
    const FIRST: i32 = 1 << 10;
    const SECOND: i32 = 1 << 11;

    for raw in classes {
        let Some((name, contents)) = raw.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let is_kerning = name.starts_with('_') || flags.contains_key(name);
        if !is_kerning {
            font.features.classes.insert(
                name.into(),
                PossiblyAutomaticCode::new(contents.replace('\'', "")),
            );
            continue;
        }

        // In kerning classes, the key glyph(s) are marked with a trailing `'`
        // and must come first in the group.
        let mut key_glyphs = Vec::new();
        let mut glyphs = Vec::new();
        for token in contents.split_whitespace() {
            let token = token.trim();
            let (glyph, is_key) = match token.strip_suffix('\'') {
                Some(glyph) => (glyph, true),
                None => (token, false),
            };
            let glyph = glyph.trim_matches(|c| c == '\'' || c == '"');
            if glyph.is_empty() {
                continue;
            }
            if is_key {
                key_glyphs.push(glyph.to_string());
            } else {
                glyphs.push(glyph.to_string());
            }
        }
        let mut ordered = key_glyphs;
        ordered.extend(glyphs);
        if ordered.is_empty() {
            continue;
        }
        let group_name = name.trim_start_matches('_');
        if group_name.is_empty() {
            continue;
        }

        match flags.get(name).copied() {
            Some((side_flags, _)) => {
                if side_flags & FIRST != 0 {
                    merge_kerning_group(&mut font.first_kern_groups, group_name, &ordered);
                }
                if side_flags & SECOND != 0 {
                    merge_kerning_group(&mut font.second_kern_groups, group_name, &ordered);
                }
            }
            None => {
                // Without side flags we cannot tell which side the class
                // belongs to, so preserve it on both.
                merge_kerning_group(&mut font.first_kern_groups, group_name, &ordered);
                merge_kerning_group(&mut font.second_kern_groups, group_name, &ordered);
            }
        }
    }
}

fn key_glyph_map(groups: &IndexMap<SmolStr, Vec<SmolStr>>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (group, glyphs) in groups {
        if let Some(key_glyph) = glyphs.first() {
            map.entry(key_glyph.to_string())
                .or_insert_with(|| format!("@{}", group));
        }
    }
    map
}

/// Resolve glyph-index kerning pairs into master kerning maps, replacing key
/// glyphs with their `@group` names.
fn apply_kerning(
    font: &mut Font,
    raw_kerning: &[(String, HashMap<i32, Vec<i32>>)],
    mm_kern_pairs: &[[i16; 5]],
    names: &[SmolStr],
) {
    let first_keys = key_glyph_map(&font.first_kern_groups);
    let second_keys = key_glyph_map(&font.second_kern_groups);
    let left_key = |name: &str| -> SmolStr {
        first_keys
            .get(name)
            .map(|group| SmolStr::new(group.as_str()))
            .unwrap_or_else(|| SmolStr::new(name))
    };
    let right_key = |name: &str| -> SmolStr {
        second_keys
            .get(name)
            .map(|group| SmolStr::new(group.as_str()))
            .unwrap_or_else(|| SmolStr::new(name))
    };

    for (left_name, right_map) in raw_kerning {
        let left = left_key(left_name);
        for (right_id, values) in right_map {
            let Some(right_name) = usize::try_from(*right_id)
                .ok()
                .and_then(|index| names.get(index))
            else {
                continue;
            };
            let right = right_key(right_name.as_str());
            for (master_index, value) in values.iter().enumerate() {
                if let Some(master) = font.masters.get_mut(master_index) {
                    master
                        .kerning
                        .insert((left.clone(), right.clone()), *value as i16);
                }
            }
        }
    }

    // Older VFB files may store multiple-master kerning in an `MM Kern Pair`
    // block: left GID, right GID, then one value per master.
    for pair in mm_kern_pairs {
        let Some(left_name) = usize::try_from(pair[0])
            .ok()
            .and_then(|index| names.get(index))
        else {
            continue;
        };
        let Some(right_name) = usize::try_from(pair[1])
            .ok()
            .and_then(|index| names.get(index))
        else {
            continue;
        };
        let left = left_key(left_name.as_str());
        let right = right_key(right_name.as_str());
        for (master_index, value) in pair[2..].iter().enumerate() {
            if let Some(master) = font.masters.get_mut(master_index) {
                master.kerning.insert((left.clone(), right.clone()), *value);
            }
        }
    }
}

fn load_glyph(
    font: &mut Font,
    items: Vec<GlyphEntry>,
    raw_kerning: &mut Vec<(String, HashMap<i32, Vec<i32>>)>,
) -> Result<(), BabelfontError> {
    let mut glyph = Glyph::new("unnamed");
    glyph.exported = true;
    // Create a layer for each master, with master name as layer name
    for master in &font.masters {
        let mut layer = Layer::new(0.0); // will fix up with metrics
        layer.master = LayerType::DefaultForMaster(master.id.clone());
        layer.id = Some(master.id.clone());
        // Set layer name to master name or master ID as fallback
        if let Some(master_name) = master.name.get_default() {
            layer.name = Some(master_name.clone());
        } else {
            layer.name = Some(master.id.clone());
        }
        glyph.layers.push(layer);
    }

    for item in items {
        match item {
            GlyphEntry::GlyphName(s) => {
                glyph.name = s.into();
            }
            GlyphEntry::Metrics(items) => {
                for (i, (width, _height)) in items.into_iter().enumerate() {
                    if let Some(layer) = glyph.layers.get_mut(i) {
                        layer.width = width as f32;
                    }
                }
            }
            GlyphEntry::Hints(_hints) => {} // => todo!(),
            GlyphEntry::Guides(guides) => {
                // Apply glyph-level guides to their corresponding master layers
                // VFB guides are stored as one guide per master per guide position
                // Skip mask and image layers (these come first if they exist)
                let master_layer_start = glyph
                    .layers
                    .iter()
                    .position(|l| {
                        l.name.as_deref() != Some("mask") && l.name.as_deref() != Some("img")
                    })
                    .unwrap_or(0);

                // Apply horizontal guides
                // Each guide_set contains one guide per master for that position
                for guide_set in guides.horizontal() {
                    for (master_index, vfb_guide) in guide_set.iter().enumerate() {
                        let layer_index = master_layer_start + master_index;
                        if let Some(layer) = glyph.layers.get_mut(layer_index) {
                            let guide_obj = Guide {
                                pos: crate::common::Position {
                                    x: 0.0,
                                    y: vfb_guide.position() as f32,
                                    angle: vfb_guide.angle(),
                                },
                                name: None,
                                color: None,
                                format_specific: FormatSpecific::default(),
                            };
                            layer.guides.push(guide_obj);
                        }
                    }
                }

                // Apply vertical guides
                for guide_set in guides.vertical() {
                    for (master_index, vfb_guide) in guide_set.iter().enumerate() {
                        let layer_index = master_layer_start + master_index;
                        if let Some(layer) = glyph.layers.get_mut(layer_index) {
                            let guide_obj = Guide {
                                pos: crate::common::Position {
                                    x: vfb_guide.position() as f32,
                                    y: 0.0,
                                    angle: 90.0 - vfb_guide.angle(),
                                },
                                name: None,
                                color: None,
                                format_specific: FormatSpecific::default(),
                            };
                            layer.guides.push(guide_obj);
                        }
                    }
                }
            }
            GlyphEntry::Components(components) => {
                for (index, layer) in glyph.layers.iter_mut().enumerate() {
                    for component in components.iter() {
                        let our_component = crate::shape::Component {
                            reference: component.glyph_index.to_string().into(), // Will fix later
                            transform: DecomposedAffine {
                                translation: (
                                    *component.x_offset.get(index).ok_or_else(|| {
                                        BabelfontError::GlyphNotInterpolatable {
                                            glyph: glyph.name.clone().into(),
                                            reason: "Not enough coordinates for component"
                                                .to_string(),
                                        }
                                    })? as f64,
                                    *component.y_offset.get(index).ok_or_else(|| {
                                        BabelfontError::GlyphNotInterpolatable {
                                            glyph: glyph.name.clone().into(),
                                            reason: "Not enough coordinates for component"
                                                .to_string(),
                                        }
                                    })? as f64,
                                ),
                                scale: (
                                    *component.x_scale.get(index).ok_or_else(|| {
                                        BabelfontError::GlyphNotInterpolatable {
                                            glyph: glyph.name.clone().into(),
                                            reason: "Not enough coordinates for component"
                                                .to_string(),
                                        }
                                    })?,
                                    *component.y_scale.get(index).ok_or_else(|| {
                                        BabelfontError::GlyphNotInterpolatable {
                                            glyph: glyph.name.clone().into(),
                                            reason: "Not enough coordinates for component"
                                                .to_string(),
                                        }
                                    })?,
                                ),
                                order: TransformOrder::Glyphs,
                                ..Default::default()
                            },
                            location: IndexMap::new(),
                            format_specific: FormatSpecific::default(),
                        };
                        layer.shapes.push(Shape::Component(our_component));
                    }
                }
            } // => todo!(),
            GlyphEntry::Kerning(hash_map) => {
                if !hash_map.is_empty() {
                    raw_kerning.push((glyph.name.to_string(), hash_map));
                }
            }
            GlyphEntry::Outlines(nodes) => {
                // For each layer (i.e. master), build the paths
                for (index, layer) in glyph.layers.iter_mut().enumerate() {
                    let paths = outline_to_paths(&nodes, index, glyph.name.as_str())?;
                    layer.shapes.extend(paths.into_iter().map(Shape::Path));
                }
            }
            GlyphEntry::Binary(_raw_data) => {} // => todo!(),
            GlyphEntry::Instructions(_raw_data) => {} // => todo!(),
        }
    }
    font.glyphs.push(glyph);
    Ok(())
}

/// Apply the metadata entries which follow a glyph's outline entry.
fn apply_glyph_metadata(
    glyph: &mut Glyph,
    gdef: Option<GlyphGDEF>,
    anchors_mm: Option<Vec<Vec<VfbAnchor>>>,
    guide_props: Vec<GuideProperty>,
    mask_nodes: Option<Vec<VFBNode>>,
    has_image: bool,
) -> Result<(), BabelfontError> {
    apply_anchors_to_glyph(glyph, gdef.as_ref(), anchors_mm)?;
    apply_guide_properties(glyph, &guide_props);
    let has_real_mask = mask_nodes
        .as_ref()
        .is_some_and(|mask_nodes| !mask_nodes.is_empty());
    if let Some(mask_nodes) = mask_nodes {
        apply_mask_layer_to_glyph(glyph, &mask_nodes)?;
    }
    if has_image {
        // FontLab only copies the master anchors onto the image layer when
        // the image layer is the sole background layer (i.e. no mask).
        apply_image_layer_to_glyph(glyph, !has_real_mask);
    }
    Ok(())
}

/// Parse a `#rrggbb` colour string.
fn parse_hex_color(value: &str) -> Option<crate::common::Color> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    Some(crate::common::Color {
        r: i32::from_str_radix(&hex[0..2], 16).ok()?,
        g: i32::from_str_radix(&hex[2..4], 16).ok()?,
        b: i32::from_str_radix(&hex[4..6], 16).ok()?,
        a: 255,
    })
}

/// Apply names and colours from a VFB guide-properties entry to a glyph's
/// guides. `index` is 1-based into the flat horizontal-then-vertical list.
fn apply_guide_properties(glyph: &mut Glyph, properties: &[GuideProperty]) {
    for layer in glyph.layers.iter_mut() {
        for property in properties {
            let index = property.index - 1;
            if index < 0 {
                continue;
            }
            if let Some(guide) = layer.guides.get_mut(index as usize) {
                if let Some(name) = &property.name {
                    guide.name = Some(name.clone());
                }
                if let Some(color) = property.color.as_deref().and_then(parse_hex_color) {
                    guide.color = Some(color);
                }
            }
        }
    }
}

/// Apply anchor data to a glyph's master layers.
///
/// Anchor names (and, for single-master fonts, anchor positions) live in the
/// glyph's GDEF entry. Multiple-master fonts store per-master positions in a
/// separate `Glyph Anchors MM` entry, in the same order as the GDEF anchors.
fn apply_anchors_to_glyph(
    glyph: &mut Glyph,
    gdef: Option<&GlyphGDEF>,
    anchors_mm: Option<Vec<Vec<VfbAnchor>>>,
) -> Result<(), BabelfontError> {
    let anchor_name = |index: usize| -> String {
        gdef.and_then(|gdef| gdef.anchors.get(index))
            .and_then(|anchor| anchor.name.clone())
            .unwrap_or_default()
    };

    if let Some(anchors_mm) = anchors_mm {
        // Outer vector is anchors, inner vector is one position per master.
        for (anchor_index, per_master) in anchors_mm.iter().enumerate() {
            let name = anchor_name(anchor_index);
            for (master_index, anchor) in per_master.iter().enumerate() {
                if let Some(layer) = glyph.layers.get_mut(master_index) {
                    layer.anchors.push(Anchor {
                        x: anchor.x as f64,
                        y: anchor.y as f64,
                        name: name.clone(),
                        format_specific: FormatSpecific::default(),
                    });
                }
            }
        }
    } else if let Some(gdef) = gdef {
        // Single-master fonts keep the anchor positions in GDEF itself.
        for layer in glyph.layers.iter_mut() {
            for anchor in &gdef.anchors {
                layer.anchors.push(Anchor {
                    x: anchor.x as f64,
                    y: anchor.y as f64,
                    name: anchor.name.clone().unwrap_or_default(),
                    format_specific: FormatSpecific::default(),
                });
            }
        }
    }
    Ok(())
}

/// Add the mask (background) layer to a glyph.
///
/// VFB stores a mask outline in the same format as the glyph outline. An empty
/// mask entry (no contours) does not produce a layer, matching FontLab's VFJ
/// output.
fn apply_mask_layer_to_glyph(
    glyph: &mut Glyph,
    mask_nodes: &[VFBNode],
) -> Result<(), BabelfontError> {
    if mask_nodes.is_empty() {
        return Ok(());
    }
    let width = glyph.layers.first().map(|layer| layer.width).unwrap_or(0.0);
    let paths = outline_to_paths(mask_nodes, 0, glyph.name.as_str())?;
    let mut mask_layer = Layer::new(width);
    mask_layer.name = Some("mask".to_string());
    mask_layer.id = Some("mask".to_string());
    mask_layer.master = LayerType::FreeFloating; // Not associated with any master
    mask_layer.shapes = paths.into_iter().map(Shape::Path).collect();
    glyph.layers.insert(0, mask_layer); // Insert at the start so mask comes first
    Ok(())
}

/// Add an image/background layer to a glyph (for FontLab image support)
fn apply_image_layer_to_glyph(glyph: &mut Glyph, copy_anchors: bool) {
    if glyph
        .layers
        .iter()
        .any(|layer| layer.name.as_deref() == Some("img"))
    {
        return;
    }
    // The mask layer may already have been inserted at index 0, so find a
    // real master layer rather than assuming `first()`.
    let master_layer = glyph
        .layers
        .iter()
        .find(|layer| matches!(layer.master, LayerType::DefaultForMaster(_)))
        .or_else(|| glyph.layers.first());
    let width = master_layer.map(|layer| layer.width).unwrap_or(0.0);
    let anchors = if copy_anchors {
        master_layer
            .map(|layer| layer.anchors.clone())
            .unwrap_or_default()
    } else {
        vec![]
    };
    let mut image_layer = Layer::new(width);
    image_layer.name = Some("img".to_string());
    image_layer.id = Some("img".to_string());
    image_layer.master = LayerType::FreeFloating; // Not associated with any master
    image_layer.anchors = anchors;
    // XXX: The bitmap itself is still stored as raw data by the reader.
    glyph.layers.insert(0, image_layer); // Insert at the start so img comes first
}

/// Apply global guides to font masters
fn apply_global_guides_to_masters(
    font: &mut Font,
    guides: VfbGuides,
    properties: &[GuideProperty],
) -> Result<(), BabelfontError> {
    // VFB guides are organized as: Vec of guides, each with one entry per master
    // We need to apply each guide entry to the corresponding master

    for (master_index, master) in font.masters.iter_mut().enumerate() {
        // Collect guides for this master from both horizontal and vertical guides
        let mut guides_for_master = Vec::new();

        // Add horizontal guides
        for guide_set in guides.horizontal() {
            if let Some(vfb_guide) = guide_set.get(master_index) {
                let guide = Guide {
                    pos: crate::common::Position {
                        x: 0.0,
                        y: vfb_guide.position() as f32,
                        angle: vfb_guide.angle(),
                    },
                    name: None,
                    color: None,
                    format_specific: FormatSpecific::default(),
                };
                guides_for_master.push(guide);
            }
        }

        // Add vertical guides
        for guide_set in guides.vertical() {
            if let Some(vfb_guide) = guide_set.get(master_index) {
                let guide = Guide {
                    pos: crate::common::Position {
                        x: vfb_guide.position() as f32,
                        y: 0.0,
                        angle: 90.0 - vfb_guide.angle(),
                    },
                    name: None,
                    color: None,
                    format_specific: FormatSpecific::default(),
                };
                guides_for_master.push(guide);
            }
        }

        // Apply guide names/colours by 1-based index.
        for property in properties {
            let index = property.index - 1;
            if index < 0 {
                continue;
            }
            if let Some(guide) = guides_for_master.get_mut(index as usize) {
                if let Some(name) = &property.name {
                    guide.name = Some(name.clone());
                }
                if let Some(color) = property.color.as_deref().and_then(parse_hex_color) {
                    guide.color = Some(color);
                }
            }
        }

        master.guides.extend(guides_for_master);
    }

    Ok(())
}

/// Fetch one master's coordinates from a VFB node, converting the error when
/// the node does not have a point for that master.
fn master_coords(
    coords: &[(i32, i32)],
    index: usize,
    what: &str,
    glyph_name: &str,
) -> Result<(f64, f64), BabelfontError> {
    let (x, y) =
        coords
            .get(index)
            .copied()
            .ok_or_else(|| BabelfontError::GlyphNotInterpolatable {
                glyph: glyph_name.to_string(),
                reason: format!("Not enough coordinates for {what} node"),
            })?;
    Ok((x as f64, y as f64))
}

/// Convert one master's worth of VFB outline nodes into paths.
///
/// VFB stores quadratic curves differently from every other format we support.
/// Rather than pairing each off-curve control point with its own on-curve
/// endpoint, it emits a run of `qcurve` nodes (the off-curve control points)
/// terminated by a `line` node which is the on-curve point ending the run. (A
/// `line` node which is *not* preceded by `qcurve` nodes is a real straight
/// line.) We rebuild the conventional representation here, i.e. a run of
/// off-curve nodes ended by a single on-curve `QCurve` node.
fn outline_to_paths(
    nodes: &[VFBNode],
    master_index: usize,
    glyph_name: &str,
) -> Result<Vec<Path>, BabelfontError> {
    let mut paths = Vec::new();
    // The nodes of the contour we are currently building, in UFO point-pen
    // order.
    let mut contour: Vec<Node> = Vec::new();
    // Whether the node we last saw was a `qcurve` control point.
    let mut in_qcurve = false;
    // Whether the current contour is open (flag bit 3) rather than closed.
    let mut is_open = false;

    for node in nodes {
        match node {
            VFBNode::Move { coords, flags } => {
                if !contour.is_empty() {
                    finish_contour(&mut contour, is_open, &mut paths);
                }
                let (x, y) = master_coords(coords, master_index, "move", glyph_name)?;
                is_open = flags & 8 != 0;
                in_qcurve = false;
                let mut node = Node::new_move(x, y);
                node.smooth = flags & 1 != 0;
                contour.push(node);
            }
            VFBNode::Line { coords, flags } => {
                let (x, y) = master_coords(coords, master_index, "line", glyph_name)?;
                if in_qcurve {
                    // A line following qcurve control points is the on-curve
                    // point which ends the quadratic run.
                    let mut node = Node::new_qcurve(x, y);
                    node.smooth = flags & 1 != 0;
                    contour.push(node);
                    in_qcurve = false;
                } else {
                    let mut node = Node::new_line(x, y);
                    node.smooth = flags & 1 != 0;
                    contour.push(node);
                }
            }
            VFBNode::QCurve { coords, .. } => {
                // A qcurve node is an off-curve control point, not the end of
                // the segment.
                let (x, y) = master_coords(coords, master_index, "qcurve", glyph_name)?;
                contour.push(Node::new_offcurve(x, y));
                in_qcurve = true;
            }
            VFBNode::Curve {
                coords,
                c1_coords,
                c2_coords,
                flags,
            } => {
                let (c1x, c1y) = master_coords(c1_coords, master_index, "c1", glyph_name)?;
                let (c2x, c2y) = master_coords(c2_coords, master_index, "c2", glyph_name)?;
                let (x, y) = master_coords(coords, master_index, "curve", glyph_name)?;
                contour.push(Node::new_offcurve(c1x, c1y));
                contour.push(Node::new_offcurve(c2x, c2y));
                let mut node = Node::new_curve(x, y);
                node.smooth = flags & 1 != 0;
                contour.push(node);
                in_qcurve = false;
            }
        }
    }
    if !contour.is_empty() {
        finish_contour(&mut contour, is_open, &mut paths);
    }
    Ok(paths)
}

/// Finish the contour currently under construction, applying the same
/// closepath fixups that vfbLib performs.
fn finish_contour(contour: &mut Vec<Node>, is_open: bool, paths: &mut Vec<Path>) {
    if contour.is_empty() {
        return;
    }

    if !is_open {
        let last_index = contour.len() - 1;
        let last_nodetype = &contour[last_index].nodetype;
        if last_nodetype == &NodeType::OffCurve {
            // Trailing control points wrap around to the contour's start point,
            // which is therefore the on-curve node ending a quadratic run.
            contour[0].nodetype = NodeType::QCurve;
        } else {
            let closes_on_start =
                contour[0].x == contour[last_index].x && contour[0].y == contour[last_index].y;
            if contour.len() > 1 && closes_on_start {
                // The last node coincides with the start point and represents
                // the closing segment, which is implicit in point-pen order.
                // Drop the duplicate and carry the closing node's type over to
                // the start point.
                #[allow(clippy::unwrap_used)] // The contour is non-empty
                let last = contour.pop().unwrap();
                if matches!(last.nodetype, NodeType::Line | NodeType::QCurve) {
                    contour[0].nodetype = last.nodetype;
                    contour[0].smooth = last.smooth;
                } else {
                    contour[0] = last;
                }
            } else {
                // Otherwise the closing segment is a straight line back to the
                // start point.
                contour[0].nodetype = NodeType::Line;
            }
        }
    }

    // Babelfont stores closed contours rotated one place from the UFO point
    // order (see `convertors::ufo::load_path`).
    let mut nodes = std::mem::take(contour);
    nodes.rotate_left(1);
    paths.push(Path {
        nodes,
        closed: !is_open,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vfb_underline_top() {
        // An odd thickness: FontLab's export writes -102 + 51 / 2 = -77.
        assert_eq!(underline_top(-102, 51), -77);
        assert_eq!(underline_top(-100, 50), -75);
    }

    #[test]
    fn test_vfb_os2_values() -> Result<(), BabelfontError> {
        let font = load(PathBuf::from("resources/IBMPlexSerif-Text.vfb"))?;
        let ot = &font.custom_ot_values;
        // The weight code, not the TrueType block's 400.
        assert_eq!(ot.os2_us_weight_class, Some(450));
        assert_eq!(ot.os2_us_width_class, Some(5));
        assert_eq!(ot.os2_fs_type, Some(0));
        assert_eq!(ot.head_lowest_rec_ppem, Some(9));
        assert_eq!(ot.os2_panose, Some([2, 6, 5, 3, 5, 4, 6, 0, 2, 3]));
        assert_eq!(ot.os2_vendor_id, Some(Tag::new(b"IBM ")));
        Ok(())
    }

    #[test]
    fn test_vfb_vendor_tag() {
        assert_eq!(vendor_tag("IBM"), Some(Tag::new(b"IBM ")));
        assert_eq!(vendor_tag("STC\0"), Some(Tag::new(b"STC ")));
        assert_eq!(vendor_tag("GOOG"), Some(Tag::new(b"GOOG")));
        assert_eq!(vendor_tag(""), None);
        assert_eq!(vendor_tag("   "), None);
        // A non-space after a space is not a legal tag.
        assert_eq!(vendor_tag("Ek T"), None);
        assert_eq!(vendor_tag("TOOLONG"), None);
    }

    #[test]
    fn test_vfb_kerning_classes_and_pairs() {
        let mut font = Font::new();
        font.masters
            .push(Master::new("Thin", "m1", Default::default()));
        font.masters
            .push(Master::new("Black", "m2", Default::default()));

        let classes = vec![
            // `A` is the key glyph for the first-side group.
            "_A: A' Delta".to_string(),
            // `C` is the key glyph for the second-side group.
            "_C: C' G O Q".to_string(),
        ];
        let mut flags = HashMap::new();
        flags.insert("_A".to_string(), (1 << 10, 0));
        flags.insert("_C".to_string(), (1 << 11, 0));
        apply_class_definitions(&mut font, &classes, &flags);

        assert_eq!(
            font.first_kern_groups.get("A").cloned(),
            Some(vec![SmolStr::new("A"), SmolStr::new("Delta")])
        );
        assert_eq!(
            font.second_kern_groups.get("C").cloned(),
            Some(vec![
                SmolStr::new("C"),
                SmolStr::new("G"),
                SmolStr::new("O"),
                SmolStr::new("Q")
            ])
        );

        let names = ["A", "C", "question", "Delta", "G", "O", "Q"]
            .iter()
            .map(|name| SmolStr::new(*name))
            .collect::<Vec<_>>();

        // Glyph-index kerning from the glyph entries.
        let mut right = HashMap::new();
        right.insert(1, vec![-21, -30]); // C
        right.insert(2, vec![-81, -90]); // question
        let raw_kerning = vec![("A".to_string(), right)];

        // Older MM kerning block: Delta / G, one value per master.
        let mm_kern_pairs = vec![[3, 4, 3, 4, 0]];

        apply_kerning(&mut font, &raw_kerning, &mm_kern_pairs, &names);

        assert_eq!(
            font.masters[0]
                .kerning
                .get(&(SmolStr::new("@A"), SmolStr::new("@C"))),
            Some(&-21)
        );
        assert_eq!(
            font.masters[1]
                .kerning
                .get(&(SmolStr::new("@A"), SmolStr::new("@C"))),
            Some(&-30)
        );
        assert_eq!(
            font.masters[0]
                .kerning
                .get(&(SmolStr::new("@A"), SmolStr::new("question"))),
            Some(&-81)
        );
        assert_eq!(
            font.masters[0]
                .kerning
                .get(&(SmolStr::new("Delta"), SmolStr::new("G"))),
            Some(&3)
        );
        assert_eq!(
            font.masters[1]
                .kerning
                .get(&(SmolStr::new("Delta"), SmolStr::new("G"))),
            Some(&4)
        );
    }
}
