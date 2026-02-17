use serde::Serialize;
use std::{ops::Range, path::PathBuf};
use thiserror::Error;
#[cfg(feature = "cli")]
extern crate serde_json_path_to_error as serde_json;

/// Errors produced while using the Babelfont crate
#[derive(Debug, Error, Serialize)]
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
    IO(String),

    #[cfg(feature = "ufo")]
    #[error("Error parsing designspace file: {0}")]
    /// Error parsing designspace file
    DesignSpaceLoad(String),

    #[cfg(feature = "ufo")]
    #[error("Error saving designspace file: {0}")]
    /// Error saving designspace file
    DesignSpaceSave(String),

    #[cfg(feature = "glyphs")]
    #[error("Error parsing Glyphs file: {0}")]
    /// Error parsing Glyphs file
    PlistParse(String),

    #[cfg(feature = "ufo")]
    #[error("Error loading UFO: {0}")]
    /// Error loading UFO
    UfoLoad(String),

    #[cfg(feature = "ufo")]
    #[error("Error in UFO naming: {0}")]
    /// Error in UFO naming
    UfoName(String),

    #[cfg(feature = "ufo")]
    #[error("Error in UFO color: {0}")]
    /// Error in UFO color
    UfoColor(String),

    #[cfg(feature = "vfb")]
    #[error("Error loading VFB: {0}")]
    /// Error loading VFB
    VfbLoad(String),

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
    VariationModel(String),

    #[error("Delta error: {0}")]
    /// An error occurred while processing a delta
    Delta(String),

    #[error("Axis conversion error: {0}")]
    /// An error occurred while converting axes
    AxisConversion(String),

    /// Called a method which requires a decomposed layer on a layer which had components
    #[error("Called a method which requires a decomposed layer on a layer which had components")]
    NeedsDecomposition,

    #[error("JSON conversion error: {0}")]
    /// JSON conversion error
    JsonSerialize(String),

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

    #[error("Binary font reading error: {0}")]
    /// Binary font reading error
    BinaryFontRead(String),

    /// Layout closure errors
    #[error("Glyphset grew unreasonably during layout closure")]
    LayoutClosureError,
    /// Feature parsing error
    #[error("Feature parsing error: {0:#?}")]
    FeatureParsing(Vec<FeatureError>),
}

#[derive(Debug, Serialize)]
pub struct FeatureError {
    pub message: String,
    pub span: Range<usize>,
    pub is_error: bool,
}

impl From<fea_rs_ast::Error> for BabelfontError {
    fn from(e: fea_rs_ast::Error) -> Self {
        match e {
            fea_rs_ast::Error::CannotConvert => {
                BabelfontError::General("Problem converting feature statement".to_string())
            }
            fea_rs_ast::Error::CannotLoadSourceFile(_source_load_error) => BabelfontError::BadPath,
            fea_rs_ast::Error::FeatureParsing(diagnostic_set) => BabelfontError::FeatureParsing(
                diagnostic_set
                    .diagnostics()
                    .iter()
                    .map(|d| FeatureError {
                        message: d.message.text.clone(),
                        span: d.span(),
                        is_error: d.is_error(),
                    })
                    .collect(),
            ),
        }
    }
}

#[cfg(feature = "ufo")]
impl From<norad::error::FontLoadError> for BabelfontError {
    fn from(e: norad::error::FontLoadError) -> Self {
        BabelfontError::UfoLoad(e.to_string())
    }
}

#[cfg(feature = "ufo")]
impl From<norad::error::NamingError> for BabelfontError {
    fn from(e: norad::error::NamingError) -> Self {
        BabelfontError::UfoName(e.to_string())
    }
}

#[cfg(feature = "ufo")]
impl From<norad::error::ColorError> for BabelfontError {
    fn from(e: norad::error::ColorError) -> Self {
        BabelfontError::UfoColor(e.to_string())
    }
}

impl From<write_fonts::read::ReadError> for BabelfontError {
    fn from(e: write_fonts::read::ReadError) -> Self {
        BabelfontError::BinaryFontRead(e.to_string())
    }
}

#[cfg(feature = "ufo")]
impl From<norad::error::DesignSpaceLoadError> for BabelfontError {
    fn from(e: norad::error::DesignSpaceLoadError) -> Self {
        match e {
            norad::error::DesignSpaceLoadError::Io(error) => error.into(),
            norad::error::DesignSpaceLoadError::DeError(de_error) => {
                BabelfontError::DesignSpaceLoad(de_error.to_string())
            }
            _ => todo!(),
        }
    }
}

#[cfg(feature = "ufo")]
impl From<norad::error::DesignSpaceSaveError> for BabelfontError {
    fn from(e: norad::error::DesignSpaceSaveError) -> Self {
        match e {
            norad::error::DesignSpaceSaveError::Io(error) => error.into(),
            norad::error::DesignSpaceSaveError::SeError(se_error) => {
                BabelfontError::DesignSpaceSave(se_error.to_string())
            }
            _ => todo!(),
        }
    }
}

impl From<serde_json::Error> for BabelfontError {
    fn from(e: serde_json::Error) -> Self {
        BabelfontError::JsonSerialize(e.to_string())
    }
}

impl From<fontdrasil::variations::DeltaError> for BabelfontError {
    fn from(e: fontdrasil::variations::DeltaError) -> Self {
        BabelfontError::Delta(e.to_string())
    }
}

impl From<fontdrasil::error::Error> for BabelfontError {
    fn from(e: fontdrasil::error::Error) -> Self {
        BabelfontError::AxisConversion(e.to_string())
    }
}

impl From<fontdrasil::variations::VariationModelError> for BabelfontError {
    fn from(e: fontdrasil::variations::VariationModelError) -> Self {
        BabelfontError::VariationModel(e.to_string())
    }
}

impl From<std::io::Error> for BabelfontError {
    fn from(e: std::io::Error) -> Self {
        BabelfontError::IO(e.to_string()) // Can we do better?
    }
}
