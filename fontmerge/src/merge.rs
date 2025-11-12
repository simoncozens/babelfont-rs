use babelfont::{Font, Glyph, LayerType};
use fontdrasil::coords::{Location, UserSpace};
use fontdrasil::types::Axes;
use indexmap::IndexSet;

use crate::designspace::{convert_between_designspaces, fontdrasil_axes, within_bounds, Strategy};
use crate::error::FontmergeError;

pub(crate) fn merge_glyph(
    font1: &mut Font,
    font1_nonsparse_master_ids: &[String],
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
    log::debug!(
        "Existing locations for this glyph: {:?}",
        layers
            .iter()
            .filter_map(|l| l.location.as_ref())
            .collect::<Vec<_>>()
    );
    let mut drop_layers = IndexSet::new();
    for (master_id, strategy) in font1_nonsparse_master_ids.iter().zip(strategies.iter()) {
        let master = font1
            .masters
            .iter()
            .find(|m| &m.id == master_id)
            .expect("Master ID from non-sparse list not found in font1 masters");
        let new_layer = match strategy {
            Strategy::Exact {
                layer: id,
                master_name: _,
                clamped,
            } => {
                // Find a layer in layers with this master ID, either in master_id or layer id
                let layer = layers.iter().find(|l| {
                    l.master == LayerType::DefaultForMaster(id.to_string())
                        || l.master == LayerType::AssociatedWithMaster(id.to_string())
                        || l.id.as_ref().map(|lid| lid == id).unwrap_or(false)
                });
                if let Some(l) = layer {
                    // We can unwrap here because we set IDs above
                    if !clamped {
                        #[allow(clippy::unwrap_used)]
                        drop_layers.insert(l.id.clone().unwrap());
                    }
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
            Strategy::InterpolateOrIntermediate { location, clamped } => {
                // I'm just going to leave it sparse, dammit.
                continue;

                // We have set locations on all layers, but they're not in the right coordinate system
                let layer = layers.iter().find(|l| l.location == Some(location.clone()));
                if let Some(l) = layer {
                    // This was an intermediate layer there, but will be a master layer here.
                    if !clamped {
                        #[allow(clippy::unwrap_used)] // We ensured all layers have an ID above
                        drop_layers.insert(l.id.clone().unwrap());
                    }
                    let mut l = l.clone();
                    l.master = LayerType::DefaultForMaster(master.id.clone());
                    Some(l)
                } else {
                    log::info!(
                        "Interpolating glyph '{}' at location {:?}",
                        glyph.name,
                        location
                    );
                    Some(
                        font2
                            .interpolate_glyph(&glyph.name, &location)
                            .map_err(|e| {
                                FontmergeError::Interpolation(format!(
                                    "Failed to interpolate glyph '{}' at location {:?}: {}",
                                    glyph.name, location, e
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
        .filter(|l| l.id.as_ref().is_some_and(|id| !drop_layers.contains(id)))
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
                .is_some_and(|l| within_bounds(&font1_axes, l))
        })
        .collect::<Vec<_>>();
    let first_master = font1.masters.first();
    #[allow(clippy::unwrap_used)] // We check above
    for layer in remaining_layers.iter_mut() {
        // Set to associated with master
        if let Some(master) = first_master {
            layer.master = LayerType::AssociatedWithMaster(master.id.clone());
        } else {
            layer.master = LayerType::FreeFloating;
        }
        // These locations are in the design space of font2. We're going to move them into font1, so what
        // we need to do is:
        // * Convert them to user space,
        // * Then fill in any missing axes with defaults from font1 and remove any axes not in font1,
        // * Then convert to design space of font1.
        let (loc_in_font1, _) = convert_between_designspaces(
            layer.location.as_ref().unwrap(),
            font2_axes,
            &font1_axes,
            true,
        );

        // Make double double sure there isn't a font master at this location already in font1
        if font1
            .masters
            .iter()
            .any(|m| m.location.to_user(&font1_axes) == loc_in_font1.to_user(&font1_axes))
        {
            log::debug!(
                "Not adding remaining layer at location {:?} to glyph '{}' because a master already exists there",
                layer.location.as_ref().unwrap(),
                glyph.name
            );
            continue;
        }
        layer.location = Some(loc_in_font1);

        // Check we haven't already added a layer at this location.
        let loc = layer.location.as_ref().unwrap();
        let already_exists = glyph
            .layers
            .iter()
            .any(|l| l.location.as_ref().is_some_and(|l2| l2 == loc));
        if already_exists {
            continue;
        }
        log::debug!(
            "Adding remaining layer at location {:?} to glyph '{}' as intermediate layer",
            layer.location.as_ref().unwrap(),
            glyph.name
        );
        glyph.layers.push(layer.clone());
    }

    Ok(())
}
