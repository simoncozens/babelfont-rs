use crate::filters::FontFilter;

/// A filter that drops all features from a font
#[derive(Default)]
pub struct DropFeatures;

impl DropFeatures {
    /// Create a new DropFeatures filter
    pub fn new() -> Self {
        DropFeatures
    }
}

impl FontFilter for DropFeatures {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Dropping all features from font");
        font.features = crate::Features::default();
        Ok(())
    }
}
