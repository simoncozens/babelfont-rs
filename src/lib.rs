#![deny(clippy::unwrap_used, clippy::expect_used)]

#[cfg(feature = "cli")]
extern crate serde_json_path_to_error as serde_json;

mod anchor;
mod axis;
mod common;
pub mod convertors;
mod error;
mod features;
pub mod filters;
mod font;
mod glyph;
mod guide;
mod i18ndictionary;
mod instance;
mod interpolate;
mod layer;
mod master;
mod metrics;
pub mod names;
mod serde_helpers;
mod shape;

pub use crate::{
    anchor::Anchor,
    axis::Axis,
    common::{Direction, Node, NodeType, OTScalar, Position},
    error::BabelfontError,
    features::Features,
    font::Font,
    glyph::{Glyph, GlyphCategory, GlyphList},
    guide::Guide,
    i18ndictionary::I18NDictionary,
    instance::Instance,
    layer::{Layer, LayerType},
    master::Master,
    metrics::MetricType,
    shape::{Component, Path, Shape},
};
pub use fontdrasil::coords::{
    DesignCoord, DesignLocation, NormalizedCoord, NormalizedLocation, UserCoord, UserLocation,
};
use std::path::PathBuf;

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
        #[cfg(feature = "glyphs")]
        Some(ext) if ext == "glyphs" || ext == "glyphspackage" => {
            crate::convertors::glyphs3::load(pb)
        }
        _ => Err(BabelfontError::UnknownFileType { path: pb }),
    }?;
    font.source = Some(pb_clone);
    Ok(font)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
