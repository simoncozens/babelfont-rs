use crate::filters::FontFilter;

#[derive(Default)]
/// A filter that drops all instances from a font
pub struct DropInstances;

impl DropInstances {
    /// Create a new DropInstances filter
    pub fn new() -> Self {
        DropInstances
    }
}

impl FontFilter for DropInstances {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Dropping all Instances from font");
        font.instances.clear();
        Ok(())
    }
}
