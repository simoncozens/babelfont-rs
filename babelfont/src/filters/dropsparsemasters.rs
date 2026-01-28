use std::collections::HashMap;

use crate::{filters::FontFilter, LayerType};

#[derive(Default)]
/// A filter that drops all sparse masters from a font
///
/// A sparse master is defined as a master that does not have a glyph layer for every glyph in the font.
/// When this filter is applied, all sparse masters are removed from the font, and their associated layers
/// are converted to associated layers of a non-sparse master.
pub struct DropSparseMasters;

impl DropSparseMasters {
    /// Create a new DropSparseMasters filter
    pub fn new() -> Self {
        DropSparseMasters
    }
}

impl FontFilter for DropSparseMasters {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let sparse_master_ids_and_locations = font
            .masters
            .iter()
            .filter(|x| x.is_sparse(font))
            .map(|x| (x.id.clone(), x.location.clone()))
            .collect::<HashMap<_, _>>();
        log::info!(
            "Moving {} sparse masters to associated layers",
            sparse_master_ids_and_locations.len()
        );
        let first_master_id = font
            .masters
            .iter()
            .find(|x| !x.is_sparse(font))
            .map(|x| x.id.clone())
            .ok_or_else(|| {
                crate::BabelfontError::FilterError(
                    "Cannot drop sparse masters: all masters are sparse".into(),
                )
            })?;

        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                if let LayerType::DefaultForMaster(master_id) = &layer.master {
                    if let Some(loc) = sparse_master_ids_and_locations.get(master_id) {
                        layer.location = Some(loc.clone());
                        layer.master = LayerType::AssociatedWithMaster(first_master_id.clone());
                    }
                }
            }
        }

        font.masters
            .retain(|m| !sparse_master_ids_and_locations.contains_key(&m.id));
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(DropSparseMasters::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("dropsparsemasters")
            .long("drop-sparse-masters")
            .help("Drop all sparse masters, converting their layers to associated layers")
            .action(clap::ArgAction::SetTrue)
    }
}
