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
    for (&tag, &value) in f1_location.iter() {
        match f2_location.get(tag) {
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
) -> Result<Vec<Strategy>, FontmergeError> {
    let ds1 = fontdrasil_axes(&font1.axes)?;
    let ds2 = fontdrasil_axes(&font2.axes)?;
    let f1_masters = &font1.masters;
    let mut results = vec![];

    let font1_axis_tags = font1.axes.iter().map(|a| a.tag).collect::<Vec<_>>();

    let remove_not_in_f1 = |loc: Location<UserSpace>| -> Location<UserSpace> {
        let mut new_loc = loc.clone();
        new_loc.retain(|tag, _| font1_axis_tags.contains(tag));
        new_loc
    };

    let font2_masters_and_locations = font2
        .masters
        .iter()
        .map(|m| (m.id.clone(), remove_not_in_f1(m.location.to_user(&ds2))))
        .collect::<Vec<_>>();

    // For each master in font1, we need to find something in font2 at that location
    for master in f1_masters.iter() {
        // Master location is in designspace, we need it in userspace
        let loc1 = &master.location.to_user(&ds1);
        if let Some(exact) = font2_masters_and_locations
            .iter()
            .find(|(_, loc2)| compatible_location(loc1, loc2))
        {
            results.push(Strategy::Exact(exact.0.clone()));
            continue;
        }
        // No exact match. If this location is strictly within the designspace bounds of font2, we can interpolate
        if loc1.iter().all(|(tag, value)| {
            if let Some(axis) = ds2.get(tag) {
                *value >= axis.min && *value <= axis.max
            } else {
                false
            }
        }) {
            results.push(Strategy::InterpolateOrIntermediate(loc1.clone()));
        } else {
            results.push(Strategy::Failed(format!(
                "No compatible master found in font2 for font1 master at location {:?}",
                loc1
            )));
        }
    }
    Ok(results)
}
