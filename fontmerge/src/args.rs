use crate::error::FontmergeError;
use clap::Parser;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodepointArgs(pub Vec<char>);

/// font font merger
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// font font file to merge into
    pub font_1: String,

    /// font font file to merge
    pub font_2: String,

    /// Output font font file
    #[arg(short, long)]
    pub output: Option<String>,

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
#[derive(clap::Args, Debug)]
#[command(next_help_heading = "Glyph selection")]
pub struct GlyphSelection {
    /// Space-separated list of glyphs to add from font 2
    #[arg(short, long, value_delimiter = ' ')]
    pub glyphs: Vec<String>,

    /// File containing glyphs to add from font 2
    #[arg(short = 'G', long)]
    pub glyphs_file: Option<String>,

    /// Unicode codepoints to add from font 2
    #[arg(short = 'u', long, value_parser = crate::args::parse_codepoints)]
    pub codepoints: Option<CodepointArgs>,

    /// File containing Unicode codepoints to add from font 2
    #[arg(short = 'U', long)]
    pub codepoints_file: Option<String>,

    /// Glyphs to exclude from font 2
    #[arg(short = 'x', long, value_delimiter = ' ')]
    pub exclude_glyphs: Vec<String>,

    /// File containing glyphs to exclude from font 2
    #[arg(short = 'X', long)]
    pub exclude_glyphs_file: Option<String>,
}

/// Existing glyph handling options
#[derive(clap::ValueEnum, Debug, Clone, Default, Copy, PartialEq, Eq)]
pub enum ExistingGlyphHandling {
    #[default]
    /// Skip glyphs already present in font 1
    Skip,
    /// Replace glyphs already present in font 1
    Replace,
}

/// Layout closure handling options
#[derive(clap::ValueEnum, Clone, Default, Debug, Copy, PartialEq, Eq)]
pub enum LayoutHandling {
    /// Drop layout rules concerning glyphs not selected
    #[default]
    Subset,
    /// Add glyphs from font 2 contained in layout rules, even if not in glyph set
    Closure,
    /// Don't try to parse the layout rules
    Ignore,
}

#[derive(clap::ValueEnum, Debug, Clone, Default, Copy, PartialEq, Eq)]
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
#[derive(clap::Args, Debug)]
#[command(next_help_heading = "Specialist fixups")]
pub struct Fixups {
    /// Merge anchors if both fonts contain a dotted circle glyph
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    pub dotted_circle: bool,
}

/// Parse a single codepoint from a string
fn parse_codepoint(input: &str) -> Result<char, FontmergeError> {
    if input.len() == 1 {
        // Single character
        #[allow(clippy::unwrap_used)] // We know input has at least one char
        return Ok(input.chars().next().unwrap());
    }

    let input = input.trim_start_matches("U+").trim_start_matches("u+");
    let cp = u32::from_str_radix(input, 16)
        .map_err(|_| FontmergeError::Parse(format!("Invalid codepoint: {}", input)))?;

    char::from_u32(cp)
        .ok_or_else(|| FontmergeError::Parse(format!("Invalid Unicode codepoint: U+{:04X}", cp)))
}

/// Parse codepoints from strings
pub fn parse_codepoints(input: &str) -> Result<CodepointArgs, FontmergeError> {
    let mut result = Vec::new();

    for item in input.split(",") {
        if item.contains('-') {
            // Parse range
            let parts: Vec<&str> = item.split('-').collect();
            if parts.len() != 2 {
                return Err(FontmergeError::Parse(format!(
                    "Invalid codepoint range: {}",
                    item
                )));
            }
            #[allow(clippy::indexing_slicing)] // We have already checked length
            let start = parse_codepoint(parts[0])?;
            #[allow(clippy::indexing_slicing)] // We have already checked length
            let end = parse_codepoint(parts[1])?;

            let start_u32 = start as u32;
            let end_u32 = end as u32;

            if start_u32 > end_u32 {
                return Err(FontmergeError::Parse(format!(
                    "Invalid codepoint range: {} > {}",
                    start_u32, end_u32
                )));
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
    /// Get the set of glyph names to include from font 2
    pub fn get_include_glyphs(&self) -> Result<Vec<String>, FontmergeError> {
        let mut glyphs = self.glyphs.clone();

        // Glyphs from file
        if let Some(ref filename) = self.glyphs_file {
            let content = std::fs::read_to_string(filename)?;
            // Treat as one glyph per line
            for line in content.lines() {
                let glyph_name = line.trim();
                if !glyph_name.is_empty() && !glyph_name.starts_with('#') {
                    glyphs.push(glyph_name.to_string());
                }
            }
        }

        Ok(glyphs)
    }

    /// Get the set of glyph names to exclude from font 2
    pub fn get_exclude_glyphs(&self) -> Result<Vec<String>, FontmergeError> {
        // Glyphs from command line
        let mut glyphs = self.exclude_glyphs.clone();

        // Glyphs from file
        if let Some(ref filename) = self.exclude_glyphs_file {
            let content = std::fs::read_to_string(filename)?;
            // Treat as one glyph per line
            for line in content.lines() {
                let glyph_name = line.trim();
                if !glyph_name.is_empty() && !glyph_name.starts_with('#') {
                    glyphs.push(glyph_name.to_string());
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
                let cp_args = parse_codepoints(line)?;
                codepoints.extend(cp_args.0);
            }
        }

        Ok(codepoints)
    }
}
