use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BabelfontError {
    #[error("Unknown file type for file {path:?}")]
    UnknownFileType { path: PathBuf },

    #[error("Wrong convertor for file {path:?}")]
    WrongConvertor { path: PathBuf },

    #[error("Error parsing font: {0}")]
    General(String),

    #[error("IO Error: {0}")]
    IO(#[from] io::Error),

    #[cfg(feature = "ufo")]
    #[error("Error parsing designspace file: {0}")]
    DesignSpaceLoad(#[from] norad::error::DesignSpaceLoadError),

    #[cfg(feature = "glyphs")]
    #[error("Error parsing Glyphs file: {0}")]
    PlistParse(Box<dyn std::error::Error + 'static>),

    #[cfg(feature = "ufo")]
    #[error("Error loading UFO: {0}")]
    UfoLoad(#[from] norad::error::FontLoadError),

    #[cfg(feature = "ufo")]
    #[error("Error in UFO naming: {0}")]
    UfoName(#[from] norad::error::NamingError),
    #[cfg(feature = "ufo")]
    #[error("Error in UFO color: {0}")]
    UfoColor(#[from] norad::error::ColorError),
    #[error("Could not find default master in {path:?}")]
    NoDefaultMaster { path: PathBuf },

    #[error("Ill-defined axis {axis_name}!: {reason}")]
    IllDefinedAxis { axis_name: String, reason: String },

    #[error("Ill-constructed path")]
    BadPath,

    #[error("Glyph {glyph} not interpolatable: {reason}")]
    GlyphNotInterpolatable { glyph: String, reason: String },

    #[error("Glyph {glyph} not found")]
    GlyphNotFound { glyph: String },

    #[error("Variation model error: {0}")]
    VariationModel(#[from] fontdrasil::variations::VariationModelError),

    #[error("Delta error: {0}")]
    Delta(#[from] fontdrasil::variations::DeltaError),

    #[error("Called a method which requires a decomposed layer on a layer which had components")]
    NeedsDecomposition,
}

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
            BabelfontError::NoDefaultMaster { path } => fontir::error::BadSource::new(
                path,
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
        }
    }
}
