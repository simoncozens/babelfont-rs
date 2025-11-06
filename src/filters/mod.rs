mod dropaxis;
mod dropsparsemasters;
mod retainglyphs;
mod scaleupem;

pub use dropaxis::DropAxis;
pub use dropsparsemasters::DropSparseMasters;
pub use retainglyphs::RetainGlyphs;
pub use scaleupem::ScaleUpem;

pub trait FontFilter {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError>;
}
