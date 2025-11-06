use babelfont::{Font, Glyph, LayerType};
use fontdrasil::types::Axes;
use indexmap::IndexSet;

use crate::designspace::{compatible_location, fontdrasil_axes, Strategy};
use crate::error::FontmergeError;

fn within_bounds(
    loc: &fontdrasil::coords::Location<fontdrasil::coords::DesignSpace>,
    axes: &fontdrasil::types::Axes,
) -> bool {
    for axis in axes.iter() {
        if let Some(value) = loc.get(axis.tag) && (
            value < axis.min.to_design(&axis.converter)
                || value > axis.max.to_design(&axis.converter)
        ){
                log::trace!(
                    "Location {:?} out of bounds on axis '{}': {} not in [{}, {}]",
                    loc,
                    axis.tag,
                    value.to_f64(),
                    axis.min.to_f64(),
                    axis.max.to_f64()
                );
                return false;
            }
    }
    true
}

pub(crate) fn merge_glyph(
    font1: &mut Font,
    font2_glyph: &Glyph,
    font2_axes: &Axes,
    font2: &Font,
    strategies: &[Strategy],
) -> Result<(), FontmergeError> {
    let font1_axes = fontdrasil_axes(&font1.axes)?;
    if font1.glyphs.get(&font2_glyph.name).is_none() {
        font1.glyphs.push(font2_glyph.clone());
    }
    #[allow(clippy::unwrap_used)] // We check existence above
    let glyph = font1.glyphs.get_mut(&font2_glyph.name).unwrap();
    // Move layers out
    let mut layers = glyph.layers.drain(..).collect::<Vec<_>>();
    // Ensure all layers have an ID
    for (i, layer) in layers.iter_mut().enumerate() {
        if layer.id.is_none() {
            layer.id = Some(format!("{}_layer_{}", glyph.name, i));
        }
    }
    let mut drop_layers = IndexSet::new();
    for (master, strategy) in font1.masters.iter().zip(strategies.iter()) {
        let new_layer = match strategy {
            Strategy::Exact(id) => {
                // Find a layer in layers with this master ID, either in master_id or layer id
                let layer = layers.iter().find(|l| {
                    l.master == LayerType::DefaultForMaster(id.to_string())
                        || l.master == LayerType::AssociatedWithMaster(id.to_string())
                        || l.id.as_ref().map(|lid| lid == id).unwrap_or(false)
                });
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
                    log::info!("Interpolating glyph '{}' at location {:?}", glyph.name, loc);
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
        new_layer.master = LayerType::DefaultForMaster(master.id.clone());
        new_layer.location = None; // Not needed now we have exact match
        glyph.layers.push(new_layer);
    }
    // Remove used layers, let's see what's left
    let remaining_layers = layers
        .into_iter()
    // XXX We should not drop layers if there was clamping involved
    //     .filter(|l| l.id.as_ref().is_some_and(|id| !drop_layers.contains(id)))
        .collect::<Vec<_>>();
    log::debug!(
        "After merging, glyph still '{}' has {} layers remaining (dropped {})",
        font2_glyph.name,
        remaining_layers.len(),
        drop_layers.len()
    );
    // These layers all have locations; if they're within the bounds of font1's designspace, and
    // if they are at the default location for any axes we don't have in font1 just
    // shove them in as intermediate layers.
    let mut remaining_layers = remaining_layers
        .into_iter()
        .filter(|l| {
            l.location
                .as_ref()
                .is_some_and(|l| within_bounds(l, &font1_axes))
        })
        .collect::<Vec<_>>();
    let first_master = font1.masters.first();
    for layer in remaining_layers.iter_mut() {
        // Set to associated with master
        if let Some(master) = first_master {
            layer.master = LayerType::AssociatedWithMaster(master.id.clone());
        } else {
            layer.master = LayerType::FreeFloating;
        }
    }
    glyph.layers.extend(remaining_layers);

    Ok(())
}
