use std::{collections::HashSet, fmt::Display};

use babelfont::{BabelfontError, Font};
use fontdrasil::coords::{ConvertSpace, DesignSpace, Location, UserSpace};
use itertools::Itertools;

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

pub(crate) fn convert_between_designspaces(
    loc: &Location<DesignSpace>,
    from_ds: &fontdrasil::types::Axes,
    to_ds: &fontdrasil::types::Axes,
    clamp_to_boundaries: bool,
) -> (Location<DesignSpace>, bool) {
    let mut clamped = false;
    // First convert to userspace
    let mut user_loc = loc.to_user(from_ds);
    // Remove all axes which do not appear in the target DS
    user_loc.retain(|tag, _| to_ds.get(tag).is_some());
    // Fill all axes which appear in the target DS but not the source DS with their default value
    for axis in to_ds.iter() {
        if !user_loc.contains(axis.tag) {
            user_loc.insert(axis.tag, axis.default);
        }
        // If we are outside the bounds of this axis, clamp if requested
        if clamp_to_boundaries && let Some(value) = user_loc.get(axis.tag) {
            if value < axis.min {
                user_loc.insert(axis.tag, axis.min);
                clamped = true;
            } else if value > axis.max {
                user_loc.insert(axis.tag, axis.max);
                clamped = true;
            }
        }
    }
    // And convert to designspace of target DS
    (user_loc.to_design(to_ds), clamped)
}


pub(crate) fn within_bounds(
    axes: &fontdrasil::types::Axes,
    loc: &fontdrasil::coords::Location<DesignSpace>,
)-> bool {
    for axis in axes.iter() {
        if let Some(value) = loc.get(axis.tag)
            && (value < axis.min.to_design(&axis.converter)
                || value > axis.max.to_design(&axis.converter))
        {
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

pub enum Strategy {
    Exact {
        layer: String,
        master_name: String,
        clamped: bool,
    },
    InterpolateOrIntermediate {
        location: Location<DesignSpace>,
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

    // let font1_axis_tags = font1.axes.iter().map(|a| a.tag).collect::<Vec<_>>();
    // let font2_axis_tags = font2.axes.iter().map(|a| a.tag).collect::<Vec<_>>();

    // let remove_not_in_f1 = |loc: Location<UserSpace>| -> Location<UserSpace> {
    //     let mut new_loc = loc.clone();
    //     new_loc.retain(|tag, _| font1_axis_tags.contains(tag));
    //     new_loc
    // };
    // let remove_not_in_f2 = |loc: Location<UserSpace>| -> Location<UserSpace> {
    //     let mut new_loc = loc.clone();
    //     new_loc.retain(|tag, _| font2_axis_tags.contains(tag));
    //     new_loc
    // };

    // For each non-sparse master in font1, we need to find something in font2 at that location
    for master_id in f1_nonsparse_master_ids.iter() {
        #[allow(clippy::unwrap_used)]
        // We know these masters are in font1 because that's where we got the IDs from
        let master = font1.masters.iter().find(|m| &m.id == master_id).unwrap();

        // What would it be in font2's designspace?
        let (loc2, clamped) = convert_between_designspaces(&master.location, &ds1, &ds2, true);
        log::trace!(
            "Looking for a match for font1 master '{}' at location {:?}",
            master.name.get_default().unwrap_or(&master.id),
            loc2
        );

        if let Some(exact) = font2.masters.iter().find(|m| m.location == loc2) {
            #[allow(clippy::unwrap_used)] // We found it
            let master_name = font2
                .masters
                .iter()
                .find(|m| m.id == exact.id)
                .unwrap()
                .name
                .get_default()
                .unwrap_or(&exact.id)
                .to_string();
            results.push(Strategy::Exact {
                layer: exact.id.clone(),
                master_name,
                clamped,
            });
            continue;
        }
        // No exact match. If this location is strictly within the designspace bounds of font2, we can interpolate
        log::trace!(
            "No exact match found for font1 master '{}' at location {:?}, checking for interpolation",
            master.id,
            loc2
        );
        results.push(Strategy::InterpolateOrIntermediate {
            location: loc2,
            clamped,
        });
    }
    Ok(results)
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
    let mut unsparsification_list = vec![];
    for f2_master in font2.masters.iter() {
        if f2_master.is_sparse(font2) {
            continue;
        } // Well thank goodness for that

        // if this is non-default for an axis we don't have in f1, ignore it
        if f2_master
            .location
            .to_user(&ds2)
            .iter()
            .any(|(tag, value)| {
                ds1.get(tag).is_none()
                    && ds2.get(tag).unwrap().default != *value
            })
        {
            log::debug!(
                "Skipping font 2 master '{}' at location {:?} because it has non-default values on axes not in font 1",
                f2_master.name.get_default().unwrap_or(&f2_master.id),
                f2_master.location.to_user(&ds2)
            );
            continue;
        }


        // Convert f2 master's location to userspace and then to f1's design space
        let (loc1_design, clamped) = convert_between_designspaces(&f2_master.location, &ds2, &ds1, true);
        if clamped {
            log::warn!(
                "Font 2 master '{}' at location {:?} is out of bounds for Font 1's designspace; skipping addition",
                f2_master.name.get_default().unwrap_or(&f2_master.id),
                f2_master.location
            );
            continue;
        }
        // AAARGH - if this includes any axes we don't have in font2, we need to add masters at min, max *and* default for those axes
        // Cartesian product time!
        let new_axes = ds1.iter()
            .filter(|a1| !ds2.iter().any(|a2| a2.tag == a1.tag))
            .map(|a| vec![
                (a.tag, a.min),
                (a.tag, a.default),
                (a.tag, a.max),
            ]);
        let mut locations_to_add = vec![];
        for new_locs in new_axes.multi_cartesian_product() {
            let mut loc = loc1_design.to_user(&ds1);
            for (tag, value) in new_locs.iter() {
                loc.insert(*tag, *value);
            }
            locations_to_add.push(loc.to_design(&ds1));
        }
        for loc1_design in locations_to_add.into_iter() {

            // Do we have one already?
            if let Some(existing_master) = font1
                .masters
                .iter()
                .find(|m| m.location == loc1_design)
            {
                // If it's sparse, we need to unsparsify it
                if f1_sparse_master_ids.contains(&existing_master.id) {
                    log::debug!(
                        "Font 2 master '{}' at location {:?} corresponds to sparse master '{}' in Font 1; unsparsifying",
                        f2_master.name.get_default().unwrap_or(&f2_master.id),
                        f2_master.location.to_user(&ds2),
                        existing_master
                            .name
                            .get_default()
                            .unwrap_or(&existing_master.id),
                    );
                    unsparsification_list
                        .push((existing_master.id.clone(), existing_master.location.clone()));
                } else {
                    log::trace!(
                        "Font 2 master '{}' at location {:?} already has a corresponding master in Font 1; skipping addition",
                        f2_master.name.get_default().unwrap_or(&f2_master.id),
                        f2_master.location.to_user(&ds2)
                    );
                }
                continue;
            }
            let master = babelfont::Master::new(
                format!("{:?}", loc1_design.to_user(&ds1)),
                uuid::Uuid::new_v4().to_string(),
                loc1_design.clone(),
            );
            log::info!(
                "Added new master '{}' at location {:?} to font 1",
                master.name.get_default().unwrap_or(&master.id),
                loc1_design.to_user(&ds1)
            );
            unsparsification_list.push((master.id.clone(), loc1_design.clone()));
            font1.masters.push(master);
        }
    }
    if !unsparsification_list.is_empty() {
        log::info!(
            "Unsparsifying {} masters in font 1 to match font 2",
            unsparsification_list.len()
        );
    }
    log::debug!("Unsparsification list: {:?}", unsparsification_list);

    for (master_id, location) in unsparsification_list.into_iter() {
        unsparsify_master(font1, &master_id, &location)?;
    }
    assert!(sanity_check(font1), "Font failed sanity check after adding needed masters");

    Ok(())
}

fn unsparsify_master(
    font: &mut babelfont::Font,
    master_id: &str,
    location: &Location<DesignSpace>,
) -> Result<(), FontmergeError> {
    assert!(sanity_check(font), "Font failed sanity check before unsparsifying master '{}'", master_id);
    if !font.masters.iter().any(|m| m.id == master_id) {
        return Err(FontmergeError::Font(format!(
            "Cannot unsparsify master '{}': not found in font",
            master_id
        )));
    }

    log::debug!(
        "Unsparsifying master '{}' ({}) at location {:?}",
        font.masters.iter().find(|m| m.id == master_id).unwrap().name.get_default().unwrap_or(&master_id.to_string()),
        master_id,
        location
    );
    let mut layer_additions = vec![];
    let mut promotion_list = vec![];

    // Interpolate layers
    for glyph in font.glyphs.iter() {
        // If there already exists a *real* layer for this master, bug out
        if glyph
            .layers
            .iter()
            .any(|l| l.master == babelfont::LayerType::DefaultForMaster(master_id.to_string()))
        {
            log::debug!(
                "Glyph '{}' already has a full layer for master '{}', skipping",
                glyph.name,
                master_id
            );
            continue;
        }
        // If there already exists a sparse layer for this master, promote it and move on
        let mut found_sparse = false;
        for layer in glyph.layers.iter() { 
            if layer.location == Some(location.clone())
            {
                log::debug!(
                    "Promoting sparse layer for glyph '{}' at location {:?} to full layer",
                    glyph.name,
                    location
                );
                promotion_list.push((glyph.name.clone(), layer.id.clone()));
                found_sparse = true;
                break;
            }
        }
        if found_sparse {
            continue;
        }
        let mut interpolated_layer =
            font.interpolate_glyph(&glyph.name, location).map_err(|e| {
                FontmergeError::Font(format!(
                    "Failed to interpolate glyph '{}' for new master at location {:?}: {}",
                    glyph.name, location, e
                ))
            })?;
        // Give it an id
        interpolated_layer.master = babelfont::LayerType::DefaultForMaster(master_id.to_string());
        interpolated_layer.id = Some(master_id.to_string());
        // Check we don't already have a layer at this location
        layer_additions.push((glyph.name.clone(), interpolated_layer));
    }

    for (glyph_name, layer_id) in promotion_list.into_iter() {
        let glyph = font.glyphs.get_mut(&glyph_name).unwrap();
        let layer = glyph
            .layers
            .iter_mut()
            .find(|l| l.id == layer_id)
            .unwrap();
        layer.master = babelfont::LayerType::DefaultForMaster(master_id.to_string());
        layer.id = Some(master_id.to_string());
    }

    for (glyph_name, layer) in layer_additions.into_iter() {
        if let Some(glyph) = font.glyphs.get_mut(&glyph_name) {
            glyph.layers.push(layer);
        }
    }
    assert!(
        !font
            .masters
            .iter()
            .find(|m| m.id == master_id)
            .unwrap()
            .is_sparse(font)
    );
    assert!(sanity_check(font), "Font failed sanity check after unsparsifying master '{}'", master_id);
    Ok(())
}

pub(crate) fn sanity_check(font: &Font) -> bool {
    // Check master locations are distinct
    let mut ok = true;
    let mut seen_locations = HashSet::new();
    for master in font.masters.iter() {
        if seen_locations.contains(&master.location) {
            log::error!(
                "Sanity check: master '{}' has the same location {:?} as another master",
                master.name.get_default().unwrap_or(&master.id),
                master.location
            );
            ok = false;
        } else {
            seen_locations.insert(master.location.clone());
        }
    }
    for glyph in font.glyphs.iter() {
        // Check that non-master layer locations do not duplicate master locations
        for layer in glyph.layers.iter() {
            if let Some(loc) = &layer.location
                && seen_locations.contains(loc)
            {
                log::error!(
                    "Sanity check: glyph '{}' has a non-master layer at location {:?} which duplicates a master location",
                    glyph.name,
                    loc
                );
                ok = false;
            }
        }
        // Check that glyphs only have one layer per master
        let layer_ids = glyph
            .layers
            .iter()
            .filter_map(|l| match &l.master {
                babelfont::LayerType::DefaultForMaster(id) => Some(id),
                babelfont::LayerType::AssociatedWithMaster(id) => None,
                babelfont::LayerType::FreeFloating => None,
            })
            .collect::<Vec<&String>>();
        let mut seen_layer_ids = HashSet::new();
        for id in layer_ids.iter() {
            if seen_layer_ids.contains(id) {
                log::error!(
                    "Sanity check: glyph '{}' has multiple layers for master ID '{}'",
                    glyph.name,
                    id
                );
                ok = false;
            } else {
                seen_layer_ids.insert(id);
            }
        }
    }

    ok
}
