use babelfont::{Features, GlyphList, load};
use clap::Parser;
use fontmerge::args::{
    DuplicateLookupHandling, ExistingGlyphHandling, Fixups, GlyphSelection, LayoutHandling,
};
use fontmerge::fontmerge;

use std::path::PathBuf;

/// Font subsetter
///
/// Creates a subset of a font, including only the specified glyphs and applying any necessary OpenType layout features.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Font file to subset
    pub font: String,

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

fn main() {
    let args = Args::parse();
    env_logger::Builder::new()
        .filter_level(args.verbosity.into())
        .init();
    log::debug!("Loading font");
    let font1 = match load(&args.font) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to load font 1 ({}): {}", args.font, e);
            std::process::exit(1);
        }
    };
    let mut target = font1.clone();
    target.features = Features::default();
    target.glyphs = GlyphList::default();

    // Check the output file is supported
    // File name should end with `.glyphs`, `.glyphspackage`, `.babelfont` or `.ttf`.
    // We can't save designspace or UFO files yet.
    {
        let output_path = PathBuf::from(&args.output);
        let output_ext = output_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        match output_ext {
            "glyphs" | "glyphspackage" | "babelfont" | "ttf" | "otf" => {}
            _ => {
                log::error!(
                    "Output file extension '{}' is not supported. Please use .glyphs, .glyphspackage, .babelfont, .ttf or .otf",
                    output_ext
                );
                return;
            }
        }
    }

    let glyphset_filter = fontmerge::GlyphsetFilter::new(
        args.glyph_selection
            .get_include_glyphs()
            .expect("Failed to get include glyphs"),
        args.glyph_selection
            .get_exclude_glyphs()
            .expect("Failed to get exclude glyphs"),
        args.glyph_selection
            .get_codepoints()
            .expect("Failed to get codepoints"),
        &mut target,
        &font1,
        args.existing_handling,
    );

    match fontmerge(
        target,
        font1,
        glyphset_filter,
        args.layout_handling,
        !args.fixups.skip_avar_masters,
    ) {
        Ok(result) => {
            log::info!("Saving merged font to {}", args.output);

            result
                .save(&args.output)
                .expect("Failed to save merged font");
        }
        Err(e) => {
            log::error!("Font merge failed: {}", e);
            std::process::exit(1);
        }
    }
}
