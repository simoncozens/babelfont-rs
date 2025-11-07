use std::fmt::Display;

use babelfont::BabelfontError;
use fontdrasil::coords::{Location, UserSpace};

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
    Exact(String),
    InterpolateOrIntermediate(Location<UserSpace>),
    Failed(String),
}

impl Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Strategy::Exact(id) => write!(f, "Exact master with ID '{}'", id),
            Strategy::InterpolateOrIntermediate(loc) => write!(
                f,
                "Interpolate at location {:?} (or try intermediate layer)",
                loc
            ),
            Strategy::Failed(reason) => write!(f, "Failed: {}", reason),
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
    font2: &babelfont::Font,
    allow_clamping: bool,
) -> Result<Vec<Strategy>, FontmergeError> {
    let ds1 = fontdrasil_axes(&font1.axes)?;
    let ds2 = fontdrasil_axes(&font2.axes)?;
    let f1_masters = &font1.masters;
    let mut results = vec![];

    let f1_axes_not_in_f2 = font1
        .axes
        .iter()
        .filter(|a1| !font2.axes.iter().any(|a2| a2.tag == a1.tag))
        .collect::<Vec<&babelfont::Axis>>();
    if !f1_axes_not_in_f2.is_empty() {
        log::warn!(
            "Font 1 has axes not present in Font 2: {}. These will be ignored when matching masters, and no variation will be present on these glyphs.",
            f1_axes_not_in_f2.iter().map(|a| a.tag.to_string()).collect::<Vec<String>>().join(", ")
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
            f2_axes_not_in_f1.iter().map(|a| a.tag.to_string()).collect::<Vec<String>>().join(", ")
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
    for master in f1_masters.iter().filter(|m| !m.is_sparse(font1)) {
        // Master location is in designspace, we need it in userspace
        let loc1 = &master.location.to_user(&ds1);
        // Remove all axes not in font 2 from loc1
        let mut loc1 = remove_not_in_f2(loc1.clone());
        let mut has_clamp = false;
        if allow_clamping {
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
                            log::warn!(
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
        }
        log::trace!(
            "Looking for a match for font1 master '{}' at location {:?}",
            master.id,
            loc1
        );
        if let Some(exact) = font2_masters_and_locations
            .iter()
            .find(|(_, loc2)| compatible_location(&loc1, loc2))
        {
            results.push(Strategy::Exact(exact.0.clone()));
            continue;
        }
        // No exact match. If this location is strictly within the designspace bounds of font2, we can interpolate
        log::trace!(
            "No exact match found for font1 master '{}' at location {:?}, checking for interpolation",
            master.id,
            loc1
        );
        if loc1.iter().all(|(tag, value)| {
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
        }) {
            // Fill out any axes that font2 has but font1 doesn't with the default value
            let mut full_loc1 = loc1.clone();
            for axis in f2_axes_not_in_f1.iter() {
                if let Some(default) = axis.default {
                    full_loc1.insert(axis.tag, default);
                }
            }
            results.push(Strategy::InterpolateOrIntermediate(full_loc1));
        } else {
            results.push(Strategy::Failed(format!(
                "No compatible master found in font2 for font1 master at location {:?}",
                loc1
            )));
        }
    }
    Ok(results)
}
