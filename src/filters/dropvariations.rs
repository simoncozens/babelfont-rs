use fontdrasil::coords::Location;

use crate::{filters::FontFilter, LayerType};

#[derive(Default)]
/// A filter that drops all variation information from a font
///
/// Only the default master is kept; all other masters are removed, and their associated layers
/// are also removed.
pub struct DropVariations;

impl DropVariations {
    /// Create a new DropVariations filter
    pub fn new() -> Self {
        DropVariations
    }
}

impl FontFilter for DropVariations {
    fn apply(&self, font: &mut crate::font::Font) -> Result<(), crate::error::BabelfontError> {
        let Some(default_master_index) = font.default_master_index() else {
            log::warn!("No default master found; cannot drop variations");
            return Ok(());
        };
        font.masters = vec![font.masters[default_master_index].clone()];
        font.axes = vec![];
        font.masters[0].location = Location::default();
        font.instances = vec![];
        for glyph in font.glyphs.iter_mut() {
            glyph.layers.retain(|layer| match &layer.master {
                LayerType::DefaultForMaster(master_id) => master_id == &font.masters[0].id,
                _ => false,
            });
        }
        Ok(())
    }
}
