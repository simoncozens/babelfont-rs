use crate::error::FontmergeError;
use babelfont::SmolStr;

#[cfg(feature = "cli")]
use clap::Parser;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodepointArgs(pub Vec<char>);

/// Font merger
///
/// Merges two font source files together, copying selected glyphs from the donor font into the host font,
/// along with any necessary OpenType layout features.
#[cfg(feature = "cli")]
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Font file to merge into ("host" font)
    pub font_1: String,

    /// Font file to merge ("donor" font)
    pub font_2: String,

    /// Output font file
    #[arg(short, long)]
    pub output: String,

    /// Include directory for feature files
    #[arg(long)]
    pub fea_include_dir: Option<String>,

    #[command(flatten)]
    pub verbosity: clap_verbosity_flag::Verbosity,

    #[command(flatten)]
    pub glyph_selection: GlyphSelection,

    #[clap(
        short,
        long,
        default_value_t,
        value_enum,
        help_heading = "Existing glyph handling"
    )]
    pub existing_handling: ExistingGlyphHandling,

    #[clap(
        short,
        long,
        default_value_t,
        value_enum,
        help_heading = "Layout handling"
    )]
    pub layout_handling: LayoutHandling,
    #[clap(
        short,
        long,
        default_value_t,
        value_enum,
        help_heading = "Layout handling"
    )]
    pub duplicate_lookups: DuplicateLookupHandling,

    #[command(flatten)]
    pub fixups: Fixups,
}

/// Glyph selection options
#[cfg_attr(feature = "cli", derive(clap::Args))]
#[derive(Debug)]
#[cfg_attr(feature = "cli", command(next_help_heading = "Glyph selection"))]
pub struct GlyphSelection {
    /// Space-separated list of glyphs to add from donor font
    #[cfg_attr(feature = "cli", arg(short, long, value_delimiter = ' '))]
    pub glyphs: Vec<SmolStr>,

    /// File containing glyphs to add from donor font
    #[cfg_attr(feature = "cli", arg(short = 'G', long))]
    pub glyphs_file: Option<String>,

    /// Unicode codepoints to add from donor font (comma separated, hexadecimal, U+ prefix allowed, ranges with hyphen allowed)
    #[cfg_attr(feature = "cli", arg(short = 'u', long, value_parser = crate::args::parse_codepoints))]
    pub codepoints: Option<CodepointArgs>,

    /// File containing Unicode codepoints to add from donor font
    #[cfg_attr(feature = "cli", arg(short = 'U', long))]
    pub codepoints_file: Option<String>,

    /// Glyphs to exclude from donor font
    #[cfg_attr(feature = "cli", arg(short = 'x', long, value_delimiter = ' '))]
    pub exclude_glyphs: Vec<SmolStr>,

    /// File containing glyphs to exclude from donor font
    #[cfg_attr(feature = "cli", arg(short = 'X', long))]
    pub exclude_glyphs_file: Option<String>,
}

/// Existing glyph handling options
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq)]
pub enum ExistingGlyphHandling {
    #[default]
    /// Skip glyphs already present in host font
    Skip,
    /// Replace glyphs already present in host font
    Replace,
}

/// Layout closure handling options
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[derive(Clone, Default, Debug, Copy, PartialEq, Eq)]
pub enum LayoutHandling {
    /// Drop layout rules concerning glyphs not selected
    #[default]
    Subset,
    /// Add glyphs from donor font contained in layout rules, even if not in glyph set
    Closure,
    /// Don't try to parse the layout rules
    Ignore,
}

#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq)]
pub enum DuplicateLookupHandling {
    /// Drop duplicate lookups
    #[default]
    First,
    /// Merge duplicate lookups
    Both,
}

impl std::fmt::Display for DuplicateLookupHandling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DuplicateLookupHandling::First => write!(f, "first"),
            DuplicateLookupHandling::Both => write!(f, "both"),
        }
    }
}

/// Specialist fixups
#[derive(Debug)]
#[cfg_attr(feature = "cli", derive(clap::Args))]
#[cfg_attr(feature = "cli", command(next_help_heading = "Specialist fixups"))]
pub struct Fixups {
    /// Merge anchors if both fonts contain a dotted circle glyph
    #[cfg_attr(feature = "cli", arg(long, default_value = "true", action = clap::ArgAction::Set))]
    pub dotted_circle: bool,

    /// Don't add in additional masters to maintain differences between avar mappings
    #[cfg_attr(feature = "cli", arg(long, default_value = "false", action = clap::ArgAction::Set))]
    pub skip_avar_masters: bool,
}

/// Parse a single codepoint from a string
fn parse_codepoint(input: &str) -> Result<char, String> {
    if input.len() == 1 {
        // Single character
        #[allow(clippy::unwrap_used)] // We know input has at least one char
        return Ok(input.chars().next().unwrap());
    }

    let input = input.to_lowercase();
    let input = input
        .trim_start_matches("u+")
        .trim_start_matches("0x")
        .trim();
    let cp = u32::from_str_radix(input, 16).map_err(|_| format!("Invalid codepoint: {}", input))?;

    char::from_u32(cp).ok_or_else(|| format!("Invalid Unicode codepoint: U+{:04X}", cp))
}

/// Parse codepoints from strings
// We just use a String error here to keep clap happy
pub fn parse_codepoints(input: &str) -> Result<CodepointArgs, String> {
    let mut result = Vec::new();

    for item in input.split(",") {
        // Handle comments
        let item = item.split("#").next().unwrap_or("").trim();
        if item.is_empty() {
            continue;
        }
        if item.contains('-') {
            // Parse range
            let parts: Vec<&str> = item.split('-').collect();
            if parts.len() != 2 {
                return Err(format!("Invalid codepoint range: {}", item));
            }
            #[allow(clippy::indexing_slicing)] // We have already checked length
            let start = parse_codepoint(parts[0])?;
            #[allow(clippy::indexing_slicing)] // We have already checked length
            let end = parse_codepoint(parts[1])?;

            let start_u32 = start as u32;
            let end_u32 = end as u32;

            if start_u32 > end_u32 {
                return Err(format!(
                    "Invalid codepoint range: {} > {}",
                    start_u32, end_u32
                ));
            }

            for cp in start_u32..=end_u32 {
                if let Some(c) = char::from_u32(cp) {
                    result.push(c);
                }
            }
        } else {
            // Parse single codepoint
            result.push(parse_codepoint(item)?);
        }
    }

    Ok(CodepointArgs(result))
}

impl GlyphSelection {
    /// Get the set of glyph names to include from donor font
    pub fn get_include_glyphs(&self) -> Result<Vec<SmolStr>, FontmergeError> {
        let mut glyphs = self.glyphs.clone();

        // Glyphs from file
        if let Some(ref filename) = self.glyphs_file {
            let content = std::fs::read_to_string(filename)?;
            // Treat as one glyph per line
            for line in content.lines() {
                let glyph_name = line.trim();
                if !glyph_name.is_empty() && !glyph_name.starts_with('#') {
                    glyphs.push(glyph_name.into());
                }
            }
        }

        Ok(glyphs)
    }

    /// Get the set of glyph names to exclude from donor font
    pub fn get_exclude_glyphs(&self) -> Result<Vec<SmolStr>, FontmergeError> {
        // Glyphs from command line
        let mut glyphs = self.exclude_glyphs.clone();

        // Glyphs from file
        if let Some(ref filename) = self.exclude_glyphs_file {
            let content = std::fs::read_to_string(filename)?;
            // Treat as one glyph per line
            for line in content.lines() {
                let glyph_name = line.trim();
                if !glyph_name.is_empty() && !glyph_name.starts_with('#') {
                    glyphs.push(glyph_name.into());
                }
            }
        }
        Ok(glyphs)
    }

    pub fn get_codepoints(&self) -> Result<Vec<char>, FontmergeError> {
        let mut codepoints = self
            .codepoints
            .clone()
            .unwrap_or_else(|| CodepointArgs(Vec::new()))
            .0;

        // Codepoints from file
        if let Some(ref filename) = self.codepoints_file {
            let content = std::fs::read_to_string(filename)?;
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let cp_args = parse_codepoints(line).map_err(FontmergeError::Parse)?;
                codepoints.extend(cp_args.0);
            }
        }

        Ok(codepoints)
    }
}
