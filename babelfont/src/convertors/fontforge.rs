use std::{collections::HashMap, fs, path::PathBuf, sync::LazyLock};

use chrono::DateTime;
use fea_rs_ast::AsFea;

use crate::{
    common::{decomposition::DecomposedAffine, tag_from_string, Color, Node, NodeType},
    convertors::fontforge::{
        layout::{make_langsys, GTable},
        utf7::decode_utf7,
    },
    names::ot_lang_id_to_iso_tag,
    BabelfontError, Component, Font, FormatSpecific, Glyph, GlyphCategory, Guide, Layer, LayerType,
    MetricType, NameId, Path, Shape,
};
use indexmap::IndexMap;
use smol_str::SmolStr;

mod layout;
mod utf7;

use regex::Regex;
static FEATURE_NAME_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // Expected format: '<feature tag>' <language code> "<feature name>"
    #[allow(clippy::unwrap_used)] // Safe because the regex is valid
    Regex::new(r#"'(?P<tag>.{4})'\s+(?P<lang>\d+)\s+"(?P<name>.+)""#).unwrap()
});

const GENERATED_KERN_SUBTABLE: &str = "generated_kern";
const HEADER_VERSION_KEY: &str = "sfd.splinefontdb_version";
const COMMENT_ENTRIES_KEY: &str = "sfd.comment_entries";
const HSTEM_KEY: &str = "sfd.HStem";
const VSTEM_KEY: &str = "sfd.VStem";
const LAYER_QUADRATIC_KEY: &str = "sfd.is_quadratic";

/// A parser for the FontForge SFD/SFDir text format.
struct SfdParser {
    path: PathBuf,
    font: Font,
    layer_defs: Vec<Option<LayerDefinition>>, // Indexed by SFD layer index
    // Kerning data parsed from SFD
    kern_classes: IndexMap<String, KernClass>,
    // subtable -> left glyph name -> [(right gid index, value)]
    kern_pairs: IndexMap<String, IndexMap<String, Vec<(usize, i16)>>>,
    gsub_lookups: GTable,
    gpos_lookups: GTable,
    feature_names: IndexMap<SmolStr, Vec<(u32, String)>>, // feature tag -> feature name
    sanitized_lookup_names: HashMap<String, usize>, // track sanitized names for de-duplication
    content: Option<String>,                        // Optional pre-loaded content for load_str()
}

#[derive(Debug, Clone, Default)]
struct LayerDefinition {
    name: Option<String>,
    #[allow(dead_code)]
    is_quadratic: bool,
    #[allow(dead_code)]
    flags: usize,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
struct KernClass {
    groups1: Vec<Vec<String>>, // first-side groups
    groups2: Vec<Vec<String>>, // second-side groups (index 0 is implicit None)
    kerns: Vec<i16>,           // flattened device table values
}

macro_rules! parse_metric {
    ($self:ident, $value:expr, $metric:ident) => {
        if let Some(v) = &$value {
            if let Ok(val) = v.parse::<i32>() {
                $self.font.masters[0]
                    .metrics
                    .insert(MetricType::$metric, val);
            }
        }
    };
}

fn remove_implicit_move_in_closed_path(p: &mut Path) {
    #[allow(clippy::unwrap_used)] // We check for is_empty() before, so unwrap is safe
    if p.closed
        && p.nodes.len() > 1
        && p.nodes.first().map(|n| n.nodetype) == Some(NodeType::Move)
        && p.nodes.first().unwrap().x == p.nodes.last().unwrap().x
        && p.nodes.first().unwrap().y == p.nodes.last().unwrap().y
    {
        p.nodes = p.nodes[1..].to_vec(); // Remove the initial move node if path is closed
    }
}

type SplineSegment = (Vec<(f64, f64)>, char, String);

fn layer_is_quadratic(layer: &Layer) -> bool {
    layer
        .format_specific
        .get(LAYER_QUADRATIC_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

impl SfdParser {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            font: Font::new(),
            layer_defs: Vec::new(),
            kern_classes: IndexMap::new(),
            kern_pairs: IndexMap::new(),
            gsub_lookups: GTable(IndexMap::new()),
            gpos_lookups: GTable(IndexMap::new()),
            feature_names: IndexMap::new(),
            sanitized_lookup_names: HashMap::new(),
            content: None,
        }
    }

    fn new_from_str(content: String) -> Self {
        Self {
            path: PathBuf::from("<string>"),
            font: Font::new(),
            layer_defs: Vec::new(),
            kern_classes: IndexMap::new(),
            kern_pairs: IndexMap::new(),
            gsub_lookups: GTable(IndexMap::new()),
            gpos_lookups: GTable(IndexMap::new()),
            feature_names: IndexMap::new(),
            sanitized_lookup_names: HashMap::new(),
            content: Some(content),
        }
    }

    /// Read the SFD file or SFDir `font.props` into a vector of lines.
    fn read_data(&self) -> Result<Vec<String>, BabelfontError> {
        // If content was pre-loaded (from load_str), use that
        if let Some(content) = &self.content {
            return Ok(content.lines().map(|l| l.to_string()).collect());
        }

        if self.path.is_dir() {
            let props = self.path.join("font.props");
            if !props.is_file() {
                return Err(BabelfontError::General(
                    "Not an SFD directory: missing font.props".to_string(),
                ));
            }
            let content = read_file_lossy(&props)?;
            return Ok(content.lines().map(|l| l.to_string()).collect());
        }

        let content = read_file_lossy(&self.path)?;
        Ok(content.lines().map(|l| l.to_string()).collect())
    }

    /// Collect lines from the current index up to (but not including) the end marker.
    /// The optional `first_line` mirrors the Python parser's behaviour of seeding the
    /// section with the value that appeared on the same line as the start marker.
    fn get_section(
        &self,
        data: &[String],
        start_index: usize,
        end: &str,
        first_line: Option<&str>,
    ) -> (Vec<String>, usize) {
        let mut section = Vec::new();
        if let Some(value) = first_line {
            section.push(value.to_string());
        }

        let mut idx = start_index;
        while idx < data.len() {
            let line = &data[idx];
            if line.starts_with(end) {
                return (section, idx + 1);
            }
            section.push(line.to_string());
            idx += 1;
        }

        // If we run out of data, return what we have; later phases can make this an error.
        (section, idx)
    }

    // /// Pretty-print a captured section to the console.
    // fn print_section(label: &str, lines: &[String]) {
    //     println!("== {label} ==");
    //     for line in lines {
    //         println!("{line}");
    //     }
    // }

    /// Parse the SFD/SFDir data into a Font structure.
    fn parse(&mut self) -> Result<(), BabelfontError> {
        let data = self.read_data()?;

        if data.is_empty() {
            return Err(BabelfontError::General(
                "Empty SFD file; nothing to parse".to_string(),
            ));
        }

        let mut i = 0usize;
        let mut first_line_checked = false;
        // Storage for glyph block
        let mut char_data: Option<Vec<String>> = None;

        // Ensure we have a default master
        if self.font.masters.is_empty() {
            let mut master: crate::Master = Default::default();
            if master.id.is_empty() {
                master.id = "default".to_string();
            }
            self.font.masters.push(master);
        }
        let master_id = self.font.masters[0].id.clone();

        while i < data.len() {
            let raw_line = &data[i];
            i += 1;

            // Ignore purely empty lines to reduce noise.
            if raw_line.trim().is_empty() {
                continue;
            }

            let (key, value, raw_value) = if let Some(pos) = raw_line.find(':') {
                let (k, v) = raw_line.split_at(pos);
                (
                    k.trim().to_string(),
                    Some(v[1..].trim().to_string()),
                    Some(v[1..].to_string()),
                )
            } else {
                (raw_line.trim().to_string(), None, None)
            };

            if !first_line_checked {
                first_line_checked = true;
                if key != "SplineFontDB" {
                    return Err(BabelfontError::General(
                        "Not an SFD file (missing SplineFontDB header)".to_string(),
                    ));
                }
                // println!("SplineFontDB version {}", value.unwrap_or_default());
                if let Some(v) = value {
                    self.font
                        .format_specific
                        .insert(HEADER_VERSION_KEY.to_string(), serde_json::Value::String(v));
                }
                continue;
            }

            match key.as_str() {
                // Sections with explicit start/end markers
                "BeginPrivate" => {
                    let (section, next_i) =
                        self.get_section(&data, i, "EndPrivate", value.as_deref());
                    self.font.format_specific.insert(
                        "sfd.private_section".to_string(),
                        serde_json::Value::Array(
                            section.into_iter().map(serde_json::Value::String).collect(),
                        ),
                    );
                    // println!(
                    //     "BeginPrivate: captured {} lines (end marker EndPrivate)",
                    //     section.len()
                    // );
                    // Self::print_section("BeginPrivate", &section);
                    i = next_i;
                }
                "BeginChars" => {
                    if let Some(v) = &value {
                        let mut parts = v.split_whitespace();
                        if let Some(first) = parts.next() {
                            if let Ok(slots) = first.parse::<usize>() {
                                self.font.format_specific.insert(
                                    "sfd.beginchars_slots".to_string(),
                                    serde_json::Value::Number(slots.into()),
                                );
                            }
                        }
                        if let Some(second) = parts.next() {
                            if let Ok(count) = second.parse::<usize>() {
                                self.font.format_specific.insert(
                                    "sfd.beginchars_count".to_string(),
                                    serde_json::Value::Number(count.into()),
                                );
                            }
                        }
                    }
                    let (section, next_i) = self.get_section(&data, i, "EndChars", None);
                    self.font.format_specific.insert(
                        "sfd.beginchars_blank_line".to_string(),
                        serde_json::Value::Bool(
                            section.first().map(|line| line.is_empty()).unwrap_or(false),
                        ),
                    );
                    char_data = Some(section);
                    i = next_i;
                }
                "ShortTable" => {
                    let (_section, next_i) = self.get_section(&data, i, "EndShort", None);
                    // Ignore
                    i = next_i;
                }
                "TtTable" => {
                    let (_section, next_i) = self.get_section(&data, i, "EndTTInstrs", None);
                    // Ignore
                    i = next_i;
                }
                "KernClass2" => {
                    if let Some(v) = &value {
                        i = self.parse_kern_class(&data, i, v);
                    }
                }
                "ContextPos2" | "ContextSub2" | "ChainPos2" | "ChainSub2" | "ReverseChain2" => {
                    let (_section, next_i) =
                        self.get_section(&data, i, "EndFPST", value.as_deref());
                    // println!(
                    //     "{}: captured {} lines (end marker EndFPST)",
                    //     key,
                    //     section.len()
                    // );
                    // Self::print_section(&key, &section);
                    i = next_i;
                }
                "Grid" => {
                    let (section, next_i) = self.get_section(&data, i, "EndSplineSet", None);
                    // This is a splineset, so we parse it into paths
                    let paths = Self::splines_to_path(&section, false)?;
                    // We only want the ones which are two nodes, move + line
                    for gridline in paths.iter().filter(|p| {
                        p.nodes.len() == 2
                            && matches!(p.nodes[0].nodetype, NodeType::Move)
                            && matches!(p.nodes[1].nodetype, NodeType::Line)
                    }) {
                        let start_x = gridline.nodes[0].x as f32;
                        let start_y = gridline.nodes[0].y as f32;
                        let end_x = gridline.nodes[1].x as f32;
                        let end_y = gridline.nodes[1].y as f32;
                        let angle = (end_y - start_y).atan2(end_x - start_x).to_degrees();
                        self.font.masters[0].guides.push(Guide {
                            pos: crate::Position {
                                x: start_x,
                                y: start_y,
                                angle,
                            },
                            ..Default::default()
                        })
                    }
                    i = next_i;
                }
                "Lookup" | "AnchorClass2" | "MarkAttachClasses" | "MarkAttachSets"
                | "KernPairs" => {
                    if key == "Lookup" {
                        if let Some(v) = &value {
                            self.parse_lookup(v);
                        }
                    } else if let Some(v) = &value {
                        self.font
                            .format_specific
                            .insert(key.clone(), serde_json::Value::String(v.clone()));
                    } else {
                        // These keys will receive real parsing later; for now we just log.
                        // println!("{key}: {}", value.unwrap_or_default());
                    }
                }
                "EndSplineFont" => {
                    break;
                }
                "LayerCount" => {
                    if let Some(Ok(count)) = value.as_ref().map(|v| v.parse::<usize>()) {
                        self.layer_defs = vec![None; count];
                        self.font.format_specific.insert(
                            "sfd.has_header_layers".to_string(),
                            serde_json::Value::Bool(true),
                        );
                    }
                }
                "Layer" => {
                    self.font.format_specific.insert(
                        "sfd.has_header_layers".to_string(),
                        serde_json::Value::Bool(true),
                    );
                    if let Some(v) = &value {
                        self.parse_layer_def(v);
                    }
                }
                // Name table entries
                "FontName" => {
                    if let Some(v) = &value {
                        self.font.names.postscript_name = v.into();
                    }
                }
                "FullName" => {
                    if let Some(v) = &value {
                        self.font.names.full_name = v.into();
                    }
                }
                "FamilyName" => {
                    if let Some(v) = &value {
                        self.font.names.family_name = v.into();
                    }
                }
                "Weight" => {
                    if let Some(v) = &value {
                        // Postscript weight name, what even is that?
                        self.font.format_specific.insert(
                            "postscript_weight_name".to_string(),
                            serde_json::Value::String(v.into()),
                        );
                    }
                }
                "Copyright" => {
                    if let Some(v) = &value {
                        // XXX Decode escaped sequences?
                        self.font.names.copyright = v.into();
                    }
                }
                "Version" => {
                    if let Some(v) = &value {
                        self.font.names.version = v.into();
                        // Try to parse the major/minor version from the string.
                        // Find a float at the start of the string.
                        if let Some(first_word) = v.split_whitespace().next() {
                            if let Ok(ver) = first_word.parse::<f32>() {
                                let major = ver.trunc() as u16;
                                let minor = ((ver - ver.trunc()) * 100.0).round() as u16;
                                self.font.version = (major, minor);
                            }
                        }
                    }
                }
                "UniqueID" => {
                    if let Some(v) = &value {
                        self.font.names.unique_id = v.into();
                    }
                }
                "LangName" => {
                    if let Some(v) = &value {
                        let entry = self
                            .font
                            .format_specific
                            .entry("sfd.lang_names".to_string())
                            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                        if let serde_json::Value::Array(arr) = entry {
                            arr.push(serde_json::Value::String(v.clone()));
                        }
                        self.parse_language_specific_name(v);
                    }
                }
                "OtfFeatName" => {
                    if let Some(v) = &value {
                        // Expected format: '<feature tag>' <language code> "<feature name>"
                        let regex = &FEATURE_NAME_REGEX;
                        if let Some(caps) = regex.captures(v) {
                            let tag = caps.name("tag").map(|m| m.as_str()).unwrap_or_default();
                            let lang_id = caps
                                .name("lang")
                                .and_then(|m| m.as_str().parse::<u32>().ok())
                                .unwrap_or(0);
                            let name = caps.name("name").map(|m| m.as_str()).unwrap_or_default();
                            self.feature_names
                                .entry(tag.into())
                                .or_default()
                                .push((lang_id, name.to_string()));
                        } else {
                            println!("Warning: invalid OTFeatureName format: {}", v);
                        }
                    }
                }
                // Metrics
                "ItalicAngle" => parse_metric!(self, value, ItalicAngle),
                "UnderlinePosition" => parse_metric!(self, value, UnderlinePosition),
                "UnderlineWidth" => parse_metric!(self, value, UnderlineThickness),
                "Ascent" => parse_metric!(self, value, Ascender),
                "Descent" => parse_metric!(self, value, Descender), // We might need to negate this?
                "LineGap" => parse_metric!(self, value, HheaLineGap),
                "HheadAscent" => parse_metric!(self, value, HheaAscender),
                "HheadDescent" => parse_metric!(self, value, HheaDescender),
                "OS2TypoLinegap" => parse_metric!(self, value, TypoLineGap),
                "OS2TypoAscent" => parse_metric!(self, value, TypoAscender),
                "OS2TypoDescent" => parse_metric!(self, value, TypoDescender),
                "OS2WinAscent" => parse_metric!(self, value, WinAscent),
                "OS2WinDescent" => parse_metric!(self, value, WinDescent),
                "OS2SubXSize" => parse_metric!(self, value, SubscriptXSize),
                "OS2SubYSize" => parse_metric!(self, value, SubscriptYSize),
                "OS2SubXOff" => parse_metric!(self, value, SubscriptXOffset),
                "OS2SubYOff" => parse_metric!(self, value, SubscriptYOffset),
                "OS2SupXSize" => parse_metric!(self, value, SuperscriptXSize),
                "OS2SupYSize" => parse_metric!(self, value, SuperscriptYSize),
                "OS2SupXOff" => parse_metric!(self, value, SuperscriptXOffset),
                "OS2SupYOff" => parse_metric!(self, value, SuperscriptYOffset),
                "OS2StrikeYSize" => parse_metric!(self, value, StrikeoutSize),
                "OS2StrikeYPos" => parse_metric!(self, value, StrikeoutPosition),
                "OS2CapHeight" => parse_metric!(self, value, CapHeight),
                "OS2XHeight" => parse_metric!(self, value, XHeight),
                // Other font-level OT values
                "FSType" => {
                    if let Some(Ok(v)) = &value.map(|v| v.parse::<u16>()) {
                        self.font.custom_ot_values.os2_fs_type = Some(*v);
                        self.font
                            .format_specific
                            .insert("sfd.has_fstype".to_string(), serde_json::Value::Bool(true));
                    }
                }
                "TTFWeight" | "PfmWeight" => {
                    if let Some(Ok(v)) = &value.map(|v| v.parse::<u16>()) {
                        self.font.custom_ot_values.os2_us_weight_class = Some(*v);
                    }
                }
                "TTFWidth" => {
                    if let Some(Ok(v)) = &value.map(|v| v.parse::<u16>()) {
                        self.font.custom_ot_values.os2_us_width_class = Some(*v);
                    }
                }
                "Panose" => {
                    if let Some(v) = &value {
                        let parts: Result<Vec<u8>, _> =
                            v.split_whitespace().map(|n| n.parse::<u8>()).collect();
                        if let Ok(pano) = parts {
                            #[allow(clippy::unwrap_used)] // Safe because we checked length
                            if pano.len() == 10 {
                                self.font.custom_ot_values.os2_panose =
                                    Some(pano.try_into().unwrap());
                            }
                        }
                    }
                }
                "OSVendor" => {
                    if let Some(v) = &value
                        .as_ref()
                        .map(|s| s.trim_matches('\''))
                        .and_then(|s| tag_from_string(s).ok())
                    {
                        self.font.custom_ot_values.os2_vendor_id = Some(*v);
                    }
                }
                "OS2FamilyClass" => {
                    if let Some(Ok(v)) = &value.map(|v| v.parse::<u16>()) {
                        self.font.custom_ot_values.os2_family_class = Some(*v);
                    }
                }
                "OS2_UseTypoMetrics" => {
                    if let Some(v) = &value {
                        self.font.format_specific.insert(
                            "OS2_UseTypoMetrics".to_string(),
                            serde_json::Value::String(v.clone()),
                        );
                    }
                    let current_fstype = self.font.custom_ot_values.os2_fs_type.unwrap_or(0);
                    let enabled = value
                        .as_deref()
                        .and_then(|v| v.parse::<u16>().ok())
                        .unwrap_or(1)
                        != 0;
                    self.font.custom_ot_values.os2_fs_type = Some(if enabled {
                        current_fstype | 1 << 7
                    } else {
                        current_fstype & !(1 << 7)
                    });
                }
                "OS2_WeightWidthSlopeOnly" => {
                    if let Some(v) = &value {
                        self.font.format_specific.insert(
                            "OS2_WeightWidthSlopeOnly".to_string(),
                            serde_json::Value::String(v.clone()),
                        );
                    }
                    let current_fstype = self.font.custom_ot_values.os2_fs_type.unwrap_or(0);
                    let enabled = value
                        .as_deref()
                        .and_then(|v| v.parse::<u16>().ok())
                        .unwrap_or(1)
                        != 0;
                    self.font.custom_ot_values.os2_fs_type = Some(if enabled {
                        current_fstype | 1 << 8
                    } else {
                        current_fstype & !(1 << 8)
                    });
                }
                "OS2CodePages" => {
                    // These are stored as period-separated hex strings
                    if let Some(v) = &value {
                        let parts: Vec<&str> = v.split('.').collect();
                        if parts.len() == 2 {
                            if let (Ok(part1), Ok(part2)) = (
                                u32::from_str_radix(parts[0], 16),
                                u32::from_str_radix(parts[1], 16),
                            ) {
                                self.font.custom_ot_values.os2_unicode_range1 = Some(part1);
                                self.font.custom_ot_values.os2_unicode_range2 = Some(part2);
                            }
                        }
                    }
                }
                "OS2UnicodeRanges" => {
                    if let Some(v) = &value {
                        let parts: Vec<&str> = v.split('.').collect();
                        if parts.len() == 4 {
                            if let (Ok(part1), Ok(part2), Ok(part3), Ok(part4)) = (
                                u32::from_str_radix(parts[0], 16),
                                u32::from_str_radix(parts[1], 16),
                                u32::from_str_radix(parts[2], 16),
                                u32::from_str_radix(parts[3], 16),
                            ) {
                                self.font.custom_ot_values.os2_unicode_range1 = Some(part1);
                                self.font.custom_ot_values.os2_unicode_range2 = Some(part2);
                                self.font.custom_ot_values.os2_unicode_range3 = Some(part3);
                                self.font.custom_ot_values.os2_unicode_range4 = Some(part4);
                            }
                        }
                    }
                }
                "OS2Vendor" => {
                    if let Some(v) = &value
                        .as_ref()
                        .map(|s| s.trim_matches('\''))
                        .and_then(|s| tag_from_string(s).ok())
                    {
                        self.font.custom_ot_values.os2_vendor_id = Some(*v);
                    }
                }

                // Things which are important, but we just store in FormatSpecific for now
                "MATH" | "VLineGap" | "OS2TypoAOffset" | "OS2TypoDOffset" | "OS2WinAOffset"
                | "OS2WinDOffset" | "HheadAOffset" | "HheadDOffset" | "GaspTable" => {
                    if let Some(v) = &value {
                        self.font
                            .format_specific
                            .insert(key.clone(), serde_json::Value::String(v.clone()));
                    }
                }
                // Fontforge GUI things we don't care about; just store them in
                // formatspecific
                "DisplayLayer" | "DisplaySize" | "AntiAlias" | "FitToEm" | "WinInfo"
                | "Encoding" | "sfntRevision" | "WidthSeparation" | "ModificationTime"
                | "PfmFamily" | "OS2Version" | "XUID" | "UnicodeInterp" | "NameList" | "DEI"
                | "NeedsXUIDChange" | "TeXData" | "InvalidEm" | "woffMajor" | "woffMinor" => {
                    if let Some(v) = &value {
                        self.font
                            .format_specific
                            .insert(key.clone(), serde_json::Value::String(v.clone()));
                    }
                }
                // Anything else
                "CreationTime" => {
                    // Capture creation time as a timestamp
                    if let Some(v) = &value {
                        self.font
                            .format_specific
                            .insert(key.clone(), serde_json::Value::String(v.clone()));
                        if let Some(ts) = DateTime::<chrono::Utc>::from_timestamp_secs(
                            v.parse::<i64>().map_err(|_| {
                                BabelfontError::General(
                                    "Invalid CreationTime timestamp".to_string(),
                                )
                            })?,
                        ) {
                            self.font.date = ts;
                        }
                    }
                }
                "Comments" | "FontLog" => {
                    if let Some(v) = &value {
                        self.push_comment_entry(&key, raw_value.as_deref().unwrap_or(""));
                        if let Some(mut note) = self.font.note.take() {
                            note.push_str(v);
                            note.push('\n');
                            self.font.note = Some(note);
                        } else {
                            self.font.note = Some(format!("{}\n", v));
                        }
                    }
                }
                "UComments" => {
                    if let Some(v) = &value {
                        self.push_comment_entry(&key, raw_value.as_deref().unwrap_or(""));
                        let v = v.trim_matches('"');
                        if let Some(mut note) = self.font.note.take() {
                            note.push_str(v);
                            note.push('\n');
                            self.font.note = Some(note);
                        } else {
                            self.font.note = Some(format!("{}\n", v));
                        }
                    }
                }
                "GlyphOrder" | "Compacted" => {
                    // Ignore for now
                }
                _ => {
                    // Default case: log any other key/value pair.
                    match &value {
                        Some(v) => println!("{key}: {v}"),
                        None => println!("{key}"),
                    }
                }
            }
        }

        // Now parse glyphs if present
        if let Some(chars) = char_data {
            self.parse_chars(&chars, &master_id)?;
        }

        Ok(())
    }

    fn parse_layer_def(&mut self, value: &str) {
        // Expected format: "<idx> <quadratic> \"Name\" <flags>"; we ignore flags
        let tokenized = Self::tokenize_preserving_quotes(value);
        let parts: Vec<&str> = tokenized.iter().map(String::as_str).collect();
        if parts.len() < 3 {
            return;
        }
        let idx = parts[0].parse::<usize>().ok();
        let quadratic = parts[1] == "1";
        let name = parts[2].trim_matches('"').to_string();
        let flags = parts
            .last()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        if let Some(i) = idx {
            if self.layer_defs.len() <= i {
                self.layer_defs.resize(i + 1, None);
            }
            self.layer_defs[i] = Some(LayerDefinition {
                name: Some(name),
                is_quadratic: quadratic,
                flags,
            });
            let serialized_defs: Vec<serde_json::Value> = self
                .layer_defs
                .iter()
                .enumerate()
                .filter_map(|(index, def)| {
                    let def = def.as_ref()?;
                    let mut obj = serde_json::Map::new();
                    obj.insert(
                        "index".to_string(),
                        serde_json::Value::Number((index as u64).into()),
                    );
                    obj.insert(
                        "name".to_string(),
                        serde_json::Value::String(def.name.clone().unwrap_or_default()),
                    );
                    obj.insert(
                        "is_quadratic".to_string(),
                        serde_json::Value::Bool(def.is_quadratic),
                    );
                    obj.insert(
                        "flags".to_string(),
                        serde_json::Value::Number((def.flags as u64).into()),
                    );
                    Some(serde_json::Value::Object(obj))
                })
                .collect();
            self.font.format_specific.insert(
                "sfd.layer_defs".to_string(),
                serde_json::Value::Array(serialized_defs),
            );
        }
    }

    fn push_comment_entry(&mut self, key: &str, raw_value: &str) {
        let entry = self
            .font
            .format_specific
            .entry(COMMENT_ENTRIES_KEY.to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if let serde_json::Value::Array(entries) = entry {
            let mut object = serde_json::Map::new();
            object.insert(
                "key".to_string(),
                serde_json::Value::String(key.to_string()),
            );
            object.insert(
                "raw".to_string(),
                serde_json::Value::String(raw_value.to_string()),
            );
            entries.push(serde_json::Value::Object(object));
        }
    }

    fn parse_chars(&mut self, data: &[String], master_id: &str) -> Result<(), BabelfontError> {
        let mut i = 0usize;
        while i < data.len() {
            let line = &data[i];
            i += 1;
            if line.starts_with("StartChar") {
                let (section, next_i) = self.get_section(data, i, "EndChar", Some(line));
                i = next_i;
                let glyph = self.parse_char(&section, master_id)?;
                self.font.glyphs.push(glyph);
            }
        }
        Ok(())
    }

    fn parse_char(&mut self, data: &[String], master_id: &str) -> Result<Glyph, BabelfontError> {
        if data.is_empty() {
            return Err(BabelfontError::General(
                "Empty glyph block while parsing SFD".to_string(),
            ));
        }

        let name_line = data[0]
            .split_once(": ")
            .map(|(_, v)| v.to_string())
            .unwrap_or_else(|| data[0].clone());
        let glyph_name = name_line.trim_matches('"');
        let mut glyph = Glyph::new(glyph_name);
        glyph.exported = true; // All FontForge glyphs are exported by default

        let mut codepoints: Vec<u32> = Vec::new();
        let mut current_layer_idx: Option<usize> = None;
        let mut layer_map: HashMap<usize, usize> = HashMap::new();
        let mut width: Option<f32> = None;

        let mut idx = 1usize;
        while idx < data.len() {
            let line = &data[idx];
            idx += 1;

            let (key, value) = if let Some(pos) = line.find(':') {
                let (k, v) = line.split_at(pos);
                (k.trim(), Some(v[1..].trim()))
            } else {
                (line.trim(), None)
            };

            // Some SFDs omit the literal "SplineSet" marker and place spline
            // segment lines directly after Fore/Back/Layer, terminated by EndSplineSet.
            if current_layer_idx.is_some() && Self::looks_like_spline_line(key) {
                let mut section = vec![line.to_string()];
                while idx < data.len() {
                    let next_line = &data[idx];
                    idx += 1;
                    if next_line.trim() == "EndSplineSet" {
                        break;
                    }
                    section.push(next_line.to_string());
                }
                if let Some(layer_idx) = current_layer_idx {
                    if let Some(layer_pos) = layer_map.get(&layer_idx) {
                        let layer = &mut glyph.layers[*layer_pos];
                        let paths = Self::splines_to_path(&section, layer_is_quadratic(layer))?;
                        layer.format_specific.insert(
                            "sfd.explicit_splineset".to_string(),
                            serde_json::Value::Bool(false),
                        );
                        layer
                            .shapes
                            .extend(paths.into_iter().map(Shape::Path).collect::<Vec<Shape>>());
                    }
                }
                continue;
            }

            match key {
                "Width" => {
                    if let Some(v) = value.and_then(|v| v.parse::<f32>().ok()) {
                        width = Some(v);
                    }
                }
                "VWidth" => {
                    // Not represented in babelfont; stash as format-specific
                    if let Some(v) = value {
                        glyph.format_specific.insert(
                            "vwidth".to_string(),
                            serde_json::Value::String(v.to_string()),
                        );
                    }
                }
                "Flags" => {
                    if let Some(v) = value {
                        glyph.format_specific.insert(
                            "sfd.flags".to_string(),
                            serde_json::Value::String(v.to_string()),
                        );
                        glyph.format_specific.insert(
                            "sfd.changed_since_last_hinted".to_string(),
                            serde_json::Value::Bool(v.contains('H')),
                        );
                        glyph.format_specific.insert(
                            "sfd.manual_hints".to_string(),
                            serde_json::Value::Bool(v.contains('M')),
                        );
                        glyph.format_specific.insert(
                            "sfd.width_set".to_string(),
                            serde_json::Value::Bool(v.contains('W')),
                        );
                        glyph.format_specific.insert(
                            "sfd.editor_state_saved".to_string(),
                            serde_json::Value::Bool(v.contains('O')),
                        );
                        glyph.format_specific.insert(
                            "sfd.instructions_out_of_date".to_string(),
                            serde_json::Value::Bool(v.contains('I')),
                        );
                    }
                }
                "Encoding" => {
                    if let Some(v) = value {
                        let parts: Vec<&str> = v.split_whitespace().collect();
                        if let Some(slot) = parts.first().and_then(|p| p.parse::<i64>().ok()) {
                            glyph.format_specific.insert(
                                "sfd.encoding_slot".to_string(),
                                serde_json::Value::Number(slot.into()),
                            );
                        }
                        glyph.format_specific.insert(
                            "sfd.encoding_has_gid".to_string(),
                            serde_json::Value::Bool(parts.len() >= 3),
                        );
                        if parts.len() >= 2 {
                            if let Ok(cp) = parts[1].parse::<i32>() {
                                glyph.format_specific.insert(
                                    "sfd.encoding_unicode".to_string(),
                                    serde_json::Value::Number((cp as i64).into()),
                                );
                                if cp >= 0 {
                                    codepoints.push(cp as u32);
                                }
                            }
                        }
                        if let Some(orig_gid) = parts.get(2).and_then(|p| p.parse::<i64>().ok()) {
                            glyph.format_specific.insert(
                                "sfd.encoding_gid".to_string(),
                                serde_json::Value::Number(orig_gid.into()),
                            );
                        }
                    }
                }
                "GlyphClass" => {
                    if let Some(v) = value.and_then(|v| v.parse::<usize>().ok()) {
                        glyph.category = match v {
                            2 => GlyphCategory::Base,
                            3 => GlyphCategory::Ligature,
                            4 => GlyphCategory::Mark,
                            _ => GlyphCategory::Unknown,
                        };
                    }
                }
                "Back" | "Fore" | "Layer" => {
                    // Determine layer index: value if present, otherwise position in list
                    let idx_val = if let Some(v) = value.and_then(|v| v.parse::<usize>().ok()) {
                        v
                    } else {
                        match key {
                            "Back" => 0,
                            "Fore" => 1,
                            _ => 2,
                        }
                    };
                    current_layer_idx = Some(idx_val);
                    Self::ensure_layer(
                        &mut glyph,
                        &mut layer_map,
                        idx_val,
                        width.unwrap_or(0.0),
                        self.layer_defs.get(idx_val).and_then(|d| d.as_ref()),
                        master_id,
                    );
                }
                "SplineSet" => {
                    let (section, next_idx) = self.get_section(data, idx, "EndSplineSet", None);
                    idx = next_idx;
                    if let Some(layer_idx) = current_layer_idx {
                        if let Some(layer_pos) = layer_map.get(&layer_idx) {
                            let layer = &mut glyph.layers[*layer_pos];
                            let paths = Self::splines_to_path(&section, layer_is_quadratic(layer))?;
                            layer.format_specific.insert(
                                "sfd.explicit_splineset".to_string(),
                                serde_json::Value::Bool(true),
                            );
                            layer
                                .shapes
                                .extend(paths.into_iter().map(Shape::Path).collect::<Vec<Shape>>());
                        }
                    }
                }
                "Image" | "Image2" => {
                    // Skip for now; still advance to end marker
                    let end = if key == "Image" {
                        "EndImage"
                    } else {
                        "EndImage2"
                    };
                    let (_section, next_idx) = self.get_section(data, idx, end, value);
                    idx = next_idx;
                }
                "Refer" => {
                    // Components referencing other glyphs by index; store raw for later resolution
                    if let Some(layer_idx) = current_layer_idx {
                        if let Some(layer_pos) = layer_map.get(&layer_idx) {
                            let entry = glyph.layers[*layer_pos]
                                .format_specific
                                .entry("sfd.refer".to_string())
                                .or_insert_with(|| serde_json::Value::Array(vec![]));
                            if let serde_json::Value::Array(arr) = entry {
                                arr.push(serde_json::Value::String(
                                    value.unwrap_or("").to_string(),
                                ));
                            }
                        }
                    }
                }
                "Kerns2" => {
                    if let Some(v) = value {
                        self.parse_kerns(glyph_name, v);
                    }
                }
                "HStem" | "VStem" => {
                    if let Some(v) = value {
                        let layer_pos = Self::ensure_default_foreground_layer(
                            &mut glyph,
                            &mut layer_map,
                            width.unwrap_or(0.0),
                            self.layer_defs.get(1).and_then(|d| d.as_ref()),
                            master_id,
                        );
                        let layer = &mut glyph.layers[layer_pos];
                        layer.format_specific.insert(
                            if key == "HStem" {
                                HSTEM_KEY.to_string()
                            } else {
                                VSTEM_KEY.to_string()
                            },
                            serde_json::Value::String(v.to_string()),
                        );
                    }
                }
                "LCarets2" => {
                    if let Some(v) = value {
                        glyph.format_specific.insert(
                            "sfd.lcarets".to_string(),
                            serde_json::Value::String(v.into()),
                        );
                    }
                }
                "AltUni2" => {
                    if let Some(v) = value {
                        // Fix these up later - these are period-separated hex sequences
                        // but I'm not totally sure how they relate to Unicode codepoints
                        glyph.format_specific.insert(
                            "sfd.altuni".to_string(),
                            serde_json::Value::String(v.into()),
                        );
                    }
                }
                "UnlinkRmOvrlpSave" => {
                    glyph.format_specific.insert(
                        "sfd.decompose_remove_overlap".to_string(),
                        serde_json::Value::Bool(true),
                    );
                }
                "Comment" => {
                    if let Some(v) = value {
                        glyph.format_specific.insert(
                            "sfd.comment".to_string(),
                            serde_json::Value::String(v.to_string()),
                        );
                    }
                }
                "AnchorPoint" => {
                    if let Some(v) = value {
                        if let Some(layer_idx) = current_layer_idx {
                            if let Some(layer_pos) = layer_map.get(&layer_idx) {
                                let layer = &mut glyph.layers[*layer_pos];
                                layer.anchors.push(self.parse_anchor(v).ok_or(
                                    BabelfontError::General("Couldn't parse anchor".to_string()),
                                )?);
                            }
                        }
                    }
                }
                "LayerCount" => {
                    glyph.format_specific.insert(
                        "sfd.has_layer_count".to_string(),
                        serde_json::Value::Bool(true),
                    );
                }
                "Colour" => {
                    if let Some(v) = value {
                        if let Some(layer_idx) = current_layer_idx {
                            if let Some(layer_pos) = layer_map.get(&layer_idx) {
                                let layer = &mut glyph.layers[*layer_pos];
                                // Hex-encoded RGB. But not all components may be present. Pad start with 0s
                                let v = format!("{:0>6}", v);
                                let r = u8::from_str_radix(&v[0..2], 16).unwrap_or(0);
                                let g = u8::from_str_radix(&v[2..4], 16).unwrap_or(0);
                                let b = u8::from_str_radix(&v[4..6], 16).unwrap_or(0);

                                layer.color = Some(Color {
                                    r: r as i32,
                                    g: g as i32,
                                    b: b as i32,
                                    a: 25,
                                })
                            }
                        }
                    }
                }
                // One-line layout rules
                "Ligature2" => {
                    // Split off the (quoted) name and the rest
                    if let Some((subtable_name, glyphs)) = self.parse_oneline_layout(value) {
                        let Some(subtable) = self.gsub_lookups.find_subtable_mut(&subtable_name)
                        else {
                            log::error!("Ligature2 references unknown subtable: {}", subtable_name);
                            continue;
                        };
                        subtable.push(fea_rs_ast::Statement::LigatureSubst(
                            layout::make_ligature_statement(&glyphs, &SmolStr::from(glyph_name)),
                        ));
                    }
                }
                _ => {
                    log::debug!("Unhandled FontForge glyph key: {}", key);
                }
            }
        }

        // If we got to the end and there were no layers, add one.
        if glyph.layers.is_empty() {
            Self::ensure_layer(
                &mut glyph,
                &mut layer_map,
                1,
                width.unwrap_or(0.0),
                self.layer_defs.get(1).and_then(|d| d.as_ref()),
                master_id,
            );
        }

        glyph.codepoints = codepoints;
        Ok(glyph)
    }

    fn ensure_layer(
        glyph: &mut Glyph,
        layer_map: &mut std::collections::HashMap<usize, usize>,
        layer_idx: usize,
        width: f32,
        def: Option<&LayerDefinition>,
        master_id: &str,
    ) {
        if layer_map.contains_key(&layer_idx) {
            return;
        }
        let mut layer = Layer::new(width);
        layer.id = Some(master_id.to_string());
        layer.name = def.and_then(|d| d.name.clone());
        layer.format_specific.insert(
            LAYER_QUADRATIC_KEY.to_string(),
            serde_json::Value::Bool(def.map(|d| d.is_quadratic).unwrap_or(false)),
        );

        // In SFD, index 1 / "Fore" is the primary drawable layer.
        // Non-foreground layers should not be treated as default master layers,
        // otherwise interpolation may combine incompatible structures.
        let is_foreground = layer_idx == 1
            || layer
                .name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("Fore"))
                .unwrap_or(false);

        layer.master = if is_foreground {
            LayerType::DefaultForMaster(master_id.to_string())
        } else {
            LayerType::AssociatedWithMaster(master_id.to_string())
        };

        if layer_idx == 0
            || layer
                .name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("Back"))
                .unwrap_or(false)
        {
            layer.is_background = true;
        }
        let pos = glyph.layers.len();
        glyph.layers.push(layer);
        layer_map.insert(layer_idx, pos);
    }

    fn ensure_default_foreground_layer(
        glyph: &mut Glyph,
        layer_map: &mut std::collections::HashMap<usize, usize>,
        width: f32,
        def: Option<&LayerDefinition>,
        master_id: &str,
    ) -> usize {
        Self::ensure_layer(glyph, layer_map, 1, width, def, master_id);
        layer_map.get(&1).copied().unwrap_or(0)
    }

    fn parse_language_specific_name(&mut self, v: &str) {
        // Format: <language_id> "string0" "string1" "string2" ...
        // Strings are UTF-7 encoded, indices correspond to OpenType Name IDs
        let tokens = Self::tokenize_preserving_quotes(v);
        if tokens.is_empty() {
            return;
        }

        // First token is the language ID
        let lang_id = match tokens[0].parse::<u16>() {
            Ok(id) => id,
            Err(_) => return,
        };

        // Convert OpenType language ID to ISO tag
        let Some(iso_tag) = ot_lang_id_to_iso_tag(lang_id) else {
            log::warn!("Unknown OpenType language ID: {}", lang_id);
            return;
        };

        // Process each quoted string
        for (ix, token) in tokens.iter().skip(1).enumerate() {
            if !token.starts_with('"') || !token.ends_with('"') {
                continue;
            }

            // Strip quotes and decode from UTF-7
            let utf7_string = token.trim_matches('"');
            let decoded = decode_utf7(utf7_string);

            // Skip empty strings
            if decoded.is_empty() {
                continue;
            }

            // Get the appropriate name field by OpenType Name ID (index)
            if let Some(name_dict) = self.font.names.get_mut(NameId::new(ix as u16)) {
                name_dict.insert(iso_tag.to_string(), decoded);
            }
        }
    }

    fn parse_lookup(&mut self, data: &str) {
        // Format per fontforge.md:
        // Lookup: <kind> <flags> <save-afm> "<lookup name>" { ...subtables... } [ ...features/scripts/languages... ]
        let head_end = data.find('"').unwrap_or(data.len());
        let head = data[..head_end].trim();
        let mut it = head.split_whitespace();
        let kind: u16 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let flag: u16 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        // let _save_afm: u16 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);

        // Lookup name between quotes
        let name = if let Some(start) = data.find('"') {
            if let Some(end) = data[start + 1..].find('"') {
                &data[start + 1..start + 1 + end]
            } else {
                ""
            }
        } else {
            ""
        };

        let subtables_vec = Self::parse_subtable_names(data);
        let mut subtables: IndexMap<SmolStr, Vec<fea_rs_ast::Statement>> = IndexMap::new();
        for sub in subtables_vec {
            subtables.entry(sub).or_default();
        }

        // Features part inside [...] (may contain multiple scripts/languages for one or more features)
        let features_part = if let Some(lb) = data.rfind('[') {
            if let Some(rb) = data.rfind(']') {
                if rb > lb {
                    Some(&data[lb + 1..rb])
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let features = features_part
            .map(Self::parse_lookup_features)
            .unwrap_or_default();

        let lookup_type = Self::lookup_type_from_kind(kind);
        let sanitized_name =
            Self::sanitize_and_dedupe_lookup_name(name, &mut self.sanitized_lookup_names);
        let info = layout::LookupInfo {
            lookup_type,
            flag,
            features,
            block: fea_rs_ast::LookupBlock::new(sanitized_name.clone().into(), vec![], false, 0..0),
            subtables,
        };

        // Determine GSUB vs GPOS from high byte of kind
        if (kind) >> 8 == 1 {
            self.gpos_lookups.0.insert(sanitized_name, info);
        } else {
            self.gsub_lookups.0.insert(sanitized_name, info);
        }
    }

    fn parse_subtable_names(data: &str) -> Vec<SmolStr> {
        // Capture content between the first '{' and the matching '}' (use last '}' if simple)
        let (start, end) = match (data.find('{'), data.rfind('}')) {
            (Some(s), Some(e)) if e > s => (s, e),
            _ => return Vec::new(),
        };
        let body = &data[start + 1..end];
        let tokens = Self::tokenize_preserving_quotes(body);
        tokens
            .into_iter()
            .filter(|t| t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
            .map(|t| SmolStr::from(t.trim_matches('"')))
            .collect()
    }

    fn lookup_type_from_kind(kind: u16) -> layout::LookupType {
        use layout::LookupType as LT;
        match kind {
            1 => LT::SingleSubstitution,
            2 => LT::MultipleSubstitution,
            3 => LT::AlternateSubstitution,
            4 => LT::LigatureSubstitution,
            5 => LT::GsubContext,
            6 => LT::GsubChainContext,
            8 => LT::ReverseChain,
            0x101 => LT::SinglePosition,
            0x102 => LT::PairPosition,
            0x103 => LT::CursivePosition,
            0x104 => LT::MarkToBasePosition,
            0x105 => LT::MarkToLigaturePosition,
            0x106 => LT::MarkToMarkPosition,
            0x107 => LT::ContextPosition,
            0x108 => LT::ChainContextPosition,
            _ => LT::SingleSubstitution,
        }
    }

    fn parse_lookup_features(s: &str) -> Vec<layout::FeatureLangSys> {
        // Expect patterns like: 'kern' ('DFLT' <'dflt' > 'latn' <'dflt' > )
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find('\'') {
            let after = &rest[start + 1..];
            if let Some(end_rel) = after.find('\'') {
                let feature = &after[..end_rel];
                // Find the following parenthesis block
                let after_feat = &after[end_rel + 1..];
                if let Some(p_start) = after_feat.find('(') {
                    if let Some(p_end) = after_feat[p_start + 1..].find(')') {
                        let body = &after_feat[p_start + 1..p_start + 1 + p_end];
                        // Body contains one or more: 'script' < 'lang' 'lang2' >
                        let mut b = body;
                        loop {
                            if let Some(s_start) = b.find('\'') {
                                let s_after = &b[s_start + 1..];
                                if let Some(s_end_rel) = s_after.find('\'') {
                                    let script = &s_after[..s_end_rel];
                                    // find angle bracket block
                                    let s_tail = &s_after[s_end_rel + 1..];
                                    if let Some(a_start) = s_tail.find('<') {
                                        if let Some(a_end) = s_tail[a_start + 1..].find('>') {
                                            let langs_blob =
                                                &s_tail[a_start + 1..a_start + 1 + a_end];
                                            // languages are quoted tokens
                                            let mut lb = langs_blob;
                                            loop {
                                                if let Some(l_start) = lb.find('\'') {
                                                    let l_after = &lb[l_start + 1..];
                                                    if let Some(l_end_rel) = l_after.find('\'') {
                                                        let language = &l_after[..l_end_rel];
                                                        out.push(layout::FeatureLangSys {
                                                            feature: SmolStr::from(feature),
                                                            script: SmolStr::from(script),
                                                            language: SmolStr::from(language),
                                                        });
                                                        lb = &l_after[l_end_rel + 1..];
                                                        continue;
                                                    }
                                                }
                                                break;
                                            }
                                            b = &s_tail[a_start + 1 + a_end + 1..];
                                            continue;
                                        }
                                    }
                                    b = s_tail;
                                    continue;
                                }
                            }
                            break;
                        }
                        // Advance rest beyond this feature block
                        rest = &after_feat[p_start + 1 + p_end + 1..];
                        continue;
                    }
                }
                // No parenthesis found; advance and continue
                rest = after;
                continue;
            } else {
                break;
            }
        }
        out
    }

    /// Parse a KernClass2 block following the value line.
    /// The value line contains: n1 [+] n2 "subtable name"
    /// We then consume:
    /// - (n1 - classstart) lines for first-side groups
    /// - (n2 - 1) lines for second-side groups (with an implicit None at index 0)
    /// - 1 line of device table values
    fn parse_kern_class(&mut self, data: &[String], mut i: usize, value: &str) -> usize {
        let (n1, classstart, n2, name) = Self::parse_kernclass_value(value);

        // First-side groups
        let mut groups1: Vec<Vec<String>> = Vec::new();
        let count1 = n1.saturating_sub(classstart);
        for line in &data[i..i + count1] {
            let toks: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();
            // Skip the first token (class id or flag)
            let grp = toks.into_iter().skip(1).collect();
            groups1.push(grp);
        }
        i += count1;

        // Second-side groups
        let mut groups2: Vec<Vec<String>> = Vec::new();
        // Insert placeholder for the implicit None at index 0
        groups2.push(Vec::new());
        let count2 = n2.saturating_sub(1);
        for line in &data[i..i + count2] {
            let toks: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();
            let grp = toks.into_iter().skip(1).collect();
            groups2.push(grp);
        }
        i += count2;

        // Device table line
        let kerns_line = data.get(i).cloned().unwrap_or_default();
        let kerns = Self::parse_devicetable(&kerns_line);
        i += 1;

        self.kern_classes.insert(
            name,
            KernClass {
                groups1,
                groups2,
                kerns,
            },
        );

        i
    }

    fn parse_kerns(&mut self, left_glyph: &str, data: &str) {
        let triples = Self::parse_kerns_line(data);
        for (gid2, kern, subtable) in triples {
            let entry = self
                .kern_pairs
                .entry(subtable)
                .or_default()
                .entry(left_glyph.to_string())
                .or_default();
            entry.push((gid2 as usize, kern as i16));
        }
    }

    fn parse_kernclass_value(value: &str) -> (usize, usize, usize, String) {
        // Regex-like parsing: <n1><+?><space><n2><space>"name"
        let mut n1 = 0usize;
        let mut n2 = 0usize;
        let mut classstart = 1usize;
        let mut name = String::new();

        // Find quoted name
        if let Some(start) = value.find('"') {
            if let Some(end) = value.rfind('"') {
                if end > start {
                    name = value[start + 1..end].to_string();
                }
            }
        }
        // Parse leading numbers and optional plus
        let head = value.split('"').next().unwrap_or("").trim();
        let mut it = head.split_whitespace();
        if let Some(a) = it.next() {
            n1 = a.parse().unwrap_or(0);
        }
        if let Some(b) = it.next() {
            if b.contains('+') {
                classstart = 0;
                n2 = b.trim_matches('+').parse().unwrap_or(0);
            } else {
                n2 = b.parse().unwrap_or(0);
            }
        }
        (n1, classstart, n2, name)
    }

    fn parse_devicetable(value: &str) -> Vec<i16> {
        // Remove braces and split on whitespace, parse integers
        let cleaned: String = value
            .chars()
            .map(|c| if c == '{' || c == '}' { ' ' } else { c })
            .collect();
        cleaned
            .split_whitespace()
            .filter_map(|t| t.parse::<i32>().ok())
            .map(|v| v as i16)
            .collect()
    }

    fn parse_kerns_line(value: &str) -> Vec<(i32, f32, String)> {
        let tokens = Self::tokenize_preserving_quotes(value);
        let mut out = Vec::new();
        let mut i = 0usize;
        while i + 2 < tokens.len() {
            let gid = match tokens[i].parse::<i32>() {
                Ok(v) => v,
                Err(_) => break,
            };
            let kern = match tokens[i + 1].parse::<f32>() {
                Ok(v) => v,
                Err(_) => break,
            };
            let sub = tokens[i + 2].trim().trim_matches('"').to_string();
            out.push((gid, kern, sub));
            i += 3;
        }
        out
    }

    fn tokenize_preserving_quotes(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_quotes = false;
        for ch in s.chars() {
            match ch {
                '"' => {
                    in_quotes = !in_quotes;
                    cur.push(ch);
                }
                c if c.is_whitespace() && !in_quotes => {
                    if !cur.is_empty() {
                        out.push(cur.clone());
                        cur.clear();
                    }
                }
                _ => cur.push(ch),
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }

    fn glyph_from_token(token: &str, glyph_order: &[String]) -> Option<SmolStr> {
        let trimmed = token.trim_matches('"');
        if let Ok(idx) = trimmed.parse::<usize>() {
            glyph_order
                .get(idx)
                .map(|name| SmolStr::from(name.as_str()))
        } else {
            Some(SmolStr::from(trimmed))
        }
    }

    fn first_member_name(members: &[String], glyph_order: &[String]) -> Option<SmolStr> {
        members
            .first()
            .and_then(|t| Self::glyph_from_token(t, glyph_order))
    }

    fn sanitize_and_dedupe_lookup_name(name: &str, seen: &mut HashMap<String, usize>) -> String {
        // Replace non-word characters with underscores
        let sanitized = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();

        // Ensure uniqueness by appending _N if needed
        let entry = seen.entry(sanitized.clone()).or_default();
        if *entry == 0 {
            *entry = 1;
            sanitized
        } else {
            *entry += 1;
            format!("{}_{}", sanitized, entry)
        }
    }

    fn make_unique_group_name(base: SmolStr, seen: &mut HashMap<SmolStr, usize>) -> SmolStr {
        let entry = seen.entry(base.clone()).or_default();
        if *entry == 0 {
            *entry = 1;
            base
        } else {
            *entry += 1;
            SmolStr::from(format!("{base}.{entry}"))
        }
    }

    fn push_group_member(
        groups: &mut IndexMap<SmolStr, Vec<SmolStr>>,
        group: &SmolStr,
        glyph: SmolStr,
    ) {
        let entry = groups.entry(group.clone()).or_default();
        if !entry.contains(&glyph) {
            entry.push(glyph);
        }
    }

    /// Map parsed FontForge kerning classes and pairs into the Babelfont kerning model.
    ///
    /// Strategy:
    /// - Assign each glyph a primary left and right group (first group encountered wins).
    /// - Populate `first_kern_groups` and `second_kern_groups` using only these primary groups.
    /// - When a class kerning pair references a group that is *not* primary for all members,
    ///   we flatten that side into explicit glyph pairs so kerning remains editable.
    /// - Finally, apply explicit pair kerning (Kerns2) as glyph-glyph pairs.
    fn process_kerning(&mut self) -> Result<(), BabelfontError> {
        if self.font.masters.is_empty() {
            return Ok(());
        }

        let glyph_order: Vec<String> = self
            .font
            .glyphs
            .iter()
            .map(|g| g.name.to_string())
            .collect();

        let mut left_primary: HashMap<SmolStr, SmolStr> = HashMap::new();
        let mut right_primary: HashMap<SmolStr, SmolStr> = HashMap::new();
        let mut first_groups: IndexMap<SmolStr, Vec<SmolStr>> = IndexMap::new();
        let mut second_groups: IndexMap<SmolStr, Vec<SmolStr>> = IndexMap::new();

        // Pass 1: establish primary group assignments (first seen wins) and collect groups
        let mut name_counts: HashMap<SmolStr, usize> = HashMap::new();
        let mut left_group_name_map: HashMap<(String, usize), SmolStr> = HashMap::new();
        let mut right_group_name_map: HashMap<(String, usize), SmolStr> = HashMap::new();

        for (class_name, class) in &self.kern_classes {
            for (i, members) in class.groups1.iter().enumerate() {
                let base = Self::first_member_name(members, &glyph_order)
                    .unwrap_or_else(|| SmolStr::from(format!("{class_name}.L{}", i + 1)));
                let group_name = Self::make_unique_group_name(base, &mut name_counts);
                left_group_name_map.insert((class_name.clone(), i), group_name.clone());
                for glyph in members
                    .iter()
                    .filter_map(|t| Self::glyph_from_token(t, &glyph_order))
                {
                    if !left_primary.contains_key(&glyph) {
                        left_primary.insert(glyph.clone(), group_name.clone());
                        Self::push_group_member(&mut first_groups, &group_name, glyph.clone());
                    }
                }
            }

            for (j, members) in class.groups2.iter().enumerate().skip(1) {
                let base = Self::first_member_name(members, &glyph_order)
                    .unwrap_or_else(|| SmolStr::from(format!("{class_name}.R{j}")));
                let group_name = Self::make_unique_group_name(base, &mut name_counts);
                right_group_name_map.insert((class_name.clone(), j), group_name.clone());
                for glyph in members
                    .iter()
                    .filter_map(|t| Self::glyph_from_token(t, &glyph_order))
                {
                    if !right_primary.contains_key(&glyph) {
                        right_primary.insert(glyph.clone(), group_name.clone());
                        Self::push_group_member(&mut second_groups, &group_name, glyph.clone());
                    }
                }
            }
        }

        let master = self.font.masters.get_mut(0).ok_or_else(|| {
            BabelfontError::General("No master available when processing kerning".to_string())
        })?;

        // Pass 2: apply class kerning, flattening non-primary memberships to glyph pairs
        for (class_name, class) in &self.kern_classes {
            let cols = class.groups2.len().max(1);

            for (i, left_members_raw) in class.groups1.iter().enumerate() {
                let left_group_name = left_group_name_map
                    .get(&(class_name.clone(), i))
                    .cloned()
                    .unwrap_or_else(|| SmolStr::from(format!("{class_name}.L{}", i + 1)));
                let left_members: Vec<SmolStr> = left_members_raw
                    .iter()
                    .filter_map(|t| Self::glyph_from_token(t, &glyph_order))
                    .collect();
                if left_members.is_empty() {
                    continue;
                }

                for (j, right_members_raw) in class.groups2.iter().enumerate() {
                    let idx = i * cols + j;
                    if idx >= class.kerns.len() {
                        break;
                    }

                    let value = class.kerns[idx];
                    if value == 0 {
                        continue;
                    }

                    let right_members: Vec<SmolStr> = right_members_raw
                        .iter()
                        .filter_map(|t| Self::glyph_from_token(t, &glyph_order))
                        .collect();
                    if right_members.is_empty() {
                        continue;
                    }

                    let right_group_name = right_group_name_map
                        .get(&(class_name.clone(), j))
                        .cloned()
                        .unwrap_or_else(|| SmolStr::from(format!("{class_name}.R{j}")));

                    let left_targets: Vec<SmolStr> = if left_members.iter().all(|g| {
                        left_primary
                            .get(g)
                            .map(|p| p == &left_group_name)
                            .unwrap_or(false)
                    }) {
                        vec![SmolStr::from(format!("@{left_group_name}"))]
                    } else {
                        left_members.clone()
                    };

                    let right_targets: Vec<SmolStr> = if right_members.iter().all(|g| {
                        right_primary
                            .get(g)
                            .map(|p| p == &right_group_name)
                            .unwrap_or(false)
                    }) {
                        vec![SmolStr::from(format!("@{right_group_name}"))]
                    } else {
                        right_members.clone()
                    };

                    for lt in &left_targets {
                        for rt in &right_targets {
                            master.kerning.insert((lt.clone(), rt.clone()), value);
                        }
                    }
                }
            }
        }

        // Pass 3: explicit kerning pairs (Kerns2), mapped by glyph index
        for pairs in self.kern_pairs.values() {
            for (left, entries) in pairs {
                for (gid, value) in entries {
                    if let Some(right_name) = glyph_order.get(*gid) {
                        master.kerning.insert(
                            (
                                SmolStr::from(left.as_str()),
                                SmolStr::from(right_name.as_str()),
                            ),
                            *value,
                        );
                    }
                }
            }
        }

        self.font.first_kern_groups = first_groups;
        self.font.second_kern_groups = second_groups;

        Ok(())
    }

    /// Parse a single spline segment line from SFD format.
    /// SFD spline lines have the format: "x1 y1 x2 y2 ... segment_type flags"
    /// Where segment_type is 'm' (move), 'l' (line), or 'c' (curve).
    /// Returns (points, segment_type, flags).
    fn parse_spline_segment(line: &str) -> Option<SplineSegment> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        // Use regex pattern similar to Python: split on " [lmc] "
        if let Some(m_pos) = line.rfind(" m ") {
            let (coords_str, rest) = line.split_at(m_pos);
            let rest = rest.trim_start_matches(" m ").trim();
            let (flags, _) = rest.split_once(' ').unwrap_or((rest, ""));
            let points = Self::parse_coordinates(coords_str)?;
            return Some((points, 'm', flags.to_string()));
        }
        if let Some(m_pos) = line.rfind(" l ") {
            let (coords_str, rest) = line.split_at(m_pos);
            let rest = rest.trim_start_matches(" l ").trim();
            let (flags, _) = rest.split_once(' ').unwrap_or((rest, ""));
            let points = Self::parse_coordinates(coords_str)?;
            return Some((points, 'l', flags.to_string()));
        }
        if let Some(m_pos) = line.rfind(" c ") {
            let (coords_str, rest) = line.split_at(m_pos);
            let rest = rest.trim_start_matches(" c ").trim();
            let (flags, _) = rest.split_once(' ').unwrap_or((rest, ""));
            let points = Self::parse_coordinates(coords_str)?;
            return Some((points, 'c', flags.to_string()));
        }

        None
    }

    /// Parse a coordinate string into pairs of (x, y) f64 values.
    fn parse_coordinates(coords_str: &str) -> Option<Vec<(f64, f64)>> {
        let values: Result<Vec<f64>, _> = coords_str
            .split_whitespace()
            .map(|s| s.parse::<f64>())
            .collect();

        let values = values.ok()?;
        if values.len() % 2 != 0 {
            return None; // Must have even number of coordinates
        }

        let mut points = Vec::new();
        for chunk in values.chunks(2) {
            if chunk.len() == 2 {
                points.push((chunk[0], chunk[1]));
            }
        }
        Some(points)
    }

    /// Convert SFD spline lines into a Path structure.
    /// Handles contours, segments, and node types.
    fn splines_to_path(
        spline_lines: &[String],
        is_quadratic: bool,
    ) -> Result<Vec<Path>, BabelfontError> {
        let mut paths = Vec::new();
        let mut nodes = Vec::new();
        let mut last_point_flags: Option<String> = None;

        for line in spline_lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Check for contour name/other markers (can be added as format-specific)
            if line.contains(": ")
                && !line.contains(|c: char| c.is_numeric() || c == '-' || c == '.')
            {
                // This looks like a key-value (e.g., "Contour name: something")
                // Finish current path if any
                if !nodes.is_empty() {
                    paths.push(Path {
                        nodes: nodes.clone(),
                        closed: !Self::is_force_open_path(last_point_flags.as_deref()),
                        ..Default::default()
                    });
                    nodes.clear();
                    last_point_flags = None;
                }
                continue;
            }

            // Try to parse as a segment line
            if let Some((points, seg_type, flags)) = Self::parse_spline_segment(line) {
                let smooth = Self::is_smooth_from_flags(&flags);

                match seg_type {
                    'm' => {
                        // Move: start a new contour
                        if !nodes.is_empty() {
                            let mut path = Path {
                                nodes: nodes.clone(),
                                closed: !Self::is_force_open_path(last_point_flags.as_deref()),
                                ..Default::default()
                            };
                            remove_implicit_move_in_closed_path(&mut path);
                            paths.push(path);
                            nodes.clear();
                            last_point_flags = None;
                        }
                        if let Some((x, y)) = points.first() {
                            let mut format_specific = FormatSpecific::default();
                            format_specific.insert(
                                "sfd.point_flags".to_string(),
                                serde_json::Value::String(flags.clone()),
                            );
                            nodes.push(Node {
                                x: *x,
                                y: *y,
                                nodetype: NodeType::Move,
                                smooth,
                                format_specific,
                            });
                            last_point_flags = Some(flags.clone());
                        }
                    }
                    'l' => {
                        // Line: add a line node
                        if let Some((x, y)) = points.first() {
                            let mut format_specific = FormatSpecific::default();
                            format_specific.insert(
                                "sfd.point_flags".to_string(),
                                serde_json::Value::String(flags.clone()),
                            );
                            nodes.push(Node {
                                x: *x,
                                y: *y,
                                nodetype: NodeType::Line,
                                smooth,
                                format_specific,
                            });
                            last_point_flags = Some(flags.clone());
                        }
                    }
                    'c' => {
                        if is_quadratic {
                            if let (Some((cx, cy)), Some((x, y))) = (points.first(), points.last())
                            {
                                nodes.push(Node {
                                    x: *cx,
                                    y: *cy,
                                    nodetype: NodeType::OffCurve,
                                    smooth: false,
                                    format_specific: Default::default(),
                                });
                                let mut format_specific = FormatSpecific::default();
                                format_specific.insert(
                                    "sfd.point_flags".to_string(),
                                    serde_json::Value::String(flags.clone()),
                                );
                                nodes.push(Node {
                                    x: *x,
                                    y: *y,
                                    nodetype: NodeType::QCurve,
                                    smooth,
                                    format_specific,
                                });
                                last_point_flags = Some(flags.clone());
                            }
                        } else {
                            // Cubic curve: add 2 off-curve points, then 1 on-curve
                            for (i, (x, y)) in points.iter().enumerate() {
                                if i < 2 {
                                    // Off-curve control points
                                    nodes.push(Node {
                                        x: *x,
                                        y: *y,
                                        nodetype: NodeType::OffCurve,
                                        smooth: false,
                                        format_specific: Default::default(),
                                    });
                                } else {
                                    // Final on-curve point
                                    let mut format_specific = FormatSpecific::default();
                                    format_specific.insert(
                                        "sfd.point_flags".to_string(),
                                        serde_json::Value::String(flags.clone()),
                                    );
                                    nodes.push(Node {
                                        x: *x,
                                        y: *y,
                                        nodetype: NodeType::Curve,
                                        smooth,
                                        format_specific,
                                    });
                                    last_point_flags = Some(flags.clone());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Finish the last path
        if !nodes.is_empty() {
            let mut path = Path {
                nodes: nodes.clone(),
                closed: !Self::is_force_open_path(last_point_flags.as_deref()),
                ..Default::default()
            };
            remove_implicit_move_in_closed_path(&mut path);
            paths.push(path);
        }

        Ok(paths)
    }

    fn is_force_open_path(flags: Option<&str>) -> bool {
        let Some(raw) = flags else {
            return false;
        };
        let parsed = Self::parse_point_flags(raw).unwrap_or(0);
        (parsed & 0x400) != 0
    }

    fn parse_point_flags(flags: &str) -> Option<u32> {
        let token = flags
            .split(',')
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;

        if let Some(hex) = token.strip_prefix("0x") {
            u32::from_str_radix(hex, 16).ok()
        } else {
            token.parse::<u32>().ok()
        }
    }

    /// Extract the smooth flag from SFD flags string.
    /// Flags are like "0x100,0x200" or just "0". The lower 2 bits encode smoothness.
    fn is_smooth_from_flags(flags: &str) -> bool {
        if let Some(part) = flags.split(',').next() {
            if let Some(num_str) = part.strip_prefix("0x") {
                if let Ok(num) = u32::from_str_radix(num_str, 16) {
                    return (num & 0x3) != 1;
                }
            } else if let Ok(num) = flags.parse::<u32>() {
                return (num & 0x3) != 1;
            }
        }
        false
    }

    /// Resolve component references after all glyphs have been parsed.
    /// SFD stores references by glyph index; we need to convert to glyph names
    /// and extract the transformation matrix.
    fn resolve_component_references(&mut self) -> Result<(), BabelfontError> {
        // Build a mapping from glyph index to glyph name
        let glyph_order: Vec<String> = self
            .font
            .glyphs
            .iter()
            .map(|g| g.name.to_string())
            .collect();

        for glyph in &mut self.font.glyphs.0 {
            for layer in &mut glyph.layers {
                // Extract and process stored references
                if let Some(serde_json::Value::Array(refer_array)) =
                    layer.format_specific.get("sfd.refer")
                {
                    let refer_strs: Vec<String> = refer_array
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();

                    for refer_str in refer_strs {
                        if let Some(component) = Self::parse_refer(&refer_str, &glyph_order)? {
                            layer.shapes.push(Shape::Component(component));
                        }
                    }

                    // Remove the temporary storage after processing
                    layer.format_specific.remove("sfd.refer");
                }
            }
        }

        Ok(())
    }

    /// Parse a single Refer line from SFD format.
    /// Format: "<glyph_index> <unicodeenc> <N|S> <xx> <xy> <yx> <yy> <tx> <ty> <flags> [base_pt ref_pt [O]]"
    fn parse_refer(
        refer_str: &str,
        glyph_order: &[String],
    ) -> Result<Option<Component>, BabelfontError> {
        let parts: Vec<&str> = refer_str.split_whitespace().collect();
        if parts.len() < 10 {
            // Malformed reference; skip it
            return Ok(None);
        }

        // Parse the glyph index
        let glyph_idx = parts[0].parse::<usize>().map_err(|_| {
            BabelfontError::General(format!("Invalid glyph index in Refer: {}", parts[0]))
        })?;

        if glyph_idx >= glyph_order.len() {
            return Err(BabelfontError::General(format!(
                "Glyph index {} out of bounds (max {})",
                glyph_idx,
                glyph_order.len()
            )));
        }

        let reference_name = glyph_order[glyph_idx].clone();

        // Extract the transformation matrix from positions 3-8
        // Format: [xx, xy, yx, yy, tx, ty]
        let matrix_parts: Result<Vec<f64>, _> =
            parts[3..9].iter().map(|p| p.parse::<f64>()).collect();

        let matrix = matrix_parts.map_err(|_| {
            BabelfontError::General("Failed to parse transformation matrix".to_string())
        })?;

        if matrix.len() != 6 {
            return Ok(None);
        }

        // Convert the matrix [xx, xy, yx, yy, tx, ty] into a kurbo::Affine
        // kurbo::Affine coefficients are [xx, xy, yx, yy, tx, ty]
        let matrix_arr = [
            matrix[0], matrix[1], matrix[2], matrix[3], matrix[4], matrix[5],
        ];
        let affine = kurbo::Affine::new(matrix_arr);
        let transform = DecomposedAffine::from(affine);

        let mut format_specific = FormatSpecific::default();

        format_specific.insert(
            "sfd.refer.unicodeenc".to_string(),
            serde_json::Value::String(parts[1].to_string()),
        );
        format_specific.insert(
            "sfd.refer.selected".to_string(),
            serde_json::Value::Bool(parts[2] == "S"),
        );

        let flags = parts[9].parse::<u32>().unwrap_or(0);
        format_specific.insert(
            "sfd.refer.flags".to_string(),
            serde_json::Value::Number(flags.into()),
        );
        format_specific.insert(
            "sfd.refer.use_my_metrics".to_string(),
            serde_json::Value::Bool((flags & 0x1) != 0),
        );
        format_specific.insert(
            "sfd.refer.round_translation_to_grid".to_string(),
            serde_json::Value::Bool((flags & 0x2) != 0),
        );
        format_specific.insert(
            "sfd.refer.point_match".to_string(),
            serde_json::Value::Bool((flags & 0x4) != 0),
        );

        if (flags & 0x4) != 0 && parts.len() >= 12 {
            if let Ok(base_pt) = parts[10].parse::<i64>() {
                format_specific.insert(
                    "sfd.refer.match_pt_base".to_string(),
                    serde_json::Value::Number(base_pt.into()),
                );
            }
            if let Ok(ref_pt) = parts[11].parse::<i64>() {
                format_specific.insert(
                    "sfd.refer.match_pt_ref".to_string(),
                    serde_json::Value::Number(ref_pt.into()),
                );
            }
            if parts.get(12).copied() == Some("O") {
                format_specific.insert(
                    "sfd.refer.point_match_out_of_date".to_string(),
                    serde_json::Value::Bool(true),
                );
            }
        }

        let component = Component {
            reference: reference_name.into(),
            transform,
            location: Default::default(),
            format_specific,
        };

        Ok(Some(component))
    }

    fn parse_anchor(&self, v: &str) -> Option<crate::Anchor> {
        // Quoted name, x, y, kind, index
        let parts: Vec<&str> = v.split_whitespace().collect();
        if parts.len() < 5 {
            return None;
        }
        let name = decode_utf7(parts[0].trim_matches('"'));
        let x = parts[1].parse::<f64>().ok()?;
        let y = parts[2].parse::<f64>().ok()?;
        let kind = parts[3];
        let index = parts[4].parse::<usize>().ok()?;
        let mut format_specific = FormatSpecific::default();
        format_specific.insert(
            "sfd.kind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
        format_specific.insert(
            "sfd.index".to_string(),
            serde_json::Value::Number(index.into()),
        );
        Some(crate::Anchor {
            name,
            x,
            y,
            format_specific,
        })
    }

    fn parse_oneline_layout(&self, value: Option<&str>) -> Option<(SmolStr, Vec<SmolStr>)> {
        if let Some(v) = value {
            // Split quoted "name" component and following glyphs
            let parts: Vec<&str> = v.split('"').collect();
            if parts.len() >= 3 {
                let name = SmolStr::from(parts[1]);
                let glyphs_part = parts[2].trim();
                let glyphs: Vec<SmolStr> = glyphs_part
                    .split_whitespace()
                    .map(|s| SmolStr::from(s.trim_matches('"')))
                    .collect();
                return Some((name, glyphs));
            }
            None
        } else {
            None
        }
    }

    fn looks_like_spline_line(line: &str) -> bool {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            return false;
        }
        let starts_numeric = trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_digit() || c == '-' || c == '+')
            .unwrap_or(false);
        if !starts_numeric {
            return false;
        }
        (trimmed.contains(" m ") || trimmed.contains(" l ") || trimmed.contains(" c "))
            && !trimmed.contains(':')
    }

    fn insert_gtables(&mut self) {
        let mut feature_map: HashMap<SmolStr, Vec<(layout::FeatureLangSys, SmolStr)>> =
            HashMap::new();
        for (name, lookup) in self
            .gsub_lookups
            .0
            .iter_mut()
            .chain(self.gpos_lookups.0.iter_mut())
        {
            // Populate the block with code from the subtables
            lookup.block.statements.extend(
                lookup
                    .subtables
                    .iter()
                    .flat_map(|(_name, st)| st.iter())
                    .cloned(),
            );
            self.font.features.prefixes.insert(
                name.into(),
                crate::features::PossiblyAutomaticCode {
                    code: lookup.block.as_fea(""),
                    ..Default::default()
                },
            );
            // Rearrange lookup.features as feature: Vec<FeatureLangSys>
            for fls in &lookup.features {
                feature_map
                    .entry(fls.feature.clone())
                    .or_default()
                    .push((fls.clone(), lookup.block.name.clone()));
            }
        }
        // Now insert a feature reference for each feature
        for (feature, langs_lookup) in feature_map {
            let mut featureblock =
                fea_rs_ast::FeatureBlock::new(feature.clone(), vec![], false, 0..0);
            for (lang, lookupname) in langs_lookup.into_iter() {
                featureblock
                    .statements
                    .extend(make_langsys(lang.script.clone(), lang.language.clone()));
                featureblock
                    .statements
                    .push(fea_rs_ast::Statement::LookupReference(
                        fea_rs_ast::LookupReferenceStatement::new(lookupname.into(), 0..0),
                    ));
            }
            // And now pop the featureblock into the feature
            // minus its wrapper
            let statements: Vec<String> = featureblock
                .statements
                .iter()
                .map(|x| x.as_fea(""))
                .collect();
            self.font.features.features.push((
                feature,
                crate::features::PossiblyAutomaticCode {
                    code: statements.join("\n"),
                    ..Default::default()
                },
            ));
        }
        for (tag, names) in self.feature_names.iter() {
            // Find the feature by name
            if let Some((_, feature)) = self
                .font
                .features
                .features
                .iter_mut()
                .find(|(fname, _)| *fname == tag.as_str())
            {
                feature.code = "featureNames {\n".to_string()
                    + (names
                        .iter()
                        .map(|(lang_id, name)| format!("    name 3 1 {} \"{}\";\n", lang_id, name))
                        .collect::<String>()
                        .as_str())
                    + "};\n"
                    + &feature.code;
            }
        }
    }
}

/// Load a FontForge SFD font or SFDir from a file path
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let mut parser = SfdParser::new(path);
    parser.parse()?;
    parser.resolve_component_references()?;
    parser.process_kerning()?;
    parser.insert_gtables();
    Ok(parser.font)
}

/// Load a FontForge SFD font from a string
pub fn load_str(content: &str) -> Result<Font, BabelfontError> {
    let mut parser = SfdParser::new_from_str(content.to_string());
    parser.parse()?;
    parser.resolve_component_references()?;
    parser.process_kerning()?;
    parser.insert_gtables();
    Ok(parser.font)
}

/// Save a Babelfont Font into a FontForge SFD file at the given path.
pub fn save_sfd(font: &Font, path: &PathBuf) -> Result<(), BabelfontError> {
    let sfd_str = to_str(font)?;
    std::fs::write(path, sfd_str)?;
    Ok(())
}

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

    emit_font_header(&mut out, font, &layer_registry);
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

#[derive(Debug, Default)]
struct LayerRegistry {
    layer_count: usize,
    defs: Vec<(usize, bool, String, usize)>,
    extras: HashMap<String, usize>,
}

impl LayerRegistry {
    fn from_font(font: &Font, default_master_id: &str) -> Self {
        let mut extra_quadratic: HashMap<String, bool> = HashMap::new();
        let mut defs: Vec<(usize, bool, String, usize)> = font
            .format_specific
            .get("sfd.layer_defs")
            .and_then(|v| v.as_array())
            .map(|defs| {
                defs.iter()
                    .filter_map(|entry| {
                        let obj = entry.as_object()?;
                        let index = obj.get("index")?.as_u64()? as usize;
                        let name = obj.get("name")?.as_str()?.to_string();
                        let is_quadratic = obj
                            .get("is_quadratic")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let flags = obj.get("flags").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        Some((index, is_quadratic, name, flags))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut extras: HashMap<String, usize> = defs
            .iter()
            .filter(|(_, _, name, _)| {
                !name.eq_ignore_ascii_case("Back") && !name.eq_ignore_ascii_case("Fore")
            })
            .map(|(idx, _, name, _)| (name.clone(), *idx))
            .collect();
        let mut next_idx = defs
            .iter()
            .map(|(idx, _, _, _)| *idx)
            .max()
            .map(|v| v + 1)
            .unwrap_or(2);

        for glyph in font.glyphs.iter() {
            for layer in &glyph.layers {
                if Self::is_background_layer(layer)
                    || Self::is_foreground_layer(layer, default_master_id)
                {
                    continue;
                }
                let key = Self::layer_key(layer);
                if let std::collections::hash_map::Entry::Vacant(v) = extras.entry(key) {
                    extra_quadratic.insert(v.key().clone(), layer_is_quadratic(layer));
                    v.insert(next_idx);
                    next_idx += 1;
                }
            }
        }

        if defs.is_empty() {
            defs = vec![
                (0, false, "Back".to_string(), 1),
                (1, false, "Fore".to_string(), 0),
            ];
        }

        let mut extra_pairs: Vec<(&String, &usize)> = extras.iter().collect();
        extra_pairs.sort_by_key(|(_, ix)| **ix);
        for (key, ix) in extra_pairs {
            if !defs
                .iter()
                .any(|(existing_idx, _, _, _)| existing_idx == ix)
            {
                defs.push((
                    *ix,
                    extra_quadratic.get(key).copied().unwrap_or(false),
                    key.clone(),
                    0,
                ));
            }
        }
        defs.sort_by_key(|(idx, _, _, _)| *idx);

        Self {
            layer_count: defs.len(),
            defs,
            extras,
        }
    }

    fn is_background_layer(layer: &Layer) -> bool {
        layer.is_background
            || layer
                .name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("Back"))
                .unwrap_or(false)
    }

    fn is_foreground_layer(layer: &Layer, default_master_id: &str) -> bool {
        matches!(&layer.master, LayerType::DefaultForMaster(id) if id == default_master_id)
            || layer
                .name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("Fore"))
                .unwrap_or(false)
    }

    fn layer_key(layer: &Layer) -> String {
        layer
            .name
            .clone()
            .or_else(|| layer.id.clone())
            .unwrap_or_else(|| "Layer".to_string())
    }

    fn index_for(&self, layer: &Layer, default_master_id: &str) -> usize {
        if Self::is_background_layer(layer) {
            0
        } else if Self::is_foreground_layer(layer, default_master_id) {
            1
        } else {
            self.extras
                .get(&Self::layer_key(layer))
                .copied()
                .unwrap_or(1)
        }
    }
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

fn emit_font_header(out: &mut Vec<String>, font: &Font, layer_registry: &LayerRegistry) {
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
        emit_header_key(out, font, layer_registry, key, &mut state);
    }

    while state.comment_index < comment_entries(font).len() {
        let entry = &comment_entries(font)[state.comment_index];
        state.comment_index += 1;
        out.push(format!("{}:{}", entry.0, entry.1));
    }

    while emit_layer_header && state.layer_index < layer_registry.defs.len() {
        emit_header_key(out, font, layer_registry, "Layer", &mut state);
    }

    emit_font_passthrough_keys_remaining(out, font, &mut state);
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
) {
    match key {
        "SplineFontDB" if !state.is_emitted(key) => {
            out.push(format!(
                "SplineFontDB: {}",
                font.format_specific
                    .get(HEADER_VERSION_KEY)
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
            if emit_metric_key(out, font, key, state)
                || emit_ot_key(out, font, key, state)
                || emit_passthrough_key(out, font, key, state)
            {}
        }
    }
}

fn comment_entries(font: &Font) -> Vec<(String, String)> {
    font.format_specific
        .get(COMMENT_ENTRIES_KEY)
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
    font.names
        .copyright
        .get_default()
        .map(|s| format!("Copyright: {}", sanitize_unquoted(s)))
}

fn version_line(font: &Font) -> String {
    let version_str = font
        .names
        .version
        .get_default()
        .cloned()
        .unwrap_or_else(|| format!("{}.{}", font.version.0, font.version.1));
    format!("Version: {}", sanitize_unquoted(&version_str))
}

fn unique_id_line(font: &Font) -> Option<String> {
    font.names
        .unique_id
        .get_default()
        .map(|s| format!("UniqueID: {}", sanitize_unquoted(s)))
}

fn emit_metric_key(
    out: &mut Vec<String>,
    font: &Font,
    key: &str,
    state: &mut HeaderEmitState,
) -> bool {
    if state.is_emitted(key) {
        return false;
    }
    let Some(master) = font.masters.first() else {
        return false;
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
        _ => return false,
    };
    if master.metrics.contains_key(&metric) {
        emit_metric(out, master, metric, key);
        state.mark_emitted(key);
    }
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
            } else if ot.os2_fs_type.map(|v| (v & (1 << 7)) != 0).unwrap_or(false) {
                Some("OS2_UseTypoMetrics: 1".to_string())
            } else {
                None
            }
        }
        "OS2_WeightWidthSlopeOnly" => {
            if let Some(raw) = font.format_specific.get(key).and_then(|v| v.as_str()) {
                Some(format!("{}: {}", key, sanitize_unquoted(raw)))
            } else if ot.os2_fs_type.map(|v| (v & (1 << 8)) != 0).unwrap_or(false) {
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

fn read_file_lossy(path: &std::path::Path) -> Result<String, BabelfontError> {
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn emit_metric(out: &mut Vec<String>, master: &crate::Master, metric: MetricType, key: &str) {
    if let Some(v) = master.metrics.get(&metric) {
        out.push(format!("{}: {}", key, v));
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

fn emit_font_level_kerning(
    _out: &mut Vec<String>,
    _font: &Font,
    _glyph_order: &[String],
    _glyph_index: &HashMap<SmolStr, usize>,
) {
    // Placeholder for class-based kerning (KernClass2) emission.
}

fn emit_features(_out: &mut Vec<String>, _font: &Font) {
    // Placeholder for Lookup/feature table emission.
}

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
            .get(HSTEM_KEY)
            .and_then(|v| v.as_str())
        {
            out.push(format!("HStem: {}", sanitize_unquoted(hstem)));
        }
        if let Some(vstem) = layer
            .format_specific
            .get(VSTEM_KEY)
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
                    format!("{} {} \"{}\"", right_gid, value, GENERATED_KERN_SUBTABLE)
                })
                .collect::<Vec<_>>()
                .join(" ");
            out.push(format!("Kerns2: {}", payload));
        }
    }

    out.push("EndChar".to_string());
    Ok(())
}

fn pick_foreground_width(glyph: &Glyph, default_master_id: &str) -> f32 {
    glyph_foreground_layer(glyph, default_master_id)
        .map(|l| l.width)
        .unwrap_or(0.0)
}

fn glyph_foreground_layer<'a>(glyph: &'a Glyph, default_master_id: &str) -> Option<&'a Layer> {
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

        out.push(format!(
            "AnchorPoint: \"{}\" {} {} {} {}",
            escape_quoted(&anchor.name),
            fmt_num(anchor.x),
            fmt_num(anchor.y),
            kind,
            index
        ));
    }
}

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

fn fmt_num(n: f64) -> String {
    if (n.round() - n).abs() < 1e-6 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{:.6}", n);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn sanitize_unquoted(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect::<String>()
}

fn escape_quoted(s: &str) -> String {
    s.replace('"', "'")
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use similar::TextDiff;

    use super::*;

    #[rstest]
    fn test_roundtrip(#[files("resources/fontforge/*.sfd")] path: PathBuf) {
        let data =
            String::from_utf8_lossy(&fs::read(&path).expect("Failed to read SFD file bytes"))
                .into_owned();
        let font = load_str(&data).expect("Failed to load SFD font");
        let output = to_str(&font).expect("Failed to convert font back to SFD");

        // Re-parse the generated SFD and compare core model fields.
        let reparsed = load_str(&output).expect("Failed to reparse emitted SFD");

        assert_eq!(
            reparsed.glyphs.len(),
            font.glyphs.len(),
            "glyph count changed"
        );
        assert_eq!(
            reparsed
                .glyphs
                .iter()
                .map(|g| g.name.to_string())
                .collect::<Vec<_>>(),
            font.glyphs
                .iter()
                .map(|g| g.name.to_string())
                .collect::<Vec<_>>(),
            "glyph order changed"
        );

        // Check a few key fields so regressions are surfaced early while
        // acknowledging that this emitter currently serializes only a subset of SFD.
        assert_eq!(
            reparsed
                .names
                .postscript_name
                .get_default()
                .map(|s| s.as_str()),
            font.names.postscript_name.get_default().map(|s| s.as_str())
        );
        assert_eq!(reparsed.masters.len(), font.masters.len());

        // Now do a full diff to see what we're missing
        if output != data && data.split("\n").count() < 1000 {
            let diff = TextDiff::from_lines(&data, &output)
                .unified_diff()
                .context_radius(5)
                .header("Original SFD", "Re-emitted SFD")
                .to_string();
            println!("{}", diff);
            panic!("Roundtrip SFD did not match original");
        }
    }

    #[test]
    fn test_quadratic_layer_parses_and_emits_qcurves() {
        let data = concat!(
            "SplineFontDB: 3.0\n",
            "LayerCount: 2\n",
            "Layer: 0 1 \"Back\" 1\n",
            "Layer: 1 1 \"Fore\" 0\n",
            "BeginChars: 1 1\n",
            "StartChar: quad\n",
            "Encoding: -1 -1 0\n",
            "Width: 500\n",
            "Fore\n",
            "SplineSet\n",
            "268 610 m 4,0,1\n",
            " 336 610 336 610 386.5 585.5 c 0x400,-1,2\n",
            "EndSplineSet\n",
            "EndChar\n",
            "EndChars\n",
            "EndSplineFont\n"
        );

        let font = load_str(data).expect("Failed to parse quadratic SFD");
        let layer = glyph_foreground_layer(&font.glyphs[0], "default").expect("Missing layer");
        let path = layer.paths().next().expect("Missing path");

        assert_eq!(
            layer
                .format_specific
                .get(LAYER_QUADRATIC_KEY)
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(path.nodes.len(), 3);
        assert_eq!(path.nodes[0].nodetype, NodeType::Move);
        assert_eq!(path.nodes[1].nodetype, NodeType::OffCurve);
        assert_eq!(path.nodes[2].nodetype, NodeType::QCurve);

        let emitted = to_str(&font).expect("Failed to emit quadratic SFD");
        assert!(emitted.contains("Layer: 1 1 \"Fore\" 0"));
        assert!(emitted.contains(" 336 610 336 610 386.5 585.5 c 0x400,-1,2"));
    }
}
