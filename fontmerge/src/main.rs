use babelfont::load;
use clap::Parser;
use fontmerge::fontmerge;
use fontmerge::Args;

use std::path::PathBuf;

fn main() {
    let args = Args::parse();
    env_logger::Builder::new()
        .filter_level(args.verbosity.into())
        .init();
    log::debug!("Loading font 1");
    let mut font1 = load(&args.font_1).expect("Failed to load font 1");
    log::debug!("Loading font 2");
    let font2 = load(&args.font_2).expect("Failed to load font 2");

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
                log::error!("Output file extension '{}' is not supported. Please use .glyphs, .glyphspackage, .babelfont, .ttf or .otf", output_ext);
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
        &mut font1,
        &font2,
        args.existing_handling,
    );

    match fontmerge(font1, font2, glyphset_filter, args.layout_handling) {
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
