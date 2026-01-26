use std::{collections::HashMap, path::PathBuf};

use babelfont::filters::FontFilter;
use clap::Command;

static SUPPORTED_EXTENSIONS: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(|| {
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

fn main() {
    let command = Command::new("babelfont")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A font manipulation tool")
        .author("Babelfont Developers")
        .arg(
            clap::Arg::new("font_path")
                .help("Path to the input font file")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::new("output")
                .help("Path to the output font file")
                .required(true)
                .index(2),
        )
        .arg(
            clap::Arg::new("verbosity")
                .short('v')
                .long("verbosity")
                .help("Set the level of verbosity")
                .action(clap::ArgAction::Count),
        )
        .arg(
            clap::Arg::new("no_production_names")
                .long("no-production-names")
                .help("Do not use production names when compiling to TTF")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("dropoutlines")
                .long("drop-outlines")
                .help("Drop outlines when compiling to TTF")
                .action(clap::ArgAction::SetTrue),
        );

    // Extend with the font filter arguments
    let command = babelfont::filters::filter_group(command);

    let args = command.get_matches();
    env_logger::Builder::new()
        .filter_level(match args.get_count("verbosity") {
            0 => log::LevelFilter::Warn,
            1 => log::LevelFilter::Info,
            2 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        })
        .init();
    let input_name = PathBuf::from(args.get_one::<String>("font_path").unwrap());
    let output_name = PathBuf::from(args.get_one::<String>("output").unwrap());
    let input_extension = input_name.extension().unwrap().to_str().unwrap();
    if !SUPPORTED_EXTENSIONS.contains(&input_extension) {
        log::error!(
            "Input extension {:?} is not in the list of supported extensions: {}",
            input_name.extension().unwrap(),
            SUPPORTED_EXTENSIONS.join(", ")
        );
        std::process::exit(1);
    }
    let output_extension = output_name.extension().unwrap().to_str().unwrap();
    if !SUPPORTED_EXTENSIONS.contains(&output_extension) {
        log::error!(
            "Output extension {:?} is not in the list of supported extensions: {}",
            output_name.extension().unwrap(),
            SUPPORTED_EXTENSIONS.join(", ")
        );
        std::process::exit(1);
    }

    let compiling = output_extension == "ttf";

    #[cfg(feature = "fontir")]
    let mut compilation_options = babelfont::convertors::fontir::CompilationOptions::default();

    #[cfg(feature = "fontir")]
    if args.get_flag("no_production_names") && compiling {
        compilation_options.dont_use_production_names = true;
    }

    // Horrible clap grubbing to get the filters and their arguments in the order that they
    // appear in the command line. Clap stores --foo 1 --bar 2 --foo 3 as
    // "foo": ["1", "3"], "bar": ["2"], losing the original order, but we can regain that order
    // by looking at the raw occurrences of the "filters" arg group.
    let filter_group = args.get_raw("filters").unwrap();
    let mut counter = HashMap::new();
    let mut filters: Vec<Box<dyn FontFilter>> = vec![];

    for filter in filter_group {
        let count = counter.entry(filter.to_str().unwrap()).or_insert(0);
        // Get the count'th occurrence of this filter
        let mut occurrences = args.get_raw_occurrences(filter.to_str().unwrap()).unwrap();
        let value = occurrences
            .nth(*count)
            .unwrap()
            .map(|v| v.to_str().unwrap())
            .collect::<String>();
        *count += 1;
        filters.push(babelfont::filters::cli_to_filter(filter.to_str().unwrap(), &value).unwrap());
    }

    #[cfg(feature = "fontir")]
    {
        if args.get_flag("dropkerning") && compiling {
            compilation_options.skip_kerning = true;
        }
        if args.get_flag("dropfeatures") && compiling {
            compilation_options.skip_features = true;
        }
        if args.get_flag("dropoutlines") && compiling {
            compilation_options.skip_outlines = true;
        }
    }

    log::info!("Loading {}", input_name.display());

    let mut input = babelfont::load(input_name).expect("Failed to load font");
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

    log::info!("Saving {}", output_name.display());
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
            std::fs::write(&output_name, bytes).expect("Failed to write output font");
            let after_safe = std::time::Instant::now();
            log::info!(
                "Compiled in {:.2?}, saved in {:.2?}",
                after_compile - before,
                after_safe - after_compile
            );
        }
    } else {
        input.save(output_name).expect("Failed to save font");
    }
}
