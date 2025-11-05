use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// Path to the input file to convert
    font_path: std::path::PathBuf,

    /// Path to the output file
    output: std::path::PathBuf,
}

const SUPPORTED_EXTENSIONS: &[&str; 5] =
    &["ufo", "designspace", "glyphs", "glyphspackage", "babelfont"];

fn main() {
    let args = Cli::parse();
    env_logger::init();
    if !SUPPORTED_EXTENSIONS.contains(&args.font_path.extension().unwrap().to_str().unwrap()) {
        log::error!(
            "Input extension {:?} is not in the list of supported extensions: {:?}",
            args.font_path.extension().unwrap(),
            SUPPORTED_EXTENSIONS
        );
        std::process::exit(1);
    }
    if !SUPPORTED_EXTENSIONS.contains(&args.output.extension().unwrap().to_str().unwrap()) {
        log::error!(
            "Output extension {:?} is not in the list of supported extensions: {:?}",
            args.output.extension().unwrap(),
            SUPPORTED_EXTENSIONS
        );
        std::process::exit(1);
    }
    println!(
        "Converting font at {:?} to {:?}",
        args.font_path, args.output
    );

    let input = babelfont::load(args.font_path).expect("Failed to load font");
    input.save(args.output).expect("Failed to save font");
}
