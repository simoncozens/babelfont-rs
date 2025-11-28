use crate::filters::FontFilter;

/// A filter that drops all guides from a font
#[derive(Default)]
pub struct DropGuides;

impl DropGuides {
    /// Create a new DropGuides filter
    pub fn new() -> Self {
        DropGuides
    }
}

impl FontFilter for DropGuides {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Dropping all guides from font");
        for master in font.masters.iter_mut() {
            master.guides.clear();
        }
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                layer.guides.clear();
            }
        }
        Ok(())
    }
}
