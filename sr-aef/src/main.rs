use clap::Parser;
use sr_aef::fea_rs_ast::AsFea;
use sr_aef::{skrifa::FontRef, uncompile, uncompile_context};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    debug: bool,
    input: std::path::PathBuf,
}

fn main() {
    let args = Args::parse();

    let data = std::fs::read(&args.input).expect("Failed to read input file");
    let fontref = FontRef::new(&data).expect("Failed to parse font reference");
    if args.debug {
        let context = uncompile_context(&fontref).expect("Failed to uncompile context");
        println!(
            "{}",
            serde_json::to_string_pretty(&context).expect("Failed to serialize context")
        );
    } else {
        let feature_file = uncompile(&fontref, true).expect("Failed to uncompile font");

        println!("{}", feature_file.as_fea(""));
    }
}
