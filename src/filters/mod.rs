mod decomposesmartcomponents;
mod dropaxis;
mod dropfeatures;
mod dropguides;
mod dropinstances;
mod dropkerning;
mod dropsparsemasters;
mod dropvariations;
#[cfg(feature = "glyphs")]
mod glyphsdata;
mod resolveincludes;
mod retainglyphs;
mod scaleupem;

pub use decomposesmartcomponents::DecomposeSmartComponents;
pub use dropaxis::DropAxis;
pub use dropfeatures::DropFeatures;
pub use dropguides::DropGuides;
pub use dropinstances::DropInstances;
pub use dropkerning::DropKerning;
pub use dropsparsemasters::DropSparseMasters;
pub use dropvariations::DropVariations;
#[cfg(feature = "glyphs")]
pub use glyphsdata::GlyphsData;
pub use resolveincludes::ResolveIncludes;
pub use retainglyphs::RetainGlyphs;
pub use scaleupem::ScaleUpem;

/// A trait for font filters that can be applied to a font
pub trait FontFilter {
    /// Apply the filter to the given font
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError>;
}
