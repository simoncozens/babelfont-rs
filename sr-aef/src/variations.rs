use std::collections::{BTreeSet, HashMap};

use fea_rs_ast::{Metric, ordered_float::OrderedFloat};
use fontdrasil::coords::{
    CoordConverter, DesignCoord, NormalizedCoord, NormalizedLocation, UserCoord, UserLocation,
};
use skrifa::{
    MetadataProvider,
    raw::{
        ReadError, TableProvider as _,
        tables::{
            gpos::DeviceOrVariationIndex,
            layout::ConditionFormat1,
            variations::{DeltaSetIndex, ItemVariationStore},
        },
        types::F2Dot14,
    },
};

use crate::{SimpleUserLocation, UncompileContext};

/// Convert a skrifa Tag to a fontdrasil Tag through their common string representation.
fn to_fd_tag(tag: skrifa::Tag) -> fontdrasil::types::Tag {
    fontdrasil::types::Tag::new_checked(tag.to_string().as_bytes()).unwrap()
}

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
                if let Some(map) = per_axis_maps
                    .get(ix)
                    .filter(|map| !map.axis_value_maps.is_empty())
                {
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

    fn variation_locations(
        &self,
        index: DeltaSetIndex,
    ) -> Result<BTreeSet<(UserLocation, Vec<F2Dot14>)>, ReadError> {
        let mut locations: BTreeSet<(UserLocation, Vec<F2Dot14>)> = BTreeSet::new();
        let variations = self.variation_store()?.unwrap();
        let regions = variations.variation_region_list()?.variation_regions();
        let mut normalized_locations = vec![];
        if index != DeltaSetIndex::NO_VARIATION_INDEX
            && let Some(data) = variations.item_variation_data().get(index.outer as usize)
        {
            let data = data?;
            for (region_index, delta) in data
                .region_indexes()
                .iter()
                .zip(data.delta_set(index.inner))
            {
                if delta == 0 {
                    continue;
                }
                let region_axes = regions.get(region_index.get() as usize)?.region_axes();
                let peak: Vec<F2Dot14> = region_axes
                    .iter()
                    .map(|region_axis| region_axis.peak_coord())
                    .collect();
                for (axis, region_axis) in region_axes.iter().enumerate() {
                    for bound in [region_axis.start_coord(), region_axis.end_coord()] {
                        if bound != F2Dot14::ZERO && bound != region_axis.peak_coord() {
                            let mut location = peak.clone();
                            location[axis] = bound;
                            normalized_locations.push(location);
                        }
                    }
                }
                normalized_locations.push(peak);
            }
        }
        for coords in normalized_locations {
            let location: NormalizedLocation = coords
                .iter()
                .map(|coord| NormalizedCoord::new(coord.to_f32() as f64))
                .zip(self.axis_tags.iter())
                .map(|(coord, tag)| (to_fd_tag(*tag), coord))
                .collect();
            if let Some(axes) = &self.axes {
                locations.insert((
                    location.convert(axes).map_err(|_e| {
                        ReadError::MalformedData("Failed to convert variation location")
                    })?,
                    coords,
                ));
            }
        }
        // Insert the default
        if let Some(axes) = &self.axes {
            let location: NormalizedLocation = self
                .axis_tags
                .iter()
                .map(|tag| (to_fd_tag(*tag), NormalizedCoord::new(0.0)))
                .collect();
            locations.insert((
                location.convert(axes).map_err(|_e| {
                    ReadError::MalformedData("Failed to convert variation location")
                })?,
                vec![F2Dot14::ZERO; self.axis_tags.len()],
            ));
        }
        Ok(locations)
    }

    pub(crate) fn resolve_condition(
        &self,
        condition: &ConditionFormat1,
    ) -> Option<(String, f32, f32)> {
        let tag = self.axis_tags.get(condition.axis_index() as usize)?;
        let axis = self.axes.as_ref()?.get(&to_fd_tag(*tag))?;
        let to_user = |value: F2Dot14| {
            let user = NormalizedCoord::new(value.to_f32() as f64)
                .to_user(&axis.converter)
                .to_f64();
            let rounded = UserCoord::new(user.round());
            if rounded
                .to_normalized(&axis.converter)
                .to_f2dot14()
                .to_bits()
                == value.to_bits()
            {
                rounded.to_f64() as f32
            } else {
                user as f32
            }
        };
        Some((
            tag.to_string(),
            to_user(condition.filter_range_min_value()),
            to_user(condition.filter_range_max_value()),
        ))
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
            let index = DeltaSetIndex {
                outer: varix.delta_set_outer_index(),
                inner: varix.delta_set_inner_index(),
            };
            variations = self
                .variation_locations(index)?
                .iter()
                .map(|(user_loc, coords)| {
                    let delta = ivs.compute_delta(index, coords).unwrap_or_default();
                    let simple_user_loc: SimpleUserLocation = user_loc
                        .iter()
                        .map(|(tag, coord)| {
                            let axis = self.axes.as_ref().and_then(|axes| axes.get(tag));
                            let coord = (0..4)
                                .map(|digits| {
                                    let scale = 10f64.powi(digits);
                                    UserCoord::new((coord.to_f64() * scale).round() / scale)
                                })
                                .find(|rounded| {
                                    axis.is_some_and(|axis| {
                                        rounded.to_normalized(&axis.converter).to_f2dot14()
                                            == coord.to_normalized(&axis.converter).to_f2dot14()
                                    })
                                })
                                .unwrap_or(*coord);
                            (tag.to_string().into(), OrderedFloat(coord.to_f64()))
                        })
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
