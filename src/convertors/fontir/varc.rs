use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    common::decomposition::DecomposedAffine, convertors::fontir::glyphs::resolve_layer_location,
    Axis, BabelfontError, Component, Font, Glyph, Layer, LayerType,
};
use fontdrasil::{
    coords::{Location, NormalizedCoord, NormalizedSpace},
    types::Tag,
    variations::{VariationModel, VariationRegion},
};
use smol_str::SmolStr;
use write_fonts::{
    read::{FontRef, TableProvider},
    tables::{
        layout::CoverageFormat1,
        varc::{
            CoverageTable, DecomposedTransform, VarComponent, VarCompositeGlyph, Varc,
            VarcVariationIndex,
        },
        variations::mivs_builder::{MultiItemVariationStoreBuilder, SparseRegion},
    },
    types::{F2Dot14, GlyphId, GlyphId16},
    FontBuilder,
};

pub fn insert_varc_table(binary: &[u8], font: &Font) -> Result<Vec<u8>, BabelfontError> {
    let mut builder = FontBuilder::new();
    let mut storebuilder = MultiItemVariationStoreBuilder::new();
    let existing_font = FontRef::new(binary)?;
    // We must get the axis names and tags from the fvar table,
    // just in case fontc reordered things or didn't include all axes.
    let fvar = existing_font.fvar()?.axes()?;
    let axis_order = fvar.iter().map(|a| a.axis_tag()).collect::<Vec<Tag>>();
    let fontdrasil_axes = font.fontdrasil_axes()?;

    let master = font
        .default_master()
        .ok_or_else(|| BabelfontError::NoDefaultMaster)?;
    let have_notdef = font
        .glyphs
        .iter()
        .any(|g| g.name == SmolStr::new(".notdef"));
    let glyph_ids = font
        .glyphs
        .iter()
        .enumerate()
        .map(|(i, g)| (g.name.clone(), i as u16 + if have_notdef { 0 } else { 1 }))
        .collect::<std::collections::HashMap<SmolStr, u16>>();
    let mut coverage_ids: Vec<GlyphId16> = vec![];
    let mut var_composites = vec![];
    for glyph in font.glyphs.iter() {
        // Partition layers into default for master and others
        let Some(default_layer) = glyph
            .layers
            .iter()
            .find(|layer| layer.master == LayerType::DefaultForMaster(master.id.clone()))
        else {
            continue;
        };
        if !default_layer.is_smart_composite() {
            continue;
        }
        let non_default_layers = glyph
            .layers
            .iter()
            .filter(|layer| layer.master != LayerType::DefaultForMaster(master.id.clone()))
            .collect::<Vec<&Layer>>();
        log::debug!("Handling smart composite glyph {}", glyph.name);
        let mut var_components = vec![];
        for (index, component) in default_layer.components().enumerate() {
            // Find the equivalent component in other layers
            let mut other_layers: Vec<(Location<NormalizedSpace>, &Component)> = vec![];
            for layer in non_default_layers.iter() {
                if let Some(other_component) = layer.components().nth(index) {
                    // Resolve layer location
                    let maybe_location = resolve_layer_location(layer, font);
                    if let Some(loc) = maybe_location {
                        other_layers.push((loc.to_normalized(&fontdrasil_axes), other_component));
                    }
                } else {
                    log::warn!(
                        "Could not find matching component for component {} in glyph {} in layer {:?}",
                        index,
                        glyph.name,
                        layer.master
                    );
                    continue;
                }
            }
            var_components.push(to_varcomp(
                component,
                &other_layers,
                glyph,
                &font.axes,
                &glyph_ids,
                &mut storebuilder,
                &axis_order,
                &fontdrasil_axes,
            )?);
        }
        // If we have outlines left, we should add a glyph outline entry as well.
        if default_layer.paths().count() > 0 {
            var_components.push(VarComponent {
                reset_unspecified_axes: true,
                gid: GlyphId::new(
                    (*glyph_ids.get(&glyph.name).ok_or_else(|| {
                        BabelfontError::General(format!(
                            "Glyph ID not found for glyph {} when building VARC table",
                            glyph.name
                        ))
                    })?)
                    .into(),
                ),
                condition_index: None,
                axis_values: None,
                axis_values_var_index: None,
                transform: DecomposedTransform::default(),
                transform_var_index: None,
            });
        }

        let var_composite = VarCompositeGlyph(var_components);

        coverage_ids.push(GlyphId16::new(*glyph_ids.get(&glyph.name).ok_or_else(
            || {
                BabelfontError::General(format!(
                    "Glyph ID not found for glyph {} when building VARC table",
                    glyph.name
                ))
            },
        )?));

        var_composites.push(var_composite);
    }
    let coverage = CoverageTable::Format1(CoverageFormat1::new(coverage_ids));
    let varc = Varc::new_from_composite_glyphs(coverage, storebuilder, vec![], var_composites);
    builder.add_table(&varc).map_err(|e| {
        BabelfontError::General(format!("Error adding VARC table to font: {:#?}", e))
    })?;
    builder.copy_missing_tables(existing_font);
    Ok(builder.build().to_vec())
}

#[allow(clippy::too_many_arguments)]
fn to_varcomp(
    component: &Component,
    other_layers: &[(Location<NormalizedSpace>, &Component)],
    glyph: &Glyph,
    axes: &[Axis],
    glyph_ids: &HashMap<SmolStr, u16>,
    storebuilder: &mut MultiItemVariationStoreBuilder,
    axis_order: &[Tag],
    fontdrasil_axes: &fontdrasil::types::Axes,
) -> Result<VarComponent, BabelfontError> {
    let glyph_id = glyph_ids.get(&component.reference).ok_or_else(|| {
        BabelfontError::General(format!(
            "Glyph ID not found for component glyph {} in glyph {}",
            component.reference, glyph.name
        ))
    })?;
    let mut axis_values = derive_axis_values(component, axes, axis_order);
    let other_axis_values = other_layers
        .iter()
        .map(|(loc, comp)| (loc.clone(), derive_axis_values(comp, axes, axis_order)))
        .collect::<Vec<(Location<NormalizedSpace>, BTreeMap<u16, f32>)>>();
    // If there are any keys in any of the other axis values that are not in axis_values,
    // we need to add them with the default value for that axis, so that they have
    // a base value from which to derive variation.
    for (_, other_av) in other_axis_values.iter() {
        for (axis_index, _) in other_av.iter() {
            if !axis_values.contains_key(axis_index) {
                let axis_tag = axis_order[*axis_index as usize];
                let default = axes
                    .iter()
                    .find(|a| a.tag == axis_tag)
                    .and_then(|a| a.default)
                    .ok_or_else(|| {
                        BabelfontError::General(format!(
                            "Could not find default value for axis {} when building VARC table",
                            axis_tag
                        ))
                    })?;
                axis_values.insert(*axis_index, default.to_f64() as f32);
            }
        }
    }
    let other_transforms = other_layers
        .iter()
        .map(|(loc, comp)| (loc.clone(), comp.transform))
        .collect::<Vec<(Location<NormalizedSpace>, DecomposedAffine)>>();
    let axis_values_var_index = store_axis_value_deltas(
        &axis_values,
        &other_axis_values,
        storebuilder,
        fontdrasil_axes,
    )?;
    let component = VarComponent {
        reset_unspecified_axes: true,
        gid: GlyphId::new((*glyph_id).into()),
        condition_index: None,
        axis_values: if axis_values.is_empty() {
            None
        } else {
            Some(axis_values)
        },
        axis_values_var_index,
        transform: DecomposedTransform {
            translate_x: Some(component.transform.translation.0),
            translate_y: Some(component.transform.translation.1),
            ..Default::default()
        },
        transform_var_index: None,
    };
    Ok(component)
}

fn derive_axis_values(
    component: &Component,
    axes: &[Axis],
    axis_order: &[Tag],
) -> BTreeMap<u16, f32> {
    let mut axis_values = BTreeMap::new();
    for axis in axes.iter() {
        let Some(name) = &axis.name.get_default() else {
            continue;
        };
        let Some(index) = axis_order.iter().position(|t| t == &axis.tag) else {
            log::warn!(
                "Axis tag '{}' not found in fvar table when building VARC table",
                axis.tag,
            );
            continue;
        };
        let axis_loc = component.location.get(*name).cloned();
        #[allow(clippy::unwrap_used)]
        if let Some(axis_value) = axis_loc {
            let normalized_value = axis_value
                .to_normalized(&axis._converter().unwrap())
                .to_f64() as f32;
            axis_values.insert(index as u16, normalized_value);
        }
    }
    axis_values
}

// fn affine_to_transforms_and_deltas(
//     base_transform: &DecomposedAffine,
//     other_transforms: &[(Location<NormalizedSpace>, DecomposedAffine)],
//     fontdrasil_axes: &fontdrasil::types::Axes,
//     storebuilder: &MultiItemVariationStoreBuilder,
// ) -> (DecomposedTransform, VariationIndex) {
//     let mut base_decomposed = DecomposedTransform::default();
//     let mut all_locations: HashSet<Location<NormalizedSpace>> = HashSet::new();
//     for (loc, _) in other_transforms.iter() {
//         all_locations.insert(loc.clone());
//     }
//     let mut model = VariationModel::new(all_locations, fontdrasil_axes.axis_order());
//     // If base.translate_x is not zero, or any other transforms has a non-zero
//     // translate_x, then it's Some
//     if base_transform.translation.0 != 0.0
//         || other_transforms.iter().any(|(_, t)| t.translation.0 != 0.0)
//     {
//         base_decomposed.translate_x = Some(base_transform.translation.0);
//     }
// }

fn store_axis_value_deltas(
    base_axis_values: &BTreeMap<u16, f32>,
    other_locations_axis_values: &[(Location<NormalizedSpace>, BTreeMap<u16, f32>)],
    storebuilder: &mut MultiItemVariationStoreBuilder,
    fontdrasil_axes: &fontdrasil::types::Axes,
) -> Result<Option<VarcVariationIndex>, BabelfontError> {
    let base_loc = Location::from_iter(
        fontdrasil_axes
            .axis_order()
            .iter()
            .map(|a| (*a, NormalizedCoord::default())),
    );
    let mut all_locations: HashSet<Location<NormalizedSpace>> = HashSet::new();
    all_locations.insert(base_loc.clone());
    for (loc, _) in other_locations_axis_values.iter() {
        all_locations.insert(loc.clone());
    }
    // if all_locations.is_empty() {
    //     return Ok(None);
    // }
    let model = VariationModel::new(all_locations, fontdrasil_axes.axis_order());
    let mut master_values: HashMap<Location<NormalizedSpace>, Vec<f64>> = HashMap::new();
    for (axis_index, base_value) in base_axis_values.iter() {
        master_values
            .entry(base_loc.clone())
            .or_default()
            .push(*base_value as f64);
        for (loc, other_axis_values) in other_locations_axis_values.iter() {
            let other_value = other_axis_values.get(axis_index).cloned().unwrap_or(0.0);
            master_values
                .entry(loc.clone())
                .or_default()
                .push(other_value as f64);
        }
    }
    log::debug!("Master values: {:#?}", master_values);
    let axis_values_deltas = model.deltas(&master_values).map_err(|e| {
        BabelfontError::General(format!("Error computing VARC axis value deltas: {:#?}", e))
    })?;
    let mut deltas = vec![];
    for (region, delta_values) in axis_values_deltas.iter() {
        let sparse_region = sparse_region_from_region(region, fontdrasil_axes);
        deltas.push((
            sparse_region,
            delta_values
                .iter()
                .map(|x| F2Dot14::from_f32(*x as f32).to_bits())
                .collect(),
        ));
    }
    // Sparsify the variation regions
    if axis_values_deltas.is_empty() {
        Ok(None)
    } else {
        let temporary_index = storebuilder.add_deltas(deltas);
        Ok(Some(VarcVariationIndex::PendingVariationIndex(
            temporary_index,
        )))
    }
}

fn sparse_region_from_region(
    region: &VariationRegion,
    axes: &fontdrasil::types::Axes,
) -> SparseRegion {
    let region = region.to_write_fonts_variation_region(axes);

    let region_axes: Vec<(u16, F2Dot14, F2Dot14, F2Dot14)> = region
        .region_axes
        .iter()
        .enumerate()
        .map(|(index, ra)| (index as u16, ra.start_coord, ra.peak_coord, ra.end_coord))
        .collect();
    SparseRegion::new(region_axes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_doesnt_crash() {}
}
