use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// Path to the input file to convert
    font_path: std::path::PathBuf,

    /// Path to the output file
    output: std::path::PathBuf,
}

fn main() {
    let args = Cli::parse();
    env_logger::init();
    println!(
        "Converting font at {:?} to {:?}",
        args.font_path, args.output
    );
    let input = babelfont::load(args.font_path).expect("Failed to load font");
    input.save(args.output).expect("Failed to save font");
}
