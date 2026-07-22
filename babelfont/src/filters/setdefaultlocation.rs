use crate::filters::FontFilter;
use fontdrasil::coords::{DesignLocation, UserCoord};

/// A filter that sets the default location to the specified location
pub struct SetDefaultLocation(DesignLocation);

impl SetDefaultLocation {
    /// Create a new SetDefaultLocation filter
    pub fn new(new_location: DesignLocation) -> Self {
        SetDefaultLocation(new_location)
    }
}

pub(crate) fn adjust_axes(
    axes: &mut Vec<crate::Axis>,
    new_location: &DesignLocation,
) -> Result<(), crate::BabelfontError> {
    // Adjust axes
    for (axis, coord) in new_location.iter() {
        if !axes.iter().any(|a| a.tag == *axis) {
            axes.push(crate::Axis::new(axis.to_string(), *axis));
        }
        #[allow(clippy::unwrap_used)] // We just ensured this above
        let font_axis = axes.iter_mut().find(|a| a.tag == *axis).unwrap();
        let userspace_loc = if font_axis.map.is_some() {
            font_axis.designspace_to_userspace(*coord)?
        } else {
            // We're still setting up this axis, min/max/default may be undefined, so don't try to convert the location to userspace
            UserCoord::new(coord.to_f64())
        };
        if font_axis.min.is_none_or(|min| min > userspace_loc) {
            font_axis.min = Some(userspace_loc);
        }
        if font_axis.max.is_none_or(|max| max < userspace_loc) {
            font_axis.max = Some(userspace_loc);
        }
        font_axis.default = Some(userspace_loc);
        if let Some(map) = font_axis.map.as_mut() {
            if !map.iter().any(|(loc, _)| *loc == userspace_loc) {
                map.push((userspace_loc, *coord));
                map.sort_by(|a, b| a.0.to_f64().total_cmp(&b.0.to_f64()));
            }
        }
    }
    Ok(())
}

impl FontFilter for SetDefaultLocation {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        if font.masters.len() == 1 {
            font.masters[0].location = self.0.clone();
        } else if !font.masters.iter().any(|m| m.location == self.0) {
            // No master at the desired location — interpolate one so source instantiation works for a
            // named instance that does not coincide with a master (rather than failing outright). The
            // font stays multi-master; a following DropVariations collapses to this new default.
            // extrapolate=false: a target outside the masters' range clamps to the default master
            // instead of extrapolating, which on an unmapped axis with min==default would distort the
            // geometry by a huge factor (e.g. an instance well below the lightest master).
            font.add_interpolated_master(&self.0, false)?;
        }
        adjust_axes(&mut font.axes, &self.0)?;
        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        let location = crate::filters::parse_location(s)?;
        Ok(SetDefaultLocation::new(location))
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("setdefaultlocation")
            .long("set-default-location")
            .help("Set the default location (design space location)")
            .value_name("LOCATION")
    }
}
