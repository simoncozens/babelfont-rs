use std::collections::{BTreeSet, HashMap};

use fea_rs_ast::Metric;
use fontdrasil::coords::{
    CoordConverter, DesignCoord, NormalizedCoord, NormalizedLocation, UserCoord, UserLocation,
};
use skrifa::{
    MetadataProvider,
    raw::{
        ReadError, TableProvider as _,
        tables::{
            gpos::DeviceOrVariationIndex,
            variations::{DeltaSetIndex, ItemVariationStore},
        },
    },
};

use crate::{SimpleUserLocation, UncompileContext};

/// Returns a set of fontdrasil `Axes` for the font
/// Stolen from fontspector
pub(crate) fn fontdrasil_axes(
    font: &skrifa::FontRef,
) -> Result<Option<fontdrasil::types::Axes>, ReadError> {
    if font.fvar().is_err() {
        return Ok(None);
    }
    let per_axis_maps = if let Ok(segments) = font.avar().map(|x| x.axis_segment_maps()) {
        segments.iter().collect::<Result<Vec<_>, _>>()?
    } else {
        vec![]
    };
    Ok(Some(
        font.axes()
            .iter()
            .enumerate()
            .map(|(ix, axis)| {
                let min = UserCoord::new(axis.min_value() as f64);
                let default = UserCoord::new(axis.default_value() as f64);
                let max = UserCoord::new(axis.max_value() as f64);
                #[allow(clippy::unwrap_used)]
                let mut fd_axis = fontdrasil::types::Axis {
                    converter: CoordConverter::default_normalization(min, default, max),
                    hidden: axis.is_hidden(),
                    // Argh version incompatibilities
                    tag: fontdrasil::types::Tag::new_checked(axis.tag().to_string().as_bytes())
                        .unwrap(),
                    name: axis.tag().to_string(),
                    min,
                    default,
                    max,
                    localized_names: HashMap::new(), // Let's not
                };
                if let Some(map) = per_axis_maps.get(ix) {
                    let desired_mapping: Vec<(
                        fontdrasil::coords::Coord<fontdrasil::coords::UserSpace>,
                        fontdrasil::coords::Coord<fontdrasil::coords::DesignSpace>,
                    )> = map
                        .axis_value_maps
                        .iter()
                        .map(|mapping| {
                            let from = mapping.from_coordinate().to_f32();
                            let to = mapping.to_coordinate().to_f32();
                            // These are both normalized coordinates. Turn the `from` back into
                            // userspace using default normalization
                            let user_from =
                                NormalizedCoord::new(from as f64).to_user(&fd_axis.converter);
                            // Let's pretend design space is just normalized space
                            let design_to = DesignCoord::new(to as f64);
                            (user_from, design_to)
                        })
                        .collect();
                    let default_idx = desired_mapping
                        .iter()
                        .position(|(_, to)| to.to_f64() == 0.0)
                        .unwrap_or(0);
                    if let Ok(converter) = CoordConverter::new(desired_mapping, default_idx) {
                        fd_axis.converter = converter;
                    }
                }
                fd_axis
            })
            .collect(),
    ))
}

impl<'a> UncompileContext<'a> {
    pub(crate) fn variation_store(&self) -> Result<Option<ItemVariationStore<'a>>, ReadError> {
        self.gdef
            .as_ref()
            .and_then(|g| g.item_var_store())
            .transpose()
    }

    fn interesting_locations(
        &self,
    ) -> Result<BTreeSet<(UserLocation, NormalizedLocation)>, ReadError> {
        let mut master_locations: BTreeSet<(UserLocation, NormalizedLocation)> = BTreeSet::new();
        let variations = self.variation_store()?.unwrap();
        let regions = variations.variation_region_list()?;
        for region in regions.variation_regions().iter().flatten() {
            let location: NormalizedLocation = region
                .region_axes()
                .iter()
                .map(|x| NormalizedCoord::new(x.peak_coord().to_f32() as f64))
                .zip(self.axis_tags.iter())
                .map(|(coord, tag)| (*tag, coord))
                .collect();
            if let Some(axes) = &self.axes {
                master_locations.insert((
                    location.convert(axes).map_err(|_e| {
                        ReadError::MalformedData("Failed to convert variation location")
                    })?,
                    location,
                ));
            }
        }
        // Insert the default
        if let Some(axes) = &self.axes {
            let location: NormalizedLocation = self
                .axis_tags
                .iter()
                .map(|tag| (*tag, NormalizedCoord::new(0.0)))
                .collect();
            master_locations.insert((
                location.convert(axes).map_err(|_e| {
                    ReadError::MalformedData("Failed to convert variation location")
                })?,
                location,
            ));
        }
        Ok(master_locations)
    }

    pub(crate) fn resolve_pos_with_variations(
        &self,
        default: i16,
        device: Option<Result<DeviceOrVariationIndex<'_>, ReadError>>,
    ) -> Result<Metric, ReadError> {
        let mut variations: Vec<(SimpleUserLocation, i16)> = Vec::new();
        if let Some(ivs) = self.variation_store()?
            && let Some(Ok(DeviceOrVariationIndex::VariationIndex(varix))) = device
        {
            variations = self
                .interesting_locations()?
                .iter()
                .map(|(user_loc, norm_loc)| {
                    let coords = norm_loc
                        .iter()
                        .map(|(_tag, coord)| coord.to_f2dot14())
                        .collect::<Vec<_>>();
                    let delta = ivs
                        .compute_delta(
                            DeltaSetIndex {
                                outer: varix.delta_set_outer_index(),
                                inner: varix.delta_set_inner_index(),
                            },
                            &coords,
                        )
                        .unwrap_or_default();
                    let simple_user_loc: SimpleUserLocation = user_loc
                        .iter()
                        .map(|(tag, coord)| (tag.to_string().into(), coord.to_f64() as i16))
                        .collect();

                    (simple_user_loc, (default as i32 + delta) as i16)
                })
                .collect();
        }

        if variations.len() < 2 {
            // we always have the default
            Ok(Metric::Scalar(default))
        } else {
            Ok(Metric::Variable(variations))
        }
    }
}
