#![deny(clippy::unwrap_used, clippy::expect_used)]

mod anchor;
mod axis;
mod common;
pub mod convertors;
mod error;
mod features;
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
    layer::Layer,
    master::Master,
    metrics::MetricType,
    shape::{Component, Path, Shape},
};
pub use fontdrasil::coords::{
    DesignCoord, DesignLocation, NormalizedCoord, NormalizedLocation, UserCoord, UserLocation,
};
use std::path::PathBuf;

pub fn load(filename: &str) -> Result<Font, BabelfontError> {
    let pb = PathBuf::from(filename);
    if filename.ends_with(".designspace") {
        #[cfg(feature = "ufo")]
        return crate::convertors::designspace::load(pb);
        #[cfg(not(feature = "ufo"))]
        Err(BabelfontError::UnknownFileType { path: pb })
    } else if filename.ends_with(".vfj") {
        #[cfg(feature = "fontlab")]
        return crate::convertors::fontlab::load(pb);
        #[cfg(not(feature = "fontlab"))]
        Err(BabelfontError::UnknownFileType { path: pb })
    } else if filename.ends_with(".ufo") {
        #[cfg(feature = "ufo")]
        return crate::convertors::ufo::load(pb);

        #[cfg(not(feature = "ufo"))]
        Err(BabelfontError::UnknownFileType { path: pb })
    } else if filename.ends_with(".glyphs") || filename.ends_with(".glyphspackage") {
        #[cfg(feature = "glyphs")]
        return crate::convertors::glyphs3::load(pb);
        #[cfg(not(feature = "glyphs"))]
        Err(BabelfontError::UnknownFileType { path: pb })
    } else {
        Err(BabelfontError::UnknownFileType { path: pb })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
