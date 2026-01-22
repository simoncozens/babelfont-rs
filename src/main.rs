use std::{str::FromStr, sync::LazyLock};

use babelfont::{filters::FontFilter, SmolStr};
use clap::Parser;
use indexmap::IndexSet;

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    pub verbosity: clap_verbosity_flag::Verbosity,

    #[clap(long)]
    filter: Vec<String>,

    #[clap(long)]
    /// Drop kerning from the output font
    drop_kerning: bool,

    #[clap(long)]
    /// Drop features from the output font
    drop_features: bool,

    #[clap(long)]
    /// Drop outlines from the output font
    drop_outlines: bool,

    #[clap(long)]
    no_production_names: bool,

    #[clap(long, value_delimiter = ',')]
    retain_glyphs: Vec<String>,

    /// Path to the input file to convert
    font_path: std::path::PathBuf,

    /// Path to the output file
    output: std::path::PathBuf,
}

static SUPPORTED_EXTENSIONS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "babelfont",
        #[cfg(feature = "ufo")]
        "ufo",
        #[cfg(feature = "ufo")]
        "designspace",
        #[cfg(feature = "glyphs")]
        "glyphs",
        #[cfg(feature = "glyphs")]
        "glyphspackage",
        #[cfg(feature = "fontir")]
        "ttf",
        #[cfg(feature = "vfb")]
        "vfb",
        #[cfg(feature = "robocjk")]
        "rcjk",
    ]
});

// Convert filters to FontFilter trait objects
fn convert_filters(filter: &[String]) -> Vec<Box<dyn FontFilter>> {
    let mut result: Vec<Box<dyn FontFilter>> = Vec::new();
    for f in filter {
        let parts: Vec<&str> = f.splitn(2, '=').collect();
        match parts[0] {
            "scaleupem" => {
                if parts.len() != 2 {
                    log::error!("scaleupem filter requires a new upem value");
                    continue;
                }
                let new_upem: u16 = match parts[1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        log::error!("Invalid upem value for scaleupem filter: {}", parts[1]);
                        continue;
                    }
                };
                result.push(Box::new(babelfont::filters::ScaleUpem::new(new_upem)));
            }
            "dropaxis" => {
                if parts.len() != 2 {
                    log::error!("dropaxis filter requires an axis tag");
                    continue;
                }
                let axis_tag =
                    fontdrasil::types::Tag::from_str(parts[1]).expect("Invalid axis tag");
                result.push(Box::new(babelfont::filters::DropAxis::new(axis_tag)));
            }
            "dropguides" => {
                result.push(Box::new(babelfont::filters::DropGuides::new()));
            }
            "dropinstances" => {
                result.push(Box::new(babelfont::filters::DropInstances::new()));
            }
            "dropvariations" => {
                result.push(Box::new(babelfont::filters::DropVariations::new()));
            }
            "resolveincludes" => {
                // help the Option<impl> out a bit
                let filter_arg: Option<&str> = if parts.len() == 2 {
                    Some(parts[1])
                } else {
                    None
                };
                result.push(Box::new(babelfont::filters::ResolveIncludes::new(
                    filter_arg,
                )));
            }
            "decomposesmartcomponents" => {
                let filter_arg: Option<IndexSet<SmolStr>> = if parts.len() == 2 {
                    let glyphs: IndexSet<SmolStr> = parts[1]
                        .split(',')
                        .map(|s| SmolStr::new(s.trim()))
                        .collect();
                    Some(glyphs)
                } else {
                    None
                };
                result.push(Box::new(babelfont::filters::DecomposeSmartComponents::new(
                    filter_arg,
                )));
            }
            "rewritesmartaxes" => {
                result.push(Box::new(babelfont::filters::RewriteSmartAxes::new()));
            }
            _ => {
                log::warn!("Unknown filter: {}", parts[0]);
            }
        }
    }
    result
}

fn main() {
    let args = Cli::parse();
    env_logger::Builder::new()
        .filter_level(args.verbosity.into())
        .init();
    let input_extension = args.font_path.extension().unwrap().to_str().unwrap();
    if !SUPPORTED_EXTENSIONS.contains(&input_extension) {
        log::error!(
            "Input extension {:?} is not in the list of supported extensions: {}",
            args.font_path.extension().unwrap(),
            SUPPORTED_EXTENSIONS.join(", ")
        );
        std::process::exit(1);
    }
    let output_extension = args.output.extension().unwrap().to_str().unwrap();
    if !SUPPORTED_EXTENSIONS.contains(&output_extension) {
        log::error!(
            "Output extension {:?} is not in the list of supported extensions: {}",
            args.output.extension().unwrap(),
            SUPPORTED_EXTENSIONS.join(", ")
        );
        std::process::exit(1);
    }

    let compiling = output_extension == "ttf";

    let mut filters = convert_filters(&args.filter);

    #[cfg(feature = "fontir")]
    let mut compilation_options = babelfont::convertors::fontir::CompilationOptions::default();

    #[cfg(feature = "fontir")]
    if args.no_production_names && compiling {
        compilation_options.dont_use_production_names = true;
    }

    if !args.retain_glyphs.is_empty() {
        filters.push(Box::new(babelfont::filters::RetainGlyphs::new(
            args.retain_glyphs.clone(),
        )));
    }

    #[cfg(feature = "fontir")]
    if args.drop_kerning {
        #[cfg(feature = "fontir")]
        if compiling {
            compilation_options.skip_kerning = true;
        }

        if !compiling {
            filters.push(Box::new(babelfont::filters::DropKerning::new()));
        }
    }
    if args.drop_features {
        #[cfg(feature = "fontir")]
        if compiling {
            compilation_options.skip_features = true;
        }
        if !compiling {
            filters.push(Box::new(babelfont::filters::DropFeatures::new()));
        }
    }

    if args.drop_outlines {
        #[cfg(feature = "fontir")]
        if compiling {
            compilation_options.skip_outlines = true;
        }
    }
    log::info!("Loading {}", args.font_path.display());

    let mut input = babelfont::load(args.font_path).expect("Failed to load font");
    assert!(input.source.is_some(), "Loaded font has no source path");
    if !filters.is_empty() {
        log::info!("Applying filters...");
        let before_filters = std::time::Instant::now();
        for filter in filters {
            filter
                .apply(&mut input)
                .expect("Failed to apply font filter");
        }
        let after_filters = std::time::Instant::now();
        log::info!("Applied filters in {:.2?}", after_filters - before_filters);
    }

    log::info!("Saving {}", args.output.display());
    if compiling {
        #[cfg(feature = "fontir")]
        {
            let before = std::time::Instant::now();
            let bytes = babelfont::convertors::fontir::BabelfontIrSource::compile(
                input,
                compilation_options,
            )
            .expect("Failed to compile font");
            let after_compile = std::time::Instant::now();
            std::fs::write(&args.output, bytes).expect("Failed to write output font");
            let after_safe = std::time::Instant::now();
            log::info!(
                "Compiled in {:.2?}, saved in {:.2?}",
                after_compile - before,
                after_safe - after_compile
            );
        }
    } else {
        input.save(args.output).expect("Failed to save font");
    }
}
