//! # Babelfont
//!
//! Babelfont is a library for working with font source files from different font editing software.
//! It provides a unified interface to load, examine, manipulate, and convert fonts between
//! various formats, abstracting over the differences between font editors' native representations.
//!
//! ## Supported Formats
//!
//! Babelfont supports the following font source formats (depending on enabled features):
//!
//! - **Glyphs** (feature: `glyphs`): Glyphs 2 and Glyphs 3 files (`.glyphs` and `.glyphspackage`) (saving and loading)
//! - **UFO/DesignSpace** (feature: `ufo`): Unified Font Object format and DesignSpace documents (loading only)
//! - **FontLab** (feature: `fontlab`): FontLab VFJ (JSON) format (in progress)
//! - **Fontra** (feature: `fontra`): Fontra format (in progress)
//! - **Babelfont JSON**: Native JSON serialization of Babelfont's internal representation
//!
//! Additionally, with the `fontir` feature enabled, fonts can be compiled to binary `.ttf` format.
//!
//! ## Core Concepts
//!
//! Babelfont represents fonts using a [`Font`] structure that contains:
//!
//! - **Glyphs**: The font's glyph set with their outlines and metadata
//! - **Masters**: Design masters for variable/multiple master fonts
//! - **Axes**: Variable font axes definitions
//! - **Instances**: Named instances (static font variants)
//! - **Features**: OpenType feature code
//! - **Metadata**: Font naming, version, dates, and other information
//!
//! ## JSON Serialization
//!
//! One of Babelfont's key features is its ability to serialize and deserialize its internal
//! representation to and from JSON. This provides a format-agnostic way to store and exchange
//! font data, and can be useful for:
//!
//! - Converting between different font editor formats
//! - Creating programmatic font workflows
//! - Debugging and inspecting font structures
//! - Storing intermediate build artifacts
//! - Keeping font sources in version control
//!
//! To save a font as JSON, use the `.babelfont` extension:
//!
//! ```no_run
//! # use babelfont::{load, BabelfontError};
//! # fn main() -> Result<(), BabelfontError> {
//! let font = load("MyFont.glyphs")?;
//! font.save("MyFont.babelfont")?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Font Filters
//!
//! Babelfont includes a set of filters for manipulating fonts. Filters implement the
//! [`filters::FontFilter`] trait and can be chained together to perform complex transformations:
//!
//! - [`filters::RetainGlyphs`]: Keep only specified glyphs, removing all others
//! - [`filters::DropAxis`]: Remove a variable font axis
//! - [`filters::DropInstances`]: Remove named instances
//! - [`filters::DropKerning`]: Remove kerning data
//! - [`filters::DropFeatures`]: Remove OpenType feature code
//! - [`filters::DropGuides`]: Remove guidelines
//! - [`filters::DropSparseMasters`]: Remove sparse masters
//! - [`filters::ResolveIncludes`]: Resolve feature file includes
//! - [`filters::ScaleUpem`]: Scale the units-per-em
//!
//! ## Example: Load, Filter, and Convert
//!
//! This example demonstrates loading a DesignSpace-based font, filtering it to retain only
//! specific glyphs, and saving it as a Glyphs file:
//!
//! ```no_run
//! use babelfont::{load, BabelfontError};
//! use babelfont::filters::{FontFilter, RetainGlyphs};
//!
//! fn main() -> Result<(), BabelfontError> {
//!     // Load a DesignSpace file
//!     let mut font = load("MyFont.designspace")?;
//!     
//!     // Create a filter to retain only certain glyphs
//!     let filter = RetainGlyphs::new(vec![
//!         "A".to_string(),
//!         "B".to_string(),
//!         "C".to_string(),
//!         "space".to_string(),
//!     ]);
//!     
//!     // Apply the filter
//!     filter.apply(&mut font)?;
//!     
//!     // Save as a Glyphs file
//!     font.save("MyFont-Subset.glyphs")?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Example: Working with Font Metadata
//!
//! ```no_run
//! # use babelfont::{load, BabelfontError};
//! # fn main() -> Result<(), BabelfontError> {
//! let font = load("MyFont.ufo")?;
//!
//! // Access font metadata
//! println!("Font family: {:?}", font.names.family_name);
//! println!("Units per em: {}", font.upm);
//! println!("Number of glyphs: {}", font.glyphs.len());
//!
//! // Iterate over axes in a variable font
//! for axis in &font.axes {
//!     println!("Axis: {:?} ({:?}-{:?})", axis.name, axis.min, axis.max);
//! }
//!
//! // Access glyphs
//! if let Some(glyph) = font.glyphs.get("A") {
//!     println!("Glyph 'A' has {} layers", glyph.layers.len());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Feature Flags
//!
//! - `glyphs`: Enable support for Glyphs format files (default: enabled)
//! - `ufo`: Enable support for UFO and DesignSpace formats (default: enabled)
//! - `fontlab`: Enable support for FontLab VFJ format (default: enabled)
//! - `fontra`: Enable support for Fontra format (default: enabled)
//! - `fontir`: Enable compilation to binary font formats (default: enabled)
//! - `cli`: Enable command-line interface support
//! - `typescript`: Enable TypeScript type definition generation

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_docs)]
#[cfg(feature = "cli")]
extern crate serde_json_path_to_error as serde_json;

mod anchor;
mod axis;
mod common;
/// Convertors for various font file formats
pub mod convertors;
mod error;
mod features;
/// Filters for font processing
pub mod filters;
mod font;
mod glyph;
mod guide;
mod i18ndictionary;
mod instance;
mod interpolate;
mod layer;
mod layout;
mod master;
mod metrics;
mod names;
mod serde_helpers;
mod shape; // exported macro_rules! helpers for FormatSpecific

pub use crate::{
    anchor::Anchor,
    axis::Axis,
    common::{CustomOTValues, Direction, FormatSpecific, Node, NodeType, Position},
    error::BabelfontError,
    features::Features,
    font::Font,
    glyph::{Glyph, GlyphCategory, GlyphList},
    guide::Guide,
    i18ndictionary::I18NDictionary,
    instance::Instance,
    layer::{Layer, LayerType},
    layout::closure::close_layout,
    master::Master,
    metrics::MetricType,
    names::Names,
    shape::{Component, OutlinePen, Path, PathBuilder, Shape},
};
pub use fontdrasil::coords::{
    DesignCoord, DesignLocation, NormalizedCoord, NormalizedLocation, UserCoord, UserLocation,
};
use std::path::PathBuf;
// Ensure we export any types re-exported that we use in our public API
pub use fontdrasil::types::Tag;
pub use kurbo::Rect;
pub use smol_str::SmolStr;
pub use write_fonts::read::tables::name::NameId;

/// Load a Babelfont Font from a file
///
/// Which file formats are supported depends on which features are enabled:
/// - "ufo": UFO and DesignSpace files
/// - "glyphs": Glyphs files
/// - "fontlab": FontLab VFJ files
pub fn load(filename: impl Into<PathBuf>) -> Result<Font, BabelfontError> {
    let pb = filename.into();
    let pb_clone = pb.clone();

    let mut font: Font = match pb.extension() {
        Some(ext) if ext == "babelfont" => {
            let buffered = std::io::BufReader::new(std::fs::File::open(&pb)?);
            Ok(serde_json::from_reader(buffered)?)
        }
        #[cfg(feature = "ufo")]
        Some(ext) if ext == "designspace" => crate::convertors::designspace::load(pb),
        #[cfg(feature = "fontlab")]
        Some(ext) if ext == "vfj" => crate::convertors::fontlab::load(pb),
        #[cfg(feature = "ufo")]
        Some(ext) if ext == "ufo" => crate::convertors::ufo::load(pb),
        #[cfg(feature = "fontforge")]
        Some(ext) if ext == "sfd" => crate::convertors::fontforge::load(pb),
        #[cfg(feature = "vfb")]
        Some(ext) if ext == "vfb" => crate::convertors::vfb::load(pb),
        #[cfg(feature = "robocjk")]
        Some(ext) if ext == "rcjk" => crate::convertors::robocjk::load(pb),
        #[cfg(feature = "glyphs")]
        Some(ext) if ext == "glyphs" || ext == "glyphspackage" => {
            crate::convertors::glyphs3::load(pb)
        }
        _ => Err(BabelfontError::UnknownFileType { path: pb }),
    }?;
    font.source = Some(pb_clone);
    Ok(font)
}
