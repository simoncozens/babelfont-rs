# Babelfont

Babelfont is a Rust library for working with font source files from different font editing software. It provides a unified interface to load, examine, manipulate, and convert fonts between various formats, abstracting over the differences between font editors' native representations.

## Features

- **Multi-format Support**: Load and save fonts in UFO, DesignSpace, Glyphs, FontLab VFJ, and Babelfont's own JSON format
- **Format Conversion**: Convert fonts between different editor formats seamlessly
- **Font Manipulation**: Apply filters to subset, scale, or otherwise transform fonts
- **JSON Serialization**: Full serialization/deserialization of Babelfont's internal representation
- **Variable Font Support**: Full support for variable/multiple master fonts with axes, masters, and instances
- **Feature-based Compilation**: Optional dependencies for specific format support

## Status

Babelfont is currently in early development. While the core architecture and many features are implemented, some format support and filters are still works in progress.

* Glyphs file format: Mostly complete for reading and writing Glyphs 2 and Glyphs 3 files.
* UFO/DesignSpace: Reading support is implemented; writing support is in progress.
* FontLab VFJ: Very basic reading support is implemented; writing support is planned.
* Fontra: Basic reading and writing support is implemented.


## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
babelfont = "0.1"
```

### Feature Flags

By default, all major format support is enabled. You can customize which formats are supported:

```toml
[dependencies]
babelfont = { version = "0.1", default-features = false, features = ["glyphs", "ufo"] }
```

Available features:

- `glyphs` - Support for Glyphs 2 and Glyphs 3 files (`.glyphs` and `.glyphspackage`)
- `ufo` - Support for UFO and DesignSpace formats
- `fontlab` - Support for FontLab VFJ (JSON) format
- `fontra` - Support for Fontra format
- `fontir` - Enable compilation to binary font formats (`.ttf`)
- `cli` - Command-line interface support
- `typescript` - TypeScript type definition generation

## Quick Start

### Loading and Converting Fonts

```rust
use babelfont::{load, BabelfontError};

fn main() -> Result<(), BabelfontError> {
    // Load a font from any supported format
    let font = load("MyFont.designspace")?;
    
    // Convert to Glyphs format
    font.save("MyFont.glyphs")?;
    
    // Or save as JSON
    font.save("MyFont.babelfont")?;
    
    Ok(())
}
```

### Subsetting with Filters

```rust
use babelfont::{load, BabelfontError};
use babelfont::filters::{FontFilter, RetainGlyphs};

fn main() -> Result<(), BabelfontError> {
    // Load a DesignSpace file
    let mut font = load("MyFont.designspace")?;
    
    // Create a filter to retain only certain glyphs
    let filter = RetainGlyphs::new(vec![
        "A".to_string(),
        "B".to_string(),
        "C".to_string(),
        "space".to_string(),
    ]);
    
    // Apply the filter
    filter.apply(&mut font)?;
    
    // Save as a Glyphs file
    font.save("MyFont-Subset.glyphs")?;
    
    Ok(())
}
```

### Inspecting Font Metadata

```rust
use babelfont::load;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let font = load("MyFont.ufo")?;

    // Access font metadata
    println!("Font family: {}", font.names.family_name);
    println!("Units per em: {}", font.upm);
    println!("Number of glyphs: {}", font.glyphs.len());

    // Iterate over axes in a variable font
    for axis in &font.axes {
        println!("Axis: {} ({} to {})", axis.name, axis.min, axis.max);
    }

    // Access specific glyphs
    if let Some(glyph) = font.glyphs.get("A") {
        println!("Glyph 'A' has {} layers", glyph.layers.len());
        
        for layer in &glyph.layers {
            println!("  Layer: {:?}", layer.id);
        }
    }

    Ok(())
}
```

## Supported Formats

### Input/Output Formats

| Format | Extension | Read | Write | Feature Flag |
|--------|-----------|------|-------|--------------|
| UFO | `.ufo` | ✓ | ✗ | `ufo` |
| DesignSpace | `.designspace` | ✓ | ✓ | `ufo` |
| Glyphs 2/3 | `.glyphs` | ✓ | ✓ | `glyphs` |
| Glyphs Package | `.glyphspackage` | ✓ | ✗ | `glyphs` |
| FontLab VFJ | `.vfj` | ✓ | ✗ | `fontlab` |
| Babelfont JSON | `.babelfont` | ✓ | ✓ | (always) |
| TrueType | `.ttf` | ✗ | ✓ | `fontir` |

## JSON Serialization

One of Babelfont's unique features is its ability to serialize and deserialize its internal representation to and from JSON. This provides a format-agnostic way to store and exchange font data.

The JSON format is complete and lossless, meaning you can:

- Convert any supported format to JSON and back without data loss
- Use JSON as an intermediate format when converting between editor formats
- Inspect the complete font structure in a human-readable format
- Create programmatic workflows using standard JSON tools

Example:

```rust
use babelfont::load;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load from Glyphs
    let font = load("MyFont.glyphs")?;
    
    // Save as JSON
    font.save("MyFont.babelfont")?;
    
    // Load from JSON
    let font2 = load("MyFont.babelfont")?;
    
    // Convert to DesignSpace
    font2.save("MyFont.designspace")?;
    
    Ok(())
}
```

To generate TypeScript type definitions for the JSON format, run:

```
$ cargo run --example dump-typescript --features=typescript
```

## Font Filters

Babelfont includes several built-in filters for font manipulation:

- **`RetainGlyphs`** - Keep only specified glyphs, removing all others. Components referencing removed glyphs are automatically decomposed.
- **`DropAxis`** - Remove a variable font axis
- **`DropInstances`** - Remove named instances
- **`DropKerning`** - Remove all kerning data
- **`DropFeatures`** - Remove OpenType feature code
- **`DropGuides`** - Remove all guidelines
- **`DropSparseMasters`** - Convert sparse masters to associated layers
- **`ResolveIncludes`** - Resolve feature file include statements
- **`ScaleUpem`** - Scale the units-per-em value

Filters can be chained together:

```rust
use babelfont::{load, filters::*};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut font = load("MyFont.glyphs")?;
    
    // Apply multiple filters
    RetainGlyphs::new(vec!["A".into(), "B".into()]).apply(&mut font)?;
    DropGuides.apply(&mut font)?;
    DropKerning.apply(&mut font)?;
    
    font.save("MyFont-Filtered.glyphs")?;
    Ok(())
}
```

## Core Types

- **`Font`** - The main font structure containing all font data
- **`Glyph`** - A single glyph with its layers and metadata
- **`Layer`** - A glyph's outline in a particular master
- **`Master`** - A design master in a variable/multiple master font
- **`Axis`** - A variation axis definition
- **`Instance`** - A named instance (static font variant)
- **`Features`** - OpenType feature code representation
- **`Shape`** - A path or component in a glyph layer

## Command-Line Interface

With the `cli` feature enabled, Babelfont also provides a command-line tool:

```bash
# Convert between formats
babelfont MyFont.glyphs --output MyFont.babelfont

# Apply filters
babelfont MyFont.babelfont --filter dropaxis=wdth --filter retainglyphs=A,B,C --output Subset.babelfont

# Compile to TTF
babelfont Subset.babelfont --output Subset.ttf

```

Compile the CLI with:

```
$ cargo build --release --bin babelfont --features=cli
```

## Related Projects

Babelfont is based on the Python [babelfont](https://github.com/simoncozens/babelfont) library. Additional development is now happening here, rather than in the original Python version.

## License

Babelfont is available under the MIT or Apache-2.0 licenses, at your option.

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues on GitHub.
