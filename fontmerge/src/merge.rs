use babelfont::{Font, Glyph};
use fontdrasil::types::Axes;
use indexmap::IndexSet;

use crate::designspace::{Strategy, compatible_location};
use crate::error::FontmergeError;

pub(crate) fn merge_glyph(
    font: &mut Font,
    font2_glyph: &Glyph,
    font2_axes: &Axes,
    font2: &Font,
    strategies: &[Strategy],
) -> Result<(), FontmergeError> {
    if font.glyphs.get(&font2_glyph.name).is_none() {
        font.glyphs.push(font2_glyph.clone());
    }
    #[allow(clippy::unwrap_used)] // We check existence above
    let glyph = font.glyphs.get_mut(&font2_glyph.name).unwrap();
    // Move layers out
    let mut layers = glyph.layers.drain(..).collect::<Vec<_>>();
    // Ensure all layers have an ID
    for (i, layer) in layers.iter_mut().enumerate() {
        if layer.id.is_none() {
            layer.id = Some(format!("{}_layer_{}", glyph.name, i));
        }
    }
    let mut drop_layers = IndexSet::new();
    for (master, strategy) in font.masters.iter().zip(strategies.iter()) {
        let new_layer = match strategy {
            Strategy::Exact(id) => {
                // Find a layer in layers with this master ID, either in master_id or layer id
                let layer = layers
                    .iter()
                    .find(|l| l.master_id.as_deref() == Some(id) || l.id.as_deref() == Some(id));
                if let Some(l) = layer {
                    // We can unwrap here because we set IDs above
                    #[allow(clippy::unwrap_used)]
                    drop_layers.insert(l.id.clone().unwrap());
                    Some(l.clone())
                } else {
                    log::warn!(
                        "No layer found for glyph '{}' matching master ID '{}'",
                        glyph.name,
                        id
                    );
                    None
                }
            }
            Strategy::InterpolateOrIntermediate(loc) => {
                // We have set locations on all layers, but they're not in the right coordinate system
                let layer = layers.iter().find(|l| {
                    l.location
                        .as_ref()
                        .map(|l| l.to_user(font2_axes))
                        .map(|l| compatible_location(loc, &l))
                        .unwrap_or(false)
                });
                if let Some(l) = layer {
                    // This was an intermediate layer there, but will be a master layer here.
                    #[allow(clippy::unwrap_used)] // I think?
                    drop_layers.insert(l.id.clone().unwrap());
                    Some(l.clone())
                } else {
                    Some(
                        font2
                            .interpolate_glyph(&glyph.name, &loc.to_design(font2_axes))
                            .map_err(|e| {
                                FontmergeError::Interpolation(format!(
                                    "Failed to interpolate glyph '{}' at location {:?}: {}",
                                    glyph.name, loc, e
                                ))
                            })?,
                    )
                }
            }
            Strategy::Failed(reason) => {
                log::warn!(
                    "Skipping layer for glyph '{}' for master '{}' due to failed strategy: {}",
                    glyph.name,
                    master.id,
                    reason
                );
                None
            }
        };
        let mut new_layer = match new_layer {
            Some(l) => l,
            None => continue,
        };
        // Set its to have the correct master ID
        new_layer.id = Some(master.id.clone());
        new_layer.master_id = Some(master.id.clone());
        new_layer.location = None; // Not needed now we have exact match
        glyph.layers.push(new_layer);
    }
    // Remove used layers, let's see what's left
    #[allow(clippy::unwrap_used)] // I think we set IDs somewhere
    glyph
        .layers
        .retain(|l| !drop_layers.contains(l.id.as_ref().unwrap()));
    log::debug!(
        "After merging, glyph '{}' has {} layers (dropped {})",
        glyph.name,
        glyph.layers.len(),
        drop_layers.len()
    );
    Ok(())
}
