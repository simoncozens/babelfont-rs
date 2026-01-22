use std::collections::HashMap;

use crate::{common::tag_from_string, font::Font, master::Master, Glyph};

/// Check if a location only references glyph-level axes
pub(crate) fn is_glyph_level_only(location: &HashMap<String, f64>, glyph: &Glyph) -> bool {
    let component_axis_names: std::collections::HashSet<String> = glyph
        .component_axes
        .iter()
        .filter_map(|a| a.name.get_default().cloned())
        .collect();
    location.keys().all(|k| component_axis_names.contains(k))
}

/// Find a master in the font at the given location
pub(crate) fn find_master_at_location<'a>(
    font: &'a Font,
    location: &HashMap<String, f64>,
) -> Option<&'a Master> {
    font.masters.iter().find(|master| {
        location.iter().all(|(axis_tag_str, desired_value)| {
            if let Ok(tag) = tag_from_string(axis_tag_str) {
                master
                    .location
                    .get(tag)
                    .map(|coord| (coord.to_f64() - desired_value).abs() < 0.001)
                    .unwrap_or(false)
            } else {
                false
            }
        })
    })
}
