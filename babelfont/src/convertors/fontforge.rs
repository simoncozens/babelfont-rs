//! FontForge SFD/SFDir conversion.
//!
//! The parser walks the file in the same order the `Font` model is shaped, and the
//! functions below are grouped the same way so the two can be read side by side:
//!
//! 1. the font as a whole (names, metrics, layer definitions, top-level dispatch)
//! 2. masters, axes and instances (vertical metric resolution, kerning declarations)
//! 3. glyphs
//! 4. layers
//! 5. anchors and shapes
//! 6. paths and components
//! 7. font-level kerning groups
//! 8. OpenType features (lookups, contextual rules, GSUB/GPOS assembly)
//!
//! Each section opens a fresh `impl SfdParser` block; they are all one type.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::LazyLock,
};

use chrono::DateTime;
use fea_rs_ast::AsFea;
use fontdrasil::coords::DesignLocation;
use itertools::Itertools as _;

use crate::{
    common::{decomposition::DecomposedAffine, tag_from_string, Color, NodeType},
    convertors::fontforge::{
        layout::{make_langsys, GTable},
        offsetmetrics::compute_font_bbox_y,
        stringhelpers::{decode_sfd_line_escapes, tokenize_preserving_quotes},
        utf7::decode_utf7,
    },
    features::PossiblyAutomaticCode,
    names::ot_lang_id_to_layout_tag,
    BabelfontError, Component, Font, FormatSpecific, Glyph, GlyphCategory, Guide, Layer, LayerType,
    MetricType, NameId, Shape,
};
use indexmap::IndexMap;
use smol_str::SmolStr;

mod emit;
mod layerregistry;
mod layout;
mod offsetmetrics;
mod pathreading;
mod stringhelpers;
#[cfg(test)]
mod tests;
mod utf7;

use regex::Regex;
#[allow(clippy::unwrap_used)] // Safe because the regex is valid
static CHAIN_POSSUB_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Matches: <kind> "<subtable name>" <n1> <n2> <n3> <nRules>. The kind
    // vocabulary is SfdKind::parse's to judge, not this pattern's: an unknown kind
    // must reach the code that can say "unhandled kind", not fail as a bad header.
    Regex::new(r#"(\w+)\s+"([^"]*)"\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)"#).unwrap()
});
pub(crate) const GENERATED_KERN_SUBTABLE: &str = "generated_kern";
pub(crate) const HEADER_VERSION_KEY: &str = "sfd.splinefontdb_version";
pub(crate) const COMMENT_ENTRIES_KEY: &str = "sfd.comment_entries";
pub(crate) const HSTEM_KEY: &str = "sfd.HStem";
pub(crate) const VSTEM_KEY: &str = "sfd.VStem";
pub(crate) const LAYER_QUADRATIC_KEY: &str = "sfd.is_quadratic";

// ===========================================================================
// Public entry points
// ===========================================================================

/// Load a FontForge SFD font or SFDir from a file path
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    SfdParser::new(path).into_font()
}

/// Load a FontForge SFD font from a string
pub fn load_str(content: &str) -> Result<Font, BabelfontError> {
    SfdParser::new_from_str(content.to_string()).into_font()
}

/// Save a Babelfont Font into a FontForge SFD file at the given path.
pub fn save_sfd(font: &Font, path: &PathBuf) -> Result<(), BabelfontError> {
    let sfd_str = emit::to_str(font)?;
    std::fs::write(path, sfd_str)?;
    Ok(())
}

fn read_file_lossy(path: &std::path::Path) -> Result<String, BabelfontError> {
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

// ===========================================================================
// Parser state
// The parser carries the whole in-progress Font, plus the raw
// SFD tables that are folded into it once every glyph is known.
// ===========================================================================

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
    /// Every lookup name already handed out, generated `_N` forms included.
    taken_lookup_names: HashSet<String>,
    /// Original lookup name -> the unique name it was given, for resolving references.
    assigned_lookup_names: HashMap<String, String>,
    /// Chain/context substitution and positioning data: subtable name -> rules.
    ///
    /// One subtable may hold several rules: a `class`-kind FPST lists one rule per
    /// class sequence, unlike `coverage`/`glyph` which have exactly one.
    chain_pos_sub: IndexMap<String, Vec<layout::ChainPosSubEntry>>,
    anchor_class_decls: Vec<(String, String)>,
    content: Option<String>, // Optional pre-loaded content for load_str()
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

// ===========================================================================
// 1. The font as a whole
// ===========================================================================

/// Write a run-together italic style the way the Glyphs convention spells it.
///
/// A PostScript name concatenates the style ("Almendra-BoldItalic"), but a
/// style name is expected to separate the slope from the weight ("Bold
/// Italic"). Left run together it is not recognised as one of the four
/// standard styles, and the family name absorbs it: the built font declares
/// family "Almendra BoldItalic" / subfamily "Italic" where the shipped binary
/// says "Almendra" / "Bold Italic".
///
/// Only the slope is separated. Weight names stay as they are -- "SemiBold" is
/// spelled without a space by the same convention, so a general split on
/// capitals would be wrong.
fn space_before_italic(style: &str) -> String {
    match style.strip_suffix("Italic") {
        Some(prefix) if !prefix.is_empty() && !prefix.ends_with(' ') => {
            format!("{prefix} Italic")
        }
        _ => style.to_string(),
    }
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
            taken_lookup_names: HashSet::new(),
            assigned_lookup_names: HashMap::new(),
            chain_pos_sub: IndexMap::new(),
            anchor_class_decls: Vec::new(),
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
            taken_lookup_names: HashSet::new(),
            assigned_lookup_names: HashMap::new(),
            chain_pos_sub: IndexMap::new(),
            anchor_class_decls: Vec::new(),
            content: Some(content),
        }
    }

    /// Parse the SFD/SFDir data into a Font structure.
    fn into_font(mut self) -> Result<Font, BabelfontError> {
        self.parse()?;
        self.resolve_component_references()?;
        self.resolve_offset_metrics()?;
        self.process_kerning()?;
        self.insert_gtables();
        Ok(self.font)
    }

    /// Read the SFD file (or SFDir directory) into a vector of lines.
    ///
    /// An SFDir is an "exploded" SFD: `font.props` holds the header (the same
    /// syntax as an SFD, ending with `EndSplineFont` and without a
    /// `BeginChars` section), and each glyph lives in its own `*.glyph` file
    /// as a complete `StartChar: ... EndChar` block. Reassemble the stream
    /// FontForge would have written as a single `.sfd`: header (minus the
    /// trailing `EndSplineFont`), a synthesized `BeginChars` line, the glyph
    /// blocks in original-GID order, then `EndChars`/`EndSplineFont`.
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
            let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            if lines.last().is_some_and(|l| l.trim() == "EndSplineFont") {
                lines.pop();
            }

            // Each glyph file carries `Encoding: <slot> <unicode> <orig-gid>`;
            // sort by (orig-gid, filename) to reproduce FontForge's glyph
            // order deterministically.
            let mut glyphs: Vec<(i64, String, Vec<String>)> = Vec::new();
            let mut max_slot: i64 = 0;
            for entry in std::fs::read_dir(&self.path)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.ends_with(".glyph") || !entry.path().is_file() {
                    continue;
                }
                let glyph_content = read_file_lossy(&entry.path())?;
                let glyph_lines: Vec<String> =
                    glyph_content.lines().map(|l| l.to_string()).collect();
                let mut gid = i64::MAX;
                for l in &glyph_lines {
                    if let Some(rest) = l.strip_prefix("Encoding: ") {
                        let fields: Vec<&str> = rest.split_whitespace().collect();
                        if let Some(Ok(slot)) = fields.first().map(|s| s.parse::<i64>()) {
                            max_slot = max_slot.max(slot + 1);
                        }
                        if let Some(Ok(g)) = fields.get(2).map(|s| s.parse::<i64>()) {
                            gid = g;
                        }
                        break;
                    }
                }
                glyphs.push((gid, name, glyph_lines));
            }
            glyphs.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));

            lines.push(format!("BeginChars: {} {}", max_slot, glyphs.len()));
            lines.push(String::new());
            for (_, _, glyph_lines) in glyphs {
                lines.extend(glyph_lines);
                lines.push(String::new());
            }
            lines.push("EndChars".to_string());
            lines.push("EndSplineFont".to_string());
            return Ok(lines);
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
            let master: crate::Master = crate::Master::new(
                "Regular",
                // Deterministic: nothing requires a random UUID here, only
                // uniqueness within the document. A random id made every
                // conversion of an unchanged .sfd produce a different file.
                "babelfont/sfd/master/Regular",
                DesignLocation::default(),
            );
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
                    let (section, next_i) = self.get_section(&data, i, "EndFPST", value.as_deref());
                    self.parse_chain_pos_sub(&key, &section);
                    i = next_i;
                }
                "Grid" => {
                    let (section, next_i) = self.get_section(&data, i, "EndSplineSet", None);
                    // This is a splineset, so we parse it into paths
                    let paths = pathreading::splines_to_path(&section, false)?;
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
                "Lookup" | "AnchorClass" | "AnchorClass2" | "MarkAttachClasses"
                | "MarkAttachSets" | "KernPairs" => {
                    if key == "Lookup" {
                        if let Some(v) = &value {
                            self.parse_lookup(v);
                        }
                    } else if let Some(v) = &value {
                        if key == "AnchorClass" || key == "AnchorClass2" {
                            self.register_anchor_classes(v);
                        }
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
                        // FontForge keeps this field on one SFD line, escaping
                        // real line breaks as "\n" (and backslashes as "\\"),
                        // and decodes them on export: 20 of 121 Google Fonts
                        // corpus SFDs carry the escape, and every shipped
                        // binary has the real line break in name ID 0.
                        self.font.names.copyright = decode_sfd_line_escapes(v).into();
                    }
                }
                "Version" => {
                    if let Some(v) = &value {
                        // FontForge's export composes name ID 5 as "Version "
                        // plus this field: across 120 SFD/shipped-binary pairs
                        // no SFD field contains the word and 114 binaries carry
                        // exactly that composition. Interpret it the same way,
                        // whatever the contents. `version_line` strips the
                        // prefix again on write, so an SFD round trip is stable.
                        self.font.names.version = format!("Version {v}").into();
                        // head.fontRevision must agree with name ID 5. The minor
                        // is the fractional digits padded to three, not a float
                        // fraction scaled by 100 -- that turned 1.002 into 1.000.
                        if let Some(first_word) = v.split_whitespace().next() {
                            if let Some(parts) = crate::common::version_major_minor(first_word) {
                                self.font.version = parts;
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
                        // '<feature tag>' followed by one `<language id> "<name>"` pair
                        // per language. The names are UTF-7, quotes included, so the
                        // quote characters on the line are always delimiters.
                        let tokens = tokenize_preserving_quotes(v);
                        let mut it = tokens.iter();
                        let tag = it
                            .next()
                            .map(|t| t.trim_matches('\''))
                            .unwrap_or_default()
                            .to_string();
                        let mut parsed_any = false;
                        while let (Some(lang), Some(name)) = (it.next(), it.next()) {
                            let Ok(lang_id) = lang.parse::<u32>() else {
                                break;
                            };
                            parsed_any = true;
                            self.feature_names
                                .entry(tag.as_str().into())
                                .or_default()
                                .push((lang_id, decode_utf7(name.trim_matches('"'))));
                        }
                        if !parsed_any {
                            log::warn!("invalid OtfFeatName line: {v}");
                        }
                    }
                }
                // Metrics
                "ItalicAngle" => {
                    // FontForge writes the angle in the OpenType `post`
                    // convention: counter-clockwise, so a right-leaning italic
                    // is NEGATIVE. We use the opposite (clockwise), so negate
                    // on reading.
                    if let Some(v) = &value {
                        if let Ok(angle) = v.trim().parse::<f64>() {
                            self.font.masters[0]
                                .metrics
                                .insert(MetricType::ItalicAngle, (-angle).round() as i32);
                        }
                    }
                }
                "UnderlinePosition" => parse_metric!(self, value, UnderlinePosition),
                "UnderlineWidth" => parse_metric!(self, value, UnderlineThickness),
                "Ascent" => parse_metric!(self, value, Ascender),
                "Descent" => {
                    // Negate
                    if let Some(v) = &value {
                        if let Ok(val) = v.parse::<i32>() {
                            self.font.masters[0]
                                .metrics
                                .insert(MetricType::Descender, -val);
                        }
                    }
                }
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
                    let current_fsselection =
                        self.font.custom_ot_values.os2_fs_selection.unwrap_or(0);
                    let enabled = value
                        .as_deref()
                        .and_then(|v| v.parse::<u16>().ok())
                        .unwrap_or(1)
                        != 0;
                    self.font.custom_ot_values.os2_fs_selection = Some(if enabled {
                        current_fsselection | 1 << 7
                    } else {
                        current_fsselection & !(1 << 7)
                    });
                }
                "OS2_WeightWidthSlopeOnly" => {
                    if let Some(v) = &value {
                        self.font.format_specific.insert(
                            "OS2_WeightWidthSlopeOnly".to_string(),
                            serde_json::Value::String(v.clone()),
                        );
                    }
                    let current_fsselection =
                        self.font.custom_ot_values.os2_fs_selection.unwrap_or(0);
                    let enabled = value
                        .as_deref()
                        .and_then(|v| v.parse::<u16>().ok())
                        .unwrap_or(1)
                        != 0;
                    self.font.custom_ot_values.os2_fs_selection = Some(if enabled {
                        current_fsselection | 1 << 8
                    } else {
                        current_fsselection & !(1 << 8)
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
                    // A vendor of four spaces is a DELIBERATE blank, not an
                    // absent value. 7 families in a Google Fonts corpus write
                    // `OS2Vendor: '    '` -- astloch, bigshotone, cambo, copse,
                    // dawningofanewday, economica, coveredbyyourgrace -- and
                    // their shipped binaries carry the blank through. Letting it
                    // fall through as "no vendor" makes the downstream default
                    // substitute the literal string "NONE", which is not a real
                    // vendor and not what the source asked for.
                    //
                    // FontForge also pads a short vendor to four bytes with NULs
                    // inside the quotes ("'STC\0'"); a NUL is not legal in a tag,
                    // so those are stripped and re-padded with spaces.
                    if let Some(raw) = value.as_ref().map(|s| s.trim_matches('\'')) {
                        let cleaned = raw.replace('\0', "");
                        let tag = if cleaned.trim().is_empty() {
                            tag_from_string("    ")
                        } else {
                            tag_from_string(cleaned.trim())
                        };
                        if let Ok(tag) = tag {
                            self.font.custom_ot_values.os2_vendor_id = Some(tag);
                        }
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
                "Stylemap" => {
                    if let Some(v) = &value {
                        self.font
                            .format_specific
                            .insert(key.clone(), serde_json::Value::String(v.clone()));
                    }
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

        // FontForge SFD defines the em square as Ascent + Descent. The font model
        // defaults `upm` to 1000, which is wrong for any SFD whose em != 1000 (e.g.
        // 1024 / 2048): outline coordinates are read at the SFD's real em scale, so a
        // stale upm=1000 would leave them unscaled and render the font at the wrong
        // size. Derive upm from the parsed Ascender + Descender metrics (from the SFD
        // Ascent/Descent lines). `desc.abs()` keeps this correct regardless of whether
        // Descent is stored as the SFD's positive value or later negated.
        if let (Some(&asc), Some(&desc)) = (
            self.font.masters[0].metrics.get(&MetricType::Ascender),
            self.font.masters[0].metrics.get(&MetricType::Descender),
        ) {
            let em = asc + desc.abs();
            if em > 0 {
                self.font.upm = em as u16;
            }
        }

        // Now parse glyphs if present
        if let Some(chars) = char_data {
            self.parse_chars(&chars, &master_id)?;
        }

        // An SFD may declare no GlyphClass at all; infer the mark category
        // from the anchors so the glyph carries a usable GDEF class.
        self.infer_mark_categories_from_anchors();

        // Prefer the style the PostScript name states, when it states one.
        //
        // Deriving the master name from the weight class alone loses the
        // slope: an italic SFD may state Weight "Book", so the weight-derived
        // name is "Regular". ItalicAngle, OS2StyleMap and MacStyle are
        // commonly zero or empty in such sources, so the PostScript name
        // ("CreteRound-Italic") is the only field that distinguishes the
        // italic from its regular sibling.
        let style_from_font_name = self
            .font
            .names
            .postscript_name
            .get_default()
            .or_else(|| self.font.names.family_name.get_default())
            .and_then(|n| n.split_once('-').map(|(_, style)| style.to_string()))
            .filter(|style| !style.is_empty())
            .map(|style| space_before_italic(&style));

        // A family named after its own weight is one family, not two.
        //
        // A source may be authored as FamilyName "Elsie Black" / FontName
        // "ElsieBlack-Regular" / Weight "Black" -- taken at face value that
        // declares usWeightClass 900 with subfamily Regular, which is
        // internally inconsistent.
        //
        // Splitting the weight off gives family "Elsie" with style "Black",
        // which is how the same design is modelled in a Glyphs source, and
        // yields the typographic family and subfamily (name IDs 16 and 17)
        // that a non-standard style needs.
        let split_weight_from_family = style_from_font_name
            .as_deref()
            .is_none_or(|style| style == "Regular")
            .then(|| self.weight_suffix_of_family_name())
            .flatten();
        let style_from_font_name = match split_weight_from_family {
            Some((family, weight)) => {
                self.font.names.family_name = family.into();
                Some(weight)
            }
            None => style_from_font_name,
        };

        // Set master name based on width/weight
        if let Some(master) = self.font.masters.get_mut(0) {
            if let Some(style) = style_from_font_name {
                master.name = style.into();
            } else if let Some(weight) = self.font.custom_ot_values.os2_us_weight_class {
                let weight_name = crate::constants::OS2_WEIGHT_TO_NAME_MAP
                    .iter()
                    .find(|(w, _)| *w == weight)
                    .map(|(_, s)| s.to_string())
                    .unwrap_or_else(|| "Regular".to_string());
                master.name = weight_name.into();
            }
            // The width class is NOT folded into the style name. A source that
            // states "TTFWidth: 1" and leaves its style as Regular means the
            // width belongs in usWidthClass, and that is where it now goes.
            // Prepending it produced "UltraCondensed Regular", which the
            // compiler reads as a non-standard style and pushes into the family
            // name -- bowlbyonesc built as family "Bowlby One SC
            // UltraCondensed" against a shipped "Bowlby One SC" that carries
            // the same usWidthClass 1.
        }

        Ok(())
    }

    fn parse_layer_def(&mut self, value: &str) {
        // Expected format: "<idx> <quadratic> \"Name\" <flags>"; we ignore flags
        let tokenized = tokenize_preserving_quotes(value);
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

    fn parse_language_specific_name(&mut self, v: &str) {
        // Format: <language_id> "string0" "string1" "string2" ...
        // Strings are UTF-7 encoded, indices correspond to OpenType Name IDs
        let tokens = tokenize_preserving_quotes(v);
        if tokens.is_empty() {
            return;
        }

        // First token is the language ID
        let lang_id = match tokens[0].parse::<u16>() {
            Ok(id) => id,
            Err(_) => return,
        };

        // Convert OpenType language ID to OT layout tag
        let Some(otl_tag) = ot_lang_id_to_layout_tag(lang_id) else {
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
                name_dict.insert(otl_tag.to_string(), decoded);
            }
        }
    }

    /// Split a family name that ends in its own weight, as (family, weight).
    ///
    /// `None` unless the file states a weight, that weight is a real one
    /// rather than a synonym for Regular, and the family name ends with it
    /// with something left over -- so "Elsie Black"/Black splits and
    /// "Black"/Black does not. The comparison ignores spacing, because the
    /// family name spaces the weight ("Elsie Swash Caps Black") where the
    /// PostScript name does not.
    fn weight_suffix_of_family_name(&self) -> Option<(String, String)> {
        let weight = self
            .font
            .format_specific
            .get("postscript_weight_name")
            .and_then(|v| v.as_str())?
            .trim();
        if weight.is_empty()
            || weight.eq_ignore_ascii_case("Book")
            || weight.eq_ignore_ascii_case("Regular")
            || weight.eq_ignore_ascii_case("Normal")
            || weight.eq_ignore_ascii_case("Medium")
        {
            return None;
        }
        let family = self.font.names.family_name.get_default()?.trim();
        let unspaced = |s: &str| s.replace(' ', "");
        // The shortest suffix that spells the weight; whatever precedes it is
        // the family. Cutting on a character boundary keeps this sound for
        // non-ASCII family names.
        let stem = family
            .char_indices()
            .map(|(ix, _)| ix)
            .find(|&ix| {
                family
                    .get(ix..)
                    .is_some_and(|tail| unspaced(tail).eq_ignore_ascii_case(weight))
            })
            .and_then(|ix| family.get(..ix))?
            .trim_end();
        if stem.is_empty() {
            return None;
        }
        Some((stem.to_string(), weight.to_string()))
    }
}

// ===========================================================================
// 2. Masters, axes and instances
// Master metrics, and the kerning declarations the file states
// (KernClass2 at font level, Kerns2 per glyph).
// ===========================================================================

impl SfdParser {
    /// Resolve FontForge offset-mode OS/2 and hhea vertical metrics.
    ///
    /// When the companion flag is nonzero (`OS2TypoAOffset`, `OS2TypoDOffset`,
    /// `OS2WinAOffset`, `OS2WinDOffset`, `HheadAOffset`, `HheadDOffset`), the
    /// stored metric is a DELTA from FontForge's computed default -- the em
    /// ascent/descent for the typo metrics, the font bounding box for the
    /// win/hhea metrics -- not an absolute value. Left unresolved, the raw
    /// deltas (typically 0 or 1) become absolute metrics and the built font
    /// gets a degenerate line height. This runs after all glyphs are parsed so
    /// the bounding box is available.
    fn resolve_offset_metrics(&mut self) -> Result<(), BabelfontError> {
        let flag = |k: &str| {
            self.font
                .format_specific
                .get(k)
                .and_then(|v| v.as_str())
                .and_then(|s| s.trim().parse::<i32>().ok())
                .unwrap_or(0)
                != 0
        };
        let needs_bbox = flag("OS2WinAOffset")
            || flag("OS2WinDOffset")
            || flag("HheadAOffset")
            || flag("HheadDOffset");
        let (ymin, ymax) = if needs_bbox {
            compute_font_bbox_y(&self.font)?
        } else {
            (0.0, 0.0)
        };
        let ascender = self.font.masters[0]
            .metrics
            .get(&MetricType::Ascender)
            .copied()
            .unwrap_or(0);
        // The Descender metric is already negated at parse time, matching the
        // sign FontForge uses as the typo-descent base.
        let descender = self.font.masters[0]
            .metrics
            .get(&MetricType::Descender)
            .copied()
            .unwrap_or(0);
        let bases = [
            (
                "OS2TypoAOffset",
                "OS2TypoAscent",
                MetricType::TypoAscender,
                ascender,
            ),
            (
                "OS2TypoDOffset",
                "OS2TypoDescent",
                MetricType::TypoDescender,
                descender,
            ),
            // FontForge's head bbox, the base of the win/hhea metrics, is
            // floor(ymin)/ceil(ymax) (tottf.c: gh.ymin = floor(bb.miny);
            // gh.ymax = ceil(bb.maxy)), not the nearest integer.
            (
                "OS2WinAOffset",
                "OS2WinAscent",
                MetricType::WinAscent,
                ymax.ceil() as i32,
            ),
            (
                "OS2WinDOffset",
                "OS2WinDescent",
                MetricType::WinDescent,
                -ymin.floor() as i32,
            ),
            (
                "HheadAOffset",
                "HheadAscent",
                MetricType::HheaAscender,
                ymax.ceil() as i32,
            ),
            (
                "HheadDOffset",
                "HheadDescent",
                MetricType::HheaDescender,
                ymin.floor() as i32,
            ),
        ];
        // Collect flag results first to avoid borrowing format_specific
        // mutably while the `flag` closure has an immutable reference.
        let active_metrics: Vec<(String, MetricType, i32)> = bases
            .iter()
            .filter(|(flag_key, _, _, _)| flag(flag_key))
            .map(|(_, sfd_key, metric, base)| (sfd_key.to_string(), metric.clone(), *base))
            .collect();
        for (sfd_key, metric, base) in &active_metrics {
            if let Some(v) = self.font.masters[0].metrics.get_mut(metric) {
                // Record that this metric was stored as a delta (offset mode)
                // so the emitter can reconstruct the delta from the (potentially
                // modified) absolute metric when writing back to SFD.
                self.font.format_specific.insert(
                    format!("sfd.offset_mode.{}", sfd_key),
                    serde_json::Value::Bool(true),
                );
                *v += base;
            }
        }
        Ok(())
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
        if classstart != 0 {
            // FontForge omits explicit class 0 unless n1 has a '+' suffix.
            // Keep a placeholder so kern matrix row indexing matches SFD semantics.
            groups1.insert(0, Vec::new());
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
            if a.contains('+') {
                classstart = 0;
            }
            n1 = a.trim_matches('+').parse().unwrap_or(0);
        }
        if let Some(b) = it.next() {
            n2 = b.parse().unwrap_or(0);
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

    fn parse_kerns_line(value: &str) -> Vec<(i32, f32, String)> {
        let tokens = tokenize_preserving_quotes(value);
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
}

// ===========================================================================
// 3. Glyphs
// ===========================================================================

impl SfdParser {
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
        // FontForge quotes a glyph name only when it has a character other than
        // printable ASCII, tab, CR or LF, and a quoted name is in its modified UTF-7,
        // like subtable, lookup, class and nameid strings. An unquoted name is
        // literal, `+` included. FontForge can leave whitespace after the closing
        // quote.
        let name_line = name_line.trim();
        let decoded_name = match name_line
            .strip_prefix('"')
            .and_then(|name| name.strip_suffix('"'))
        {
            Some(quoted) => decode_utf7(quoted),
            None => name_line.to_string(),
        };
        let glyph_name = decoded_name.as_str();
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
                        let paths =
                            pathreading::splines_to_path(&section, layer_is_quadratic(layer))?;
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
                        // fontc classifies a glyph as a GDEF mark only when both
                        // category == Mark and subCategory is Nonspacing (or
                        // SpacingCombining). SFD only records the coarse
                        // GlyphClass, so emit an explicit Nonspacing subCategory
                        // for marks; without it no mark2base lookups (and thus no
                        // abvm/blwm) are generated.
                        //
                        // Ligatures need subCategory = Ligature so fontc assigns
                        // them GDEF class 2; otherwise the bare "Ligature"
                        // category is unknown to fontc and its bundled GlyphData
                        // may reclassify some conjuncts as marks, dropping them
                        // from mark2base base coverage.
                        match v {
                            3 => {
                                glyph.format_specific.insert(
                                    "subcategory".to_string(),
                                    serde_json::Value::String("Ligature".to_string()),
                                );
                            }
                            4 => {
                                glyph.format_specific.insert(
                                    "subcategory".to_string(),
                                    serde_json::Value::String("Nonspacing".to_string()),
                                );
                            }
                            _ => {}
                        }
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
                            let paths =
                                pathreading::splines_to_path(&section, layer_is_quadratic(layer))?;
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
                        glyph.format_specific.insert(
                            "sfd.altuni".to_string(),
                            serde_json::Value::String(v.into()),
                        );
                        // FontForge records a glyph's additional Unicode mappings here as
                        // space-separated `uni.vs.reserved` hex triples. When the variation
                        // selector `vs` is 0xffffffff ("none"), the entry is a plain
                        // alternate encoding — the glyph is also mapped at `uni` in the cmap
                        // — so add it to the glyph's codepoints. A real `vs` describes a
                        // Unicode Variation Sequence (cmap format 14), which is not a plain
                        // codepoint, so skip it.
                        for entry in v.split_whitespace() {
                            let mut fields = entry.split('.');
                            let Some(cp) =
                                fields.next().and_then(|s| u32::from_str_radix(s, 16).ok())
                            else {
                                continue;
                            };
                            let is_variation_sequence = fields
                                .next()
                                .and_then(|s| u32::from_str_radix(s, 16).ok())
                                .is_some_and(|vs| vs != 0xffff_ffff);
                            if !is_variation_sequence && !codepoints.contains(&cp) {
                                codepoints.push(cp);
                            }
                        }
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
                        let mut anchor = self
                            .parse_anchor(v)
                            .ok_or(BabelfontError::General("Couldn't parse anchor".to_string()))?;
                        // In SFD, AnchorPoint lines usually appear before the
                        // "Fore"/"Layer" markers, so current_layer_idx is still
                        // None. The anchor belongs to the glyph's foreground
                        // outline, so fall back to the default foreground layer
                        // instead of silently dropping the anchor.
                        let layer_pos = current_layer_idx
                            .and_then(|layer_idx| layer_map.get(&layer_idx).copied())
                            .unwrap_or_else(|| {
                                Self::ensure_default_foreground_layer(
                                    &mut glyph,
                                    &mut layer_map,
                                    width.unwrap_or(0.0),
                                    self.layer_defs.get(1).and_then(|d| d.as_ref()),
                                    master_id,
                                )
                            });
                        // Copy the registered class into format_specific
                        if let Some(decl) = self
                            .anchor_class_decls
                            .iter()
                            .find(|(c, _)| c == &anchor.name)
                        {
                            anchor.format_specific.insert(
                                "sfd.associatedLookup".to_string(),
                                serde_json::Value::String(decl.1.clone()),
                            );
                        }
                        glyph.layers[layer_pos].anchors.push(anchor);
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
                "Substitution2" => {
                    if let Some((subtable_name, glyphs)) = self.parse_oneline_layout(value) {
                        let Some(replacement) = glyphs.first() else {
                            log::error!(
                                "Substitution2 has no replacement glyph for {}",
                                glyph_name
                            );
                            continue;
                        };
                        let Some(subtable) = self.gsub_lookups.find_subtable_mut(&subtable_name)
                        else {
                            log::error!(
                                "Substitution2 references unknown subtable: {}",
                                subtable_name
                            );
                            continue;
                        };
                        subtable.push(fea_rs_ast::Statement::SingleSubst(
                            fea_rs_ast::SingleSubstStatement::new(
                                vec![Self::glyph_container(glyph_name)],
                                vec![Self::glyph_container(replacement)],
                                vec![],
                                vec![],
                                0..0,
                                false,
                            ),
                        ));
                    }
                }
                "AlternateSubs2" => {
                    if let Some((subtable_name, glyphs)) = self.parse_oneline_layout(value) {
                        if glyphs.is_empty() {
                            log::error!("AlternateSubs2 has no replacements for {}", glyph_name);
                            continue;
                        }
                        let Some(subtable) = self.gsub_lookups.find_subtable_mut(&subtable_name)
                        else {
                            log::error!(
                                "AlternateSubs2 references unknown subtable: {}",
                                subtable_name
                            );
                            continue;
                        };
                        let replacement_class =
                            fea_rs_ast::GlyphContainer::GlyphClass(fea_rs_ast::GlyphClass::new(
                                glyphs.iter().map(Self::glyph_container).collect(),
                                0..0,
                            ));
                        subtable.push(fea_rs_ast::Statement::AlternateSubst(
                            fea_rs_ast::AlternateSubstStatement::new(
                                Self::glyph_container(glyph_name),
                                replacement_class,
                                vec![],
                                vec![],
                                0..0,
                                false,
                            ),
                        ));
                    }
                }
                "MultipleSubs2" => {
                    if let Some((subtable_name, glyphs)) = self.parse_oneline_layout(value) {
                        if glyphs.is_empty() {
                            log::error!("MultipleSubs2 has no replacements for {}", glyph_name);
                            continue;
                        }
                        let Some(subtable) = self.gsub_lookups.find_subtable_mut(&subtable_name)
                        else {
                            log::error!(
                                "MultipleSubs2 references unknown subtable: {}",
                                subtable_name
                            );
                            continue;
                        };
                        subtable.push(fea_rs_ast::Statement::MultipleSubst(
                            fea_rs_ast::MultipleSubstStatement::new(
                                Self::glyph_container(glyph_name),
                                glyphs.iter().map(Self::glyph_container).collect(),
                                vec![],
                                vec![],
                                0..0,
                                false,
                            ),
                        ));
                    }
                }
                "Position2" => {
                    if let Some((subtable_name, tokens)) = self.parse_oneline_layout(value) {
                        let Some(vr) = Self::parse_pos_value_record(&tokens) else {
                            log::error!("Position2 has invalid value record for {}", glyph_name);
                            continue;
                        };
                        let Some(subtable) = self.gpos_lookups.find_subtable_mut(&subtable_name)
                        else {
                            log::error!("Position2 references unknown subtable: {}", subtable_name);
                            continue;
                        };
                        subtable.push(fea_rs_ast::Statement::SinglePos(
                            fea_rs_ast::SinglePosStatement::new(
                                vec![],
                                vec![],
                                vec![(Self::glyph_container(glyph_name), Some(vr))],
                                false,
                                0..0,
                            ),
                        ));
                    }
                }
                "PairPos2" => {
                    if let Some((subtable_name, tokens)) = self.parse_oneline_layout(value) {
                        if tokens.len() < 9 {
                            log::error!("PairPos2 has insufficient tokens for {}", glyph_name);
                            continue;
                        }
                        let right = &tokens[0];
                        let Some(vr1) = Self::parse_pos_value_record(&tokens[1..5]) else {
                            log::error!(
                                "PairPos2 first value record is invalid for {}",
                                glyph_name
                            );
                            continue;
                        };
                        let Some(vr2) = Self::parse_pos_value_record(&tokens[5..9]) else {
                            log::error!(
                                "PairPos2 second value record is invalid for {}",
                                glyph_name
                            );
                            continue;
                        };
                        let Some(subtable) = self.gpos_lookups.find_subtable_mut(&subtable_name)
                        else {
                            log::error!("PairPos2 references unknown subtable: {}", subtable_name);
                            continue;
                        };
                        subtable.push(fea_rs_ast::Statement::PairPos(
                            fea_rs_ast::PairPosStatement::new(
                                Self::glyph_container(glyph_name),
                                Self::glyph_container(right),
                                vr1,
                                Some(vr2),
                                false,
                                0..0,
                            ),
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

    fn parse_oneline_layout(&self, value: Option<&str>) -> Option<(SmolStr, Vec<SmolStr>)> {
        if let Some(v) = value {
            // Split quoted "name" component and following glyphs
            let parts: Vec<&str> = v.split('"').collect();
            if parts.len() >= 3 {
                // Decoded, to match the subtable names the Lookup: line declares.
                let name = SmolStr::from(decode_utf7(parts[1]));
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
}

// ===========================================================================
// 4. Layers
// ===========================================================================

pub(crate) fn layer_is_quadratic(layer: &Layer) -> bool {
    layer
        .format_specific
        .get(LAYER_QUADRATIC_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

impl SfdParser {
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
            // Unique within the document, and the same on every run.
            layer.id = Some(format!("babelfont/sfd/layer/{}/{layer_idx}", glyph.name));
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
}

// ===========================================================================
// 5. Anchors and shapes
// ===========================================================================

impl SfdParser {
    fn register_anchor_classes(&mut self, v: &str) {
        let tokens = tokenize_preserving_quotes(v);
        let mut i = 0;
        while i + 1 < tokens.len() {
            let class_name = decode_utf7(tokens[i].trim_matches('"'));
            let subtable = tokens[i + 1].trim_matches('"').to_lowercase();
            i += 2;
            if class_name.is_empty()
                || self
                    .anchor_class_decls
                    .iter()
                    .any(|(c, _)| c == &class_name)
            {
                continue;
            }
            self.anchor_class_decls.push((class_name, subtable));
        }
    }

    fn parse_anchor(&self, v: &str) -> Option<crate::Anchor> {
        // Quoted name, x, y, kind, index
        let parts: Vec<&str> = v.split_whitespace().collect();
        if parts.len() < 5 {
            return None;
        }
        let mut name = decode_utf7(parts[0].trim_matches('"'));
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
        if kind == "mark" {
            name = "_".to_string() + &name;
        }
        Some(crate::Anchor {
            name,
            x,
            y,
            format_specific,
        })
    }

    /// Classify GlyphClass-less glyphs that carry only mark-side anchors as
    /// Nonspacing marks.
    ///
    /// SFD records a glyph's role in two ways: the coarse `GlyphClass` line and,
    /// per anchor, the attachment `kind` (`mark` = the glyph attaches AS a mark,
    /// `basechar`/`baselig`/... = base side). Some sources (e.g. Glegoo Bold)
    /// omit `GlyphClass` on conjunct marks, leaving their category Unknown. Such
    /// a glyph then exports without a category, and fontc — seeing an
    /// underscore-joined name — treats it as a ligature and drops it from the
    /// abvm/blwm mark coverage. The anchor `kind` is the authoritative signal:
    /// a glyph whose anchors are exclusively mark-side is a mark, so mirror the
    /// `GlyphClass: 4` path (category = Mark, subCategory = Nonspacing).
    fn infer_mark_categories_from_anchors(&mut self) {
        for glyph in self.font.glyphs.0.iter_mut() {
            if glyph.category != GlyphCategory::Unknown {
                continue;
            }
            let mut has_mark_anchor = false;
            let mut has_base_anchor = false;
            for layer in glyph.layers.iter() {
                for anchor in layer.anchors.iter() {
                    match anchor
                        .format_specific
                        .get("sfd.kind")
                        .and_then(|v| v.as_str())
                    {
                        Some("mark") => has_mark_anchor = true,
                        Some(_) => has_base_anchor = true,
                        None => {}
                    }
                }
            }
            if has_mark_anchor && !has_base_anchor {
                glyph.category = GlyphCategory::Mark;
                glyph.format_specific.insert(
                    "subcategory".to_string(),
                    serde_json::Value::String("Nonspacing".to_string()),
                );
            }
        }
    }
}

// ===========================================================================
// 6. Paths and components
// Paths themselves are read and written in `pathreading`; these are the
// glyph-level component references that point at them.
// ===========================================================================

impl SfdParser {
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
}

// ===========================================================================
// 7. Font-level kerning groups
// Folds the parsed classes and pairs into `Master::kerning` and the
// font-wide `@group` definitions.
// ===========================================================================

impl SfdParser {
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
}

// ===========================================================================
// 8. OpenType features
// ===========================================================================

/// Is this FEA line a rule that may appear directly inside `aalt`?
///
/// The spec allows only feature references and single or alternate substitutions
/// there. A single sub is `sub <glyph> by <glyph>;` and an alternate sub is
/// `sub <glyph> from [<glyphs>];`. Anything else -- ligatures, multiples,
/// contextual rules -- has to stay in its own lookup and out of `aalt`.
fn is_single_or_alternate_sub(line: &str) -> bool {
    let line = line.trim();
    let Some(rest) = line
        .strip_prefix("sub ")
        .or_else(|| line.strip_prefix("substitute "))
    else {
        return false;
    };
    if rest.contains(" from ") {
        // Alternate substitution.
        return true;
    }
    let Some((from, to)) = rest.split_once(" by ") else {
        return false;
    };
    // Single substitution: exactly one glyph on each side, and no class or
    // sequence syntax that would make it something else.
    let one_glyph = |part: &str| {
        let part = part.trim().trim_end_matches(';').trim();
        !part.is_empty()
            && !part.contains('[')
            && !part.contains(']')
            && !part.contains('\'')
            // A glyph class expands to several rules; keep aalt to plain glyphs.
            && !part.starts_with('@')
            && part.split_whitespace().count() == 1
    };
    one_glyph(from) && one_glyph(to)
}

impl SfdParser {
    fn parse_lookup(&mut self, data: &str) {
        // Format per fontforge.md:
        // Lookup: <kind> <flags> <save-afm> "<lookup name>" { ...subtables... } [ ...features/scripts/languages... ]
        let head_end = data.find('"').unwrap_or(data.len());
        let head = data[..head_end].trim();
        let mut it = head.split_whitespace();
        let kind: u16 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let flag: u16 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        // let _save_afm: u16 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);

        // Lookup name between quotes. Decoded, because SeqLookup references to it are
        // decoded too, and a name carrying a UTF-7 escape has to match on both sides.
        let name = if let Some(start) = data.find('"') {
            if let Some(end) = data[start + 1..].find('"') {
                decode_utf7(&data[start + 1..start + 1 + end])
            } else {
                String::new()
            }
        } else {
            String::new()
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
            Self::sanitize_and_dedupe_lookup_name(&name, &mut self.taken_lookup_names);
        if let Some(previous) = self
            .assigned_lookup_names
            .insert(name.clone(), sanitized_name.clone())
        {
            // Two SFD lookups with the same name: references can only mean one of
            // them, and they now mean this one.
            log::warn!(
                "two lookups are both named {name:?}; references resolve to the \
                 later one ({sanitized_name}), not {previous}"
            );
        }
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
        let tokens = tokenize_preserving_quotes(body);
        tokens
            .into_iter()
            .filter(|t| t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
            // Decoded: chain_pos_sub is keyed by the decoded subtable name, so a name
            // carrying a UTF-7 escape would otherwise never match its rules.
            .map(|t| SmolStr::from(decode_utf7(t.trim_matches('"'))))
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
                                                        // FontForge writes a
                                                        // blank script tag for
                                                        // lookups with no
                                                        // script; treat it as
                                                        // DFLT/dflt so the FEA
                                                        // stays valid.
                                                        let script = if script.trim().is_empty() {
                                                            "DFLT"
                                                        } else {
                                                            script
                                                        };
                                                        let language = if language.trim().is_empty()
                                                        {
                                                            "dflt"
                                                        } else {
                                                            language
                                                        };
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

    /// Sanitize a lookup name for FEA and make it unique: the first taker keeps
    /// the bare form, later ones get `_2`, `_3`, ... Every returned name goes into
    /// `taken`, generated ones included, so a later lookup whose own name happens
    /// to sanitize to an already-generated form cannot collide with it.
    fn sanitize_and_dedupe_lookup_name(name: &str, taken: &mut HashSet<String>) -> String {
        let mut sanitized: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        // A label may not begin with a digit, and a label that IS a keyword fails
        // to parse wherever it stands ("Expected LABEL found SubKw"); either one
        // aborts the whole conversion downstream. The list mirrors FEA_KEYWORDS in
        // fea-rs-ast 0.1.6 (glyphcontainers.rs), which that crate keeps private.
        const FEA_KEYWORDS: [&str; 52] = [
            "anchor",
            "anchordef",
            "anon",
            "anonymous",
            "by",
            "contour",
            "cursive",
            "device",
            "enum",
            "enumerate",
            "excludedflt",
            "exclude_dflt",
            "feature",
            "from",
            "ignore",
            "ignorebaseglyphs",
            "ignoreligatures",
            "ignoremarks",
            "include",
            "includedflt",
            "include_dflt",
            "language",
            "languagesystem",
            "lookup",
            "lookupflag",
            "mark",
            "markattachmenttype",
            "markclass",
            "nameid",
            "null",
            "parameters",
            "pos",
            "position",
            "required",
            "righttoleft",
            "reversesub",
            "rsub",
            "script",
            "sub",
            "substitute",
            "subtable",
            "table",
            "usemarkfilteringset",
            "useextension",
            "valuerecorddef",
            "base",
            "gdef",
            "head",
            "hhea",
            "name",
            "vhea",
            "vmtx",
        ];
        if sanitized.starts_with(|c: char| c.is_ascii_digit()) {
            sanitized.insert(0, '_');
        }
        if FEA_KEYWORDS.contains(&sanitized.as_str()) {
            sanitized.push('_');
        }
        if taken.insert(sanitized.clone()) {
            return sanitized;
        }
        let mut n = 2usize;
        loop {
            let candidate = format!("{sanitized}_{n}");
            if taken.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }

    fn parse_chain_pos_sub(&mut self, lkey: &str, data: &[String]) {
        // Python _parseChainPosSub equivalent
        let possub: Vec<&str> = data.iter().map(|l| l.trim()).collect();
        if possub.is_empty() {
            return;
        }

        let Some(caps) = CHAIN_POSSUB_RE.captures(possub[0]) else {
            log::error!("Failed to parse ChainPosSub header: {}", possub[0]);
            return;
        };

        let kind = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let subtable = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let count_at = |group: usize| -> usize {
            caps.get(group)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0)
        };
        let declared = layout::FpstHeaderCounts {
            classes: count_at(3),
            backtrack_classes: count_at(4),
            lookahead_classes: count_at(5),
            rules: count_at(6),
        };

        let subtable = decode_utf7(subtable);

        let rule_kind = match lkey {
            "ChainSub2" | "ContextSub2" => layout::RuleKind::Sub,
            "ChainPos2" | "ContextPos2" => layout::RuleKind::Pos,
            // Reverse chaining substitutes inline from a replacement list and is
            // spelled `rsub ... by ...`. Neither is modelled here, and a forward
            // `sub` carrying no action is an `ignore`, which suppresses the
            // substitution the section asked for.
            "ReverseChain2" => {
                log::warn!(
                    "reverse chaining lookup {subtable:?} is not supported; its rules are dropped"
                );
                return;
            }
            _ => return,
        };

        let Some(sfd_kind) = layout::SfdKind::parse(kind) else {
            log::warn!("Unhandled FPST kind {kind:?} in subtable {subtable:?}; its rules are lost");
            return;
        };

        if sfd_kind == layout::SfdKind::Class {
            let entries = Self::parse_class_fpst(&possub[1..], rule_kind, declared, &subtable);
            if !entries.is_empty() {
                self.chain_pos_sub
                    .entry(subtable.to_string())
                    .or_default()
                    .extend(entries);
            }
            return;
        }

        // A rule opens differently per kind: a coverage rule with a bare
        // `<ninput> <nbacktrack> <nlookahead>` line, a glyph rule with its `String:`
        // line. Everything until the next opener belongs to the current rule, so a
        // section holds as many rules as its header declares -- reading the whole
        // body as one rule concatenates them into nonsense.
        let mut entries: Vec<layout::ChainPosSubEntry> = Vec::new();
        let mut cur: Option<layout::ChainPosSubEntry> = None;
        let open = |entries: &mut Vec<layout::ChainPosSubEntry>,
                    cur: &mut Option<layout::ChainPosSubEntry>| {
            if let Some(e) = cur.take() {
                entries.push(e);
            }
            *cur = Some(layout::ChainPosSubEntry {
                kind: rule_kind,
                sfd_kind,
                matches: Vec::new(),
                backtracks: Vec::new(),
                lookaheads: Vec::new(),
                lookups: IndexMap::new(),
            });
        };
        // `<key>: <count> <glyph> ...` -- the count is a byte length, not a glyph
        // count, so it is dropped rather than trusted.
        let glyphs_of = |val: &str| -> Vec<String> {
            val.split_whitespace().skip(1).map(String::from).collect()
        };

        for line in possub[1..].iter() {
            let Some(colon_pos) = line.find(": ") else {
                let ints: Vec<&str> = line.split_whitespace().collect();
                if ints.len() == 3 && ints.iter().all(|n| n.parse::<usize>().is_ok()) {
                    open(&mut entries, &mut cur);
                }
                // A bare single integer is the SeqLookup count; the SeqLookup lines
                // carry their own indices, so there is nothing to record.
                continue;
            };
            let key_part = &line[..colon_pos];
            let val_part = &line[colon_pos + 2..];

            match key_part {
                "Coverage" => {
                    if let Some(e) = cur.as_mut() {
                        e.matches
                            .push(layout::GlyphGroup::Glyphs(glyphs_of(val_part)));
                    }
                }
                "String" => {
                    open(&mut entries, &mut cur);
                    if let Some(e) = cur.as_mut() {
                        e.matches
                            .push(layout::GlyphGroup::Glyphs(glyphs_of(val_part)));
                    }
                }
                "BCoverage" | "FCoverage" => {
                    // An absent context is no constraint: not a position at all.
                    let glyphs = glyphs_of(val_part);
                    if glyphs.is_empty() {
                        continue;
                    }
                    if let Some(e) = cur.as_mut() {
                        if key_part == "BCoverage" {
                            e.backtracks.push(layout::GlyphGroup::Glyphs(glyphs));
                        } else {
                            e.lookaheads.push(layout::GlyphGroup::Glyphs(glyphs));
                        }
                    }
                }
                "BString" | "FString" => {
                    // A glyph-kind section spells its context one glyph per position,
                    // exactly as it spells the input. An absent context is no
                    // constraint: not a position at all.
                    if let Some(e) = cur.as_mut() {
                        let groups = val_part
                            .split_whitespace()
                            .skip(1)
                            .map(|g| layout::GlyphGroup::Glyphs(vec![g.to_string()]));
                        if key_part == "BString" {
                            e.backtracks.extend(groups);
                        } else {
                            e.lookaheads.extend(groups);
                        }
                    }
                }
                "SeqLookup" => {
                    // Format: "SeqLookup: <index> "<lookup name>""
                    let trimmed = val_part.trim();
                    if let Some(space) = trimmed.find(' ') {
                        let index_str = &trimmed[..space];
                        let lookup_name = trimmed[space + 1..].trim().trim_matches('"');
                        if let (Ok(index), Some(e)) = (index_str.parse::<usize>(), cur.as_mut()) {
                            e.lookups
                                .entry(index)
                                .or_default()
                                .push(decode_utf7(lookup_name));
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(e) = cur.take() {
            entries.push(e);
        }

        let body = layout::FpstBodyCounts {
            classes: 0,
            backtrack_classes: 0,
            lookahead_classes: 0,
            rules: entries.len(),
        };
        for mismatch in declared.mismatches(&body) {
            log::warn!("FPST {subtable:?} {mismatch}");
        }

        if !entries.is_empty() {
            self.chain_pos_sub
                .entry(subtable.to_string())
                .or_default()
                .extend(entries);
        }
    }

    /// Parse the body of a `class`-kind FPST section (ContextSub2/ChainSub2/...).
    ///
    /// Unlike the `coverage` and `glyph` kinds, a class section defines glyph classes
    /// once and then lists several rules that reference them by index:
    ///
    /// ```text
    /// ContextSub2: class "ccmp subtable"  4 0 0 4
    ///   Class: 11 i j uni0268                 <- class 1 (the leading number is a
    ///   Class: 153 acutecomb uni0302 ...         string length, not a glyph count)
    ///   Class: 68 dotbelowcomb ...
    ///  2 0 0                                  <- rule: 2 input, 0 backtrack, 0 lookahead
    ///   ClsList: 1 2
    ///   BClsList:
    ///   FClsList:
    ///  1                                      <- one SeqLookup follows
    ///   SeqLookup: 0 "Single Substitution lookup 1"
    ///   ...
    ///   ClassNames: "0" "1" "2" "3"
    /// ```
    ///
    /// Class 0 is implicit and never written out, so index `n` is the `n`-th `Class:`
    /// line and index 0 means "any glyph in none of them", kept as
    /// [`layout::GlyphGroup::AllOthers`] until the glyph list is known.
    ///
    /// Backtrack classes are stored in feature-file order, farthest from the input
    /// first. FontForge's compiler applies the OpenType reversal; the SFD does not.
    fn parse_class_fpst(
        lines: &[&str],
        rule_kind: layout::RuleKind,
        declared: layout::FpstHeaderCounts,
        subtable: &str,
    ) -> Vec<layout::ChainPosSubEntry> {
        let mut classes: Vec<Vec<String>> = Vec::new();
        let mut bclasses: Vec<Vec<String>> = Vec::new();
        let mut fclasses: Vec<Vec<String>> = Vec::new();
        let mut entries: Vec<layout::ChainPosSubEntry> = Vec::new();

        // The rule currently being accumulated.
        let mut cur: Option<layout::ChainPosSubEntry> = None;

        // `Class: <len> <glyph> <glyph> ...` -- drop the leading length.
        let glyphs_of = |val: &str| -> Vec<String> {
            val.split_whitespace().skip(1).map(String::from).collect()
        };
        // `ClsList: 1 3 2` -> the glyph groups for classes 1, 3, 2.
        let resolve = |val: &str, defs: &[Vec<String>]| -> Vec<layout::GlyphGroup> {
            val.split_whitespace()
                .map(|t| match t.parse::<usize>() {
                    // Class 0 is implicit: every glyph no sibling class claims.
                    Ok(0) => layout::GlyphGroup::AllOthers(defs.to_vec()),
                    Ok(i) if i <= defs.len() => layout::GlyphGroup::Glyphs(defs[i - 1].clone()),
                    // Keep the position rather than closing the gap: SeqLookup indices
                    // count positions, so dropping one here would move a lookup onto the
                    // wrong glyph. An empty group makes the emitter discard the rule.
                    _ => {
                        log::warn!(
                            "FPST class index {t:?} does not name one of the {} declared \
                             classes; the rule it appears in is dropped",
                            defs.len()
                        );
                        layout::GlyphGroup::Glyphs(Vec::new())
                    }
                })
                .collect()
        };

        for line in lines {
            if let Some((key, val)) = line
                .split_once(": ")
                .or(line.strip_suffix(':').map(|k| (k, "")))
            {
                match key {
                    "Class" => classes.push(glyphs_of(val)),
                    "BClass" => bclasses.push(glyphs_of(val)),
                    "FClass" => fclasses.push(glyphs_of(val)),
                    "ClsList" => {
                        if let Some(e) = cur.as_mut() {
                            e.matches = resolve(val, &classes);
                        }
                    }
                    "BClsList" => {
                        if let Some(e) = cur.as_mut() {
                            e.backtracks = resolve(val, &bclasses);
                        }
                    }
                    "FClsList" => {
                        if let Some(e) = cur.as_mut() {
                            e.lookaheads = resolve(val, &fclasses);
                        }
                    }
                    "SeqLookup" => {
                        if let Some(e) = cur.as_mut() {
                            let trimmed = val.trim();
                            if let Some(space) = trimmed.find(' ') {
                                if let Ok(index) = trimmed[..space].parse::<usize>() {
                                    let name = trimmed[space + 1..].trim().trim_matches('"');
                                    e.lookups
                                        .entry(index)
                                        .or_default()
                                        .push(decode_utf7(name).to_string());
                                }
                            }
                        }
                    }
                    _ => {} // ClassNames / BClassNames / FClassNames
                }
                continue;
            }

            // A bare `<ninput> <nbacktrack> <nlookahead>` line opens the next rule.
            let nums: Vec<&str> = line.split_whitespace().collect();
            if nums.len() == 3 && nums.iter().all(|n| n.parse::<usize>().is_ok()) {
                if let Some(e) = cur.take() {
                    entries.push(e);
                }
                cur = Some(layout::ChainPosSubEntry {
                    kind: rule_kind,
                    sfd_kind: layout::SfdKind::Class,
                    matches: Vec::new(),
                    backtracks: Vec::new(),
                    lookaheads: Vec::new(),
                    lookups: IndexMap::new(),
                });
            }
            // A bare single integer is the SeqLookup count. A count of zero makes
            // the rule an `ignore`, which the emitter reads back off the empty
            // lookup map, so there is nothing to store here.
        }
        if let Some(e) = cur.take() {
            entries.push(e);
        }
        let body = layout::FpstBodyCounts {
            classes: classes.len(),
            backtrack_classes: bclasses.len(),
            lookahead_classes: fclasses.len(),
            rules: entries.len(),
        };
        for mismatch in declared.mismatches(&body) {
            log::warn!("FPST {subtable:?} {mismatch}");
        }

        // A rule with no input sequence cannot be expressed; drop it rather than
        // emitting invalid FEA.
        let before = entries.len();
        entries.retain(|e| !e.matches.is_empty());
        if entries.len() < before {
            log::warn!(
                "dropped {} class FPST rule(s) with an empty input sequence",
                before - entries.len()
            );
        }
        entries
    }

    /// Turn one chain/context rule into a feature statement:
    ///
    ///   [ignore] sub <backtrack> <input>' lookup <Name> <lookahead>;
    ///
    /// The rule is validated before any statement is built or any class is named,
    /// so a rule that cannot be expressed leaves nothing behind -- no orphan
    /// `@class` definitions, no references to lookups the file never defines.
    /// `None` means the rule was dropped, with one warning stating the reason.
    fn make_chain_context_statement(
        entry: &layout::ChainPosSubEntry,
        assigned_lookup_names: &HashMap<String, String>,
        emitted_lookups: &HashSet<String>,
        all_glyphs: &[String],
        known_glyphs: &HashSet<&str>,
        lookup_name: &str,
        namer: &mut layout::ClassNamer,
    ) -> Option<fea_rs_ast::Statement> {
        // Resolve every position to concrete glyphs. An SFD keeps the class lists of
        // glyphs that were later deleted, and a name the compiler cannot resolve must
        // not reach the feature file.
        let mut missing: Vec<String> = Vec::new();
        let mut resolve = |groups: &[layout::GlyphGroup]| -> Vec<Vec<String>> {
            groups
                .iter()
                .map(|g| {
                    let (kept, gone): (Vec<String>, Vec<String>) = g
                        .resolve(all_glyphs)
                        .into_iter()
                        .partition(|n| known_glyphs.contains(n.as_str()));
                    missing.extend(gone);
                    kept
                })
                .collect()
        };
        let matches = resolve(&entry.matches);
        let backtracks = resolve(&entry.backtracks);
        let lookaheads = resolve(&entry.lookaheads);

        if !missing.is_empty() {
            // A class or coverage position is a set, so it stands without the absent
            // names. A glyph-kind position is a single glyph, and a position that
            // SeqLookup indices count, so the rule cannot survive losing one.
            if entry.sfd_kind == layout::SfdKind::Glyph {
                log::warn!(
                    "lookup {lookup_name:?}: a contextual rule names glyphs the font does \
                     not have ({}); the rule is dropped",
                    missing.join(", ")
                );
                return None;
            }
            log::warn!(
                "lookup {lookup_name:?}: glyphs the font does not have removed from a \
                 contextual rule: {}",
                missing.join(", ")
            );
        }
        if matches.is_empty() {
            log::warn!("lookup {lookup_name:?}: a contextual rule has no input; it is dropped");
            return None;
        }
        if [&matches, &backtracks, &lookaheads]
            .iter()
            .any(|groups| groups.iter().any(|g| g.is_empty()))
        {
            // No glyph can occupy the position, so the rule can never fire. Leaving
            // the position out would silently widen the rule instead.
            log::warn!(
                "lookup {lookup_name:?}: a contextual rule has a position no glyph can \
                 occupy, so it can never fire; it is dropped"
            );
            return None;
        }

        // Resolve every lookup call against the lookups the feature file will
        // actually define, so a reference can never dangle.
        let position_count = match entry.sfd_kind {
            layout::SfdKind::Glyph => matches.iter().map(Vec::len).sum(),
            layout::SfdKind::Coverage | layout::SfdKind::Class => matches.len(),
        };
        let mut refs: IndexMap<usize, Vec<SmolStr>> = IndexMap::new();
        for (&index, names) in &entry.lookups {
            if index >= position_count {
                log::warn!(
                    "lookup {lookup_name:?}: SeqLookup index {index} is past the last \
                     input position of its rule; the rule is dropped"
                );
                return None;
            }
            for name in names {
                let Some(assigned) = assigned_lookup_names.get(name) else {
                    log::warn!(
                        "lookup {lookup_name:?}: a contextual rule calls {name:?}, which \
                         the font never defines; the rule is dropped"
                    );
                    return None;
                };
                if !emitted_lookups.contains(assigned) {
                    // The lookup exists but is empty, so it is never written out.
                    // Calling it substitutes nothing, yet the match still stops later
                    // rules at this position -- which is what `ignore` says, and what
                    // this rule becomes if no other call survives.
                    log::warn!(
                        "lookup {lookup_name:?}: a contextual rule calls {name:?}, which \
                         is empty; the call is dropped"
                    );
                    continue;
                }
                refs.entry(index).or_default().push(SmolStr::from(assigned));
            }
        }

        // Everything is validated: build the statement. Nothing below may fail.
        //
        // The kinds disagree about backtrack order. A class section lists BClsList in
        // feature-file order, farthest from the input first, which is what we want. A
        // coverage or glyph section stores BCoverage/BString the way OpenType does,
        // nearest the input first, so it has to be turned round.
        let backtracks: Vec<Vec<String>> = match entry.sfd_kind {
            layout::SfdKind::Class => backtracks,
            layout::SfdKind::Coverage | layout::SfdKind::Glyph => {
                backtracks.into_iter().rev().collect()
            }
        };

        // A class-kind section declares its classes once, so name them and refer to
        // them; the other kinds list their glyphs inline.
        let container =
            |glyphs: &[String], namer: &mut layout::ClassNamer| -> fea_rs_ast::GlyphContainer {
                match entry.sfd_kind {
                    layout::SfdKind::Class => namer.reference(glyphs, lookup_name),
                    layout::SfdKind::Coverage | layout::SfdKind::Glyph => {
                        if let [only] = glyphs {
                            Self::glyph_container(only)
                        } else {
                            fea_rs_ast::GlyphContainer::GlyphClass(fea_rs_ast::GlyphClass::new(
                                glyphs.iter().map(Self::glyph_container).collect(),
                                0..0,
                            ))
                        }
                    }
                }
            };
        let prefix: Vec<_> = backtracks.iter().map(|g| container(g, namer)).collect();
        let suffix: Vec<_> = lookaheads.iter().map(|g| container(g, namer)).collect();
        // Each input position is one container; in a glyph-kind rule each glyph of
        // the sequence is its own position, which is what SeqLookup indices count.
        let glyphs: Vec<fea_rs_ast::GlyphContainer> = match entry.sfd_kind {
            layout::SfdKind::Glyph => matches
                .iter()
                .flatten()
                .map(Self::glyph_container)
                .collect(),
            layout::SfdKind::Coverage | layout::SfdKind::Class => {
                matches.iter().map(|g| container(g, namer)).collect()
            }
        };

        // A rule that calls no lookup matches only to stop a later, broader rule
        // from firing. That is `ignore` in a feature file, not a rule with no action.
        if refs.is_empty() {
            let contexts = vec![(prefix, glyphs, suffix)];
            return Some(match entry.kind {
                layout::RuleKind::Sub => fea_rs_ast::Statement::IgnoreSubst(
                    fea_rs_ast::IgnoreStatement::new(contexts, 0..0, fea_rs_ast::Subst),
                ),
                layout::RuleKind::Pos => fea_rs_ast::Statement::IgnorePos(
                    fea_rs_ast::IgnoreStatement::new(contexts, 0..0, fea_rs_ast::Pos),
                ),
            });
        }

        let lookups: Vec<Vec<SmolStr>> = (0..glyphs.len())
            .map(|i| refs.get(&i).cloned().unwrap_or_default())
            .collect();
        Some(match entry.kind {
            layout::RuleKind::Sub => fea_rs_ast::Statement::ChainedContextSubst(
                fea_rs_ast::ChainedContextStatement::new(
                    glyphs,
                    prefix,
                    suffix,
                    lookups,
                    0..0,
                    fea_rs_ast::Subst,
                ),
            ),
            layout::RuleKind::Pos => {
                fea_rs_ast::Statement::ChainedContextPos(fea_rs_ast::ChainedContextStatement::new(
                    glyphs,
                    prefix,
                    suffix,
                    lookups,
                    0..0,
                    fea_rs_ast::Pos,
                ))
            }
        })
    }

    fn insert_gtables(&mut self) {
        // Needed to expand FontForge's implicit "All_Others" class (class 0), which is
        // every glyph not named by a sibling class. Glyphs are fully parsed by now.
        let all_glyphs: Vec<String> = self
            .font
            .glyphs
            .0
            .iter()
            .map(|g| g.name.to_string())
            .collect();
        let known_glyphs: HashSet<&str> = all_glyphs.iter().map(String::as_str).collect();
        // The lookups written out so far, in their assigned names. A chain rule may
        // only call a lookup that is already defined above it in the feature file,
        // so its references are checked against this.
        let mut emitted_lookups: HashSet<String> = HashSet::new();

        // The lookups a chain lookup's rules call, by the caller's assigned name.
        // Needed below to hoist a dependency defined after its caller.
        let mut deps_by_lookup: HashMap<String, Vec<String>> = HashMap::new();
        for (name, lookup) in self.gsub_lookups.0.iter().chain(self.gpos_lookups.0.iter()) {
            let deps: Vec<String> = lookup
                .subtables
                .keys()
                .filter_map(|sub| self.chain_pos_sub.get(sub.as_str()))
                .flatten()
                .flat_map(|entry| entry.lookups.values().flatten())
                .filter_map(|n| self.assigned_lookup_names.get(n).cloned())
                .collect();
            if !deps.is_empty() {
                deps_by_lookup.insert(name.clone(), deps);
            }
        }

        let mut is_chain: HashMap<String, bool> = HashMap::new();
        for (name, lookup) in self.gsub_lookups.0.iter().chain(self.gpos_lookups.0.iter()) {
            let has_chain = lookup
                .subtables
                .keys()
                .any(|s| self.chain_pos_sub.contains_key(s.as_str()));
            is_chain.insert(name.clone(), has_chain);
        }

        // Definition order is application order: the compiled font applies lookups
        // by their LookupList index, which follows the order they are defined here,
        // and the feature block's reference order is discarded at compile time. So
        // lookups are defined in the order the SFD declared them, with exactly one
        // deviation: a lookup that a chain rule calls must already be defined when
        // the chain is, so a dependency declared after its caller is hoisted to
        // just before it. The names come from the lookup tables, which are ordered
        // maps -- an unordered source here made conversion nondeterministic once.
        let mut seen_names = HashSet::new();
        let declaration: Vec<String> = self
            .gsub_lookups
            .0
            .keys()
            .chain(self.gpos_lookups.0.keys())
            .filter(|name| seen_names.insert((*name).clone()))
            .cloned()
            .collect();
        let mut ordered_names: Vec<String> = Vec::new();
        let mut placed: HashSet<String> = HashSet::new();
        fn place(
            name: &str,
            deps_by_lookup: &HashMap<String, Vec<String>>,
            placed: &mut HashSet<String>,
            out: &mut Vec<String>,
        ) {
            if !placed.insert(name.to_string()) {
                return;
            }
            for dep in deps_by_lookup.get(name).into_iter().flatten() {
                place(dep, deps_by_lookup, placed, out);
            }
            out.push(name.to_string());
        }
        for name in &declaration {
            place(name, &deps_by_lookup, &mut placed, &mut ordered_names);
        }

        // The `aalt` feature may only contain feature references and single or
        // alternate substitution rules -- a lookup reference is a spec error and
        // the compiler refuses the font. Keep each lookup's inlinable rules so
        // `aalt` can carry them directly instead of pointing at a lookup.
        let mut inlinable_rules: HashMap<SmolStr, Vec<String>> = HashMap::new();
        // Ordered, because the features are emitted by iterating this. A
        // HashMap here shuffles the feature blocks on every run: two
        // conversions of one unchanged .sfd emitted `subs`, `calt`, `liga` in
        // different orders and produced different binaries.
        let mut feature_map: IndexMap<SmolStr, Vec<(layout::FeatureLangSys, SmolStr)>> =
            IndexMap::new();
        let mut used_script_language_pairs = HashSet::new();

        for name in &ordered_names {
            // Look up in GSUB first, then GPOS
            let lookup = if let Some(l) = self.gsub_lookups.0.get_mut(name) {
                l
            } else if let Some(l) = self.gpos_lookups.0.get_mut(name) {
                l
            } else {
                continue;
            };

            // Anchor-based GPOS mark/cursive lookups (mark-to-base,
            // mark-to-mark, mark-to-ligature, cursive) carry no FEA rules in
            // FontForge: their attachment data lives entirely in the glyph
            // AnchorClass/AnchorPoint entries (now converted to Glyphs anchors).
            // Emitting them here would produce EMPTY `feature abvm/blwm/mark/...`
            // blocks in the exported source, and fontc skips auto-generating any
            // feature that is already declared in the FEA (without an insertion
            // marker) — which would suppress the anchor-driven mark features
            // entirely. Skip them so fontc rebuilds abvm/blwm/mark/mkmk/curs
            // from the anchors.
            if matches!(
                lookup.lookup_type,
                layout::LookupType::MarkToBasePosition
                    | layout::LookupType::MarkToMarkPosition
                    | layout::LookupType::MarkToLigaturePosition
                    | layout::LookupType::CursivePosition
            ) {
                continue;
            }
            // Populate the block with code from the subtables
            lookup.block.statements.extend(
                lookup
                    .subtables
                    .iter()
                    .flat_map(|(_sub_name, st)| st.iter())
                    .cloned(),
            );

            let has_chain = is_chain.get(name).copied().unwrap_or(false);

            if has_chain {
                // Generate the chain/context statements from the parsed data. They
                // join the block like any other lookup's statements, so the one path
                // below writes every kind of lookup out.
                let mut namer = layout::ClassNamer::default();
                let rules: Vec<fea_rs_ast::Statement> = lookup
                    .subtables
                    .keys()
                    .filter_map(|sub_name| self.chain_pos_sub.get(sub_name.as_str()))
                    .flatten()
                    .filter_map(|entry| {
                        Self::make_chain_context_statement(
                            entry,
                            &self.assigned_lookup_names,
                            &emitted_lookups,
                            &all_glyphs,
                            &known_glyphs,
                            &lookup.block.name,
                            &mut namer,
                        )
                    })
                    .collect();
                // Every rule was dropped: leave the block empty, so the shared skip
                // below keeps the feature registration from referencing a lookup
                // that is never defined.
                if !rules.is_empty() {
                    // Only the four low bits have feature-file names; the high byte
                    // is a mark-attachment class this convertor does not model yet,
                    // and asserting `lookupflag 0` for it would claim the opposite
                    // of what the SFD said.
                    if lookup.flag & !0x000F != 0 {
                        log::warn!(
                            "lookup {:?}: flag {:#06x} carries bits (mark-attachment \
                             class or filtering set) that are not converted",
                            lookup.block.name,
                            lookup.flag
                        );
                    }
                    if lookup.flag & 0x000F != 0 {
                        lookup
                            .block
                            .statements
                            .push(fea_rs_ast::Statement::LookupFlag(
                                fea_rs_ast::LookupFlagStatement::new(
                                    lookup.flag & 0x000F,
                                    None,
                                    None,
                                    0..0,
                                ),
                            ));
                    }
                    lookup.block.statements.extend(namer.definitions());
                    lookup.block.statements.extend(rules);
                }
            }
            if lookup.block.statements.is_empty() {
                // No statements: skip this lookup
                continue;
            }
            emitted_lookups.insert(name.clone());
            self.font.features.prefixes.insert(
                SmolStr::from(name.as_str()),
                crate::features::PossiblyAutomaticCode {
                    code: lookup.block.as_fea(""),
                    ..Default::default()
                },
            );

            inlinable_rules.insert(
                SmolStr::from(lookup.block.name.as_str()),
                lookup
                    .block
                    .statements
                    .iter()
                    .map(|st| st.as_fea("").trim().to_string())
                    .filter(|line| is_single_or_alternate_sub(line))
                    .collect(),
            );
        }

        // Register each emitted lookup with its features in declaration order. The
        // compiled font ignores this order -- it applies lookups by LookupList
        // index, arranged above -- but the feature file reads best when both agree,
        // and FontForge's own export writes it this way.
        let mut seen = HashSet::new();
        let declaration_order: Vec<String> = self
            .gsub_lookups
            .0
            .keys()
            .chain(self.gpos_lookups.0.keys())
            .filter(|name| seen.insert((*name).clone()))
            .cloned()
            .collect();
        for name in &declaration_order {
            if !emitted_lookups.contains(name) {
                continue;
            }
            let Some(lookup) = self
                .gsub_lookups
                .0
                .get(name)
                .or_else(|| self.gpos_lookups.0.get(name))
            else {
                continue;
            };
            for fls in &lookup.features {
                feature_map
                    .entry(fls.feature.clone())
                    .or_default()
                    .push((fls.clone(), lookup.block.name.clone()));
                used_script_language_pairs.insert((fls.script.clone(), fls.language.clone()));
            }
        }
        // Now insert a feature reference for each feature
        for (feature, langs_lookup) in feature_map {
            let mut statements: Vec<String> = if feature == "aalt" {
                // Inline the rules rather than referencing the lookups, and drop
                // the script/language statements: neither is legal inside aalt.
                let mut seen: Vec<String> = vec![];
                for (_lang, lookupname) in langs_lookup.into_iter() {
                    if let Some(rules) = inlinable_rules.get(&lookupname) {
                        for rule in rules {
                            if !seen.contains(rule) {
                                seen.push(rule.clone());
                            }
                        }
                    }
                }
                seen
            } else {
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
                featureblock
                    .statements
                    .iter()
                    .map(|x| x.as_fea(""))
                    .collect()
            };
            // Add automatic code markers for anything which would have feature writers
            if feature == "abvm"
                || feature == "blwm"
                || feature == "kern"
                || feature == "dist"
                || feature == "mark"
                || feature == "mkmk"
            {
                statements.insert(0, "# Automatic code start".to_string());
            }
            self.font.features.features.push((
                feature,
                crate::features::PossiblyAutomaticCode {
                    code: statements.join("\n"),
                    ..Default::default()
                },
            ));
        }
        if !used_script_language_pairs.is_empty() {
            // These must be arranged DFLT/dflt first if it exists, then <script>/dflt before <script>/<language>
            let mut pairs: Vec<(SmolStr, SmolStr)> =
                used_script_language_pairs.into_iter().collect();
            pairs.sort_by(|(a_script, a_lang), (b_script, b_lang)| {
                #[allow(clippy::nonminimal_bool)] // Easier to follow
                if (a_script == "DFLT" && a_lang == "dflt")
                    || (a_lang == "dflt" && b_script == a_script)
                {
                    std::cmp::Ordering::Less
                } else if b_lang == "dflt" && a_script == b_script {
                    std::cmp::Ordering::Greater
                } else {
                    (a_script, a_lang).cmp(&(b_script, b_lang))
                }
            });
            self.font.features.prefixes.insert_before(
                0,
                "LanguageSystems".into(),
                PossiblyAutomaticCode::new(
                    pairs
                        .iter()
                        .map(|(script, language)| {
                            fea_rs_ast::LanguageSystemStatement::new(
                                script.to_string(),
                                language.to_string(),
                            )
                            .as_fea("")
                        })
                        .join("\n"),
                ),
            );
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
                        .map(|(lang_id, name)| {
                            format!(
                                "    name 3 1 {} \"{}\";\n",
                                lang_id,
                                stringhelpers::fea_string_escape(name)
                            )
                        })
                        .collect::<String>()
                        .as_str())
                    + "};\n"
                    + &feature.code;
            }
        }
    }

    fn glyph_container(name: impl AsRef<str>) -> fea_rs_ast::GlyphContainer {
        fea_rs_ast::GlyphContainer::GlyphName(fea_rs_ast::GlyphName::new(name.as_ref()))
    }

    fn parse_pos_value_record(tokens: &[SmolStr]) -> Option<fea_rs_ast::ValueRecord> {
        let mut x_placement: Option<fea_rs_ast::Metric> = None;
        let mut y_placement: Option<fea_rs_ast::Metric> = None;
        let mut x_advance: Option<fea_rs_ast::Metric> = None;
        let mut y_advance: Option<fea_rs_ast::Metric> = None;

        for token in tokens {
            let (k, v) = token.split_once('=')?;
            let value: i16 = v.parse().ok()?;
            match k {
                "dx" => x_placement = Some(value.into()),
                "dy" => y_placement = Some(value.into()),
                "dh" => x_advance = Some(value.into()),
                "dv" => y_advance = Some(value.into()),
                _ => {}
            }
        }

        Some(fea_rs_ast::ValueRecord::new(
            x_placement,
            y_placement,
            x_advance,
            y_advance,
            None,
            None,
            None,
            None,
            false,
            0..0,
            None,
        ))
    }
}
