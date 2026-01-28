use std::{collections::HashSet, fmt::Display};

use babelfont::{BabelfontError, Font};
use fontdrasil::coords::{DesignSpace, Location};
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
) -> Result<(Location<DesignSpace>, bool), crate::error::FontmergeError> {
    let mut clamped = false;
    // First convert to userspace
    let mut user_loc = loc.to_user(from_ds)?;
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
    Ok((user_loc.to_design(to_ds)?, clamped))
}

pub(crate) fn within_bounds(
    axes: &fontdrasil::types::Axes,
    loc: &fontdrasil::coords::Location<DesignSpace>,
) -> bool {
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
    #[allow(dead_code)]
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

    // For each non-sparse master in font1, we need to find something in font2 at that location
    for master_id in f1_nonsparse_master_ids.iter() {
        #[allow(clippy::unwrap_used)]
        // We know these masters are in font1 because that's where we got the IDs from
        let master = font1.masters.iter().find(|m| &m.id == master_id).unwrap();

        // What would it be in font2's designspace?
        let (loc2, clamped) = convert_between_designspaces(&master.location, &ds1, &ds2, true)?;
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

pub(crate) fn add_needed_masters(
    font1: &mut babelfont::Font,
    font2: &mut babelfont::Font,
) -> Result<(), FontmergeError> {
    let ds1 = fontdrasil_axes(&font1.axes)?;
    let ds2 = fontdrasil_axes(&font2.axes)?;

    assert!(
        sanity_check(font1),
        "Font failed sanity check *before* adding needed masters, file was insane"
    );

    // If there are axis mappings in font2 that don't exist in font1 we need to
    // make intermediate masters for them in font2! This may get messy.
    let mut intermediate_mappings = vec![];
    for f2_axis in font2.axes.iter() {
        let Some(f1_axis) = font1.axes.iter().find(|a| a.tag == f2_axis.tag) else {
            continue;
        };
        let Some(f2_map) = f2_axis.map.as_ref() else {
            continue;
        };
        for (user, design) in f2_map.iter() {
            if f1_axis.userspace_to_designspace(*user)? != *design {
                // If the problem is at the min or max, ignore it, we're mapping those already
                #[allow(clippy::unwrap_used)]
                // We know these are Some() because we converted to fontdrasil previously
                if (user.to_f64() - f2_axis.min.unwrap().to_f64()).abs() < f64::EPSILON
                    || (user.to_f64() - f2_axis.max.unwrap().to_f64()).abs() < f64::EPSILON
                {
                    continue;
                }
                intermediate_mappings.push((f2_axis.tag, design));
            }
        }
    }
    let mut already_added = font2
        .masters
        .iter()
        .filter(|m| !m.is_sparse(font2))
        .map(|m| m.location.clone())
        .collect::<HashSet<_>>();
    for (tag, design) in intermediate_mappings {
        log::info!(
            "Adding intermediate masters to font 2 for axis '{}' at design value {} to satisfy avar table change",
            tag,
            design.to_f64()
        );
        // I *think* we need to add a master for *each master* in font2 already
        let new_masters = font2
            .masters
            .iter()
            .filter(|m| !m.is_sparse(font2))
            .filter_map(|m| {
                let mut new_m = m.clone();
                new_m.location.insert(tag, *design);
                // Only add if we don't already have one at this location
                if already_added.contains(&new_m.location) {
                    log::debug!(" Already have master at {:?}", new_m.location);
                    return None;
                }
                log::debug!(" Adding intermediate master at {:?}", new_m.location);
                already_added.insert(new_m.location.clone());
                Some(new_m)
            })
            .collect::<Vec<_>>();
        // Interpolate!
        let mut new_layer_list = Vec::new();
        for new_master in new_masters.iter() {
            for glyph in font2.glyphs.iter() {
                let mut new_layer = font2.interpolate_glyph(&glyph.name, &new_master.location)?;
                new_layer.master = babelfont::LayerType::DefaultForMaster(new_master.id.clone());
                new_layer_list.push((glyph.name.clone(), new_layer));
            }
        }
        // Insert new layers
        for (glyph_name, new_layer) in new_layer_list.into_iter() {
            if let Some(glyph) = font2.glyphs.iter_mut().find(|g| g.name == glyph_name) {
                glyph.layers.push(new_layer);
            }
        }

        font2.masters.extend(new_masters);
    }
    log::debug!(
        "font2 master situation now: {}",
        font2
            .masters
            .iter()
            .map(|m| format!(
                "'{}' at {:?} {}",
                m.name.get_default().unwrap_or(&m.id),
                m.location,
                if m.is_sparse(font2) { "(sparse)" } else { "" }
            ))
            .collect::<Vec<String>>()
            .join(", ")
    );

    for f2_master in font2.masters.iter() {
        if f2_master.is_sparse(font2) {
            continue;
        } // Well thank goodness for that

        // if this is non-default for an axis we don't have in f1, ignore it
        #[allow(clippy::unwrap_used)] // We know these axes are in ds2
        if f2_master
            .location
            .to_user(&ds2)?
            .iter()
            .any(|(tag, value)| ds1.get(tag).is_none() && ds2.get(tag).unwrap().default != *value)
        {
            log::debug!(
                "Skipping font 2 master '{}' at location {:?} because it has non-default values on axes not in font 1",
                f2_master.name.get_default().unwrap_or(&f2_master.id),
                f2_master.location.to_user(&ds2)
            );
            continue;
        }

        // Convert f2 master's location to userspace and then to f1's design space
        let (loc1_design, clamped) =
            convert_between_designspaces(&f2_master.location, &ds2, &ds1, true)?;
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
        let new_axes = ds1
            .iter()
            .filter(|a1| !ds2.iter().any(|a2| a2.tag == a1.tag))
            .map(|a| vec![(a.tag, a.min), (a.tag, a.default), (a.tag, a.max)]);
        let mut locations_to_add = vec![];
        for new_locs in new_axes.multi_cartesian_product() {
            let mut loc = loc1_design.to_user(&ds1)?;
            for (tag, value) in new_locs.iter() {
                loc.insert(*tag, *value);
            }
            locations_to_add.push(loc.to_design(&ds1)?);
        }
        for loc1_design in locations_to_add.into_iter() {
            // Do we have one already?
            if font1.masters.iter().any(|m| m.location == loc1_design) {
                continue;
            }
            let master = babelfont::Master::new(
                format!("{:?}", loc1_design.to_user(&ds1)),
                uuid::Uuid::new_v4().to_string(),
                loc1_design.clone(),
            );
            log::info!(
                "Added new master '{}' at location {:?} (font2 location {:?}) to font 1",
                master.name.get_default().unwrap_or(&master.id),
                loc1_design.to_user(&ds1),
                f2_master.location
            );
            font1.masters.push(master);
        }
    }

    assert!(
        sanity_check(font1),
        "Font failed sanity check after adding needed masters"
    );

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
            if layer.is_background {
                continue;
            } // I don't care about background layers here
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
            .filter(|layer| !layer.is_background)
            .filter_map(|l| match &l.master {
                babelfont::LayerType::DefaultForMaster(id) => Some(id),
                babelfont::LayerType::AssociatedWithMaster(_) => None,
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
