use std::{collections::HashSet, fmt::Display};

use babelfont::BabelfontError;
use fontdrasil::coords::{DesignSpace, Location, UserSpace};

use crate::error::FontmergeError;

pub(crate) fn fontdrasil_axes(
    axes: &[babelfont::Axis],
) -> Result<fontdrasil::types::Axes, FontmergeError> {
    let axes = axes
        .iter()
        .map(|ax| ax.clone().try_into())
        .collect::<Result<Vec<fontdrasil::types::Axis>, _>>()
        .map_err(|e: BabelfontError| FontmergeError::Font(format!("Axis conversion error: {}", e)));
    Ok(fontdrasil::types::Axes::new(axes?))
}

pub enum Strategy {
    Exact {
        layer: String,
        master_name: String,
        clamped: bool,
    },
    InterpolateOrIntermediate {
        location: Location<UserSpace>,
        clamped: bool,
    },
    Failed(String),
}

impl Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Strategy::Exact {
                layer,
                master_name,
                clamped,
            } => write!(
                f,
                "use font 2's layer '{}' from master {} {}",
                layer,
                master_name,
                if *clamped { " (clamped)" } else { "" }
            ),
            Strategy::InterpolateOrIntermediate { location, clamped } => write!(
                f,
                "interpolate at location {:?} (or try an intermediate layer){}",
                location,
                if *clamped { " (clamped)" } else { "" }
            ),
            Strategy::Failed(reason) => write!(f, "give up: {}", reason),
        }
    }
}

pub(crate) fn compatible_location(
    f1_location: &Location<UserSpace>,
    f2_location: &Location<UserSpace>,
) -> bool {
    log::trace!("Considering locations {:?}", f2_location);
    for (&tag, &value) in f1_location.iter() {
        match f2_location.get(tag) {
            None => {
                // Font 2 doesn't have that axis; this shouldn't happen, we removed the axis already
                continue;
            }
            Some(v2) if v2 == value => continue,
            _ => {
                log::trace!(
                    "Incompatible location for tag '{}': f1 has {}, f2 has {:?}",
                    tag,
                    value.to_f64(),
                    f2_location.get(tag).map(|v| v.to_f64())
                );
                return false;
            }
        }
    }
    true
}

pub(crate) fn map_designspaces(
    font1: &babelfont::Font,
    f1_nonsparse_master_ids: &[String],
    font2: &babelfont::Font,
) -> Result<Vec<Strategy>, FontmergeError> {
    let ds1 = fontdrasil_axes(&font1.axes)?;
    let ds2 = fontdrasil_axes(&font2.axes)?;
    let mut results = vec![];

    let f1_axes_not_in_f2 = font1
        .axes
        .iter()
        .filter(|a1| !font2.axes.iter().any(|a2| a2.tag == a1.tag))
        .collect::<Vec<&babelfont::Axis>>();
    if !f1_axes_not_in_f2.is_empty() {
        log::warn!(
            "Font 1 has axes not present in Font 2: {}. These will be ignored when matching masters, and no variation will be present on these glyphs.",
            f1_axes_not_in_f2
                .iter()
                .map(|a| a.tag.to_string())
                .collect::<Vec<String>>()
                .join(", ")
        );
    }
    let f2_axes_not_in_f1 = font2
        .axes
        .iter()
        .filter(|a2| !font1.axes.iter().any(|a1| a1.tag == a2.tag))
        .collect::<Vec<&babelfont::Axis>>();
    if !f2_axes_not_in_f1.is_empty() {
        log::warn!(
            "Font 2 has axes not present in Font 1: {}. These will be ignored when matching masters; the default location will be used.",
            f2_axes_not_in_f1
                .iter()
                .map(|a| a.tag.to_string())
                .collect::<Vec<String>>()
                .join(", ")
        );
    }

    let font1_axis_tags = font1.axes.iter().map(|a| a.tag).collect::<Vec<_>>();
    let font2_axis_tags = font2.axes.iter().map(|a| a.tag).collect::<Vec<_>>();

    let remove_not_in_f1 = |loc: Location<UserSpace>| -> Location<UserSpace> {
        let mut new_loc = loc.clone();
        new_loc.retain(|tag, _| font1_axis_tags.contains(tag));
        new_loc
    };
    let remove_not_in_f2 = |loc: Location<UserSpace>| -> Location<UserSpace> {
        let mut new_loc = loc.clone();
        new_loc.retain(|tag, _| font2_axis_tags.contains(tag));
        new_loc
    };

    let font2_masters_and_locations = font2
        .masters
        .iter()
        .map(|m| (m.id.clone(), remove_not_in_f1(m.location.to_user(&ds2))))
        .collect::<Vec<_>>();

    // For each non-sparse master in font1, we need to find something in font2 at that location
    for master_id in f1_nonsparse_master_ids.iter() {
        #[allow(clippy::unwrap_used)]
        // We know these masters are in font1 because that's where we got the IDs from
        let master = font1.masters.iter().find(|m| &m.id == master_id).unwrap();
        // Master location is in designspace, we need it in userspace
        let loc1 = &master.location.to_user(&ds1);
        // Remove all axes not in font 2 from loc1
        let mut loc1 = remove_not_in_f2(loc1.clone());
        log::trace!(
            "Looking for a match for font1 master '{}' at location {:?}",
            master.name.get_default().unwrap_or(&master.id),
            loc1
        );

        let mut has_clamp = false;
        loc1 = loc1
            .iter()
            .map(|(tag, value)| {
                if let Some(axis) = ds2.get(tag) {
                    let clamped_value = if *value < axis.min {
                        axis.min
                    } else if *value > axis.max {
                        axis.max
                    } else {
                        *value
                    };
                    if clamped_value != *value {
                        log_once::debug_once!(
                            "Clamping location on axis '{}': {} -> {}",
                            tag,
                            value.to_f64(),
                            clamped_value.to_f64()
                        );
                        has_clamp = true;
                    }
                    (*tag, clamped_value)
                } else {
                    (*tag, *value)
                }
            })
            .collect();
        log::debug!("Clamped location: {:?}", loc1);
        if let Some(exact) = font2_masters_and_locations
            .iter()
            .find(|(_, loc2)| compatible_location(&loc1, loc2))
        {
            #[allow(clippy::unwrap_used)] // We found it
            let master_name = font2
                .masters
                .iter()
                .find(|m| m.id == exact.0)
                .unwrap()
                .name
                .get_default()
                .unwrap_or(&exact.0)
                .to_string();
            results.push(Strategy::Exact {
                layer: exact.0.clone(),
                master_name,
                clamped: has_clamp,
            });
            continue;
        }
        // No exact match. If this location is strictly within the designspace bounds of font2, we can interpolate
        log::trace!(
            "No exact match found for font1 master '{}' at location {:?}, checking for interpolation",
            master.id,
            loc1
        );
        if within_bounds(&ds2, &loc1) {
            // Fill out any axes that font2 has but font1 doesn't with the default value
            let mut full_loc1 = loc1.clone();
            for axis in f2_axes_not_in_f1.iter() {
                if let Some(default) = axis.default {
                    full_loc1.insert(axis.tag, default);
                }
            }
            results.push(Strategy::InterpolateOrIntermediate {
                location: full_loc1,
                clamped: has_clamp,
            });
        } else {
            results.push(Strategy::Failed(format!(
                "No compatible master found in font2 for font1 master at location {:?}",
                loc1
            )));
        }
    }
    Ok(results)
}

fn within_bounds(ds2: &fontdrasil::types::Axes, loc1: &Location<UserSpace>) -> bool {
    loc1.iter().all(|(tag, value)| {
        if let Some(axis) = ds2.get(tag) {
            let ok = *value >= axis.min && *value <= axis.max;
            log::trace!(
                "Checking axis '{}': {} ({}..{})",
                tag,
                ok,
                axis.min.to_f64(),
                axis.max.to_f64()
            );
            ok
        } else {
            true // Axis not in font2, ignore
        }
    })
}

// We have to insert full, interpolated masters for every point that font2 has a master. Why?
// If I have font1 with masters at wght=400 and wght=1000, and font2 has range 400-700,
// you might think you could put font2's 400 at 400 and its 700 at 1000, and then add an intermediate
// layer at 700 so there's no variation between 700-1000.
// Very clever - but no! Because of kerning. Intermediate layers don't hold kerning values, so
// in the two-master-with-intermediate-layer setup, your outlines vary between 400-700 but your
// kerning varies between 400-1000, which is wrong.
// Also, avar tables. Suppose font2 has an avar mapping which maps 600 to 550 in user space, and
// font1 does not. We can't add an avar to font1 because that'll bend its designspace. How do we
// represent the mapping? We can't, unless we have a full master at 600 in font1.
// So: full masters for each master in font2 *and* every point in the avar map.
pub(crate) fn add_needed_masters(
    font1: &mut babelfont::Font,
    font2: &babelfont::Font,
) -> Result<(), FontmergeError> {
    let ds1 = fontdrasil_axes(&font1.axes)?;
    let ds2 = fontdrasil_axes(&font2.axes)?;
    let f1_sparse_master_ids = font1
        .masters
        .iter()
        .filter(|m| m.is_sparse(font1))
        .map(|m| m.id.clone())
        .collect::<HashSet<_>>();
    let f1_master_locations = font1
        .masters
        .iter()
        .map(|m| m.location.to_user(&ds1))
        .collect::<HashSet<_>>();
    let mut unsparsification_list = vec![];
    for f2_master in font2.masters.iter() {
        if f2_master.is_sparse(font2) {
            continue;
        } // Well thank goodness for that

        // Convert f2 master's location to userspace and then to f1's design space

        // First prep the location by dropping axes not in font1 and filling in defaults for axes in font1 not in font2
        let mut f2_loc_user = f2_master.location.to_user(&ds2);
        f2_loc_user = f2_loc_user
            .iter()
            .filter_map(|(tag, value)| {
                if ds1.get(tag).is_some() {
                    Some((*tag, *value))
                } else {
                    None
                }
            })
            .collect();
        for axis in font1.axes.iter() {
            if ds2.get(&axis.tag).is_none()
                && let Some(default) = axis.default
            {
                f2_loc_user.insert(axis.tag, default);
            }
        }
        // Now check if within bounds of font1
        if !within_bounds(&ds1, &f2_loc_user) {
            log::warn!(
                "Font 2 master '{}' at location {:?} is out of bounds for Font 1's designspace; skipping addition",
                f2_master.name.get_default().unwrap_or(&f2_master.id),
                f2_loc_user
            );
            continue;
        }
        // Do we have one already?
        if let Some(existing_master) = font1
            .masters
            .iter()
            .find(|m| m.location.to_user(&ds1) == f2_loc_user)
        {
            // If it's sparse, we need to unsparsify it
            if f1_sparse_master_ids.contains(&existing_master.id) {
                log::debug!(
                    "Font 2 master '{}' at location {:?} corresponds to sparse master '{}' in Font 1; unsparsifying",
                    f2_master.name.get_default().unwrap_or(&f2_master.id),
                    f2_loc_user,
                    existing_master.name.get_default().unwrap_or(&existing_master.id),
                );
                unsparsification_list.push((existing_master.id.clone(), existing_master.location.clone()));
            } else {
                log::trace!(
                    "Font 2 master '{}' at location {:?} already has a corresponding master in Font 1; skipping addition",
                    f2_master.name.get_default().unwrap_or(&f2_master.id),
                    f2_loc_user
                );
            }
            continue;
        }
        let location = f2_loc_user.to_design(&ds1);
        let master = babelfont::Master::new(
            f2_master.name.clone(),
            f2_master.id.clone(),
            location.clone(),
        );
        log::debug!(
            "Added new master '{}' at location {:?} to font 1",
            master.name.get_default().unwrap_or(&master.id),
            location
        );
        font1.masters.push(master);
        unsparsification_list.push((f2_master.id.clone(), location.clone()));
    }
    log::debug!("Unsparsification list: {:?}", unsparsification_list);

    for (master_id, location) in unsparsification_list.into_iter() {
        unsparsify_master(font1, &master_id, &location)?;
    }

    Ok(())
}

fn unsparsify_master(
    font: &mut babelfont::Font,
    master_id: &str,
    location: &Location<DesignSpace>,
) -> Result<(), FontmergeError> {
    log::debug!(
        "Unsparsifying master '{}' at location {:?}",
        master_id,
        location
    );
    if !font.masters.iter().any(|m| m.id == master_id) {
        return Err(FontmergeError::Font(format!(
            "Cannot unsparsify master '{}': not found in font",
            master_id
        )));
    }
    let mut layer_additions = vec![];

    // Interpolate kerning
    // Interpolate layers
    for glyph in font.glyphs.iter() {
        let mut interpolated_layer =
            font.interpolate_glyph(&glyph.name, location)
                .map_err(|e| {
                    FontmergeError::Font(format!(
                        "Failed to interpolate glyph '{}' for new master at location {:?}: {}",
                        glyph.name, location, e
                    ))
                })?;
        // Give it an id
        interpolated_layer.master =
            babelfont::LayerType::DefaultForMaster(master_id.to_string());
        interpolated_layer.id = Some(master_id.to_string());
        // Check we don't already have a layer at this location
        layer_additions.push((glyph.name.clone(), interpolated_layer));
    }

    for (glyph_name, layer) in layer_additions.into_iter() {
        if let Some(glyph) = font.glyphs.get_mut(&glyph_name) {
            glyph.layers.push(layer);
        }
    }
    assert!(!font.masters.iter().find(|m| m.id == master_id).unwrap().is_sparse(font));
    Ok(())
}
