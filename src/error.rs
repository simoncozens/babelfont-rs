use std::{io, path::PathBuf};
use thiserror::Error;
#[cfg(feature = "cli")]
extern crate serde_json_path_to_error as serde_json;

/// Errors produced while using the Babelfont crate
#[derive(Debug, Error)]
pub enum BabelfontError {
    #[error("Unknown file type for file {path:?}")]
    /// The file type is unknown
    UnknownFileType {
        /// The path of the file
        path: PathBuf,
    },

    /// The convertor used is wrong for the file
    #[error("Wrong convertor for file {path:?}")]
    WrongConvertor {
        /// The path of the file
        path: PathBuf,
    },

    /// General error with a message
    #[error("Error parsing font: {0}")]
    General(String),

    #[error("IO Error: {0}")]
    /// IO error
    IO(#[from] io::Error),

    #[cfg(feature = "ufo")]
    #[error("Error parsing designspace file: {0}")]
    /// Error parsing designspace file
    DesignSpaceLoad(#[from] norad::error::DesignSpaceLoadError),

    #[cfg(feature = "ufo")]
    #[error("Error saving designspace file: {0}")]
    /// Error saving designspace file
    DesignSpaceSave(#[from] norad::error::DesignSpaceSaveError),

    #[cfg(feature = "glyphs")]
    #[error("Error parsing Glyphs file: {0}")]
    /// Error parsing Glyphs file
    PlistParse(String),

    #[cfg(feature = "ufo")]
    #[error("Error loading UFO: {0}")]
    /// Error loading UFO
    UfoLoad(#[from] norad::error::FontLoadError),

    #[cfg(feature = "ufo")]
    #[error("Error in UFO naming: {0}")]
    /// Error in UFO naming
    UfoName(#[from] norad::error::NamingError),

    #[cfg(feature = "ufo")]
    #[error("Error in UFO color: {0}")]
    /// Error in UFO color
    UfoColor(#[from] norad::error::ColorError),

    #[cfg(feature = "vfb")]
    #[error("Error loading VFB: {0}")]
    /// Error loading VFB
    VfbLoad(Box<dyn std::error::Error>),

    /// Could not find default master for the font at the given path
    #[error("Could not find default master")]
    NoDefaultMaster,

    #[error("Master not found: {0}")]
    /// Could not find the specified master
    MasterNotFound(String),

    #[error("Ill-defined axis {axis_name}!: {reason}")]
    /// An axis is ill-defined
    IllDefinedAxis {
        /// The name of the axis
        axis_name: String,
        /// The reason why the axis is ill-defined
        reason: String,
    },

    #[error("Ill-constructed path")]
    /// A path could not be constructed properly
    BadPath,

    #[error("Glyph {glyph} not interpolatable: {reason}")]
    /// A glyph could not be interpolated
    GlyphNotInterpolatable {
        /// The name of the glyph
        glyph: String,
        /// The reason why the glyph is not interpolatable
        reason: String,
    },

    #[error("Glyph {glyph} not found")]
    /// A glyph was not found
    GlyphNotFound {
        /// The name of the glyph requested
        glyph: String,
    },

    #[error("Variation model error: {0}")]
    /// An error occurred while constructing a variation model
    VariationModel(#[from] fontdrasil::variations::VariationModelError),

    #[error("Delta error: {0}")]
    /// An error occurred while processing a delta
    Delta(#[from] fontdrasil::variations::DeltaError),

    /// Called a method which requires a decomposed layer on a layer which had components
    #[error("Called a method which requires a decomposed layer on a layer which had components")]
    NeedsDecomposition,

    #[error("JSON conversion error: {0}")]
    /// JSON conversion error
    JsonSerialize(#[from] serde_json::Error),

    #[error("Filter error: {0}")]
    /// General error when running a filter
    FilterError(String),

    #[error("Layer '{layer}' refered to a smart component axis '{axis}' which was not defined in its glyph")]
    /// Unknown smart component axis
    UnknownSmartComponentAxis {
        /// The axis name
        axis: String,
        /// The layer name
        layer: String,
    },
}

#[cfg(feature = "fontir")]
impl From<BabelfontError> for fontir::error::Error {
    fn from(val: BabelfontError) -> Self {
        match val {
            BabelfontError::UnknownFileType { path } => fontir::error::BadSource::new(
                path,
                fontir::error::BadSourceKind::UnrecognizedExtension,
            )
            .into(),
            BabelfontError::WrongConvertor { path } => fontir::error::BadSource::new(
                path,
                fontir::error::BadSourceKind::Custom("Wrong convertor".to_string()),
            )
            .into(),
            BabelfontError::General(msg) => {
                fontir::error::BadSource::new("Unknown", fontir::error::BadSourceKind::Custom(msg))
                    .into()
            }
            BabelfontError::IO(source) => {
                fontir::error::BadSource::new("Unknown", fontir::error::BadSourceKind::Io(source))
                    .into()
            }
            #[cfg(feature = "ufo")]
            BabelfontError::DesignSpaceLoad(orig) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(orig.to_string()),
            )
            .into(),
            #[cfg(feature = "ufo")]
            BabelfontError::DesignSpaceSave(orig) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(orig.to_string()),
            )
            .into(),
            #[cfg(feature = "glyphs")]
            BabelfontError::PlistParse(orig) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(orig.to_string()),
            )
            .into(),
            #[cfg(feature = "ufo")]
            BabelfontError::UfoLoad(orig) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(orig.to_string()),
            )
            .into(),
            #[cfg(feature = "ufo")]
            BabelfontError::UfoName(orig) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(orig.to_string()),
            )
            .into(),
            #[cfg(feature = "ufo")]
            BabelfontError::UfoColor(orig) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(orig.to_string()),
            )
            .into(),
            BabelfontError::NoDefaultMaster => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom("No default master".into()),
            )
            .into(),
            BabelfontError::IllDefinedAxis {
                axis_name,
                reason: _,
            } => fontir::error::Error::NoAxisDefinitions(axis_name),
            BabelfontError::BadPath => fontir::error::BadGlyph::new(
                fontdrasil::types::GlyphName::from("<unknown>"),
                fontir::error::BadGlyphKind::PathConversion(
                    fontir::error::PathConversionError::Parse("Bad path".into()),
                ),
            )
            .into(),
            BabelfontError::NeedsDecomposition => todo!(),
            BabelfontError::GlyphNotInterpolatable { glyph, reason: _ } => {
                fontir::error::BadGlyph::new(
                    fontdrasil::types::GlyphName::from(glyph),
                    fontir::error::BadGlyphKind::NoInstances,
                )
                .into()
            }
            BabelfontError::GlyphNotFound { glyph } => fontir::error::BadGlyph::new(
                fontdrasil::types::GlyphName::from(glyph),
                fontir::error::BadGlyphKind::NoInstances,
            )
            .into(),
            BabelfontError::VariationModel(e) => e.into(),
            BabelfontError::Delta(e) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(e.to_string()),
            )
            .into(),
            BabelfontError::JsonSerialize(e) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(e.to_string()),
            )
            .into(),
            #[cfg(feature = "vfb")]
            BabelfontError::VfbLoad(e) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(e.to_string()),
            )
            .into(),
            BabelfontError::FilterError(msg) => {
                fontir::error::BadSource::new("Unknown", fontir::error::BadSourceKind::Custom(msg))
                    .into()
            }
            BabelfontError::MasterNotFound(master_name) => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(format!("Master not found: {}", master_name)),
            )
            .into(),
            BabelfontError::UnknownSmartComponentAxis { axis, layer } => fontir::error::BadSource::new(
                "Unknown",
                fontir::error::BadSourceKind::Custom(format!(
                    "Layer '{}' refered to a smart component axis '{}' which was not defined in its glyph",
                    layer, axis
                )),
            ).into()
        }
    }
}
