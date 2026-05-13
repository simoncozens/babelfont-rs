use std::collections::HashMap;

use fontdrasil::{
    coords::{Location, NormalizedSpace},
    variations::VariationModel,
};
use write_fonts::{
    from_obj::ToOwnedTable,
    read::{FontRef, TableProvider},
    tables::{
        avar::{Avar, AxisValueMap, SegmentMaps},
        variations::{
            common_builder::NO_VARIATION_INDEX, ivs_builder::VariationStoreBuilder,
            DeltaSetIndexMap,
        },
    },
    types::F2Dot14,
    FontBuilder,
};

use crate::{BabelfontError, Font};

pub(crate) fn insert_avar2_table(binary: &[u8], font: &Font) -> Result<Vec<u8>, BabelfontError> {
    let fontref = FontRef::new(binary)?;
    let axes = font.fontdrasil_axes()?;
    let axis_tags = axes.axis_order();
    let mut avar: Avar = match fontref.avar() {
        Ok(existing) => existing.to_owned_table(),
        Err(_) => {
            // Populate identity maps for all axes
            let identity = vec![
                AxisValueMap::new(F2Dot14::from_f32(-1.0), F2Dot14::from_f32(-1.0)),
                AxisValueMap::new(F2Dot14::from_f32(0.0), F2Dot14::from_f32(0.0)),
                AxisValueMap::new(F2Dot14::from_f32(1.0), F2Dot14::from_f32(1.0)),
            ];
            let maps = axis_tags
                .iter()
                .map(|_| SegmentMaps::new(identity.clone()))
                .collect();
            Avar::new(maps)
        }
    };

    // Collect locations and ensure default is present
    let cross_locs: Vec<_> = font
        .cross_axis_mappings
        .iter()
        .map(|mapping| mapping.input.to_normalized(&axes))
        .collect::<Result<Vec<_>, _>>()?;

    let has_default = cross_locs.iter().any(|loc| loc.is_default());
    let mut all_locations = cross_locs.clone();
    if !has_default {
        all_locations.insert(0, Location::new());
    }

    log::debug!("Building avar v2 with {} locations", all_locations.len());

    let model = VariationModel::new(
        all_locations.clone().into_iter().collect(),
        axis_tags.clone(),
    );

    let mut builder = VariationStoreBuilder::new(font.axes.len() as u16);
    // Map from axis index (in axis_tags order) to var store index
    let mut axis_var_idxes: Vec<Option<u32>> = vec![None; axis_tags.len()];

    for (axis_pos, tag) in axis_tags.iter().enumerate() {
        // Build master values map: location -> [output delta for this axis]
        let mut master_values_map: HashMap<Location<NormalizedSpace>, Vec<f64>> = HashMap::new();

        for loc in &all_locations {
            let delta = if !loc.is_default() {
                font.cross_axis_mappings
                    .iter()
                    .find(|m| matches!(m.input.to_normalized(&axes), Ok(ml) if ml == *loc))
                    .and_then(|mapping| {
                        let output_loc = mapping.output.to_normalized(&axes).ok()?;
                        let input_loc = mapping.input.to_normalized(&axes).ok()?;
                        Some(
                            (output_loc.get(*tag).unwrap_or_default().to_f64()
                                - input_loc.get(*tag).unwrap_or_default().to_f64())
                                * 16384.0,
                        )
                    })
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            master_values_map.insert(loc.clone(), vec![delta]);
        }

        let deltas = model.deltas(&master_values_map)?;

        // Convert fontdrasil regions to write-fonts regions with proper axis coordinates
        let store_deltas: Vec<(write_fonts::tables::variations::VariationRegion, i16)> = deltas
            .into_iter()
            .flat_map(|(region, vals)| {
                let wf_region = region.to_write_fonts_variation_region(&axes);
                vals.into_iter().map(move |v| (wf_region.clone(), v as i16))
            })
            .collect();

        axis_var_idxes[axis_pos] = Some(builder.add_deltas(store_deltas));
    }

    let (store, varidx_map) = builder.build();
    avar.var_store = store.into();

    // Build axis_index_map with one entry per axis in fvar order.
    let varidx_map: DeltaSetIndexMap = axis_var_idxes
        .iter()
        .map(|maybe_idx| {
            maybe_idx
                .and_then(|idx| varidx_map.get(idx))
                .map(Into::into)
                .unwrap_or(NO_VARIATION_INDEX)
        })
        .collect();

    avar.axis_index_map = varidx_map.into();

    let mut newfont = FontBuilder::new();
    newfont
        .add_table(&avar)
        .map_err(|e| BabelfontError::General(format!("Error adding avar2 table: {:#?}", e)))?;
    newfont.copy_missing_tables(fontref);

    Ok(newfont.build())
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use crate::{
        axis::CrossAxisMapping, convertors::fontir::CompilationOptions, filters::parse_location,
    };

    use super::*;

    #[test]
    fn test_avar2_opsz() {
        let font = crate::load("resources/avar2test.babelfont").unwrap();
        let bytes = crate::convertors::fontir::BabelfontIrSource::compile(
            font,
            CompilationOptions::default(),
        )
        .unwrap();
        let fontref = FontRef::new(&bytes).unwrap();
        let avar = fontref.avar().unwrap();
        let varstore = avar.var_store().unwrap().unwrap();
        assert_eq!(
            varstore.compute_delta(
                skrifa::raw::tables::variations::DeltaSetIndex { outer: 0, inner: 0 },
                &[
                    F2Dot14::from_f32(0.0),
                    F2Dot14::from_f32(0.0),
                    F2Dot14::from_f32(-1.0)
                ]
            ),
            Ok(5461)
        );
    }

    #[test]
    fn test_avar2_full() {
        let mut font = crate::load("resources/avar2test.babelfont").unwrap();
        let mappings = [
            ("wght=1", "wght=1"),
            ("wght=100", "wght=300"),
            ("wght=400,wdth=100", "wght=400,wdth=100"),
            ("wght=700", "wght=600"),
            ("wght=900", "wght=700"),
            ("wght=1000", "wght=1000"),
            ("wdth=50", "wdth=50"),
            ("wdth=75", "wdth=90"),
            ("wdth=125", "wdth=110"),
            ("wdth=150", "wdth=150"),
        ];
        font.cross_axis_mappings = mappings
            .iter()
            .map(|(input, output)| CrossAxisMapping {
                description: None,
                input: parse_location(input).unwrap(),
                output: parse_location(output).unwrap(),
            })
            .collect();
        // Compile it
        let bytes = crate::convertors::fontir::BabelfontIrSource::compile(
            font,
            CompilationOptions::default(),
        )
        .unwrap();
        let fontref = FontRef::new(&bytes).unwrap();
        let avar = fontref.avar().unwrap();
        // Assert some things
        let varstore = avar.var_store().unwrap().unwrap();
        assert_eq!(
            varstore.compute_delta(
                skrifa::raw::tables::variations::DeltaSetIndex { outer: 1, inner: 0 },
                &[
                    F2Dot14::from_f32(-0.7519),
                    F2Dot14::from_f32(1.0),
                    F2Dot14::from_f32(1.0)
                ]
            ),
            Ok(8213)
        );
    }
}
